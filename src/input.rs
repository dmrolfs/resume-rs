//! ABOUTME: Reads a resume's text from a regular file, bounded, so a path that is not a file cannot hang a build.
//! ABOUTME: A position spec can name any path, and opening a FIFO waits for a writer that may never come.

use std::io::Read as _;
use std::path::Path;

use snafu::ResultExt;

use crate::error::{Error, InputNotAFileSnafu, InputUtf8Snafu, ReadInputSnafu};
use crate::markdown::MAX_INPUT_BYTES;

/// Reads the resume at `open`, and calls it `path` in every error, so the user sees the name they know it by.
/// It is a regular file, at most [`MAX_INPUT_BYTES`] bytes of UTF-8.
///
/// A longer file is not an error here. Its first `MAX_INPUT_BYTES + 1` bytes come back (cut at a character
/// boundary by lossy decoding), which is more than the limit, so the parser reports the size limit with the
/// finding it already has for it and never reads further.
///
/// # Errors
/// - `Error::ReadInput` - the file cannot be opened or read.
/// - `Error::InputNotAFile` - the path is a directory, a FIFO, a device, or anything but a regular file.
/// - `Error::InputUtf8` - the file is within the limit and is not UTF-8.
pub fn read_text(open: &Path, path: &Path) -> Result<String, Error> {
    // Asked before the file is opened, because opening a FIFO waits for a writer. `metadata` follows links, as
    // `open` does, so a link to a regular file is a regular file.
    let meta = std::fs::metadata(open).context(ReadInputSnafu { path })?;
    if !meta.is_file() {
        return InputNotAFileSnafu { path }.fail();
    }
    let file = std::fs::File::open(open).context(ReadInputSnafu { path })?;
    let mut bytes = Vec::new();
    // The file may have grown since `metadata`, so the read itself is capped.
    file.take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .context(ReadInputSnafu { path })?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    String::from_utf8(bytes).context(InputUtf8Snafu { path })
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;

    fn file_with(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("r.md");
        assert_ok!(std::fs::write(&path, bytes));
        (dir, path)
    }

    #[test]
    fn a_regular_file_is_read_whole() {
        let (_dir, path) = file_with("# Ada\n".as_bytes());
        assert_eq!(assert_ok!(read_text(&path, &path)), "# Ada\n");
    }

    #[test]
    fn a_missing_file_names_the_path() {
        let dir = assert_ok!(tempfile::tempdir());
        let error = assert_err!(read_text(&dir.path().join("nope.md"), Path::new("nope.md")));
        assert!(matches!(error, Error::ReadInput { .. }), "{error}");
        let text = error.to_string();
        assert!(text.contains("nope.md"), "{text}");
        assert!(
            !text.contains(&dir.path().display().to_string()),
            "the shown name, not the opened one: {text}"
        );
    }

    #[test]
    fn a_directory_is_refused_unread() {
        let dir = assert_ok!(tempfile::tempdir());
        let error = assert_err!(read_text(dir.path(), dir.path()));
        assert!(matches!(error, Error::InputNotAFile { .. }), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_waiting_for_a_writer() {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("pipe.md");
        let made = std::process::Command::new("mkfifo").arg(&path).status();
        assert!(assert_ok!(made).success(), "mkfifo failed");
        // Opening a FIFO waits for a writer, so a regression would block here forever. The read runs on its own
        // thread and the test waits a bounded time, so a regression fails the test and does not hang the suite.
        let (sender, receiver) = std::sync::mpsc::channel();
        let shown = path.clone();
        std::thread::spawn(move || {
            let _ = sender.send(read_text(&path, &shown).map(|_| ()));
        });
        let result = assert_ok!(
            receiver.recv_timeout(std::time::Duration::from_secs(10)),
            "reading a FIFO waited for a writer"
        );
        let error = assert_err!(result);
        assert!(matches!(error, Error::InputNotAFile { .. }), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_file_is_read_and_a_link_to_a_directory_is_refused() {
        let (dir, path) = file_with("# Ada\n".as_bytes());
        let link = dir.path().join("link.md");
        assert_ok!(std::os::unix::fs::symlink(&path, &link));
        assert_eq!(assert_ok!(read_text(&link, &link)), "# Ada\n");
        let dir_link = dir.path().join("dir-link");
        assert_ok!(std::os::unix::fs::symlink(dir.path(), &dir_link));
        let error = assert_err!(read_text(&dir_link, &dir_link));
        assert!(matches!(error, Error::InputNotAFile { .. }), "{error}");
    }

    #[test]
    fn a_file_over_the_limit_comes_back_longer_than_the_limit_and_no_further() {
        let (_dir, path) = file_with(&vec![b'a'; MAX_INPUT_BYTES + 10]);
        let text = assert_ok!(read_text(&path, &path));
        assert_eq!(text.len(), MAX_INPUT_BYTES + 1, "capped one byte past the limit");
    }

    #[test]
    fn a_file_over_the_limit_that_is_cut_inside_a_character_is_still_over_the_limit() {
        // Two-byte characters, so the cap at MAX + 1 lands in the middle of one when MAX is even.
        let mut bytes = "é".repeat(MAX_INPUT_BYTES / 2 + 5).into_bytes();
        bytes.truncate(MAX_INPUT_BYTES + 5);
        let (_dir, path) = file_with(&bytes);
        let text = assert_ok!(read_text(&path, &path));
        assert!(text.len() > MAX_INPUT_BYTES, "{}", text.len());
    }

    #[test]
    fn a_file_exactly_at_the_limit_is_read_whole() {
        let (_dir, path) = file_with(&vec![b'a'; MAX_INPUT_BYTES]);
        assert_eq!(assert_ok!(read_text(&path, &path)).len(), MAX_INPUT_BYTES);
    }

    #[test]
    fn invalid_utf8_exactly_at_the_limit_is_still_an_error_and_not_a_size_finding() {
        let mut bytes = vec![b'a'; MAX_INPUT_BYTES];
        bytes[0] = 0xff;
        let (_dir, path) = file_with(&bytes);
        let error = assert_err!(read_text(&path, &path));
        assert!(matches!(error, Error::InputUtf8 { .. }), "{error}");
    }

    #[test]
    fn invalid_utf8_within_the_limit_is_an_error_naming_the_path() {
        let (_dir, path) = file_with(&[0xff, 0xfe]);
        let error = assert_err!(read_text(&path, &path));
        assert!(matches!(error, Error::InputUtf8 { .. }), "{error}");
    }
}

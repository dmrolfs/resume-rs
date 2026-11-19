//! ABOUTME: The one way the tool writes a file: atomically, and with the same checks in a dry run.
//! ABOUTME: A temp file in the target's directory is persisted over the target, so a viewer never reads half a PDF.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use snafu::{IntoError as _, ResultExt};
use tempfile::{Builder, NamedTempFile};

use crate::error::{
    CreateOutputDirSnafu, Error, OutputExistsSnafu, OutputIsDirectorySnafu, OutputLockedSnafu, OutputReadOnlySnafu,
    WriteOutputSnafu,
};

/// Whether a write happens or is only checked and described.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Write the file.
    Write,
    /// Check everything a write would check, report what it would do, and touch nothing.
    DryRun,
}

/// What to do when the target already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    /// Replace it.
    Replace,
    /// Fail with `Error::OutputExists`.
    Refuse,
}

/// One file to write.
#[derive(Debug, Clone, Copy)]
pub struct Planned<'a> {
    /// Where the file goes.
    pub target: &'a Path,
    /// What goes in it.
    pub bytes: &'a [u8],
    /// What to do if the target exists.
    pub overwrite: Overwrite,
}

/// What a commit did, or in a dry run what it would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written {
    /// The file was written.
    Wrote {
        /// An earlier file at the target was replaced.
        replaced: bool,
    },
    /// A dry run: nothing was written.
    WouldWrite {
        /// A file at the target would be replaced.
        replaces: bool,
        /// The directory that would be created, if it does not exist.
        creates_dir: Option<PathBuf>,
    },
}

/// The only code that writes a file for the CLI.
#[derive(Debug, Clone, Copy)]
pub struct Writer {
    mode: WriteMode,
}

impl Writer {
    /// A writer that writes, or that only reports.
    #[must_use]
    pub fn new(mode: WriteMode) -> Self {
        Self { mode }
    }

    /// Writes `planned` (or in a dry run, reports what it would do).
    ///
    /// # Errors
    /// - `Error::OutputIsDirectory`, `Error::OutputReadOnly`, `Error::OutputExists` - the target cannot be
    ///   replaced; a dry run reports these too
    /// - `Error::CreateOutputDir` - the directory could not be created
    /// - `Error::OutputLocked` - another program holds the target open (Windows)
    /// - `Error::WriteOutput` - any other failure writing the file
    pub fn commit(&self, planned: Planned<'_>) -> Result<Written, Error> {
        let replaces = preflight(planned.target, planned.overwrite)?;
        match self.mode {
            WriteMode::DryRun => Ok(Written::WouldWrite {
                replaces,
                creates_dir: Some(directory_of(planned.target))
                    .filter(|dir| !dir.exists())
                    .map(Path::to_path_buf),
            }),
            WriteMode::Write => {
                write_atomic(planned.target, planned.bytes, planned.overwrite)?;
                Ok(Written::Wrote { replaced: replaces })
            }
        }
    }
}

/// The directory a target is written into: its parent, or the current directory for a bare file name.
fn directory_of(target: &Path) -> &Path {
    target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Checks, in a write and in a dry run alike, that the target can be replaced, and returns whether it exists.
///
/// A read-only file is refused on every OS: Unix `rename` would silently replace it while Windows refuses
/// with an access error, so refusing here gives the same behavior everywhere. A symlink is looked at, not
/// followed: the rename replaces the link, not the file it points to.
fn preflight(target: &Path, overwrite: Overwrite) -> Result<bool, Error> {
    match fs::symlink_metadata(target) {
        Ok(meta) if meta.is_dir() => OutputIsDirectorySnafu { path: target }.fail(),
        Ok(_) if overwrite == Overwrite::Refuse => OutputExistsSnafu { path: target }.fail(),
        Ok(meta) if meta.permissions().readonly() => OutputReadOnlySnafu { path: target }.fail(),
        Ok(_) => Ok(true),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(source).context(WriteOutputSnafu { path: target }),
    }
}

/// Writes `bytes` to a temp file beside `target` and renames it over `target`, so a reader sees the old file
/// or the new one and never a half-written one. Nothing of the old file is kept (permissions, attributes):
/// the output is a generated artifact.
fn write_atomic(target: &Path, bytes: &[u8], overwrite: Overwrite) -> Result<(), Error> {
    let dir = directory_of(target);
    fs::create_dir_all(dir).context(CreateOutputDirSnafu { path: dir })?;
    let mut temp = create_temp(dir).context(WriteOutputSnafu { path: target })?;
    temp.write_all(bytes).context(WriteOutputSnafu { path: target })?;
    temp.as_file().sync_all().context(WriteOutputSnafu { path: target })?;
    let persisted = match overwrite {
        Overwrite::Replace => temp.persist(target),
        Overwrite::Refuse => temp.persist_noclobber(target),
    };
    // A failed persist returns the temp file inside its error, and dropping that deletes it.
    persisted.map(drop).map_err(|failed| classify(failed.error, target))
}

fn create_temp(dir: &Path) -> io::Result<NamedTempFile> {
    let mut builder = Builder::new();
    builder.prefix(".resume-").suffix(".tmp");
    // A temp file is created private (0600), but a PDF should get the permissions a plain write would.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    builder.tempfile_in(dir)
}

/// Turns the failure of the final rename into the error that tells the user what to do.
fn classify(source: io::Error, target: &Path) -> Error {
    // Windows reports an access error for a read-only file and for a directory too, so look again before
    // blaming a program that holds the file open.
    match fs::symlink_metadata(target) {
        Ok(meta) if meta.is_dir() => return OutputIsDirectorySnafu { path: target }.build(),
        Ok(meta) if meta.permissions().readonly() => return OutputReadOnlySnafu { path: target }.build(),
        _ => {}
    }
    if source.kind() == io::ErrorKind::AlreadyExists {
        OutputExistsSnafu { path: target }.build()
    } else if is_locked(&source) {
        OutputLockedSnafu { path: target }.into_error(source)
    } else {
        WriteOutputSnafu { path: target }.into_error(source)
    }
}

/// True when a Windows error says another program holds the file: ERROR_ACCESS_DENIED (5),
/// ERROR_SHARING_VIOLATION (32), or ERROR_LOCK_VIOLATION (33). Which one a given viewer causes is only known
/// by running on Windows, so all three are mapped. Read-only files and directories also give 5, and
/// `classify` has separated those out before this is asked.
#[cfg(windows)]
fn is_locked(error: &io::Error) -> bool {
    const LOCKED: [i32; 3] = [5, 32, 33];
    error.raw_os_error().is_some_and(|code| LOCKED.contains(&code))
}

/// Only Windows refuses to replace a file another program has open.
#[cfg(not(windows))]
fn is_locked(_: &io::Error) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;
    use std::fs;

    fn dir() -> tempfile::TempDir {
        assert_ok!(tempfile::tempdir())
    }

    fn write(path: &Path, bytes: &[u8]) -> Result<Written, Error> {
        Writer::new(WriteMode::Write).commit(Planned {
            target: path,
            bytes,
            overwrite: Overwrite::Replace,
        })
    }

    fn with(mode: WriteMode, overwrite: Overwrite, path: &Path, bytes: &[u8]) -> Result<Written, Error> {
        Writer::new(mode).commit(Planned {
            target: path,
            bytes,
            overwrite,
        })
    }

    fn read(path: &Path) -> Vec<u8> {
        assert_ok!(fs::read(path))
    }

    /// The names of the temp files `commit` leaves in `dir`.
    fn leftovers(dir: &Path) -> Vec<String> {
        assert_ok!(fs::read_dir(dir))
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".resume-"))
            .collect()
    }

    /// Makes a file writable again when dropped, so a test that made it read-only can clean up (Windows
    /// refuses to delete a read-only file).
    struct Writable<'a>(&'a Path);

    impl Drop for Writable<'_> {
        fn drop(&mut self) {
            if let Ok(meta) = fs::metadata(self.0) {
                let mut permissions = meta.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                let _ = fs::set_permissions(self.0, permissions);
            }
        }
    }

    // ========================================================================
    // Writing
    // ========================================================================

    #[test]
    fn commit_writes_a_new_file() {
        let dir = dir();
        let target = dir.path().join("out.pdf");
        assert_eq!(assert_ok!(write(&target, b"one")), Written::Wrote { replaced: false });
        assert_eq!(read(&target), b"one");
        assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn commit_replaces_an_existing_file() {
        let dir = dir();
        let target = dir.path().join("out.pdf");
        assert_ok!(fs::write(&target, b"old"));
        assert_eq!(assert_ok!(write(&target, b"new")), Written::Wrote { replaced: true });
        assert_eq!(read(&target), b"new");
        assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn commit_creates_the_output_directory() {
        let dir = dir();
        let target = dir.path().join("a").join("b").join("out.pdf");
        assert_ok!(write(&target, b"deep"));
        assert_eq!(read(&target), b"deep");
    }

    #[test]
    fn a_target_without_a_directory_part_is_written_beside_the_current_directory() {
        assert_eq!(directory_of(Path::new("out.pdf")), Path::new("."));
        assert_eq!(directory_of(Path::new("a/out.pdf")), Path::new("a"));
    }

    #[cfg(unix)]
    #[test]
    fn written_files_get_the_permissions_a_plain_write_would_give() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir();
        let plain = dir.path().join("plain");
        let committed = dir.path().join("committed.pdf");
        assert_ok!(fs::write(&plain, b"x"));
        assert_ok!(write(&committed, b"x"));
        let mode = |p: &Path| assert_ok!(fs::metadata(p)).permissions().mode() & 0o777;
        assert_eq!(
            mode(&committed),
            mode(&plain),
            "the temp file's private mode leaked into the PDF"
        );
    }

    // ========================================================================
    // What cannot be replaced
    // ========================================================================

    #[test]
    fn a_directory_target_is_refused() {
        let dir = dir();
        let err = assert_err!(write(dir.path(), b"x"));
        assert!(matches!(err, Error::OutputIsDirectory { .. }), "{err}");
    }

    #[test]
    fn a_read_only_target_is_refused_on_every_os() {
        let dir = dir();
        let target = dir.path().join("out.pdf");
        assert_ok!(fs::write(&target, b"keep"));
        let mut permissions = assert_ok!(fs::metadata(&target)).permissions();
        permissions.set_readonly(true);
        assert_ok!(fs::set_permissions(&target, permissions));
        let _writable = Writable(&target);
        let err = assert_err!(write(&target, b"new"));
        assert!(matches!(err, Error::OutputReadOnly { .. }), "{err}");
        assert_eq!(read(&target), b"keep");
        assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn refuse_never_overwrites_but_writes_a_new_file() {
        let dir = dir();
        let target = dir.path().join("style.typ");
        assert_eq!(
            assert_ok!(with(WriteMode::Write, Overwrite::Refuse, &target, b"first")),
            Written::Wrote { replaced: false }
        );
        let err = assert_err!(with(WriteMode::Write, Overwrite::Refuse, &target, b"second"));
        assert!(matches!(err, Error::OutputExists { .. }), "{err}");
        assert_eq!(read(&target), b"first");
        assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    }

    // ========================================================================
    // Dry run
    // ========================================================================

    #[test]
    fn a_dry_run_touches_nothing_and_says_what_it_would_do() {
        let dir = dir();
        let fresh = dir.path().join("new").join("out.pdf");
        assert_eq!(
            assert_ok!(with(WriteMode::DryRun, Overwrite::Replace, &fresh, b"x")),
            Written::WouldWrite {
                replaces: false,
                creates_dir: Some(dir.path().join("new")),
            }
        );
        assert!(!dir.path().join("new").exists(), "a dry run created a directory");

        let existing = dir.path().join("old.pdf");
        assert_ok!(fs::write(&existing, b"old"));
        assert_eq!(
            assert_ok!(with(WriteMode::DryRun, Overwrite::Replace, &existing, b"new")),
            Written::WouldWrite {
                replaces: true,
                creates_dir: None,
            }
        );
        assert_eq!(read(&existing), b"old");
        assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn a_dry_run_reports_the_refusals_a_write_would_hit() {
        let dir = dir();
        let existing = dir.path().join("style.typ");
        assert_ok!(fs::write(&existing, b"old"));
        let exists = assert_err!(with(WriteMode::DryRun, Overwrite::Refuse, &existing, b"new"));
        assert!(matches!(exists, Error::OutputExists { .. }), "{exists}");
        let directory = assert_err!(with(WriteMode::DryRun, Overwrite::Replace, dir.path(), b"new"));
        assert!(matches!(directory, Error::OutputIsDirectory { .. }), "{directory}");
    }

    // ========================================================================
    // Failures of the final step
    // ========================================================================

    #[test]
    fn a_failed_persist_leaves_no_temp_file() {
        // `commit` would have refused a non-empty directory as a target; going around it makes the final
        // rename fail, which is the step whose temp file must not be left behind.
        let dir = dir();
        let target = dir.path().join("occupied");
        assert_ok!(fs::create_dir(&target));
        assert_ok!(fs::write(target.join("inside"), b"x"));
        let err = assert_err!(write_atomic(&target, b"new", Overwrite::Replace));
        assert!(matches!(err, Error::OutputIsDirectory { .. }), "{err}");
        assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn classify_names_an_existing_target_and_reports_anything_else_as_a_write_failure() {
        let target = Path::new("out.pdf");
        let exists = classify(std::io::Error::from(std::io::ErrorKind::AlreadyExists), target);
        assert!(matches!(exists, Error::OutputExists { .. }), "{exists}");
        let other = classify(std::io::Error::other("disk full"), target);
        assert!(matches!(other, Error::WriteOutput { .. }), "{other}");
        assert!(other.to_string().contains("disk full"), "{other}");
    }

    #[test]
    fn classify_looks_at_the_target_before_blaming_a_viewer_or_a_full_disk() {
        let dir = dir();
        let denied = || std::io::Error::other("denied");

        let directory = classify(denied(), dir.path());
        assert!(matches!(directory, Error::OutputIsDirectory { .. }), "{directory}");

        let read_only = dir.path().join("read-only.pdf");
        assert_ok!(fs::write(&read_only, b"x"));
        let mut permissions = assert_ok!(fs::metadata(&read_only)).permissions();
        permissions.set_readonly(true);
        assert_ok!(fs::set_permissions(&read_only, permissions));
        let _writable = Writable(&read_only);
        let refused = classify(denied(), &read_only);
        assert!(matches!(refused, Error::OutputReadOnly { .. }), "{refused}");

        let plain = dir.path().join("plain.pdf");
        assert_ok!(fs::write(&plain, b"x"));
        let failed = classify(denied(), &plain);
        assert!(matches!(failed, Error::WriteOutput { .. }), "{failed}");
        let exists = classify(std::io::Error::from(std::io::ErrorKind::AlreadyExists), &plain);
        assert!(matches!(exists, Error::OutputExists { .. }), "{exists}");
    }

    #[cfg(unix)]
    #[test]
    fn a_target_under_a_file_is_a_write_error_not_a_missing_file() {
        // Looking at `file/out.pdf` fails with "not a directory", which is not "not found": the target
        // cannot be treated as new.
        let dir = dir();
        let file = dir.path().join("file");
        assert_ok!(fs::write(&file, b"x"));
        let err = assert_err!(write(&file.join("out.pdf"), b"new"));
        assert!(matches!(err, Error::WriteOutput { .. }), "{err}");
    }

    // ========================================================================
    // Windows: a viewer holding the file open
    // ========================================================================

    /// Holds `path` open the way a program that does not share deletes would, and returns the handle.
    #[cfg(windows)]
    fn held_open(path: &Path, share_mode: u32) -> fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        assert_ok!(fs::OpenOptions::new().read(true).share_mode(share_mode).open(path))
    }

    #[cfg(windows)]
    fn assert_locked(share_mode: u32) {
        let dir = dir();
        let target = dir.path().join("out.pdf");
        assert_ok!(fs::write(&target, b"old"));
        let _viewer = held_open(&target, share_mode);
        let err = assert_err!(write(&target, b"new"));
        if let Error::OutputLocked { source, .. } = &err {
            // The CI log records which Windows error a held file really causes (5, 32, or 33).
            eprintln!(
                "share_mode {share_mode}: locked with OS error {:?}",
                source.raw_os_error()
            );
        }
        assert!(matches!(err, Error::OutputLocked { .. }), "{err}");
        assert!(
            assert_some_hint(&err).contains("close the PDF viewer"),
            "the hint says what to do: {err}"
        );
        assert_eq!(read(&target), b"old");
        assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    }

    #[cfg(windows)]
    fn assert_some_hint(err: &Error) -> &'static str {
        claims::assert_some!(err.hint(), "{err} has no hint")
    }

    #[cfg(windows)]
    #[test]
    fn output_locked_reports_hint() {
        assert_locked(0);
    }

    #[cfg(windows)]
    #[test]
    fn output_locked_by_a_reader_that_shares_reads() {
        // FILE_SHARE_READ, the way a typical viewer opens a file.
        assert_locked(1);
    }
}

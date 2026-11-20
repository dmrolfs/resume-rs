//! ABOUTME: The one way the tool writes a file: atomically, and with the same checks in a dry run.
//! ABOUTME: A temp file in the target's directory is persisted over the target, so a viewer never reads half a PDF.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use snafu::{IntoError as _, ResultExt};
use tempfile::{Builder, NamedTempFile};

use crate::error::{
    CreateOutputDirSnafu, Error, InputChangedSnafu, OutputExistsSnafu, OutputIsDirectorySnafu, OutputLockedSnafu,
    OutputReadOnlySnafu, WriteOutputSnafu,
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
pub enum Overwrite<'a> {
    /// Replace it. For a file the tool generated: the old file's permissions are not kept, and a symlink at
    /// the target is replaced, not followed.
    Replace,
    /// Fail with `Error::OutputExists`.
    Refuse,
    /// Edit a file the user owns, which `expected` is the text of as it was read.
    ///
    /// The file is replaced by a rename, so it is a new inode: a hard link elsewhere keeps the old text, and
    /// owner, ACLs, extended attributes, and creation time are not carried over (the permission bits are).
    ///
    /// The file must exist, a symlink is followed (so the link stays a link), the file's permissions are kept
    /// (a resume holds a phone number and an address, so a 0600 file must not become readable by everyone), and
    /// the write fails with `Error::InputChanged` when the file no longer holds `expected`, so an edit saved in
    /// an editor in the meantime is not lost.
    ReplaceSource { expected: &'a [u8] },
}

/// One file to write.
#[derive(Debug, Clone, Copy)]
pub struct Planned<'a> {
    /// Where the file goes.
    pub target: &'a Path,
    /// What goes in it.
    pub bytes: &'a [u8],
    /// What to do if the target exists.
    pub overwrite: Overwrite<'a>,
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

    /// Whether this writer writes or only reports.
    #[must_use]
    pub fn mode(&self) -> WriteMode {
        self.mode
    }

    /// Writes `planned` (or in a dry run, reports what it would do).
    ///
    /// # Errors
    /// - `Error::OutputIsDirectory`, `Error::OutputReadOnly`, `Error::OutputExists` - the target cannot be
    ///   replaced; a dry run reports these too
    /// - `Error::CreateOutputDir` - the directory could not be created; a dry run also reports a parent that
    ///   is not a directory (a file, or a dangling link), which a write could not create either
    /// - `Error::OutputLocked` - another program holds the target open (Windows)
    /// - `Error::InputChanged` - [`Overwrite::ReplaceSource`] only: the file no longer holds what was read
    /// - `Error::WriteOutput` - any other failure writing the file
    pub fn commit(&self, planned: Planned<'_>) -> Result<Written, Error> {
        // A source edit is made to the file a link points to, so everything below looks at that file.
        let resolved = match planned.overwrite {
            Overwrite::ReplaceSource { .. } => {
                Some(fs::canonicalize(planned.target).context(WriteOutputSnafu { path: planned.target })?)
            }
            Overwrite::Replace | Overwrite::Refuse => None,
        };
        let target = resolved.as_deref().unwrap_or(planned.target);
        let replaces = preflight(target, planned.overwrite)?;
        let creates_dir = missing_directory(directory_of(target))?;
        match self.mode {
            WriteMode::DryRun => Ok(Written::WouldWrite { replaces, creates_dir }),
            WriteMode::Write => {
                write_atomic(target, planned.bytes, planned.overwrite)?;
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

/// The topmost directory a write would have to create, or `None` when `dir` exists.
///
/// Walks up to the nearest existing ancestor and refuses, in a write and in a dry run alike, when that is not
/// a directory: `create_dir_all` would fail on it, so a dry run must not say "new file". Permissions and free
/// space are not checked; only attempting the write can tell.
fn missing_directory(dir: &Path) -> Result<Option<PathBuf>, Error> {
    let mut missing = None;
    for ancestor in dir.ancestors().filter(|ancestor| !ancestor.as_os_str().is_empty()) {
        match fs::metadata(ancestor) {
            Ok(meta) if meta.is_dir() => return Ok(missing.map(Path::to_path_buf)),
            Ok(_) => {
                let source = io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "a parent of the output is not a directory",
                );
                return Err(source).context(CreateOutputDirSnafu { path: ancestor });
            }
            // `metadata` follows links, so a dangling symlink looks missing, but `create_dir_all` cannot create
            // a directory where the link is: refuse it like any other non-directory.
            Err(source) if source.kind() == io::ErrorKind::NotFound && fs::symlink_metadata(ancestor).is_err() => {
                missing = Some(ancestor);
            }
            Err(source) => return Err(source).context(CreateOutputDirSnafu { path: ancestor }),
        }
    }
    Ok(missing.map(Path::to_path_buf))
}

/// Checks, in a write and in a dry run alike, that the target can be replaced, and returns whether it exists.
///
/// A read-only file is refused on every OS: Unix `rename` would silently replace it while Windows refuses
/// with an access error, so refusing here gives the same behavior everywhere. A symlink is looked at, not
/// followed: the rename replaces the link, not the file it points to.
fn preflight(target: &Path, overwrite: Overwrite<'_>) -> Result<bool, Error> {
    match fs::symlink_metadata(target) {
        Ok(meta) if meta.is_dir() => OutputIsDirectorySnafu { path: target }.fail(),
        Ok(_) if overwrite == Overwrite::Refuse => OutputExistsSnafu { path: target }.fail(),
        Ok(meta) if meta.permissions().readonly() => OutputReadOnlySnafu { path: target }.fail(),
        Ok(_) => {
            check_unchanged(target, overwrite)?;
            Ok(true)
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(source).context(WriteOutputSnafu { path: target }),
    }
}

/// For a source edit, fails unless `target` still holds the text that was read.
fn check_unchanged(target: &Path, overwrite: Overwrite<'_>) -> Result<(), Error> {
    let Overwrite::ReplaceSource { expected } = overwrite else {
        return Ok(());
    };
    let now = fs::read(target).context(WriteOutputSnafu { path: target })?;
    if now == expected {
        Ok(())
    } else {
        InputChangedSnafu { path: target }.fail()
    }
}

/// Writes `bytes` to a temp file beside `target` and renames it over `target`, so a reader sees the old file
/// or the new one and never a half-written one. Nothing of the old file is kept (permissions, attributes):
/// the output is a generated artifact.
fn write_atomic(target: &Path, bytes: &[u8], overwrite: Overwrite<'_>) -> Result<(), Error> {
    let dir = directory_of(target);
    fs::create_dir_all(dir).context(CreateOutputDirSnafu { path: dir })?;
    let source = matches!(overwrite, Overwrite::ReplaceSource { .. });
    // A source edit starts private (0600) and takes the file's permissions before any byte is written, so the
    // text of a private resume is never in a file others can read, even if the process dies mid-write.
    let mut temp = create_temp(dir, source).context(WriteOutputSnafu { path: target })?;
    if source {
        let permissions = fs::metadata(target)
            .context(WriteOutputSnafu { path: target })?
            .permissions();
        fs::set_permissions(temp.path(), permissions).context(WriteOutputSnafu { path: target })?;
    }
    temp.write_all(bytes).context(WriteOutputSnafu { path: target })?;
    temp.as_file().sync_all().context(WriteOutputSnafu { path: target })?;
    if source {
        // Looked at once more just before the file is replaced, which narrows the window for a concurrent edit
        // to the rename itself.
        check_unchanged(target, overwrite)?;
    }
    let persisted = match overwrite {
        Overwrite::Replace | Overwrite::ReplaceSource { .. } => temp.persist(target),
        Overwrite::Refuse => temp.persist_noclobber(target),
    };
    // A failed persist returns the temp file inside its error, and dropping that deletes it.
    persisted.map(drop).map_err(|failed| classify(failed.error, target))
}

fn create_temp(dir: &Path, private: bool) -> io::Result<NamedTempFile> {
    let mut builder = Builder::new();
    builder.prefix(".resume-").suffix(".tmp");
    // A generated PDF should get the permissions a plain write would, and `tempfile` would make it private
    // (0600); a source edit keeps tempfile's private default until the file's own permissions are applied.
    #[cfg(unix)]
    if !private {
        use std::os::unix::fs::PermissionsExt as _;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    #[cfg(not(unix))]
    let _ = private;
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

    fn with(mode: WriteMode, overwrite: Overwrite<'_>, path: &Path, bytes: &[u8]) -> Result<Written, Error> {
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

    #[cfg(unix)]
    #[test]
    fn a_symlink_target_is_replaced_and_the_file_it_pointed_to_is_kept() {
        let dir = dir();
        let real = dir.path().join("real.pdf");
        let link = dir.path().join("out.pdf");
        assert_ok!(fs::write(&real, b"keep"));
        assert_ok!(std::os::unix::fs::symlink(&real, &link));
        assert_eq!(assert_ok!(write(&link, b"new")), Written::Wrote { replaced: true });
        assert!(!assert_ok!(fs::symlink_metadata(&link)).file_type().is_symlink());
        assert_eq!(read(&link), b"new");
        assert_eq!(read(&real), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_exists_for_refuse_and_is_replaced_by_replace() {
        let dir = dir();
        let link = dir.path().join("out.pdf");
        assert_ok!(std::os::unix::fs::symlink(dir.path().join("missing"), &link));
        let err = assert_err!(with(WriteMode::Write, Overwrite::Refuse, &link, b"x"));
        assert!(matches!(err, Error::OutputExists { .. }), "{err}");
        assert_eq!(assert_ok!(write(&link, b"new")), Written::Wrote { replaced: true });
        assert_eq!(read(&link), b"new");
    }

    #[test]
    fn a_dry_run_refuses_a_read_only_target_like_a_write() {
        let dir = dir();
        let target = dir.path().join("out.pdf");
        assert_ok!(fs::write(&target, b"old"));
        let mut permissions = assert_ok!(fs::metadata(&target)).permissions();
        permissions.set_readonly(true);
        assert_ok!(fs::set_permissions(&target, permissions));
        let _writable = Writable(&target);
        for mode in [WriteMode::DryRun, WriteMode::Write] {
            let err = assert_err!(with(mode, Overwrite::Replace, &target, b"new"));
            assert!(matches!(err, Error::OutputReadOnly { .. }), "{err}");
        }
    }

    #[test]
    fn missing_directory_refuses_a_file_in_the_chain_directly() {
        let dir = dir();
        let file = dir.path().join("file");
        assert_ok!(fs::write(&file, b"x"));
        for below in [file.clone(), file.join("a"), file.join("a").join("b")] {
            let err = assert_err!(missing_directory(&below));
            assert!(matches!(err, Error::CreateOutputDir { .. }), "{err}");
        }
        assert_eq!(assert_ok!(missing_directory(dir.path())), None);
        assert_eq!(
            assert_ok!(missing_directory(&dir.path().join("a").join("b"))),
            Some(dir.path().join("a"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_as_the_output_directory_is_refused_in_both_modes() {
        let dir = dir();
        let link = dir.path().join("out");
        assert_ok!(std::os::unix::fs::symlink(dir.path().join("missing"), &link));
        for mode in [WriteMode::DryRun, WriteMode::Write] {
            let err = assert_err!(with(mode, Overwrite::Replace, &link.join("x.pdf"), b"new"));
            assert!(matches!(err, Error::CreateOutputDir { .. }), "{err}");
        }
    }

    // ========================================================================
    // Editing a source file
    // ========================================================================

    fn edit(mode: WriteMode, path: &Path, expected: &[u8], bytes: &[u8]) -> Result<Written, Error> {
        with(mode, Overwrite::ReplaceSource { expected }, path, bytes)
    }

    #[test]
    fn a_source_edit_replaces_the_file_when_it_is_unchanged() {
        let dir = dir();
        let file = dir.path().join("resume.md");
        assert_ok!(fs::write(&file, b"old"));
        assert_eq!(
            assert_ok!(edit(WriteMode::Write, &file, b"old", b"new")),
            Written::Wrote { replaced: true }
        );
        assert_eq!(read(&file), b"new");
        assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn a_source_edit_refuses_a_file_that_changed_since_it_was_read_and_keeps_it() {
        let dir = dir();
        let file = dir.path().join("resume.md");
        assert_ok!(fs::write(&file, b"edited in an editor"));
        for mode in [WriteMode::DryRun, WriteMode::Write] {
            let err = assert_err!(edit(mode, &file, b"old", b"new"));
            assert!(matches!(err, Error::InputChanged { .. }), "{err}");
        }
        assert_eq!(read(&file), b"edited in an editor");
        assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn a_source_edit_needs_the_file_to_exist() {
        let dir = dir();
        let err = assert_err!(edit(WriteMode::Write, &dir.path().join("missing.md"), b"", b"x"));
        assert!(matches!(err, Error::WriteOutput { .. }), "{err}");
    }

    #[test]
    fn a_source_edit_dry_run_touches_nothing_and_says_it_replaces() {
        let dir = dir();
        let file = dir.path().join("resume.md");
        assert_ok!(fs::write(&file, b"old"));
        assert_eq!(
            assert_ok!(edit(WriteMode::DryRun, &file, b"old", b"new")),
            Written::WouldWrite {
                replaces: true,
                creates_dir: None
            }
        );
        assert_eq!(read(&file), b"old");
    }

    #[test]
    fn a_source_edit_refuses_a_read_only_file_in_both_modes() {
        let dir = dir();
        let file = dir.path().join("resume.md");
        assert_ok!(fs::write(&file, b"old"));
        let mut permissions = assert_ok!(fs::metadata(&file)).permissions();
        permissions.set_readonly(true);
        assert_ok!(fs::set_permissions(&file, permissions));
        let _writable = Writable(&file);
        for mode in [WriteMode::DryRun, WriteMode::Write] {
            let err = assert_err!(edit(mode, &file, b"old", b"new"));
            assert!(matches!(err, Error::OutputReadOnly { .. }), "{err}");
        }
        assert_eq!(read(&file), b"old");
    }

    #[cfg(unix)]
    #[test]
    fn a_source_edit_keeps_the_files_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir();
        let file = dir.path().join("resume.md");
        assert_ok!(fs::write(&file, b"old"));
        assert_ok!(fs::set_permissions(&file, fs::Permissions::from_mode(0o600)));
        assert_ok!(edit(WriteMode::Write, &file, b"old", b"new"));
        let mode = assert_ok!(fs::metadata(&file)).permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "a private resume must not become readable by others");
    }

    #[test]
    fn watch_replaces_pdf_atomically() {
        // `watch` rewrites the same PDF again and again while a viewer reads it. A reader must see the old file or
        // the new one, never a mix or a part, because the write is a temp file renamed into place.
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = dir();
        let target = dir.path().join("out.pdf");
        let whole = |round: usize| -> Vec<u8> {
            let mut bytes = b"%PDF-1.7\n".to_vec();
            bytes.extend(std::iter::repeat_n(
                b'a' + u8::try_from(round % 26).expect("small"),
                200_000,
            ));
            bytes.extend_from_slice(b"\n%%EOF\n");
            bytes
        };
        assert_ok!(write(&target, &whole(0)));
        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let (done, target) = (Arc::clone(&done), target.clone());
            std::thread::spawn(move || {
                let mut reads = 0;
                while !done.load(Ordering::SeqCst) {
                    // Windows refuses a rename onto a file that is open, so a reader that holds it only briefly
                    // may make a write fail; what it must never do is see a partial file.
                    if let Ok(bytes) = fs::read(&target) {
                        assert!(
                            bytes.starts_with(b"%PDF-1.7\n") && bytes.ends_with(b"\n%%EOF\n"),
                            "a partial file"
                        );
                        assert_eq!(bytes.len(), 200_000 + 9 + 7, "a partial file");
                        let letters: std::collections::HashSet<u8> =
                            bytes[9..bytes.len() - 7].iter().copied().collect();
                        assert_eq!(letters.len(), 1, "a mix of two files");
                        reads += 1;
                    }
                }
                reads
            })
        };
        let mut written = 0;
        for round in 1..=40 {
            // On Windows a write can fail while the reader has the file open; a build tells the user and goes on.
            written += usize::from(write(&target, &whole(round)).is_ok());
        }
        assert!(
            written > 0,
            "no write got through, so nothing was replaced and the reader proved nothing"
        );
        done.store(true, Ordering::SeqCst);
        assert!(reader.join().expect("the reader saw only whole files") > 0);
        assert_eq!(
            leftovers(dir.path()),
            Vec::<String>::new(),
            "no temp file is left behind"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_temp_of_a_source_edit_is_private_until_it_takes_the_files_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir();
        let mode = |temp: &NamedTempFile| assert_ok!(fs::metadata(temp.path())).permissions().mode() & 0o777;
        let source = assert_ok!(create_temp(dir.path(), true));
        assert_eq!(mode(&source), 0o600, "a source edit's temp must start private");
        let artifact = assert_ok!(create_temp(dir.path(), false));
        // A generated PDF gets what a plain write would: 0666 less the umask, which is never private-only.
        assert_ne!(mode(&artifact), 0o600);
        assert_eq!(leftovers(dir.path()).len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_source_edit_follows_a_symlink_and_keeps_the_link() {
        let dir = dir();
        let real = dir.path().join("real.md");
        let link = dir.path().join("resume.md");
        assert_ok!(fs::write(&real, b"old"));
        assert_ok!(std::os::unix::fs::symlink(&real, &link));
        assert_ok!(edit(WriteMode::Write, &link, b"old", b"new"));
        assert!(
            assert_ok!(fs::symlink_metadata(&link)).file_type().is_symlink(),
            "the link was replaced"
        );
        assert_eq!(read(&real), b"new");
        assert_eq!(read(&link), b"new");
    }

    #[test]
    fn dry_run_and_write_refuse_a_missing_directory_below_a_file_alike() {
        let dir = dir();
        let file = dir.path().join("file");
        assert_ok!(fs::write(&file, b"x"));
        let target = file.join("missing").join("out.pdf");
        for mode in [WriteMode::DryRun, WriteMode::Write] {
            let err = assert_err!(with(mode, Overwrite::Replace, &target, b"new"));
            assert!(
                matches!(err, Error::CreateOutputDir { .. } | Error::WriteOutput { .. }),
                "{err}"
            );
        }
        assert_eq!(read(&file), b"x");
    }

    #[test]
    fn a_dry_run_names_the_topmost_directory_it_would_create() {
        let dir = dir();
        let target = dir.path().join("a").join("b").join("out.pdf");
        assert_eq!(
            assert_ok!(with(WriteMode::DryRun, Overwrite::Replace, &target, b"x")),
            Written::WouldWrite {
                replaces: false,
                creates_dir: Some(dir.path().join("a")),
            }
        );
        assert!(!dir.path().join("a").exists());
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
        let viewer = held_open(&target, share_mode);
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
        // The viewer still holds the file with no sharing, so even reading it back is refused until it lets go.
        drop(viewer);
        assert_eq!(read(&target), b"old");
        assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    }

    #[cfg(windows)]
    #[test]
    fn a_source_edit_of_a_file_held_open_by_a_viewer_is_refused_and_nothing_changes() {
        let dir = dir();
        let file = dir.path().join("resume.md");
        assert_ok!(fs::write(&file, b"old"));
        let viewer = held_open(&file, 0);
        let err = assert_err!(edit(WriteMode::Write, &file, b"old", b"new"));
        assert!(
            matches!(err, Error::OutputLocked { .. } | Error::WriteOutput { .. }),
            "a held source must not be edited: {err}"
        );
        drop(viewer);
        assert_eq!(read(&file), b"old");
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

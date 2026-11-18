//! ABOUTME: Where style files come from: a built-in embedded directory or a directory on disk.
//! ABOUTME: Serves bytes by virtual path and denies anything that resolves outside the style root.

use std::path::{Path, PathBuf};

use include_dir::Dir;
use snafu::{OptionExt, ResultExt};
use typst::diag::{FileError, FileResult};
use typst::syntax::VirtualPath;

use crate::error::{Error, StyleMissingMainSnafu, StyleRootSnafu, UnknownStyleSnafu};
use crate::model::StyleName;

/// Display root for files served from the embedded set. Angle brackets are invalid in Windows
/// file names, so this can never collide with a real path and is never opened.
const BUILT_IN: &str = "<built-in>";

/// The file every style must provide, directly under its own directory.
const MAIN_FILE: &str = "style.typ";

/// A resolved style plus the files it may read.
///
/// The virtual root is the directory that contains the style directories, so a style can
/// import shared files such as `/lib/components.typ`.
#[derive(Debug, Clone)]
pub struct StyleSource {
    name: StyleName,
    files: StyleFiles,
}

#[derive(Debug, Clone)]
enum StyleFiles {
    Embedded(&'static Dir<'static>),
    Directory {
        /// As given by the user; used for display.
        root: PathBuf,
        /// Resolved once; used only for containment checks, never for display.
        canonical_root: PathBuf,
    },
}

impl StyleSource {
    /// Opens the style `name` inside the directory `root`.
    ///
    /// # Errors
    /// - `Error::StyleRoot` - `root` cannot be resolved
    /// - `Error::UnknownStyle` - `root` has no directory called `name`
    /// - `Error::StyleMissingMain` - the style directory has no `style.typ`
    pub fn directory(root: impl AsRef<Path>, name: StyleName) -> Result<Self, Error> {
        let root = root.as_ref();
        let canonical_root = std::fs::canonicalize(root).context(StyleRootSnafu { path: root })?;
        let style_dir = canonical_root.join(name.as_ref() as &str);
        if !style_dir.is_dir() {
            return UnknownStyleSnafu { name, searched: root }.fail();
        }
        if !style_dir.join(MAIN_FILE).is_file() {
            return StyleMissingMainSnafu {
                dir: root.join(name.as_ref() as &str),
            }
            .fail();
        }
        Ok(Self {
            name,
            files: StyleFiles::Directory {
                root: root.to_path_buf(),
                canonical_root,
            },
        })
    }

    /// Opens the style `name` inside an embedded directory.
    ///
    /// # Errors
    /// - `Error::UnknownStyle` - the set has no directory called `name`
    /// - `Error::StyleMissingMain` - the style directory has no `style.typ`
    pub fn embedded(dir: &'static Dir<'static>, name: StyleName) -> Result<Self, Error> {
        let style_dir = dir.get_dir(name.as_ref() as &str).context(UnknownStyleSnafu {
            name: name.clone(),
            searched: PathBuf::from(BUILT_IN),
        })?;
        let main = format!("{}/{MAIN_FILE}", name.as_ref() as &str);
        if dir.get_file(&main).is_none() {
            return StyleMissingMainSnafu {
                dir: PathBuf::from(BUILT_IN).join(style_dir.path()),
            }
            .fail();
        }
        Ok(Self {
            name,
            files: StyleFiles::Embedded(dir),
        })
    }

    #[must_use]
    pub fn name(&self) -> &StyleName {
        &self.name
    }

    /// The virtual path of the file Typst compiles: `/<name>/style.typ`.
    #[must_use]
    pub fn main_path(&self) -> VirtualPath {
        VirtualPath::new(format!("/{}/{MAIN_FILE}", self.name.as_ref() as &str))
            .expect("a validated style name makes a valid virtual path")
    }

    /// Reads one file by virtual path.
    ///
    /// # Errors
    /// `FileError::AccessDenied` when the path resolves outside the style root (for example a
    /// symlink pointing out of it), `NotFound`, `IsDirectory`, or the underlying I/O failure.
    pub(crate) fn read(&self, vpath: &VirtualPath) -> FileResult<Vec<u8>> {
        match &self.files {
            StyleFiles::Embedded(dir) => read_embedded(dir, vpath),
            StyleFiles::Directory { canonical_root, .. } => read_directory(canonical_root, vpath),
        }
    }

    /// The path to show users for a virtual path.
    #[must_use]
    pub(crate) fn display_path(&self, vpath: &VirtualPath) -> PathBuf {
        let relative = Path::new(vpath.get_without_slash());
        match &self.files {
            StyleFiles::Embedded(_) => PathBuf::from(BUILT_IN).join(relative),
            StyleFiles::Directory { root, .. } => root.join(relative),
        }
    }
}

fn read_embedded(dir: &Dir<'_>, vpath: &VirtualPath) -> FileResult<Vec<u8>> {
    let relative = vpath.get_without_slash();
    if let Some(file) = dir.get_file(relative) {
        return Ok(file.contents().to_vec());
    }
    if dir.get_dir(relative).is_some() || vpath.is_root() {
        return Err(FileError::IsDirectory);
    }
    Err(FileError::NotFound(PathBuf::from(relative)))
}

fn read_directory(canonical_root: &Path, vpath: &VirtualPath) -> FileResult<Vec<u8>> {
    let relative = Path::new(vpath.get_without_slash());
    let target = vpath.realize(canonical_root).map_err(FileError::Realize)?;
    let resolved = std::fs::canonicalize(&target).map_err(|e| FileError::from_io(e, relative))?;
    if !resolved.starts_with(canonical_root) {
        return Err(FileError::AccessDenied);
    }
    if resolved.is_dir() {
        return Err(FileError::IsDirectory);
    }
    std::fs::read(&resolved).map_err(|e| FileError::from_io(e, relative))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use include_dir::include_dir;
    use pretty_assertions::assert_eq;

    static FIXTURES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/tests/fixtures/styles");

    fn fixtures_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/styles")
    }

    fn name(s: &str) -> StyleName {
        assert_ok!(StyleName::try_new(s))
    }

    fn vpath(s: &str) -> VirtualPath {
        assert_ok!(VirtualPath::new(s))
    }

    #[test]
    fn directory_style_requires_style_typ() {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(tmp.path().join("empty")));
        let err = assert_err!(StyleSource::directory(tmp.path(), name("empty")));
        assert!(matches!(err, Error::StyleMissingMain { .. }), "{err:?}");
    }

    #[test]
    fn unknown_style_reports_searched_directory() {
        let err = assert_err!(StyleSource::directory(fixtures_path(), name("nope")));
        match err {
            Error::UnknownStyle { name, searched } => {
                assert_eq!(name.as_ref() as &str, "nope");
                assert_eq!(searched, fixtures_path());
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn missing_styles_root_is_a_style_root_error() {
        let err = assert_err!(StyleSource::directory(
            fixtures_path().join("does-not-exist"),
            name("x")
        ));
        assert!(matches!(err, Error::StyleRoot { .. }), "{err:?}");
    }

    #[test]
    fn main_file_is_style_typ_under_the_style_name() {
        let style = assert_ok!(StyleSource::directory(fixtures_path(), name("minimal")));
        assert_eq!(style.main_path().get_with_slash(), "/minimal/style.typ");
    }

    #[test]
    fn directory_style_reads_shared_files_through_the_root() {
        let style = assert_ok!(StyleSource::directory(fixtures_path(), name("lib-import")));
        let bytes = assert_ok!(style.read(&vpath("/lib/helper.typ")));
        assert!(String::from_utf8_lossy(&bytes).contains("greeting"));
    }

    #[test]
    fn embedded_style_reads_by_relative_path() {
        let style = assert_ok!(StyleSource::embedded(&FIXTURES, name("minimal")));
        let bytes = assert_ok!(style.read(&vpath("/minimal/style.typ")));
        assert!(String::from_utf8_lossy(&bytes).contains("resume.json"));
    }

    #[test]
    fn embedded_style_reports_unknown_and_missing_main() {
        let err = assert_err!(StyleSource::embedded(&FIXTURES, name("nope")));
        assert!(matches!(err, Error::UnknownStyle { .. }), "{err:?}");
        // `lib` is a directory in the fixtures but has no style.typ.
        let err = assert_err!(StyleSource::embedded(&FIXTURES, name("lib")));
        assert!(matches!(err, Error::StyleMissingMain { .. }), "{err:?}");
    }

    #[test]
    fn missing_file_is_not_found_in_both_sources() {
        let disk = assert_ok!(StyleSource::directory(fixtures_path(), name("minimal")));
        let built_in = assert_ok!(StyleSource::embedded(&FIXTURES, name("minimal")));
        for style in [disk, built_in] {
            let err = assert_err!(style.read(&vpath("/minimal/absent.typ")));
            assert!(matches!(err, FileError::NotFound(_)), "{err:?}");
        }
    }

    #[test]
    fn reading_a_directory_is_is_directory_in_both_sources() {
        let disk = assert_ok!(StyleSource::directory(fixtures_path(), name("minimal")));
        let built_in = assert_ok!(StyleSource::embedded(&FIXTURES, name("minimal")));
        for style in [disk, built_in] {
            let err = assert_err!(style.read(&vpath("/lib")));
            assert!(matches!(err, FileError::IsDirectory), "{err:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_access_denied() {
        let tmp = assert_ok!(tempfile::tempdir());
        let outside = tmp.path().join("secret.txt");
        assert_ok!(std::fs::write(&outside, "secret"));
        let styles = tmp.path().join("styles");
        assert_ok!(std::fs::create_dir_all(styles.join("evil")));
        assert_ok!(std::fs::write(styles.join("evil/style.typ"), ""));
        assert_ok!(std::os::unix::fs::symlink(&outside, styles.join("evil/leak.txt")));
        let style = assert_ok!(StyleSource::directory(&styles, name("evil")));
        let err = assert_err!(style.read(&vpath("/evil/leak.txt")));
        assert!(matches!(err, FileError::AccessDenied), "{err:?}");
    }

    #[test]
    fn display_paths_hide_the_canonical_root_and_mark_built_ins() {
        let disk = assert_ok!(StyleSource::directory(fixtures_path(), name("minimal")));
        assert_eq!(
            disk.display_path(&vpath("/minimal/style.typ")),
            fixtures_path().join("minimal").join("style.typ")
        );
        let built_in = assert_ok!(StyleSource::embedded(&FIXTURES, name("minimal")));
        assert_eq!(
            built_in.display_path(&vpath("/minimal/style.typ")),
            PathBuf::from("<built-in>").join("minimal").join("style.typ")
        );
    }
}

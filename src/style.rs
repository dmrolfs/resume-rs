//! ABOUTME: Where style files come from: a built-in embedded directory or a directory on disk.
//! ABOUTME: Serves bytes by virtual path and denies anything that resolves outside the style root.

use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};
use snafu::{OptionExt, ResultExt};
use typst::diag::{FileError, FileResult};
use typst::foundations::Bytes;
use typst::syntax::VirtualPath;

use crate::error::{
    Error, StyleDirectorySnafu, StyleFileSnafu, StyleMissingMainSnafu, StyleOutsideRootSnafu, StyleRootSnafu,
    UnknownStyleSnafu,
};
use crate::model::StyleName;
use crate::model::options::BuiltinStyle;

/// The built-in styles and the files they share (`lib/`), embedded at build time.
static BUILT_IN_STYLES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/styles");

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

/// Where a style listed by `resume styles` lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleLocation {
    /// Embedded in the binary.
    BuiltIn,
    /// A directory under `--styles-dir`.
    Directory(PathBuf),
}

/// One line of `resume styles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleEntry {
    /// The directory name, as it is on disk.
    pub name: String,
    /// Where it lives.
    pub location: StyleLocation,
    /// What is notable about it: that it overrides a built-in, or why it cannot be used.
    pub note: Option<String>,
}

/// Every style that can be chosen: the built-in ones, then the directories under `styles_dir`.
///
/// A directory that cannot be used is listed with the reason, not hidden, so the user can see why a style they
/// just made is not picked up.
///
/// # Errors
/// `Error::StyleRoot` - `styles_dir` cannot be read.
pub fn catalog(styles_dir: Option<&Path>) -> Result<Vec<StyleEntry>, Error> {
    let built_in = built_in_names();
    let mut entries: Vec<StyleEntry> = built_in
        .iter()
        .map(|name| StyleEntry {
            name: name.clone(),
            location: StyleLocation::BuiltIn,
            note: StyleName::try_new(name.as_str())
                .ok()
                .and_then(|name| StyleSource::embedded(&BUILT_IN_STYLES, name).ok())
                .filter(|style| style.require_letter().is_ok())
                .map(|_| "letter PDF".to_owned()),
        })
        .collect();
    if let Some(root) = styles_dir {
        entries.extend(directory_entries(root, &built_in)?);
    }
    Ok(entries)
}

/// The files of the built-in style `name` and of the `lib/` it shares, as paths relative to a styles
/// directory (`pragmatic/style.typ`, `lib/components.typ`) and their bytes.
///
/// # Errors
/// `Error::UnknownStyle` - there is no built-in style called `name`.
pub fn builtin_files(name: &StyleName) -> Result<Vec<(PathBuf, &'static [u8])>, Error> {
    let style_dir = built_in_names()
        .iter()
        .any(|built_in| built_in == name.as_ref() as &str)
        .then(|| BUILT_IN_STYLES.get_dir(name.as_ref() as &str))
        .flatten();
    let Some(style_dir) = style_dir else {
        return UnknownStyleSnafu {
            name: name.clone(),
            searched: PathBuf::from(BUILT_IN),
            available: available_styles(None),
        }
        .fail();
    };
    let mut files = Vec::new();
    for dir in std::iter::once(style_dir).chain(BUILT_IN_STYLES.get_dir(SHARED_DIR)) {
        collect_files(dir, &mut files);
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

fn collect_files(dir: &'static Dir<'static>, files: &mut Vec<(PathBuf, &'static [u8])>) {
    files.extend(dir.files().map(|file| (file.path().to_path_buf(), file.contents())));
    for inner in dir.dirs() {
        collect_files(inner, files);
    }
}

/// The directory every built-in style shares; it is not a style itself.
const SHARED_DIR: &str = "lib";

/// A directory under a styles root is a style unless it is the shared one or hidden.
fn is_style_dir(name: &str) -> bool {
    name != SHARED_DIR && !name.starts_with('.')
}

/// The built-in styles, by name: the embedded directories other than the shared `lib`.
fn built_in_names() -> Vec<String> {
    let mut names: Vec<String> = BUILT_IN_STYLES
        .dirs()
        .filter_map(|dir| dir.path().file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| is_style_dir(name))
        .collect();
    names.sort();
    names
}

/// The styles found under `root`, sorted by name, each with the reason it cannot be used if it cannot.
fn directory_entries(root: &Path, built_in: &[String]) -> Result<Vec<StyleEntry>, Error> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).context(StyleRootSnafu { path: root })? {
        let path = entry.context(StyleRootSnafu { path: root })?.path();
        let Some(name) = path.file_name().map(|name| name.to_string_lossy().into_owned()) else {
            continue;
        };
        if path.is_dir() && is_style_dir(&name) {
            found.push(StyleEntry {
                note: {
                    let note = directory_note(&path, &name, built_in);
                    let letter = StyleName::try_new(name.as_str())
                        .ok()
                        .and_then(|name| StyleSource::directory(root, name).ok())
                        .is_some_and(|style| style.require_letter().is_ok());
                    match (note, letter) {
                        (Some(note), true) => Some(format!("{note}; letter PDF")),
                        (None, true) => Some("letter PDF".to_owned()),
                        (note, false) => note,
                    }
                },
                name,
                location: StyleLocation::Directory(path),
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// Why a directory under a styles root cannot be used, or that it replaces a built-in style; `None` when it
/// is an ordinary style.
fn directory_note(path: &Path, name: &str, built_in: &[String]) -> Option<String> {
    // A name is usable only as it is written: `Mine` would be looked up as `mine`, which a case-sensitive
    // file system does not have.
    let valid_name = StyleName::try_new(name).is_ok_and(|valid| valid.as_ref() as &str == name);
    if !valid_name {
        Some("invalid: not a valid style name".to_string())
    } else if !path.join(MAIN_FILE).is_file() {
        Some("invalid: no style.typ".to_string())
    } else if built_in.iter().any(|built_in| built_in == name) {
        Some("overrides the built-in".to_string())
    } else {
        None
    }
}

/// The names that can be chosen, for an error to offer; a directory that cannot be read adds none.
fn available_styles(styles_dir: Option<&Path>) -> Vec<StyleName> {
    let mut names: Vec<StyleName> = catalog(styles_dir)
        .or_else(|_| catalog(None))
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.note.as_deref().is_none_or(|note| !note.starts_with("invalid")))
        .filter_map(|entry| StyleName::try_new(entry.name.as_str()).ok())
        .collect();
    names.sort_by(|a, b| (a.as_ref() as &str).cmp(b.as_ref()));
    names.dedup();
    names
}

/// Finds the style `name`: in `styles_dir` if one is given and has it, otherwise among the built-in styles.
///
/// A style in the directory wins over a built-in of the same name, so a copy made with `export-style` can be
/// edited in place.
///
/// # Errors
/// - `Error::StyleRoot` - `styles_dir` cannot be opened
/// - `Error::StyleMissingMain` - the style's directory has no `style.typ`
/// - `Error::UnknownStyle` - neither the directory nor the built-in set has the style
pub fn resolve(name: &StyleName, styles_dir: Option<&Path>) -> Result<StyleSource, Error> {
    // `lib` holds what the styles share and is not a style: it is not listed, and as a style it would have its
    // files counted twice.
    if name.as_ref() as &str == SHARED_DIR {
        return UnknownStyleSnafu {
            name: name.clone(),
            searched: styles_dir.map_or_else(|| PathBuf::from(BUILT_IN), Path::to_path_buf),
            available: available_styles(styles_dir),
        }
        .fail();
    }
    if let Some(root) = styles_dir {
        match StyleSource::directory(root, name.clone()) {
            // The directory has no such style: the built-in set may.
            Err(Error::UnknownStyle { .. }) => {}
            found => return found,
        }
    }
    StyleSource::embedded(&BUILT_IN_STYLES, name.clone()).map_err(|error| match error {
        Error::UnknownStyle { name, searched, .. } => UnknownStyleSnafu {
            name,
            searched: styles_dir.map_or(searched, Path::to_path_buf),
            available: available_styles(styles_dir),
        }
        .build(),
        other => other,
    })
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
            return UnknownStyleSnafu {
                name,
                searched: root,
                available: Vec::new(),
            }
            .fail();
        }
        let resolved_dir = std::fs::canonicalize(&style_dir).context(StyleRootSnafu { path: &style_dir })?;
        if !is_within(&canonical_root, &resolved_dir) {
            return StyleOutsideRootSnafu {
                dir: root.join(name.as_ref() as &str),
            }
            .fail();
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
            available: Vec::new(),
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

    /// Opens a style embedded in the binary.
    ///
    /// # Errors
    /// `Error::UnknownStyle` or `Error::StyleMissingMain` when the embedded set lacks the style, which is a
    /// build defect: every [`BuiltinStyle`] is written.
    pub fn builtin(style: BuiltinStyle) -> Result<Self, Error> {
        let name = StyleName::try_new(style.as_ref()).expect("built-in style names are valid style names");
        Self::embedded(&BUILT_IN_STYLES, name)
    }

    /// True when `vpath` is a file this style can read; for tests that check what was embedded.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn reads(&self, vpath: &VirtualPath) -> bool {
        self.read(vpath).is_ok()
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

    /// The optional letter entry, distinct from the required resume entry.
    #[must_use]
    pub fn letter_path(&self) -> VirtualPath {
        VirtualPath::new(format!("/{}/letter.typ", self.name.as_ref() as &str))
            .expect("a validated style name makes a valid virtual path")
    }

    /// Requires a readable letter entry without weakening sandbox checks.
    ///
    /// # Errors
    /// Missing capability, or a sandbox/file read failure.
    pub fn require_letter(&self) -> Result<(), Error> {
        let path = self.letter_path();
        match self.read(&path) {
            Ok(_) => Ok(()),
            Err(FileError::NotFound(_)) => crate::error::StyleMissingLetterSnafu {
                path: self.display_path(&path),
            }
            .fail(),
            Err(source) => Err(source).context(StyleFileSnafu {
                path: self.display_path(&path),
            }),
        }
    }

    /// Reads one file by virtual path.
    ///
    /// # Errors
    /// `FileError::AccessDenied` when the path resolves outside the style root (for example a
    /// symlink pointing out of it), `NotFound`, `IsDirectory`, or the underlying I/O failure.
    pub(crate) fn read(&self, vpath: &VirtualPath) -> FileResult<Bytes> {
        match &self.files {
            StyleFiles::Embedded(dir) => read_embedded(dir, vpath),
            StyleFiles::Directory { canonical_root, .. } => read_directory(canonical_root, vpath),
        }
    }

    /// Every file of this style and of the `lib/` it shares, as paths relative to the styles directory
    /// (`pragmatic/style.typ`, `lib/components.typ`) with their bytes, sorted by path.
    ///
    /// A directory style is walked: only real directories are entered, dot-files and dot-directories are left out
    /// (an editor's swap file, `.DS_Store`), and every file is read through the same sandboxed `read` that
    /// compiling uses, so a link that leaves the styles directory is refused. One file that cannot be read fails
    /// the whole call, because a silent skip would give a copy that breaks later with a less clear message. A
    /// style that imports a file outside its own directory and `lib/` is legal for `build` and is not followed.
    ///
    /// # Errors
    /// - `Error::StyleRoot` - a directory of the style cannot be listed
    /// - `Error::StyleFile` - a file cannot be read, or resolves outside the styles directory
    pub fn files(&self) -> Result<Vec<(PathBuf, Vec<u8>)>, Error> {
        let style_dir: &str = self.name.as_ref();
        let mut files = Vec::new();
        match &self.files {
            StyleFiles::Embedded(dir) => {
                let mut found = Vec::new();
                for part in [dir.get_dir(style_dir), dir.get_dir(SHARED_DIR)].into_iter().flatten() {
                    collect_files(part, &mut found);
                }
                files.extend(found.into_iter().map(|(path, bytes)| (path, bytes.to_vec())));
            }
            StyleFiles::Directory { canonical_root, .. } => {
                for part in [style_dir, SHARED_DIR] {
                    walk_directory(canonical_root, Path::new(part), &mut files)?;
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }

    /// The path to show users for a virtual path.
    #[must_use]
    pub(crate) fn display_path(&self, vpath: &VirtualPath) -> PathBuf {
        let base = match &self.files {
            StyleFiles::Embedded(_) => PathBuf::from(BUILT_IN),
            StyleFiles::Directory { root, .. } => root.clone(),
        };
        // Push each segment so Windows gets one separator style, not `\\` mixed with `/`.
        vpath
            .get_without_slash()
            .split('/')
            .filter(|s| !s.is_empty())
            .fold(base, |path, segment| path.join(segment))
    }
}

/// Adds every file under `root/relative` to `files`; a missing `relative` adds none (a style need not use `lib/`).
fn walk_directory(root: &Path, relative: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) -> Result<(), Error> {
    let here = root.join(relative);
    let entries = match std::fs::read_dir(&here) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(source).context(StyleDirectorySnafu { path: here }),
    };
    for entry in entries {
        let entry = entry.context(StyleDirectorySnafu { path: &here })?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let child = relative.join(&name);
        let kind = entry
            .file_type()
            .context(StyleDirectorySnafu { path: here.join(&name) })?;
        if kind.is_dir() {
            walk_directory(root, &child, files)?;
        } else {
            let virtual_path = VirtualPath::new(format!("/{}", slash_separated(&child)));
            let read = virtual_path
                .map_err(|_| FileError::AccessDenied)
                .and_then(|vpath| read_directory(root, &vpath));
            let bytes = read.context(StyleFileSnafu { path: here.join(&name) })?;
            files.push((child, bytes.to_vec()));
        }
    }
    Ok(())
}

/// `a/b/c` for a relative path, with `/` on every platform, which is what a virtual path wants.
fn slash_separated(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn read_embedded(dir: &'static Dir<'static>, vpath: &VirtualPath) -> FileResult<Bytes> {
    let relative = vpath.get_without_slash();
    if let Some(file) = dir.get_file(relative) {
        return Ok(Bytes::new(file.contents()));
    }
    if dir.get_dir(relative).is_some() || vpath.is_root() {
        return Err(FileError::IsDirectory);
    }
    Err(FileError::NotFound(PathBuf::from(relative)))
}

fn read_directory(canonical_root: &Path, vpath: &VirtualPath) -> FileResult<Bytes> {
    let relative = Path::new(vpath.get_without_slash());
    let target = vpath.realize(canonical_root).map_err(FileError::Realize)?;
    let resolved = std::fs::canonicalize(&target).map_err(|e| FileError::from_io(e, relative))?;
    if !is_within(canonical_root, &resolved) {
        return Err(FileError::AccessDenied);
    }
    let metadata = std::fs::metadata(&resolved).map_err(|e| FileError::from_io(e, relative))?;
    if metadata.is_dir() {
        return Err(FileError::IsDirectory);
    }
    if !metadata.is_file() {
        // A FIFO or device inside the root would block the read forever.
        return Err(FileError::Other(Some("not a regular file".into())));
    }
    std::fs::read(&resolved)
        .map(Bytes::new)
        .map_err(|e| FileError::from_io(e, relative))
}

/// True when `path` is `root` or lies under it, compared by path components, not string prefix.
///
/// Both arguments must already be canonical.
fn is_within(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok, assert_some};
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
    fn files_of_a_built_in_style_are_its_directory_and_the_lib_sorted() {
        let style = assert_ok!(resolve(&name("pragmatic"), None));
        let files = assert_ok!(style.files());
        let paths: Vec<String> = files.iter().map(|(p, _)| slash_separated(p)).collect();
        assert!(paths.windows(2).all(|w| w[0] < w[1]), "{paths:?}");
        assert!(paths.contains(&"pragmatic/style.typ".to_string()), "{paths:?}");
        assert!(paths.contains(&"lib/components.typ".to_string()), "{paths:?}");
        assert!(
            paths
                .iter()
                .all(|p| p.starts_with("pragmatic/") || p.starts_with("lib/")),
            "{paths:?}"
        );
        let (_, theme) = assert_some!(files.iter().find(|(p, _)| slash_separated(p) == "pragmatic/theme.typ"));
        assert_eq!(
            theme.as_slice(),
            assert_some!(BUILT_IN_STYLES.get_file("pragmatic/theme.typ")).contents()
        );
    }

    #[test]
    fn files_of_a_directory_style_match_its_bytes_and_skip_dot_files_and_other_styles() {
        let tmp = assert_ok!(tempfile::tempdir());
        for (path, text) in [
            ("mine/style.typ", "a"),
            ("mine/sub/part.typ", "b"),
            ("mine/.hidden", "x"),
            ("mine/.swap/inner.typ", "x"),
            ("lib/shared.typ", "c"),
            ("other/style.typ", "not mine"),
        ] {
            let full = tmp.path().join(path);
            assert_ok!(std::fs::create_dir_all(full.parent().expect("parent")));
            assert_ok!(std::fs::write(full, text));
        }
        let style = assert_ok!(StyleSource::directory(tmp.path(), name("mine")));
        let files = assert_ok!(style.files());
        let listed: Vec<(String, &[u8])> = files.iter().map(|(p, b)| (slash_separated(p), b.as_slice())).collect();
        assert_eq!(
            listed,
            [
                ("lib/shared.typ".to_string(), b"c".as_slice()),
                ("mine/style.typ".to_string(), b"a".as_slice()),
                ("mine/sub/part.typ".to_string(), b"b".as_slice()),
            ]
        );
    }

    #[test]
    fn a_directory_style_without_a_lib_lists_just_its_own_files() {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(tmp.path().join("mine")));
        assert_ok!(std::fs::write(tmp.path().join("mine/style.typ"), "a"));
        let files = assert_ok!(assert_ok!(StyleSource::directory(tmp.path(), name("mine"))).files());
        assert_eq!(files.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_links_outside_the_styles_directory_fails_the_listing() {
        let tmp = assert_ok!(tempfile::tempdir());
        let root = tmp.path().join("root");
        assert_ok!(std::fs::create_dir_all(root.join("mine")));
        assert_ok!(std::fs::write(root.join("mine/style.typ"), "a"));
        assert_ok!(std::fs::write(tmp.path().join("secret"), "s"));
        assert_ok!(std::os::unix::fs::symlink(
            tmp.path().join("secret"),
            root.join("mine/link.typ")
        ));
        let err = assert_err!(assert_ok!(StyleSource::directory(&root, name("mine"))).files());
        assert!(
            matches!(
                err,
                Error::StyleFile {
                    source: FileError::AccessDenied,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_directory_is_not_entered() {
        let tmp = assert_ok!(tempfile::tempdir());
        let root = tmp.path().join("root");
        assert_ok!(std::fs::create_dir_all(root.join("mine")));
        assert_ok!(std::fs::write(root.join("mine/style.typ"), "a"));
        assert_ok!(std::os::unix::fs::symlink(&root, root.join("mine/loop")));
        let err = assert_err!(assert_ok!(StyleSource::directory(&root, name("mine"))).files());
        // A link to a directory is read as a file, which is refused: no loop and no escape.
        assert!(matches!(err, Error::StyleFile { .. }), "{err:?}");
    }

    #[test]
    fn the_shared_lib_directory_is_not_a_style_built_in_or_in_a_styles_directory() {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir_all(tmp.path().join("lib")));
        assert_ok!(std::fs::write(tmp.path().join("lib/style.typ"), "x"));
        for styles_dir in [None, Some(tmp.path())] {
            let err = assert_err!(resolve(&name("lib"), styles_dir));
            assert!(matches!(err, Error::UnknownStyle { .. }), "{err:?}");
        }
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
            Error::UnknownStyle { name, searched, .. } => {
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

    #[test]
    fn is_within_compares_components_not_string_prefixes() {
        let root = Path::new("/tmp/styles");
        assert!(is_within(root, Path::new("/tmp/styles/lib/helper.typ")));
        assert!(is_within(root, root));
        assert!(
            !is_within(root, Path::new("/tmp/styles-evil/x.typ")),
            "a sibling sharing a string prefix is outside"
        );
        assert!(!is_within(root, Path::new("/tmp/other/styles/x.typ")));
    }

    #[test]
    fn dot_segments_in_the_given_root_are_resolved_but_shown_as_given() {
        let given = fixtures_path().join("minimal").join("..");
        let style = assert_ok!(StyleSource::directory(&given, name("minimal")));
        assert_ok!(style.read(&vpath("/minimal/style.typ")));
        assert_eq!(
            style.display_path(&vpath("/minimal/style.typ")),
            given.join("minimal").join("style.typ")
        );
    }

    #[cfg(unix)]
    #[test]
    fn style_directory_symlinked_outside_the_root_is_refused_up_front() {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir_all(tmp.path().join("elsewhere/mine")));
        assert_ok!(std::fs::write(tmp.path().join("elsewhere/mine/style.typ"), ""));
        assert_ok!(std::fs::create_dir(tmp.path().join("styles")));
        assert_ok!(std::os::unix::fs::symlink(
            tmp.path().join("elsewhere/mine"),
            tmp.path().join("styles/mine")
        ));
        let err = assert_err!(StyleSource::directory(tmp.path().join("styles"), name("mine")));
        assert!(matches!(err, Error::StyleOutsideRoot { .. }), "{err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_inside_the_root_is_readable_and_a_dangling_one_is_not_found() {
        let tmp = assert_ok!(tempfile::tempdir());
        let dir = tmp.path().join("ok");
        assert_ok!(std::fs::create_dir(&dir));
        assert_ok!(std::fs::write(dir.join("style.typ"), "real"));
        assert_ok!(std::os::unix::fs::symlink(dir.join("style.typ"), dir.join("alias.typ")));
        assert_ok!(std::os::unix::fs::symlink(
            dir.join("gone.typ"),
            dir.join("dangling.typ")
        ));
        let style = assert_ok!(StyleSource::directory(tmp.path(), name("ok")));
        assert_eq!(assert_ok!(style.read(&vpath("/ok/alias.typ"))).as_slice(), b"real");
        let err = assert_err!(style.read(&vpath("/ok/dangling.typ")));
        assert!(matches!(err, FileError::NotFound(_)), "{err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_rejected_instead_of_blocking_the_read() {
        let tmp = assert_ok!(tempfile::tempdir());
        let dir = tmp.path().join("pipe");
        assert_ok!(std::fs::create_dir(&dir));
        assert_ok!(std::fs::write(dir.join("style.typ"), ""));
        let made = std::process::Command::new("mkfifo").arg(dir.join("fifo")).status();
        assert!(assert_ok!(made).success());
        let style = assert_ok!(StyleSource::directory(tmp.path(), name("pipe")));
        let err = assert_err!(style.read(&vpath("/pipe/fifo")));
        assert!(matches!(err, FileError::Other(_)), "{err:?}");
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

    #[test]
    fn resolve_finds_a_built_in_style_when_there_is_no_directory() {
        let style = assert_ok!(resolve(&name("pragmatic"), None));
        assert_eq!(style.name().as_ref() as &str, "pragmatic");
    }

    #[test]
    fn resolve_uses_the_style_in_the_directory_when_it_has_one() {
        // `minimal` exists only as a fixture directory, so finding it proves the directory is searched.
        let found = assert_ok!(resolve(&name("minimal"), Some(&fixtures_path())));
        assert_eq!(found.name().as_ref() as &str, "minimal");
        assert!(found.reads(&found.main_path()));
    }

    #[test]
    fn resolve_falls_back_to_the_built_in_set_when_the_directory_lacks_the_style() {
        let style = assert_ok!(resolve(&name("pragmatic"), Some(&fixtures_path())));
        assert_eq!(style.name().as_ref() as &str, "pragmatic");
        assert!(style.reads(&style.main_path()), "the built-in files are read");
    }

    #[test]
    fn resolve_reports_an_unknown_style_and_a_directory_that_cannot_be_opened() {
        let unknown = assert_err!(resolve(&name("nothing-like-it"), Some(&fixtures_path())));
        assert!(matches!(unknown, Error::UnknownStyle { .. }), "{unknown}");
        let missing = assert_err!(resolve(&name("pragmatic"), Some(Path::new("/no/such/styles"))));
        assert!(matches!(missing, Error::StyleRoot { .. }), "{missing}");
    }

    // ========================================================================
    // The catalog, the built-in files, and the list of styles an error offers
    // ========================================================================

    /// The names of the built-in styles, read from the directories under `styles/` (except the shared `lib`), so
    /// a new built-in style does not break a test that only means "the built-in ones".
    fn built_in_names() -> Vec<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("styles");
        let mut names: Vec<String> = assert_ok!(std::fs::read_dir(root))
            .map(|entry| assert_ok!(entry).path())
            .filter(|path| path.is_dir())
            .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .filter(|name| name != "lib" && !name.starts_with('.'))
            .collect();
        names.sort();
        names
    }

    /// A styles directory holding every kind of entry the catalog has to tell apart.
    fn varied_styles_dir() -> tempfile::TempDir {
        let dir = assert_ok!(tempfile::tempdir());
        for (style, has_main) in [("mine", true), ("pragmatic", true), ("Bad Name", true), ("half", false)] {
            let path = dir.path().join(style);
            assert_ok!(std::fs::create_dir(&path));
            if has_main {
                assert_ok!(std::fs::write(path.join("style.typ"), b"x"));
            }
        }
        assert_ok!(std::fs::create_dir(dir.path().join("lib")));
        assert_ok!(std::fs::create_dir(dir.path().join(".hidden")));
        assert_ok!(std::fs::write(dir.path().join(".hidden").join("style.typ"), b"x"));
        assert_ok!(std::fs::write(dir.path().join("notes.txt"), b"x"));
        dir
    }

    #[test]
    fn the_catalog_lists_the_embedded_styles_and_not_the_shared_lib() {
        let entries = assert_ok!(catalog(None));
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, built_in_names());
        assert!(entries.iter().all(|e| e.location == StyleLocation::BuiltIn
            && e.note.as_deref() == (e.name == "pragmatic").then_some("letter PDF")));
    }

    #[test]
    fn the_catalog_adds_directory_styles_and_says_which_override_or_cannot_be_used() {
        let dir = varied_styles_dir();
        let entries = assert_ok!(catalog(Some(dir.path())));
        let summary: Vec<(&str, Option<&str>)> = entries.iter().map(|e| (e.name.as_str(), e.note.as_deref())).collect();
        // The built-in styles come first; the directory's entries follow, with its `pragmatic` overriding.
        let built_in = built_in_names();
        let mut expected: Vec<(&str, Option<&str>)> = built_in
            .iter()
            .map(|name| (name.as_str(), (name == "pragmatic").then_some("letter PDF")))
            .collect();
        expected.extend([
            ("Bad Name", Some("invalid: not a valid style name")),
            ("half", Some("invalid: no style.typ")),
            ("mine", None),
            ("pragmatic", Some("overrides the built-in")),
        ]);
        assert_eq!(summary, expected);
        let mine = built_in.len() + 2;
        assert_eq!(
            entries[mine].location,
            StyleLocation::Directory(dir.path().join("mine"))
        );
    }

    #[test]
    fn the_catalog_fails_when_the_styles_directory_cannot_be_read() {
        let err = assert_err!(catalog(Some(Path::new("/no/such/styles"))));
        assert!(matches!(err, Error::StyleRoot { .. }), "{err}");
    }

    #[test]
    fn the_built_in_files_are_the_style_and_the_lib_it_shares_with_their_bytes() {
        let files = assert_ok!(builtin_files(&name("pragmatic")));
        let paths: Vec<String> = files
            .iter()
            .map(|(p, _)| p.to_string_lossy().replace('\\', "/"))
            .collect();
        for expected in [
            "pragmatic/style.typ",
            "pragmatic/theme.typ",
            "pragmatic/README.md",
            "lib/components.typ",
        ] {
            assert!(
                paths.iter().any(|p| p == expected),
                "{expected} is missing from {paths:?}"
            );
        }
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("styles");
        for (path, bytes) in &files {
            assert_eq!(
                *bytes,
                assert_ok!(std::fs::read(repo.join(path))).as_slice(),
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn an_unknown_built_in_style_has_no_files() {
        let err = assert_err!(builtin_files(&name("nothing-like-it")));
        assert!(matches!(err, Error::UnknownStyle { .. }), "{err}");
    }

    #[test]
    fn the_styles_an_error_offers_leave_out_the_ones_that_cannot_be_used() {
        // `half` has no style.typ and `Bad Name` is not a name, so neither can be chosen; `pragmatic` is
        // both built in and in the directory and is offered once.
        let dir = varied_styles_dir();
        let found = assert_err!(resolve(&name("nothing-like-it"), Some(dir.path())));
        let Error::UnknownStyle { available, .. } = &found else {
            panic!("expected UnknownStyle, got {found}")
        };
        let names: Vec<&str> = available.iter().map(AsRef::as_ref).collect();
        let mut expected = built_in_names();
        expected.push("mine".to_string());
        expected.sort();
        assert_eq!(names, expected);
    }

    #[test]
    fn an_unknown_style_lists_the_styles_that_exist() {
        let found = assert_err!(resolve(&name("nothing-like-it"), Some(&fixtures_path())));
        let Error::UnknownStyle { available, .. } = &found else {
            panic!("expected UnknownStyle, got {found}")
        };
        let names: Vec<&str> = available.iter().map(AsRef::as_ref).collect();
        assert!(names.contains(&"pragmatic") && names.contains(&"minimal"), "{names:?}");
        assert!(found.to_string().contains("; available styles: "), "{found}");
        assert!(!names.contains(&"lib"), "{names:?}");
    }
}

//! ABOUTME: The Typst `World` for one render: style files, the resume as /resume.json, and a fixed font set.
//! ABOUTME: Exposes no clock, no packages, and nothing outside the style root; every file is read once.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Array, Bytes, Datetime, Dict, Duration, Str, Value};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

use crate::error::{StyleFile, StyleSources};
use crate::model::options::Paper;
use crate::style::StyleSource;
use crate::typeset::fonts::FontSet;

/// The virtual path every style reads the resume from.
pub(crate) const RESUME_JSON: &str = "/resume.json";

pub(crate) const LETTER_JSON: &str = "/letter.json";

/// Entry and payload identity travel together; no caller can mix their document kinds.
#[derive(Debug, Clone, Copy, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum DocumentKind {
    Resume,
    Letter,
}

impl DocumentKind {
    pub(crate) fn entry(self, style: &StyleSource) -> VirtualPath {
        match self {
            Self::Resume => style.main_path(),
            Self::Letter => style.letter_path(),
        }
    }
    fn payload(self) -> &'static str {
        match self {
            Self::Resume => RESUME_JSON,
            Self::Letter => LETTER_JSON,
        }
    }
}

fn reserved(id: FileId) -> bool {
    matches!(id.vpath().get_with_slash(), RESUME_JSON | LETTER_JSON)
}

const PACKAGES_UNSUPPORTED: &str = "packages are not supported; copy the file into the style directory";

/// The UTF-8 byte order mark that Windows editors prepend; Typst's own CLI strips it.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Everything Typst may see while compiling one resume with one style.
///
/// Typst assumes a `World` answers the same question the same way for a whole compile, so each file
/// is read from the style once and then served from memory.
pub(crate) struct ResumeWorld {
    library: LazyHash<Library>,
    fonts: Arc<FontSet>,
    main: FileId,
    resume_id: FileId,
    resume_json: Bytes,
    style: StyleSource,
    /// Raw bytes of every style file read so far, including failed reads.
    files: Mutex<HashMap<FileId, FileResult<Bytes>>>,
    /// Decoded sources served so far; also the text diagnostics are shown against.
    sources: Mutex<HashMap<FileId, Source>>,
}

impl ResumeWorld {
    pub(crate) fn new(style: StyleSource, fonts: Arc<FontSet>, resume_json: Vec<u8>, paper: Paper) -> Self {
        Self::for_kind(DocumentKind::Resume, style, fonts, resume_json, paper)
    }

    pub(crate) fn for_kind(
        kind: DocumentKind,
        style: StyleSource,
        fonts: Arc<FontSet>,
        resume_json: Vec<u8>,
        paper: Paper,
    ) -> Self {
        let inputs = Dict::from_iter([
            (Str::from("paper"), Value::Str(Str::from(paper.as_ref()))),
            (Str::from("fonts"), Value::Array(installed_families(&fonts))),
        ]);
        Self {
            library: LazyHash::new(Library::builder().with_inputs(inputs).build()),
            fonts,
            main: project_file(kind.entry(&style)),
            resume_id: project_file(VirtualPath::new(kind.payload()).expect("document payload paths are valid")),
            resume_json: Bytes::new(resume_json),
            style,
            files: Mutex::new(HashMap::new()),
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// The path to show users for a file, or `None` for the in-memory resume and package files.
    pub(crate) fn display_path(&self, id: FileId) -> Option<PathBuf> {
        (!reserved(id) && matches!(id.root(), VirtualRoot::Project)).then(|| self.style.display_path(id.vpath()))
    }

    /// The text of every style source file served so far, in display-path order.
    pub(crate) fn served_sources(&self) -> StyleSources {
        let mut served: StyleSources = lock(&self.sources)
            .iter()
            .filter_map(|(id, source)| {
                Some(StyleFile {
                    path: self.display_path(*id)?,
                    text: source.text().to_string(),
                })
            })
            .collect();
        served.sort_by(|a, b| a.path.cmp(&b.path));
        served
    }

    /// Reads a style file once; later calls return the first answer, success or failure.
    fn load(&self, id: FileId) -> FileResult<Bytes> {
        require_project_file(id)?;
        if id == self.resume_id {
            return Ok(self.resume_json.clone());
        }
        if reserved(id) {
            return Err(FileError::NotFound(PathBuf::from(id.vpath().get_with_slash())));
        }
        if let Some(known) = lock(&self.files).get(&id) {
            return known.clone();
        }
        let read = self.style.read(id.vpath());
        lock(&self.files).insert(id, read.clone());
        read
    }

    fn decode(&self, id: FileId, bytes: &Bytes) -> FileResult<String> {
        let bytes = bytes.as_slice();
        let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
        String::from_utf8(bytes.to_vec()).map_err(|_| {
            let shown = self
                .display_path(id)
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            FileError::Other(Some(format!("{shown}: file is not valid UTF-8").into()))
        })
    }
}

/// Locks a map; a panic while holding the lock cannot leave it half-updated, because entries are inserted whole.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn require_project_file(id: FileId) -> FileResult<()> {
    match id.root() {
        VirtualRoot::Project => Ok(()),
        VirtualRoot::Package(_) => Err(FileError::Package(PackageError::Other(Some(
            PACKAGES_UNSUPPORTED.into(),
        )))),
    }
}

/// The families Typst can use, lowercased as Typst folds them, for `sys.inputs.fonts`.
///
/// Typst warns `unknown font family` for every missing name in a `text(font: (..))` list and cannot
/// be told not to, so a style builds its list from this instead (the `font-stack` helper).
pub(crate) fn installed_families(fonts: &FontSet) -> Array {
    fonts
        .families()
        .into_iter()
        .map(|family| Value::Str(Str::from(family.to_lowercase())))
        .collect()
}

fn project_file(vpath: VirtualPath) -> FileId {
    RootedPath::new(VirtualRoot::Project, vpath).intern()
}

impl World for ResumeWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        require_project_file(id)?;
        if reserved(id) {
            return Err(FileError::NotSource);
        }
        if let Some(cached) = lock(&self.sources).get(&id) {
            return Ok(cached.clone());
        }
        let source = Source::new(id, self.decode(id, &self.load(id)?)?);
        lock(&self.sources).insert(id, source.clone());
        Ok(source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.load(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    /// Always `None`: a style must not make output depend on the date.
    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::StyleName;
    use claims::{assert_err, assert_none, assert_ok};
    use pretty_assertions::assert_eq;
    use typst::syntax::package::PackageSpec;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/styles")
    }

    fn style(root: &std::path::Path, name: &str) -> StyleSource {
        assert_ok!(StyleSource::directory(root, assert_ok!(StyleName::try_new(name))))
    }

    fn world_of(style: StyleSource) -> ResumeWorld {
        ResumeWorld::new(
            style,
            Arc::new(FontSet::bundled()),
            br#"{"name":"Ada"}"#.to_vec(),
            Paper::Letter,
        )
    }

    fn world(root: &std::path::Path, name: &str) -> ResumeWorld {
        world_of(style(root, name))
    }

    fn id(path: &str) -> FileId {
        project_file(assert_ok!(VirtualPath::new(path)))
    }

    /// A temporary styles directory whose style `s` has the given `style.typ` bytes.
    fn temp_style(bytes: &[u8]) -> (tempfile::TempDir, ResumeWorld) {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(tmp.path().join("s")));
        assert_ok!(std::fs::write(tmp.path().join("s/style.typ"), bytes));
        let world = world(tmp.path(), "s");
        (tmp, world)
    }

    #[test]
    fn style_files_resolve_through_virtual_path_components() {
        let world = world(&fixtures(), "lib-import");
        assert_eq!(world.main().vpath().get_with_slash(), "/lib-import/style.typ");
        assert!(assert_ok!(world.source(world.main())).text().contains("import"));
        let helper = assert_ok!(world.source(id("/lib/helper.typ")));
        assert!(helper.text().contains("greeting"));
    }

    #[test]
    fn installed_families_are_the_lowercased_families_of_the_font_set() {
        let families = installed_families(&FontSet::bundled());
        let names: Vec<String> = families
            .iter()
            .map(|v| assert_ok!(v.clone().cast::<Str>()).to_string())
            .collect();
        assert_eq!(
            names,
            [
                "arimo",
                "carlito",
                "dejavu sans mono",
                "gelasio",
                "libertinus serif",
                "new computer modern",
                "new computer modern math",
                "tex gyre pagella"
            ]
        );
    }

    #[test]
    fn both_payload_paths_are_reserved_across_document_kinds() {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(tmp.path().join("s")));
        for file in ["s/style.typ", "s/letter.typ", "resume.json", "letter.json"] {
            assert_ok!(std::fs::write(tmp.path().join(file), "stale private content"));
        }
        for (kind, own, other, entry) in [
            (DocumentKind::Resume, RESUME_JSON, LETTER_JSON, "/s/style.typ"),
            (DocumentKind::Letter, LETTER_JSON, RESUME_JSON, "/s/letter.typ"),
        ] {
            let world = ResumeWorld::for_kind(
                kind,
                style(tmp.path(), "s"),
                Arc::new(FontSet::bundled()),
                b"current".to_vec(),
                Paper::Letter,
            );
            assert_eq!(world.main().vpath().get_with_slash(), entry);
            assert_eq!(assert_ok!(world.file(id(own))).as_slice(), b"current");
            assert!(matches!(assert_err!(world.file(id(other))), FileError::NotFound(_)));
            for payload in [own, other] {
                assert_none!(world.display_path(id(payload)));
                assert!(matches!(assert_err!(world.source(id(payload))), FileError::NotSource));
            }
        }
    }

    #[test]
    fn today_is_none() {
        let world = world(&fixtures(), "minimal");
        assert_none!(world.today(None));
        assert_none!(world.today(Some(Duration::from(time::Duration::hours(1)))));
    }

    #[test]
    fn resume_json_is_served_from_memory_and_shadows_disk() {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(tmp.path().join("shadow")));
        assert_ok!(std::fs::write(tmp.path().join("shadow/style.typ"), ""));
        assert_ok!(std::fs::write(tmp.path().join("resume.json"), "from disk"));
        let world = world(tmp.path(), "shadow");
        assert_eq!(assert_ok!(world.file(id(RESUME_JSON))).as_slice(), br#"{"name":"Ada"}"#);
        assert!(matches!(
            assert_err!(world.source(id(RESUME_JSON))),
            FileError::NotSource
        ));
    }

    #[test]
    fn package_roots_are_refused_with_a_clear_message() {
        let world = world(&fixtures(), "minimal");
        let spec = assert_ok!("@preview/example:0.1.0".parse::<PackageSpec>());
        let package_file =
            RootedPath::new(VirtualRoot::Package(spec), assert_ok!(VirtualPath::new("/typst.toml"))).intern();
        for err in [
            assert_err!(world.source(package_file)),
            assert_err!(world.file(package_file)),
        ] {
            let shown = err.to_string();
            assert!(shown.contains("packages are not supported"), "{shown}");
        }
    }

    #[test]
    fn served_sources_are_sorted_by_path_with_their_text() {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(tmp.path().join("s")));
        assert_ok!(std::fs::write(tmp.path().join("s/style.typ"), "main"));
        for name in ["d", "c", "b", "a"] {
            assert_ok!(std::fs::write(
                tmp.path().join(format!("s/{name}.typ")),
                format!("text of {name}")
            ));
        }
        let world = world(tmp.path(), "s");
        // Request in reverse order so insertion order cannot explain a sorted result.
        for name in ["d", "c", "b", "a"] {
            assert_ok!(world.source(id(&format!("/s/{name}.typ"))));
        }
        assert_ok!(world.source(world.main()));
        assert_ok!(world.file(id(RESUME_JSON)));
        let served: Vec<(String, String)> = world
            .served_sources()
            .into_iter()
            .map(|f| {
                (
                    f.path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                    f.text,
                )
            })
            .collect();
        let expected: Vec<(String, String)> = [
            ("a.typ", "text of a"),
            ("b.typ", "text of b"),
            ("c.typ", "text of c"),
            ("d.typ", "text of d"),
            ("style.typ", "main"),
        ]
        .into_iter()
        .map(|(n, t)| (n.to_string(), t.to_string()))
        .collect();
        assert_eq!(served, expected);
    }

    #[test]
    fn a_utf8_bom_is_stripped_from_sources() {
        let (_tmp, world) = temp_style(b"\xEF\xBB\xBF#set page(width: 10cm)\n");
        let source = assert_ok!(world.source(world.main()));
        assert_eq!(source.text(), "#set page(width: 10cm)\n");
        let bytes = assert_ok!(world.file(world.main()));
        assert!(
            bytes.as_slice().starts_with(b"\xEF\xBB\xBF"),
            "file() serves the raw bytes"
        );
    }

    #[test]
    fn invalid_utf8_is_reported_with_the_file_name() {
        let (_tmp, world) = temp_style(b"#let x = \xFF\xFE\n");
        let message = assert_err!(world.source(world.main())).to_string();
        assert!(
            message.contains("style.typ") && message.contains("not valid UTF-8"),
            "{message}"
        );
    }

    #[test]
    fn a_file_changed_mid_render_still_answers_as_first_read() {
        let (tmp, world) = temp_style(b"first");
        let path = tmp.path().join("s/style.typ");
        assert_eq!(assert_ok!(world.file(world.main())).as_slice(), b"first");
        assert_ok!(std::fs::write(&path, "second"));
        assert_eq!(assert_ok!(world.file(world.main())).as_slice(), b"first");
        assert_eq!(assert_ok!(world.source(world.main())).text(), "first");
    }

    #[test]
    fn a_failed_read_is_remembered_too() {
        let (tmp, world) = temp_style(b"x");
        let missing = id("/s/later.typ");
        assert!(matches!(assert_err!(world.file(missing)), FileError::NotFound(_)));
        assert_ok!(std::fs::write(tmp.path().join("s/later.typ"), "appeared"));
        assert!(matches!(assert_err!(world.file(missing)), FileError::NotFound(_)));
    }

    #[test]
    fn a_poisoned_cache_lock_does_not_stop_the_render() {
        let (_tmp, world) = temp_style(b"ok");
        let poisoner = std::thread::scope(|s| {
            s.spawn(|| {
                let _held = world.sources.lock();
                panic!("poison the lock");
            })
            .join()
        });
        assert!(poisoner.is_err());
        assert!(world.sources.is_poisoned());
        assert_eq!(assert_ok!(world.source(world.main())).text(), "ok");
        assert_eq!(world.served_sources().len(), 1);
    }

    #[test]
    fn backslash_style_path_is_rejected_by_virtual_path() {
        assert_err!(VirtualPath::new("/lib\\helper.typ"));
    }

    #[test]
    fn virtualize_builds_from_path_components_not_strings() {
        let root = PathBuf::from("styles");
        let nested: PathBuf = ["styles", "lib", "helper.typ"].iter().collect();
        let vpath = assert_ok!(VirtualPath::virtualize(&root, &nested));
        assert_eq!(vpath.get_with_slash(), "/lib/helper.typ");
    }

    #[cfg(windows)]
    #[test]
    fn virtualize_and_realize_handle_drive_letter_roots() {
        let root = PathBuf::from(r"C:\styles");
        let nested = PathBuf::from(r"C:\styles\lib\helper.typ");
        let vpath = assert_ok!(VirtualPath::virtualize(&root, &nested));
        assert_eq!(vpath.get_with_slash(), "/lib/helper.typ");
        assert_eq!(assert_ok!(vpath.realize(&root)), nested);
    }
}

//! ABOUTME: The Typst `World` for one render: style files, the resume as /resume.json, and a fixed font set.
//! ABOUTME: Exposes no clock, no packages, and nothing outside the style root.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Bytes, Datetime, Dict, Duration, Str, Value};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

use crate::error::StyleSources;
use crate::model::options::Paper;
use crate::style::StyleSource;
use crate::typeset::fonts::FontSet;

/// The virtual path every style reads the resume from.
pub(crate) const RESUME_JSON: &str = "/resume.json";

const PACKAGES_UNSUPPORTED: &str = "packages are not supported; copy the file into the style directory";

/// Everything Typst may see while compiling one resume with one style.
pub(crate) struct ResumeWorld<'a> {
    library: LazyHash<Library>,
    fonts: Arc<FontSet>,
    main: FileId,
    resume_id: FileId,
    resume_json: Bytes,
    style: &'a StyleSource,
    /// Style sources served so far; also the text diagnostics are shown against.
    sources: Mutex<HashMap<FileId, Source>>,
}

impl<'a> ResumeWorld<'a> {
    pub(crate) fn new(style: &'a StyleSource, fonts: Arc<FontSet>, resume_json: Vec<u8>, paper: Paper) -> Self {
        let inputs = Dict::from_iter([(Str::from("paper"), Value::Str(Str::from(paper.as_ref())))]);
        Self {
            library: LazyHash::new(Library::builder().with_inputs(inputs).build()),
            fonts,
            main: project_file(style.main_path()),
            resume_id: project_file(VirtualPath::new(RESUME_JSON).expect("the resume path is a valid virtual path")),
            resume_json: Bytes::new(resume_json),
            style,
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// The path to show users for a file, or `None` for the in-memory resume and package files.
    pub(crate) fn display_path(&self, id: FileId) -> Option<PathBuf> {
        (id != self.resume_id && matches!(id.root(), VirtualRoot::Project)).then(|| self.style.display_path(id.vpath()))
    }

    /// The text of every style file served so far, in display-path order.
    pub(crate) fn served_sources(&self) -> StyleSources {
        let mut served: StyleSources = self
            .lock_sources()
            .iter()
            .filter_map(|(id, source)| Some((self.display_path(*id)?, source.text().to_string())))
            .collect();
        served.sort();
        served
    }

    fn lock_sources(&self) -> MutexGuard<'_, HashMap<FileId, Source>> {
        // A panic while holding the lock cannot leave the cache half-updated: entries are inserted whole.
        self.sources.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn require_project_file(id: FileId) -> FileResult<()> {
        match id.root() {
            VirtualRoot::Project => Ok(()),
            VirtualRoot::Package(_) => Err(FileError::Package(PackageError::Other(Some(
                PACKAGES_UNSUPPORTED.into(),
            )))),
        }
    }
}

fn project_file(vpath: VirtualPath) -> FileId {
    RootedPath::new(VirtualRoot::Project, vpath).intern()
}

impl World for ResumeWorld<'_> {
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
        Self::require_project_file(id)?;
        if id == self.resume_id {
            return Err(FileError::NotSource);
        }
        if let Some(cached) = self.lock_sources().get(&id) {
            return Ok(cached.clone());
        }
        let bytes = self.style.read(id.vpath())?;
        let text = String::from_utf8(bytes).map_err(|_| FileError::InvalidUtf8)?;
        let source = Source::new(id, text);
        self.lock_sources().insert(id, source.clone());
        Ok(source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Self::require_project_file(id)?;
        if id == self.resume_id {
            return Ok(self.resume_json.clone());
        }
        self.style.read(id.vpath()).map(Bytes::new)
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

    fn world<'a>(style: &'a StyleSource) -> ResumeWorld<'a> {
        ResumeWorld::new(
            style,
            Arc::new(FontSet::bundled()),
            br#"{"name":"Ada"}"#.to_vec(),
            Paper::Letter,
        )
    }

    fn id(path: &str) -> FileId {
        project_file(assert_ok!(VirtualPath::new(path)))
    }

    #[test]
    fn style_files_resolve_through_virtual_path_components() {
        let style = style(&fixtures(), "lib-import");
        let world = world(&style);
        assert_eq!(world.main().vpath().get_with_slash(), "/lib-import/style.typ");
        assert!(assert_ok!(world.source(world.main())).text().contains("import"));
        let helper = assert_ok!(world.source(id("/lib/helper.typ")));
        assert!(helper.text().contains("greeting"));
    }

    #[test]
    fn today_is_none() {
        let style = style(&fixtures(), "minimal");
        let world = world(&style);
        assert_none!(world.today(None));
        assert_none!(world.today(Some(Duration::from(time::Duration::hours(1)))));
    }

    #[test]
    fn resume_json_is_served_from_memory_and_shadows_disk() {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(tmp.path().join("shadow")));
        assert_ok!(std::fs::write(tmp.path().join("shadow/style.typ"), ""));
        assert_ok!(std::fs::write(tmp.path().join("resume.json"), "from disk"));
        let style = style(tmp.path(), "shadow");
        let world = world(&style);
        assert_eq!(assert_ok!(world.file(id(RESUME_JSON))).as_slice(), br#"{"name":"Ada"}"#);
        assert!(matches!(
            assert_err!(world.source(id(RESUME_JSON))),
            FileError::NotSource
        ));
    }

    #[test]
    fn package_roots_are_refused_with_a_clear_message() {
        let style = style(&fixtures(), "minimal");
        let world = world(&style);
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
    fn served_sources_record_only_style_files_in_display_order() {
        let style = style(&fixtures(), "lib-import");
        let world = world(&style);
        assert_ok!(world.source(id("/lib/helper.typ")));
        assert_ok!(world.source(world.main()));
        assert_ok!(world.file(id(RESUME_JSON)));
        let names: Vec<String> = world
            .served_sources()
            .into_iter()
            .map(|(path, _)| path.file_name().unwrap_or_default().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["helper.typ", "style.typ"]);
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

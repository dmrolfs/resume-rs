//! ABOUTME: The public `StyleWorld` serves what the renderer serves, and compiling through it gives the renderer's PDF.
//! ABOUTME: Uses only the public API, as the desktop app does; the world's own unit tests stay beside its code.

use std::path::PathBuf;
use std::sync::Arc;

use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;
use resume::model::options::Paper;
use resume::model::{Resume, StyleName};
use resume::style::{self, StyleSource};
use resume::typeset::fonts::FontSet;
use resume::typeset::{DocumentKind, RenderOptions, StyleWorld, Typesetter, resume_json};
use typst::World as _;
use typst::syntax::{FileId, RootedPath, VirtualPath, VirtualRoot};

fn full_resume() -> Resume {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/full.md");
    let text = assert_ok!(std::fs::read_to_string(&path));
    let (resume, diagnostics) = resume::markdown::parse(&path, &text).into_parts();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    resume.expect("the full fixture yields a resume")
}

fn built_in(name: &str) -> StyleSource {
    assert_ok!(style::resolve(&assert_ok!(StyleName::try_new(name)), None))
}

fn fonts() -> Arc<FontSet> {
    Arc::new(FontSet::bundled())
}

fn world_of(style: &StyleSource) -> StyleWorld {
    let json = assert_ok!(resume_json(&full_resume(), false));
    StyleWorld::new(style.clone(), fonts(), json, Paper::Letter)
}

fn project_file(path: &str) -> FileId {
    RootedPath::new(VirtualRoot::Project, assert_ok!(VirtualPath::new(path))).intern()
}

/// The text of a built-in file, found by the path `builtin_files` lists it under.
fn embedded_text(style_name: &str, relative: &str) -> String {
    let name = assert_ok!(StyleName::try_new(style_name));
    let files = assert_ok!(style::builtin_files(&name));
    let (_, bytes) = files
        .iter()
        .find(|(path, _)| path.to_string_lossy().replace('\\', "/") == relative)
        .unwrap_or_else(|| panic!("{relative} is a built-in file"));
    String::from_utf8(bytes.to_vec()).expect("a style file is UTF-8")
}

#[test]
fn the_entry_file_of_every_built_in_style_is_the_one_the_build_reads() {
    let names: Vec<String> = assert_ok!(style::catalog(None)).into_iter().map(|e| e.name).collect();
    assert!(names.len() >= 2, "the built-in styles are listed: {names:?}");
    for name in names {
        let world = world_of(&built_in(&name));
        let main = world.main();
        assert_eq!(
            main.vpath().get_with_slash(),
            format!("/{name}/style.typ"),
            "the entry is inside the style's own directory"
        );
        let served = assert_ok!(world.source(main));
        assert_eq!(
            served.text(),
            embedded_text(&name, &format!("{name}/style.typ")),
            "{name}"
        );
    }
}

#[test]
fn the_shared_library_and_the_theme_are_served_as_the_build_serves_them() {
    let world = world_of(&built_in("pragmatic"));
    let components = assert_ok!(world.source(project_file("/lib/components.typ")));
    assert_eq!(components.text(), embedded_text("pragmatic", "lib/components.typ"));
    let theme = assert_ok!(world.source(project_file("/pragmatic/theme.typ")));
    assert_eq!(theme.text(), embedded_text("pragmatic", "pragmatic/theme.typ"));
}

#[test]
fn the_resume_payload_is_served_from_memory_as_json() {
    let world = world_of(&built_in("pragmatic"));
    let bytes = assert_ok!(world.file(project_file("/resume.json")));
    let value: serde_json::Value = assert_ok!(serde_json::from_slice(bytes.as_slice()));
    assert!(value.is_object(), "the payload is the resume's JSON");
    // A payload is data, not a source file.
    assert_err!(world.source(project_file("/resume.json")));
    // The other document's payload is not served to a resume.
    assert_err!(world.file(project_file("/letter.json")));
}

#[test]
fn a_letter_world_serves_the_letter_payload_and_the_letter_entry() {
    let style = built_in("pragmatic");
    let world = StyleWorld::for_kind(
        DocumentKind::Letter,
        style,
        fonts(),
        br#"{"salutation":"Dear team"}"#.to_vec(),
        Paper::Letter,
    );
    assert_eq!(world.main().vpath().get_with_slash(), "/pragmatic/letter.typ");
    assert_ok!(world.file(project_file("/letter.json")));
    assert_err!(world.file(project_file("/resume.json")));
}

#[test]
fn a_file_that_is_not_in_the_style_is_not_found_and_a_package_file_is_refused() {
    let world = world_of(&built_in("pragmatic"));
    assert_err!(world.source(project_file("/pragmatic/nothing-here.typ")));
    // A path that climbs out of the root cannot even be named: Typst refuses to build the id.
    assert_err!(VirtualPath::new("/../outside.typ"));
    let package = RootedPath::new(
        VirtualRoot::Package(assert_ok!("@preview/example:0.1.0".parse())),
        assert_ok!(VirtualPath::new("/lib.typ")),
    )
    .intern();
    let refused = assert_err!(world.source(package));
    assert!(refused.to_string().to_lowercase().contains("package"), "{refused}");
}

#[test]
fn compiling_through_a_world_built_outside_gives_the_same_pdf_as_the_build() {
    let style = built_in("pragmatic");
    let resume = full_resume();
    let typesetter = Typesetter::new(fonts());
    let built = assert_ok!(typesetter.compile(&resume, &style, RenderOptions::new(Paper::Letter)));
    let through_world = assert_ok!(Typesetter::compile_world(world_of(&style)));
    assert_eq!(built.page_count(), through_world.page_count());
    assert_eq!(
        assert_ok!(built.export_pdf("same")),
        assert_ok!(through_world.export_pdf("same")),
        "one world, one PDF"
    );
}

#[test]
fn a_world_for_another_paper_is_a_different_document() {
    let style = built_in("pragmatic");
    let json = || assert_ok!(resume_json(&full_resume(), false));
    let letter = StyleWorld::new(style.clone(), fonts(), json(), Paper::Letter);
    let a4 = StyleWorld::new(style, fonts(), json(), Paper::A4);
    let a = assert_ok!(Typesetter::compile_world(letter));
    let b = assert_ok!(Typesetter::compile_world(a4));
    assert_ne!(
        assert_ok!(a.export_pdf("p")),
        assert_ok!(b.export_pdf("p")),
        "paper reaches the style through sys.inputs"
    );
}

#[test]
fn display_path_names_style_files_and_hides_the_payload() {
    let world = world_of(&built_in("pragmatic"));
    assert!(world.display_path(project_file("/lib/components.typ")).is_some());
    assert!(world.display_path(project_file("/resume.json")).is_none());
}

#[test]
fn a_letter_world_on_a_style_without_a_letter_entry_names_the_missing_capability() {
    // The test styles directory holds a style with `style.typ` and no `letter.typ`.
    let name = assert_ok!(StyleName::try_new("minimal"));
    let style = assert_ok!(StyleSource::directory(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/styles"),
        name
    ));
    let world = StyleWorld::for_kind(DocumentKind::Letter, style, fonts(), b"{}".to_vec(), Paper::Letter);
    let error = match Typesetter::compile_world(world) {
        Err(error) => error,
        Ok(_) => panic!("a style with no letter entry cannot compile a letter"),
    };
    assert!(
        matches!(error, resume::error::Error::StyleMissingLetter { .. }),
        "{error}"
    );
}

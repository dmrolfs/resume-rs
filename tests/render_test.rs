//! ABOUTME: End-to-end tests of the Typst World: compile a style to PDF, map errors, enforce the sandbox.
//! ABOUTME: Styles are fixtures under tests/fixtures/styles; the resume is the synthetic full.md.

use std::path::PathBuf;
use std::sync::Arc;

use claims::{assert_err, assert_ok};
use lopdf::{Document, Object};
use pretty_assertions::assert_eq;
use resume::diagnostics::{Diagnostic, DiagnosticCode, Severity, SourceKind};
use resume::error::Error;
use resume::model::options::Paper;
use resume::model::{Resume, StyleName};
use resume::style::StyleSource;
use resume::typeset::fonts::FontSet;
use resume::typeset::{Compiled, RenderOptions, Typesetter};

fn crate_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn full_resume() -> Resume {
    let path = crate_path("tests/fixtures/full.md");
    let text = assert_ok!(std::fs::read_to_string(&path));
    let (resume, diagnostics) = resume::markdown::parse(&path, &text).into_parts();
    assert!(
        diagnostics.is_empty(),
        "the full fixture must parse cleanly: {diagnostics:?}"
    );
    resume.expect("the full fixture yields a resume")
}

fn style(name: &str) -> StyleSource {
    let name = assert_ok!(StyleName::try_new(name));
    assert_ok!(StyleSource::directory(crate_path("tests/fixtures/styles"), name))
}

fn typesetter() -> Typesetter {
    Typesetter::new(Arc::new(FontSet::bundled()))
}

fn compile_with(style_name: &str, paper: Paper) -> Result<Compiled, Error> {
    typesetter().compile(&full_resume(), &style(style_name), &RenderOptions { paper })
}

fn compile(style_name: &str) -> Result<Compiled, Error> {
    compile_with(style_name, Paper::Letter)
}

fn pdf_of(style_name: &str) -> Vec<u8> {
    assert_ok!(assert_ok!(compile(style_name)).export_pdf("fixture"))
}

/// The diagnostics inside a failed compile.
fn compile_errors(style_name: &str) -> Vec<Diagnostic> {
    match assert_err!(compile(style_name)) {
        Error::Compile { diagnostics, .. } => diagnostics,
        other => panic!("expected a compile failure, got {other:?}"),
    }
}

fn first_error_message(style_name: &str) -> String {
    let diagnostics = compile_errors(style_name);
    let error = diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .unwrap_or_else(|| panic!("no error diagnostic in {diagnostics:?}"));
    error.message.clone()
}

fn pdf_string(object: &Object) -> Option<String> {
    let bytes = object.as_str().ok()?;
    match bytes {
        [0xFE, 0xFF, rest @ ..] => {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
            String::from_utf16(&units).ok()
        }
        _ => Some(String::from_utf8_lossy(bytes).into_owned()),
    }
}

fn pdf_title(doc: &Document) -> Option<String> {
    let info = doc.trailer.get(b"Info").ok()?.as_reference().ok()?;
    pdf_string(doc.get_dictionary(info).ok()?.get(b"Title").ok()?)
}

fn media_box(doc: &Document) -> Vec<f32> {
    let (_, page_id) = doc.get_pages().into_iter().next().expect("one page");
    let page = assert_ok!(doc.get_dictionary(page_id));
    let array = assert_ok!(assert_ok!(page.get(b"MediaBox")).as_array());
    array.iter().map(|n| n.as_float().expect("number")).collect()
}

#[test]
fn compiles_minimal_style_to_pdf() {
    let compiled = assert_ok!(compile("minimal"));
    assert_eq!(compiled.page_count(), 1);
    let pdf = assert_ok!(compiled.export_pdf("fixture"));
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(pdf.trim_ascii_end().ends_with(b"%%EOF"));
    let doc = assert_ok!(Document::load_mem(&pdf));
    assert_eq!(doc.get_pages().len(), 1);
    assert_eq!(
        pdf_title(&doc).as_deref(),
        Some("Ada Lovelace"),
        "the style read /resume.json"
    );
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("+LibertinusSerif"),
        "an embedded subset of the requested font must be present"
    );
}

#[test]
fn paper_input_selects_page_size() {
    let letter = assert_ok!(Document::load_mem(&assert_ok!(
        assert_ok!(compile_with("minimal", Paper::Letter)).export_pdf("fixture")
    )));
    let a4 = assert_ok!(Document::load_mem(&assert_ok!(
        assert_ok!(compile_with("minimal", Paper::A4)).export_pdf("fixture")
    )));
    let (lw, lh) = (media_box(&letter)[2], media_box(&letter)[3]);
    let (aw, ah) = (media_box(&a4)[2], media_box(&a4)[3]);
    assert!(
        (lw - 612.0).abs() < 0.5 && (lh - 792.0).abs() < 0.5,
        "letter was {lw}x{lh}"
    );
    assert!((aw - 595.3).abs() < 0.5 && (ah - 841.9).abs() < 0.5, "a4 was {aw}x{ah}");
}

#[test]
fn root_relative_import_through_the_style_parent() {
    let compiled = assert_ok!(compile("lib-import"));
    assert_eq!(compiled.page_count(), 1);
    assert!(compiled.warnings().is_empty(), "{:?}", compiled.warnings());
}

#[test]
fn style_syntax_error_maps_to_style_file_line() {
    let diagnostics = compile_errors("syntax-error");
    let error = diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .expect("an error diagnostic");
    assert_eq!(error.code, DiagnosticCode::TypstError);
    let span = error.span.as_ref().expect("a span in the style file");
    assert_eq!(span.kind, SourceKind::Style);
    assert!(span.file.ends_with("syntax-error/style.typ"), "{:?}", span.file);
    let text = assert_ok!(std::fs::read_to_string(&span.file));
    let line = text[..span.range.start].matches('\n').count() + 1;
    assert_eq!(line, 4, "the unclosed `(` is on line 4: {error:?}");
}

#[test]
fn today_is_unavailable() {
    let message = first_error_message("today");
    assert!(message.contains("unable to get the current date"), "{message}");
}

#[test]
fn relative_path_escaping_the_root_is_denied() {
    let message = first_error_message("read-outside");
    assert!(message.contains("escape the project root"), "{message}");
}

#[test]
fn absolute_path_is_root_relative_and_never_reads_the_host() {
    let message = first_error_message("read-absolute");
    assert!(
        message.contains("not found") || message.contains("No such file"),
        "{message}"
    );
}

#[test]
fn package_import_is_refused_without_network() {
    let message = first_error_message("package");
    assert!(message.contains("packages are not supported"), "{message}");
}

#[test]
fn unknown_font_family_in_a_stack_warns() {
    let compiled = assert_ok!(compile("unknown-font"));
    let warning = compiled
        .warnings()
        .iter()
        .find(|d| d.code == DiagnosticCode::TypstWarning)
        .unwrap_or_else(|| panic!("no typst-warning in {:?}", compiled.warnings()));
    assert_eq!(warning.severity, Severity::Warning);
    assert!(
        warning.message.starts_with("unknown font family"),
        "{}",
        warning.message
    );
}

#[test]
fn compile_errors_carry_the_style_text_for_excerpts() {
    match assert_err!(compile("syntax-error")) {
        Error::Compile { sources, .. } => {
            assert!(
                sources
                    .iter()
                    .any(|(path, text)| path.ends_with("syntax-error/style.typ") && text.contains("broken"))
            );
        }
        other => panic!("expected a compile failure, got {other:?}"),
    }
}

#[test]
fn same_input_twice_is_byte_identical() {
    let a = pdf_of("minimal");
    let b = pdf_of("minimal");
    assert!(a.len() > 1000, "a vacuously small PDF proves nothing");
    assert!(a == b, "two renders of the same input differ");
}

#[test]
fn content_style_renders_every_node_kind_without_warnings() {
    let compiled = assert_ok!(compile("content"));
    assert!(compiled.warnings().is_empty(), "{:?}", compiled.warnings());
    assert!(compiled.page_count() >= 2, "the fixture has an explicit page break");
    let pdf = assert_ok!(compiled.export_pdf("fixture"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Link"), "link annotations must be present");
}

#[cfg(unix)]
#[test]
fn symlink_pointing_out_of_the_styles_directory_is_denied_end_to_end() {
    let tmp = assert_ok!(tempfile::tempdir());
    let secret = tmp.path().join("secret.txt");
    assert_ok!(std::fs::write(&secret, "do not leak"));
    let styles = tmp.path().join("styles");
    assert_ok!(std::fs::create_dir_all(styles.join("leaky")));
    assert_ok!(std::fs::write(
        styles.join("leaky/style.typ"),
        "#set page(paper: sys.inputs.paper)\n#read(\"leak.txt\")\n"
    ));
    assert_ok!(std::os::unix::fs::symlink(&secret, styles.join("leaky/leak.txt")));
    let source = assert_ok!(StyleSource::directory(&styles, assert_ok!(StyleName::try_new("leaky"))));
    let result = typesetter().compile(&full_resume(), &source, &RenderOptions::default());
    match assert_err!(result) {
        Error::Compile { diagnostics, .. } => {
            let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
            assert!(messages.iter().any(|m| m.contains("access denied")), "{messages:?}");
            assert!(!messages.iter().any(|m| m.contains("do not leak")));
        }
        other => panic!("expected a compile failure, got {other:?}"),
    }
}

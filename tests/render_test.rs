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

static EMBEDDED_STYLES: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/tests/fixtures/styles");

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
    typesetter().compile(&full_resume(), &style(style_name), RenderOptions::new(paper))
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
    assert_eq!(span.file, crate_path("tests/fixtures/styles/syntax-error/style.typ"));
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
            let expected = crate_path("tests/fixtures/styles/syntax-error/style.typ");
            assert!(
                sources.iter().any(|f| f.path == expected && f.text.contains("broken")),
                "{sources:?}"
            );
        }
        other => panic!("expected a compile failure, got {other:?}"),
    }
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
    let result = typesetter().compile(&full_resume(), &source, RenderOptions::default());
    match assert_err!(result) {
        Error::Compile { diagnostics, .. } => {
            let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
            assert!(messages.iter().any(|m| m.contains("access denied")), "{messages:?}");
            assert!(!messages.iter().any(|m| m.contains("do not leak")));
        }
        other => panic!("expected a compile failure, got {other:?}"),
    }
}

/// A styles directory with one style `s` whose `style.typ` has the given bytes.
fn temp_style(bytes: &[u8]) -> (tempfile::TempDir, StyleSource) {
    let tmp = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir(tmp.path().join("s")));
    assert_ok!(std::fs::write(tmp.path().join("s/style.typ"), bytes));
    let source = assert_ok!(StyleSource::directory(tmp.path(), assert_ok!(StyleName::try_new("s"))));
    (tmp, source)
}

fn pdf_with(style: &StyleSource) -> Vec<u8> {
    let compiled = assert_ok!(typesetter().compile(&full_resume(), style, RenderOptions::default()));
    assert_ok!(compiled.export_pdf("fixture"))
}

const SMALL_STYLE: &str =
    "#set page(paper: sys.inputs.paper, margin: 1cm)\n#set text(font: \"Libertinus Serif\")\nHello\n";

#[test]
fn a_utf8_bom_does_not_change_the_output() {
    let (_plain_dir, plain) = temp_style(SMALL_STYLE.as_bytes());
    let with_bom = [b"\xEF\xBB\xBF".as_slice(), SMALL_STYLE.as_bytes()].concat();
    let (_bom_dir, bom) = temp_style(&with_bom);
    assert!(pdf_with(&plain) == pdf_with(&bom), "a BOM changed the PDF");
}

#[test]
fn crlf_line_endings_do_not_change_the_output() {
    let (_lf_dir, lf) = temp_style(SMALL_STYLE.as_bytes());
    let (_crlf_dir, crlf) = temp_style(SMALL_STYLE.replace('\n', "\r\n").as_bytes());
    assert!(pdf_with(&lf) == pdf_with(&crlf), "CRLF changed the PDF");
}

#[test]
fn a_style_that_is_not_utf8_fails_naming_the_file() {
    let (_tmp, source) = temp_style(b"#let x = \xFF\xFE\n");
    match assert_err!(typesetter().compile(&full_resume(), &source, RenderOptions::default())) {
        Error::Compile { diagnostics, .. } => {
            let message = &diagnostics[0].message;
            assert!(
                message.contains("style.typ") && message.contains("not valid UTF-8"),
                "{message}"
            );
        }
        other => panic!("expected a compile failure, got {other:?}"),
    }
}

#[test]
fn an_error_in_a_shared_file_is_located_in_that_file() {
    let Error::Compile { diagnostics, sources } = assert_err!(compile("lib-error")) else {
        panic!("expected a compile failure");
    };
    let error = diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .expect("an error");
    let span = error.span.as_ref().expect("a span");
    assert_eq!(span.file, crate_path("tests/fixtures/styles/lib/broken.typ"));
    assert_eq!(error.message, "unclosed delimiter");
    let paths: Vec<_> = sources.iter().map(|f| f.path.clone()).collect();
    assert!(
        paths.contains(&crate_path("tests/fixtures/styles/lib/broken.typ")),
        "{paths:?}"
    );
    assert!(
        paths.contains(&crate_path("tests/fixtures/styles/lib-error/style.typ")),
        "{paths:?}"
    );
}

#[test]
fn a_failed_compile_reports_the_error_before_the_warnings() {
    let diagnostics = compile_errors("warn-then-error");
    let severities: Vec<Severity> = diagnostics.iter().map(|d| d.severity).collect();
    assert_eq!(severities.first(), Some(&Severity::Error), "{diagnostics:?}");
    assert!(
        diagnostics
            .last()
            .is_some_and(|d| d.code == DiagnosticCode::TypstWarning),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics[0].message.contains("deliberate failure"),
        "{}",
        diagnostics[0].message
    );
}

#[test]
fn a_successful_compile_keeps_the_style_text_for_later_diagnostics() {
    let compiled = assert_ok!(compile("lib-import"));
    let paths: Vec<_> = compiled.sources().iter().map(|f| f.path.clone()).collect();
    assert_eq!(
        paths,
        [
            crate_path("tests/fixtures/styles/lib-import/style.typ"),
            crate_path("tests/fixtures/styles/lib/helper.typ")
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
    );
    assert!(compiled.sources().iter().any(|f| f.text.contains("greeting")));
}

#[test]
fn the_requested_font_is_the_one_embedded() {
    let pdf = pdf_of("font-dejavu");
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("+DejaVuSansMono"), "DejaVu Sans Mono was not embedded");
    assert!(
        !text.contains("LibertinusSerif"),
        "the default font leaked into the output"
    );
}

#[test]
fn the_pdf_is_tagged_and_names_the_creator_without_dates() {
    let doc = assert_ok!(Document::load_mem(&pdf_of("minimal")));
    let root = assert_ok!(doc.trailer.get(b"Root"))
        .as_reference()
        .expect("a reference");
    let catalog = assert_ok!(doc.get_dictionary(root));
    assert!(catalog.get(b"StructTreeRoot").is_ok(), "the PDF is not tagged");
    let info = assert_ok!(assert_ok!(doc.trailer.get(b"Info")).as_reference());
    let creator = pdf_string(assert_ok!(assert_ok!(doc.get_dictionary(info)).get(b"Creator"))).expect("a creator");
    assert_eq!(creator, format!("resume-rs {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn an_embedded_style_renders_the_same_bytes_as_the_same_files_on_disk() {
    let name = assert_ok!(StyleName::try_new("lib-import"));
    let embedded = assert_ok!(StyleSource::embedded(&EMBEDDED_STYLES, name));
    let on_disk = style("lib-import");
    assert!(
        pdf_with(&embedded) == pdf_with(&on_disk),
        "embedded and directory styles differ"
    );
}

#[test]
fn errors_in_an_embedded_style_show_the_built_in_path() {
    let name = assert_ok!(StyleName::try_new("syntax-error"));
    let embedded = assert_ok!(StyleSource::embedded(&EMBEDDED_STYLES, name));
    match assert_err!(typesetter().compile(&full_resume(), &embedded, RenderOptions::default())) {
        Error::Compile { diagnostics, .. } => {
            let span = diagnostics[0].span.as_ref().expect("a span");
            assert_eq!(
                span.file,
                PathBuf::from("<built-in>").join("syntax-error").join("style.typ")
            );
        }
        other => panic!("expected a compile failure, got {other:?}"),
    }
}

#[test]
fn export_works_after_the_memoization_cache_is_evicted() {
    let compiled = assert_ok!(compile("minimal"));
    Typesetter::evict_cache(0);
    assert!(assert_ok!(compiled.export_pdf("fixture")).starts_with(b"%PDF-"));
}

// ---- the font-stack mechanism (rsm-141.7.1) -------------------------------------------------------

fn bundled_mode_typesetter() -> Typesetter {
    let set = assert_ok!(
        resume::typeset::fonts::FontSet::builder()
            .mode(resume::model::options::FontsMode::Bundled)
            .build()
    );
    Typesetter::new(Arc::new(set))
}

#[test]
fn a_style_sees_the_installed_families_as_a_lowercased_array_and_the_helper_behaves() {
    // The assertions live in the fixture; a failing one is a compile error shown by assert_ok.
    let compiled = assert_ok!(bundled_mode_typesetter().compile(
        &full_resume(),
        &style("font-stack-input"),
        RenderOptions::default()
    ));
    assert!(compiled.warnings().is_empty(), "{:?}", compiled.warnings());
}

#[test]
fn font_stack_keeps_only_installed_families_and_renders_without_warnings() {
    let compiled = assert_ok!(bundled_mode_typesetter().compile(
        &full_resume(),
        &style("font-stack-helper"),
        RenderOptions::default()
    ));
    assert!(
        compiled.warnings().is_empty(),
        "Calibri is not installed, but the stack must not name it: {:?}",
        compiled.warnings()
    );
}

#[test]
fn bundled_mode_embeds_the_bundled_fallback_and_never_an_installed_calibri() {
    let compiled = assert_ok!(bundled_mode_typesetter().compile(
        &full_resume(),
        &style("font-stack-helper"),
        RenderOptions::default()
    ));
    let pdf = assert_ok!(compiled.export_pdf("fixture"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("+Carlito"), "the bundled fallback was not used");
    assert!(
        !text.contains("Calibri"),
        "an installed Calibri leaked into a bundled-mode PDF"
    );
}

#[test]
fn font_stack_with_neither_family_installed_still_warns_so_the_problem_is_visible() {
    let compiled = assert_ok!(bundled_mode_typesetter().compile(
        &full_resume(),
        &style("font-stack-none"),
        RenderOptions::default()
    ));
    let warning = assert_some_warning(compiled.warnings(), "unknown font family: nada sans");
    assert_eq!(warning.code, DiagnosticCode::TypstWarning);
}

fn assert_some_warning<'a>(warnings: &'a [Diagnostic], message: &str) -> &'a Diagnostic {
    warnings
        .iter()
        .find(|d| d.message == message)
        .unwrap_or_else(|| panic!("no warning {message:?} in {warnings:?}"))
}

#[test]
fn font_stack_uses_the_preferred_family_when_it_is_installed() {
    // Libertinus Serif is in the bundled set, so it counts as installed and wins over the fallback.
    let compiled = assert_ok!(bundled_mode_typesetter().compile(
        &full_resume(),
        &style("font-stack-installed"),
        RenderOptions::default()
    ));
    assert!(compiled.warnings().is_empty(), "{:?}", compiled.warnings());
    let pdf = assert_ok!(compiled.export_pdf("fixture"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("+LibertinusSerif"),
        "the installed preferred family was not used"
    );
    assert!(
        !text.contains("+Carlito"),
        "the fallback was used although the preferred family is installed"
    );
}

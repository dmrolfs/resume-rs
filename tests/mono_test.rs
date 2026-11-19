//! ABOUTME: End-to-end tests of the built-in mono style on the full fixture, through the public API.
//! ABOUTME: Text order comes from lopdf extraction of the PDF; layout facts come from the probes.

use std::sync::{Arc, OnceLock};

use claims::{assert_ok, assert_some};
use lopdf::Document;
use pretty_assertions::assert_eq;
use resume::model::Resume;
use resume::model::options::{BuiltinStyle, Paper};
use resume::style::StyleSource;
use resume::typeset::fonts::{FontSet, Metrics};
use resume::typeset::probe::ProbeKind;
use resume::typeset::{Compiled, RenderOptions, Typesetter};

fn full_resume() -> Resume {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/full.md");
    let text = assert_ok!(std::fs::read_to_string(&path));
    let (resume, diagnostics) = resume::markdown::parse(&path, &text).into_parts();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_some!(resume)
}

fn bundled_fonts() -> Arc<FontSet> {
    static FONTS: OnceLock<Arc<FontSet>> = OnceLock::new();
    Arc::clone(FONTS.get_or_init(|| Arc::new(FontSet::bundled())))
}

fn mono(paper: Paper) -> Compiled {
    let style = assert_ok!(StyleSource::builtin(BuiltinStyle::Mono));
    assert_ok!(Typesetter::new(bundled_fonts()).compile(&full_resume(), &style, RenderOptions::new(paper)))
}

/// The text of each page in content-stream order, with runs of whitespace collapsed to one space.
fn page_texts(pdf: &[u8]) -> Vec<String> {
    let doc = assert_ok!(Document::load_mem(pdf));
    doc.get_pages()
        .keys()
        .map(|n| {
            let text = assert_ok!(doc.extract_text(&[*n]));
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .collect()
}

fn position(haystack: &str, needle: &str) -> usize {
    assert_some!(haystack.find(needle), "`{needle}` not in {haystack:?}")
}

#[test]
fn full_fixture_renders_in_mono_without_warnings() {
    for paper in [Paper::Letter, Paper::A4] {
        let compiled = mono(paper);
        assert!(compiled.warnings().is_empty(), "{:?}", compiled.warnings());
        assert!(compiled.page_count() >= 2, "the page-break block forces a page 2");
    }
}

#[test]
fn mono_text_order_title_then_company() {
    let pdf = assert_ok!(mono(Paper::Letter).export_pdf("fixture"));
    let text = page_texts(&pdf).remove(0);
    let at = |s| position(&text, s);
    assert!(at("Ada Lovelace") < at("London, UK"), "the name opens the text");
    assert!(
        at("Principal Engineer") < at("Analytical Engines Ltd"),
        "the title comes first"
    );
    assert!(at("Analytical Engines Ltd") < at("Aug 2017 – Present"));
    assert!(
        at("Aug 2017 – Present") < at("Builds mechanical computers"),
        "the blurb follows the heading"
    );
}

#[test]
fn section_headings_are_set_in_capitals() {
    let pdf = assert_ok!(mono(Paper::Letter).export_pdf("fixture"));
    let text = page_texts(&pdf).remove(0);
    for heading in ["SUMMARY", "EXPERIENCE"] {
        assert!(text.contains(heading), "{heading} not in {text}");
    }
    assert!(
        !text.contains("Summary"),
        "the section title is not also set as written"
    );
}

#[test]
fn the_contact_details_are_one_line_under_the_name() {
    let pdf = assert_ok!(mono(Paper::Letter).export_pdf("fixture"));
    let text = page_texts(&pdf).remove(0);
    let line = "London, UK · ada@example.com · Notes · GitHub";
    assert!(text.contains(line), "{text}");
    assert!(position(&text, "Ada Lovelace") < position(&text, line));
}

#[test]
fn footer_shows_name_and_page_of_total() {
    let compiled = mono(Paper::Letter);
    let total = compiled.page_count();
    let pages = page_texts(&assert_ok!(compiled.export_pdf("fixture")));
    for (i, page) in pages.iter().enumerate() {
        let footer = format!("Ada Lovelace Page {} of {total}", i + 1);
        assert!(page.ends_with(&footer), "{page}");
    }
}

#[test]
fn roles_and_eras_leave_probes_in_document_order() {
    let probes = assert_ok!(mono(Paper::Letter).probes());
    let kinds: Vec<_> = probes
        .iter()
        .map(|p| match &p.kind {
            ProbeKind::Role { index, company, .. } => format!("role {index} {company}"),
            ProbeKind::Era { role, index, name, .. } => format!("era {role}.{index} {name}"),
            _ => unreachable!("only roles and eras are probed"),
        })
        .collect();
    assert_eq!(kinds.len(), 7);
    assert_eq!(kinds[0], "role 0 Analytical Engines Ltd");
    assert!(probes.iter().all(|p| p.y > 0.0 && p.y < p.page_height));
}

#[test]
fn font_roles_prefer_calibri_and_fall_back_to_carlito() {
    let roles = assert_ok!(mono(Paper::Letter).font_roles());
    let body = assert_some!(roles.iter().find(|r| r.name == "body"));
    assert_eq!(
        (body.preferred.as_str(), body.fallback.as_str(), body.metrics),
        ("Calibri", "Carlito", Metrics::Compatible)
    );
}

#[test]
fn bundled_mode_embeds_only_bundled_families() {
    let pdf = assert_ok!(mono(Paper::Letter).export_pdf("fixture"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("Carlito"), "the fallback body font is embedded");
    assert!(
        !text.contains("Calibri"),
        "no installed font leaks into a bundled render"
    );
}

#[test]
fn the_built_in_mono_style_and_the_same_files_on_disk_render_the_same_bytes() {
    let embedded = assert_ok!(mono(Paper::Letter).export_pdf("fixture"));
    let on_disk_style = assert_ok!(StyleSource::directory(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("styles"),
        assert_ok!(resume::model::StyleName::try_new("mono"))
    ));
    let on_disk = assert_ok!(
        assert_ok!(Typesetter::new(bundled_fonts()).compile(
            &full_resume(),
            &on_disk_style,
            RenderOptions::new(Paper::Letter)
        ))
        .export_pdf("fixture")
    );
    assert!(embedded == on_disk, "the embedded copy differs from styles/ on disk");
}

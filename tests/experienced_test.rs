//! ABOUTME: End-to-end tests of the built-in experienced style on the full fixture, through the public API.
//! ABOUTME: Text order comes from lopdf extraction of the PDF; layout facts come from the probes.

mod common;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::diagnostics::DiagnosticCode;
use resume::model::options::{BuiltinStyle, FontsMode, Paper};
use resume::model::{Resume, StyleName};
use resume::style::StyleSource;
use resume::typeset::fonts::{FontResolution, FontSet, Metrics};
use resume::typeset::probe::ProbeKind;
use resume::typeset::{Compiled, RenderOptions, Typesetter};

use common::{bundled_fonts, full_resume, page_texts, position};

fn experienced_style() -> StyleSource {
    assert_ok!(StyleSource::builtin(BuiltinStyle::Experienced))
}

fn compile(resume: &Resume, paper: Paper) -> Compiled {
    assert_ok!(Typesetter::new(bundled_fonts()).compile(resume, &experienced_style(), RenderOptions::new(paper)))
}

fn experienced(paper: Paper) -> Compiled {
    compile(&full_resume(), paper)
}

fn parsed(markdown: &str) -> Resume {
    let (resume, _) = resume::markdown::parse("test.md", markdown).into_parts();
    assert_some!(resume)
}

fn first_page_text(compiled: &Compiled) -> String {
    page_texts(&assert_ok!(compiled.export_pdf("fixture"))).remove(0)
}

#[test]
fn full_fixture_renders_in_experienced_without_warnings() {
    for paper in [Paper::Letter, Paper::A4] {
        let compiled = experienced(paper);
        assert!(compiled.warnings().is_empty(), "{:?}", compiled.warnings());
        assert_eq!(compiled.page_count(), 2, "the page-break block forces page 2");
    }
}

#[test]
fn experienced_section_labels_precede_content() {
    // The label is in the gutter, but it comes first in the text, so a reader and a parser meet the label
    // before what it names.
    let text = first_page_text(&experienced(Paper::Letter));
    let at = |s| position(&text, s);
    assert!(at("Ada Lovelace") < at("SUMMARY"), "the name opens the text");
    assert!(at("SUMMARY") < at("Principal engineer who designs"));
    assert!(at("LANGUAGES AND TECHNOLOGIES") < at("Languages: Rust"));
    assert!(at("EXPERIENCE") < at("Analytical Engines Ltd, Principal Engineer"));
    assert!(
        at("Analytical Engines Ltd, Principal Engineer") < at("Aug 2017 – Present"),
        "company and title come before the period"
    );
    assert!(
        at("Aug 2017 – Present") < at("Builds mechanical computers"),
        "the blurb follows the heading"
    );
}

#[test]
fn experienced_skills_render_label_value_rows() {
    let text = first_page_text(&experienced(Paper::Letter));
    // A label, then its value, one row each, in document order.
    let rows = [
        "Languages: Rust, Java, Scala",
        "Cloud and infrastructure: AWS (EKS, EC2), Kubernetes",
        "VP of Engineering: Led all product development.",
    ];
    let mut last = 0;
    assert!(text.contains("Acme"), "the link in the last label is shown");
    for row in rows {
        let at = position(&text, row);
        assert!(at > last, "`{row}` is out of order in {text}");
        last = at;
    }
}

#[test]
fn a_page_after_the_first_keeps_the_gutter_and_the_footer_spans_the_page() {
    let compiled = experienced(Paper::Letter);
    let pdf = assert_ok!(compiled.export_pdf("fixture"));
    let pages = page_texts(&pdf);
    let total = compiled.page_count();
    assert!(
        pages[0].ends_with(&format!("Ada Lovelace Page 1 of {total}")),
        "{}",
        pages[0]
    );
    assert!(
        pages[1].contains("Consulting and Early-Stage Companies"),
        "the page break moves the third role to page 2: {}",
        pages[1]
    );
    let probes = assert_ok!(compiled.probes());
    let consulting = assert_some!(probes.iter().find(|p| matches!(
        &p.kind,
        ProbeKind::Role { company, .. } if company.starts_with("Consulting")
    )));
    assert_eq!(consulting.page.get(), 2);
}

#[test]
fn font_roles_prefer_book_antiqua_for_labels_and_say_the_fallback_has_other_metrics() {
    let roles = assert_ok!(experienced(Paper::Letter).font_roles());
    let described: Vec<_> = roles
        .iter()
        .map(|r| (r.name.as_str(), r.preferred.as_str(), r.fallback.as_str(), r.metrics))
        .collect();
    assert_eq!(
        described,
        [
            ("body", "Calibri", "Carlito", Metrics::Compatible),
            ("label", "Book Antiqua", "TeX Gyre Pagella", Metrics::Different),
            ("mono", "DejaVu Sans Mono", "DejaVu Sans Mono", Metrics::Compatible),
        ]
    );
}

#[test]
fn a_pagella_fallback_for_book_antiqua_is_reported_as_font_fallback_layout() {
    let compiled = experienced(Paper::Letter);
    let roles = assert_ok!(compiled.font_roles());
    let set = FontSet::bundled();
    let resolutions = set.resolve(&roles);
    let label = assert_some!(resolutions.iter().find(|r| r.role() == "label"));
    assert!(
        matches!(label, FontResolution::Fallback { family, metrics: Metrics::Different, .. } if family == "TeX Gyre Pagella"),
        "{label:?}"
    );
    let finding = assert_some!(label.diagnostic(FontsMode::Auto));
    assert_eq!(finding.code, DiagnosticCode::FontFallbackLayout);
    assert!(finding.message.contains("Book Antiqua"), "{}", finding.message);
    // Carlito for Calibri is only a note: its widths are the same.
    let body = assert_some!(resolutions.iter().find(|r| r.role() == "body"));
    assert_eq!(
        assert_some!(body.diagnostic(FontsMode::Auto)).code,
        DiagnosticCode::FontFallbackMetric
    );
}

#[test]
fn bundled_mode_embeds_pagella_for_the_labels_and_carlito_for_the_body() {
    let pdf = assert_ok!(experienced(Paper::Letter).export_pdf("fixture"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("TeXGyrePagella"), "the label fallback is embedded");
    assert!(text.contains("Carlito"), "the body fallback is embedded");
    assert!(
        !text.contains("Calibri") && !text.contains("BookAntiqua"),
        "no installed font leaks into a bundled render"
    );
}

#[test]
fn the_built_in_experienced_style_and_the_same_files_on_disk_render_the_same_bytes() {
    let embedded = assert_ok!(experienced(Paper::Letter).export_pdf("fixture"));
    let on_disk_style = assert_ok!(StyleSource::directory(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("styles"),
        assert_ok!(StyleName::try_new("experienced"))
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

#[test]
fn a_resume_without_a_contact_line_renders_the_name_then_the_first_section() {
    let compiled = compile(&parsed("# Ada Lovelace\n\n## Summary\n\n- one\n"), Paper::Letter);
    assert_eq!(compiled.page_count(), 1);
    let text = first_page_text(&compiled);
    assert!(position(&text, "Ada Lovelace") < position(&text, "SUMMARY"), "{text}");
}

#[test]
fn a_section_with_no_blocks_still_shows_its_label() {
    let compiled = compile(
        &parsed("# Ada Lovelace\n\nCity · a@b.co\n\n## Summary\n\n- one\n\n## Interests\n\n"),
        Paper::Letter,
    );
    let text = first_page_text(&compiled);
    assert!(text.contains("INTERESTS"), "{text}");
}

//! ABOUTME: Tests that measure real compiled documents: the page body, how far content reaches, and the layout report.
//! ABOUTME: They compile with the built-in pragmatic style and the bundled fonts, so the numbers are the shipped ones.

use std::path::PathBuf;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;

use super::geometry::PageGeometry;
use super::pragmatic_tests::{compile_lenient, fonts};
use super::{Compiled, RenderOptions, Typesetter};
use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::layout::LayoutReport;
use crate::model::StyleName;
use crate::model::options::Paper;
use crate::style::StyleSource;

const FULL: &str = include_str!("../../tests/fixtures/full.md");

/// One role whose first bullet is `words` words long, followed by a short bullet.
fn role_with_a_bullet(words: usize) -> String {
    format!(
        "# Ada Lovelace\n\nada@example.com\n\n## Experience\n\n### Acme · Engineer · 2020 – Present\n\n- {}\n- short after\n",
        "word ".repeat(words)
    )
}

fn report_of(markdown: &str) -> LayoutReport {
    assert_ok!(LayoutReport::of(&compile_lenient(markdown), &fonts(), None))
}

fn with_code(report: &LayoutReport, code: DiagnosticCode) -> Vec<&Diagnostic> {
    report.diagnostics.iter().filter(|d| d.code == code).collect()
}

// ============================================================================
// Page geometry
// ============================================================================

#[test]
fn the_page_body_is_measured_from_the_frames() {
    let pages = compile_lenient(FULL).geometry();
    assert!(
        pages.len() >= 2,
        "the fixture is meant to run onto a second page: {pages:?}"
    );
    for page in &pages {
        assert!(page.body_top > 0.0 && page.body_top < 100.0, "{page:?}");
        assert!(
            page.body_bottom < page.height - 10.0,
            "the footer was not found, so the body runs to the page edge: {page:?}"
        );
        assert!(page.body_top < page.body_bottom, "{page:?}");
    }
    assert!(
        pages.iter().all(|p| p.body_top == pages[0].body_top),
        "the body top is one number for the document: {pages:?}"
    );
}

#[test]
fn the_footer_is_an_artifact_and_not_body_content() {
    // The footer's baseline lies below the footer's own top, which is where the body ends: had it been
    // counted as content, every page would look overflowing.
    let pages = compile_lenient(FULL).geometry();
    assert!(!pages.is_empty(), "no pages were measured");
    for page in pages {
        let content = assert_some!(page.content_bottom, "{page:?}");
        assert!(
            content < page.body_bottom,
            "the footer counted as body content: {page:?}"
        );
    }
}

#[test]
fn a_bullet_taller_than_a_page_is_measured_past_the_body() {
    let pages = compile_lenient(&role_with_a_bullet(2000)).geometry();
    let overflowing: Vec<usize> = pages
        .iter()
        .enumerate()
        .filter(|(_, p)| p.content_bottom.is_some_and(|c| c > p.body_bottom + 100.0))
        .map(|(i, _)| i + 1)
        .collect();
    assert_eq!(overflowing.len(), 1, "exactly one page overflows: {pages:?}");
}

// ============================================================================
// The layout report of a real document
// ============================================================================

#[test]
fn a_bullet_taller_than_a_page_is_one_error_naming_the_role() {
    let report = report_of(&role_with_a_bullet(2000));
    let found = with_code(&report, DiagnosticCode::ContentOverflowsPage);
    assert_eq!(found.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(found[0].severity, Severity::Error);
    assert!(
        found[0].hints.iter().any(|h| h.contains("Acme")),
        "{:?}",
        found[0].hints
    );
}

#[test]
fn a_long_bullet_that_fits_is_not_an_overflow() {
    let report = report_of(&role_with_a_bullet(400));
    assert_eq!(
        with_code(&report, DiagnosticCode::ContentOverflowsPage).len(),
        0,
        "{:?}",
        report.diagnostics
    );
}

#[test]
fn the_full_fixture_has_a_sparse_last_page() {
    let report = report_of(FULL);
    let found = with_code(&report, DiagnosticCode::SparseLastPage);
    assert_eq!(found.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(found[0].severity, Severity::Warning);
}

#[test]
fn every_role_of_the_full_fixture_is_placed_on_a_page() {
    let report = report_of(FULL);
    let roles = FULL.lines().filter(|l| l.starts_with("### ")).count();
    assert_eq!(report.roles.len(), roles, "{:?}", report.roles);
    for placement in &report.roles {
        assert!(placement.first_page <= placement.last_page, "{placement:?}");
        assert!(placement.last_page.get() <= report.pages, "{placement:?}");
    }
    let last = assert_some!(report.roles.last());
    assert_eq!(last.last_page.get(), report.pages);
}

// ============================================================================
// Styles that lay content out in other ways than the shipped one
// ============================================================================

/// Compiles a tiny resume with the fixture style `name`; each one draws a single kind of thing.
fn compile_fixture(name: &str) -> Compiled {
    let (resume, diagnostics) =
        crate::markdown::parse("fixture.md", "# Ada Lovelace\n\nada@example.com\n").into_parts();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/styles");
    let style = assert_ok!(StyleSource::directory(root, assert_ok!(StyleName::try_new(name))));
    assert_ok!(Typesetter::new(fonts()).compile(&assert_some!(resume), &style, RenderOptions::new(Paper::Letter)))
}

/// The geometry of the single page a fixture style produces.
fn only_page(name: &str) -> PageGeometry {
    let pages = compile_fixture(name).geometry();
    assert_eq!(pages.len(), 1, "{name}: {pages:?}");
    pages[0]
}

#[test]
fn a_shape_taller_than_the_page_is_measured_to_its_bottom() {
    // A 900 pt rectangle below a 28 pt margin ends at 928 pt on a 792 pt page.
    let page = only_page("geometry-shape");
    let bottom = assert_some!(page.content_bottom, "{page:?}");
    assert!((bottom - 928.35).abs() < 0.1, "{page:?}");
}

#[test]
fn an_image_taller_than_the_page_is_measured_to_its_bottom() {
    let page = only_page("geometry-image");
    let bottom = assert_some!(page.content_bottom, "{page:?}");
    assert!((bottom - 928.35).abs() < 0.1, "{page:?}");
}

#[test]
fn a_shape_or_an_image_that_runs_off_the_page_is_an_overflow() {
    for name in ["geometry-shape", "geometry-image"] {
        let report = assert_ok!(LayoutReport::of(&compile_fixture(name), &fonts(), None));
        let found = with_code(&report, DiagnosticCode::ContentOverflowsPage);
        assert_eq!(found.len(), 1, "{name}: {:?}", report.diagnostics);
    }
}

#[test]
fn a_header_and_a_footer_of_plain_text_are_artifacts_and_not_body_content() {
    // Typst inlines a marginal made of plain text into the page frame, with no group around it, so only its
    // start and end tags say it is an artifact.
    let page = only_page("geometry-header");
    let content = assert_some!(page.content_bottom, "{page:?}");
    assert!(content < 100.0, "the footer counted as body content: {page:?}");
    assert!(
        page.body_top > 25.0 && page.body_top < 50.0,
        "the header counted as body: {page:?}"
    );
    assert!(
        page.body_bottom < page.height - 5.0,
        "the footer was not found: {page:?}"
    );
}

#[test]
fn content_moved_below_the_page_is_measured_where_it_lands() {
    let page = only_page("geometry-moved");
    let content = assert_some!(page.content_bottom, "{page:?}");
    assert!(content > 900.0, "the move was not applied: {page:?}");
    assert!(page.body_top > 25.0 && page.body_top < 50.0, "{page:?}");
}

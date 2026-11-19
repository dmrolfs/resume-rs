//! ABOUTME: End-to-end tests of the built-in pragmatic style on the full fixture, through the public API.
//! ABOUTME: Text order comes from lopdf extraction of the PDF; layout facts come from the probes.

mod common;

use std::sync::Arc;

use claims::{assert_ok, assert_some};
use lopdf::Document;
use pretty_assertions::assert_eq;
use resume::error::Error;
use resume::model::StyleName;
use resume::model::options::{BuiltinStyle, Paper};
use resume::style::StyleSource;
use resume::typeset::fonts::{FontSet, Metrics};
use resume::typeset::probe::ProbeKind;
use resume::typeset::{Compiled, RenderOptions, Typesetter};

use common::{bundled_fonts, full_resume, page_texts, position};

fn compile_with(style: &StyleSource, paper: Paper) -> Compiled {
    assert_ok!(Typesetter::new(bundled_fonts()).compile(&full_resume(), style, RenderOptions::new(paper)))
}

fn pragmatic(paper: Paper) -> Compiled {
    compile_with(&assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic)), paper)
}

/// A style under `tests/fixtures/styles`.
fn fixture_style(name: &str) -> StyleSource {
    assert_ok!(StyleSource::directory(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/styles"),
        assert_ok!(StyleName::try_new(name))
    ))
}

fn compiled_with(style: &StyleSource) -> Compiled {
    compile_with(style, Paper::Letter)
}

#[test]
fn full_fixture_renders_in_pragmatic_without_warnings() {
    for paper in [Paper::Letter, Paper::A4] {
        let compiled = pragmatic(paper);
        assert!(compiled.warnings().is_empty(), "{:?}", compiled.warnings());
        assert_eq!(compiled.page_count(), 2, "the page-break block forces page 2");
    }
}

#[test]
fn pragmatic_text_order_name_then_title_company_period() {
    let pdf = assert_ok!(pragmatic(Paper::Letter).export_pdf("fixture"));
    let text = page_texts(&pdf).remove(0);
    let at = |s| position(&text, s);
    assert!(at("Ada Lovelace") < at("London, UK"), "the name opens the text");
    assert!(at("Principal Engineer") < at("Analytical Engines Ltd"));
    assert!(at("Analytical Engines Ltd") < at("Aug 2017 – Present"));
    assert!(
        at("Aug 2017 – Present") < at("Builds mechanical computers"),
        "the blurb follows the heading"
    );
}

#[test]
fn footer_shows_name_and_page_of_total() {
    let pdf = assert_ok!(pragmatic(Paper::Letter).export_pdf("fixture"));
    let pages = page_texts(&pdf);
    assert!(pages[0].ends_with("Ada Lovelace Page 1 of 2"), "{}", pages[0]);
    assert!(pages[1].ends_with("Ada Lovelace Page 2 of 2"), "{}", pages[1]);
}

#[test]
fn pdf_is_tagged() {
    let pdf = assert_ok!(pragmatic(Paper::Letter).export_pdf("fixture"));
    let doc = assert_ok!(Document::load_mem(&pdf));
    assert_ok!(assert_ok!(doc.catalog()).get(b"StructTreeRoot"));
}

#[test]
fn theme_fonts_metadata_is_queryable() {
    let roles = assert_ok!(pragmatic(Paper::Letter).font_roles());
    let described: Vec<_> = roles
        .iter()
        .map(|r| (r.name.as_str(), r.preferred.as_str(), r.fallback.as_str(), r.metrics))
        .collect();
    assert_eq!(
        described,
        [
            ("body", "Calibri", "Carlito", Metrics::Compatible),
            ("display", "Calibri", "Carlito", Metrics::Compatible),
            ("mono", "DejaVu Sans Mono", "DejaVu Sans Mono", Metrics::Compatible),
        ]
    );
}

#[test]
fn a_role_without_metric_compatibility_is_reported_as_different() {
    let roles = assert_ok!(compiled_with(&fixture_style("different-metrics")).font_roles());
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].metrics, Metrics::Different);
}

#[test]
fn every_role_and_era_leaves_a_probe_in_document_order() {
    let probes = assert_ok!(pragmatic(Paper::Letter).probes());
    let kinds: Vec<_> = probes
        .iter()
        .map(|p| match &p.kind {
            ProbeKind::Role {
                index,
                company,
                title,
                period,
                ..
            } => format!("role {index} {company} / {title:?} / {period}"),
            ProbeKind::Era {
                role,
                index,
                name,
                period,
                ..
            } => format!("era {role}.{index} {name} / {period}"),
            _ => unreachable!("only roles and eras are probed"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "role 0 Analytical Engines Ltd / Some(\"Principal Engineer\") / Aug 2017 – Present",
            "era 0.0 Programming languages / 2026 – Present",
            "era 0.1 Early machines / Spring 2020 – Winter 2021",
            "era 0.2 Infrastructure / 2022 – 2023",
            "role 1 Babbage & Co / None / Fall 2012 – Sept 2017",
            "role 2 Consulting and Early-Stage Companies / None / 2008 – 2012",
            "role 3 University of London / None / 2004 – 2008",
        ]
    );
    assert!(probes.iter().all(|p| p.y > 0.0 && p.y < p.page_height));
}

#[test]
fn the_page_break_block_moves_the_third_role_to_page_two() {
    let probes = assert_ok!(pragmatic(Paper::Letter).probes());
    let consulting = assert_some!(probes.iter().find(|p| matches!(
        &p.kind,
        ProbeKind::Role { company, .. } if company.starts_with("Consulting")
    )));
    assert_eq!(consulting.page.get(), 2);
}

#[test]
fn probes_report_the_height_of_the_paper_they_are_on() {
    let height = |paper| assert_some!(assert_ok!(pragmatic(paper).probes()).first().map(|p| p.page_height));
    assert!((height(Paper::Letter) - 792.0).abs() < 0.01);
    assert!((height(Paper::A4) - 841.89).abs() < 0.01);
}

#[test]
fn bullet_labels_come_before_their_text() {
    let pdf = assert_ok!(pragmatic(Paper::Letter).export_pdf("fixture"));
    let text = page_texts(&pdf).join(" ");
    assert!(text.contains("Languages: Rust, Java, Scala"), "{text}");
    assert!(text.contains("Korrio, Director: Led a youth-sports platform"), "{text}");
}

#[test]
fn inline_code_is_set_in_the_mono_font() {
    let pdf = assert_ok!(pragmatic(Paper::Letter).export_pdf("fixture"));
    assert!(
        String::from_utf8_lossy(&pdf).contains("DejaVuSansMono"),
        "the mono face is embedded"
    );
}

#[test]
fn bundled_mode_embeds_only_bundled_families() {
    let pdf = assert_ok!(pragmatic(Paper::Letter).export_pdf("fixture"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("Carlito"), "the fallback body font is embedded");
    assert!(
        !text.contains("Calibri"),
        "no installed font leaks into a bundled render"
    );
}

// ---- embedding and the contract between the Rust and Typst sides -------------------------------

fn typst_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn the_built_in_pragmatic_style_and_the_same_files_on_disk_render_the_same_bytes() {
    let embedded = assert_ok!(pragmatic(Paper::Letter).export_pdf("fixture"));
    let on_disk_style = assert_ok!(StyleSource::directory(
        typst_dir().join("styles"),
        assert_ok!(StyleName::try_new("pragmatic"))
    ));
    let on_disk = assert_ok!(
        assert_ok!(Typesetter::new(Arc::new(FontSet::bundled())).compile(
            &full_resume(),
            &on_disk_style,
            RenderOptions::new(Paper::Letter)
        ))
        .export_pdf("fixture")
    );
    assert!(embedded == on_disk, "the embedded copy differs from styles/ on disk");
}

#[test]
fn an_edited_copy_of_the_theme_renders_differently() {
    let tmp = assert_ok!(tempfile::tempdir());
    for relative in ["lib/components.typ", "pragmatic/style.typ", "pragmatic/theme.typ"] {
        let target = tmp.path().join(relative);
        assert_ok!(std::fs::create_dir_all(assert_some!(target.parent())));
        let text = assert_ok!(std::fs::read_to_string(typst_dir().join("styles").join(relative)));
        assert_ok!(std::fs::write(target, text.replace("#1155cc", "#cc0000")));
    }
    let style = assert_ok!(StyleSource::directory(
        tmp.path(),
        assert_ok!(StyleName::try_new("pragmatic"))
    ));
    let edited = assert_ok!(
        assert_ok!(Typesetter::new(Arc::new(FontSet::bundled())).compile(
            &full_resume(),
            &style,
            RenderOptions::new(Paper::Letter)
        ))
        .export_pdf("fixture")
    );
    assert!(edited != assert_ok!(pragmatic(Paper::Letter).export_pdf("fixture")));
}

#[test]
fn a_style_that_is_named_but_not_written_yet_is_unknown_not_a_panic() {
    let err = StyleSource::builtin(BuiltinStyle::Experienced).expect_err("rsm-141.9 writes this");
    assert!(matches!(err, Error::UnknownStyle { .. }), "{err:?}");
}

#[test]
fn malformed_font_metadata_in_a_user_style_is_a_typed_error_not_a_panic() {
    let compiled = compiled_with(&fixture_style("bad-probe"));
    let err = compiled
        .font_roles()
        .expect_err("the metadata is not a dictionary of roles");
    assert!(
        matches!(
            err,
            Error::Probe {
                label: "resume-fonts",
                ..
            }
        ),
        "{err:?}"
    );
    assert!(err.to_string().contains("resume-fonts"), "{err}");
}

#[test]
fn malformed_role_metadata_is_a_typed_error_naming_the_field() {
    for (fixture, field) in [("bad-probe-title", "title"), ("bad-probe-index", "index")] {
        let err = compiled_with(&fixture_style(fixture))
            .probes()
            .expect_err("the role metadata is malformed");
        assert!(
            matches!(
                err,
                Error::Probe {
                    label: "resume-role",
                    ..
                }
            ),
            "{fixture}: {err:?}"
        );
        assert!(err.to_string().contains(field), "{fixture}: {err}");
    }
}

#[test]
fn a_style_without_probes_has_no_font_roles_and_no_probes() {
    let compiled = compiled_with(&fixture_style("minimal"));
    assert!(assert_ok!(compiled.font_roles()).is_empty());
    assert!(assert_ok!(compiled.probes()).is_empty());
}

#[test]
fn the_font_stack_helper_in_the_fixtures_is_the_one_components_ships() {
    // Code lines only: the comments differ. The block in components.typ sits between font-stack markers.
    fn code(text: &str) -> Vec<String> {
        text.lines()
            .map(str::trim_end)
            .filter(|l| !l.trim_start().starts_with("//") && !l.trim().is_empty())
            .map(str::to_string)
            .collect()
    }
    let components = assert_ok!(std::fs::read_to_string(typst_dir().join("styles/lib/components.typ")));
    let shipped = assert_some!(components.split("// font-stack:begin").nth(1));
    let shipped = assert_some!(shipped.split("// font-stack:end").next());
    let shipped = assert_some!(shipped.split_once('\n')).1; // the rest of the begin line is a comment
    let fixture = assert_ok!(std::fs::read_to_string(
        typst_dir().join("tests/fixtures/styles/lib/font-stack.typ")
    ));
    assert_eq!(code(shipped), code(&fixture));
}

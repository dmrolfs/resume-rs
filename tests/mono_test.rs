//! ABOUTME: End-to-end tests of the built-in mono style on the full fixture, through the public API.
//! ABOUTME: Text order comes from lopdf extraction of the PDF; layout facts come from the probes.

mod common;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::model::options::{BuiltinStyle, Paper};
use resume::style::StyleSource;
use resume::typeset::fonts::Metrics;
use resume::typeset::probe::ProbeKind;
use resume::typeset::{Compiled, RenderOptions, Typesetter};

use common::{bundled_fonts, full_resume, page_texts, position};

fn mono(paper: Paper) -> Compiled {
    let style = assert_ok!(StyleSource::builtin(BuiltinStyle::Mono));
    assert_ok!(Typesetter::new(bundled_fonts()).compile(&full_resume(), &style, RenderOptions::new(paper)))
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
    for paper in [Paper::Letter, Paper::A4] {
        let pdf = assert_ok!(mono(paper).export_pdf("fixture"));
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
        assert!(
            at("Programming languages") < at("2026 – Present"),
            "an era's name comes before its period"
        );
    }
}

#[test]
fn section_headings_are_set_in_capitals() {
    let compiled = mono(Paper::Letter);
    let text = page_texts(&assert_ok!(compiled.export_pdf("fixture"))).join(" ");
    let titles = ["Summary", "Languages and Technologies", "Experience", "Education"];
    for title in titles {
        assert!(
            text.contains(&title.to_uppercase()),
            "{title} is not capitalized in {text}"
        );
        assert!(!text.contains(title), "`{title}` is also set as written in {text}");
    }
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
fn font_roles_prefer_calibri_and_fall_back_to_carlito() {
    let roles = assert_ok!(mono(Paper::Letter).font_roles());
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

#[test]
fn an_edited_copy_of_the_mono_theme_renders_differently() {
    // `header.gap` and `tracking.heading` are mono's own keys: each must change the output.
    let reference = assert_ok!(mono(Paper::Letter).export_pdf("fixture"));
    for (from, to) in [
        ("header: (gap: 4pt)", "header: (gap: 20pt)"),
        ("heading: 0.6pt", "heading: 3pt"),
    ] {
        let tmp = assert_ok!(tempfile::tempdir());
        for relative in ["lib/components.typ", "mono/style.typ", "mono/theme.typ"] {
            let target = tmp.path().join(relative);
            assert_ok!(std::fs::create_dir_all(assert_some!(target.parent())));
            let text = assert_ok!(std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("styles")
                    .join(relative)
            ));
            assert_ok!(std::fs::write(target, text.replace(from, to)));
        }
        let style = assert_ok!(StyleSource::directory(
            tmp.path(),
            assert_ok!(resume::model::StyleName::try_new("mono"))
        ));
        let edited = assert_ok!(
            assert_ok!(Typesetter::new(bundled_fonts()).compile(
                &full_resume(),
                &style,
                RenderOptions::new(Paper::Letter)
            ))
            .export_pdf("fixture")
        );
        assert!(edited != reference, "editing `{from}` changed nothing");
    }
}

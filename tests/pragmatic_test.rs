//! ABOUTME: End-to-end tests of the built-in pragmatic style on the full fixture, through the public API.
//! ABOUTME: Text order comes from lopdf extraction of the PDF; layout facts come from the probes.

use std::sync::Arc;

use claims::{assert_ok, assert_some};
use lopdf::Document;
use pretty_assertions::assert_eq;
use resume::model::Resume;
use resume::model::options::{BuiltinStyle, Paper};
use resume::style::StyleSource;
use resume::typeset::fonts::FontSet;
use resume::typeset::probe::ProbeKind;
use resume::typeset::{Compiled, RenderOptions, Typesetter};

fn full_resume() -> Resume {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/full.md");
    let text = assert_ok!(std::fs::read_to_string(&path));
    let (resume, diagnostics) = resume::markdown::parse(&path, &text).into_parts();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_some!(resume)
}

fn pragmatic(paper: Paper) -> Compiled {
    let style = assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic));
    let typesetter = Typesetter::new(Arc::new(FontSet::bundled()));
    assert_ok!(typesetter.compile(&full_resume(), &style, RenderOptions::new(paper)))
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
        .map(|r| (r.name.as_str(), r.preferred.as_str(), r.fallback.as_str()))
        .collect();
    assert_eq!(
        described,
        [
            ("body", "Calibri", "Carlito"),
            ("display", "Calibri", "Carlito"),
            ("mono", "DejaVu Sans Mono", "DejaVu Sans Mono"),
        ]
    );
}

#[test]
fn every_role_and_era_leaves_a_probe_in_document_order() {
    let probes = assert_ok!(pragmatic(Paper::Letter).probes());
    let kinds: Vec<_> = probes
        .iter()
        .map(|p| match &p.kind {
            ProbeKind::Role { index, company, .. } => format!("role {index} {company}"),
            ProbeKind::Era { role, index, name } => format!("era {role}.{index} {name}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "role 0 Analytical Engines Ltd",
            "era 0.0 Programming languages",
            "era 0.1 Early machines",
            "era 0.2 Infrastructure",
            "role 1 Babbage & Co",
            "role 2 Consulting and Early-Stage Companies",
            "role 3 University of London",
        ]
    );
    assert!(probes.iter().all(|p| p.y > 0.0 && p.y < p.page_height));
    assert_eq!(probes[5].page.get(), 2, "the page-break block moves role 2 to page 2");
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

use resume::error::Error;
use resume::model::StyleName;

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
fn styles_that_are_named_but_not_written_yet_are_unknown_not_a_panic() {
    for style in [BuiltinStyle::Mono, BuiltinStyle::Experienced] {
        let err = StyleSource::builtin(style).expect_err("rsm-141.8 and rsm-141.9 write these");
        assert!(matches!(err, Error::UnknownStyle { .. }), "{err:?}");
    }
}

#[test]
fn every_file_under_styles_is_embedded_in_the_binary() {
    // include_dir does not notice a file added later; build.rs makes cargo re-embed, and this catches a miss.
    fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("styles/ is readable").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                let relative = path.strip_prefix(base).expect("under styles/");
                out.push(
                    relative
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
    }
    let mut on_disk = Vec::new();
    walk(&typst_dir().join("styles"), &typst_dir().join("styles"), &mut on_disk);
    on_disk.sort();
    assert_eq!(
        on_disk,
        [
            "lib/components.typ",
            "pragmatic/README.md",
            "pragmatic/style.typ",
            "pragmatic/theme.typ"
        ]
    );
    let style = assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic));
    for relative in &on_disk {
        let vpath = assert_ok!(typst::syntax::VirtualPath::new(format!("/{relative}")));
        assert!(style.reads(&vpath), "{relative} is not embedded");
    }
}

#[test]
fn malformed_font_metadata_in_a_user_style_is_a_typed_error_not_a_panic() {
    let style = assert_ok!(StyleSource::directory(
        typst_dir().join("tests/fixtures/styles"),
        assert_ok!(StyleName::try_new("bad-probe"))
    ));
    let compiled = assert_ok!(Typesetter::new(Arc::new(FontSet::bundled())).compile(
        &full_resume(),
        &style,
        RenderOptions::new(Paper::Letter)
    ));
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
fn a_style_without_probes_has_no_font_roles_and_no_probes() {
    let style = assert_ok!(StyleSource::directory(
        typst_dir().join("tests/fixtures/styles"),
        assert_ok!(StyleName::try_new("minimal"))
    ));
    let compiled = assert_ok!(Typesetter::new(Arc::new(FontSet::bundled())).compile(
        &full_resume(),
        &style,
        RenderOptions::new(Paper::Letter)
    ));
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

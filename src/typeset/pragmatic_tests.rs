//! ABOUTME: Pagination guarantees of the shared components, checked on generated resumes whose page breaks sweep.
//! ABOUTME: Lives inside the crate to read frames directly; the check is proven able to fail by a style without the guarantees.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use typst::layout::{Frame, FrameItem, Point};

use super::*;
use crate::model::StyleName;
use crate::model::options::{BuiltinStyle, FontOrigin, FontsMode};

/// Everything below this distance from the page bottom is footer, not body.
const FOOTER_BAND_PT: f64 = 52.0;

/// Resumes per spacer count in a sweep, and the spacer counts swept: enough that a page break lands at every
/// offset in a role, short enough to run in seconds in a debug build.
const SEEDS: u64 = 2;
const SWEEP: std::ops::Range<usize> = 0..40;

fn text_runs(frame: &Frame, origin: Point, out: &mut Vec<(f64, f64, String)>) {
    for (position, item) in frame.items() {
        let at = origin + *position;
        match item {
            FrameItem::Group(group) => text_runs(
                &group.frame,
                Point::new(at.x + group.transform.tx, at.y + group.transform.ty),
                out,
            ),
            FrameItem::Text(run) => out.push((at.y.to_pt(), at.x.to_pt(), run.text.to_string())),
            _ => {}
        }
    }
}

/// Per page, the body lines.
type Lines = Vec<Vec<(i64, String)>>;

/// The body lines of every page, top to bottom: baseline in hundredths of a point, and the joined text.
fn body_lines(compiled: &Compiled) -> Lines {
    compiled
        .document
        .pages()
        .iter()
        .map(|page| {
            let mut runs = Vec::new();
            text_runs(&page.frame, Point::zero(), &mut runs);
            runs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
            let limit = page.frame.height().to_pt() - FOOTER_BAND_PT;
            let mut lines: Vec<(f64, String)> = Vec::new();
            for (y, _, text) in runs.into_iter().filter(|(y, ..)| *y < limit) {
                match lines.last_mut() {
                    Some((last_y, line)) if (y - *last_y).abs() < 0.3 => line.push_str(&text),
                    _ => lines.push((y, text)),
                }
            }
            lines
                .into_iter()
                .map(|(y, line)| ((y * 100.0).round() as i64, line))
                .collect()
        })
        .collect()
}

/// A resume with `spacers` one-line paragraphs ahead of seven roles, so the page break lands elsewhere for each count.
fn markdown(spacers: usize, seed: u64) -> String {
    let mut state = seed;
    let mut next = move |modulus: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % modulus
    };
    let words = |n: u64, next: &mut dyn FnMut(u64) -> u64| {
        (0..n)
            .map(|_| {
                [
                    "delivered",
                    "platform",
                    "latency",
                    "of",
                    "teams",
                    "across",
                    "the",
                    "migration",
                ][next(8) as usize]
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut md = String::from("# Test Person\n\nCity, ST · 555-123-4567 · a@b.co\n\n## Summary\n\n");
    for i in 0..spacers {
        md += &format!("Spacer {i}.\n\n");
    }
    md += "## Experience\n\n";
    let mut bullet = 0;
    for r in 0..7 {
        md += &format!(
            "### ZR{r} · Title{r} · 20{:02} – 20{:02}\n\nZB{r} blurb line.\n\n",
            20 - r,
            21 - r
        );
        for e in 0..next(3) {
            md += &format!("**ZE{r}x{e} · 20{:02} – 20{:02}**\n\n", 20 - r, 21 - r);
            for _ in 0..=next(3) {
                let n = 4 + next(50);
                md += &format!("- B{bullet} {} end{bullet}\n", words(n, &mut next));
                bullet += 1;
            }
            md += "\n";
        }
        for _ in 0..=next(3) {
            let n = 4 + next(50);
            md += &format!("- B{bullet} {} end{bullet}\n", words(n, &mut next));
            bullet += 1;
        }
        md += "\n";
    }
    md + "## Education\n\n- B999 last end999\n"
}

fn compile(style: &StyleSource, md: &str) -> Compiled {
    let (resume, diagnostics) = crate::markdown::parse("sweep.md", md).into_parts();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let typesetter = Typesetter::new(Arc::new(FontSet::bundled()));
    assert_ok!(typesetter.compile(&assert_some!(resume), style, RenderOptions::new(Paper::Letter)))
}

/// Counts of each kind of broken guarantee over a sweep of page-break positions.
fn violations(style: &StyleSource, spacers: std::ops::Range<usize>) -> HashMap<&'static str, usize> {
    let mut found = HashMap::new();
    let mut bump = |what| *found.entry(what).or_insert(0) += 1;
    for seed in 0..SEEDS {
        for count in spacers.clone() {
            let compiled = compile(style, &markdown(count, seed));
            let mut started: HashMap<String, usize> = HashMap::new();
            for (page, lines) in body_lines(&compiled).iter().enumerate() {
                if let Some((_, last)) = lines.last() {
                    if last.contains("ZR") && last.contains("Title") {
                        bump("a role heading ends a page");
                    }
                    if last.contains("ZE") {
                        bump("an era heading ends a page");
                    }
                    if ["Summary", "Experience", "Education"]
                        .iter()
                        .any(|s| last.starts_with(s))
                    {
                        bump("a section heading ends a page");
                    }
                    if last.contains("ZB") {
                        bump("a blurb ends a page though bullets follow");
                    }
                }
                for token in lines.iter().flat_map(|(_, l)| l.split(|c: char| !c.is_alphanumeric())) {
                    let digits = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
                    if token.strip_prefix('B').is_some_and(digits) {
                        started.insert(token.to_string(), page);
                    } else if let Some(n) = token.strip_prefix("end").filter(|n| digits(n))
                        && started.get(&format!("B{n}")) != Some(&page)
                    {
                        bump("a bullet splits across pages");
                    }
                }
            }
        }
    }
    found
}

/// The sweep over the built-in style, computed once for the tests that read it.
fn builtin_violations() -> HashMap<&'static str, usize> {
    static FOUND: std::sync::OnceLock<HashMap<&'static str, usize>> = std::sync::OnceLock::new();
    FOUND.get_or_init(|| violations(&builtin(), SWEEP)).clone()
}

fn builtin() -> StyleSource {
    assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic))
}

#[test]
fn role_heading_is_kept_with_first_bullet() {
    let found = builtin_violations();
    assert_eq!(found, HashMap::new(), "headings or blurbs stranded at a page bottom");
}

#[test]
fn bullet_does_not_split_across_pages() {
    assert!(!builtin_violations().contains_key("a bullet splits across pages"));
}

/// Copies the built-in styles to a temp directory with `from` replaced by `to` in the shared components.
fn weakened(from: &str, to: &str) -> (tempfile::TempDir, StyleSource) {
    let dir = assert_ok!(tempfile::tempdir());
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("styles");
    for relative in ["lib/components.typ", "pragmatic/style.typ", "pragmatic/theme.typ"] {
        let target = dir.path().join(relative);
        assert_ok!(std::fs::create_dir_all(assert_some!(target.parent())));
        let text = assert_ok!(std::fs::read_to_string(source.join(relative)));
        assert_ok!(std::fs::write(&target, text.replace(from, to)));
    }
    let style = assert_ok!(StyleSource::directory(
        dir.path(),
        assert_ok!(StyleName::try_new("pragmatic"))
    ));
    (dir, style)
}

#[test]
fn the_check_fails_for_a_style_without_sticky_headings() {
    let (_dir, style) = weakened("sticky: true", "sticky: false");
    let found = violations(&style, SWEEP);
    assert!(found.contains_key("an era heading ends a page"), "{found:?}");
}

#[test]
fn the_check_fails_for_a_style_with_breakable_bullets() {
    let (_dir, style) = weakened("breakable: false", "breakable: true");
    assert!(violations(&style, SWEEP).contains_key("a bullet splits across pages"));
}

/// A directory holding the originals (Calibri, Georgia) as `RESUME_ORIGINAL_FONTS_DIR`; proprietary, local only.
fn originals_dir() -> PathBuf {
    let dir = std::env::var_os("RESUME_ORIGINAL_FONTS_DIR").expect(
        "set RESUME_ORIGINAL_FONTS_DIR to a directory with Calibri and Georgia (regular, bold, italic, bold italic)",
    );
    PathBuf::from(dir)
}

/// Lines of the same resume set with the originals (Calibri, Georgia) and with only the bundled fonts.
fn originals_and_bundled(style: &StyleSource) -> (Lines, Lines) {
    let md = markdown(12, 1);
    let render = |set: FontSet| {
        let (resume, _) = crate::markdown::parse("lines.md", &md).into_parts();
        let typesetter = Typesetter::new(Arc::new(set));
        body_lines(&assert_ok!(typesetter.compile(
            &assert_some!(resume),
            style,
            RenderOptions::new(Paper::Letter)
        )))
    };
    let originals = assert_ok!(
        FontSet::builder()
            .application_dirs(vec![])
            .system_source(Vec::new)
            .font_path(originals_dir())
            .build()
    );
    assert!(
        originals
            .faces()
            .iter()
            .any(|(family, origin)| family == "Calibri" && *origin == FontOrigin::FontPath)
    );
    (
        render(originals),
        render(assert_ok!(FontSet::builder().mode(FontsMode::Bundled).build())),
    )
}

#[test]
#[ignore = "needs RESUME_ORIGINAL_FONTS_DIR: proprietary Calibri and Georgia files, local only"]
fn bundled_and_original_body_fonts_break_lines_identically() {
    let (originals, bundled) = originals_and_bundled(&builtin());
    assert_eq!(originals, bundled);
}

#[test]
#[ignore = "needs RESUME_ORIGINAL_FONTS_DIR: proprietary Calibri and Georgia files, local only"]
fn the_identity_check_catches_line_pitch_taken_from_font_metrics() {
    let (_dir, style) = weakened(
        "top: 0.952em, bottom: -0.269em, leading: 0pt",
        "top: \"cap-height\", bottom: \"baseline\", leading: 0.589em",
    );
    let (originals, bundled) = originals_and_bundled(&style);
    assert_ne!(originals, bundled);
}

#[test]
fn consecutive_and_trailing_page_breaks_never_leave_a_blank_page() {
    // A plain `pagebreak()` would add an empty page for the second marker and for the trailing one.
    let md = "# T P\n\nCity · a@b.co\n\n## A\n\n- one\n\n<div class=\"page-break\"></div>\n\n<div class=\"page-break\"></div>\n\n## B\n\n- two\n\n<div class=\"page-break\"></div>\n";
    let compiled = compile(&builtin(), md);
    assert_eq!(compiled.page_count(), 2, "{:?}", body_lines(&compiled));
}

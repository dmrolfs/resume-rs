//! ABOUTME: Pagination guarantees and header layout of the shared components, checked on generated resumes.
//! ABOUTME: Lives inside the crate to read frames directly; each check is proven able to fail by a weakened style.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use typst::layout::{Frame, FrameItem, Point};

use super::*;
use crate::model::StyleName;
use crate::model::options::{BuiltinStyle, FontOrigin, FontsMode};

/// Resumes per spacer count in a sweep, and the spacer counts swept: enough that a page break lands at every
/// offset in a role, short enough to run in seconds in a debug build.
const SEEDS: u64 = 2;
const SWEEP: std::ops::Range<usize> = 0..40;

/// A broken pagination guarantee, as reported by a sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Violation {
    RoleHeadingEndsPage,
    EraHeadingEndsPage,
    SectionHeadingEndsPage,
    BlurbEndsPageThoughBodyFollows,
    BulletSplitsAcrossPages,
    BulletLostOrRepeated,
}

/// One shaped piece of text and where it sits on the page, in points.
#[derive(Debug, Clone)]
struct Run {
    y: f64,
    x: f64,
    text: String,
}

fn collect_runs(frame: &Frame, origin: Point, out: &mut Vec<Run>) {
    for (position, item) in frame.items() {
        let at = origin + *position;
        match item {
            FrameItem::Group(group) => collect_runs(
                &group.frame,
                Point::new(at.x + group.transform.tx, at.y + group.transform.ty),
                out,
            ),
            FrameItem::Text(run) => out.push(Run {
                y: at.y.to_pt(),
                x: at.x.to_pt(),
                text: run.text.to_string(),
            }),
            _ => {}
        }
    }
}

/// Every text run of a page, top to bottom and left to right.
fn page_runs(compiled: &Compiled, page: usize) -> Vec<Run> {
    let mut runs = Vec::new();
    collect_runs(&compiled.document.pages()[page].frame, Point::zero(), &mut runs);
    runs.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    runs
}

/// The runs of a page joined into lines: baseline in points, and the text left to right.
fn page_lines(compiled: &Compiled, page: usize) -> Vec<(f64, String)> {
    let mut lines: Vec<(f64, String)> = Vec::new();
    for run in page_runs(compiled, page) {
        match lines.last_mut() {
            Some((y, line)) if (run.y - *y).abs() < 0.3 => line.push_str(&run.text),
            _ => lines.push((run.y, run.text)),
        }
    }
    lines
}

/// The footer is the line that reads `Page N of M`; found by its content, not by where it sits.
fn is_footer(line: &str) -> bool {
    line.contains("Page ") && line.contains(" of ")
}

/// Per page, the body lines.
type Lines = Vec<Vec<(i64, String)>>;

/// The body lines of every page, top to bottom: baseline in hundredths of a point, and the text. Each page
/// must have exactly one footer line, which is left out.
fn body_lines(compiled: &Compiled) -> Lines {
    (0..compiled.page_count())
        .map(|page| {
            let (footers, body): (Vec<_>, Vec<_>) = page_lines(compiled, page)
                .into_iter()
                .partition(|(_, line)| is_footer(line));
            assert_eq!(footers.len(), 1, "page {page} has no single footer: {footers:?}");
            body.into_iter()
                .map(|(y, line)| ((y * 100.0).round() as i64, line))
                .collect()
        })
        .collect()
}

/// A resume with `spacers` one-line paragraphs ahead of eight roles, so the page break lands elsewhere for each
/// count. The roles vary in shape: with and without a blurb, with a paragraph, eras, labeled bullets, and one
/// with nothing under its heading. Also returns how many numbered bullets precede the one in Education, `B999`.
fn markdown(spacers: usize, seed: u64) -> (String, usize) {
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
    let mut bullet = 0;
    let mut one_bullet = |md: &mut String, next: &mut dyn FnMut(u64) -> u64| {
        let label = if bullet % 3 == 0 {
            format!("**Label{bullet}:** ")
        } else {
            String::new()
        };
        let n = 4 + next(50);
        *md += &format!("- {label}B{bullet} {} end{bullet}\n", words(n, next));
        bullet += 1;
    };
    let mut md = String::from("# Test Person\n\nCity, ST · 555-123-4567 · a@b.co\n\n## Summary\n\n");
    for i in 0..spacers {
        md += &format!("Spacer {i}.\n\n");
    }
    md += "## Experience\n\n";
    for r in 0..8 {
        let years = format!("20{:02} – 20{:02}", 20 - r, 21 - r);
        if r == 6 {
            md += &format!("### ZQ{r} · Title{r} · {years}\n\n");
            continue;
        }
        md += &format!("### ZR{r} · Title{r} · {years}\n\n");
        if r % 3 != 1 {
            md += &format!("ZB{r} blurb line.\n\n");
        }
        if r % 2 == 0 {
            md += &format!("ZP{r} a paragraph of {}.\n\n", words(12, &mut next));
        }
        for e in 0..next(3) {
            md += &format!("**ZE{r}x{e} · {years}**\n\n");
            for _ in 0..=next(3) {
                one_bullet(&mut md, &mut next);
            }
            md += "\n";
        }
        for _ in 0..=next(3) {
            one_bullet(&mut md, &mut next);
        }
        md += "\n";
    }
    md += "## Education\n\n- B999 last end999\n";
    (md, bullet)
}

fn fonts() -> Arc<FontSet> {
    static FONTS: OnceLock<Arc<FontSet>> = OnceLock::new();
    Arc::clone(FONTS.get_or_init(|| Arc::new(FontSet::bundled())))
}

/// Compiles `md`, which must parse without a single diagnostic.
fn compile(style: &StyleSource, md: &str) -> Compiled {
    let (resume, diagnostics) = crate::markdown::parse("sweep.md", md).into_parts();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let typesetter = Typesetter::new(fonts());
    assert_ok!(typesetter.compile(&assert_some!(resume), style, RenderOptions::new(Paper::Letter)))
}

/// Compiles `md` whatever warnings it parses with.
fn compile_lenient(md: &str) -> Compiled {
    let (resume, _) = crate::markdown::parse("lenient.md", md).into_parts();
    let typesetter = Typesetter::new(fonts());
    assert_ok!(typesetter.compile(&assert_some!(resume), &builtin(), RenderOptions::new(Paper::Letter)))
}

/// The broken guarantees over a sweep of page-break positions.
fn violations(style: &StyleSource, spacers: std::ops::Range<usize>) -> BTreeSet<Violation> {
    let mut found = BTreeSet::new();
    for seed in 0..SEEDS {
        for count in spacers.clone() {
            let (md, bullets) = markdown(count, seed);
            let lines = body_lines(&compile(style, &md));
            found.extend(page_ends(&lines));
            found.extend(bullet_integrity(&lines, bullets));
        }
    }
    found
}

/// What ends a page that must not.
fn page_ends(lines: &Lines) -> BTreeSet<Violation> {
    let mut found = BTreeSet::new();
    for (_, last) in lines.iter().filter_map(|page| page.last()) {
        if last.contains("ZR") && last.contains("Title") {
            found.insert(Violation::RoleHeadingEndsPage);
        }
        if last.contains("ZE") {
            found.insert(Violation::EraHeadingEndsPage);
        }
        if ["Summary", "Experience", "Education"]
            .iter()
            .any(|s| last.starts_with(s))
        {
            found.insert(Violation::SectionHeadingEndsPage);
        }
        if last.contains("ZB") {
            found.insert(Violation::BlurbEndsPageThoughBodyFollows);
        }
    }
    found
}

/// Each of the `bullets` numbered bullets and `B999` (`B<n> ... end<n>`) appears once, whole, on one page.
fn bullet_integrity(lines: &Lines, bullets: usize) -> BTreeSet<Violation> {
    let digits = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
    let mut begun: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut ended: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (page, page_lines) in lines.iter().enumerate() {
        for token in page_lines
            .iter()
            .flat_map(|(_, l)| l.split(|c: char| !c.is_alphanumeric()))
        {
            if let Some(n) = token.strip_prefix('B').filter(|n| digits(n)) {
                begun.entry(n.to_string()).or_default().push(page);
            } else if let Some(n) = token.strip_prefix("end").filter(|n| digits(n)) {
                ended.entry(n.to_string()).or_default().push(page);
            }
        }
    }
    let mut found = BTreeSet::new();
    let expected = (0..bullets).map(|n| n.to_string()).chain(["999".to_string()]);
    let keys: BTreeSet<String> = expected
        .chain(begun.keys().cloned())
        .chain(ended.keys().cloned())
        .collect();
    for key in &keys {
        match (begun.get(key).map(Vec::as_slice), ended.get(key).map(Vec::as_slice)) {
            (Some([b]), Some([e])) if b == e => {}
            (Some([_]), Some([_])) => {
                found.insert(Violation::BulletSplitsAcrossPages);
            }
            _ => {
                found.insert(Violation::BulletLostOrRepeated);
            }
        }
    }
    found
}

fn builtin() -> StyleSource {
    assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic))
}

/// The sweep over the built-in style, computed once for the tests that read it.
fn builtin_violations() -> BTreeSet<Violation> {
    static FOUND: OnceLock<BTreeSet<Violation>> = OnceLock::new();
    FOUND.get_or_init(|| violations(&builtin(), SWEEP)).clone()
}

#[test]
fn headings_blurbs_and_bullets_keep_their_pagination_guarantees() {
    assert_eq!(
        builtin_violations(),
        BTreeSet::new(),
        "headings or blurbs stranded, or bullets split"
    );
}

/// Copies the built-in styles to a temp directory with `from` replaced by `to` in exactly one place.
fn weakened(from: &str, to: &str) -> (tempfile::TempDir, StyleSource) {
    let dir = assert_ok!(tempfile::tempdir());
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("styles");
    let mut replaced = 0;
    for relative in ["lib/components.typ", "pragmatic/style.typ", "pragmatic/theme.typ"] {
        let target = dir.path().join(relative);
        assert_ok!(std::fs::create_dir_all(assert_some!(target.parent())));
        let text = assert_ok!(std::fs::read_to_string(source.join(relative)));
        replaced += text.matches(from).count();
        assert_ok!(std::fs::write(&target, text.replace(from, to)));
    }
    assert_eq!(replaced, 1, "`{from}` must name exactly one place in the styles");
    let style = assert_ok!(StyleSource::directory(
        dir.path(),
        assert_ok!(StyleName::try_new("pragmatic"))
    ));
    (dir, style)
}

/// Each guarantee on its own, the one line that holds it, and what the sweep must report once it is gone.
const WEAKENINGS: [(&str, &str, &str, Violation); 6] = [
    (
        "role heading stickiness",
        "sticky: follows,",
        "sticky: false,",
        Violation::RoleHeadingEndsPage,
    ),
    (
        "era heading stickiness",
        "sticky: true,\n  breakable: false,\n  above: theme.spacing.era,",
        "sticky: false,\n  breakable: false,\n  above: theme.spacing.era,",
        Violation::EraHeadingEndsPage,
    ),
    (
        "section heading stickiness",
        "width: 100%,\n  sticky: true,",
        "width: 100%,\n  sticky: false,",
        Violation::SectionHeadingEndsPage,
    ),
    (
        "blurb stickiness",
        "sticky: sticky,",
        "sticky: false,",
        Violation::BlurbEndsPageThoughBodyFollows,
    ),
    (
        "unbreakable bullets",
        "breakable: false,\n    above: 0pt,",
        "breakable: true,\n    above: 0pt,",
        Violation::BulletSplitsAcrossPages,
    ),
    (
        "bullet text",
        "\n      render-inlines(item.content)\n",
        "\n      []\n",
        Violation::BulletLostOrRepeated,
    ),
];

#[test]
fn each_pagination_check_fails_once_its_guarantee_is_removed() {
    for (what, from, to, expected) in WEAKENINGS {
        let (_dir, style) = weakened(from, to);
        let found = violations(&style, SWEEP);
        assert!(found.contains(&expected), "without {what} the sweep saw only {found:?}");
    }
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
    let (md, _) = markdown(12, 1);
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
        "top: 0.952em, bottom: -0.269em, leading: 0.1em",
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

#[test]
fn wrapped_bullet_lines_sit_one_theme_line_pitch_apart() {
    // The pitch comes from explicit text edges and leading (0.952em + 0.269em + 0.1em at 11pt), so it is the same
    // for any body font.
    let md = format!(
        "# T P\n\nCity · a@b.co\n\n## A\n\n- {}\n",
        "wrapword ".repeat(80).trim_end()
    );
    let compiled = compile(&builtin(), &md);
    let ys: Vec<f64> = page_lines(&compiled, 0)
        .into_iter()
        .filter(|(_, line)| line.contains("wrapword"))
        .map(|(y, _)| y)
        .collect();
    assert!(ys.len() >= 3, "the bullet must wrap: {ys:?}");
    for pair in ys.windows(2) {
        assert!(
            (pair[1] - pair[0] - 11.0 * 1.321).abs() < 0.01,
            "line pitch {} is not 14.53pt",
            pair[1] - pair[0]
        );
    }
}

// ---- header ----------------------------------------------------------------------------------------

fn with_contact(contact: &str) -> String {
    format!("# Ada Lovelace\n\n{contact}\n\n## Summary\n\n- one\n")
}

fn text_of(compiled: &Compiled) -> String {
    page_lines(compiled, 0)
        .into_iter()
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_containing(runs: &[Run], needle: &str) -> Run {
    runs.iter()
        .find(|run| run.text.contains(needle))
        .cloned()
        .unwrap_or_else(|| panic!("no run contains `{needle}` in {runs:?}"))
}

#[test]
fn text_before_a_link_keeps_its_space() {
    let compiled = compile(
        &builtin(),
        &with_contact("City, ST · LinkedIn: [ada](https://linkedin.com/in/ada) · Email: [a@b.com](mailto:a@b.com)"),
    );
    let text = text_of(&compiled);
    assert!(text.contains("LinkedIn: ada"), "{text}");
    assert!(text.contains("Email: a@b.com"), "{text}");
}

#[test]
fn phone_and_email_go_right_and_location_and_links_go_left_whatever_the_order() {
    let compiled = compile(
        &builtin(),
        &with_contact("a@b.co · [@ada](https://mastodon.social/@ada) · (555) 123-4567 · City, ST"),
    );
    let runs = page_runs(&compiled, 0);
    let middle = compiled.document.pages()[0].frame.width().to_pt() / 2.0;
    let (city, link) = (run_containing(&runs, "City, ST"), run_containing(&runs, "@ada"));
    let (phone, email) = (run_containing(&runs, "(555) 123-4567"), run_containing(&runs, "a@b.co"));
    assert!(city.x < middle && link.x < middle, "location and links on the left");
    assert!(phone.x > middle && email.x > middle, "phone and email on the right");
    assert!(city.y < link.y, "the location comes before the links");
    assert!(phone.y < email.y, "the phone comes before the email");
}

#[test]
fn a_link_whose_text_has_an_at_sign_is_not_an_email() {
    let compiled = compile(&builtin(), &with_contact("City · [@ada](https://mastodon.social/@ada)"));
    let runs = page_runs(&compiled, 0);
    let middle = compiled.document.pages()[0].frame.width().to_pt() / 2.0;
    assert!(run_containing(&runs, "@ada").x < middle);
}

/// True when the name shares its line with the contact items: they sit within the height of the name.
fn shares_the_line(compiled: &Compiled, name_word: &str, side: &str) -> bool {
    let runs = page_runs(compiled, 0);
    (run_containing(&runs, side).y - run_containing(&runs, name_word).y).abs() < 10.0
}

#[test]
fn a_name_too_long_to_share_the_line_stacks_above_the_contact_items() {
    let name = "Bartholomew Featherstonehaugh Cholmondeley";
    let compiled = compile(
        &builtin(),
        &format!("# {name}\n\nCity, ST · a@b.co\n\n## Summary\n\n- one\n"),
    );
    let lines = page_lines(&compiled, 0);
    assert_eq!(lines[0].1, name, "the name is on a line of its own");
    assert!(lines[1].1.contains("City, ST"), "{lines:?}");
}

#[test]
fn a_name_wider_than_half_the_line_still_shares_it_when_the_sides_are_short() {
    let compiled = compile(
        &builtin(),
        "# Bartholomew Featherstonehaugh\n\nCity, ST · a@b.co\n\n## Summary\n\n- one\n",
    );
    assert!(shares_the_line(&compiled, "Bartholomew", "City, ST"));
    assert!(shares_the_line(&compiled, "Bartholomew", "a@b.co"));
}

#[test]
fn wide_contact_sides_stack_a_short_name_instead_of_overlapping_it() {
    let compiled = compile(
        &builtin(),
        &with_contact("1234 Long Street Name Avenue, Springfield, Massachusetts, United States · a@b.co"),
    );
    let lines = page_lines(&compiled, 0);
    assert_eq!(lines[0].1, "Ada Lovelace", "{lines:?}");
}

#[test]
fn a_short_name_with_short_sides_shares_the_line_with_them() {
    let compiled = compile(&builtin(), &with_contact("City, ST · a@b.co"));
    assert!(shares_the_line(&compiled, "Lovelace", "City, ST"));
    assert!(shares_the_line(&compiled, "Lovelace", "a@b.co"));
}

#[test]
fn a_resume_without_a_contact_line_renders_the_name_alone() {
    let compiled = compile_lenient("# Ada Lovelace\n\n## Summary\n\n- one\n");
    assert_eq!(page_lines(&compiled, 0)[0].1, "Ada Lovelace");
    assert_eq!(compiled.page_count(), 1);
}

// ---- the style and the resume contract must agree ----------------------------------------------------

#[test]
fn an_unknown_inline_kind_stops_the_render_instead_of_vanishing() {
    let (_dir, style) = weakened("#header(theme, resume)", "#render-inlines(((kind: \"blink\"),))");
    let (resume, _) = crate::markdown::parse("x.md", &with_contact("City")).into_parts();
    let err =
        assert_err!(Typesetter::new(fonts()).compile(&assert_some!(resume), &style, RenderOptions::new(Paper::Letter)));
    assert!(format!("{err:?}").contains("unknown inline kind"), "{err:?}");
}

#[test]
fn a_resume_of_another_schema_version_stops_the_render() {
    let (_dir, style) = weakened(
        "#let resume = index-roles(json(\"/resume.json\"))",
        "#let resume = index-roles((..json(\"/resume.json\"), schema: 2))",
    );
    let (resume, _) = crate::markdown::parse("x.md", &with_contact("City")).into_parts();
    let err =
        assert_err!(Typesetter::new(fonts()).compile(&assert_some!(resume), &style, RenderOptions::new(Paper::Letter)));
    assert!(format!("{err:?}").contains("schema"), "{err:?}");
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
    let styles = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("styles");
    let mut on_disk = Vec::new();
    walk(&styles, &styles, &mut on_disk);
    assert!(
        on_disk.iter().any(|f| f == "pragmatic/style.typ"),
        "the walk found nothing: {on_disk:?}"
    );
    let style = builtin();
    for relative in &on_disk {
        let vpath = assert_ok!(typst::syntax::VirtualPath::new(format!("/{relative}")));
        assert!(style.reads(&vpath), "{relative} is not embedded");
    }
}

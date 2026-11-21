//! ABOUTME: Layout of the built-in experienced style: gutter labels, skills table, role line, header, footer, fitting.
//! ABOUTME: A child of pragmatic_tests so it shares the frame-run and pagination-sweep helpers; reads frames directly.

use std::collections::BTreeSet;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;

use super::*;

// The experienced theme's numbers, in points (styles/experienced/theme.typ); a theme change is one edit here.
/// `gutter.width`, `gutter.inset`, `gutter.gap`.
const GUTTER: f64 = 113.15;
const LABEL_INSET: f64 = 5.9;
const GUTTER_GAP: f64 = 5.95;
/// `page-margin.left` and `.right`.
const PAGE_MARGIN: f64 = 36.0;
/// `size.*`.
const BODY: f64 = 11.0;
const NAME: f64 = 24.0;
const LABEL: f64 = 12.0;
const LABEL_MIN: f64 = 8.0;
const ROLE: f64 = 12.0;
const PERIOD: f64 = 11.0;
const FOOTER: f64 = 8.5;
/// The text edges, 0.952em above the baseline and 0.269em below (`line.top`, `line.bottom`), and `line.leading`.
const LINE_EDGES_EM: f64 = 0.952 + 0.269;
/// `spacing.section`, `spacing.skill-row`.
const SECTION_GAP: f64 = 14.0;
const SKILL_ROW_GAP: f64 = 2.0;
/// `skills.label-max` (2.2in) and `skills.column-gap` (0.15in).
const SKILL_LABEL_MAX: f64 = 2.2 * 72.0;
const SKILL_COLUMN_GAP: f64 = 0.15 * 72.0;
/// The template puts the bullet marker 18pt and the text 36pt into the text column (`bullet.indent`, `bullet.gap`).
const BULLET_MARKER: f64 = 18.0;
const BULLET_TEXT: f64 = 36.0;
/// Where the labels and the text column start with the default theme.
const LABEL_X: f64 = PAGE_MARGIN + LABEL_INSET;
const COLUMN_X: f64 = PAGE_MARGIN + GUTTER + GUTTER_GAP;

fn experienced() -> StyleSource {
    builtin_style(BuiltinStyle::Experienced)
}

/// The left edge, in points, of the first run of page 1 that holds each needle.
fn lefts(compiled: &Compiled, needles: &[&str]) -> Vec<f64> {
    let runs = page_runs(compiled, 0);
    needles.iter().map(|needle| run_containing(&runs, needle).x).collect()
}

fn all_equal(values: &[f64]) -> bool {
    values.windows(2).all(|pair| (pair[0] - pair[1]).abs() < 0.01)
}

#[test]
fn experienced_keeps_the_same_pagination_guarantees() {
    // The label shares a grid row with the first block of its section; the guarantees must survive that.
    assert_eq!(
        violations(&experienced(), SWEEP),
        BTreeSet::new(),
        "headings or blurbs stranded, or bullets split"
    );
}

#[test]
fn the_experienced_heading_checks_fail_once_their_stickiness_is_removed() {
    // The sweep reads headings by their text; this proves it still sees the experienced style's (labels in
    // capitals beside the first block, a role without a title) by removing each guarantee from a copy.
    for (what, from, to, expected) in &WEAKENINGS[..2] {
        let (_dir, style) = weakened_in("experienced", from, to);
        let found = violations(&style, SWEEP);
        assert!(
            found.contains(expected),
            "without {what} the experienced sweep saw only {found:?}"
        );
    }
    // The first role of a section sits in the label's row, so the row's stickiness holds the heading and blurb.
    let (_dir, style) = weakened_in(
        "experienced",
        "row(render-first(theme, first), sticky: follows)",
        "row(render-first(theme, first), sticky: false)",
    );
    let found = violations(&style, SWEEP);
    assert!(
        found.contains(&Violation::BlurbEndsPageThoughBodyFollows),
        "without the row's stickiness the sweep saw only {found:?}"
    );
}

/// A resume whose Experience opens with a role that has no blurb, so the label's row holds only the role heading.
fn first_role_without_blurb(spacers: usize) -> String {
    let mut md = String::from("# A B\n\nCity · a@b.co\n\n## Summary\n\n");
    for i in 0..spacers {
        md += &format!("Spacer {i}.\n\n");
    }
    md + "## Experience\n\n### ZR0 · Title0 · 2020 – 2021\n\n- B1 one end1\n- B2 two end2\n"
}

#[test]
fn the_experienced_label_row_with_only_a_role_heading_moves_with_the_first_bullet() {
    let ends_page_alone = |style: &StyleSource| {
        SWEEP.clone().any(|spacers| {
            let lines = body_lines(&compile(style, &first_role_without_blurb(spacers)));
            page_ends(&lines).contains(&Violation::RoleHeadingEndsPage)
        })
    };
    assert!(!ends_page_alone(&experienced()));
    let (_dir, weakened) = weakened_in(
        "experienced",
        "row(render-first(theme, first), sticky: follows)",
        "row(render-first(theme, first), sticky: false)",
    );
    assert!(
        ends_page_alone(&weakened),
        "the sweep cannot see a stranded role heading"
    );
}

#[test]
fn the_section_labels_share_a_left_edge_and_the_content_starts_one_gutter_to_the_right() {
    let compiled = compile(&experienced(), &full_fixture_text());
    let labels = lefts(&compiled, &["SUMMARY", "LANGUAGES", "EXPERIENCE"]);
    let content = lefts(&compiled, &["Principal engineer who designs", "Analytical Engines Ltd"]);
    assert!(all_equal(&labels), "labels: {labels:?}");
    assert!(all_equal(&content), "content: {content:?}");
    // A label is inset from the page margin; the content starts one gutter and its gap after the page margin.
    let gutter = content[0] - labels[0];
    assert!(
        (gutter - (COLUMN_X - LABEL_X)).abs() < 0.5,
        "the content starts {gutter}pt right of the labels"
    );
}

#[test]
fn the_skill_values_start_in_one_column_right_of_the_widest_label() {
    let compiled = compile(&experienced(), &full_fixture_text());
    let values = lefts(&compiled, &["Rust, Java", "AWS (EKS", "Led all product"]);
    assert!(all_equal(&values), "the values do not line up: {values:?}");
    let runs = page_runs(&compiled, 0);
    let widest = run_containing(&runs, "Cloud and infrastructure");
    assert!(
        values[0] > widest.x + widest.width,
        "the values start at {} but the widest label ends at {}",
        values[0],
        widest.x + widest.width
    );
}

#[test]
fn a_skill_label_longer_than_the_label_column_wraps_inside_it() {
    let long = "Distributed systems design and operation across many regions and clouds";
    let md = format!("# Ada Lovelace\n\nCity · a@b.co\n\n## Skills\n\n- **{long}:** Rust\n- **Go:** Fast\n");
    let compiled = compile(&experienced(), &md);
    let runs = page_runs(&compiled, 0);
    let value = run_containing(&runs, "Rust");
    let label = run_containing(&runs, "Distributed");
    // theme.skills.label-max is 2.2in: the label wraps and the value column starts right of that.
    assert!(
        value.x - label.x <= SKILL_LABEL_MAX + SKILL_COLUMN_GAP + 0.5,
        "{} from the label to the value",
        value.x - label.x
    );
    assert!(
        page_lines(&compiled, 0)
            .iter()
            .filter(|(_, l)| l.contains("Distributed") || l.contains("clouds"))
            .count()
            >= 2,
        "the label wraps over two lines or more"
    );
}

#[test]
fn the_experienced_role_line_is_bold_company_comma_title_and_a_bold_period_at_the_right() {
    let md = "# Ada Lovelace\n\nCity · a@b.co\n\n## Experience\n\n### Acme Corp · Principal Engineer · 2020 – Present\n\nA blurb.\n\n- one\n\n### Babbage & Co · 2012 – 2017\n\n- two\n";
    let compiled = compile(&experienced(), md);
    let runs = page_runs(&compiled, 0);
    let middle = compiled.document.pages()[0].frame.width().to_pt() / 2.0;
    let (lead, period) = (
        run_containing(&runs, "Acme Corp, Principal Engineer"),
        run_containing(&runs, "2020 – Present"),
    );
    assert!(lead.bold && period.bold, "{lead:?} {period:?}");
    assert!(period.x > middle && (lead.y - period.y).abs() < 0.3);
    let (alone, alone_period) = (run_containing(&runs, "Babbage"), run_containing(&runs, "2012 – 2017"));
    assert!(
        alone.bold && alone_period.bold && alone_period.x > middle,
        "{alone:?} {alone_period:?}"
    );
}

#[test]
fn the_name_and_contact_lines_are_centered_on_the_page_not_on_the_text_column() {
    let md = "# Ada Lovelace\n\n[GitHub](https://github.com/ada) · City, ST · a@b.co\n\n## Summary\n\n- one\n";
    let compiled = compile(&experienced(), md);
    let page = compiled.document.pages()[0].frame.width().to_pt();
    let lines = page_lines(&compiled, 0);
    assert_eq!(lines[0].1, "Ada Lovelace");
    assert_eq!(lines[1].1, "a@b.co • City, ST");
    assert_eq!(lines[2].1, "GitHub");
    let runs = page_runs(&compiled, 0);
    for needle in ["Lovelace", "GitHub", "City, ST"] {
        let run = run_containing(&runs, needle);
        let on_line: Vec<_> = runs.iter().filter(|r| (r.y - run.y).abs() < 0.3).collect();
        let (left, right) = (
            on_line.iter().map(|r| r.x).fold(f64::MAX, f64::min),
            on_line.iter().map(|r| r.x + r.width).fold(f64::MIN, f64::max),
        );
        // The page margin is 0.5in on the right and 0.5in + the gutter on the left; centered text is
        // centered between the two edges of the page, not of the column.
        assert!(
            ((left + right) / 2.0 - page / 2.0).abs() < 1.5,
            "`{needle}` is centered off the page middle"
        );
    }
}

#[test]
fn the_experienced_footer_spans_the_page_over_the_gutter() {
    let compiled = compile(&experienced(), &full_fixture_text());
    let runs = page_runs(&compiled, 0);
    // Runs are sorted top to bottom, so the footer's are the last two.
    let footer_name = assert_some!(runs.iter().rfind(|r| r.text == "Ada Lovelace")).clone();
    let page_of = run_containing(&runs, "Page 1 of");
    assert!(
        footer_name.y > 700.0,
        "that is the footer's name, not the header's: {footer_name:?}"
    );
    // The footer name sits at the left page margin, over the gutter, and the labels are inset from it;
    // `Page N of M` ends at the right margin.
    let labels = lefts(&compiled, &["SUMMARY"]);
    assert!(
        (footer_name.x - PAGE_MARGIN).abs() < 0.5 && (labels[0] - footer_name.x - LABEL_INSET).abs() < 0.5,
        "the footer starts at {} and the labels at {}",
        footer_name.x,
        labels[0]
    );
    let page = compiled.document.pages()[0].frame.width().to_pt();
    assert!((page_of.x + page_of.width - (page - PAGE_MARGIN)).abs() < 0.5);
}

#[test]
fn experienced_capitalizes_whatever_the_section_is_called() {
    let compiled = compile(
        &experienced(),
        "# A B\n\nCity · a@b.co\n\n## Open source & writing\n\n- one\n",
    );
    // The label wraps in the gutter, so the words are checked in order, not as one string.
    let text = text_of(&compiled);
    assert!(position(&text, "OPEN SOURCE") < position(&text, "WRITING"), "{text}");
    assert!(!text.contains("Open source"), "{text}");
}

#[test]
fn a_page_break_before_a_sections_first_block_keeps_the_label_with_its_content() {
    let md = "# A B\n\nCity · a@b.co\n\n## Summary\n\n- one\n\n## Education\n\n<div class=\"page-break\"></div>\n\n### University · 2004 – 2008\n\n- Mathematics.\n";
    let compiled = compile(&experienced(), md);
    let lines = page_lines(&compiled, 1);
    assert!(lines[0].1.starts_with("EDUCATION"), "{lines:?}");
}

#[test]
fn a_section_label_shares_a_baseline_with_the_first_line_beside_it() {
    // The label is 12pt, the body 11pt and a role line 12pt; the label takes the text edges of the line it sits beside.
    let md = "# A B\n\nCity · a@b.co\n\n## Summary\n\nA paragraph.\n\n## Experience\n\n### Acme · Lead · 2020 – 2021\n\n- one\n\n## Education\n\n- Mathematics.\n";
    let compiled = compile(&experienced(), md);
    let runs = page_runs(&compiled, 0);
    for (label, line) in [
        ("SUMMARY", "A paragraph."),
        ("EXPERIENCE", "Acme, Lead"),
        ("EDUCATION", "Mathematics."),
    ] {
        let (label_run, line_run) = (run_containing(&runs, label), run_containing(&runs, line));
        assert!(
            (label_run.y - line_run.y).abs() < 0.05,
            "`{label}` baseline {} and `{line}` baseline {}",
            label_run.y,
            line_run.y
        );
    }
}

#[test]
fn the_experienced_style_sets_each_kind_of_text_in_its_own_font_weight_and_color() {
    // The full fixture, bundled fonts: Pagella for the name and the labels, Carlito for the rest.
    let compiled = compile(&experienced(), &full_fixture_text());
    let runs = page_runs(&compiled, 0);
    let check = |needle: &str, family: &str, bold: bool, fill: &str, size: f64| {
        let run = run_containing(&runs, needle);
        assert_eq!(
            (run.family.as_str(), run.bold, run.fill.as_deref(), run.size),
            (family, bold, Some(fill), size),
            "`{needle}`: {run:?}"
        );
    };
    check("Lovelace", "TeX Gyre Pagella", false, "#000000", NAME); // the name, regular weight
    check("SUMMARY", "TeX Gyre Pagella", true, "#000000", LABEL); // a section label, black
    check("Languages:", "Carlito", false, "#000000", BODY); // a skills label is regular weight
    check("Rust, Java", "Carlito", false, "#000000", BODY); // a skills value
    check("Cloud and infrastructure:", "Carlito", false, "#000000", BODY);
    check("Analytical Engines Ltd", "Carlito", true, "#000000", ROLE); // the company and title are bold
    check("Aug 2017", "Carlito", true, "#000000", PERIOD); // and so is the period
    check("Programming languages", "Carlito", false, "#000000", BODY); // an era name is italic, not bold
    check("Notes", "Carlito", false, "#cc0000", BODY); // a link is red
    check("Page 1 of", "Carlito", false, "#595959", FOOTER); // the footer is muted
    // Red is only on links: no label, role line or value is red.
    let red: Vec<_> = runs.iter().filter(|r| r.fill.as_deref() == Some("#cc0000")).collect();
    assert!(
        red.iter()
            .all(|r| ["Notes", "GitHub", "ABAC", "Acme"].iter().any(|l| r.text.contains(l))),
        "{red:?}"
    );
}

#[test]
fn a_skills_table_after_the_first_block_of_a_section_is_still_a_table() {
    let md = "# A B\n\nCity · a@b.co\n\n## Skills\n\nA paragraph first.\n\n- **Languages:** Rust\n- **Cloud and infrastructure:** AWS\n";
    let compiled = compile(&experienced(), md);
    let values = lefts(&compiled, &["Rust", "AWS"]);
    assert!(all_equal(&values), "the values do not line up: {values:?}");
    let runs = page_runs(&compiled, 0);
    assert!(
        runs.iter().all(|r| r.text != "•"),
        "a labeled block has no bullet markers: {runs:?}"
    );
    let widest = run_containing(&runs, "Cloud and infrastructure");
    // The value column starts one `skills.column-gap` (0.15in) after the widest label.
    assert!(
        (values[0] - (widest.x + widest.width) - SKILL_COLUMN_GAP).abs() < 0.5,
        "values at {}, widest label ends at {}",
        values[0],
        widest.x + widest.width
    );
}

#[test]
fn a_skills_block_with_one_unlabeled_bullet_is_a_list_not_a_table() {
    let md = "# A B\n\nCity · a@b.co\n\n## Skills\n\n- **Languages:** Rust\n- **Cloud and infrastructure:** AWS\n- an unlabeled one\n";
    let compiled = compile(&experienced(), md);
    let runs = page_runs(&compiled, 0);
    let markers = runs.iter().filter(|r| r.text == "•").count();
    assert_eq!(markers, 3, "every item has a bullet marker: {runs:?}");
    let values = lefts(&compiled, &["Rust", "AWS"]);
    assert!(
        (values[0] - values[1]).abs() > 0.5,
        "the values sit right after their own label, not in one column: {values:?}"
    );
}

#[test]
fn a_skill_row_never_splits_across_a_page_break() {
    // The middle row is long enough to run past a page foot when the spacers put it there: its label, the first
    // of its words, and the `end1` that closes it must all be on one page, which only holds if the row is
    // unbreakable. A short row would never reach the foot of the page mid-row.
    let long = "word ".repeat(400);
    let mut reached_a_page_break = 0;
    for spacers in 0..8 {
        let spacer: String = (0..spacers * 3).map(|i| format!("Spacer {i}.\n\n")).collect();
        let md = format!(
            "# A B\n\nCity · a@b.co\n\n## Skills\n\n{spacer}- **Short:** one end0\n- **Long:** {long} end1\n- **Last:** three end2\n"
        );
        let compiled = compile(&experienced(), &md);
        let mut pages = std::collections::BTreeMap::<String, BTreeSet<usize>>::new();
        for page in 0..compiled.page_count() {
            for run in page_runs(&compiled, page) {
                for token in run.text.split_whitespace() {
                    if token.starts_with("end") || token == "Long:" {
                        pages.entry(token.to_string()).or_default().insert(page);
                    }
                }
            }
        }
        assert_eq!(
            pages["Long:"], pages["end1"],
            "with {spacers} spacer paragraphs the long row splits: {pages:?}"
        );
        reached_a_page_break += usize::from(compiled.page_count() > 1);
    }
    assert!(
        reached_a_page_break >= 2,
        "the spacers never pushed the long row to a page break"
    );
}

#[test]
fn a_section_label_wider_than_the_gutter_is_set_smaller_and_stays_inside_it() {
    for title in ["Accomplishments", "Responsibilities", "Contributions"] {
        let md = format!("# A B\n\nCity · a@b.co\n\n## {title}\n\nA paragraph of text beside it.\n");
        let compiled = compile(&experienced(), &md);
        let runs = page_runs(&compiled, 0);
        // The word stays whole: one run holds all of it.
        let label = run_containing(&runs, &title.to_uppercase());
        let text = run_containing(&runs, "A paragraph");
        assert!(
            label.x + label.width <= text.x && label.size < LABEL && label.size >= LABEL_MIN,
            "{title}: set at {}pt, ends at {} but the text starts at {}",
            label.size,
            label.x + label.width,
            text.x
        );
    }
    // A label that fits is not shrunk.
    let compiled = compile(&experienced(), "# A B\n\nCity · a@b.co\n\n## Summary\n\nText.\n");
    assert_eq!(run_containing(&page_runs(&compiled, 0), "SUMMARY").size, LABEL);
}

#[test]
fn a_word_too_wide_even_at_the_smallest_size_breaks_inside_the_gutter() {
    // Sixty capital W, the widest title the parser accepts, is about 480pt at 8pt in a 101pt cell. It breaks between
    // letters, none are lost, and no part reaches the text column.
    let title = "W".repeat(60);
    let md = format!("# A B\n\nCity · a@b.co\n\n## {title}\n\nA paragraph of text beside it.\n");
    let compiled = compile(&experienced(), &md);
    let runs = page_runs(&compiled, 0);
    let text = run_containing(&runs, "A paragraph");
    let parts: Vec<_> = runs.iter().filter(|r| r.text.starts_with('W')).collect();
    assert!(parts.len() >= 2, "the word broke over several lines: {parts:?}");
    assert!(
        parts.iter().all(|r| r.size == LABEL_MIN),
        "set at the smallest size: {parts:?}"
    );
    assert!(
        parts.iter().all(|r| r.x + r.width <= text.x),
        "a part of the label reaches the text column at {}: {parts:?}",
        text.x
    );
    let letters: usize = parts.iter().map(|r| r.text.chars().filter(|c| *c == 'W').count()).sum();
    assert_eq!(letters, 60, "no letter is lost: {parts:?}");
}

/// A resume with `spacers` paragraphs, then an empty section between two others, then `ZNEXT`.
fn empty_section_between(spacers: usize) -> String {
    let spacer: String = (0..spacers).map(|i| format!("Spacer {i}.\n\n")).collect();
    format!("# A B\n\nCity · a@b.co\n\n## Summary\n\n{spacer}## Interests\n\n## Education\n\nZNEXT a paragraph.\n")
}

/// True when some page of the sweep ends with the empty section's label, so it is left alone at a page foot.
fn empty_label_ends_a_page(style: &StyleSource) -> bool {
    (0..60).any(|spacers| {
        let lines = body_lines(&compile(style, &empty_section_between(spacers)));
        lines
            .iter()
            .filter_map(|page| page.last())
            .any(|(_, last)| last == "INTERESTS")
    })
}

#[test]
fn an_empty_section_keeps_its_label_with_the_section_after_it() {
    assert!(!empty_label_ends_a_page(&experienced()));
    // The same sweep sees it once the row is not sticky, so the check can fail.
    let (_dir, weakened) = weakened_in("experienced", "row([], sticky: true)", "row([], sticky: false)");
    assert!(
        empty_label_ends_a_page(&weakened),
        "the sweep never put the empty label at a page foot, so it proves nothing"
    );
}

#[test]
fn roles_and_eras_come_in_document_order_with_each_era_after_its_role() {
    let probes = assert_ok!(compile(&experienced(), &full_fixture_text()).probes());
    let mut last = (0, f64::MIN);
    let mut roles = Vec::new();
    for probe in &probes {
        let at = (probe.page.get(), probe.y);
        assert!(at >= last, "the probes are out of order: {probes:?}");
        last = at;
        match &probe.kind {
            crate::typeset::probe::ProbeKind::Role { index, .. } => roles.push(*index),
            crate::typeset::probe::ProbeKind::Era { role, .. } => {
                assert_eq!(Some(role), roles.last(), "an era follows its own role");
            }
        }
    }
    assert_eq!(roles, [0, 1, 2, 3]);
}

#[test]
fn the_experienced_spacing_follows_the_theme() {
    // Positions come from the theme: bullets 18pt and 36pt in from the text column (0.25in indent, 12.5pt gap),
    // 2pt between skill rows, and a section gap above a label.
    let md = "# A B\n\nCity · a@b.co\n\n## Summary\n\n- one bullet\n\n## Skills\n\n- **A:** x\n- **B:** y\n";
    let compiled = compile(&experienced(), md);
    let runs = page_runs(&compiled, 0);
    let text = run_containing(&runs, "one bullet");
    // The list marker is the `•` on the bullet's own line; the header has one between its items too.
    let marker = assert_some!(runs.iter().find(|r| r.text == "•" && (r.y - text.y).abs() < 0.3)).clone();
    let column = COLUMN_X;
    assert!(
        (marker.x - (column + BULLET_MARKER)).abs() < 0.2,
        "marker at {}, column at {column}",
        marker.x
    );
    assert!(
        (text.x - (column + BULLET_TEXT)).abs() < 0.3,
        "text at {}, column at {column}",
        text.x
    );
    let (a, b) = (run_containing(&runs, "A:"), run_containing(&runs, "B:"));
    // A single-line row is the distance between the text edges of 11pt text (0.952em above the baseline and 0.269em
    // below it, 1.221em) plus the 2pt `skill-row` gap; rows of a table carry no extra leading.
    assert!(
        (b.y - a.y - (BODY * LINE_EDGES_EM + SKILL_ROW_GAP)).abs() < 0.05,
        "skill rows are {}pt apart",
        b.y - a.y
    );
    // From the bullet's baseline to the next label's: the section gap (14pt) and one line of 11pt text.
    let skills = run_containing(&runs, "SKILLS");
    assert!(
        (skills.y - text.y - (SECTION_GAP + BODY * LINE_EDGES_EM)).abs() < 0.3,
        "SKILLS is {}pt below the bullet",
        skills.y - text.y
    );
}

#[test]
fn editing_only_the_page_margin_or_the_gutter_moves_the_whole_layout_together() {
    // The style derives the page margin and the reach over the gutter from `gutter` and `page-margin`, so one edit
    // keeps the header centered on the page, the labels inside the page margin and the footer under them.
    let md = "# Ada Lovelace\n\nCity · a@b.co\n\n## Summary\n\nText beside it.\n";
    for (from, to, label_x, content_x) in [
        (
            "page-margin: (left: 0.5in,",
            "page-margin: (left: 1in,",
            72.0 + LABEL_INSET,
            72.0 + GUTTER + GUTTER_GAP,
        ),
        (
            "gutter: (width: 113.15pt,",
            "gutter: (width: 80pt,",
            PAGE_MARGIN + LABEL_INSET,
            PAGE_MARGIN + 80.0 + GUTTER_GAP,
        ),
    ] {
        let (_dir, style) = weakened_in("experienced", from, to);
        let compiled = compile(&style, md);
        let runs = page_runs(&compiled, 0);
        let (label, text) = (run_containing(&runs, "SUMMARY"), run_containing(&runs, "Text beside"));
        assert!((label.x - label_x).abs() < 0.2, "{to}: the label is at {}", label.x);
        assert!((text.x - content_x).abs() < 0.2, "{to}: the content is at {}", text.x);
        let footer = assert_some!(runs.iter().rfind(|r| r.text == "Ada Lovelace")).clone();
        assert!(
            (footer.x - (label_x - LABEL_INSET)).abs() < 0.2,
            "{to}: the footer is at {}",
            footer.x
        );
        // The name is centered between the page margins: midway between the left page margin and the right one.
        let name = run_containing(&runs, "Ada Lovelace");
        let page = compiled.document.pages()[0].frame.width().to_pt();
        let middle = (label_x - LABEL_INSET + page - PAGE_MARGIN) / 2.0;
        assert!(
            ((name.x + name.width / 2.0) - middle).abs() < 0.5,
            "{to}: the name is centered at {}",
            name.x + name.width / 2.0
        );
    }
}

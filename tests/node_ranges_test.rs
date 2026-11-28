//! ABOUTME: `Parsed::node_ranges` gives each model node the source text it was written as, in `Resume::nodes` order.
//! ABOUTME: Exact snippets for a document with every node kind, then invariants over the fixtures and the guide's examples.

use std::path::{Path, PathBuf};

use claims::assert_some;
use pretty_assertions::assert_eq;
use resume::markdown::parse;

const HEAD: &str = "# Ada Lovelace\n\nLondon, UK · ada@example.com\n\n";

/// The text each node's range covers, with the node's kind.
fn slices(markdown: &str) -> Vec<(String, String)> {
    let parsed = parse("r.md", markdown);
    let resume = assert_some!(parsed.best_effort());
    let nodes = resume.nodes();
    let ranges = parsed.node_ranges();
    assert_eq!(ranges.len(), nodes.len(), "one range per node");
    nodes
        .iter()
        .zip(ranges)
        .map(|(node, range)| (node.kind.to_string(), markdown[range.clone()].to_string()))
        .collect()
}

fn owned(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect()
}

const EVERY_KIND: &str = "## Summary\n\nIntro with `code` and **bold**.\n\n- S1 <!-- #a -->\n- S2\n\n## Experience\n\n\
### Acme · Engineer · 2015 – 2020\n\nThe blurb.\n\n- R1\n  - Nested one\n- R2\n\n**AI · 2016 – 2017**\n\n- E1\n\n\
Closing.\n\n<div class=\"page-break\"></div>\n\n### Beta · 2010 – 2014\n\n<!-- #first -->\n- B1\n";

#[test]
fn each_node_kind_is_the_text_it_was_written_as() {
    let markdown = format!("{HEAD}{EVERY_KIND}");
    assert_eq!(
        slices(&markdown),
        owned(&[
            ("section", "## Summary"),
            ("paragraph", "Intro with `code` and **bold**."),
            ("bullet", "S1 <!-- #a -->"),
            ("bullet", "S2"),
            ("section", "## Experience"),
            ("role", "### Acme · Engineer · 2015 – 2020"),
            ("blurb", "The blurb."),
            ("bullet", "R1"),
            ("bullet", "Nested one"),
            ("bullet", "R2"),
            ("era", "**AI · 2016 – 2017**"),
            ("bullet", "E1"),
            ("paragraph", "Closing."),
            ("role", "### Beta · 2010 – 2014"),
            ("bullet", "B1"),
        ])
    );
}

#[test]
fn a_nested_bullet_is_not_inside_its_parents_range() {
    let markdown = format!("{HEAD}## S\n\n- Parent\n  - Child\n");
    let found = slices(&markdown);
    assert_eq!(found[1].1, "Parent");
    assert_eq!(found[2].1, "Child");
}

#[test]
fn crlf_and_unicode_ranges_are_on_character_boundaries() {
    let lf = format!("{HEAD}## Résumé 履歴書\n\nÜber café.\n\n- Naïve – ok\n");
    let crlf = lf.replace('\n', "\r\n");
    for markdown in [lf, crlf] {
        // Slicing would panic off a character boundary.
        let found = slices(&markdown);
        assert_eq!(
            found.iter().map(|(_, text)| text.as_str()).collect::<Vec<_>>(),
            ["## Résumé 履歴書", "Über café.", "Naïve – ok"],
            "{markdown:?}"
        );
    }
}

#[test]
fn a_dropped_role_leaves_no_range_and_the_survivors_keep_theirs() {
    let markdown = format!("{HEAD}## S\n\n- A\n\n### Acme · bad · role\n\n- Lost\n\n## T\n\n- B\n");
    let found: Vec<String> = slices(&markdown).into_iter().map(|(_, text)| text).collect();
    assert_eq!(found, ["## S", "A", "## T", "B"]);
}

#[test]
fn a_text_without_a_model_has_no_ranges() {
    let parsed = parse("r.md", "no name heading here\n");
    assert!(parsed.node_ranges().is_empty());
}

#[test]
fn ranges_are_in_document_order_and_do_not_overlap() {
    let markdown = format!("{HEAD}{EVERY_KIND}");
    let parsed = parse("r.md", &markdown);
    let ranges = parsed.node_ranges();
    for pair in ranges.windows(2) {
        assert!(
            pair[0].end <= pair[1].start,
            "{:?} then {:?} overlap or are out of order",
            pair[0],
            pair[1]
        );
    }
}

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn every_fixture_and_guide_example_has_one_valid_range_per_node() {
    let mut texts = vec![assert_some!(
        std::fs::read_to_string(fixture("tests/fixtures/full.md")).ok()
    )];
    let guide = std::fs::read_to_string(fixture("docs/User-Guide.md")).expect("the guide is readable");
    let mut in_block = false;
    let mut block = String::new();
    for line in guide.lines() {
        if line.starts_with("```markdown resume") {
            in_block = true;
            block.clear();
        } else if in_block && line.starts_with("```") {
            in_block = false;
            texts.push(format!("{block}\n"));
        } else if in_block {
            block.push_str(line);
            block.push('\n');
        }
    }
    assert!(texts.len() >= 3, "the guide has resume examples: {}", texts.len());
    let mut checked = 0;
    for text in &texts {
        let parsed = parse("r.md", text);
        let Some(resume) = parsed.best_effort() else { continue };
        let ranges = parsed.node_ranges();
        assert_eq!(ranges.len(), resume.nodes().len(), "{text}");
        for range in ranges {
            assert!(range.start < range.end && range.end <= text.len(), "{range:?}");
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
            assert!(!text[range.clone()].trim().is_empty(), "an empty node range {range:?}");
            checked += 1;
        }
    }
    assert!(checked >= 20, "the check covers real nodes: {checked}");
}

#[test]
fn a_node_written_over_several_lines_is_covered_to_its_last_line() {
    let lf = format!(
        "{HEAD}## S\n\nFirst line of a paragraph\ncontinues here\nand ends here.\n\n- A bullet that\n  wraps to a second line\n- Next\n"
    );
    let crlf = lf.replace('\n', "\r\n");
    for (markdown, newline) in [(lf, "\n"), (crlf, "\r\n")] {
        let found = slices(&markdown);
        let expected_paragraph = format!("First line of a paragraph{newline}continues here{newline}and ends here.");
        let expected_bullet = format!("A bullet that{newline}  wraps to a second line");
        assert_eq!(found[1].1, expected_paragraph, "{markdown:?}");
        assert_eq!(found[2].1, expected_bullet, "{markdown:?}");
        assert_eq!(found[3].1, "Next", "{markdown:?}");
    }
}

// ---------------------------------------------------------------------------------------------------
// `Parsed::node_nested_in`: the source parent of a nested bullet, which the flat model does not keep.
// ---------------------------------------------------------------------------------------------------

/// `(kind, text, parent's text)` for every node, with the parent named by the text of the node it points at.
fn parents(markdown: &str) -> Vec<(String, String, Option<String>)> {
    let parsed = parse("r.md", markdown);
    let resume = assert_some!(parsed.best_effort());
    let nodes = resume.nodes();
    let ranges = parsed.node_ranges();
    let nested = parsed.node_nested_in();
    assert_eq!(nested.len(), nodes.len(), "one slot per node");
    nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let own = markdown[ranges[i].clone()].to_string();
            let parent = nested[i].map(|p| {
                assert!(p < i, "a parent comes before its child: node {i} names {p}");
                markdown[ranges[p].clone()].to_string()
            });
            (node.kind.to_string(), own, parent)
        })
        .collect()
}

fn nested_pairs(markdown: &str) -> Vec<(String, String)> {
    parents(markdown)
        .into_iter()
        .filter_map(|(_, own, parent)| parent.map(|p| (own, p)))
        .collect()
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    owned(items)
}

#[test]
fn a_nested_bullet_names_the_bullet_it_is_nested_in() {
    let markdown = format!("{HEAD}## S\n\n- Parent\n  - Child\n    - Grandchild\n  - Second child\n- Sibling\n");
    assert_eq!(
        nested_pairs(&markdown),
        pairs(&[("Child", "Parent"), ("Grandchild", "Child"), ("Second child", "Parent"),])
    );
}

#[test]
fn nesting_inside_a_role_and_an_era_is_found_too() {
    let markdown = format!(
        "{HEAD}## Experience\n\n### Acme · Engineer · 2015 – 2020\n\n- R1\n  - R1 detail\n\n**AI · 2016 – 2017**\n\n- E1\n  - E1 detail\n"
    );
    assert_eq!(
        nested_pairs(&markdown),
        pairs(&[("R1 detail", "R1"), ("E1 detail", "E1")])
    );
}

#[test]
fn only_nested_bullets_have_a_parent() {
    let markdown = format!("{HEAD}{EVERY_KIND}");
    let all = parents(&markdown);
    let with_parent: Vec<_> = all.iter().filter(|(_, _, p)| p.is_some()).collect();
    assert_eq!(with_parent.len(), 1, "{all:?}");
    assert_eq!(
        (
            with_parent[0].0.as_str(),
            with_parent[0].1.as_str(),
            with_parent[0].2.as_deref()
        ),
        ("bullet", "Nested one", Some("R1"))
    );
}

#[test]
fn a_nested_bullet_keeps_its_parent_when_the_parent_is_a_draft() {
    let markdown = format!("{HEAD}## S\n\n- Parent <!-- @draft -->\n  - Child\n- Sibling\n");
    assert_eq!(nested_pairs(&markdown), pairs(&[("Child", "Parent <!-- @draft -->")]));
}

#[test]
fn nesting_survives_crlf_and_a_byte_order_mark() {
    let lf = format!("{HEAD}## S\n\n- Parent\n  - Child\n");
    for text in [lf.replace('\n', "\r\n"), format!("\u{feff}{lf}")] {
        let found = nested_pairs(&text);
        assert_eq!(found.len(), 1, "{text:?}");
        assert_eq!((found[0].0.as_str(), found[0].1.as_str()), ("Child", "Parent"));
    }
}

#[test]
fn a_text_without_a_model_has_no_nesting() {
    assert!(parse("r.md", "no name heading here\n").node_nested_in().is_empty());
}

#[test]
fn a_bullet_with_nothing_to_show_passes_its_parent_down_to_its_children() {
    // The middle bullet shows nothing (an image only), so it is no node. `C` must still be tied to `A`, or a draft
    // `A` would not reach it.
    let markdown = format!("{HEAD}## S\n\n- A <!-- @draft -->\n  - ![](x.png)\n    - C\n- S\n");
    assert_eq!(nested_pairs(&markdown), pairs(&[("C", "A <!-- @draft -->")]));
}

#[test]
fn a_bullet_with_nothing_to_show_at_the_top_leaves_its_children_unparented() {
    let markdown = format!("{HEAD}## S\n\n-\n  - C\n- S\n");
    assert_eq!(nested_pairs(&markdown), pairs(&[]));
}

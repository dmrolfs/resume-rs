//! ABOUTME: Tests of `promote` through `select`: what is a list, how an item ranks, and what the report says.
//! ABOUTME: Each builds its resume by parsing Markdown, so lists and containers are the parser's, as in real use.

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;

use super::{MovedUp, NodeKind, Reordered, Scope, SelectedTags, Selection, select};
use crate::markdown::parse;
use crate::model::{Block, Inline, Resume, RoleBlock, Tag};

const HEAD: &str = "# Ada Lovelace\n\nLondon · a@b.co\n\n";
const ROLE: &str = "### Acme · Engineer · 2015 – 2017\n\n";

fn resume(body: &str) -> Resume {
    let markdown = format!("{HEAD}{body}");
    let parsed = parse("r.md", &markdown);
    assert_some!(parsed.valid(), "{markdown}\n{:?}", parsed.diagnostics()).clone()
}

fn tag_list(names: &[&str]) -> Vec<Tag> {
    names.iter().map(|n| assert_ok!(Tag::try_new(*n))).collect()
}

fn promoting(names: &[&str]) -> Selection {
    Selection {
        promote: tag_list(names),
        ..Selection::default()
    }
}

fn text(content: &[Inline]) -> String {
    crate::model::plain_text(content)
}

/// Every block's leaves, one string per block: `p:` for a paragraph, `-` items for a list, and container heads.
fn shape(resume: &Resume) -> Vec<String> {
    let bullets = |items: &[crate::model::Bullet]| {
        let names: Vec<String> = items.iter().map(|b| text(&b.content)).collect();
        format!("- {}", names.join(" "))
    };
    let mut out = Vec::new();
    for section in &resume.sections {
        out.push(format!("## {}", section.title));
        for block in &section.blocks {
            match block {
                Block::Paragraph { content, .. } => out.push(format!("p: {}", text(content))),
                Block::Bullets { items } => out.push(bullets(items)),
                Block::PageBreak => out.push("page-break".into()),
                Block::Role(role) => {
                    out.push(format!("### {}", role.company));
                    if let Some(blurb) = &role.blurb {
                        out.push(format!("blurb: {}", text(blurb)));
                    }
                    for part in &role.body {
                        match part {
                            RoleBlock::Paragraph { content, .. } => out.push(format!("p: {}", text(content))),
                            RoleBlock::Bullets { items } => out.push(bullets(items)),
                            RoleBlock::Era { name, items, .. } => {
                                out.push(format!("era: {name}"));
                                out.push(bullets(items));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

fn ordered(body: &str, selection: &Selection) -> (Vec<String>, super::SelectionReport) {
    let (tailored, report) = assert_ok!(select(&resume(body), selection));
    (shape(&tailored), report)
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

// ---- the rule ----------------------------------------------------------------------------------------------

#[test]
fn promoted_bullets_rise_within_their_role_and_equal_ranks_keep_their_order() {
    let body = format!("## X\n\n{ROLE}- a\n- b <!-- #lead -->\n- c\n- d <!-- #lead -->\n");
    let (out, _) = ordered(&body, &promoting(&["lead"]));
    assert_eq!(out, strings(&["## X", "### Acme", "- b d a c"]));
}

#[test]
fn the_first_place_among_an_items_tags_decides_its_rank_whatever_order_it_writes_them_in() {
    let body = "## S\n\n- a <!-- #scale -->\n- b <!-- #scale #people -->\n- c <!-- #lead -->\n- d\n";
    let (out, _) = ordered(body, &promoting(&["people", "lead", "scale"]));
    assert_eq!(out, strings(&["## S", "- b c a d"]));
}

#[test]
fn each_ancestor_caps_rank_before_the_stable_sort() {
    // Promote [people, lead, scale]; the ancestor carries `lead` (place 1). `a` is untagged, `b` is `scale`
    // (place 2), `c` is `people` (place 0). With the cap `a` and `b` both rank 1 and keep their order: c a b.
    // Without it `a` would rank last, below `b`: c b a.
    let bullets = "- a\n- b <!-- #scale -->\n- c <!-- #people -->\n";
    let promote = promoting(&["people", "lead", "scale"]);
    let section = format!("## S <!-- #lead -->\n\n{bullets}");
    assert_eq!(ordered(&section, &promote).0, strings(&["## S", "- c a b"]), "section");
    let role = format!("## X\n\n### Acme · Engineer · 2015 – 2017 <!-- #lead -->\n\n{bullets}");
    assert_eq!(
        ordered(&role, &promote).0,
        strings(&["## X", "### Acme", "- c a b"]),
        "role"
    );
    let era = format!("## X\n\n{ROLE}**AI · 2016 – 2017** <!-- #lead -->\n\n{bullets}");
    assert_eq!(
        ordered(&era, &promote).0,
        strings(&["## X", "### Acme", "era: AI", "- c a b"]),
        "era"
    );
}

#[test]
fn a_container_tag_at_place_k_gives_every_untagged_sibling_rank_k() {
    // The role's `lead` caps every bullet at place 1: `a` (untagged) and `b` (`scale`, place 2) tie at 1 and keep
    // their order; `c` (`people`, place 0) rises above both.
    let body =
        "## X\n\n### Acme · Engineer · 2015 – 2017 <!-- #lead -->\n\n- a\n- b <!-- #scale -->\n- c <!-- #people -->\n";
    let (out, _) = ordered(body, &promoting(&["people", "lead", "scale"]));
    assert_eq!(out, strings(&["## X", "### Acme", "- c a b"]));
}

#[test]
fn nothing_crosses_a_role_an_era_or_a_section_and_containers_keep_their_order() {
    let body = "## S\n\n- s1\n- s2 <!-- #lead -->\n\n## X\n\n### One · E · 2015 – 2017\n\n- o1\n\n**Early · 2015** \n\n- e1\n- e2 <!-- #lead -->\n\n### Two · E · 2013 – 2015\n\n- t1\n- t2 <!-- #lead -->\n";
    let (out, _) = ordered(body, &promoting(&["lead"]));
    assert_eq!(
        out,
        strings(&[
            "## S",
            "- s2 s1",
            "## X",
            "### One",
            "- o1",
            "era: Early",
            "- e2 e1",
            "### Two",
            "- t2 t1"
        ])
    );
}

#[test]
fn bullets_either_side_of_a_paragraph_or_a_page_break_are_separate_lists() {
    let body = "## S\n\n- a\n- b\n\nMiddle.\n\n- c\n- d <!-- #lead -->\n\n<div class=\"page-break\"></div>\n\n- e\n- f <!-- #lead -->\n";
    let (out, _) = ordered(body, &promoting(&["lead"]));
    assert_eq!(
        out,
        strings(&["## S", "- a b", "p: Middle.", "- d c", "page-break", "- f e"])
    );
}

#[test]
fn a_run_of_adjacent_paragraphs_is_one_list_and_never_mixes_with_bullets() {
    let body = format!(
        "## S\n\nP1.\n\nP2. <!-- #lead -->\n\n- a\n- b <!-- #lead -->\n\nP3.\n\nP4. <!-- #lead -->\n\n## X\n\n{ROLE}Blurb.\n\nR1.\n\nR2. <!-- #lead -->\n"
    );
    let (out, report) = ordered(&body, &promoting(&["lead"]));
    assert_eq!(
        out,
        strings(&[
            "## S",
            "p: P2.",
            "p: P1.",
            "- b a",
            "p: P4.",
            "p: P3.",
            "## X",
            "### Acme",
            "blurb: Blurb.",
            "p: R2.",
            "p: R1."
        ])
    );
    let kinds: Vec<NodeKind> = report.reordered.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [
            NodeKind::Paragraph,
            NodeKind::Bullet,
            NodeKind::Paragraph,
            NodeKind::Paragraph
        ],
        "records are in document order"
    );
}

#[test]
fn paragraphs_become_one_run_when_the_list_between_them_is_dropped_and_stay_apart_when_it_is_kept() {
    let selection = |tags: &[&str]| Selection {
        scope: Scope::Tags(assert_some!(SelectedTags::new(tag_list(tags).into_iter().collect()))),
        ..promoting(&["lead"])
    };
    let section = "## S\n\nP1.\n\n- x <!-- #arch -->\n\nP2. <!-- #lead -->\n";
    let (out, report) = ordered(section, &selection(&["lead"]));
    assert_eq!(
        out,
        strings(&["## S", "p: P2.", "p: P1."]),
        "the dropped list no longer separates them"
    );
    assert_eq!(
        (report.reordered[0].kind, report.reordered[0].kept),
        (NodeKind::Paragraph, 2)
    );
    let (out, _) = ordered(section, &selection(&["lead", "arch"]));
    assert_eq!(
        out,
        strings(&["## S", "p: P1.", "- x", "p: P2."]),
        "a kept list keeps them apart"
    );
    let role =
        format!("## X\n\n{ROLE}Blurb.\n\n- x <!-- #arch -->\n\nR1.\n\n- y <!-- #arch -->\n\nR2. <!-- #lead -->\n");
    let (out, _) = ordered(&role, &selection(&["lead"]));
    assert_eq!(out, strings(&["## X", "### Acme", "blurb: Blurb.", "p: R2.", "p: R1."]));
}

#[test]
fn paragraph_and_era_records_name_exact_nodes_and_positions_among_the_kept() {
    // Nodes: 0 S, 1 P1, 2 Px (dropped), 3 P2, 4 P3, 5 X, 6 role, 7 era, 8 e1, 9 e2, 10 e3.
    let body = format!(
        "## S\n\nP1.\n\nPx. <!-- #arch -->\n\nP2. <!-- #people -->\n\nP3. <!-- #lead -->\n\n## X\n\n{ROLE}**AI · 2016 – 2017**\n\n- e1\n- e2 <!-- #lead -->\n- e3 <!-- #people -->\n"
    );
    let selection = Selection {
        scope: Scope::Tags(assert_some!(SelectedTags::new(
            tag_list(&["lead", "people"]).into_iter().collect()
        ))),
        ..promoting(&["people", "lead"])
    };
    let (_, report) = ordered(&body, &selection);
    let node = |kind, label: &str, index| super::NodeRef {
        kind,
        label: label.into(),
        index,
    };
    let up = |kind, label: &str, index, from, to| MovedUp {
        node: node(kind, label, index),
        from,
        to,
    };
    assert_eq!(
        report.reordered,
        vec![
            Reordered {
                container: node(NodeKind::Section, "S", 0),
                kind: NodeKind::Paragraph,
                kept: 3,
                moved_up: vec![
                    up(NodeKind::Paragraph, "P2.", 3, 2, 1),
                    up(NodeKind::Paragraph, "P3.", 4, 3, 2)
                ],
            },
            Reordered {
                container: node(NodeKind::Era, "AI", 7),
                kind: NodeKind::Bullet,
                kept: 3,
                // e1 e2 e3 becomes e3 e2 e1: `e2` stays 2nd, so only `e3` rose.
                moved_up: vec![up(NodeKind::Bullet, "e3", 10, 3, 1)],
            },
        ]
    );
}

#[test]
fn a_core_only_selection_with_promote_orders_the_core_items() {
    // `Session::core_pages` keeps the caller's `promote` and changes only the scope, so its count is measured on
    // this order.
    let body = "## S\n\n- a <!-- @core -->\n- b <!-- @core #lead -->\n- c\n";
    let core = Selection {
        scope: Scope::CoreOnly,
        ..promoting(&["lead"])
    };
    assert_eq!(ordered(body, &core).0, strings(&["## S", "- b a"]));
}

#[test]
fn the_blurb_never_moves_even_when_a_paragraph_below_it_is_promoted() {
    let body = format!("## X\n\n{ROLE}Blurb.\n\nR1. <!-- #lead -->\n");
    let (out, report) = ordered(&body, &promoting(&["lead"]));
    assert_eq!(out, strings(&["## X", "### Acme", "blurb: Blurb.", "p: R1."]));
    assert!(report.reordered.is_empty());
}

#[test]
fn a_chosen_blurb_wording_keeps_the_blurbs_place_and_the_paragraphs_after_it_order_among_themselves() {
    let body = format!(
        "## X\n\n{ROLE}Long. <!-- =intro.default -->\n\nShort. <!-- =intro.short -->\n\nR1.\n\nR2. <!-- #lead -->\n"
    );
    let selection = Selection {
        prefer: vec![assert_ok!(crate::model::VariantName::try_new("short"))],
        ..promoting(&["lead"])
    };
    let (out, _) = ordered(&body, &selection);
    assert_eq!(out, strings(&["## X", "### Acme", "blurb: Short.", "p: R2.", "p: R1."]));
}

#[test]
fn a_group_ranks_by_the_tags_of_all_its_wordings_and_its_chosen_wording_keeps_the_groups_place() {
    // `lead` is on the `long` wording only; the group's tags are their union, so the default wording rises too.
    let body = "## S\n\n- a\n- g <!-- =g.default -->\n- g long <!-- =g.long #lead -->\n";
    let (out, _) = ordered(body, &promoting(&["lead"]));
    assert_eq!(out, strings(&["## S", "- g a"]));
}

#[test]
fn priority_and_status_do_not_change_rank() {
    let body =
        "## S\n\n- a <!-- @core -->\n- b <!-- @extra #lead -->\n- c <!-- @verify -->\n- d <!-- @draft #lead -->\n";
    let selection = Selection {
        drafts: true,
        ..promoting(&["lead"])
    };
    let (out, _) = ordered(body, &selection);
    assert_eq!(out, strings(&["## S", "- b d a c"]));
}

#[test]
fn a_promote_tag_on_an_untagged_bullet_makes_it_tagged_for_selection() {
    // Ordering never changes what is kept, so a tag added only to rank a general bullet also selects it: under
    // `tags = ["arch"]` it is dropped unless it carries `arch` too.
    let body = "## S\n\n- general\n- ranked <!-- #people -->\n- both <!-- #people #arch -->\n";
    let arch = Selection {
        scope: Scope::Tags(assert_some!(SelectedTags::new(
            tag_list(&["arch"]).into_iter().collect()
        ))),
        ..promoting(&["people"])
    };
    let (out, _) = ordered(body, &arch);
    assert_eq!(out, strings(&["## S", "- both general"]));
}

#[test]
fn nested_bullets_flattened_by_the_parser_are_ordered_as_siblings() {
    let body = "## S\n\n- parent\n  - child <!-- #lead -->\n- other\n";
    let parsed = parse("r.md", &format!("{HEAD}{body}"));
    let input = assert_some!(parsed.valid()).clone();
    let (tailored, _) = assert_ok!(select(&input, &promoting(&["lead"])));
    assert_eq!(shape(&tailored), strings(&["## S", "- child parent other"]));
}

#[test]
fn an_empty_promote_list_changes_nothing_and_reports_nothing() {
    let body = format!(
        "## S\n\nP1.\n\nP2. <!-- #lead -->\n\n- a\n- b <!-- #lead -->\n\n## X\n\n{ROLE}- c\n- d <!-- #lead -->\n"
    );
    let input = resume(&body);
    let (output, report) = assert_ok!(select(&input, &Selection::default()));
    assert_eq!(output, input);
    assert!(report.reordered.is_empty());
}

#[test]
fn selecting_a_promoted_resume_again_changes_nothing() {
    let body = format!(
        "## S\n\nP1.\n\nP2. <!-- #lead -->\n\n- a <!-- #scale -->\n- b <!-- #lead -->\n- c\n\n## X\n\n{ROLE}- d\n- e <!-- #lead -->\n"
    );
    let selection = promoting(&["lead", "scale"]);
    let (once, _) = assert_ok!(select(&resume(&body), &selection));
    let (twice, report) = assert_ok!(select(&once, &selection));
    assert_eq!(twice, once);
    assert!(report.reordered.is_empty());
}

// ---- the report --------------------------------------------------------------------------------------------

#[test]
fn the_report_names_each_list_that_changed_with_the_baseline_nodes_of_the_items_that_rose() {
    let body = format!("## S\n\n- a\n- b\n\n## X\n\n{ROLE}- c\n- d\n- e <!-- #lead -->\n- f <!-- #people -->\n");
    let input = resume(&body);
    let (_, report) = assert_ok!(select(&input, &promoting(&["people", "lead"])));
    let nodes = input.nodes();
    let at =
        |label: &str| {
            assert_some!(nodes.iter().position(|n| matches!(n.kind, NodeKind::Bullet)
                && crate::model::plain_text(n.content.unwrap_or_default()) == label))
        };
    let role = assert_some!(nodes.iter().position(|n| n.kind == NodeKind::Role));
    let leaf = |label: &str, from, to| MovedUp {
        node: super::NodeRef {
            kind: NodeKind::Bullet,
            label: label.into(),
            index: at(label),
        },
        from,
        to,
    };
    assert_eq!(
        report.reordered,
        vec![Reordered {
            container: super::NodeRef {
                kind: NodeKind::Role,
                label: "Acme · Engineer".into(),
                index: role,
            },
            kind: NodeKind::Bullet,
            kept: 4,
            moved_up: vec![leaf("f", 4, 1), leaf("e", 3, 2)],
        }]
    );
}

#[test]
fn positions_count_only_the_items_the_selection_kept() {
    // `x` is dropped, so `b` moves from 2nd to 1st among the kept, not from 3rd.
    let body = "## S\n\n- a\n- x <!-- #arch -->\n- b <!-- #lead -->\n";
    let selection = Selection {
        scope: Scope::Tags(assert_some!(SelectedTags::new(
            tag_list(&["lead"]).into_iter().collect()
        ))),
        ..promoting(&["lead"])
    };
    let (_, report) = ordered(body, &selection);
    let record = assert_some!(report.reordered.first());
    assert_eq!((record.kept, record.moved_up[0].from, record.moved_up[0].to), (2, 2, 1));
}

#[test]
fn a_list_already_in_promoted_order_has_no_record() {
    let (_, report) = ordered("## S\n\n- a <!-- #lead -->\n- b\n", &promoting(&["lead"]));
    assert!(report.reordered.is_empty());
}

// ---- the summary and the JSON record -----------------------------------------------------------------------

fn summary(body: &str, selection: &Selection) -> super::SelectionSummary {
    let input = resume(body);
    let (tailored, report) = assert_ok!(select(&input, selection));
    super::SelectionSummary::new(&input, &tailored, selection, &report)
}

/// Line numbers equal to node index plus 100, so a test can tell which node a line names.
fn line_of(index: usize) -> Option<usize> {
    Some(index + 100)
}

#[test]
fn the_summary_names_the_promote_list_and_each_list_where_items_rose() {
    let body = format!("## X\n\n{ROLE}- a\n- b <!-- #lead -->\n- c <!-- #people -->\n\nP1.\n\nP2. <!-- #lead -->\n");
    let sum = summary(&body, &promoting(&["people", "lead"]));
    assert!(!sum.is_silent());
    let lines = sum.lines(&line_of);
    assert_eq!(lines[0], "selection: everything; promote people, lead");
    let moved: Vec<&String> = lines.iter().filter(|l| l.starts_with("moved up")).collect();
    // Nodes: 0 section, 1 role, 2 a, 3 b, 4 c, 5 P1, 6 P2. Order c b a: `b` stays 2nd, so only `c` rose.
    assert_eq!(
        moved,
        [
            "moved up in role at line 101 \"Acme · Engineer\": 1 of 3 bullets (line 104)",
            "moved up in role at line 101 \"Acme · Engineer\": 1 of 2 paragraphs (line 106)",
        ]
    );
}

#[test]
fn a_promote_list_that_moved_nothing_says_so_and_is_not_silent() {
    let sum = summary("## S\n\n- a <!-- #lead -->\n- b\n", &promoting(&["lead"]));
    assert!(!sum.is_silent());
    assert!(sum.lines(&line_of).contains(&"moved up: nothing".to_string()));
    assert!(
        !summary("## S\n\n- a\n", &Selection::default())
            .lines(&line_of)
            .iter()
            .any(|l| l.starts_with("moved up")),
        "without promote, nothing is said about moving"
    );
}

#[test]
fn the_json_record_carries_promote_and_what_rose() {
    let sum = summary("## S\n\n- a\n- b <!-- #lead -->\n", &promoting(&["lead"]));
    let record = assert_ok!(serde_json::to_value(super::SelectionRecord::new(
        &sum, "r.md", &line_of
    )));
    assert_eq!(record["promote"], serde_json::json!(["lead"]));
    assert_eq!(
        record["reordered"],
        serde_json::json!([{
            "container": {"kind": "section", "line": 100, "label": "S"},
            "kind": "bullet",
            "kept": 2,
            "moved_up": [{"kind": "bullet", "line": 102, "label": "b", "from": 2, "to": 1}],
        }])
    );
    let plain = summary("## S\n\n- a <!-- @draft -->\n", &Selection::default());
    let record = assert_ok!(serde_json::to_value(super::SelectionRecord::new(
        &plain, "r.md", &line_of
    )));
    assert_eq!(
        (&record["promote"], &record["reordered"]),
        (&serde_json::json!([]), &serde_json::json!([]))
    );
}

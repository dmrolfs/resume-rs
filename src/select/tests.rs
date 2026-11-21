//! ABOUTME: Tests of selecting a whole resume: wordings, drafts, tags and inheritance, pruning, and the report.
//! ABOUTME: Each builds its resume by parsing Markdown, so the parser's group guarantees hold as in real use.

use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;

use super::{ChosenBy, DropReason, NodeKind, Scope, SelectError, SelectedTags, Selection, UnknownPick, select};
use crate::markdown::parse;
use crate::model::{Alternate, Block, GroupName, Inline, Resume, RoleBlock, Tag, VariantName};

const HEAD: &str = "# Ada Lovelace\n\nLondon · a@b.co\n\n";

fn resume(body: &str) -> Resume {
    let markdown = format!("{HEAD}{body}");
    let parsed = parse("r.md", &markdown);
    assert_some!(parsed.valid(), "{markdown}\n{:?}", parsed.diagnostics()).clone()
}

fn tags(names: &[&str]) -> Selection {
    let set = names.iter().map(|n| assert_ok!(Tag::try_new(*n))).collect();
    Selection {
        scope: Scope::Tags(assert_some!(SelectedTags::new(set))),
        ..Selection::default()
    }
}

fn variant(n: &str) -> VariantName {
    assert_ok!(VariantName::try_new(n))
}

fn group(n: &str) -> GroupName {
    assert_ok!(GroupName::try_new(n))
}

fn text(content: &[Inline]) -> String {
    content
        .iter()
        .map(|i| match i {
            Inline::Text { text } => text.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

/// Every leaf's text, in document order, with a kind prefix.
fn leaves(resume: &Resume) -> Vec<String> {
    let mut out = Vec::new();
    for section in &resume.sections {
        out.push(format!("## {}", section.title));
        for block in &section.blocks {
            match block {
                Block::Paragraph { content, .. } => out.push(text(content)),
                Block::Bullets { items } => out.extend(items.iter().map(|b| format!("- {}", text(&b.content)))),
                Block::Role(role) => {
                    out.push(format!("### {}", role.company));
                    if let Some(blurb) = &role.blurb {
                        out.push(format!("blurb: {}", text(blurb)));
                    }
                    for part in &role.body {
                        match part {
                            RoleBlock::Paragraph { content, .. } => out.push(text(content)),
                            RoleBlock::Bullets { items } => {
                                out.extend(items.iter().map(|b| format!("- {}", text(&b.content))));
                            }
                            RoleBlock::Era { name, items, .. } => {
                                out.push(format!("era: {name}"));
                                out.extend(items.iter().map(|b| format!("- {}", text(&b.content))));
                            }
                        }
                    }
                }
                Block::PageBreak => out.push("page-break".into()),
            }
        }
    }
    out
}

fn selected(body: &str, selection: &Selection) -> (Vec<String>, super::SelectionReport) {
    let (tailored, report) = assert_ok!(select(&resume(body), selection));
    (leaves(&tailored), report)
}

fn dropped(report: &super::SelectionReport) -> Vec<(NodeKind, String, DropReason)> {
    report
        .dropped
        .iter()
        .map(|d| (d.node.kind, d.node.label.clone(), d.reason))
        .collect()
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

const ROLE: &str = "### Acme · Engineer · 2015 – 2017\n\n";

// ---- identity ----------------------------------------------------------------------------------------------

#[test]
fn an_empty_selection_on_an_unannotated_resume_returns_it_unchanged_with_an_empty_report() {
    let body = format!("## S\n\nIntro.\n\n- A\n- B\n\n## X\n\n{ROLE}Blurb.\n\n- C\n\n**AI · 2016 – 2017**\n\n- D\n");
    let input = resume(&body);
    let (output, report) = assert_ok!(select(&input, &Selection::default()));
    assert_eq!(output, input);
    assert_eq!(report, super::SelectionReport::default());
}

// ---- wordings ----------------------------------------------------------------------------------------------

const GATEWAY: &str = "## S\n\n- Led the gateway. <!-- =gateway.default -->\n- Built it. <!-- =gateway.short -->\n- Gave control. <!-- =gateway.business -->\n- Other.\n";

#[test]
fn exactly_one_wording_renders_where_the_group_was() {
    let (out, report) = selected(GATEWAY, &Selection::default());
    assert_eq!(out, strings(&["## S", "- Led the gateway.", "- Other."]));
    assert_eq!(report.chosen.len(), 1);
    assert_eq!(report.chosen[0].by, ChosenBy::Default);
    assert_eq!(report.chosen[0].not_chosen, vec![variant("short"), variant("business")]);
}

#[test]
fn a_pick_beats_the_preference_list_and_the_preference_list_beats_default() {
    let prefer = Selection {
        prefer: vec![variant("business"), variant("short")],
        ..Selection::default()
    };
    assert_eq!(selected(GATEWAY, &prefer).0[1], "- Gave control.");
    let pick = Selection {
        picks: [(group("gateway"), variant("short"))].into(),
        ..prefer
    };
    let (out, report) = selected(GATEWAY, &pick);
    assert_eq!(out[1], "- Built it.");
    assert_eq!(report.chosen[0].by, ChosenBy::Pick);
}

#[test]
fn a_group_with_only_some_preferred_names_gets_the_first_it_has() {
    let body = "## S\n\n- Long. <!-- =g.default -->\n- Short. <!-- =g.short -->\n";
    let prefer = Selection {
        prefer: vec![variant("business"), variant("short")],
        ..Selection::default()
    };
    assert_eq!(selected(body, &prefer).0, strings(&["## S", "- Short."]));
}

#[test]
fn one_wording_renders_in_every_kind_of_container() {
    let body = format!(
        "## S\n\nP1. <!-- =p.default -->\n\nP2. <!-- =p.short -->\n\n{ROLE}- R1 <!-- =r.default -->\n- R2 <!-- =r.short -->\n\n\
         **AI · 2016 – 2017**\n\n- E1 <!-- =e.default -->\n- E2 <!-- =e.short -->\n- N\n  - N1 <!-- =n.default -->\n  - N2 <!-- =n.short -->\n"
    );
    let prefer = Selection {
        prefer: vec![variant("short")],
        ..Selection::default()
    };
    let (out, report) = selected(&body, &prefer);
    assert_eq!(
        out,
        strings(&["## S", "P2.", "### Acme", "- R2", "era: AI", "- E2", "- N", "- N2"])
    );
    assert_eq!(report.chosen.len(), 4);
}

#[test]
fn a_chosen_blurb_wording_written_as_the_next_paragraph_becomes_the_blurb() {
    let body = format!(
        "## S\n\n{ROLE}Long blurb. <!-- =intro.default -->\n\nShort blurb. <!-- =intro.short -->\n\nBody prose.\n"
    );
    let prefer = Selection {
        prefer: vec![variant("short")],
        ..Selection::default()
    };
    let (tailored, _) = assert_ok!(select(&resume(&body), &prefer));
    let Block::Role(role) = &tailored.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(text(assert_some!(role.blurb.as_ref())), "Short blurb.");
    assert_eq!(role.body.len(), 1, "only the body prose is left in the body");
    assert_eq!(
        role.blurb_annotations.alternate.as_ref().map(|a| a.variant.as_ref()),
        Some("short")
    );
    // Selecting again changes nothing.
    assert_eq!(assert_ok!(select(&tailored, &prefer)).0, tailored);
}

#[test]
fn unknown_picks_are_reported_by_group_and_by_variant() {
    let selection = Selection {
        picks: [(group("nope"), variant("short")), (group("gateway"), variant("shrot"))].into(),
        ..Selection::default()
    };
    let (out, report) = selected(GATEWAY, &selection);
    assert_eq!(out[1], "- Led the gateway.", "the typo falls back to default");
    assert_eq!(
        report.unknown_picks,
        vec![
            UnknownPick::Variant(Alternate {
                group: group("gateway"),
                variant: variant("shrot")
            }),
            UnknownPick::Group(group("nope")),
        ]
    );
}

// ---- drafts --------------------------------------------------------------------------------------------

#[test]
fn drafts_are_left_out_by_default_and_counted() {
    let body = "## S\n\n- Ready.\n- Not yet. <!-- @draft -->\n- Core but not yet. <!-- @core @draft -->\n";
    let (out, report) = selected(body, &Selection::default());
    assert_eq!(out, strings(&["## S", "- Ready."]));
    assert_eq!(report.drafts_left_out, 2);
    let on = Selection {
        drafts: true,
        ..Selection::default()
    };
    let (out, report) = selected(body, &on);
    assert_eq!(out.len(), 4);
    assert_eq!((report.drafts_left_out, report.drafts_included), (0, 2));
}

#[test]
fn a_draft_section_role_or_era_takes_everything_under_it() {
    let body = "## Draft section <!-- @draft -->\n\n- In it.\n\n## S\n\n### Acme · Engineer · 2015 – 2017 <!-- @draft -->\n\n- Under the role.\n\n\
         ### Beta · Engineer · 2013 – 2015\n\n**AI · 2014 – 2015** <!-- @draft -->\n\n- Under the era.\n\n**Data · 2013 – 2014**\n\n- Kept.\n".to_string();
    let (out, report) = selected(&body, &Selection::default());
    assert_eq!(out, strings(&["## S", "### Beta", "era: Data", "- Kept."]));
    assert_eq!(
        dropped(&report),
        vec![
            (NodeKind::Section, "Draft section".into(), DropReason::Draft),
            (NodeKind::Bullet, "In it.".into(), DropReason::UnderDraft),
            (NodeKind::Role, "Acme · Engineer".into(), DropReason::Draft),
            (NodeKind::Bullet, "Under the role.".into(), DropReason::UnderDraft),
            (NodeKind::Era, "AI".into(), DropReason::Draft),
            (NodeKind::Bullet, "Under the era.".into(), DropReason::UnderDraft),
        ]
    );
    assert_eq!(
        report.drafts_left_out, 3,
        "only the nodes marked @draft are counted as drafts"
    );
}

#[test]
fn drafts_off_preference_skips_a_draft_wording_and_reports_it() {
    let body = "## S\n\n- Long. <!-- =g.default -->\n- Short. <!-- =g.short @draft -->\n";
    let prefer = Selection {
        prefer: vec![variant("short")],
        ..Selection::default()
    };
    let (out, report) = selected(body, &prefer);
    assert_eq!(out, strings(&["## S", "- Long."]));
    assert_eq!(
        report.skipped_drafts,
        vec![Alternate {
            group: group("g"),
            variant: variant("short")
        }]
    );
}

#[test]
fn drafts_off_picks_of_draft_wordings_are_one_error_naming_every_one() {
    // Groups `z` then `a` in the document: the error lists them in that order, not by name.
    let body = "## S\n\n- A. <!-- =z.default -->\n- A2. <!-- =z.short @draft -->\n- B. <!-- =a.default -->\n- B2. <!-- =a.long @draft -->\n";
    let picks = Selection {
        picks: [(group("z"), variant("short")), (group("a"), variant("long"))].into(),
        ..Selection::default()
    };
    let error = assert_err!(select(&resume(body), &picks));
    let SelectError::PickedDraft { picks: named } = &error;
    assert_eq!(named.len(), 2);
    assert!(error.to_string().contains("`z.short`, `a.long`"), "{error}");
    // Even when the tags cut the picked group, the spec still asked for a draft.
    let cut = Selection {
        scope: tags(&["zzz"]).scope,
        picks: [(group("z"), variant("short"))].into(),
        ..Selection::default()
    };
    let error = assert_err!(select(&resume(&body.replace("<!-- =z", "<!-- #other =z")), &cut));
    assert!(
        error.to_string().contains("`z.short`") && !error.to_string().contains("`a."),
        "{error}"
    );
}

#[test]
fn drafts_off_draft_default_renders_the_first_ready_wording() {
    let body = "## S\n\n- Long. <!-- =g.default @draft -->\n- Short. <!-- =g.short -->\n";
    let (out, report) = selected(body, &Selection::default());
    assert_eq!(out, strings(&["## S", "- Short."]));
    assert_eq!(report.chosen[0].by, ChosenBy::FirstReady);
}

#[test]
fn drafts_off_a_group_with_no_ready_wording_is_dropped_and_reported() {
    let body = "## S\n\n- Long. <!-- =g.default @draft -->\n- Short. <!-- =g.short @draft -->\n- Other.\n";
    let (out, report) = selected(body, &Selection::default());
    assert_eq!(out, strings(&["## S", "- Other."]));
    assert_eq!(
        dropped(&report),
        vec![(NodeKind::Bullet, "Long.".into(), DropReason::NoReadyVariant)]
    );
}

// ---- tags, priority, inheritance -------------------------------------------------------------------------

#[test]
fn tags_inherit_from_section_role_and_era_and_untagged_items_are_general() {
    let body = format!(
        "## S <!-- #ai -->\n\n- Under an ai section.\n\n## T\n\n- General.\n- Data only. <!-- #data -->\n\n\
         {ROLE}- Plain role bullet.\n\n**Era · 2016 – 2017** <!-- #data -->\n\n- Under a data era.\n"
    );
    let (out, report) = selected(&body, &tags(&["ai"]));
    assert_eq!(
        out,
        strings(&[
            "## S",
            "- Under an ai section.",
            "## T",
            "- General.",
            "### Acme",
            "- Plain role bullet."
        ])
    );
    let reasons = dropped(&report);
    assert!(reasons.contains(&(NodeKind::Bullet, "Data only.".into(), DropReason::TagsNotSelected)));
    assert!(reasons.contains(&(
        NodeKind::Bullet,
        "Under a data era.".into(),
        DropReason::InheritedTagsOnly
    )));
    assert!(reasons.contains(&(NodeKind::Era, "Era".into(), DropReason::Empty)));
}

#[test]
fn priority_does_not_inherit_but_a_role_priority_keeps_its_blurb() {
    let body = "## S\n\n### Acme · Engineer · 2015 – 2017 <!-- @core #data -->\n\nBlurb.\n\n- Bullet.\n".to_string();
    let (out, report) = selected(&body, &tags(&["ai"]));
    assert_eq!(out, strings(&["## S", "### Acme", "blurb: Blurb."]));
    assert_eq!(
        dropped(&report),
        vec![(NodeKind::Bullet, "Bullet.".into(), DropReason::InheritedTagsOnly)]
    );
}

#[test]
fn extra_on_a_container_does_not_gate_a_core_bullet() {
    let body = "## S\n\n### Acme · Engineer · 2015 – 2017 <!-- @extra #data -->\n\n- Core. <!-- @core -->\n- Plain.\n"
        .to_string();
    assert_eq!(
        selected(&body, &tags(&["ai"])).0,
        strings(&["## S", "### Acme", "- Core."])
    );
}

#[test]
fn core_only_keeps_core_leaves_and_the_headings_above_them() {
    let body = format!("## S\n\n- Core. <!-- @core -->\n- Plain.\n\n## T\n\n{ROLE}- Plain too.\n");
    let core = Selection {
        scope: Scope::CoreOnly,
        ..Selection::default()
    };
    let (out, report) = selected(&body, &core);
    assert_eq!(out, strings(&["## S", "- Core."]));
    let reasons = dropped(&report);
    assert!(reasons.contains(&(NodeKind::Role, "Acme · Engineer".into(), DropReason::Empty)));
    assert!(reasons.contains(&(NodeKind::Section, "T".into(), DropReason::Empty)));
}

#[test]
fn a_core_section_or_role_left_with_nothing_is_dropped_and_reported() {
    let body = "## Core section <!-- @core -->\n\n- Plain.\n\n## S\n\n### Acme · Engineer · 2015 – 2017 <!-- @core -->\n\n- Plain.\n".to_string();
    let core = Selection {
        scope: Scope::CoreOnly,
        ..Selection::default()
    };
    let (out, report) = selected(&body, &core);
    assert_eq!(out, Vec::<String>::new());
    let empties: Vec<_> = dropped(&report)
        .into_iter()
        .filter(|d| d.2 == DropReason::Empty)
        .collect();
    assert_eq!(
        empties.len(),
        3,
        "the core section, the core role, and the section that held it: {empties:?}"
    );
}

#[test]
fn a_section_left_with_only_a_page_break_is_dropped() {
    let body = "## S\n\n- Plain.\n\n<div class=\"page-break\"></div>\n\n## T\n\n- Core. <!-- @core -->\n";
    let core = Selection {
        scope: Scope::CoreOnly,
        ..Selection::default()
    };
    assert_eq!(selected(body, &core).0, strings(&["## T", "- Core."]));
}

#[test]
fn a_group_is_judged_by_the_union_of_its_wordings_tags_and_the_survivor_carries_them() {
    let body = "## S\n\n- Platform. <!-- =g.default #platform -->\n- AI. <!-- =g.short #ai -->\n";
    let selection = tags(&["ai"]);
    let (tailored, report) = assert_ok!(select(&resume(body), &selection));
    assert_eq!(leaves(&tailored), strings(&["## S", "- Platform."]));
    assert_eq!(report.chosen[0].by, ChosenBy::Default);
    let Block::Bullets { items } = &tailored.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(
        items[0].annotations.tags.len(),
        2,
        "the kept wording carries the group's tags"
    );
    assert_eq!(
        assert_ok!(select(&tailored, &selection)).0,
        tailored,
        "so selecting again keeps it"
    );
}

#[test]
fn verify_items_are_kept_and_listed() {
    let body = format!("## S\n\n- Check me. <!-- @verify -->\n\n{ROLE}- Plain.\n");
    let (out, report) = selected(&body, &Selection::default());
    assert_eq!(out.len(), 4);
    assert_eq!(report.verify.len(), 1);
    assert_eq!(report.verify[0].label, "Check me.");
}

#[test]
fn a_long_label_is_cut_to_its_first_words() {
    let body = "## S\n\n- One two three four five six seven eight nine ten. <!-- @draft -->\n";
    let (_, report) = selected(body, &Selection::default());
    assert_eq!(
        report.dropped[0].node.label,
        "One two three four five six seven eight …"
    );
}

#[test]
fn an_empty_tag_set_is_not_a_selection() {
    assert!(SelectedTags::new(std::collections::BTreeSet::new()).is_none());
}

#[test]
fn an_era_under_a_draft_role_is_reported_as_under_a_draft_and_not_counted_as_one() {
    let body = "## S\n\n### Acme · Engineer · 2015 – 2017 <!-- @draft -->\n\n**AI · 2016 – 2017**\n\n- Under both.\n\n## T\n\n- Kept.\n";
    let (out, report) = selected(body, &Selection::default());
    assert_eq!(out, strings(&["## T", "- Kept."]));
    assert_eq!(
        dropped(&report),
        vec![
            (NodeKind::Role, "Acme · Engineer".into(), DropReason::Draft),
            (NodeKind::Era, "AI".into(), DropReason::UnderDraft),
            (NodeKind::Bullet, "Under both.".into(), DropReason::UnderDraft),
            (NodeKind::Section, "S".into(), DropReason::Empty),
        ]
    );
    assert_eq!(report.drafts_left_out, 1);
}

// ---- authored-empty containers, drafts on, blurb groups, verify (review fixes) --------------------------

#[test]
fn a_plain_build_keeps_a_heading_only_role_and_an_empty_section_the_author_wrote() {
    let body = format!("## Experience\n\n{ROLE}- Built it.\n\n### Startup · Founder · 2010 – 2011\n\n## Interests\n");
    let input = resume(&body);
    let (output, report) = assert_ok!(select(&input, &Selection::default()));
    assert_eq!(output, input);
    assert_eq!(report, super::SelectionReport::default());
    // A role that selection empties is still dropped.
    let (out, report) = selected(
        &body.replace("- Built it.", "- Built it. <!-- @draft -->"),
        &Selection::default(),
    );
    assert_eq!(out, strings(&["## Experience", "### Startup", "## Interests"]));
    assert!(dropped(&report).contains(&(NodeKind::Role, "Acme · Engineer".into(), DropReason::Empty)));
}

#[test]
fn drafts_on_picked_draft_wordings_render() {
    let body = "## S\n\n- A. <!-- =z.default -->\n- A2. <!-- =z.short @draft -->\n- B. <!-- =a.default -->\n- B2. <!-- =a.long @draft -->\n";
    let picks = Selection {
        picks: [(group("z"), variant("short")), (group("a"), variant("long"))].into(),
        drafts: true,
        ..Selection::default()
    };
    let (out, report) = selected(body, &picks);
    assert_eq!(out, strings(&["## S", "- A2.", "- B2."]));
    assert!(report.chosen.iter().all(|c| c.by == ChosenBy::Pick));
    assert_eq!(report.drafts_included, 2);
}

#[test]
fn drafts_on_a_draft_section_role_or_era_and_everything_under_it_render() {
    let body = "## Draft section <!-- @draft -->\n\n- In it.\n\n## S\n\n### Acme · Engineer · 2015 – 2017 <!-- @draft -->\n\n- Under the role.\n\n\
         **AI · 2016 – 2017** <!-- @draft -->\n\n- Under the era.\n";
    let on = Selection {
        drafts: true,
        ..Selection::default()
    };
    let (out, report) = selected(body, &on);
    assert_eq!(out.len(), 7, "{out:?}");
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
    assert_eq!(report.drafts_included, 3);
}

const BLURB_GROUP: &str = "Long. <!-- =intro.default -->\n\nShort. <!-- =intro.short -->\n\nBody.\n";

#[test]
fn a_blurb_group_rendering_its_default_keeps_the_body_without_the_other_wording() {
    let (out, _) = selected(&format!("## S\n\n{ROLE}{BLURB_GROUP}"), &Selection::default());
    assert_eq!(out, strings(&["## S", "### Acme", "blurb: Long.", "Body."]));
}

#[test]
fn a_draft_blurb_default_promotes_the_first_ready_wording() {
    let body = format!(
        "## S\n\n{ROLE}{}",
        BLURB_GROUP.replace("=intro.default", "=intro.default @draft")
    );
    let (out, report) = selected(&body, &Selection::default());
    assert_eq!(out, strings(&["## S", "### Acme", "blurb: Short.", "Body."]));
    assert_eq!(report.chosen[0].by, ChosenBy::FirstReady);
    assert_eq!(report.drafts_left_out, 1);
}

#[test]
fn a_blurb_group_cut_by_tags_is_reported_once_and_the_body_stays_a_body() {
    let body = format!(
        "## S\n\n{ROLE}{}",
        BLURB_GROUP.replace("=intro.default", "=intro.default #data")
    );
    let (out, report) = selected(&body, &tags(&["ai"]));
    assert_eq!(out, strings(&["## S", "### Acme", "Body."]));
    assert_eq!(
        dropped(&report),
        vec![(NodeKind::Blurb, "Long.".into(), DropReason::TagsNotSelected)]
    );
}

#[test]
fn a_blurbs_own_priority_beats_its_roles() {
    let body = "## S\n\n### Acme · Engineer · 2015 – 2017 <!-- @core -->\n\nBlurb. <!-- @extra #x -->\n\n- Core. <!-- @core -->\n";
    let core = Selection {
        scope: Scope::CoreOnly,
        ..Selection::default()
    };
    assert_eq!(selected(body, &core).0, strings(&["## S", "### Acme", "- Core."]));
}

#[test]
fn verify_is_listed_for_kept_containers_and_chosen_wordings_only() {
    let body = "## S <!-- @verify -->\n\n- L. <!-- =g.default -->\n- V. <!-- =g.short @verify -->\n- Cut. <!-- @verify #data -->\n";
    let (_, report) = selected(body, &tags(&["ai"]));
    let listed: Vec<_> = report.verify.iter().map(|n| (n.kind, n.label.as_str())).collect();
    assert_eq!(
        listed,
        vec![(NodeKind::Section, "S")],
        "a not-chosen and a dropped @verify are not listed"
    );
    let prefer = Selection {
        prefer: vec![variant("short")],
        ..Selection::default()
    };
    let (_, report) = selected(body, &prefer);
    let listed: Vec<_> = report.verify.iter().map(|n| n.label.as_str()).collect();
    assert_eq!(listed, vec!["V.", "Cut.", "S"]);
}

#[test]
fn skipped_draft_wordings_are_reported_only_for_groups_that_render() {
    let body = "## AI <!-- #ai -->\n\n- Long. <!-- =g.default -->\n- Short. <!-- =g.short @draft -->\n";
    let prefer = |scope| Selection {
        scope,
        prefer: vec![variant("short")],
        ..Selection::default()
    };
    assert_eq!(selected(body, &prefer(Scope::All)).1.skipped_drafts.len(), 1);
    assert!(
        selected(body, &prefer(tags(&["data"]).scope))
            .1
            .skipped_drafts
            .is_empty()
    );
}

// ---- summary and inventory (rsm-141.22.3) ------------------------------------------------------------------

use super::{CorePages, Inventory, SelectionSummary};

fn summary(body: &str, selection: &Selection) -> SelectionSummary {
    let input = resume(body);
    let (tailored, report) = assert_ok!(select(&input, selection));
    SelectionSummary::new(&input, &tailored, selection, &report)
}

#[test]
fn the_summary_is_silent_for_a_default_selection_that_changed_nothing() {
    let quiet = summary(&format!("## S\n\n{ROLE}- A\n- B\n"), &Selection::default());
    assert!(quiet.is_silent());
    assert_eq!(quiet.lines(&|_| None), Vec::<String>::new());
    // Defaults chosen in groups say nothing new either.
    assert!(summary(GATEWAY, &Selection::default()).is_silent());
    // A draft left out, a flag, or a @verify item speaks.
    assert!(!summary("## S\n\n- A <!-- @draft -->\n- B\n", &Selection::default()).is_silent());
    assert!(!summary("## S\n\n- A <!-- @verify -->\n", &Selection::default()).is_silent());
    assert!(!summary("## S\n\n- A\n", &tags(&["x"])).is_silent());
}

#[test]
fn the_summary_counts_routine_wordings_and_lists_first_ready_fallbacks() {
    let body = "## S\n\n- A <!-- =a.default @draft -->\n- A2 <!-- =a.short -->\n- B <!-- =b.default -->\n- B2 <!-- =b.short -->\n";
    let lines = summary(body, &Selection::default()).lines(&|i| Some(i + 100));
    assert!(
        lines.contains(&"wordings: 2 chosen (1 default, 1 first-ready)".to_string()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"wording a.short: its default is a draft, so the first ready wording is used".to_string()),
        "{lines:?}"
    );
    let mut core = summary(body, &Selection::default());
    core.core = Some(CorePages::Measured { pages: 2 });
    assert_eq!(
        core.lines(&|_| None).last().map(String::as_str),
        Some("core alone: 2 pages")
    );
}

#[test]
fn the_inventory_counts_own_and_inherited_tags_and_marks_drafts_under_a_container() {
    let body = "## S <!-- #ai -->\n\n- A <!-- #ai -->\n- B\n\n## T\n\n### Acme · Eng · 2015 – 2017 <!-- @draft -->\n\n- C <!-- =g.default -->\n- D <!-- =g.short -->\n".to_string();
    let input = resume(&body);
    let lines: Vec<usize> = (1..=input.nodes().len()).collect();
    let inventory = Inventory::of(&input, &lines);
    assert_eq!(inventory.tags.len(), 1);
    assert_eq!(inventory.tags[0].own.len(), 2, "the section and A");
    assert_eq!(inventory.tags[0].inherited, 1, "B inherits; A carries it itself");
    assert!(
        inventory.groups[0].variants.iter().all(|v| v.draft),
        "under a draft role"
    );
    assert_eq!(
        inventory.keywords.iter().map(|k| k.name).collect::<Vec<_>>(),
        ["core", "extra", "draft", "verify"]
    );
}

#[test]
fn the_inventory_does_not_carry_tags_or_drafts_across_siblings() {
    let body = "## S <!-- #ai -->\n\n### Acme · Eng · 2015 – 2017 <!-- @draft #data -->\n\n- In draft role. <!-- =g.default -->\n- Its short. <!-- =g.short -->\n\n\
         ### Beta · Eng · 2013 – 2015\n\n- After it. <!-- =h.default -->\n- After short. <!-- =h.short -->\n\n## T\n\n- Untagged.\n".to_string();
    let input = resume(&body);
    let lines: Vec<usize> = (1..=input.nodes().len()).collect();
    let inventory = Inventory::of(&input, &lines);
    let tag = |name: &str| assert_some!(inventory.tags.iter().find(|t| t.name == name)).clone();
    // #ai: the section owns it; both roles and the four bullets under it inherit it; T's bullet does not.
    assert_eq!((tag("ai").own.len(), tag("ai").inherited), (1, 6));
    // #data: the Acme role owns it; only its two bullets inherit it.
    assert_eq!((tag("data").own.len(), tag("data").inherited), (1, 2));
    let drafts = |group: &str| -> Vec<bool> {
        assert_some!(inventory.groups.iter().find(|g| g.name == group))
            .variants
            .iter()
            .map(|v| v.draft)
            .collect()
    };
    assert_eq!(drafts("g"), [true, true], "under the draft role");
    assert_eq!(drafts("h"), [false, false], "the next role is not under it");
}

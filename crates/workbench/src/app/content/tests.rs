use super::*;
use ::resume::{markdown, select, spec};
use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use std::path::Path;

fn prepared(text: &str) -> ::resume::pipeline::PreparedResume {
    let parsed = markdown::parse("resume.md", text);
    let baseline = assert_some!(parsed.valid(), "{:?}", parsed.diagnostics());
    let origins: Vec<_> = parsed
        .node_ranges()
        .iter()
        .map(|range| ItemOrigin::Baseline {
            file: "resume.md".into(),
            range: range.clone(),
        })
        .collect();
    assert_ok!(spec::items::assemble_resume(baseline, &origins, text.len(), &[], None))
}

#[test]
fn no_spec_keeps_all_authored_drafts_and_alternatives_unselected() {
    let p = prepared(
        "# Ada\n\n## Work\n\n- Default <!-- =work.default @core -->\n- Draft <!-- =work.short @core @draft -->\n",
    );
    let content = Content::resume(&p, None);
    assert_eq!(content.items.len(), 3);
    assert!(content.items.iter().all(|i| i.outcome == Outcome::Unselected));
    assert!(content.items[2].chips().contains(&"@draft".to_owned()));
    assert!(!content.assessed);
}

#[test]
fn grouped_drop_propagates_report_reason_to_every_variant() {
    let p = prepared("# Ada\n\n## Work\n\n- Default <!-- =work.default -->\n- Short <!-- =work.short -->\n");
    let selection = select::Selection {
        scope: select::Scope::CoreOnly,
        ..Default::default()
    };
    let (selected, report) = assert_ok!(select::select(p.resume(), &selection));
    assert_eq!(report.dropped.len(), 2);
    let content = Content::resume(&p, Some((&selected, &report)));
    assert_eq!(content.items[1].outcome, Outcome::Dropped(select::DropReason::CoreOnly));
    assert_eq!(content.items[2].outcome, content.items[1].outcome);
    assert_eq!(content.items[0].outcome, Outcome::Dropped(select::DropReason::Empty));
}

#[test]
fn chosen_variant_is_not_confused_with_ordinary_drop_or_duplicate_text() {
    let p = prepared(
        "# Ada\n\n## Work\n\n- Same <!-- =work.default @core -->\n- Same <!-- =work.short @core -->\n- Same <!-- @extra -->\n",
    );
    let selection = select::Selection {
        scope: select::Scope::CoreOnly,
        prefer: vec![assert_ok!(::resume::model::VariantName::try_new("short"))],
        ..Default::default()
    };
    let (selected, report) = assert_ok!(select::select(p.resume(), &selection));
    let content = Content::resume(&p, Some((&selected, &report)));
    assert_eq!(content.items[1].outcome, Outcome::NotChosen);
    assert_eq!(content.items[2].outcome, Outcome::Chosen(select::ChosenBy::Preference));
    assert_eq!(content.items[3].outcome, Outcome::Dropped(select::DropReason::CoreOnly));
    assert_eq!(content.visible(Filter::Kept), vec![(0, false), (2, false)]);
    assert_eq!(
        content.visible(Filter::Dropped),
        vec![(0, true), (1, false), (3, false)]
    );
}

#[test]
fn draft_ancestor_group_reports_and_own_plus_inherited_tags_match_engine() {
    let p = prepared(
        "# Ada\n\n## Drafts <!-- @draft -->\n\n- One <!-- =group.default -->\n- Two <!-- =group.short -->\n\n## Ready <!-- #platform -->\n\n- Tagged <!-- #other -->\n",
    );
    let selection = select::Selection {
        scope: select::Scope::Tags(assert_some!(select::SelectedTags::new(
            [assert_ok!(::resume::model::Tag::try_new("platform"))]
                .into_iter()
                .collect()
        ))),
        ..Default::default()
    };
    let (selected, report) = assert_ok!(select::select(p.resume(), &selection));
    let content = Content::resume(&p, Some((&selected, &report)));
    assert_eq!(
        content.items[1].outcome,
        Outcome::Dropped(select::DropReason::UnderDraft)
    );
    assert_eq!(content.items[2].outcome, content.items[1].outcome);
    assert_eq!(content.items[4].outcome, Outcome::Kept);
    assert!(content.items[4].chips().contains(&"#platform (inherited)".to_owned()));
    assert!(content.items[4].chips().contains(&"#other".to_owned()));
}

#[test]
fn explicit_membership_and_unready_groups_use_engine_drop_reasons() {
    for (text, selection, expected) in [
        (
            "# Ada\n\n## Work\n\n- Default <!-- =work.default -->\n- Short <!-- =work.short -->\n",
            select::Selection {
                groups: Some(Default::default()),
                ..Default::default()
            },
            select::DropReason::NotListed,
        ),
        (
            "# Ada\n\n## Work\n\n- Default <!-- =work.default @draft -->\n- Short <!-- =work.short @draft -->\n",
            select::Selection::default(),
            select::DropReason::NoReadyVariant,
        ),
    ] {
        let p = prepared(text);
        let (selected, report) = assert_ok!(select::select(p.resume(), &selection));
        let content = Content::resume(&p, Some((&selected, &report)));
        assert_eq!(content.items.len(), p.resume().nodes().len());
        assert_eq!(content.items[1].outcome, Outcome::Dropped(expected));
        assert_eq!(content.items[2].outcome, Outcome::Dropped(expected));
        assert!(content.items.iter().all(|i| i.outcome != Outcome::Unavailable));
    }
}

#[test]
fn letter_frames_and_unresolved_fields_remain_visible_without_spec() {
    let parsed = markdown::parse_letter(
        Path::new("letter.md"),
        "# Ada\n\n## Greeting\n\nDear {{recipient}},\n\n## Body\n\nFor {{company}}. <!-- @core -->\n\n## Closing\n\nRegards,\n",
    );
    let source = assert_some!(parsed.valid(), "{:?}", parsed.diagnostics());
    let content = Content::letter(source, None);
    assert_eq!(content.items.len(), 3);
    assert!(content.items[0].mandatory);
    assert!(content.items[2].mandatory);
    assert!(content.items[1].text.contains("{{company}}"));
    assert!(content.items.iter().all(|i| i.outcome == Outcome::Unselected));
}

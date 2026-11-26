//! Exact group membership is independent of tags and wording choice.
use super::tests::{group, leaves, resume, tags, variant};
use super::*;
use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;

fn members(names: &[&str]) -> Selection {
    Selection {
        groups: Some(names.iter().map(|n| group(n)).collect()),
        ..Selection::default()
    }
}

#[test]
fn exact_membership_overrides_tags_and_core_but_not_ungrouped_rules() {
    let input = resume(
        "## Summary\n\n- Chosen. <!-- =yes.default #other @extra -->\n- Core. <!-- =no.default @core -->\n- General.\n- Filtered. <!-- #other -->\n",
    );
    let selection = Selection {
        groups: members(&["yes"]).groups,
        ..tags(&["wanted"])
    };
    let (out, report) = assert_ok!(select(&input, &selection));
    assert_eq!(leaves(&out), ["## Summary", "- Chosen.", "- General."]);
    assert_eq!(report.chosen.len(), 1);
    assert_eq!(report.chosen[0].group, group("yes"));
    assert_eq!(report.excluded_core.len(), 1);
    assert!(report.dropped.iter().any(|d| d.reason == DropReason::NotListed));
    let summary = SelectionSummary::new(&input, &out, &selection, &report);
    assert!(
        summary
            .lines(&|_| None)
            .iter()
            .any(|l| l.contains("core group excluded"))
    );
}

#[test]
fn empty_membership_keeps_general_text_and_prunes_empty_sections() {
    let input = resume("## Summary\n\nGeneral.\n\n## Projects\n\n- Gone. <!-- =g.default -->\n");
    let (out, report) = assert_ok!(select(&input, &members(&[])));
    assert_eq!(leaves(&out), ["## Summary", "General."]);
    assert!(report.chosen.is_empty());
    assert!(report.dropped.iter().any(|d| d.reason == DropReason::Empty));
    assert!(selection_line(&members(&[])).contains("groups none; ungrouped: everything"));
}

#[test]
fn membership_does_not_change_pick_precedence_or_enable_an_unlisted_pick() {
    let input = resume(
        "## Summary\n\n- Default. <!-- =a.default #same -->\n- Short. <!-- =a.short #same -->\n- Other. <!-- =b.default #same -->\n",
    );
    let selection = Selection {
        prefer: vec![variant("default")],
        picks: [(group("a"), variant("short")), (group("b"), variant("default"))].into(),
        ..members(&["a"])
    };
    let (out, report) = assert_ok!(select(&input, &selection));
    assert_eq!(leaves(&out), ["## Summary", "- Short."]);
    assert_eq!(report.chosen[0].by, ChosenBy::Pick);
    assert!(report.unknown_picks.is_empty());
}

#[test]
fn unknown_requests_are_sorted_errors_not_partial_success() {
    let input = resume("## Summary\n\nText. <!-- =known.default -->\n");
    assert_eq!(
        assert_err!(select(&input, &members(&["z", "known", "a"]))),
        SelectError::UnknownGroups {
            groups: vec![group("a"), group("z")]
        }
    );
}

#[test]
fn unavailable_requests_error_and_draft_preview_can_satisfy_them() {
    for body in [
        "## Summary\n\nText. <!-- =g.default @draft -->\n",
        "## Summary <!-- @draft -->\n\nText. <!-- =g.default -->\n",
        "## Experience\n\n### Acme · Engineer · 2020–2024 <!-- @draft -->\n\n- Text. <!-- =g.default -->\n",
        "## Experience\n\n### Acme · Engineer · 2020–2024\n\n**Era · 2020–2024** <!-- @draft -->\n\n- Text. <!-- =g.default -->\n",
    ] {
        let input = resume(body);
        let error = assert_err!(select(&input, &members(&["g"])));
        let SelectError::UnavailableGroups { groups } = error else {
            panic!("{error}")
        };
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group, group("g"));
        assert!(matches!(
            groups[0].reason,
            DropReason::NoReadyVariant | DropReason::UnderDraft
        ));
        let (_, report) = assert_ok!(select(
            &input,
            &Selection {
                drafts: true,
                ..members(&["g"])
            }
        ));
        assert_eq!(report.chosen.len(), 1);
    }
}

#[test]
fn failures_are_aggregated_in_document_order() {
    let input = resume("## Summary\n\n- Z. <!-- =z.default @draft -->\n- A. <!-- =a.default @draft -->\n");
    let error = assert_err!(select(&input, &members(&["a", "z"])));
    let SelectError::UnavailableGroups { groups } = error else {
        panic!("{error}")
    };
    assert_eq!(groups.iter().map(|g| g.group.as_ref()).collect::<Vec<_>>(), ["z", "a"]);
}

#[test]
fn inherited_core_blurb_can_be_excluded_without_removing_a_selected_bullet() {
    let input = resume(
        "## Experience\n\n### Acme · Engineer · 2020–2024 <!-- @core -->\n\nBlurb. <!-- =blurb.default -->\n\n- Work. <!-- =work.default #other -->\n",
    );
    let (out, report) = assert_ok!(select(
        &input,
        &Selection {
            scope: Scope::CoreOnly,
            ..members(&["work"])
        }
    ));
    assert_eq!(leaves(&out), ["## Experience", "### Acme", "- Work."]);
    assert_eq!(report.excluded_core.len(), 1);
}

#[test]
fn ungrouped_blurb_is_not_forced_in_by_selected_bullet() {
    let input = resume(
        "## Experience\n\n### Acme · Engineer · 2020–2024\n\nBlurb. <!-- #other -->\n\n- Work. <!-- =work.default #other -->\n",
    );
    let (out, _) = assert_ok!(select(
        &input,
        &Selection {
            groups: members(&["work"]).groups,
            ..tags(&["wanted"])
        }
    ));
    assert_eq!(leaves(&out), ["## Experience", "### Acme", "- Work."]);
}

#[test]
fn unlisted_draft_picks_still_error() {
    let input = resume("## Summary\n\nText. <!-- =g.default @draft -->\n");
    let error = assert_err!(select(
        &input,
        &Selection {
            picks: [(group("g"), variant("default"))].into(),
            ..members(&[])
        }
    ));
    assert!(matches!(error, SelectError::PickedDraft { .. }));
}

#[test]
fn promotion_and_nondefault_blurbs_preserve_exact_membership() {
    let input = resume(
        "## Experience\n\n### Acme · Engineer · 2020–2024\n\nDefault. <!-- =blurb.default -->\n\nShort. <!-- =blurb.short -->\n\n- First. <!-- =a.default -->\n- Second. <!-- =b.default #rise -->\n",
    );
    let selection = Selection {
        picks: [(group("blurb"), variant("short"))].into(),
        promote: vec![claims::assert_ok!(crate::model::Tag::try_new("rise"))],
        ..members(&["a", "b", "blurb"])
    };
    let (out, report) = assert_ok!(select(&input, &selection));
    assert_eq!(
        leaves(&out),
        ["## Experience", "### Acme", "blurb: Short.", "- Second.", "- First."]
    );
    assert_eq!(report.chosen.len(), 3);
    assert_eq!(report.reordered.len(), 1);
}

#[test]
fn selection_json_distinguishes_no_list_and_empty_list() {
    let input = resume("## Summary\n\nGeneral.\n");
    for selection in [Selection::default(), members(&[])] {
        let (out, report) = assert_ok!(select(&input, &selection));
        let summary = SelectionSummary::new(&input, &out, &selection, &report);
        let json = assert_ok!(serde_json::to_value(SelectionRecord::new(&summary, "r.md", &|_| None)));
        assert_eq!(
            json["groups"],
            if selection.groups.is_some() {
                serde_json::json!([])
            } else {
                serde_json::Value::Null
            }
        );
    }
}

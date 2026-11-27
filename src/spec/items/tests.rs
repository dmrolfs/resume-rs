//! ABOUTME: Tests of applying per-item overrides to a copy of the baseline: what changes, what must not, what is refused.
//! ABOUTME: Overrides are built the way merging builds them, then applied to a parsed resume or letter.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;

use super::*;
use crate::letter::SourceItem;
use crate::markdown::{parse, parse_letter};
use crate::model::PriorityEdit;
use crate::spec::merge::{ChoiceKey, ItemOverride};

const HEAD: &str = "# Ada Lovelace\n\nLondon · a@b.co\n\n";

fn item(name: &str) -> ItemId {
    assert_ok!(ItemId::try_new(name))
}

fn tag(name: &str) -> Tag {
    assert_ok!(Tag::try_new(name))
}

fn tags(names: &[&str]) -> BTreeSet<Tag> {
    names.iter().map(|n| tag(n)).collect()
}

fn at(file: &str, line: usize) -> Located {
    Located {
        path: PathBuf::from(file).into(),
        line,
        column: 1,
    }
}

fn baseline(body: &str) -> Resume {
    let parsed = parse("r.md", &format!("{HEAD}{body}"));
    assert_eq!(parsed.diagnostics(), [], "{body}");
    assert_some!(parsed.valid()).clone()
}

/// One override and where its fields were written, as merging would hand them over.
fn change(
    target: &str,
    tags_set: Option<&[&str]>,
    priority: Option<PriorityEdit>,
) -> (BTreeMap<ItemId, ItemOverride>, BTreeMap<ChoiceKey, Located>) {
    let mut origins = BTreeMap::new();
    if tags_set.is_some() {
        origins.insert(ChoiceKey::OverrideTags(item(target)), at("acme.toml", 7));
    }
    if priority.is_some() {
        origins.insert(ChoiceKey::OverridePriority(item(target)), at("acme.toml", 8));
    }
    let overrides = BTreeMap::from([(
        item(target),
        ItemOverride {
            tags: tags_set.map(tags),
            priority,
        },
    )]);
    (overrides, origins)
}

fn prepared(resume: &Resume, target: &str, tags_set: Option<&[&str]>, priority: Option<PriorityEdit>) -> Resume {
    let (overrides, origins) = change(target, tags_set, priority);
    assert_ok!(prepare_resume(resume, ItemOverrides::new(&overrides, &origins)))
}

/// The own annotations of every node, in `nodes()` order.
fn notes(mut resume: Resume) -> Vec<(Option<String>, Vec<String>, Option<String>)> {
    resume
        .node_annotations_mut()
        .into_iter()
        .map(|a| {
            (
                a.id.as_deref().map(ToString::to_string),
                a.tags.iter().map(ToString::to_string).collect(),
                a.priority.map(|p| p.to_string()),
            )
        })
        .collect()
}

const BULLETS: &str = "## S\n\n- First. <!-- &first @core #ai -->\n- Second. <!-- &second #data -->\n- Third.\n";

#[test]
fn an_override_replaces_the_own_tags_of_the_named_item_and_no_other() {
    let after = notes(prepared(&baseline(BULLETS), "first", Some(&["platform"]), None));
    assert_eq!(after[1].1, ["platform"], "first's tags are replaced, not added to");
    assert_eq!(after[2].1, ["data"], "second is untouched");
    assert!(after[3].1.is_empty());
}

#[test]
fn an_empty_tag_set_clears_the_own_tags() {
    let after = notes(prepared(&baseline(BULLETS), "first", Some(&[]), None));
    assert!(after[1].1.is_empty(), "{after:?}");
}

#[test]
fn a_priority_override_sets_core_or_extra_and_none_clears_it() {
    let base = baseline(BULLETS);
    assert_eq!(
        notes(prepared(&base, "second", None, Some(PriorityEdit::Core)))[2]
            .2
            .as_deref(),
        Some("core")
    );
    assert_eq!(
        notes(prepared(&base, "second", None, Some(PriorityEdit::Extra)))[2]
            .2
            .as_deref(),
        Some("extra")
    );
    assert_eq!(
        notes(prepared(&base, "first", None, Some(PriorityEdit::None)))[1].2,
        None
    );
}

#[test]
fn a_field_the_override_does_not_name_is_left_as_written() {
    let after = notes(prepared(&baseline(BULLETS), "first", Some(&["platform"]), None));
    assert_eq!(after[1].2.as_deref(), Some("core"), "priority stays");
    let after = notes(prepared(&baseline(BULLETS), "first", None, Some(PriorityEdit::Extra)));
    assert_eq!(after[1].1, ["ai"], "tags stay");
}

#[test]
fn the_ids_are_kept_so_a_second_application_would_find_the_same_targets() {
    let after = notes(prepared(
        &baseline(BULLETS),
        "first",
        Some(&[]),
        Some(PriorityEdit::None),
    ));
    assert_eq!(after[1].0.as_deref(), Some("first"));
}

#[test]
fn a_container_id_changes_the_container_and_nothing_under_it() {
    let base = baseline("## S <!-- &section #ai -->\n\n- A. <!-- #ai -->\n");
    let after = notes(prepared(&base, "section", Some(&["data"]), Some(PriorityEdit::Core)));
    assert_eq!(
        (after[0].1.clone(), after[0].2.clone()),
        (vec!["data".to_string()], Some("core".to_string()))
    );
    assert_eq!(after[1].1, ["ai"], "a child keeps its own tags");
    assert_eq!(after[1].2, None, "priority does not pass down");
}

#[test]
fn node_count_order_and_kinds_do_not_change() {
    let base = baseline(BULLETS);
    let after = prepared(&base, "first", Some(&["x"]), Some(PriorityEdit::Extra));
    let kinds = |r: &Resume| r.nodes().iter().map(|n| n.kind).collect::<Vec<_>>();
    assert_eq!(kinds(&base), kinds(&after));
    assert_eq!(
        assert_ok!(serde_json::to_string(&base)),
        assert_ok!(serde_json::to_string(&after)),
        "overrides never reach what a style reads"
    );
}

#[test]
fn the_baseline_is_not_changed() {
    let base = baseline(BULLETS);
    let before = notes(base.clone());
    prepared(&base, "first", Some(&[]), Some(PriorityEdit::None));
    assert_eq!(notes(base), before);
}

#[test]
fn a_group_is_addressed_by_its_group_name_and_every_variant_changes() {
    let base = baseline("## S\n\n- A. <!-- =gateway.default @core #ai -->\n- B. <!-- =gateway.short @core #data -->\n");
    let after = notes(prepared(
        &base,
        "gateway",
        Some(&["platform"]),
        Some(PriorityEdit::Extra),
    ));
    assert_eq!(after[1].1, ["platform"]);
    assert_eq!(after[2].1, ["platform"]);
    assert_eq!(after[1].2.as_deref(), Some("extra"));
    assert_eq!(after[2].2.as_deref(), Some("extra"));
}

#[test]
fn a_group_override_keeps_each_variants_status() {
    let mut base = baseline("## S\n\n- A. <!-- =gateway.default -->\n- B. <!-- =gateway.short @draft -->\n");
    let before: Vec<_> = base.node_annotations_mut().iter().map(|a| a.status).collect();
    let mut after = prepared(&base, "gateway", Some(&["ai"]), Some(PriorityEdit::Core));
    let statuses: Vec<_> = after.node_annotations_mut().iter().map(|a| a.status).collect();
    assert_eq!(before, statuses);
}

#[test]
fn a_target_the_document_lacks_is_refused_naming_every_line_that_wrote_it() {
    let base = baseline(BULLETS);
    let (overrides, origins) = change("nope", Some(&["ai"]), Some(PriorityEdit::Core));
    let error = assert_err!(prepare_resume(&base, ItemOverrides::new(&overrides, &origins)));
    let PrepareError::UnknownTargets { missing } = &error else {
        panic!("expected missing targets: {error:?}")
    };
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].id, item("nope"));
    assert_eq!(
        missing[0].at.iter().map(|l| l.line).collect::<Vec<_>>(),
        [7, 8],
        "tags then priority"
    );
    assert!(error.to_string().contains("acme.toml:7:1"), "{error}");
    assert!(error.to_string().contains("`&nope`"), "{error}");
}

#[test]
fn every_missing_target_is_reported_together_in_id_order_and_nothing_is_half_applied() {
    let base = baseline(BULLETS);
    let mut overrides = BTreeMap::new();
    let mut origins = BTreeMap::new();
    for (name, line) in [("zeta", 3), ("alpha", 9), ("first", 5)] {
        overrides.insert(
            item(name),
            ItemOverride {
                tags: Some(tags(&["x"])),
                priority: None,
            },
        );
        origins.insert(ChoiceKey::OverrideTags(item(name)), at("acme.toml", line));
    }
    let error = assert_err!(prepare_resume(&base, ItemOverrides::new(&overrides, &origins)));
    let PrepareError::UnknownTargets { missing } = error else {
        panic!("expected missing targets")
    };
    assert_eq!(
        missing.iter().map(|m| m.id.to_string()).collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
}

#[test]
fn a_target_inside_a_subtree_selection_would_drop_is_still_checked() {
    // The role is a draft, so it will never be kept, but a typo in its override must still be found.
    let base = baseline("## S\n\n### Acme · 2015 – 2020 <!-- @draft -->\n\n- A. <!-- &real -->\n");
    let (overrides, origins) = change("typo", Some(&["ai"]), None);
    assert_err!(prepare_resume(&base, ItemOverrides::new(&overrides, &origins)));
    let (ok_overrides, ok_origins) = change("real", Some(&["ai"]), None);
    assert_ok!(prepare_resume(&base, ItemOverrides::new(&ok_overrides, &ok_origins)));
}

#[test]
fn no_overrides_gives_an_identical_copy() {
    let base = baseline(BULLETS);
    let (overrides, origins) = (BTreeMap::new(), BTreeMap::new());
    assert_eq!(
        assert_ok!(prepare_resume(&base, ItemOverrides::new(&overrides, &origins))),
        base
    );
}

#[test]
fn an_address_is_by_id_not_by_position_so_rewording_and_reordering_keep_the_target() {
    let a = baseline("## S\n\n- Old words. <!-- &key -->\n- Other.\n");
    let b = baseline("## S\n\n- Other.\n- Completely new words. <!-- &key #ai -->\n");
    for base in [a, b] {
        let after = notes(prepared(&base, "key", Some(&["platform"]), None));
        assert!(
            after
                .iter()
                .any(|n| n.0.as_deref() == Some("key") && n.1 == ["platform"]),
            "{after:?}"
        );
    }
}

// ---- letters --------------------------------------------------------------------------------------------

fn letter(body: &str) -> LetterSource {
    let text = format!("# Alex\n\n## Greeting\n\nDear team,\n\n## Body\n\n{body}\n\n## Closing\n\nSincerely,\n");
    let parsed = parse_letter("l.md", &text);
    assert_eq!(parsed.diagnostics(), [], "{body}");
    assert_some!(parsed.valid()).clone()
}

fn body_notes(mut source: LetterSource) -> Vec<(Vec<String>, Option<String>)> {
    source
        .body_annotations_mut()
        .into_iter()
        .map(|a| {
            (
                a.tags.iter().map(ToString::to_string).collect(),
                a.priority.map(|p| p.to_string()),
            )
        })
        .collect()
}

#[test]
fn a_letter_body_paragraph_is_addressed_by_its_id() {
    let source = letter("One. <!-- &opening #ai -->\n\nTwo. <!-- &evidence -->");
    let (overrides, origins) = change("opening", Some(&["platform"]), Some(PriorityEdit::Core));
    let after = assert_ok!(prepare_letter(&source, ItemOverrides::new(&overrides, &origins)));
    let notes = body_notes(after);
    assert_eq!(notes[0], (vec!["platform".to_string()], Some("core".to_string())));
    assert_eq!(notes[1], (vec![], None));
}

#[test]
fn a_letter_group_changes_every_variant() {
    let source = letter("One. <!-- =x.default #ai -->\n\nTwo. <!-- =x.short #data -->");
    let (overrides, origins) = change("x", Some(&["platform"]), None);
    let notes = body_notes(assert_ok!(prepare_letter(
        &source,
        ItemOverrides::new(&overrides, &origins)
    )));
    assert_eq!(
        notes,
        vec![
            (vec!["platform".to_string()], None),
            (vec!["platform".to_string()], None)
        ]
    );
}

const FRAMED: &str = "# Alex\n\n## Greeting\n\nHi. <!-- =hello.default -->\n\nHello. <!-- =hello.short -->\n\n## Body\n\nOne. <!-- &real -->\n\n## Closing\n\nBye. <!-- =bye.default -->\n\nCheers. <!-- =bye.short -->\n";

#[test]
fn a_letter_frame_group_is_a_typed_frame_target_error_not_an_unknown_target() {
    let source = assert_some!(parse_letter("l.md", FRAMED).valid()).clone();
    let (overrides, origins) = change("hello", Some(&["ai"]), None);
    let error = assert_err!(prepare_letter(&source, ItemOverrides::new(&overrides, &origins)));
    let PrepareError::FrameTargets { frames } = &error else {
        panic!("expected a frame-target error, got {error:?}")
    };
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].id, item("hello"));
    assert_eq!(frames[0].section, "greeting");
    assert!(error.to_string().contains("greeting"), "{error}");
    assert!(error.to_string().contains("`hello`"), "{error}");
    assert!(
        !error.to_string().contains("no item"),
        "not claimed to be missing: {error}"
    );
}

#[test]
fn a_closing_frame_group_is_named_as_the_closing() {
    let source = assert_some!(parse_letter("l.md", FRAMED).valid()).clone();
    let (overrides, origins) = change("bye", None, Some(PriorityEdit::Core));
    let PrepareError::FrameTargets { frames } =
        assert_err!(prepare_letter(&source, ItemOverrides::new(&overrides, &origins)))
    else {
        panic!("expected a frame-target error")
    };
    assert_eq!(frames[0].section, "closing");
}

#[test]
fn frame_targets_and_missing_targets_are_all_reported_none_is_lost() {
    let source = assert_some!(parse_letter("l.md", FRAMED).valid()).clone();
    let mut overrides = BTreeMap::new();
    let mut origins = BTreeMap::new();
    for (name, line) in [("hello", 3), ("nope", 4), ("bye", 5)] {
        overrides.insert(
            item(name),
            ItemOverride {
                tags: Some(tags(&["x"])),
                priority: None,
            },
        );
        origins.insert(ChoiceKey::OverrideTags(item(name)), at("acme.toml", line));
    }
    let error = assert_err!(prepare_letter(&source, ItemOverrides::new(&overrides, &origins)));
    let PrepareError::Targets { frames, missing } = &error else {
        panic!("expected both kinds: {error:?}")
    };
    assert_eq!(
        frames.iter().map(|f| f.id.to_string()).collect::<Vec<_>>(),
        ["bye", "hello"]
    );
    assert_eq!(missing.iter().map(|m| m.id.to_string()).collect::<Vec<_>>(), ["nope"]);
    let text = error.to_string();
    assert!(
        text.contains("`hello`") && text.contains("`bye`") && text.contains("`&nope`"),
        "{text}"
    );
}

#[test]
fn the_contributing_lines_are_listed_in_the_order_they_were_written() {
    // priority on line 4 (a profile), tags on line 9 (the spec): file order is not tags-before-priority.
    let base = baseline(BULLETS);
    let id = item("nope");
    let overrides = BTreeMap::from([(
        id.clone(),
        ItemOverride {
            tags: Some(tags(&["x"])),
            priority: Some(PriorityEdit::Core),
        },
    )]);
    let origins = BTreeMap::from([
        (ChoiceKey::OverridePriority(id.clone()), at("a-profile.toml", 4)),
        (ChoiceKey::OverrideTags(id), at("acme.toml", 9)),
    ]);
    let PrepareError::UnknownTargets { missing } =
        assert_err!(prepare_resume(&base, ItemOverrides::new(&overrides, &origins)))
    else {
        panic!("expected missing targets")
    };
    let places: Vec<(String, usize)> = missing[0]
        .at
        .iter()
        .map(|l| (l.path.display().to_string(), l.line))
        .collect();
    assert_eq!(
        places,
        [("a-profile.toml".to_string(), 4), ("acme.toml".to_string(), 9)]
    );
    let reversed = BTreeMap::from([
        (ChoiceKey::OverrideTags(item("nope")), at("acme.toml", 5)),
        (ChoiceKey::OverridePriority(item("nope")), at("acme.toml", 8)),
    ]);
    let PrepareError::UnknownTargets { missing } =
        assert_err!(prepare_resume(&base, ItemOverrides::new(&overrides, &reversed)))
    else {
        panic!("expected missing targets")
    };
    assert_eq!(missing[0].at.iter().map(|l| l.line).collect::<Vec<_>>(), [5, 8]);
}

#[test]
fn a_letter_target_the_body_lacks_is_refused() {
    let source = letter("One. <!-- &opening -->");
    let (overrides, origins) = change("missing", Some(&["ai"]), None);
    let error = assert_err!(prepare_letter(&source, ItemOverrides::new(&overrides, &origins)));
    assert!(error.to_string().contains("`&missing`"), "{error}");
}

#[test]
fn the_letter_greeting_and_closing_are_never_changed() {
    let source = letter("One. <!-- &opening -->");
    let (overrides, origins) = change("opening", Some(&["ai"]), Some(PriorityEdit::Core));
    let after = assert_ok!(prepare_letter(&source, ItemOverrides::new(&overrides, &origins)));
    assert_eq!(after.greeting, source.greeting);
    assert_eq!(after.closing, source.closing);
    assert!(matches!(after.body[0], SourceItem::Paragraph(_)));
}

// ---- the cases the independent test review named ---------------------------------------------------------

#[test]
fn a_group_override_reaches_a_draft_variant_too_and_keeps_it_a_draft() {
    let base = baseline("## S\n\n- A. <!-- =g.default @draft -->\n- B. <!-- =g.short @draft -->\n");
    let mut after = prepared(&base, "g", Some(&["ai"]), Some(PriorityEdit::Core));
    for slot in after.node_annotations_mut().into_iter().skip(1) {
        assert_eq!(slot.tags.iter().map(ToString::to_string).collect::<Vec<_>>(), ["ai"]);
        assert_eq!(slot.priority, Some(crate::model::Priority::Core));
        assert_eq!(
            slot.status,
            Some(crate::model::Status::Draft),
            "status is never changed"
        );
    }
}

#[test]
fn a_role_with_no_own_priority_still_lets_its_blurb_fall_back_after_the_blurb_is_cleared() {
    // The blurb's own priority is cleared by the spec; the role's priority is untouched, so it is what is left.
    let base =
        baseline("## S\n\n### Acme · 2015 – 2020 <!-- &role @core -->\n\nBlurb. <!-- &blurb @extra -->\n\n- A.\n");
    let mut after = prepared(&base, "blurb", None, Some(PriorityEdit::None));
    let notes = after.node_annotations_mut();
    assert_eq!(notes[2].priority, None, "the blurb has no own priority now");
    assert_eq!(
        notes[1].priority,
        Some(crate::model::Priority::Core),
        "the role keeps its own"
    );
}

#[test]
fn clearing_a_roles_priority_leaves_its_blurb_and_bullets_as_they_were() {
    let base = baseline(
        "## S\n\n### Acme · 2015 – 2020 <!-- &role @core -->\n\nBlurb. <!-- @extra -->\n\n- A. <!-- @core -->\n",
    );
    let mut after = prepared(&base, "role", None, Some(PriorityEdit::None));
    let priorities: Vec<_> = after.node_annotations_mut().iter().map(|a| a.priority).collect();
    assert_eq!(
        priorities,
        [
            None,
            None,
            Some(crate::model::Priority::Extra),
            Some(crate::model::Priority::Core)
        ]
    );
}

#[test]
fn an_era_and_a_role_take_overrides_like_any_container() {
    let base =
        baseline("## S\n\n### Acme · 2015 – 2020 <!-- &role -->\n\n**AI · 2016 – 2017** <!-- &era -->\n\n- A.\n");
    for target in ["role", "era"] {
        let after = notes(prepared(&base, target, Some(&["x"]), Some(PriorityEdit::Core)));
        let changed: Vec<_> = after.iter().filter(|n| n.1 == ["x"]).collect();
        assert_eq!(changed.len(), 1, "{target}: exactly the one node: {after:?}");
        assert_eq!(changed[0].0.as_deref(), Some(target));
    }
}

#[test]
fn an_id_kept_by_rewording_survives_a_move_into_another_container_and_a_conversion_to_a_group() {
    let before = baseline("## One\n\n- Item. <!-- &key #ai -->\n\n## Two\n\n- Other.\n");
    let moved = baseline("## One\n\n- Other.\n\n## Two\n\n- Item reworded. <!-- &key #ai -->\n");
    let converted = baseline(
        "## One\n\n- Item. <!-- =key.default #ai -->\n- Item again. <!-- =key.short #ai -->\n\n## Two\n\n- Other.\n",
    );
    for (name, base) in [("before", before), ("moved", moved), ("converted", converted)] {
        let after = notes(prepared(&base, "key", Some(&["platform"]), None));
        assert!(
            after.iter().any(|n| n.1 == ["platform"]),
            "{name}: the same override still finds its item: {after:?}"
        );
    }
}

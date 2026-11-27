use super::*;
use crate::diagnostics::{SourceKind, SourceText};
use crate::letter::{self, LetterFields, SourceItem};
use crate::model::{Priority, Status};
use crate::spec::{Located, additions::AdditionMetadata};
use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;

fn baseline(body: &str) -> LetterSource {
    let text = format!(
        "# Ada\n\n## Greeting\n\nHello <!-- =g.default -->\n\n## Body\n\n{body}\n\n## Closing\n\nThanks <!-- =c.default -->\n"
    );
    let parsed = crate::markdown::parse_letter("letter.md", &text);
    assert_some!(parsed.valid(), "{:?}", parsed.diagnostics()).clone()
}
fn named(id: &str) -> Sourced<ItemId> {
    Sourced {
        value: assert_ok!(ItemId::try_new(id)),
        at: Located {
            path: std::path::Path::new("position.toml").into(),
            line: 2,
            column: 1,
        },
    }
}
fn addition(id: &str, place: LetterPlace, text: &str, metadata: AdditionMetadata) -> LetterAddition {
    let literal = format!("'{text}'");
    // Each helper declaration owns a distinct snapshot; same-file batching is tested separately.
    let snapshot = SourceText::new(format!("{id}.toml").into(), SourceKind::Spec, literal.as_str());
    assert_ok!(LetterAddition::new(
        named(id),
        place,
        metadata,
        &snapshot,
        0..literal.len()
    ))
}
fn after(id: &str) -> LetterPlace {
    LetterPlace::After(named(id))
}
fn start() -> LetterPlace {
    LetterPlace::Start(named("start").at)
}
fn ids(letter: &LetterSource) -> Vec<String> {
    letter
        .body
        .iter()
        .map(|source| {
            let first = &items(source)[0];
            first
                .annotations
                .id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| first.annotations.alternate.as_ref().unwrap().group.to_string())
        })
        .collect()
}
fn reasons(error: PrepareError) -> Vec<BindingReason> {
    let PrepareError::AdditionBinding { problems } = error else {
        panic!("{error:?}")
    };
    problems.into_iter().map(|p| p.reason).collect()
}

#[test]
fn equivalent_gaps_preserve_array_order_and_repeated_assembly_never_accumulates() {
    let baseline = baseline("A <!-- &a -->\n\nB <!-- &b -->");
    let original = baseline.clone();
    let x = addition("x", LetterPlace::Before(named("b")), "X", Default::default());
    let y = addition("y", after("a"), "Y", Default::default());
    let assembled = assert_ok!(assemble_letter(&baseline, 100, &[x.clone(), y.clone()], None));
    assert_eq!(ids(&assembled), ["a", "x", "y", "b"]);
    assert_eq!(assembled.greeting, baseline.greeting);
    assert_eq!(assembled.closing, baseline.closing);
    assert_eq!(
        assert_ok!(assemble_letter(&baseline, 100, &[x.clone(), y.clone()], None)),
        assembled
    );
    assert_eq!(
        ids(&assert_ok!(assemble_letter(&baseline, 100, &[y, x], None))),
        ["a", "y", "x", "b"]
    );
    assert_eq!(baseline, original);
}

#[test]
fn groups_are_indivisible_and_body_edges_do_not_change_frames() {
    let baseline = baseline("A <!-- =body.default -->\n\nAlternate <!-- =body.short -->");
    let additions = [
        addition("first", start(), "First", Default::default()),
        addition("last", after("body"), "Last", Default::default()),
    ];
    let assembled = assert_ok!(assemble_letter(&baseline, 100, &additions, None));
    assert_eq!(ids(&assembled), ["first", "body", "last"]);
    assert_eq!(assembled.body[1], baseline.body[0]);
    assert_eq!(assembled.greeting, baseline.greeting);
    assert_eq!(assembled.closing, baseline.closing);
    let end = addition("end", LetterPlace::End(named("end").at), "End", Default::default());
    assert_eq!(
        ids(&assert_ok!(assemble_letter(&baseline, 100, &[end], None))),
        ["body", "end"]
    );
}

#[test]
fn aggregates_namespace_collisions_and_distinguishes_anchor_errors() {
    let baseline = baseline("A <!-- &a -->\n\nB <!-- =b.default -->");
    let additions = [
        addition("a", start(), "Collision", Default::default()),
        addition("b", start(), "Collision", Default::default()),
        addition("g", start(), "Frame collision", Default::default()),
        addition("x", after("c"), "Frame reference", Default::default()),
        addition("x", after("missing"), "Frame reference", Default::default()),
        addition("y", after("x"), "Addition reference", Default::default()),
    ];
    let original = baseline.clone();
    let got = reasons(assert_err!(assemble_letter(&baseline, 100, &additions, None)));
    assert_eq!(
        got,
        [
            BindingReason::BaselineCollision,
            BindingReason::BaselineCollision,
            BindingReason::BaselineCollision,
            BindingReason::DuplicateAddition,
            BindingReason::FrameTarget,
            BindingReason::MissingReference,
            BindingReason::AdditionTarget
        ]
    );
    assert_eq!(baseline, original);
}

#[test]
fn overrides_affect_only_baseline_and_cannot_target_additions() {
    use crate::spec::merge::{ChoiceKey, ItemOverride};
    let baseline = baseline("A <!-- &a @extra -->");
    let addition = addition("x", after("a"), "X", Default::default());
    let id = named("a").value;
    let changes = BTreeMap::from([(
        id.clone(),
        ItemOverride {
            tags: None,
            priority: Some(crate::model::PriorityEdit::Core),
        },
    )]);
    let origins = BTreeMap::from([(ChoiceKey::OverridePriority(id), named("a").at)]);
    let assembled = assert_ok!(assemble_letter(
        &baseline,
        100,
        std::slice::from_ref(&addition),
        Some(ItemOverrides::new(&changes, &origins))
    ));
    assert_eq!(items(&assembled.body[0])[0].annotations.priority, Some(Priority::Core));
    assert_eq!(items(&assembled.body[1])[0].annotations.priority, None);
    let id = named("x").value;
    let changes = BTreeMap::from([(
        id.clone(),
        ItemOverride {
            tags: None,
            priority: Some(crate::model::PriorityEdit::Core),
        },
    )]);
    let origins = BTreeMap::from([(ChoiceKey::OverridePriority(id), named("x").at)]);
    assert_eq!(
        reasons(assert_err!(assemble_letter(
            &baseline,
            100,
            &[addition],
            Some(ItemOverrides::new(&changes, &origins))
        ))),
        [BindingReason::AdditionTarget]
    );
}

#[test]
fn byte_budget_counts_drafts_and_checks_exact_boundary_and_overflow() {
    let baseline = baseline("A <!-- &a -->");
    let added = addition(
        "x",
        start(),
        "é",
        AdditionMetadata {
            status: Some(Sourced {
                value: Status::Draft,
                at: named("x").at,
            }),
            ..Default::default()
        },
    );
    let limit = crate::markdown::MAX_INPUT_BYTES;
    assert_ok!(assemble_letter(
        &baseline,
        limit - 2,
        std::slice::from_ref(&added),
        None
    ));
    for bytes in [limit - 1, usize::MAX] {
        assert!(matches!(
            assert_err!(assemble_letter(&baseline, bytes, std::slice::from_ref(&added), None)),
            PrepareError::PreparedContentLimit { .. }
        ));
    }
    assert_eq!(assert_ok!(assemble_letter(&baseline, 100, &[], None)), baseline);
}

#[test]
fn normal_selection_drops_draft_core_and_keeps_ready_addition_when_baseline_is_draft() {
    let baseline = baseline("A <!-- &a @draft -->");
    let draft = addition(
        "draft",
        after("a"),
        "For {{company}}",
        AdditionMetadata {
            priority: Some(Sourced {
                value: Priority::Core,
                at: named("draft").at,
            }),
            status: Some(Sourced {
                value: Status::Draft,
                at: named("draft").at,
            }),
            ..Default::default()
        },
    );
    let ready = addition("ready", after("a"), "For {{title}}", Default::default());
    let assembled = assert_ok!(assemble_letter(&baseline, 100, &[draft, ready.clone()], None));
    let (selected, report) = assert_ok!(letter::select(&assembled, &crate::select::Selection::default()));
    assert_eq!(selected.body.len(), 1);
    assert_eq!(report.dropped.len(), 2);
    let findings = assert_err!(letter::resolve(selected, &LetterFields::default()));
    assert_eq!(findings.len(), 1);
    assert_eq!(
        assert_some!(findings[0].span.as_ref()),
        &ready.content().origin().span()
    );
    assert!(findings[0].message.contains("title"));
    let SourceItem::Paragraph(item) = &assembled.body[2] else {
        panic!("leaf")
    };
    assert_eq!(item.piece.range, 0..ready.content().markdown().len());
    assert!(item.annotation_range.is_none());
}

#[test]
fn shared_snapshot_literals_keep_their_own_origins_and_before_group_is_atomic() {
    let baseline = baseline("A <!-- =body.default -->\n\nB <!-- =body.short -->");
    let snapshot = SourceText::new("shared.toml".into(), SourceKind::Spec, "a = 'X'\nb = 'Y'\n");
    let x = assert_ok!(LetterAddition::new(
        named("x"),
        LetterPlace::Before(named("body")),
        Default::default(),
        &snapshot,
        4..7
    ));
    let y = assert_ok!(LetterAddition::new(
        named("y"),
        after("body"),
        Default::default(),
        &snapshot.clone(),
        12..15
    ));
    let assembled = assert_ok!(assemble_letter(&baseline, 100, &[x, y], None));
    assert_eq!(ids(&assembled), ["x", "body", "y"]);
    assert_eq!(assembled.body[1], baseline.body[0]);
    for (index, literal) in [(0, "'X'"), (2, "'Y'")] {
        let span = items(&assembled.body[index])[0].piece.origin.span();
        assert_eq!(span.file, snapshot.path());
        assert_eq!(&snapshot.text()[span.range], literal);
    }
}

#[test]
fn collisions_do_not_misclassify_real_baseline_references() {
    use crate::spec::merge::{ChoiceKey, ItemOverride};
    let baseline = baseline("A <!-- &a -->");
    let collision = addition("a", start(), "Collision", Default::default());
    let linked = addition("x", after("a"), "Linked", Default::default());
    let id = named("a").value;
    let changes = BTreeMap::from([(
        id.clone(),
        ItemOverride {
            tags: None,
            priority: Some(crate::model::PriorityEdit::Core),
        },
    )]);
    let origins = BTreeMap::from([(ChoiceKey::OverridePriority(id), named("a").at)]);
    assert_eq!(
        reasons(assert_err!(assemble_letter(
            &baseline,
            100,
            &[collision, linked],
            Some(ItemOverrides::new(&changes, &origins))
        ))),
        [BindingReason::BaselineCollision]
    );
}

#[test]
fn override_addition_errors_include_each_written_field_in_source_order() {
    use crate::spec::merge::{ChoiceKey, ItemOverride};
    let baseline = baseline("A <!-- &a -->");
    let added = addition("x", start(), "X", Default::default());
    let id = named("x").value;
    let changes = BTreeMap::from([(
        id.clone(),
        ItemOverride {
            tags: Some(BTreeSet::new()),
            priority: Some(crate::model::PriorityEdit::Core),
        },
    )]);
    let mut tags_at = named("x").at;
    tags_at.line = 8;
    let mut priority_at = tags_at.clone();
    priority_at.line = 4;
    let origins = BTreeMap::from([
        (ChoiceKey::OverrideTags(id.clone()), tags_at),
        (ChoiceKey::OverridePriority(id), priority_at),
    ]);
    let error = assert_err!(assemble_letter(
        &baseline,
        100,
        &[added],
        Some(ItemOverrides::new(&changes, &origins))
    ));
    let PrepareError::AdditionBinding { problems } = error else {
        panic!("{error:?}")
    };
    assert_eq!(problems.iter().map(|p| p.at.line).collect::<Vec<_>>(), [4, 8]);
    assert!(problems.iter().all(|p| p.reason == BindingReason::AdditionTarget));
}

#[test]
fn budget_then_binding_then_ordinary_override_errors_have_explicit_precedence() {
    use crate::spec::merge::{ChoiceKey, ItemOverride};
    let baseline = baseline("A <!-- &a -->");
    let id = named("missing").value;
    let frame = named("g").value;
    let change = ItemOverride {
        tags: None,
        priority: Some(crate::model::PriorityEdit::Core),
    };
    let changes = BTreeMap::from([(id.clone(), change.clone()), (frame.clone(), change)]);
    let origins = BTreeMap::from([
        (ChoiceKey::OverridePriority(id), named("missing").at),
        (ChoiceKey::OverridePriority(frame), named("g").at),
    ]);
    let valid = addition("x", start(), "X", Default::default());
    assert!(matches!(
        assert_err!(assemble_letter(
            &baseline,
            100,
            &[valid],
            Some(ItemOverrides::new(&changes, &origins))
        )),
        PrepareError::Targets { .. }
    ));
    let invalid = addition("x", after("absent"), "X", Default::default());
    assert!(matches!(
        assert_err!(assemble_letter(
            &baseline,
            100,
            std::slice::from_ref(&invalid),
            Some(ItemOverrides::new(&changes, &origins))
        )),
        PrepareError::AdditionBinding { .. }
    ));
    let conflicting = addition("x", after("absent"), "Different", Default::default());
    assert!(matches!(
        assert_err!(assemble_letter(&baseline, usize::MAX, &[invalid, conflicting], None)),
        PrepareError::PreparedContentLimit { .. }
    ));
}

#[test]
fn many_same_gap_additions_keep_order_and_can_fill_an_empty_body() {
    let mut baseline = baseline("A <!-- &a -->");
    baseline.body.clear();
    let additions: Vec<_> = (0..1000)
        .map(|i| addition(&format!("item-{i}"), start(), "Text", Default::default()))
        .collect();
    let assembled = assert_ok!(assemble_letter(&baseline, 100, &additions, None));
    assert_eq!(assembled.body.len(), 1000);
    assert_eq!(
        ids(&assembled),
        additions.iter().map(|a| a.id().value.to_string()).collect::<Vec<_>>()
    );
    let selection = crate::select::Selection {
        scope: crate::select::Scope::Tags(assert_some!(crate::select::SelectedTags::new(BTreeSet::from([
            assert_ok!(crate::model::Tag::try_new("rust"))
        ])))),
        ..Default::default()
    };
    let (selected, _) = assert_ok!(letter::select(&assembled, &selection));
    assert_eq!(
        selected.body.len(),
        1000,
        "untagged additions follow ordinary general-item rules"
    );
}

#[test]
fn additions_cannot_supply_conflicting_snapshots_for_one_path() {
    let baseline = baseline("A <!-- &a -->");
    let first = SourceText::new("same.toml".into(), SourceKind::Spec, "'first'");
    let second = SourceText::new("same.toml".into(), SourceKind::Spec, "'second'");
    let a = assert_ok!(LetterAddition::new(
        named("x"),
        start(),
        Default::default(),
        &first,
        0..7
    ));
    let b = assert_ok!(LetterAddition::new(
        named("y"),
        start(),
        Default::default(),
        &second,
        0..8
    ));
    assert!(matches!(
        assert_err!(assemble_letter(&baseline, 100, &[a, b], None)),
        PrepareError::ConflictingAdditionSources { .. }
    ));
}

use super::*;
use crate::{
    markdown::parse_letter,
    model::{Priority, VariantName},
    select::{ChosenBy, Scope, SelectedTags},
};
use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;

fn parsed(body: &str) -> LetterSource {
    let text = format!("# Alex Example\n\n## Greeting\n\nHello,\n\n## Body\n\n{body}\n\n## Closing\n\nRegards,\n");
    let p = parse_letter("letter.md", &text);
    assert!(!p.has_errors(), "{:?}", p.diagnostics());
    assert_some!(p.into_parts().0)
}
fn variant(name: &str) -> VariantName {
    assert_ok!(VariantName::try_new(name))
}
fn pick(name: &str) -> Selection {
    Selection {
        picks: [(assert_ok!(crate::model::GroupName::try_new("g")), variant(name))].into(),
        ..Selection::default()
    }
}
fn text(piece: &Piece) -> String {
    piece
        .content
        .iter()
        .map(|n| match n {
            super::super::LetterInline::Text(s) => s.as_str(),
            _ => panic!("test text"),
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

#[test]
fn letter_drafts_off_preference_skips_draft_variant_and_falls_back() {
    let s = parsed("Default. <!-- =g.default -->\n\nShort. <!-- =g.short @draft -->");
    let (l, r) = assert_ok!(select(
        &s,
        &Selection {
            prefer: vec![variant("short")],
            ..Selection::default()
        }
    ));
    assert_eq!(text(&l.body[0]), "Default.");
    assert_eq!(r.chosen[0].by, ChosenBy::Default);
    assert_eq!(
        r.skipped_drafts,
        vec![Alternate {
            group: assert_ok!(crate::model::GroupName::try_new("g")),
            variant: variant("short")
        }]
    );
}
#[test]
fn letter_drafts_off_pick_of_draft_variant_is_an_error_naming_it() {
    let s = parsed("Default. <!-- =g.default -->\n\nShort. <!-- =g.short @draft -->");
    let e = assert_err!(select(&s, &pick("short")));
    assert!(matches!(e,LetterSelectError::PickedDraft{picks} if picks.len()==1 && picks[0].variant==variant("short")));
}
#[test]
fn letter_drafts_off_draft_default_falls_back_to_first_ready_variant() {
    let s = parsed("Default. <!-- =g.default @draft -->\n\nReady. <!-- =g.short -->");
    let (l, r) = assert_ok!(select(&s, &Selection::default()));
    assert_eq!(text(&l.body[0]), "Ready.");
    assert_eq!(r.chosen[0].by, ChosenBy::FirstReady);
}
#[test]
fn letter_drafts_off_group_with_no_ready_variant_has_nothing_to_render() {
    let s = parsed("Default. <!-- =g.default @draft -->\n\nShort. <!-- =g.short @draft -->\n\nOther.");
    let (l, r) = assert_ok!(select(&s, &Selection::default()));
    assert_eq!(l.body.len(), 1);
    assert_eq!(r.dropped[0].reason, DropReason::NoReadyVariant);
    assert_eq!(r.drafts_left_out, 2);
}
#[test]
fn letter_drafts_on_every_draft_variant_can_render() {
    let s = parsed("Default. <!-- =g.default @draft -->\n\nShort. <!-- =g.short @draft -->");
    for name in ["default", "short"] {
        let (l, r) = assert_ok!(select(
            &s,
            &Selection {
                drafts: true,
                ..pick(name)
            }
        ));
        assert_eq!(l.body.len(), 1);
        assert_eq!(r.chosen[0].variant, variant(name));
        assert_eq!(r.drafts_included, 2);
    }
}
#[test]
fn letter_a_group_is_judged_as_one_item_by_its_shared_priority_and_tag_union() {
    let s = parsed("Default. <!-- =g.default @extra #one -->\n\nShort. <!-- =g.short @extra #two -->");
    let selection = Selection {
        scope: Scope::Tags(assert_some!(SelectedTags::new(
            [assert_ok!(crate::model::Tag::try_new("two"))].into()
        ))),
        ..Selection::default()
    };
    let (l, r) = assert_ok!(select(&s, &selection));
    assert_eq!(text(&l.body[0]), "Default.");
    assert_eq!(r.chosen.len(), 1);
}
#[test]
fn core_scope_keeps_frames_and_eligible_core_but_not_drafts() {
    let s = parsed("Core. <!-- @core @verify -->\n\nDraft. <!-- @core @draft -->\n\nExtra. <!-- @extra -->");
    let (l, r) = assert_ok!(select(
        &s,
        &Selection {
            scope: Scope::CoreOnly,
            ..Selection::default()
        }
    ));
    assert_eq!(text(&l.greeting), "Hello,");
    assert_eq!(text(&l.closing), "Regards,");
    assert_eq!(l.body.len(), 1);
    assert_eq!(r.verify.len(), 1);
    assert_eq!(r.dropped.len(), 2);
    assert_eq!(r.kept.len(), 3);
}
#[test]
fn empty_body_carries_explanation_and_unknown_picks_are_not_silent() {
    let s = parsed("Draft. <!-- @draft -->");
    let e = assert_err!(select(&s, &pick("typo")));
    let LetterSelectError::EmptyBody { report } = e else {
        panic!("empty body")
    };
    assert_eq!(report.dropped.len(), 1);
    assert_eq!(report.unknown_picks.len(), 1);
    let s = parsed("Default. <!-- =g.default -->");
    let (_, r) = assert_ok!(select(&s, &pick("typo")));
    assert!(matches!(r.unknown_picks[0], UnknownPick::Variant(_)));
}
#[test]
fn picks_of_drafts_are_aggregated_in_authored_order_even_when_scope_drops_groups() {
    let s = parsed(
        "Z. <!-- =z.default #other -->\n\nZD. <!-- =z.short #other @draft -->\n\nA. <!-- =a.default #other -->\n\nAD. <!-- =a.short #other @draft -->",
    );
    let selection = Selection {
        scope: Scope::CoreOnly,
        picks: ["a", "z"]
            .into_iter()
            .map(|g| (assert_ok!(crate::model::GroupName::try_new(g)), variant("short")))
            .collect(),
        ..Selection::default()
    };
    let LetterSelectError::PickedDraft { picks } = assert_err!(select(&s, &selection)) else {
        panic!("draft picks")
    };
    assert_eq!(
        picks.iter().map(|p| p.group.as_ref()).collect::<Vec<&str>>(),
        vec!["z", "a"]
    );
}

#[test]
fn frame_variants_survive_core_only_and_preserve_their_exact_piece() {
    let source = "# Alex\n\n## Greeting\n\nHello, <!-- =hello.default -->\n\nWelcome, <!-- =hello.short -->\n\n## Body\n\nCore. <!-- @core -->\n\n## Closing\n\nRegards, <!-- =bye.default -->\n\nThanks, <!-- =bye.short -->\n";
    let s = assert_some!(parse_letter("letter.md", source).into_parts().0);
    let (l, r) = assert_ok!(select(
        &s,
        &Selection {
            scope: Scope::CoreOnly,
            prefer: vec![variant("short")],
            ..Selection::default()
        }
    ));
    let SourceItem::Alternates(group) = &s.greeting else {
        panic!("group")
    };
    assert_eq!(l.greeting, group.variants()[1].piece);
    let SourceItem::Alternates(group) = &s.closing else {
        panic!("group")
    };
    assert_eq!(l.closing, group.variants()[1].piece);
    assert_eq!(r.chosen.len(), 2);
    assert_eq!(
        r.kept.iter().map(|node| node.part).collect::<Vec<_>>(),
        vec![LetterPart::Greeting, LetterPart::Body, LetterPart::Closing]
    );
}

#[test]
fn exact_report_locations_and_frames_are_stable() {
    let source = "# Alex\n\n## Greeting\n\nHello, <!-- =hello.default -->\n\nWelcome, <!-- =hello.short -->\n\n## Body\n\nCore. <!-- @core @verify -->\n\nDefault. <!-- =g.default @extra -->\n\nShort. <!-- =g.short @extra -->\n\nDrop. <!-- =d.default #other -->\n\nDrop too. <!-- =d.short #other -->\n\n## Closing\n\nRegards,\n";
    let s = assert_some!(parse_letter("letter.md", source).into_parts().0);
    let selection = Selection {
        scope: Scope::Tags(assert_some!(SelectedTags::new(
            [assert_ok!(crate::model::Tag::try_new("chosen"))].into()
        ))),
        picks: [(assert_ok!(crate::model::GroupName::try_new("hello")), variant("short"))].into(),
        ..Selection::default()
    };
    let (l, r) = assert_ok!(select(&s, &selection));
    let SourceItem::Alternates(greeting) = &s.greeting else {
        panic!("group")
    };
    let loc = |part, piece: &Piece| LetterRef {
        part,
        range: piece.range.clone(),
    };
    assert_eq!(
        r.kept,
        vec![
            loc(LetterPart::Greeting, &greeting.variants()[1].piece),
            loc(LetterPart::Body, assert_some!(s.body[0].paragraph())),
            loc(LetterPart::Closing, assert_some!(s.closing.paragraph()))
        ]
    );
    assert_eq!(r.verify, vec![r.kept[1].clone()]);
    let first = |item: &SourceItem| match item {
        SourceItem::Alternates(g) => g.variants()[0].piece.clone(),
        _ => panic!("group"),
    };
    assert_eq!(
        r.dropped,
        vec![
            LetterDrop {
                item: loc(LetterPart::Body, &first(&s.body[1])),
                reason: DropReason::ExtraNotSelected
            },
            LetterDrop {
                item: loc(LetterPart::Body, &first(&s.body[2])),
                reason: DropReason::TagsNotSelected
            }
        ]
    );
    assert_eq!(r.chosen.len(), 1);
    assert_eq!(r.chosen[0].by, ChosenBy::Pick);
    assert!(r.unknown_picks.is_empty());
    assert_eq!(l.greeting, greeting.variants()[1].piece);
    for node in &r.kept {
        assert!(!source[node.range.clone()].is_empty());
    }
}

#[test]
fn unavailable_frame_groups_and_error_precedence_are_explicit() {
    let mut s = parsed("Default. <!-- =g.default @draft -->\n\nShort. <!-- =g.short @draft -->");
    let group = s.body[0].clone();
    s.greeting = group.clone();
    s.body.clear();
    let LetterSelectError::FrameNotReady { part, range } = assert_err!(select(&s, &Selection::default())) else {
        panic!("frame error before empty body")
    };
    assert_eq!(part, LetterPart::Greeting);
    let SourceItem::Alternates(g) = &s.greeting else {
        panic!("group")
    };
    assert_eq!(range, g.variants()[0].piece.range);
    assert!(matches!(
        assert_err!(select(&s, &pick("short"))),
        LetterSelectError::PickedDraft { .. }
    ));
    s.body = parsed("Body.").body;
    let (_, r) = assert_ok!(select(
        &s,
        &Selection {
            drafts: true,
            ..pick("short")
        }
    ));
    assert_eq!(r.drafts_included, 2);
    let SourceItem::Alternates(g) = &mut s.greeting else {
        panic!("group")
    };
    g.variants[1].annotations.status = Some(Status::Verify);
    let (_, r) = assert_ok!(select(&s, &pick("short")));
    assert_eq!(r.verify[0].part, LetterPart::Greeting);
    s.closing = group;
    s.greeting = parsed("Body.").greeting;
    assert!(matches!(
        assert_err!(select(&s, &Selection::default())),
        LetterSelectError::FrameNotReady {
            part: LetterPart::Closing,
            ..
        }
    ));
}

#[test]
fn draft_picks_in_frames_and_body_are_aggregated_before_other_failures() {
    let mut s = parsed(
        "G. <!-- =g.default -->\n\nGD. <!-- =g.short @draft -->\n\nB. <!-- =b.default -->\n\nBD. <!-- =b.short @draft -->",
    );
    s.greeting = s.body.remove(0);
    let selection = Selection {
        scope: Scope::CoreOnly,
        picks: ["b", "g"]
            .into_iter()
            .map(|g| (assert_ok!(crate::model::GroupName::try_new(g)), variant("short")))
            .collect(),
        ..Selection::default()
    };
    let e = assert_err!(select(&s, &selection));
    assert!(e.to_string().contains("`g.short`, `b.short`"));
    let LetterSelectError::PickedDraft { picks } = e else {
        panic!("draft errors")
    };
    assert_eq!(picks.len(), 2);
}

#[test]
fn direct_draft_frame_fails_closed_and_frames_ignore_scope() {
    let mut s = parsed("Core. <!-- @core -->");
    let SourceItem::Paragraph(item) = &mut s.greeting else {
        panic!("plain")
    };
    item.annotations.status = Some(Status::Draft);
    item.annotations.priority = Some(Priority::Core);
    assert!(matches!(
        assert_err!(select(&s, &Selection::default())),
        LetterSelectError::FrameNotReady {
            part: LetterPart::Greeting,
            ..
        }
    ));
    assert_ok!(select(
        &s,
        &Selection {
            drafts: true,
            ..Selection::default()
        }
    ));
}

use super::*;
use crate::diagnostics::SourceKind;
use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;

fn named(name: &str) -> Sourced<ItemId> {
    Sourced {
        value: assert_ok!(ItemId::try_new(name)),
        at: Located {
            path: std::path::Path::new("position.toml").into(),
            line: 1,
            column: 1,
        },
    }
}

#[test]
fn placement_determines_content_kind_and_metadata_cannot_make_an_alternate() {
    let source = SourceText::new("position.toml".into(), SourceKind::Spec, "'**Label:** text'");
    for place in [
        ResumePlace::Bullet(LeafPlace::After(named("anchor"))),
        ResumePlace::Paragraph(LeafPlace::Start(named("section"))),
        ResumePlace::Blurb { role: named("role") },
    ] {
        let kind = place.kind();
        let metadata = AdditionMetadata {
            priority: Some(Sourced {
                value: Priority::Core,
                at: named("item").at,
            }),
            status: Some(Sourced {
                value: Status::Draft,
                at: named("item").at,
            }),
            ..Default::default()
        };
        let added = assert_ok!(ResumeAddition::new(
            named("item"),
            place,
            metadata,
            &source,
            0..source.text().len()
        ));
        assert_eq!(added.place().kind(), kind);
        assert!(matches!(
            (kind, added.content().value()),
            (ResumeFragmentKind::Bullet, ResumeFragment::Bullet { .. })
                | (ResumeFragmentKind::Paragraph, ResumeFragment::Paragraph(_))
                | (ResumeFragmentKind::Blurb, ResumeFragment::Blurb(_))
        ));
        let annotations = added.metadata().annotations(&added.id().value);
        assert_eq!(annotations.priority, Some(Priority::Core));
        assert_eq!(annotations.status, Some(Status::Draft));
        assert!(annotations.alternate.is_none());
        assert_eq!(annotations.id.as_deref(), Some(&added.id().value));
    }
}

#[test]
fn letter_declarations_use_letter_placeholder_rules_even_for_drafts() {
    for (literal, valid) in [("'For {{company}}.'", true), ("'For {{unknown}}.'", false)] {
        let source = SourceText::new("position.toml".into(), SourceKind::Spec, literal);
        let metadata = AdditionMetadata {
            status: Some(Sourced {
                value: Status::Draft,
                at: named("item").at,
            }),
            ..Default::default()
        };
        let result = LetterAddition::new(
            named("item"),
            LetterPlace::Start(named("item").at),
            metadata,
            &source,
            0..literal.len(),
        );
        if valid {
            assert_ok!(result);
        } else {
            assert_err!(result);
        }
    }
}

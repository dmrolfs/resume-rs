//! ABOUTME: Direct-value domain and resolution tests, independent of the future Markdown parser.
//! ABOUTME: Pins literal insertion, original spans, validated fields, and the letter JSON contract.

use super::*;
use crate::diagnostics::{DiagnosticCode, Severity, SourceKind};
use crate::model::{Inline, LinkUrl, Name};
use claims::{assert_err, assert_none, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use std::path::Path;

fn piece(content: Vec<LetterInline>) -> Piece {
    Piece {
        content,
        range: 0..100,
        origin: crate::diagnostics::ItemOrigin::Baseline {
            file: "letter.md".into(),
            range: 0..100,
        },
    }
}

fn selected(content: Vec<LetterInline>) -> SelectedLetter {
    SelectedLetter {
        name: assert_ok!(Name::try_new("Alex Example")),
        contact: vec![],
        greeting: piece(vec![LetterInline::Text("Dear Hiring Team,".into())]),
        body: vec![piece(content)],
        closing: piece(vec![LetterInline::Text("Sincerely,".into())]),
    }
}

fn placeholder(key: Placeholder, start: usize) -> LetterInline {
    LetterInline::Placeholder {
        key,
        range: start..start + 11,
    }
}

fn text(value: &str) -> Inline {
    Inline::Text { text: value.into() }
}

#[test]
fn field_values_trim_edges_but_preserve_internal_text() {
    let value = assert_ok!(FieldValue::try_new(
        "  *Acme*  {{title}} [site](https://example.invalid)  "
    ));
    assert_eq!(value.as_ref(), "*Acme*  {{title}} [site](https://example.invalid)");
    assert_ok!(FieldValue::try_new("é".repeat(120)));
    assert_ok!(FieldValue::try_new(format!(" {} ", "é".repeat(120))));
    assert_eq!(assert_ok!(FieldValue::try_new("Acme\n")).as_ref(), "Acme");
    assert_err!(FieldValue::try_new("é".repeat(121)));
    for input in [
        "",
        "   ",
        "Acme\nCo",
        "Acme\rCo",
        "Acme\tCo",
        "Acme\0Co",
        "Acme\u{7f}Co",
        "Acme\u{2028}Co",
        "Acme\u{2029}Co",
    ] {
        assert_err!(FieldValue::try_new(input), "{input:?}");
    }
}

#[test]
fn a_calendar_date_converts_when_its_year_has_four_digits() {
    let four = time::Date::from_calendar_date(2026, time::Month::November, 19).unwrap();
    assert_eq!(assert_ok!(LetterDate::try_from(four)).to_string(), "2026-11-19");
    let low = time::Date::from_calendar_date(7, time::Month::January, 2).unwrap();
    assert_eq!(assert_ok!(LetterDate::try_from(low)).to_string(), "0007-01-02");
    let before_year_zero = time::Date::from_calendar_date(-1, time::Month::January, 1).unwrap();
    assert_err!(LetterDate::try_from(before_year_zero));
}

#[test]
fn dates_have_an_exact_iso_wire_format_and_typed_errors() {
    for input in ["0000-01-01", "0001-01-01", "0999-12-31", "2028-02-29", "9999-12-31"] {
        let date = assert_ok!(input.parse::<LetterDate>());
        assert_eq!(date.to_string(), input);
        assert_eq!(assert_ok!(serde_json::to_value(date)), serde_json::json!(input));
    }
    for input in [
        "",
        "2026-1-01",
        "2026/01/01",
        " 2026-01-01",
        "+2026-01-01",
        "２０２６-01-01",
        "2026-01-01 ",
    ] {
        assert!(matches!(
            assert_err!(input.parse::<LetterDate>()),
            LetterDateError::Shape { .. }
        ));
    }
    for input in ["2027-02-29", "2026-13-01", "2026-00-10", "2026-04-31", "2026-01-00"] {
        let error = assert_err!(input.parse::<LetterDate>());
        assert!(matches!(error, LetterDateError::OutOfRange { .. }));
        assert_some!(std::error::Error::source(&error));
    }
}

#[test]
fn every_placeholder_has_a_stable_key() {
    assert_eq!(
        DiagnosticCode::PlaceholderUnresolved.to_string(),
        "placeholder-unresolved"
    );
    assert_eq!(
        DiagnosticCode::PlaceholderUnresolved.category(),
        crate::diagnostics::Category::Format
    );
    for (key, spelling) in [
        (Placeholder::Company, "company"),
        (Placeholder::Title, "title"),
        (Placeholder::Recipient, "recipient"),
    ] {
        assert_eq!(key.to_string(), spelling);
        assert_eq!(assert_ok!(spelling.parse::<Placeholder>()), key);
    }
    assert_err!("Company".parse::<Placeholder>());
}

#[test]
fn values_are_literal_and_whitespace_is_not_rewritten() {
    let source = selected(vec![
        LetterInline::Text("the ".into()),
        placeholder(Placeholder::Company, 4),
        LetterInline::Text("  role".into()),
    ]);
    let fields = LetterFields {
        company: Some(assert_ok!(FieldValue::try_new("*Acme* {{title}}"))),
        title: Some(assert_ok!(FieldValue::try_new("Architect"))),
        ..Default::default()
    };
    let resolved = assert_ok!(resolve(source, &fields));
    assert_eq!(resolved.body, vec![vec![text("the *Acme* {{title}}  role")]]);
    assert_none!(resolved.date);
    assert_none!(resolved.recipient);
    assert_eq!(resolved.company, fields.company);
}

#[test]
fn nested_nodes_links_code_and_all_three_fields_are_preserved() {
    let url = assert_ok!(LinkUrl::try_new(assert_ok!(url::Url::parse("https://example.invalid"))));
    let source = selected(vec![
        LetterInline::Strong(vec![LetterInline::Emph(vec![placeholder(Placeholder::Title, 1)])]),
        LetterInline::Link {
            url: url.clone(),
            content: vec![placeholder(Placeholder::Recipient, 20)],
        },
        LetterInline::Code("{{company}}".into()),
        placeholder(Placeholder::Company, 40),
    ]);
    let fields = LetterFields {
        company: Some(assert_ok!(FieldValue::try_new("Acme"))),
        title: Some(assert_ok!(FieldValue::try_new("Architect"))),
        recipient: Some(assert_ok!(FieldValue::try_new("Hiring Team"))),
        date: Some(assert_ok!("2028-02-29".parse())),
    };
    let resolved = assert_ok!(resolve(source, &fields));
    assert_eq!(
        resolved.body[0],
        vec![
            Inline::Strong {
                content: vec![Inline::Emph {
                    content: vec![text("Architect")]
                }]
            },
            Inline::Link {
                url,
                content: vec![text("Hiring Team")]
            },
            Inline::Code {
                text: "{{company}}".into()
            },
            text("Acme"),
        ]
    );
    assert_eq!(resolved.date, fields.date);
    assert_eq!(resolved.company, fields.company);
    assert_eq!(resolved.recipient, fields.recipient);
}

#[test]
fn missing_values_report_each_original_span_sorted_and_return_no_letter() {
    let source = selected(vec![
        placeholder(Placeholder::Company, 42),
        LetterInline::Strong(vec![placeholder(Placeholder::Title, 7)]),
        LetterInline::Link {
            url: assert_ok!(LinkUrl::try_new(assert_ok!(url::Url::parse("https://example.invalid")))),
            content: vec![placeholder(Placeholder::Company, 20)],
        },
    ]);
    let mut source = source;
    source.body[0].origin = crate::diagnostics::ItemOrigin::Baseline {
        file: "é-letter.md".into(),
        range: 0..100,
    };
    let errors = assert_err!(resolve(source, &LetterFields::default()));
    assert_eq!(errors.len(), 3);
    assert_eq!(errors[0].message, "`{{title}}` has no value");
    assert_eq!(errors[0].hints, vec!["supply the title application field"]);
    for (error, start) in errors.iter().zip([7, 20, 42]) {
        assert_eq!(error.code, DiagnosticCode::PlaceholderUnresolved);
        assert_eq!(error.severity, Severity::Error);
        let span = assert_some!(error.span.as_ref());
        assert_eq!(span.file, Path::new("é-letter.md"));
        assert_eq!(span.kind, SourceKind::Markdown);
        assert_eq!(span.range, start..start + 11);
        assert!(error.fix.is_none());
        assert!(!error.hints.is_empty());
    }
}

#[test]
fn missing_in_frame_is_reported_even_when_body_fields_resolve() {
    let mut source = selected(vec![placeholder(Placeholder::Company, 20)]);
    source.greeting = piece(vec![placeholder(Placeholder::Recipient, 0)]);
    source.closing = piece(vec![placeholder(Placeholder::Title, 40)]);
    let fields = LetterFields {
        company: Some(assert_ok!(FieldValue::try_new("Acme"))),
        ..Default::default()
    };
    let errors = assert_err!(resolve(source, &fields));
    assert_eq!(errors.len(), 2);
    assert_eq!(assert_some!(errors[0].span.as_ref()).range, 0..11);
    assert_eq!(assert_some!(errors[1].span.as_ref()).range, 40..51);
    assert_eq!(errors[0].message, "`{{recipient}}` has no value");
    assert_eq!(errors[1].message, "`{{title}}` has no value");
}

#[test]
fn missing_values_obey_existing_diagnostic_cap() {
    let source = selected((0..503).map(|i| placeholder(Placeholder::Company, i * 11)).collect());
    let errors = assert_err!(resolve(source, &LetterFields::default()));
    assert_eq!(errors.len(), 501);
    assert_eq!(errors[500].code, DiagnosticCode::InputLimit);
    assert_eq!(errors[500].severity, Severity::Error);
    assert_eq!(errors[500].message, "3 more findings (3 errors) not shown");
    assert_none!(errors[500].span.as_ref());
}

#[test]
fn mixed_sources_keep_literal_ownership_and_baseline_offsets_in_authored_order() {
    use crate::diagnostics::ItemOrigin;
    let mut source = selected(vec![placeholder(Placeholder::Company, 70)]);
    let mut added = piece(vec![LetterInline::Strong(vec![placeholder(Placeholder::Title, 2)])]);
    added.origin = ItemOrigin::SpecContent {
        file: "position.toml".into(),
        content: 10..60,
    };
    let later = piece(vec![placeholder(Placeholder::Recipient, 90)]);
    source.body.extend([added, later]);
    let findings = assert_err!(resolve(source, &LetterFields::default()));
    let spans: Vec<_> = findings.iter().map(|d| assert_some!(d.span.as_ref())).collect();
    assert_eq!(
        spans.iter().map(|s| s.file.to_str()).collect::<Vec<_>>(),
        vec![Some("letter.md"), Some("position.toml"), Some("letter.md")]
    );
    assert_eq!(
        spans.iter().map(|s| s.range.clone()).collect::<Vec<_>>(),
        vec![70..81, 10..60, 90..101]
    );
    assert_eq!(spans[1].kind, SourceKind::Spec);
    assert!(findings.iter().all(|d| d.fix.is_none()));
}

#[test]
fn adjacent_spec_pieces_keep_prepared_order_not_toml_offset_order() {
    use crate::diagnostics::ItemOrigin;
    let mut source = selected(vec![placeholder(Placeholder::Company, 0)]);
    source.body[0].origin = ItemOrigin::SpecContent {
        file: "position.toml".into(),
        content: 100..150,
    };
    let mut second = piece(vec![LetterInline::Emph(vec![placeholder(Placeholder::Title, 1)])]);
    second.origin = ItemOrigin::SpecContent {
        file: "position.toml".into(),
        content: 10..50,
    };
    source.body.push(second);
    let findings = assert_err!(resolve(source, &LetterFields::default()));
    assert_eq!(assert_some!(findings[0].span.as_ref()).range, 100..150);
    assert_eq!(assert_some!(findings[1].span.as_ref()).range, 10..50);
}

#[test]
fn diagnostic_budget_is_shared_by_all_letter_sources() {
    use crate::diagnostics::ItemOrigin;
    let mut source = selected((0..300).map(|i| placeholder(Placeholder::Company, i * 11)).collect());
    let mut added = piece((0..300).map(|i| placeholder(Placeholder::Title, i * 11)).collect());
    added.origin = ItemOrigin::SpecContent {
        file: "position.toml".into(),
        content: 8..80,
    };
    source.body.push(added);
    let findings = assert_err!(resolve(source, &LetterFields::default()));
    assert_eq!(findings.len(), 501);
    assert_eq!(findings[500].message, "100 more findings (100 errors) not shown");
    assert_none!(findings[500].span.as_ref());
    assert_eq!(findings[500].severity, Severity::Error);
}

#[test]
fn letter_json_is_a_separate_complete_contract() {
    let mut source = selected(vec![LetterInline::Text("My experience.".into())]);
    source.contact = vec![text("alex@example.invalid")];
    let fields = LetterFields {
        date: Some(assert_ok!("2028-02-29".parse())),
        ..Default::default()
    };
    let letter = assert_ok!(resolve(source, &fields));
    assert_eq!(
        assert_ok!(serde_json::to_value(letter)),
        serde_json::json!({
            "schema": 1, "kind": "letter", "name": "Alex Example",
            "contact": [{"kind":"text", "text":"alex@example.invalid"}],
            "date":"2028-02-29", "company":null, "recipient":null,
            "greeting":[{"kind":"text","text":"Dear Hiring Team,"}],
            "body":[[{"kind":"text","text":"My experience."}]],
            "closing":[{"kind":"text","text":"Sincerely,"}]
        })
    );
    assert_eq!(crate::model::SCHEMA_VERSION, 1);
}

proptest! {
    #[test]
    fn any_valid_field_is_inserted_without_reparsing(value in "[^\\p{Cc}\\u{2028}\\u{2029}]{1,120}", prefix in ".{0,40}", suffix in ".{0,40}") {
        let value = FieldValue::try_new(value);
        prop_assume!(value.is_ok());
        let value = assert_ok!(value);
        let expected = format!("{prefix}{value}{suffix}");
        let fields = LetterFields { company: Some(value), ..Default::default() };
        let source = selected(vec![LetterInline::Text(prefix), placeholder(Placeholder::Company, 0), LetterInline::Text(suffix)]);
        let letter = assert_ok!(resolve(source, &fields));
        prop_assert_eq!(letter.body, vec![vec![text(&expected)]]);
    }
}

mod bridge {
    use super::{
        AlternateGroup, Item, LetterInline, LetterSource, Name, SelectedLetter, SourceItem, assert_err, assert_ok,
        piece,
    };
    use crate::model::{Alternate, Annotations, GroupName, Status, VariantName};
    use pretty_assertions::assert_eq;

    fn annotated(annotations: Annotations, range: Option<std::ops::Range<usize>>) -> SourceItem {
        SourceItem::Paragraph(Item {
            piece: piece(vec![LetterInline::Text("Words.".into())]),
            annotations,
            annotation_range: range,
        })
    }

    fn letter_with_body(item: SourceItem) -> LetterSource {
        LetterSource {
            name: assert_ok!(Name::try_new("Alex Example")),
            contact: vec![],
            greeting: piece(vec![LetterInline::Text("Hi,".into())]).into(),
            body: vec![item],
            closing: piece(vec![LetterInline::Text("Bye,".into())]).into(),
        }
    }

    fn draft() -> Annotations {
        Annotations {
            status: Some(Status::Draft),
            ..Annotations::default()
        }
    }

    #[test]
    fn a_plain_paragraph_crosses_with_its_piece_unchanged() {
        let plain = piece(vec![LetterInline::Text("Words.".into())]);
        let selected = assert_ok!(SelectedLetter::try_from(letter_with_body(plain.clone().into())));
        assert_eq!(selected.body, vec![plain]);
    }

    #[test]
    fn an_annotation_with_no_recorded_comment_is_still_refused_at_its_paragraph() {
        // The parser records a range for every annotation, but a value built by hand might not.
        let refused = assert_err!(SelectedLetter::try_from(letter_with_body(annotated(draft(), None))));
        assert_eq!(refused.span.range, 0..100);
    }

    #[test]
    fn a_recorded_comment_with_nothing_understood_is_still_refused_at_the_comment() {
        let refused = assert_err!(SelectedLetter::try_from(letter_with_body(annotated(
            Annotations::default(),
            Some(7..20)
        ))));
        assert_eq!(refused.span.range, 7..20);
    }

    #[test]
    fn a_lone_default_is_a_group_and_is_refused_like_any_other() {
        let alternate = Alternate {
            group: assert_ok!(GroupName::try_new("g")),
            variant: assert_ok!(VariantName::try_new("default")),
        };
        let item = Item {
            piece: piece(vec![LetterInline::Text("Words.".into())]),
            annotations: Annotations {
                alternate: Some(alternate),
                ..Annotations::default()
            },
            annotation_range: Some(3..9),
        };
        let group = SourceItem::Alternates(AlternateGroup { variants: vec![item] });
        assert_eq!(
            assert_err!(SelectedLetter::try_from(letter_with_body(group)))
                .span
                .range,
            3..9
        );
    }

    #[test]
    fn the_greeting_is_checked_before_the_body_and_the_closing() {
        let mut source = letter_with_body(annotated(draft(), Some(50..60)));
        source.greeting = annotated(draft(), Some(10..20));
        source.closing = annotated(draft(), Some(70..80));
        assert_eq!(assert_err!(SelectedLetter::try_from(source)).span.range, 10..20);
    }
}

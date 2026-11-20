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
    Piece { content, range: 0..100 }
}

fn selected(content: Vec<LetterInline>) -> SelectedLetter {
    LetterSource {
        name: assert_ok!(Name::try_new("Alex Example")),
        contact: vec![],
        greeting: piece(vec![LetterInline::Text("Dear Hiring Team,".into())]),
        body: vec![piece(content)],
        closing: piece(vec![LetterInline::Text("Sincerely,".into())]),
    }
    .into()
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
    let resolved = assert_ok!(resolve(source, &fields, Path::new("letter.md")));
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
    let resolved = assert_ok!(resolve(source, &fields, Path::new("letter.md")));
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
    let errors = assert_err!(resolve(source, &LetterFields::default(), Path::new("é-letter.md")));
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
    let errors = assert_err!(resolve(source, &fields, Path::new("letter.md")));
    assert_eq!(errors.len(), 2);
    assert_eq!(assert_some!(errors[0].span.as_ref()).range, 0..11);
    assert_eq!(assert_some!(errors[1].span.as_ref()).range, 40..51);
    assert_eq!(errors[0].message, "`{{recipient}}` has no value");
    assert_eq!(errors[1].message, "`{{title}}` has no value");
}

#[test]
fn missing_values_obey_existing_diagnostic_cap() {
    let source = selected((0..503).map(|i| placeholder(Placeholder::Company, i * 11)).collect());
    let errors = assert_err!(resolve(source, &LetterFields::default(), Path::new("letter.md")));
    assert_eq!(errors.len(), 501);
    assert_eq!(errors[500].code, DiagnosticCode::InputLimit);
    assert_eq!(errors[500].severity, Severity::Error);
    assert_eq!(errors[500].message, "3 more findings (3 errors) not shown");
}

#[test]
fn letter_json_is_a_separate_complete_contract() {
    let mut source = selected(vec![LetterInline::Text("My experience.".into())]);
    source.contact = vec![text("alex@example.invalid")];
    let fields = LetterFields {
        date: Some(assert_ok!("2028-02-29".parse())),
        ..Default::default()
    };
    let letter = assert_ok!(resolve(source, &fields, Path::new("letter.md")));
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
        let letter = assert_ok!(resolve(source, &fields, Path::new("letter.md")));
        prop_assert_eq!(letter.body, vec![vec![text(&expected)]]);
    }
}

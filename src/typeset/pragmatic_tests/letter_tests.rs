//! Full-block letter layout checked against actual shaped text positions.
use super::*;
use crate::letter::{FieldValue, LetterFields, SelectedLetter};
use pretty_assertions::assert_eq;

#[test]
fn letter_blocks_are_left_aligned_body_sized_and_visibly_separated() {
    let source = "# Alex Example\n\n[alex@example.invalid](mailto:alex@example.invalid)\n\n## Greeting\n\nHiring Team:\n\n## Body\n\nFirst paragraph.\n\nSecond paragraph.\n\n## Closing\n\nSincerely,\n";
    let parsed = crate::markdown::parse_letter("letter.md", source);
    let selected = assert_ok!(SelectedLetter::try_from(assert_some!(parsed.into_parts().0)));
    let fields = LetterFields {
        recipient: Some(assert_ok!(FieldValue::try_new("Hiring Team"))),
        company: Some(assert_ok!(FieldValue::try_new("Acme"))),
        date: Some(assert_ok!("2028-02-29".parse())),
        ..LetterFields::default()
    };
    let letter = assert_ok!(crate::letter::resolve(
        selected,
        &fields,
        std::path::Path::new("letter.md")
    ));
    let style = assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic));
    let compiled = assert_ok!(Typesetter::new(Arc::new(FontSet::bundled())).compile_letter(
        &letter,
        &style,
        RenderOptions::default()
    ));
    assert_eq!(compiled.page_count(), 1);
    let mut runs = Vec::new();
    collect_runs(&compiled.document.pages()[0].frame, Point::zero(), &mut runs);
    let find = |text: &str| assert_some!(runs.iter().find(|run| run.text == text), "missing {text:?}: {runs:?}");
    let name = find("Alex Example");
    let contact = find("alex@example.invalid");
    let date = find("February 29, 2028");
    let recipient = find("Hiring Team");
    let company = find("Acme");
    let greeting = find("Hiring Team:");
    let first = find("First paragraph.");
    let second = find("Second paragraph.");
    let closing = find("Sincerely,");
    for run in [
        name, contact, date, recipient, company, greeting, first, second, closing,
    ] {
        assert!((run.x - first.x).abs() < 0.1, "not aligned: {run:?}");
        assert_eq!(run.family, first.family);
        assert_eq!(run.fill, first.fill);
        assert!((run.size - first.size).abs() < 0.1, "not body sized: {run:?}");
    }
    let sender_pitch = contact.y - name.y;
    assert!(
        sender_pitch > 10.0 && sender_pitch < 17.0,
        "sender block not compact: {sender_pitch}"
    );
    let line_pitch = company.y - recipient.y;
    assert!(
        line_pitch > 10.0 && line_pitch < 17.0,
        "recipient block not compact: {line_pitch}"
    );
    for (before, after) in [
        (contact, date),
        (date, recipient),
        (company, greeting),
        (greeting, first),
        (first, second),
        (second, closing),
    ] {
        assert!(
            after.y - before.y >= line_pitch + 10.0,
            "missing blank line between {before:?} and {after:?}"
        );
    }
    assert!(!runs.iter().any(|run| run.text.contains("Dear")));
}

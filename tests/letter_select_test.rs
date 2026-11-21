//! Selection uses only valid parsed letters, before resolving fields.
use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::{
    letter::{self, LetterFields},
    markdown::parse_letter,
    select::Selection,
};

fn source(body: &str) -> String {
    format!("# Alex Example\n\n## Greeting\n\nHello,\n\n## Body\n\n{body}\n\n## Closing\n\nRegards,\n")
}

#[test]
fn only_selected_wordings_require_application_fields_and_keep_original_spans() {
    let text = source("General. <!-- =g.default -->\n\nFor {{recipient}}. <!-- =g.personal -->");
    let parsed = parse_letter("letter.md", &text);
    let model = assert_some!(parsed.into_parts().0);
    let (selected, _) = assert_ok!(letter::select(&model, &Selection::default()));
    assert_ok!(letter::resolve(
        selected,
        &LetterFields::default(),
        std::path::Path::new("letter.md")
    ));
    let selection = Selection {
        prefer: vec![assert_ok!(resume::model::VariantName::try_new("personal"))],
        ..Selection::default()
    };
    let (selected, _) = assert_ok!(letter::select(&model, &selection));
    let findings = assert_err!(letter::resolve(
        selected,
        &LetterFields::default(),
        std::path::Path::new("letter.md")
    ));
    let range = assert_some!(&findings[0].span).range.clone();
    assert_eq!(&text[range], "{{recipient}}");
}

#[test]
fn malformed_unselected_wordings_fail_before_selection() {
    for (body, code, token) in [
        (
            "General. <!-- =g.default -->\n\nBad {{unknown}}. <!-- =g.personal -->",
            "placeholder-unknown",
            "{{unknown}}",
        ),
        (
            "General.\n\nDraft. <!-- @draft @bogus -->",
            "annotation-unknown-keyword",
            "@bogus",
        ),
    ] {
        let input = source(body);
        let parsed = parse_letter("letter.md", &input);
        assert!(parsed.has_errors());
        let finding = assert_some!(parsed.diagnostics().iter().find(|d| d.code.to_string() == code));
        let range = assert_some!(&finding.span).range.clone();
        assert!(input[range].contains(token));
        assert!(parsed.into_parts().0.is_none());
    }
}

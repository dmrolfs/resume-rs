//! ABOUTME: Public cover-letter parser contract and original-source diagnostics.
//! ABOUTME: Synthetic inputs cover structure, encoded fields, reserved comments and bounded parsing.
use claims::{assert_none, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use resume::letter::{
    LetterFields, LetterInline, LetterSource, Piece, Placeholder, SelectedLetter, SourceItem, resolve,
};
use resume::markdown::parse_letter;

const LETTER: &str =
    "# Alex Example\n\n## Greeting\n\nDear Hiring Team,\n\n## Body\n\nBODY\n\n## Closing\n\nSincerely,\n";
fn source(body: &str) -> String {
    LETTER.replace("BODY", body)
}
fn codes(input: &str) -> Vec<String> {
    parse_letter("letter.md", input)
        .diagnostics()
        .iter()
        .map(|d| d.code.to_string())
        .collect()
}
/// The `index`th body item, which these tests write as a plain paragraph.
fn body_piece(model: &LetterSource, index: usize) -> &Piece {
    assert_some!(model.body[index].paragraph())
}
/// Every piece an item holds: its paragraph, or each wording of its group.
fn pieces(item: &SourceItem) -> Vec<&Piece> {
    match item {
        SourceItem::Paragraph(item) => vec![&item.piece],
        SourceItem::Alternates(group) => group.variants().iter().map(|v| &v.piece).collect(),
    }
}
fn contains(input: &str, code: &str) {
    assert!(codes(input).iter().any(|c| c == code), "{code}: {:?}", codes(input));
}

#[test]
fn minimal_letter_and_optional_contact() {
    for contact in ["", "alex@example.invalid\n\n"] {
        let input = source("My experience.").replacen("## Greeting", &format!("{contact}## Greeting"), 1);
        let parsed = parse_letter("letter.md", &input);
        let model = assert_some!(parsed.valid(), "{:?}", parsed.diagnostics());
        assert_eq!(model.body.len(), 1);
        let letter = assert_ok!(resolve(
            assert_ok!(SelectedLetter::try_from(model.clone())),
            &LetterFields::default(),
        ));
        assert_eq!(letter.name.as_ref(), "Alex Example");
        assert_eq!(model.contact.is_empty(), contact.is_empty());
    }
}

#[test]
fn encoded_placeholders_have_exact_original_spans() {
    for token in [
        "{{company}}",
        "&#123;&#123;company&#125;&#125;",
        "&lbrace;&lbrace;company&rbrace;&rbrace;",
        "{&#123;company}&#125;",
    ] {
        for ending in ["\n", "\r\n", "\r"] {
            let input = source(&format!("é &amp; ‘quote’ {token} end.")).replace('\n', ending);
            let parsed = parse_letter("letter.md", &input);
            let model = assert_some!(parsed.valid(), "{:?}", parsed.diagnostics());
            let uses: Vec<_> = body_piece(model, 0)
                .content
                .iter()
                .filter_map(|n| match n {
                    LetterInline::Placeholder { key, range } => Some((*key, range.clone())),
                    _ => None,
                })
                .collect();
            assert_eq!(uses.len(), 1);
            assert_eq!(uses[0].0, Placeholder::Company);
            assert_eq!(&input[uses[0].1.clone()], token);
        }
    }
}

#[test]
fn malformed_and_split_placeholders_are_errors() {
    for token in [
        "{{compnay}}",
        "{{company",
        "{{ company }}",
        "{{**company**}}",
        "{{com<!-- note -->pany}}",
        r"\{\{company\}\}",
    ] {
        let parsed = parse_letter("letter.md", &source(token));
        assert!(parsed.has_errors(), "{token}");
        assert_none!(parsed.valid());
    }
    assert_some!(parse_letter("letter.md", &source("A } and }} are literal.")).valid());
}

#[test]
fn placements_are_rejected_including_unused_references() {
    for body in [
        "`{{company}}`",
        "[label](https://example.invalid/{{company}})",
        "[label](https://example.invalid/&#123;&#123;company&#125;&#125;)",
        "Text.\n\n[x]: https://example.invalid/{{company}}",
    ] {
        contains(&source(body), "placeholder-position");
    }
    contains(
        &source("Fine.").replace("Alex Example", "{{company}}"),
        "placeholder-position",
    );
    contains(
        &source("Fine.").replace("## Greeting", "{{company}}\n\n## Greeting"),
        "placeholder-position",
    );
    assert_some!(
        parse_letter(
            "letter.md",
            &source("**{{company}}** and [{{title}}](https://example.invalid)")
        )
        .valid()
    );
}

#[test]
fn annotations_in_unreadable_places_fail_but_visible_code_examples_are_not_comments() {
    // A comment in a supported place is read now (tests/letter_annotation_test.rs); these sit where it cannot be.
    for (body, expected) in [
        ("Text.\n\n<!--#ai-->", vec!["annotation-detached"]),
        (
            "> <!-- =group.default -->",
            vec![
                "letter-section-shape",
                "letter-unsupported-block",
                "annotation-misplaced",
            ],
        ),
        (
            "| x |\n|---|\n| <!-- @draft --> |",
            vec![
                "letter-section-shape",
                "letter-unsupported-block",
                "annotation-misplaced",
            ],
        ),
        (
            "<!--\n @draft\n-->",
            vec!["letter-section-shape", "annotation-detached"],
        ),
    ] {
        assert_eq!(codes(&source(body)), expected, "{body:?}");
    }
    for body in ["`<!-- ordinary` text <!-- @draft -->", "Text. <!-- @draft -->"] {
        let parsed = parse_letter("letter.md", &source(body));
        assert_some!(parsed.valid(), "{body}: {:?}", parsed.diagnostics());
    }
    for body in [
        "`<!-- @draft -->`",
        r"\<!-- @draft -->",
        "&lt;!-- @draft --&gt;",
        "Text. <!-- Example: {{company}} -->",
    ] {
        let parsed = parse_letter("letter.md", &source(body));
        assert_some!(parsed.valid(), "{body}: {:?}", parsed.diagnostics());
    }
    contains(
        &source("Fine.").replace("Alex Example", "Alex <!-- @draft -->"),
        "annotation-misplaced",
    );
}

#[test]
fn structure_is_strict_and_unsupported_content_is_never_silently_lost() {
    let valid = source("Fine.");
    for (input, code) in [
        (valid.replace("## Greeting", "## Hello"), "letter-unknown-section"),
        (valid.replace("## Body", "## Greeting"), "letter-duplicate-section"),
        (valid.replace("## Greeting", "## Closing"), "letter-section-order"),
        (
            valid.replace("## Closing\n\nSincerely,\n", ""),
            "letter-missing-section",
        ),
        (source(""), "letter-section-shape"),
        (
            valid.replace("Dear Hiring Team,", "Hello\n\nAgain"),
            "letter-section-shape",
        ),
    ] {
        contains(&input, code);
    }
    for body in [
        "- bullet",
        "> quote",
        "```\ncode\n```",
        "### Role",
        "---",
        "![image](x)",
        "<div>hidden</div>",
    ] {
        assert!(parse_letter("letter.md", &source(body)).has_errors(), "{body}");
    }
}

#[test]
fn breaks_fold_to_spaces_without_resume_fix_edits() {
    let parsed = parse_letter("letter.md", &source("One  \ntwo\nthree."));
    let model = assert_some!(parsed.valid());
    assert_eq!(
        body_piece(model, 0).content,
        vec![LetterInline::Text("One two three.".into())]
    );
    assert!(parsed.diagnostics().iter().all(|d| d.fix.is_none()));
}

#[test]
fn limits_bom_and_missing_name_are_located() {
    assert_some!(parse_letter("letter.md", &format!("\u{feff}{}", source("Fine."))).valid());
    contains("", "missing-name");
    contains(&"x".repeat(resume::markdown::MAX_INPUT_BYTES + 1), "input-limit");
}

#[test]
fn fixture_snapshots_pin_model_and_diagnostics() {
    let valid = parse_letter("valid.md", include_str!("fixtures/letters/valid.md"));
    insta::assert_debug_snapshot!("letter_valid", assert_some!(valid.valid()));
    let invalid = parse_letter("invalid.md", include_str!("fixtures/letters/invalid.md"));
    insta::assert_debug_snapshot!("letter_invalid", invalid.diagnostics());
    assert_none!(invalid.valid());
}

#[test]
fn comment_and_formatting_boundaries_cannot_create_hidden_fields() {
    for body in [
        "{{com<!-- ordinary -->pany}}",
        "{{com\npany}}",
        "{{company}} <!-- @draft",
    ] {
        assert!(parse_letter("letter.md", &source(body)).has_errors(), "{body}");
    }
    // A complete multi-line annotation is an annotation now, not a hidden field boundary.
    assert_some!(parse_letter("letter.md", &source("Text. <!--\n@draft\n-->")).valid());
}

#[test]
fn container_comment_spans_include_original_prefixes_and_unicode() {
    for body in ["> <!--\n> @draft\n> é-->", "- <!--\n  @draft\n  é-->"] {
        let input = source(body);
        let parsed = parse_letter("letter.md", &input);
        let annotation = assert_some!(
            parsed
                .diagnostics()
                .iter()
                .find(|d| d.code.to_string() == "annotation-misplaced")
        );
        let span = assert_some!(annotation.span.as_ref());
        assert_eq!(&input[span.range.clone()], &body[assert_some!(body.find("<!--"))..]);
    }
}

#[test]
fn header_uses_letter_rules_and_unclosed_escaped_comments_stay_literal() {
    let input = source("Fine.").replace("## Greeting", "email  \nlocation\n\n## Greeting");
    let parsed = parse_letter("letter.md", &input);
    assert_some!(parsed.valid(), "{:?}", parsed.diagnostics());
    assert!(parsed.diagnostics().iter().all(|d| d.fix.is_none()));
    let image = source("Fine.").replace("## Greeting", "![face](x)\n\n## Greeting");
    assert!(parse_letter("letter.md", &image).has_errors());
    assert_some!(parse_letter("letter.md", &source(r"Visible \<!-- @draft")).valid());
    contains(&source("Hidden? <!-- ordinary"), "unsupported-html");
    contains(
        &source("Fine.\n\n[x]: https://example.invalid/&#123;&#123;company&#125;&#125;"),
        "placeholder-position",
    );
}

#[test]
fn split_opening_braces_and_multiple_openers_cannot_bypass_checks() {
    assert_eq!(codes(&source("{<!-- x -->{company}}")), vec!["placeholder-malformed"]);
    contains(
        &source("Fine.").replace("Alex Example", "{<!-- -->{company}}"),
        "placeholder-position",
    );
    contains(
        &source("Fine.").replace("## Greeting", "{<!-- -->{title}}\n\n## Greeting"),
        "placeholder-position",
    );
    assert_eq!(codes(&source(r"\<!-- a <!-- @draft")), vec!["annotation-malformed"]);
    assert_some!(parse_letter("letter.md", &source(r"\<!-- a \<!-- b")).valid());
}

#[test]
fn into_parts_gates_models_and_exact_placeholder_errors() {
    for (body, code) in [
        ("{{ company }}", "placeholder-unknown"),
        ("{{company", "placeholder-malformed"),
        ("`{{title}}`", "placeholder-position"),
    ] {
        let parsed = parse_letter("letter.md", &source(body));
        assert_eq!(
            parsed
                .diagnostics()
                .iter()
                .map(|d| d.code.to_string())
                .collect::<Vec<_>>(),
            vec![code]
        );
        assert_none!(parsed.into_parts().0);
    }
    let parsed = parse_letter("letter.md", &source("Fine."));
    let expected = parsed.valid().cloned();
    assert_eq!(parsed.into_parts().0, expected);
}

fn check_nodes(nodes: &[LetterInline], input: &str) {
    for node in nodes {
        match node {
            LetterInline::Text(text) => assert!(!text.contains("{{"), "unrecognized field in {input:?}"),
            LetterInline::Strong(children)
            | LetterInline::Emph(children)
            | LetterInline::Link { content: children, .. } => check_nodes(children, input),
            LetterInline::Placeholder { key, range } => {
                let source = &input[range.clone()];
                let decoded: String = pulldown_cmark::Parser::new(source)
                    .filter_map(|e| match e {
                        pulldown_cmark::Event::Text(text) => Some(text.into_string()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(decoded, format!("{{{{{key}}}}}"));
            }
            _ => {}
        }
    }
}

#[test]
fn visually_empty_frames_and_extra_contact_are_invalid() {
    contains(
        &source("Fine.").replace("Dear Hiring Team,", "**<!-- ordinary -->**"),
        "letter-section-shape",
    );
    contains(
        &source("Fine.").replace("## Greeting", "one\n\ntwo\n\n## Greeting"),
        "content-before-section",
    );
    let parsed = parse_letter("letter.md", &source("One\\\ntwo."));
    assert_eq!(
        body_piece(assert_some!(parsed.valid()), 0).content,
        vec![LetterInline::Text("One two.".into())]
    );
}

proptest! {
    #[test]
    fn structured_content_preserves_gating_and_never_prints_unrecognized_fields(
        fragments in prop::collection::vec(prop::sample::select(vec!["text", "{", "}", "\\", "<!--", "-->", "@draft", "`", "&#123;", "&#125;", "*", "\n", "{{company}}"]), 1..35)
    ) {
        let input = source(&fragments.concat());
        let parsed = parse_letter("letter.md", &input);
        if let Some(model) = parsed.valid() {
            for piece in model.body.iter().flat_map(pieces) { check_nodes(&piece.content, &input); }
        }
        let errors = parsed.has_errors();
        prop_assert_eq!(parsed.into_parts().0.is_none(), errors);
    }

    #[test]
    fn arbitrary_input_never_panics_and_spans_are_valid(input in "(?s).{0,2000}") {
        let parsed = parse_letter("letter.md", &input);
        prop_assert_eq!(parsed.valid().is_none(), parsed.has_errors());
        for d in parsed.diagnostics() {
            let span = assert_some!(d.span.as_ref());
            prop_assert!(input.get(span.range.clone()).is_some());
        }
    }
}

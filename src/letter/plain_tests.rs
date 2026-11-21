//! ABOUTME: Direct-model tests for canonical letter text, Unicode counts and nonmutating limits.
//! ABOUTME: Expected counts are independent literals; no Markdown parser is involved.
use super::plain::*;
use super::{FieldValue, LETTER_SCHEMA_VERSION, Letter, LetterKind};
use crate::model::{Inline, LinkUrl, Name};
use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use proptest::prelude::*;

fn text(s: &str) -> Inline {
    Inline::Text { text: s.into() }
}
fn letter(body: Vec<Vec<Inline>>) -> Letter {
    Letter {
        schema: LETTER_SCHEMA_VERSION,
        kind: LetterKind::Letter,
        name: assert_ok!(Name::try_new("Alex Example")),
        contact: vec![text("CONTACT OMITTED")],
        date: Some(assert_ok!("2028-02-29".parse())),
        recipient: Some(assert_ok!(FieldValue::try_new("RECIPIENT OMITTED"))),
        company: Some(assert_ok!(FieldValue::try_new("COMPANY OMITTED"))),
        greeting: vec![text("Dear Team,")],
        body,
        closing: vec![text("Sincerely,")],
    }
}
fn link(dest: &str, label: &str) -> Inline {
    Inline::Link {
        url: assert_ok!(LinkUrl::try_new(assert_ok!(url::Url::parse(dest)))),
        content: vec![text(label)],
    }
}
fn body_text(body: Vec<Inline>, style: LinkStyle) -> String {
    let rendered = to_plain(&letter(vec![body]), style);
    assert_some!(
        rendered
            .as_str()
            .strip_prefix("Dear Team,\n\n")
            .and_then(|s| s.strip_suffix("\n\nSincerely,\nAlex Example\n"))
    )
    .to_owned()
}

#[test]
fn exact_golden_layout_omits_header_but_keeps_signature() {
    let result = to_plain(
        &letter(vec![vec![text("One.")], vec![text("Two.")]]),
        LinkStyle::default(),
    );
    assert_eq!(result.as_str(), include_str!("../../tests/fixtures/letters/plain.txt"));
    assert_eq!(result.counts().line_breaks, 8);
    for hidden in ["CONTACT", "RECIPIENT", "COMPANY", "2028"] {
        assert!(!result.as_str().contains(hidden));
    }
    let owned = result.clone().into_string();
    assert_eq!(owned, result.as_str());
}

#[test]
fn markup_is_removed_without_changing_unicode_or_spacing() {
    let nodes = vec![
        text("é e\u{301}\u{a0}"),
        Inline::Strong {
            content: vec![Inline::Emph {
                content: vec![text("‘漢字’")],
            }],
        },
        Inline::Code {
            text: " *code* {{company}} ".into(),
        },
    ];
    assert_eq!(
        body_text(nodes, LinkStyle::default()),
        "é e\u{301}\u{a0}‘漢字’ *code* {{company}} "
    );
    assert_eq!(
        body_text(vec![text("a\u{2028}b\u{2029}c")], LinkStyle::default()),
        "a\u{2028}b\u{2029}c"
    );
}

#[test]
fn breaks_fold_once_after_markup_is_joined() {
    for value in ["a\nb", "a\rb", "a\r\nb"] {
        assert_eq!(body_text(vec![text(value)], LinkStyle::default()), "a b");
    }
    assert_eq!(
        body_text(
            vec![
                text("a\r"),
                Inline::Strong {
                    content: vec![text("\nb")]
                }
            ],
            LinkStyle::default()
        ),
        "a b"
    );
    assert_eq!(body_text(vec![text(" a\n\nb ")], LinkStyle::default()), " a  b ");
}

#[test]
fn links_collapse_only_exact_serialized_equality_and_keep_mailto_parameters() {
    let cases = [
        (
            "https://example.invalid/",
            "https://example.invalid/",
            "https://example.invalid/",
        ),
        (
            "https://example.invalid",
            "https://example.invalid",
            "https://example.invalid (https://example.invalid/)",
        ),
        (
            "https://example.invalid/",
            "example.invalid",
            "example.invalid (https://example.invalid/)",
        ),
        (
            "mailto:alex@example.invalid",
            "alex@example.invalid",
            "alex@example.invalid",
        ),
        (
            "mailto:alex@example.invalid",
            "mailto:alex@example.invalid",
            "mailto:alex@example.invalid",
        ),
        (
            "mailto:alex@example.invalid",
            "Email me",
            "Email me (alex@example.invalid)",
        ),
        (
            "mailto:alex@example.invalid?subject=Hi",
            "Email me",
            "Email me (mailto:alex@example.invalid?subject=Hi)",
        ),
        (
            "mailto:alex@example.invalid#part",
            "alex@example.invalid",
            "alex@example.invalid (mailto:alex@example.invalid#part)",
        ),
        (
            "mailto:a%2Bb@example.invalid",
            "a+b@example.invalid",
            "a+b@example.invalid (a%2Bb@example.invalid)",
        ),
        ("https://example.invalid/", "a\nb", "a b (https://example.invalid/)"),
        (
            "mailto:josé@example.invalid",
            "josé@example.invalid",
            "josé@example.invalid (jos%C3%A9@example.invalid)",
        ),
        ("https://example.invalid/", "", " (https://example.invalid/)"),
    ];
    for (dest, label, expected) in cases {
        assert_eq!(body_text(vec![link(dest, label)], LinkStyle::TextAndUrl), expected);
        assert_eq!(
            body_text(vec![link(dest, label)], LinkStyle::TextOnly),
            label.replace('\n', " ")
        );
    }
    let nested = Inline::Link {
        url: assert_ok!(LinkUrl::try_new(assert_ok!(url::Url::parse(
            "https://example.invalid/"
        )))),
        content: vec![Inline::Emph {
            content: vec![text("site")],
        }],
    };
    assert_eq!(
        body_text(vec![nested], LinkStyle::default()),
        "site (https://example.invalid/)"
    );
}

#[test]
fn unicode_counts_match_independent_hand_computed_oracles() {
    for (s, bytes, scalars, utf16, words, line_breaks) in [
        ("", 0, 0, 0, 0, 0),
        ("é", 2, 1, 1, 1, 0),
        ("e\u{301}", 3, 2, 2, 1, 0),
        ("😀", 4, 1, 2, 1, 0),
        ("👨‍👩‍👧", 18, 5, 8, 1, 0),
        ("漢字", 6, 2, 2, 1, 0),
        ("a\u{a0}b", 4, 3, 3, 2, 0),
        ("a\u{2028}b", 5, 3, 3, 2, 0),
        ("x\r\ny\nz", 6, 6, 6, 3, 2),
    ] {
        assert_eq!(
            TextCounts::of(s),
            TextCounts {
                bytes,
                scalars,
                utf16,
                words,
                line_breaks
            }
        );
    }
    assert_eq!(LengthUnit::default(), LengthUnit::Utf16);
    assert_eq!(LinkStyle::TextAndUrl.to_string(), "text-and-url");
    assert_eq!(LinkStyle::TextOnly.to_string(), "text-only");
    for (unit, name) in [
        (LengthUnit::Utf16, "utf16"),
        (LengthUnit::Scalars, "scalars"),
        (LengthUnit::Bytes, "bytes"),
        (LengthUnit::Words, "words"),
    ] {
        assert_eq!(unit.to_string(), name);
    }
}

#[test]
fn limits_check_the_chosen_unit_at_every_boundary_without_truncation() {
    let rendered = to_plain(&letter(vec![vec![text("👨‍👩‍👧 a\u{a0}b")]]), LinkStyle::default());
    let before = rendered.as_str().to_string();
    let counts = rendered.counts();
    // Four unequal expected totals also catch unit-swapping implementations.
    assert_eq!(
        (counts.bytes, counts.scalars, counts.utf16, counts.words),
        (61, 47, 50, 8)
    );
    for (unit, actual) in [
        (LengthUnit::Utf16, 50),
        (LengthUnit::Scalars, 47),
        (LengthUnit::Bytes, 61),
        (LengthUnit::Words, 8),
    ] {
        assert_eq!(counts.get(unit), actual);
        for max in [actual, actual + 1] {
            assert_ok!(TextLimit { max, unit }.check(&rendered));
        }
        for max in [0, actual - 1] {
            let error = assert_err!(TextLimit { max, unit }.check(&rendered));
            assert_eq!(error, TextLimitError::OverLimit { actual, max, unit });
            assert_eq!(
                error.to_string(),
                format!("letter text is {actual} {unit}, over the limit of {max} {unit}")
            );
            assert_eq!(rendered.as_str(), before);
        }
    }
}

#[test]
fn empty_model_parts_still_have_deterministic_layout() {
    let mut input = letter(vec![]);
    input.greeting.clear();
    input.closing.clear();
    assert_eq!(to_plain(&input, LinkStyle::default()).as_str(), "\n\n\nAlex Example\n");
}

proptest! {
    #[test]
    fn final_counts_describe_only_final_text_and_layout(body in prop::collection::vec("(?s).{0,100}", 0..12), greeting in "(?s).{0,100}", closing in "(?s).{0,100}") {
        let mut input = letter(body.iter().map(|s| vec![text(s)]).collect());
        input.greeting = vec![text(&greeting)];
        input.closing = vec![text(&closing)];
        let output = to_plain(&input, LinkStyle::default());
        prop_assert!(!output.as_str().contains('\r'));
        prop_assert!(output.as_str().ends_with("Alex Example\n"));
        prop_assert_eq!(output.counts().line_breaks, 2 * body.len() + 4);
        prop_assert_eq!(output.counts(), TextCounts::of(output.as_str()));
        prop_assert_eq!(output.counts().bytes, output.as_str().len());
    }
}

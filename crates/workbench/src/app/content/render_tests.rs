use super::render;
use claims::assert_some;
use pretty_assertions::assert_eq;
use resume::model::Inline;

#[test]
fn code_spans_round_trip_backticks_spaces_and_punctuation() {
    for value in ["serde", "`tick`", "a``b", " a ", "   ", "a\\b"] {
        let markdown = render::inline(&[Inline::Code { text: value.into() }]);
        let codes: Vec<_> = pulldown_cmark::Parser::new(&markdown)
            .filter_map(|event| match event {
                pulldown_cmark::Event::Code(value) => Some(value.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(codes, vec![value], "{markdown}");
    }
}

#[test]
fn links_keep_checked_destinations_and_escaped_labels() {
    let parsed = resume::markdown::parse(
        "r.md",
        "# Ada\n\n## Work\n\n[**Example**](https://example.com/path?a=1&b=2).\n",
    );
    let resume = assert_some!(parsed.valid());
    let output = render::resume(resume);
    let links: Vec<_> = pulldown_cmark::Parser::new(&output)
        .filter_map(|e| match e {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) => Some(dest_url.into_string()),
            _ => None,
        })
        .collect();
    assert_eq!(links, vec!["https://example.com/path?a=1&b=2"]);
}

#[test]
fn punctuation_fields_cannot_create_images_or_links() {
    let output = render::inline(&[Inline::Text {
        text: "![x](https://example.com) <script>".into(),
    }]);
    assert!(!pulldown_cmark::Parser::new(&output).any(|e| matches!(
        e,
        pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { .. } | pulldown_cmark::Tag::Link { .. })
            | pulldown_cmark::Event::Html(_)
    )));
}

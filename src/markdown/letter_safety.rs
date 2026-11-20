//! ABOUTME: Observes Markdown comments and reference destinations before unsupported content is discarded.
//! ABOUTME: A bounded second event pass preserves resume lowering behavior and never treats code text as HTML.

use super::{Finding, Sink, annotation, tree};
use crate::diagnostics::DiagnosticCode;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::ops::Range;

pub(super) fn inspect(text: &str, base: usize, sink: &mut Sink<'_>) {
    let parser = Parser::new_ext(text, tree::options());
    for (_, def) in parser.reference_definitions().iter() {
        if def.dest.contains("{{") || def.title.as_ref().is_some_and(|t| t.contains("{{")) {
            report_position(def.span.start + base..def.span.end + base, sink);
        }
    }
    let mut html = tree::TextNode::default();
    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::HtmlBlock) => html = tree::TextNode::default(),
            Event::Html(part) => {
                let verbatim = text.get(range.clone()) == Some(part.as_ref());
                html.push(&part, range.start + base..range.end + base, verbatim);
            }
            Event::End(TagEnd::HtmlBlock) => comments(&html, sink),
            Event::InlineHtml(part) => {
                let mut node = tree::TextNode::default();
                let verbatim = text.get(range.clone()) == Some(part.as_ref());
                node.push(&part, range.start + base..range.end + base, verbatim);
                comments(&node, sink);
            }
            Event::Start(Tag::Link { dest_url, title, .. }) if (dest_url.contains("{{") || title.contains("{{")) => {
                report_position(range.start + base..range.end + base, sink);
            }
            _ => {}
        }
    }
}

pub(super) fn report_position(range: Range<usize>, sink: &mut Sink<'_>) {
    sink.report(
        Finding::new(
            DiagnosticCode::PlaceholderPosition,
            range,
            "placeholder outside paragraph text or a link label",
        )
        .hint("put application fields in the Greeting, Body or Closing text"),
    );
}

fn comments(node: &tree::TextNode, sink: &mut Sink<'_>) {
    let html = &node.text;
    let mut offset = 0;
    while let Some(open) = html[offset..].find("<!--") {
        let start = offset + open;
        let body_start = start + 4;
        let rest = &html[body_start..];
        let (body, end) = if rest.starts_with('>') {
            ("", body_start + 1)
        } else if rest.starts_with("->") {
            ("", body_start + 2)
        } else if let Some(close) = rest.find("-->") {
            (&rest[..close], body_start + close + 3)
        } else {
            (rest, html.len())
        };
        if annotation::is_annotation(&format!("<!--{body}-->")) {
            sink.report(
                Finding::new(
                    DiagnosticCode::LetterAnnotationUnsupported,
                    node.source_range(start..end),
                    "letter annotation cannot be honored before tailoring support",
                )
                .hint("remove the annotation only if this content is ready to include"),
            );
        }
        offset = end;
    }
}

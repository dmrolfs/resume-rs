//! ABOUTME: Observes Markdown comments and reference destinations before unsupported content is discarded.
//! ABOUTME: A bounded second event pass preserves resume lowering behavior and never treats code text as HTML.

use super::{Finding, Sink, annotation, tree};
use crate::diagnostics::DiagnosticCode;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::collections::HashSet;
use std::ops::Range;

/// Reports what the tree cannot show the interpreter: placeholders in reference definitions and link targets, and
/// annotation-shaped comments inside unsupported containers or never closed.
///
/// `owned` holds the start of every comment the interpreter has already judged, which are not reported again.
pub(super) fn inspect(text: &str, base: usize, owned: &HashSet<usize>, sink: &mut Sink<'_>) {
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
            Event::Html(part) => push_html(&mut html, &part, text, range, base),
            Event::End(TagEnd::HtmlBlock) => comments(&html, owned, sink),
            Event::InlineHtml(part) => {
                let mut node = tree::TextNode::default();
                push_html(&mut node, &part, text, range, base);
                comments(&node, owned, sink);
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

/// Adds one HTML event to `node`, mapped to its source range; exact only when the event is a verbatim slice.
fn push_html(node: &mut tree::TextNode, part: &str, text: &str, range: Range<usize>, base: usize) {
    let verbatim = text.get(range.clone()) == Some(part);
    node.push(part, range.start + base..range.end + base, verbatim);
}

/// Reports each annotation-shaped comment in `node` that no helper looked at, using the shared comment splitter.
fn comments(node: &tree::TextNode, owned: &HashSet<usize>, sink: &mut Sink<'_>) {
    let html = &node.text;
    let mut offset = 0;
    while let Some(open) = html[offset..].find("<!--") {
        let start = offset + open;
        let rest = &html[start..];
        // A comment that never closes owns the rest of the text; close it only to classify its first word.
        let (len, closed) = annotation::comment_len(rest).map_or((rest.len(), false), |len| (len, true));
        let range = node.source_range(start..start + len);
        let looks_like_one = if closed {
            annotation::is_annotation(&rest[..len])
        } else {
            annotation::is_annotation(&format!("{rest}-->"))
        };
        if looks_like_one && !owned.contains(&range.start) {
            sink.report(unseen(range, closed));
        }
        offset = start + len;
    }
}

/// An annotation-shaped comment that no annotation helper looked at: it sits where a letter cannot honor it.
pub(super) fn unseen(range: Range<usize>, closed: bool) -> Finding {
    if closed {
        annotation::misplaced(
            range,
            "an annotation is only read at the end of a paragraph or alone on the line above one",
        )
        .hint("move it to the end of the paragraph it marks, or alone on the line directly above that paragraph")
    } else {
        Finding::new(
            DiagnosticCode::AnnotationMalformed,
            range,
            "an annotation comment is never closed",
        )
        .hint("close the comment with -->, or escape the opening < to print it literally")
    }
}

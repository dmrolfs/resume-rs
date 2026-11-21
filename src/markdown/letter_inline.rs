//! ABOUTME: Lowers letter inline text with encoded placeholders and original-source locations.
//! ABOUTME: Formatting boundaries cannot assemble a placeholder and field values are resolved later.

use super::{
    Finding, Sink, Source, annotation, inline, letter_safety,
    tree::{InlineNode, TextNode},
};
use crate::{
    diagnostics::DiagnosticCode,
    letter::{LetterInline, Placeholder},
};

pub(super) fn convert(nodes: &[InlineNode], src: &Source<'_>, sink: &mut Sink<'_>) -> Vec<LetterInline> {
    let mut output = Vec::new();
    for node in nodes {
        match node {
            InlineNode::Text(text) => text_nodes(text, src, sink, &mut output),
            InlineNode::Strong { children, .. } => output.push(LetterInline::Strong(convert(children, src, sink))),
            InlineNode::Emph { children, .. } => output.push(LetterInline::Emph(convert(children, src, sink))),
            InlineNode::Link {
                dest,
                email,
                children,
                range,
            } => match inline::link_url(dest, *email) {
                Ok(url) => output.push(LetterInline::Link {
                    url,
                    content: convert(children, src, sink),
                }),
                Err(_) => sink.report(
                    Finding::new(DiagnosticCode::InvalidLink, range.clone(), "invalid letter link")
                        .hint("use an absolute http, https or mailto URL"),
                ),
            },
            InlineNode::Code { text, range } => {
                if text.contains("{{") {
                    letter_safety::report_position(range.clone(), sink);
                }
                output.push(LetterInline::Code(text.clone()));
            }
            InlineNode::Break { .. } => push(&mut output, " "),
            InlineNode::Html { text, range } => {
                if !inline::is_comment(text) {
                    unsupported(range.clone(), sink);
                }
            }
            InlineNode::Image { range } | InlineNode::Unsupported { range, .. } => unsupported(range.clone(), sink),
        }
    }
    if output
        .iter()
        .any(|n| matches!(n, LetterInline::Text(t) if t.contains("{{")))
        && let (Some(first), Some(last)) = (nodes.first(), nodes.last())
    {
        malformed(first.range().start..last.range().end, sink);
    }
    output
}

pub(super) fn forbidden(nodes: &[InlineNode], sink: &mut Sink<'_>) {
    if inline::flatten_text(nodes).contains("{{")
        && !nodes
            .iter()
            .any(|n| matches!(n, InlineNode::Text(t) if t.text.contains("{{")))
        && let (Some(first), Some(last)) = (nodes.first(), nodes.last())
    {
        letter_safety::report_position(first.range().start..last.range().end, sink);
    }
    for node in nodes {
        match node {
            InlineNode::Text(text) if text.text.contains("{{") => letter_safety::report_position(text.range(), sink),
            InlineNode::Code { text, range } if text.contains("{{") => {
                letter_safety::report_position(range.clone(), sink)
            }
            InlineNode::Strong { children, .. }
            | InlineNode::Emph { children, .. }
            | InlineNode::Link { children, .. } => forbidden(children, sink),
            _ => {}
        }
    }
}

fn text_nodes(text: &TextNode, src: &Source<'_>, sink: &mut Sink<'_>, out: &mut Vec<LetterInline>) {
    unclosed_comment(text, src, sink);
    let mut cursor = 0;
    while let Some(relative) = text.text[cursor..].find("{{") {
        let start = cursor + relative;
        push(out, &text.text[cursor..start]);
        let Some(close) = text.text[start + 2..].find("}}") else {
            malformed(text.source_range(start..text.text.len()), sink);
            return;
        };
        let end = start + 2 + close + 2;
        let range = text.source_range(start..end);
        let spelling = src.slice(&range);
        // The first escaping slash may lie just outside pulldown's text event range.
        let slashes = src.text[..range.start]
            .bytes()
            .rev()
            .take_while(|b| *b == b'\\')
            .count();
        if spelling.contains('\\') || slashes % 2 == 1 {
            malformed(range, sink);
        } else if let Ok(key) = text.text[start + 2..end - 2].parse::<Placeholder>() {
            out.push(LetterInline::Placeholder { key, range });
        } else {
            sink.report(
                Finding::new(DiagnosticCode::PlaceholderUnknown, range, "unknown letter field")
                    .hint("use company, title or recipient without internal spaces or formatting"),
            );
        }
        cursor = end;
    }
    push(out, &text.text[cursor..]);
}

fn unclosed_comment(text: &TextNode, src: &Source<'_>, sink: &mut Sink<'_>) {
    let range = text.range();
    let raw = src.slice(&range);
    let last_close = raw.rfind("-->");
    for (start, _) in raw.match_indices("<!--") {
        let at = range.start + start;
        let escapes = src.text[..at].bytes().rev().take_while(|b| *b == b'\\').count();
        if escapes % 2 == 0 && last_close.is_none_or(|close| close < start) {
            let reserved = annotation::is_annotation(&format!("<!--{}-->", &raw[start + 4..]));
            if reserved {
                sink.report(letter_safety::unseen(at..range.end, false));
            } else {
                sink.report(
                    Finding::new(
                        DiagnosticCode::UnsupportedHtml,
                        at..range.end,
                        "unclosed HTML comment in letter text",
                    )
                    .hint("close the comment with -->, or escape the opening < to print it literally"),
                );
            }
            break; // An unclosed real comment owns the remainder of this text run.
        }
    }
}

/// Header values share letter inline policies, but never contain unresolved nodes.
pub(super) fn contact(nodes: &[InlineNode], src: &Source<'_>, sink: &mut Sink<'_>) -> Vec<crate::model::Inline> {
    forbidden(nodes, sink);
    fn resolved(nodes: Vec<LetterInline>) -> Vec<crate::model::Inline> {
        use crate::model::Inline;
        nodes
            .into_iter()
            .filter_map(|node| {
                Some(match node {
                    LetterInline::Text(text) => Inline::Text { text },
                    LetterInline::Code(text) => Inline::Code { text },
                    LetterInline::Strong(content) => Inline::Strong {
                        content: resolved(content),
                    },
                    LetterInline::Emph(content) => Inline::Emph {
                        content: resolved(content),
                    },
                    LetterInline::Link { url, content } => Inline::Link {
                        url,
                        content: resolved(content),
                    },
                    LetterInline::Placeholder { .. } => return None, // forbidden() has already made the parse invalid
                })
            })
            .collect()
    }
    resolved(convert(nodes, src, sink))
}

pub(super) fn has_content(nodes: &[LetterInline]) -> bool {
    nodes.iter().any(|node| match node {
        LetterInline::Text(text) | LetterInline::Code(text) => !text.trim().is_empty(),
        LetterInline::Placeholder { .. } => true,
        LetterInline::Strong(content) | LetterInline::Emph(content) | LetterInline::Link { content, .. } => {
            has_content(content)
        }
    })
}

pub(super) fn unsupported(range: std::ops::Range<usize>, sink: &mut Sink<'_>) {
    sink.report(
        Finding::new(
            DiagnosticCode::LetterUnsupportedBlock,
            range,
            "unsupported cover-letter construct",
        )
        .hint("use paragraphs with text, emphasis, links or inline code"),
    );
}
fn malformed(range: std::ops::Range<usize>, sink: &mut Sink<'_>) {
    sink.report(
        Finding::new(
            DiagnosticCode::PlaceholderMalformed,
            range,
            "malformed or escaped letter placeholder",
        )
        .hint("keep {{company}}, {{title}} or {{recipient}} in one text run without backslashes"),
    );
}
fn push(output: &mut Vec<LetterInline>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(LetterInline::Text(previous)) = output.last_mut() {
        previous.push_str(text);
    } else {
        output.push(LetterInline::Text(text.into()));
    }
}

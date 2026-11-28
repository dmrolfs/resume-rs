//! ABOUTME: Parses one spec-owned Markdown leaf with the baseline inline converters.
//! ABOUTME: Findings use decoded-text ranges, never file offsets or automatic edits.

use std::ops::Range;
use std::path::Path;

use pulldown_cmark::Parser;

use super::tree::{BlockNode, InlineNode, ParagraphNode};
use super::{
    Finding, Sink, Source, annotation, enforce_span_contract, inline, letter_inline, letter_safety, lower_document,
    normalize_lone_cr, tree,
};
use crate::diagnostics::has_unsafe_multiline_characters;
use crate::diagnostics::{DiagnosticCode, Severity};
use crate::letter::LetterInline;
use crate::model::Inline;

/// The resume leaf that an addition supplies; placement is checked separately against the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum ResumeFragmentKind {
    Bullet,
    Paragraph,
    Blurb,
}

/// Lowered content only: annotation fields and placement belong to the spec declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeFragment {
    Bullet {
        label: Option<Vec<Inline>>,
        content: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    Blurb(Vec<Inline>),
}

/// A finding located in decoded Markdown, not in the TOML string that supplied it.
///
/// No file or edit is attached: only the spec loader can locate the owning literal safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentIssue {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub hints: Vec<String>,
    pub decoded: Range<usize>,
}

/// Valid content and the ordinary parser warnings that must still be shown to the author.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lowered<T> {
    pub value: T,
    pub warnings: Vec<FragmentIssue>,
}

/// Parses one resume leaf without inventing a containing Markdown document.
pub fn resume_fragment(kind: ResumeFragmentKind, text: &str) -> Result<Lowered<ResumeFragment>, Vec<FragmentIssue>> {
    let mut sink = Sink::new(Path::new(""));
    let value = paragraph(text, &mut sink).map(|paragraph| {
        let src = Source::new(text);
        match kind {
            ResumeFragmentKind::Bullet => {
                let (label, content) = inline::label_and_content(&paragraph.content, &src, &mut sink);
                ResumeFragment::Bullet { label, content }
            }
            ResumeFragmentKind::Paragraph => {
                ResumeFragment::Paragraph(inline::convert(&paragraph.content, &src, &mut sink))
            }
            ResumeFragmentKind::Blurb => ResumeFragment::Blurb(inline::convert(&paragraph.content, &src, &mut sink)),
        }
    });
    finish(value, sink, text)
}

/// Parses one letter body paragraph, retaining placeholder ranges in the decoded text.
pub fn letter_paragraph(text: &str) -> Result<Lowered<Vec<LetterInline>>, Vec<FragmentIssue>> {
    let mut sink = Sink::new(Path::new(""));
    let value = paragraph(text, &mut sink).map(|paragraph| {
        let content = letter_inline::convert(&paragraph.content, &Source::new(text), &mut sink);
        let owned = annotation::judged_in(&paragraph.content).into_iter().collect();
        let base = if text.starts_with(super::BOM) {
            super::BOM.len_utf8()
        } else {
            0
        };
        let normalized = normalize_lone_cr(&text[base..]);
        letter_safety::inspect(&normalized, base, &owned, &mut sink);
        content
    });
    finish(value, sink, text)
}

/// Checks shape before the permissive baseline converters can drop or flatten anything.
fn paragraph(text: &str, sink: &mut Sink<'_>) -> Option<ParagraphNode> {
    let doc = lower_document(text, sink)?;
    if has_unsafe_multiline_characters(text) {
        shape(
            0..text.len(),
            "content contains a control or direction-changing character",
            sink,
        );
    }
    let base = if text.starts_with(super::BOM) {
        super::BOM.len_utf8()
    } else {
        0
    };
    let normalized = normalize_lone_cr(&text[base..]);
    let parser = Parser::new_ext(&normalized, tree::options());
    for (_, definition) in parser.reference_definitions().iter() {
        shape(
            definition.span.start + base..definition.span.end + base,
            "reference definitions are not allowed in an addition",
            sink,
        );
    }
    let mut paragraph = None;
    for block in doc.blocks {
        match block {
            BlockNode::Paragraph(node) if paragraph.is_none() => {
                annotation::reject_in(&node.content, sink);
                inspect_inline(&node.content, sink);
                if inline::flatten_text(&node.content).trim().is_empty() {
                    shape(node.range.clone(), "content has no visible text", sink);
                }
                paragraph = Some(node);
            }
            BlockNode::Html(html) if annotation::comments(&html.text).is_some() => {
                if annotation::comments(&html.text)
                    .into_iter()
                    .flatten()
                    .any(|range| annotation::is_annotation(&html.text[range]))
                {
                    shape(
                        html.range,
                        "put annotations in the addition's fields, not its content",
                        sink,
                    );
                }
            }
            other => shape(
                other.range(),
                "content must be one paragraph without a list marker or other blocks",
                sink,
            ),
        }
    }
    if paragraph.is_none() {
        shape(0..text.len(), "content must contain one nonempty paragraph", sink);
    }
    paragraph
}

fn inspect_inline(nodes: &[InlineNode], sink: &mut Sink<'_>) {
    for node in nodes {
        let decoded = match node {
            InlineNode::Text(text) => Some(text.text.as_str()),
            InlineNode::Code { text, .. } => Some(text.as_str()),
            InlineNode::Link { dest, children, .. } => {
                inspect_inline(children, sink);
                Some(dest.as_str())
            }
            InlineNode::Strong { children, .. } | InlineNode::Emph { children, .. } => {
                inspect_inline(children, sink);
                None
            }
            InlineNode::Html { text, .. } if inline::is_comment(text) => None,
            InlineNode::Break { .. } => None,
            InlineNode::Html { .. } | InlineNode::Image { .. } | InlineNode::Unsupported { .. } => {
                shape(
                    node.range(),
                    "images, raw HTML and unsupported inline content are not allowed in an addition",
                    sink,
                );
                None
            }
        };
        if decoded.is_some_and(has_unsafe_multiline_characters) {
            shape(
                node.range(),
                "content decodes to a control or direction-changing character",
                sink,
            );
        }
    }
}

fn shape(range: Range<usize>, message: &str, sink: &mut Sink<'_>) {
    sink.report(Finding::new(DiagnosticCode::SpecFragmentShape, range, message));
}

fn finish<T>(value: Option<T>, sink: Sink<'_>, text: &str) -> Result<Lowered<T>, Vec<FragmentIssue>> {
    let mut diagnostics = sink.finish();
    enforce_span_contract(&mut diagnostics, text);
    let issues: Vec<_> = diagnostics
        .into_iter()
        .map(|diagnostic| FragmentIssue {
            code: diagnostic.code,
            severity: diagnostic.severity,
            message: diagnostic.message,
            hints: diagnostic.hints,
            decoded: diagnostic.span.expect("Markdown sink always supplies a span").range,
        })
        .collect();
    match value {
        Some(value) if !issues.iter().any(|issue| issue.severity == Severity::Error) => Ok(Lowered {
            value,
            warnings: issues,
        }),
        _ => Err(issues),
    }
}

#[cfg(test)]
mod tests;

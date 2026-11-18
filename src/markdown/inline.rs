//! ABOUTME: Converts located inline nodes to model Inlines: text, strong, emph, code, and validated links.
//! ABOUTME: Reports invalid or unsupported links, raw inline HTML, hard breaks, and extracts bullet labels.

use std::ops::Range;

use url::Url;

use super::tree::{InlineNode, TextNode};
use super::{Finding, Sink, Source, lint};
use crate::diagnostics::{DiagnosticCode, Edit};
use crate::model::{Inline, LinkUrl};

/// Why a link destination was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkProblem {
    /// `url` could not parse it; the parser's own message is kept for the hint.
    Invalid(url::ParseError),
    /// It parsed, but the scheme is not `http`, `https`, or `mailto`.
    Scheme(String),
}

/// Validates a link destination into a [`LinkUrl`]; email autolinks get their `mailto:` prefix first.
pub(crate) fn link_url(dest: &str, email: bool) -> Result<LinkUrl, LinkProblem> {
    let full = if email {
        format!("mailto:{dest}")
    } else {
        dest.to_string()
    };
    let url = Url::parse(&full).map_err(LinkProblem::Invalid)?;
    let scheme = url.scheme().to_string();
    LinkUrl::try_new(url).map_err(|_| LinkProblem::Scheme(scheme))
}

/// Converts an inline run; adjacent text is merged and breaks become spaces.
pub(crate) fn convert(nodes: &[InlineNode], src: &Source<'_>, sink: &mut Sink<'_>) -> Vec<Inline> {
    convert_run(nodes, true, src, sink)
}

/// Splits a leading `**Label:**` from a bullet's nodes and converts both parts.
///
/// The label is the bold run's content with the colon removed; `None` when the bullet has no label.
pub(crate) fn label_and_content(
    nodes: &[InlineNode],
    src: &Source<'_>,
    sink: &mut Sink<'_>,
) -> (Option<Vec<Inline>>, Vec<Inline>) {
    let Some((children, rest)) = split_label(nodes) else {
        return (None, convert(nodes, src, sink));
    };
    let mut label = convert_run(children, false, src, sink);
    strip_trailing_colon(&mut label);
    if label.is_empty() {
        return (None, convert(nodes, src, sink));
    }
    let mut content = convert(rest, src, sink);
    trim_leading_space(&mut content);
    (Some(label), content)
}

/// The children of a leading bold run whose text ends in a colon, and the nodes after it.
fn split_label(nodes: &[InlineNode]) -> Option<(&[InlineNode], &[InlineNode])> {
    let (InlineNode::Strong { children, .. }, rest) = nodes.split_first()? else {
        return None;
    };
    match children.last() {
        Some(InlineNode::Text(text)) if text.text.ends_with(':') => Some((children, rest)),
        _ => None,
    }
}

fn strip_trailing_colon(label: &mut Vec<Inline>) {
    if let Some(Inline::Text { text }) = label.last_mut() {
        text.truncate(text.trim_end_matches(':').len());
        if text.is_empty() {
            label.pop();
        }
    }
}

fn trim_leading_space(content: &mut Vec<Inline>) {
    if let Some(Inline::Text { text }) = content.first_mut() {
        *text = text.trim_start().to_string();
        if text.is_empty() {
            content.remove(0);
        }
    }
}

/// Plain text of heading or era content, joined across inline comments; `None` if it has any markup.
pub(crate) fn plain_text(nodes: &[InlineNode]) -> Option<TextNode> {
    let mut texts = Vec::new();
    for node in nodes {
        match node {
            InlineNode::Text(text) => texts.push(text),
            InlineNode::Html { text, .. } if is_comment(text) => {}
            _ => return None,
        }
    }
    Some(TextNode::concat(texts))
}

/// True for a complete inline HTML comment.
fn is_comment(html: &str) -> bool {
    html.starts_with("<!--") && html.ends_with("-->")
}

/// True when a node adds nothing visible: an HTML comment or whitespace.
pub(crate) fn is_blank(node: &InlineNode) -> bool {
    match node {
        InlineNode::Text(text) => text.text.trim().is_empty(),
        InlineNode::Html { text, .. } => is_comment(text),
        InlineNode::Break { .. } => true,
        _ => false,
    }
}

/// True when the run is one bold span and nothing else visible.
fn is_sole_strong(nodes: &[InlineNode]) -> bool {
    let mut visible = nodes.iter().filter(|n| !is_blank(n));
    matches!(
        (visible.next(), visible.next()),
        (Some(InlineNode::Strong { .. }), None)
    )
}

fn convert_run(nodes: &[InlineNode], top: bool, src: &Source<'_>, sink: &mut Sink<'_>) -> Vec<Inline> {
    let sole = top && is_sole_strong(nodes);
    let mut out = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let next = nodes.get(index + 1);
        convert_node(node, Context { sole, next }, src, sink, &mut out);
    }
    out
}

#[derive(Clone, Copy)]
struct Context<'n> {
    /// This node is the only visible content of its paragraph or bullet.
    sole: bool,
    next: Option<&'n InlineNode>,
}

fn convert_node(node: &InlineNode, ctx: Context<'_>, src: &Source<'_>, sink: &mut Sink<'_>, out: &mut Vec<Inline>) {
    match node {
        InlineNode::Text(text) => push_text(out, &text.text),
        InlineNode::Strong { range, children } => {
            if !ctx.sole {
                lint::mid_sentence_bold(range.clone(), colon_fix(range, ctx.next, src), sink);
            }
            let content = convert_run(children, false, src, sink);
            if !content.is_empty() {
                out.push(Inline::Strong { content });
            }
        }
        InlineNode::Emph { children, .. } => {
            let content = convert_run(children, false, src, sink);
            if !content.is_empty() {
                out.push(Inline::Emph { content });
            }
        }
        InlineNode::Link {
            range,
            dest,
            email,
            children,
        } => {
            convert_link(range, dest, *email, children, src, sink, out);
        }
        InlineNode::Code { text, .. } => out.push(Inline::Code { text: text.clone() }),
        InlineNode::Html { range, text } => {
            if !is_comment(text) {
                report_inline_html(range, text, sink);
            }
        }
        InlineNode::Image { range } => lint::image(range.clone(), sink),
        InlineNode::Break { range, hard } => {
            push_text(out, " ");
            if *hard {
                report_hard_break(range, src, sink);
            }
        }
        InlineNode::Unsupported { range, construct } => {
            let message = format!("{construct}s are not supported");
            let finding = Finding::new(DiagnosticCode::UnsupportedMarkdown, range.clone(), message)
                .hint("write plain text, bold, italics, code, and links only");
            sink.report(finding);
        }
    }
}

fn convert_link(
    range: &Range<usize>,
    dest: &str,
    email: bool,
    children: &[InlineNode],
    src: &Source<'_>,
    sink: &mut Sink<'_>,
    out: &mut Vec<Inline>,
) {
    let content = convert_run(children, false, src, sink);
    let visible = content
        .iter()
        .any(|inline| !matches!(inline, Inline::Text { text } if text.trim().is_empty()));
    match link_url(dest, email) {
        Ok(_) if !visible => {
            let finding = Finding::new(
                DiagnosticCode::InvalidLink,
                range.clone(),
                "the link has no visible text",
            )
            .hint("describe the link: [LinkedIn](https://www.linkedin.com/in/you)");
            sink.report(finding);
        }
        Ok(url) => out.push(Inline::Link { url, content }),
        Err(problem) => {
            sink.report(link_finding(range, dest, &problem));
            // Keep the text so the best-effort model loses nothing.
            for inline in content {
                match inline {
                    Inline::Text { text } => push_text(out, &text),
                    other => out.push(other),
                }
            }
        }
    }
}

fn link_finding(range: &Range<usize>, dest: &str, problem: &LinkProblem) -> Finding {
    match problem {
        LinkProblem::Invalid(error) => Finding::new(
            DiagnosticCode::InvalidLink,
            range.clone(),
            format!("`{dest}` is not a valid link"),
        )
        .hint(format!("{error}; links need a full address like https://example.com")),
        LinkProblem::Scheme(scheme) => Finding::new(
            DiagnosticCode::UnsupportedLink,
            range.clone(),
            format!("`{scheme}:` links are not supported"),
        )
        .hint("use https://, http://, or mailto: links"),
    }
}

fn report_inline_html(range: &Range<usize>, html: &str, sink: &mut Sink<'_>) {
    let finding = Finding::new(DiagnosticCode::UnsupportedHtml, range.clone(), format!("raw HTML `{html}` can't be typeset"))
        .hint("use Markdown (**bold**, *italic*, [links](https://example.com)); only comments and the page-break marker are allowed");
    sink.report(finding);
}

/// A hard break (two trailing spaces or a backslash) is almost always accidental; the edit removes it.
///
/// No edit is offered when a literal backslash precedes trailing spaces: deleting the spaces would
/// leave a backslash before the newline, which is itself a hard break.
fn report_hard_break(range: &Range<usize>, src: &Source<'_>, sink: &mut Sink<'_>) {
    let written = src.slice(range);
    let visible = written.trim_end_matches(['\r', '\n']);
    let span = range.start..range.start + visible.len();
    let spaces_after_backslash = !visible.starts_with('\\') && src.text[..range.start].ends_with('\\');
    let mut finding = Finding::new(
        DiagnosticCode::HardBreak,
        span.clone(),
        "a hard line break is rendered as a space",
    )
    .hint("remove the trailing spaces or backslash; a blank line starts a new paragraph");
    if !spaces_after_backslash {
        finding = finding.fix(span, "");
    }
    sink.report(finding);
}

/// For `**Label**: text`, the edit that moves the colon inside the bold: `**Label:** text`.
fn colon_fix(strong: &Range<usize>, next: Option<&InlineNode>, src: &Source<'_>) -> Option<Edit> {
    let Some(InlineNode::Text(text)) = next else {
        return None;
    };
    if !text.text.starts_with(':') || strong.len() < 4 {
        return None;
    }
    let colon = strong.end..strong.end + 1;
    let delimiter = strong.end - 2..strong.end;
    if src.slice(&colon) != ":" || !matches!(src.slice(&delimiter), "**" | "__") {
        return None;
    }
    Some(Edit {
        range: delimiter.start..colon.end,
        replacement: format!(":{}", src.slice(&delimiter)),
    })
}

/// Appends `more` after `out`, separated by a space, merging the text runs at the join.
pub(crate) fn append_with_space(out: &mut Vec<Inline>, more: Vec<Inline>) {
    push_text(out, " ");
    for inline in more {
        match inline {
            Inline::Text { text } => push_text(out, &text),
            other => out.push(other),
        }
    }
}

/// Appends text, merging with a preceding text run.
fn push_text(out: &mut Vec<Inline>, text: &str) {
    if let Some(Inline::Text { text: last }) = out.last_mut() {
        last.push_str(text);
    } else {
        out.push(Inline::Text { text: text.to_string() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostic;
    use crate::markdown::tree::{self, BlockNode};
    use claims::assert_some;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::path::Path;

    fn run(
        markdown: &str,
        f: impl FnOnce(&[InlineNode], &Source<'_>, &mut Sink<'_>) -> Value,
    ) -> (Value, Vec<Diagnostic>) {
        let mut sink = Sink::new(Path::new("r.md"));
        let doc = tree::lower(markdown, 0, &mut sink);
        let src = Source { text: markdown };
        let Some(BlockNode::Paragraph(p)) = doc.blocks.first() else {
            panic!("expected a paragraph in {markdown:?}")
        };
        let value = f(&p.content, &src, &mut sink);
        (value, sink.finish())
    }

    fn converted(markdown: &str) -> (Value, Vec<Diagnostic>) {
        run(markdown, |nodes, src, sink| {
            serde_json::to_value(convert(nodes, src, sink)).unwrap_or(Value::Null)
        })
    }

    fn codes(diagnostics: &[Diagnostic]) -> Vec<DiagnosticCode> {
        diagnostics.iter().map(|d| d.code).collect()
    }

    #[test]
    fn converts_text_strong_emph_code() {
        let (value, diagnostics) = converted("plain *soft* and `code` here");
        assert_eq!(
            value,
            json!([
                {"kind": "text", "text": "plain "},
                {"kind": "emph", "content": [{"kind": "text", "text": "soft"}]},
                {"kind": "text", "text": " and "},
                {"kind": "code", "text": "code"},
                {"kind": "text", "text": " here"},
            ])
        );
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn smart_punctuation_converts_quotes_and_dashes() {
        let (value, _) = converted("\"Quoted\" -- don't...");
        assert_eq!(value, json!([{"kind": "text", "text": "“Quoted” – don’t…"}]));
    }

    #[test]
    fn soft_break_becomes_space() {
        let (lf, _) = converted("one\ntwo");
        let (crlf, _) = converted("one\r\ntwo");
        assert_eq!(lf, json!([{"kind": "text", "text": "one two"}]));
        assert_eq!(lf, crlf);
    }

    #[test]
    fn hard_break_warns_with_edit() {
        let (value, diagnostics) = converted("one  \ntwo");
        assert_eq!(value, json!([{"kind": "text", "text": "one two"}]));
        let d = &diagnostics[0];
        assert_eq!(d.code, DiagnosticCode::HardBreak);
        assert_eq!(assert_some!(d.span.as_ref()).range, 3..5);
        let fix = assert_some!(d.fix.as_ref());
        assert_eq!((fix.range.clone(), fix.replacement.as_str()), (3..5, ""));
        let (_, diagnostics) = converted("one\\\ntwo");
        let fix = assert_some!(diagnostics[0].fix.as_ref());
        assert_eq!(fix.range, 3..4, "the backslash");
    }

    #[test]
    fn hard_break_after_a_literal_backslash_has_no_edit() {
        // Deleting the spaces would leave `\` before the newline, which is itself a hard break.
        let (_, diagnostics) = converted("one\\  \ntwo");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::HardBreak]);
        assert_eq!(diagnostics[0].fix, None);
        assert!(!diagnostics[0].hints.is_empty());
    }

    #[test]
    fn hard_break_edit_excludes_crlf() {
        let (_, diagnostics) = converted("one  \r\ntwo");
        assert_eq!(assert_some!(diagnostics[0].fix.as_ref()).range, 3..5);
    }

    #[test]
    fn inline_html_is_unsupported_html() {
        let (value, diagnostics) = converted("a <b>bold</b> c");
        assert_eq!(
            codes(&diagnostics),
            vec![DiagnosticCode::UnsupportedHtml, DiagnosticCode::UnsupportedHtml]
        );
        assert_eq!(value, json!([{"kind": "text", "text": "a bold c"}]));
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 2..5);
    }

    #[test]
    fn inline_comment_is_dropped() {
        let (value, diagnostics) = converted("a <!-- note --> b");
        assert_eq!(value, json!([{"kind": "text", "text": "a  b"}]));
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn image_warns_and_is_dropped() {
        let (value, diagnostics) = converted("see ![photo](me.png) here");
        assert_eq!(value, json!([{"kind": "text", "text": "see  here"}]));
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::Image]);
    }

    #[test]
    fn valid_link_is_kept() {
        let (value, diagnostics) = converted("[Docs](https://example.com/a?b=1)");
        assert_eq!(
            value,
            json!([{"kind": "link", "url": "https://example.com/a?b=1", "content": [{"kind": "text", "text": "Docs"}]}])
        );
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn malformed_link_is_invalid_link_with_parser_hint() {
        let (_, diagnostics) = converted("[home](http://)");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::InvalidLink]);
        assert!(
            diagnostics[0].hints[0].contains("empty host"),
            "{:?}",
            diagnostics[0].hints
        );
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 0..15);
    }

    #[test]
    fn relative_link_is_invalid_link() {
        let (value, diagnostics) = converted("[notes](notes.md)");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::InvalidLink]);
        assert!(
            diagnostics[0].hints[0].contains("relative URL without a base"),
            "{:?}",
            diagnostics[0].hints
        );
        assert_eq!(value, json!([{"kind": "text", "text": "notes"}]), "the text is kept");
    }

    #[test]
    fn ftp_link_is_unsupported_link() {
        for dest in [
            "ftp://example.com/x",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/plain,hi",
            "tel:+15555550100",
        ] {
            let (value, diagnostics) = converted(&format!("[x]({dest})"));
            assert_eq!(codes(&diagnostics), vec![DiagnosticCode::UnsupportedLink], "{dest}");
            assert_eq!(value, json!([{"kind": "text", "text": "x"}]), "{dest}");
        }
        let (_, diagnostics) = converted("[x](ftp://example.com/x)");
        assert_eq!(diagnostics[0].message, "`ftp:` links are not supported");
    }

    #[test]
    fn email_autolink_gets_mailto() {
        let (value, diagnostics) = converted("<ada@example.com>");
        assert_eq!(diagnostics, vec![]);
        assert_eq!(
            value,
            json!([{"kind": "link", "url": "mailto:ada@example.com", "content": [{"kind": "text", "text": "ada@example.com"}]}])
        );
    }

    #[test]
    fn mailto_and_http_links_are_valid() {
        let (_, diagnostics) = converted("[a](mailto:a@b.co) [b](http://x.y)");
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn empty_link_text_is_invalid_link() {
        let (value, diagnostics) = converted("a [](https://example.com) b");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::InvalidLink]);
        assert_eq!(value, json!([{"kind": "text", "text": "a  b"}]));
    }

    #[test]
    fn link_content_keeps_nested_markup() {
        let (value, _) = converted("[**Acme** site](https://acme.co)");
        assert_eq!(
            value,
            json!([{"kind": "link", "url": "https://acme.co/", "content": [
                {"kind": "strong", "content": [{"kind": "text", "text": "Acme"}]},
                {"kind": "text", "text": " site"}
            ]}])
        );
    }

    #[test]
    fn whole_paragraph_bold_is_not_mid_sentence() {
        let (_, diagnostics) = converted("**Just bold**");
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn mid_sentence_bold_warns() {
        let (_, diagnostics) = converted("this is **important** text");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::MidSentenceBold]);
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 8..21);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn bold_nested_in_emphasis_inside_a_sentence_warns() {
        let (_, diagnostics) = converted("a *very **bold** word* b");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::MidSentenceBold]);
    }

    #[test]
    fn label_colon_outside_bold_warns_with_edit() {
        let (_, diagnostics) = converted("**Languages**: Rust");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::MidSentenceBold]);
        let fix = assert_some!(diagnostics[0].fix.as_ref());
        let mut fixed = "**Languages**: Rust".to_string();
        fixed.replace_range(fix.range.clone(), &fix.replacement);
        assert_eq!(fixed, "**Languages:** Rust");
    }

    #[test]
    fn underscore_bold_colon_outside_is_fixed_too() {
        let (_, diagnostics) = converted("__Languages__: Rust");
        let fix = assert_some!(diagnostics[0].fix.as_ref());
        let mut fixed = "__Languages__: Rust".to_string();
        fixed.replace_range(fix.range.clone(), &fix.replacement);
        assert_eq!(fixed, "__Languages:__ Rust");
    }

    #[test]
    fn label_is_split_from_content() {
        let (value, diagnostics) = run("**Languages:** Rust, Java", |nodes, src, sink| {
            let (label, content) = label_and_content(nodes, src, sink);
            json!({"label": label, "content": content})
        });
        assert_eq!(
            value,
            json!({
                "label": [{"kind": "text", "text": "Languages"}],
                "content": [{"kind": "text", "text": "Rust, Java"}]
            })
        );
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn label_may_contain_a_link() {
        let (value, diagnostics) = run("**[Acme](https://acme.co), VP:** led it", |nodes, src, sink| {
            let (label, content) = label_and_content(nodes, src, sink);
            json!({"label": label, "content": content})
        });
        assert_eq!(value["label"][0]["kind"], json!("link"));
        assert_eq!(value["label"][1], json!({"kind": "text", "text": ", VP"}));
        assert_eq!(value["content"], json!([{"kind": "text", "text": "led it"}]));
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn bullet_without_label_keeps_all_content() {
        let (value, _) = run("plain **bold** text", |nodes, src, sink| {
            let (label, content) = label_and_content(nodes, src, sink);
            json!({"has_label": label.is_some(), "len": content.len()})
        });
        assert_eq!(value, json!({"has_label": false, "len": 3}));
    }

    #[test]
    fn plain_text_joins_across_comments_and_rejects_markup() {
        let mut sink = Sink::new(Path::new("r.md"));
        let doc = tree::lower("A <!-- c --> B\n\n*italic* C\n", 0, &mut sink);
        let BlockNode::Paragraph(first) = &doc.blocks[0] else {
            panic!("paragraph")
        };
        let BlockNode::Paragraph(second) = &doc.blocks[1] else {
            panic!("paragraph")
        };
        assert_eq!(assert_some!(plain_text(&first.content)).text, "A  B");
        assert!(plain_text(&second.content).is_none());
    }

    #[test]
    fn link_url_reports_the_scheme() {
        assert_eq!(link_url("ftp://x.y", false), Err(LinkProblem::Scheme("ftp".into())));
        assert!(matches!(link_url("nope", false), Err(LinkProblem::Invalid(_))));
        assert!(link_url("a@b.co", true).is_ok());
    }
}

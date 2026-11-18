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

/// Where an inline run sits; decides which lints and Edits apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    /// Inside emphasis, strong, or a link.
    Nested,
    /// The top level of a paragraph, blurb, or contact line.
    Paragraph,
    /// The top level of a bullet, where a leading bold run may be a label.
    Bullet,
}

/// Converts the top level of a paragraph, blurb, or contact line.
///
/// Adjacent text is merged, breaks become spaces, and the spaces that dropped comments or images
/// leave at the edges and joins are removed.
pub(crate) fn convert(nodes: &[InlineNode], src: &Source<'_>, sink: &mut Sink<'_>) -> Vec<Inline> {
    convert_run(nodes, Run::Paragraph, src, sink)
}

/// Splits a leading `**Label:**` from a bullet's nodes and converts both parts.
///
/// The label is the bold run's content with the colon removed; `None` when the bullet has no label.
pub(crate) fn label_and_content(
    nodes: &[InlineNode],
    src: &Source<'_>,
    sink: &mut Sink<'_>,
) -> (Option<Vec<Inline>>, Vec<Inline>) {
    let label_children = split_label(nodes).filter(|(children, _)| !label_is_empty(children));
    let Some((children, rest)) = label_children else {
        return (None, convert_run(nodes, Run::Bullet, src, sink));
    };
    let mut label = convert_run(children, Run::Nested, src, sink);
    strip_trailing_colon(&mut label);
    let content = convert_run(rest, Run::Paragraph, src, sink);
    ((!label.is_empty()).then_some(label), content)
}

/// True when a label's text is only colons and whitespace, so the bold run is not a label at all.
///
/// Decided from the text alone, so the children are converted (and their findings reported) once.
fn label_is_empty(children: &[InlineNode]) -> bool {
    flatten_text(children).trim_end_matches(':').trim().is_empty()
}

/// All visible text of `nodes`, with markup removed; comments, images, and raw HTML contribute nothing.
pub(crate) fn flatten_text(nodes: &[InlineNode]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            InlineNode::Text(text) => text.text.clone(),
            InlineNode::Code { text, .. } => text.clone(),
            InlineNode::Strong { children, .. }
            | InlineNode::Emph { children, .. }
            | InlineNode::Link { children, .. } => flatten_text(children),
            InlineNode::Break { .. } => " ".to_string(),
            InlineNode::Html { .. } | InlineNode::Image { .. } | InlineNode::Unsupported { .. } => String::new(),
        })
        .collect()
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

/// Plain text of heading or era content, joined across inline comments; `None` if it has any markup.
///
/// A soft line break reads as a space, so an era line wrapped over two lines is still one line.
pub(crate) fn plain_text(nodes: &[InlineNode]) -> Option<TextNode> {
    let mut joined = TextNode::default();
    for node in nodes {
        match node {
            InlineNode::Text(text) => joined = TextNode::concat([&joined, text]),
            InlineNode::Break { range, hard: false } => joined.push(" ", range.clone(), false),
            InlineNode::Html { text, .. } if is_comment(text) => {}
            _ => return None,
        }
    }
    Some(joined)
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

fn convert_run(nodes: &[InlineNode], run: Run, src: &Source<'_>, sink: &mut Sink<'_>) -> Vec<Inline> {
    let sole = run != Run::Nested && is_sole_strong(nodes);
    let mut out = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let ctx = Context {
            sole,
            next: nodes.get(index + 1),
            label_position: run == Run::Bullet && index == 0,
        };
        convert_node(node, ctx, src, sink, &mut out);
    }
    if run != Run::Nested {
        trim_edges(&mut out);
    }
    out
}

#[derive(Clone, Copy)]
struct Context<'n> {
    /// This node is the only visible content of its paragraph or bullet.
    sole: bool,
    next: Option<&'n InlineNode>,
    /// This node starts a bullet, the only place `**Label**:` can be fixed by moving the colon.
    label_position: bool,
}

/// Removes the spaces that dropped comments or images leave at the start and end of a run.
fn trim_edges(out: &mut Vec<Inline>) {
    if let Some(Inline::Text { text }) = out.first_mut() {
        *text = text.trim_start().to_string();
        if text.is_empty() {
            out.remove(0);
        }
    }
    if let Some(Inline::Text { text }) = out.last_mut() {
        *text = text.trim_end().to_string();
        if text.is_empty() {
            out.pop();
        }
    }
}

fn convert_node(node: &InlineNode, ctx: Context<'_>, src: &Source<'_>, sink: &mut Sink<'_>, out: &mut Vec<Inline>) {
    match node {
        InlineNode::Text(text) => {
            report_unclosed_comment(text, src, sink);
            push_text(out, &text.text);
        }
        InlineNode::Strong { range, children } => {
            if !ctx.sole {
                let edit = if ctx.label_position {
                    colon_fix(range, ctx.next, src)
                } else {
                    None
                };
                lint::mid_sentence_bold(range.clone(), edit, sink);
            }
            let content = convert_run(children, Run::Nested, src, sink);
            if !content.is_empty() {
                out.push(Inline::Strong { content });
            }
        }
        InlineNode::Emph { children, .. } => {
            let content = convert_run(children, Run::Nested, src, sink);
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
    let content = convert_run(children, Run::Nested, src, sink);
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

/// A `<!--` in text never closed (a closed one is an inline HTML node), so everything after it would
/// print on the resume, with `--` turned into an en dash.
fn report_unclosed_comment(text: &TextNode, src: &Source<'_>, sink: &mut Sink<'_>) {
    let range = text.range();
    let Some(at) = src.slice(&range).find("<!--") else {
        return;
    };
    let start = range.start + at;
    let finding = Finding::new(
        DiagnosticCode::UnsupportedHtml,
        start..start + 4,
        "the comment is never closed, so its text would print on the resume",
    )
    .hint("end the comment with `-->`, or remove the `<!--`");
    sink.report(finding);
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

/// For a leading `**Label**: text`, the edit that moves the colon inside the bold: `**Label:** text`.
fn colon_fix(strong: &Range<usize>, next: Option<&InlineNode>, src: &Source<'_>) -> Option<Edit> {
    let Some(InlineNode::Text(text)) = next else {
        return None;
    };
    if !text.text.starts_with(':') {
        return None;
    }
    // The colon must be a literal `:` in the source (not an entity such as `&#58;`) right after the
    // closing delimiter, which is the last two bytes of a bold span.
    let colon = strong.end..strong.end + 1;
    let delimiter = strong.end.checked_sub(2)?..strong.end;
    if src.slice(&colon) != ":" {
        return None;
    }
    // After the Edit the closing delimiter follows a colon, so CommonMark only closes the bold if
    // whitespace (or the end of the text) comes next; `**Label:**text` would not close.
    if !src.text[colon.end..].chars().next().is_none_or(char::is_whitespace) {
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
///
/// Two text runs only meet when a node between them was dropped (a comment or an image), so the
/// space on both sides of the gap becomes one. Spacing inside a single run is the author's.
fn push_text(out: &mut Vec<Inline>, text: &str) {
    if let Some(Inline::Text { text: last }) = out.last_mut() {
        last.push_str(if last.ends_with(' ') {
            text.trim_start_matches(' ')
        } else {
            text
        });
    } else {
        out.push(Inline::Text { text: text.to_string() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostic;
    use crate::markdown::testutil::codes;
    use crate::markdown::tree::{self, BlockNode};
    use claims::{assert_none, assert_ok, assert_some};
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
        assert_eq!(value, json!([{"kind": "text", "text": "a b"}]));
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn image_warns_and_is_dropped() {
        let (value, diagnostics) = converted("see ![photo](me.png) here");
        assert_eq!(value, json!([{"kind": "text", "text": "see here"}]));
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
        assert_eq!(value, json!([{"kind": "text", "text": "a b"}]));
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

    fn bullet(markdown: &str) -> (Value, Vec<Diagnostic>) {
        run(markdown, |nodes, src, sink| {
            let (label, content) = label_and_content(nodes, src, sink);
            json!({"label": label, "content": content})
        })
    }

    #[test]
    fn label_colon_outside_bold_warns_with_edit() {
        let (_, diagnostics) = bullet("**Languages**: Rust");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::MidSentenceBold]);
        let fix = assert_some!(diagnostics[0].fix.as_ref());
        let mut fixed = "**Languages**: Rust".to_string();
        fixed.replace_range(fix.range.clone(), &fix.replacement);
        assert_eq!(fixed, "**Languages:** Rust");
    }

    #[test]
    fn underscore_bold_colon_outside_is_fixed_too() {
        let (_, diagnostics) = bullet("__Languages__: Rust");
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
        assert_none!(plain_text(&second.content));
    }

    #[test]
    fn link_url_reports_the_scheme() {
        assert_eq!(link_url("ftp://x.y", false), Err(LinkProblem::Scheme("ftp".into())));
        assert!(matches!(link_url("nope", false), Err(LinkProblem::Invalid(_))));
        assert_ok!(link_url("a@b.co", true));
    }

    #[test]
    fn colon_edit_needs_whitespace_after_the_colon() {
        // `**Label:**text` would not close the bold (the closing `**` is not right-flanking).
        let (_, diagnostics) = bullet("**Label**:text here");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::MidSentenceBold]);
        assert_eq!(diagnostics[0].fix, None);
        let (_, diagnostics) = bullet("**Label**:");
        assert_some!(diagnostics[0].fix.as_ref(), "a colon at the end of the line is fine");
    }

    #[test]
    fn colon_edit_is_only_offered_at_label_position() {
        for markdown in ["Use **Rust**: fast", "a **B**: c"] {
            let (_, diagnostics) = bullet(markdown);
            assert_eq!(diagnostics[0].fix, None, "{markdown}");
        }
        // A paragraph, a blurb, or the contact line is not a label position.
        let (_, diagnostics) = converted("**Email**: a@b.co");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::MidSentenceBold]);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn only_the_leading_bold_of_a_bullet_gets_the_colon_edit() {
        let (_, diagnostics) = bullet("**L**: a **M**: b");
        assert_eq!(diagnostics.len(), 2);
        assert_some!(diagnostics[0].fix.as_ref());
        assert_eq!(diagnostics[1].fix, None);
    }

    #[test]
    fn unclosed_inline_comment_is_unsupported_html() {
        let (_, diagnostics) = converted("Text <!-- more");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::UnsupportedHtml]);
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 5..9);
        assert!(
            diagnostics[0].message.contains("never closed"),
            "{}",
            diagnostics[0].message
        );
        let (_, diagnostics) = converted("`<!--` in code and <!-- closed --> are fine");
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn dropped_nodes_do_not_leave_stray_spaces() {
        let (value, _) = converted("![me](me.png) Seattle");
        assert_eq!(value, json!([{"kind": "text", "text": "Seattle"}]));
        let (value, _) = converted("**Bold** <!-- c -->");
        assert_eq!(
            value,
            json!([{"kind": "strong", "content": [{"kind": "text", "text": "Bold"}]}])
        );
        let (value, _) = converted("a <!-- one --> <!-- two --> b");
        assert_eq!(value, json!([{"kind": "text", "text": "a b"}]));
    }

    #[test]
    fn authored_inner_spacing_is_kept() {
        let (value, _) = converted("a  b");
        assert_eq!(value, json!([{"kind": "text", "text": "a  b"}]));
    }

    #[test]
    fn empty_label_reports_findings_once() {
        let (value, diagnostics) = bullet("**![i](a.png):** text");
        let images = diagnostics.iter().filter(|d| d.code == DiagnosticCode::Image).count();
        assert_eq!(images, 1, "{diagnostics:?}");
        assert_eq!(value["label"], Value::Null);
        let kept = value["content"].to_string();
        assert!(
            kept.contains(":") && kept.contains("text"),
            "the bold text is kept, not dropped: {kept}"
        );
    }

    #[test]
    fn a_label_may_be_only_a_link() {
        let (value, diagnostics) = bullet("**[Acme](https://acme.co):** led it");
        assert_eq!(value["label"][0]["kind"], json!("link"));
        assert_eq!(
            value["label"].as_array().map(Vec::len),
            Some(1),
            "the colon is not a label of its own"
        );
        assert_eq!(value["content"], json!([{"kind": "text", "text": "led it"}]));
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn a_dropped_image_before_bold_leaves_no_leading_space() {
        let (value, _) = converted("![i](a.png) **Bold**");
        assert_eq!(
            value,
            json!([{"kind": "strong", "content": [{"kind": "text", "text": "Bold"}]}])
        );
    }

    #[test]
    fn an_invalid_link_keeps_its_formatted_content() {
        let (value, diagnostics) = converted("[**x**](ftp://a.b)");
        assert!(
            codes(&diagnostics).contains(&DiagnosticCode::UnsupportedLink),
            "{diagnostics:?}"
        );
        assert_eq!(
            value,
            json!([{"kind": "strong", "content": [{"kind": "text", "text": "x"}]}])
        );
    }

    #[test]
    fn colon_edit_is_withheld_when_the_delimiters_are_not_a_plain_pair() {
        for markdown in ["***Label***: x", "**Label** : x", "**Label**; x"] {
            let (_, diagnostics) = bullet(markdown);
            assert!(
                !diagnostics.is_empty(),
                "{markdown}: bold followed by text is a finding"
            );
            assert_eq!(diagnostics[0].fix, None, "{markdown}");
        }
    }

    #[test]
    fn a_bold_colon_alone_is_not_a_label() {
        let (value, diagnostics) = bullet("**:** text");
        assert_eq!(value["label"], Value::Null);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::MidSentenceBold]);
        assert_eq!(value["content"][0]["kind"], json!("strong"));
    }

    #[test]
    fn a_bold_run_followed_only_by_a_comment_is_still_the_whole_paragraph() {
        let (_, diagnostics) = converted("**Bold** <!-- note -->");
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn colon_edit_needs_a_literal_colon() {
        // `&#58;` decodes to a colon, but the source bytes after the bold are not `:`.
        let (_, diagnostics) = bullet("**Label**&#58; text");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::MidSentenceBold]);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn a_comment_needs_both_ends() {
        for complete in ["<!-- x -->", "<!-->", "<!--->", "<!---->"] {
            assert!(is_comment(complete), "{complete}");
        }
        for other in ["<!-- x", "x -->", "<b>", "-->", "<!-"] {
            assert!(!is_comment(other), "{other}");
        }
    }
}

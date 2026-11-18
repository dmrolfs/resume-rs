//! ABOUTME: Phase 2: interprets the located tree into the Resume model (name, contact, sections, roles, eras).
//! ABOUTME: Reports every convention violation with its range and hint, and recovers so parsing never aborts.

use std::ops::Range;

use super::inline;
use super::lint::{self, RoleKey};
use super::role::{self, EraCheck, EraHead, RoleHead};
use super::tree::{
    BlockNode, Construct, Document, HeadingNode, HtmlNode, ItemNode, ListNode, ParagraphNode, UnsupportedNode,
};
use super::{Finding, Sink, Source};
use crate::diagnostics::DiagnosticCode;
use crate::model::text::{NameError, SectionTitleError};
use crate::model::{Block, Bullet, Inline, Name, Resume, Role, RoleBlock, Section, SectionTitle};

/// The one HTML construct allowed besides comments: a forced page break.
const PAGE_BREAK: &str = r#"<div class="page-break"></div>"#;

/// Interprets the document; `None` only when no valid name heading exists.
pub(crate) fn interpret(doc: &Document, src: &Source<'_>, sink: &mut Sink<'_>) -> Option<Resume> {
    let blocks = &doc.blocks;
    let name_heading = blocks.iter().enumerate().find_map(|(index, block)| match block {
        BlockNode::Heading(heading) if heading.level == 1 => Some((index, heading)),
        _ => None,
    });
    let Some((at, heading)) = name_heading else {
        report_missing_name(blocks, sink);
        walk_sections(blocks, src, sink);
        return None;
    };
    for block in blocks[..at].iter().filter(|b| !is_comment(b)) {
        report_before_section(block.range(), "the name heading", sink);
    }
    let name = name(heading, sink);
    let rest = &blocks[at + 1..];
    let (contact, consumed) = contact(heading, rest, src, sink);
    let sections = walk_sections(&rest[consumed..], src, sink);
    name.map(|name| Resume::new(name, contact, sections))
}

fn report_missing_name(blocks: &[BlockNode], sink: &mut Sink<'_>) {
    let start = blocks.iter().find(|b| !is_comment(b)).map_or(0, |b| b.range().start);
    let finding = Finding::new(
        DiagnosticCode::MissingName,
        start..start,
        "the resume has no `# Name` heading",
    )
    .hint("start the file with `# Your Name` (comments may come first)");
    sink.report(finding);
}

/// Reports content that appears where only sections may start.
fn report_before_section(range: Range<usize>, before: &str, sink: &mut Sink<'_>) {
    let finding = Finding::new(
        DiagnosticCode::ContentBeforeSection,
        range,
        format!("content before {before}"),
    )
    .hint("only the name and the contact line may come before the first `## Section`; move this under a section");
    sink.report(finding);
}

fn name(heading: &HeadingNode, sink: &mut Sink<'_>) -> Option<Name> {
    let text = heading_text(heading, sink);
    match Name::try_new(text) {
        Ok(name) => Some(name),
        Err(NameError::NotEmptyViolated) => {
            report_empty_heading(heading, "the name heading is empty", "write `# Your Name`", sink);
            None
        }
        Err(NameError::LenCharMaxViolated) => {
            report_too_long(heading, "the name", 80, sink);
            None
        }
    }
}

/// Plain text of a heading; markup and setext underlines are reported and then flattened.
fn heading_text(heading: &HeadingNode, sink: &mut Sink<'_>) -> String {
    if heading.setext {
        let message = "underlined (setext) headings are not supported";
        unsupported(
            heading.range.clone(),
            message,
            "write `# Name` or `## Section` with `#` markers",
            sink,
        );
    }
    if inline::plain_text(&heading.content).is_none() {
        let message = "headings must be plain text";
        unsupported(
            role::content_range(heading),
            message,
            "remove the bold, italics, links, or code",
            sink,
        );
    }
    inline::flatten_text(&heading.content)
}

fn report_empty_heading(heading: &HeadingNode, message: &str, hint: &str, sink: &mut Sink<'_>) {
    sink.report(Finding::new(DiagnosticCode::EmptyHeading, heading.range.clone(), message).hint(hint));
}

fn report_too_long(heading: &HeadingNode, what: &str, max: usize, sink: &mut Sink<'_>) {
    let message = format!("{what} is longer than {max} characters");
    let finding = Finding::new(DiagnosticCode::TextTooLong, role::content_range(heading), message).hint("shorten it");
    sink.report(finding);
}

fn unsupported(range: Range<usize>, message: &str, hint: &str, sink: &mut Sink<'_>) {
    sink.report(Finding::new(DiagnosticCode::UnsupportedMarkdown, range, message).hint(hint));
}

/// Warns when a paragraph starts with `#` markers that no space follows: it is text, not a heading,
/// so the hashes would print and the content would land in the previous section or role.
fn check_heading_marker(paragraph: &ParagraphNode, src: &Source<'_>, sink: &mut Sink<'_>) {
    let text = src.slice(&paragraph.range);
    let hashes = text.bytes().take_while(|b| *b == b'#').count();
    let next_is_text = text[hashes..].chars().next().is_some_and(|c| !c.is_whitespace());
    if (1..=6).contains(&hashes) && next_is_text {
        let range = paragraph.range.start..paragraph.range.start + hashes;
        let marker = &text[..hashes];
        let message = format!("`{marker}` has no space after it, so this line is text, not a heading");
        let finding = Finding::new(DiagnosticCode::HeadingMarkerSpacing, range, message).hint(format!(
            "write `{marker} ` with a space, or remove the `#` if this is meant to be text"
        ));
        sink.report(finding);
    }
}

/// The contact paragraph directly after the name, and how many blocks it consumed.
fn contact(name: &HeadingNode, rest: &[BlockNode], src: &Source<'_>, sink: &mut Sink<'_>) -> (Vec<Inline>, usize) {
    let first = rest.iter().position(|b| !is_comment(b));
    if let Some(index) = first
        && let BlockNode::Paragraph(paragraph) = &rest[index]
    {
        check_heading_marker(paragraph, src, sink);
        lint::contact_items(&paragraph.content, paragraph.range.clone(), sink);
        return (inline::convert(&paragraph.content, src, sink), index + 1);
    }
    let finding = Finding::new(
        DiagnosticCode::MissingContact,
        name.range.clone(),
        "the name has no contact line",
    )
    .hint("add a line with location, email, phone, and links right after the name, separated by ` · `");
    sink.report(finding);
    (Vec::new(), 0)
}

// ---------------------------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------------------------

/// What an HTML block is, judged from its text alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HtmlClass {
    /// Only well-formed comments.
    Comment,
    PageBreak,
    /// A comment that never closes swallows the rest of the document.
    UnclosedComment,
    /// Text after `-->` on the same line is part of the HTML block and is hidden.
    TextAfterComment,
    /// More lines than the marker: the next line was swallowed into the block.
    PageBreakNotIsolated,
    Other,
}

fn classify_html(text: &str) -> HtmlClass {
    let trimmed = text.trim();
    if trimmed == PAGE_BREAK {
        return HtmlClass::PageBreak;
    }
    if trimmed.starts_with(PAGE_BREAK) {
        return HtmlClass::PageBreakNotIsolated;
    }
    if !trimmed.starts_with("<!--") {
        return HtmlClass::Other;
    }
    let mut rest = trimmed;
    while let Some(after_open) = rest.strip_prefix("<!--") {
        // CommonMark 0.31: `<!-->` and `<!--->` are complete, empty comments.
        let after = if let Some(after) = after_open.strip_prefix('>') {
            after
        } else if let Some(after) = after_open.strip_prefix("->") {
            after
        } else {
            let Some(close) = after_open.find("-->") else {
                return HtmlClass::UnclosedComment;
            };
            &after_open[close + 3..]
        };
        rest = after.trim_start();
    }
    if rest.is_empty() {
        HtmlClass::Comment
    } else {
        HtmlClass::TextAfterComment
    }
}

fn is_comment(block: &BlockNode) -> bool {
    matches!(block, BlockNode::Html(html) if classify_html(&html.text) == HtmlClass::Comment)
}

/// Classifies an HTML block, reporting everything but comments and the page-break marker.
fn html_block(node: &HtmlNode, src: &Source<'_>, sink: &mut Sink<'_>) -> HtmlClass {
    let class = classify_html(&node.text);
    let (range, message, hint) = match class {
        HtmlClass::Comment | HtmlClass::PageBreak => return class,
        HtmlClass::UnclosedComment => {
            let line = src.line_of(node.range.start);
            let message = format!("the comment is never closed; everything after line {line} is hidden");
            (
                node.range.start..(node.range.start + 4).min(node.range.end),
                message,
                "end the comment with `-->`",
            )
        }
        HtmlClass::TextAfterComment => (
            node.range.clone(),
            "text after a comment on the same line is hidden".to_string(),
            "put the text on its own line, after a blank line",
        ),
        HtmlClass::PageBreakNotIsolated => (
            node.range.clone(),
            "the page-break marker swallowed the lines after it".to_string(),
            "add a blank line after `<div class=\"page-break\"></div>`",
        ),
        HtmlClass::Other => (
            node.range.clone(),
            "raw HTML can't be typeset".to_string(),
            "use Markdown; only comments and `<div class=\"page-break\"></div>` are allowed",
        ),
    };
    sink.report(Finding::new(DiagnosticCode::UnsupportedHtml, range, message).hint(hint));
    class
}

// ---------------------------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------------------------

/// Walks the blocks after the name, building sections; used with or without a valid name.
fn walk_sections(blocks: &[BlockNode], src: &Source<'_>, sink: &mut Sink<'_>) -> Vec<Section> {
    let next = next_visible(blocks);
    let mut walker = Walker {
        current: None,
        sections: Vec::new(),
    };
    for (block, next) in blocks.iter().zip(next) {
        walker.block(block, next.map(|index| &blocks[index]), src, sink);
    }
    walker.close_section(sink);
    walker.sections
}

/// For each block, the index of the next block that is not a comment, computed in one backward pass.
///
/// Looking ahead per block would rescan a whole run of comments for every one of them: quadratic in
/// the length of the run, which is minutes at the input size cap.
fn next_visible(blocks: &[BlockNode]) -> Vec<Option<usize>> {
    let mut next = vec![None; blocks.len()];
    let mut following = None;
    for (index, block) in blocks.iter().enumerate().rev() {
        next[index] = following;
        if !is_comment(block) {
            following = Some(index);
        }
    }
    next
}

struct Walker {
    current: Option<SectionBuilder>,
    sections: Vec<Section>,
}

impl Walker {
    fn block(&mut self, block: &BlockNode, next: Option<&BlockNode>, src: &Source<'_>, sink: &mut Sink<'_>) {
        match block {
            BlockNode::Heading(heading) => self.heading(heading, src, sink),
            BlockNode::Html(html) => self.html(html, next, src, sink),
            BlockNode::Unsupported(node) => report_unsupported(node, sink),
            BlockNode::Paragraph(paragraph) => match self.current.as_mut() {
                Some(section) => section.paragraph(paragraph, src, sink),
                None => report_before_section(paragraph.range.clone(), "the first section", sink),
            },
            BlockNode::List(list) => match self.current.as_mut() {
                Some(section) => section.list(list, src, sink),
                None => report_before_section(list.range.clone(), "the first section", sink),
            },
        }
    }

    fn heading(&mut self, heading: &HeadingNode, src: &Source<'_>, sink: &mut Sink<'_>) {
        match heading.level {
            1 => {
                let message = "a resume has one `# ` name heading";
                let finding = Finding::new(DiagnosticCode::DuplicateName, heading.range.clone(), message)
                    .hint("make the extra heading a `## Section`, or remove it");
                sink.report(finding);
            }
            2 => {
                self.close_section(sink);
                self.current = Some(SectionBuilder::open(heading, sink));
            }
            3 => match self.current.as_mut() {
                Some(section) => section.start_role(heading, src, sink),
                None => report_before_section(heading.range.clone(), "the first section", sink),
            },
            _ => {
                let message = format!("level-{} headings are not supported", heading.level);
                let hint = "use an era line (`**Name · Period**`) or a `**Label:**` bullet instead";
                unsupported(heading.range.clone(), &message, hint, sink);
            }
        }
    }

    fn html(&mut self, html: &HtmlNode, next: Option<&BlockNode>, src: &Source<'_>, sink: &mut Sink<'_>) {
        if html_block(html, src, sink) != HtmlClass::PageBreak {
            return;
        }
        match self.current.as_mut() {
            Some(section) => section.page_break(html.range.clone(), next, sink),
            None => {
                let finding = Finding::new(
                    DiagnosticCode::ContentBeforeSection,
                    html.range.clone(),
                    "content before the first section",
                )
                .hint("a page break belongs inside a section");
                sink.report(finding);
            }
        }
    }

    fn close_section(&mut self, sink: &mut Sink<'_>) {
        if let Some(section) = self.current.take().and_then(|builder| builder.finish(sink)) {
            self.sections.push(section);
        }
    }
}

fn report_unsupported(node: &UnsupportedNode, sink: &mut Sink<'_>) {
    let hint = match node.construct {
        Construct::BlockQuote => "remove the `> ` quote markers",
        Construct::CodeBlock => "use plain text, or inline `code` within a sentence",
        Construct::IndentedCode => "remove the 4-space indentation: indented text is code, not a paragraph",
        Construct::Table => "use bullets such as `**Label:** value` instead of a table",
        Construct::Footnote => "put the note in the text or remove it",
        Construct::ThematicBreak => {
            "remove the `---` line; at the top of a file it looks like front matter, which isn't supported"
        }
        Construct::Other => "write plain text, bold, italics, code, and links only",
    };
    unsupported(
        node.range.clone(),
        &format!("{}s are not supported", node.construct),
        hint,
        sink,
    );
}

struct SectionBuilder {
    title: Option<SectionTitle>,
    blocks: Vec<Block>,
    role: Option<RoleBuilder>,
    order: Vec<RoleKey>,
}

impl SectionBuilder {
    fn open(heading: &HeadingNode, sink: &mut Sink<'_>) -> Self {
        let text = heading_text(heading, sink);
        let title = match SectionTitle::try_new(text) {
            Ok(title) => Some(title),
            Err(SectionTitleError::NotEmptyViolated) => {
                report_empty_heading(heading, "the section heading is empty", "write `## Experience`", sink);
                None
            }
            Err(SectionTitleError::LenCharMaxViolated) => {
                report_too_long(heading, "the section title", 60, sink);
                None
            }
        };
        Self {
            title,
            blocks: Vec::new(),
            role: None,
            order: Vec::new(),
        }
    }

    fn start_role(&mut self, heading: &HeadingNode, src: &Source<'_>, sink: &mut Sink<'_>) {
        self.close_role();
        let head = role::role_line(heading, src, sink);
        self.role = Some(RoleBuilder {
            head,
            blurb: None,
            body: Vec::new(),
            era: None,
            started: false,
        });
    }

    /// Closes the open role into the section, recording its end for the chronology lint.
    fn close_role(&mut self) {
        let Some(mut role) = self.role.take() else { return };
        role.close_era();
        let Some(head) = role.head else { return };
        self.order.push(RoleKey {
            end: head.period.end,
            range: head.range,
            company: head.company.to_string(),
        });
        self.blocks.push(Block::Role(Role {
            company: head.company,
            title: head.title,
            period: head.period,
            blurb: role.blurb,
            body: role.body,
        }));
    }

    fn page_break(&mut self, range: Range<usize>, next: Option<&BlockNode>, sink: &mut Sink<'_>) {
        let ends_role = match next {
            None => true,
            Some(BlockNode::Heading(heading)) => heading.level <= 3,
            Some(_) => false,
        };
        if self.role.is_some() && !ends_role {
            let message = "a page break inside a role can't be represented";
            let hint = "move the page break above the `###` role line, or remove it";
            unsupported(range, message, hint, sink);
            return;
        }
        self.close_role();
        self.blocks.push(Block::PageBreak);
    }

    fn paragraph(&mut self, paragraph: &ParagraphNode, src: &Source<'_>, sink: &mut Sink<'_>) {
        check_heading_marker(paragraph, src, sink);
        match (role::era_line(paragraph, src, sink), self.role.as_mut()) {
            (EraCheck::Era(head), Some(role)) => role.start_era(head),
            (EraCheck::Era(_), None) => {
                let message = "an era line must be inside a role";
                let finding = Finding::new(DiagnosticCode::EraOutsideRole, paragraph.range.clone(), message)
                    .hint("put it under a `### Company · Title · Period` role line");
                sink.report(finding);
                self.plain_paragraph(paragraph, src, sink);
            }
            _ => self.plain_paragraph(paragraph, src, sink),
        }
    }

    fn plain_paragraph(&mut self, paragraph: &ParagraphNode, src: &Source<'_>, sink: &mut Sink<'_>) {
        let content = inline::convert(&paragraph.content, src, sink);
        if content.is_empty() {
            return;
        }
        match self.role.as_mut() {
            Some(role) => role.paragraph(content),
            None => self.blocks.push(Block::Paragraph { content }),
        }
    }

    fn list(&mut self, list: &ListNode, src: &Source<'_>, sink: &mut Sink<'_>) {
        let mut items = Vec::new();
        bullets(list, src, sink, &mut items);
        if items.is_empty() {
            return;
        }
        match self.role.as_mut() {
            Some(role) => role.bullets(items),
            None => self.blocks.push(Block::Bullets { items }),
        }
    }

    fn finish(mut self, sink: &mut Sink<'_>) -> Option<Section> {
        self.close_role();
        lint::role_order(&self.order, sink);
        Some(Section {
            title: self.title?,
            blocks: self.blocks,
        })
    }
}

/// A role in progress; `head: None` means the role line was unrecoverable, so the body is dropped.
struct RoleBuilder {
    head: Option<RoleHead>,
    blurb: Option<Vec<Inline>>,
    body: Vec<RoleBlock>,
    era: Option<EraBuilder>,
    /// True once any block followed the role line; only the first paragraph can be the blurb.
    started: bool,
}

struct EraBuilder {
    head: EraHead,
    items: Vec<Bullet>,
}

impl RoleBuilder {
    fn start_era(&mut self, head: EraHead) {
        self.close_era();
        self.started = true;
        self.era = Some(EraBuilder {
            head,
            items: Vec::new(),
        });
    }

    fn close_era(&mut self) {
        if let Some(EraBuilder { head, items }) = self.era.take() {
            self.body.push(RoleBlock::Era {
                name: head.name,
                period: head.period,
                items,
            });
        }
    }

    fn paragraph(&mut self, content: Vec<Inline>) {
        if !self.started {
            self.started = true;
            self.blurb = Some(content);
            return;
        }
        self.close_era();
        self.body.push(RoleBlock::Paragraph { content });
    }

    fn bullets(&mut self, items: Vec<Bullet>) {
        self.started = true;
        match self.era.as_mut() {
            Some(era) => era.items.extend(items),
            None => self.body.push(RoleBlock::Bullets { items }),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Bullets
// ---------------------------------------------------------------------------------------------

/// Converts a list into bullets, flattening nested lists depth-first after their parent.
fn bullets(list: &ListNode, src: &Source<'_>, sink: &mut Sink<'_>, out: &mut Vec<Bullet>) {
    if list.ordered {
        let message = "numbered lists are not supported";
        unsupported(
            list.range.clone(),
            message,
            "use `- ` bullets; the numbers would be lost",
            sink,
        );
    }
    for item in &list.items {
        bullet(item, src, sink, out);
    }
}

fn bullet(item: &ItemNode, src: &Source<'_>, sink: &mut Sink<'_>, out: &mut Vec<Bullet>) {
    let mut paragraphs = Vec::new();
    let mut nested = Vec::new();
    for block in &item.blocks {
        match block {
            BlockNode::Paragraph(paragraph) => paragraphs.push(paragraph),
            BlockNode::List(list) => nested.push(list),
            BlockNode::Html(html) => {
                if html_block(html, src, sink) == HtmlClass::PageBreak {
                    unsupported(
                        html.range.clone(),
                        "a page break inside a bullet is not supported",
                        "remove it",
                        sink,
                    );
                }
            }
            BlockNode::Heading(heading) => {
                unsupported(
                    heading.range.clone(),
                    "a heading inside a bullet is not supported",
                    "remove the `#` markers",
                    sink,
                );
            }
            BlockNode::Unsupported(node) => report_unsupported(node, sink),
        }
    }
    if let Some(bullet) = bullet_from(&paragraphs, src, sink) {
        out.push(bullet);
    }
    for list in nested {
        lint::nested_list(list, sink);
        bullets(list, src, sink, out);
    }
}

/// Builds a bullet from the item's paragraphs; extra paragraphs are reported and joined with a space.
fn bullet_from(paragraphs: &[&ParagraphNode], src: &Source<'_>, sink: &mut Sink<'_>) -> Option<Bullet> {
    let (first, extra) = paragraphs.split_first()?;
    let (label, mut content) = inline::label_and_content(&first.content, src, sink);
    for paragraph in extra {
        let message = "a bullet has more than one paragraph";
        unsupported(
            paragraph.range.clone(),
            message,
            "put each paragraph in its own bullet",
            sink,
        );
        inline::append_with_space(&mut content, inline::convert(&paragraph.content, src, sink));
    }
    if label.is_none() && content.is_empty() {
        return None;
    }
    Some(Bullet { label, content })
}

#[cfg(test)]
mod tests {
    use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
    use crate::markdown::{Parsed, parse};
    use claims::{assert_none, assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn parsed(markdown: &str) -> Parsed {
        parse("r.md", markdown)
    }

    fn model(markdown: &str) -> Value {
        let parsed = parsed(markdown);
        assert_ok!(serde_json::to_value(parsed.best_effort()))
    }

    fn codes(markdown: &str) -> Vec<DiagnosticCode> {
        parsed(markdown).diagnostics().iter().map(|d| d.code).collect()
    }

    fn diagnostics(markdown: &str) -> Vec<Diagnostic> {
        parsed(markdown).diagnostics().to_vec()
    }

    fn text(s: &str) -> Value {
        json!({"kind": "text", "text": s})
    }

    const DOC: &str = "# Ada Lovelace\n\nLondon · a@b.co\n\n## Experience\n\n";
    const ROLE_LINE: &str = "### Acme · Engineer · 2015 – 2017\n";

    fn role_doc(body: &str) -> String {
        format!("{DOC}### Acme · Engineer · 2015 – 2017\n\n{body}")
    }

    fn role_value(body: &str) -> Value {
        model(&role_doc(body))["sections"][0]["blocks"][0].clone()
    }

    #[test]
    fn parses_name_and_contact() {
        let value = model("# Ada Lovelace\n\nLondon · [site](https://ada.example)\n\n## Summary\n\nHello.\n");
        assert_eq!(value["schema"], json!(1));
        assert_eq!(value["name"], json!("Ada Lovelace"));
        assert_eq!(value["contact"][0], text("London · "));
        assert_eq!(value["contact"][1]["kind"], json!("link"));
        assert_eq!(value["sections"][0]["title"], json!("Summary"));
        assert_eq!(
            value["sections"][0]["blocks"][0],
            json!({"kind": "paragraph", "content": [text("Hello.")]})
        );
    }

    #[test]
    fn leading_comments_before_name_are_allowed() {
        let markdown = "<!-- ABOUTME: a -->\n<!-- ABOUTME: b -->\n\n# Ada\n\nHere\n\n## S\n\ntext\n";
        assert_eq!(codes(markdown), vec![]);
        assert_eq!(model(markdown)["name"], json!("Ada"));
    }

    #[test]
    fn missing_name_is_error() {
        let markdown = "## Summary\n\ntext\n";
        let parsed = parsed(markdown);
        assert_eq!(codes(markdown), vec![DiagnosticCode::MissingName]);
        assert_none!(parsed.best_effort());
        let d = &parsed.diagnostics()[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(assert_some!(d.span.as_ref()).range, 0..0);
        assert!(!d.hints.is_empty());
    }

    #[test]
    fn missing_name_with_leading_comment_points_past_it() {
        let markdown = "<!-- c -->\n\ntext\n";
        let d = &diagnostics(markdown)[0];
        assert_eq!(assert_some!(d.span.as_ref()).range, 12..12);
    }

    #[test]
    fn duplicate_name_is_error() {
        let markdown = "# Ada\n\nHere\n\n# Grace\n\n## S\n\ntext\n";
        assert_eq!(codes(markdown), vec![DiagnosticCode::DuplicateName]);
        assert_eq!(model(markdown)["name"], json!("Ada"));
        assert_eq!(assert_some!(diagnostics(markdown)[0].span.as_ref()).range, 13..20);
    }

    #[test]
    fn empty_name_heading_is_error() {
        let parsed = parsed("#\n\ntext\n");
        assert_eq!(parsed.diagnostics()[0].code, DiagnosticCode::EmptyHeading);
        assert_none!(parsed.best_effort());
    }

    #[test]
    fn overlong_name_is_text_too_long() {
        let markdown = format!("# {}\n", "A".repeat(81));
        assert_eq!(
            codes(&markdown),
            vec![DiagnosticCode::MissingContact, DiagnosticCode::TextTooLong]
        );
    }

    #[test]
    fn missing_contact_warns() {
        let markdown = "# Ada\n\n## Summary\n\ntext\n";
        let d = &diagnostics(markdown)[0];
        assert_eq!(
            (d.code, d.severity),
            (DiagnosticCode::MissingContact, Severity::Warning)
        );
        assert_eq!(model(markdown)["contact"], json!([]));
        assert_eq!(model(markdown)["sections"][0]["title"], json!("Summary"));
    }

    #[test]
    fn content_before_section_is_error() {
        let markdown = "# Ada\n\nContact\n\nstray paragraph\n\n- stray bullet\n\n## S\n\nok\n";
        assert_eq!(
            codes(markdown),
            vec![
                DiagnosticCode::ContentBeforeSection,
                DiagnosticCode::ContentBeforeSection
            ]
        );
        assert_eq!(
            model(markdown)["sections"][0]["blocks"].as_array().map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn content_before_name_is_error() {
        let markdown = "intro\n\n# Ada\n\nContact\n";
        assert_eq!(codes(markdown), vec![DiagnosticCode::ContentBeforeSection]);
        assert_eq!(model(markdown)["name"], json!("Ada"));
    }

    #[test]
    fn role_outside_a_section_is_content_before_section() {
        let markdown = "# Ada\n\nContact\n\n### Acme · 2015 – 2017\n\n## S\n";
        assert_eq!(codes(markdown), vec![DiagnosticCode::ContentBeforeSection]);
    }

    #[test]
    fn empty_section_title_is_error() {
        let markdown = "# Ada\n\nContact\n\n##\n\ntext\n";
        assert_eq!(codes(markdown), vec![DiagnosticCode::EmptyHeading]);
        assert_eq!(
            model(markdown)["sections"],
            json!([]),
            "a section without a title is dropped"
        );
    }

    #[test]
    fn deep_and_setext_headings_are_unsupported() {
        let markdown = format!("{DOC}#### Deep\n\nSetext\n---\n");
        assert_eq!(
            codes(&markdown),
            vec![DiagnosticCode::UnsupportedMarkdown, DiagnosticCode::UnsupportedMarkdown]
        );
    }

    #[test]
    fn first_paragraph_after_role_is_blurb() {
        let role = role_value("Location data platform.\n\n- one\n");
        assert_eq!(role["blurb"], json!([text("Location data platform.")]));
        assert_eq!(
            role["body"][0],
            json!({"kind": "bullets", "items": [{"label": null, "content": [text("one")]}]})
        );
    }

    #[test]
    fn role_starting_with_list_has_no_blurb() {
        let role = role_value("- one\n\nA later paragraph.\n");
        assert_eq!(role["blurb"], Value::Null);
        assert_eq!(
            role["body"][1],
            json!({"kind": "paragraph", "content": [text("A later paragraph.")]})
        );
    }

    #[test]
    fn role_starting_with_era_has_no_blurb() {
        let role = role_value("**Phase · 2015 – 2016**\n\n- one\n");
        assert_eq!(role["blurb"], Value::Null);
        assert_eq!(role["body"][0]["kind"], json!("era"));
    }

    #[test]
    fn second_paragraph_after_role_is_role_paragraph() {
        let role = role_value("Blurb.\n\nSecond.\n");
        assert_eq!(role["blurb"], json!([text("Blurb.")]));
        assert_eq!(
            role["body"],
            json!([{"kind": "paragraph", "content": [text("Second.")]}])
        );
    }

    #[test]
    fn bold_only_undated_paragraph_is_a_blurb() {
        let role = role_value("**Platform team lead**\n\n- one\n");
        assert_eq!(role["blurb"][0]["kind"], json!("strong"));
    }

    #[test]
    fn bold_only_paragraph_with_dates_is_era() {
        let role = role_value("**AI platforms · 2016 – 2017**\n\n- one\n- two\n");
        assert_eq!(role["body"][0]["kind"], json!("era"));
        assert_eq!(role["body"][0]["name"], json!("AI platforms"));
        assert_eq!(role["body"][0]["period"]["text"], json!("2016 – 2017"));
        assert_eq!(role["body"][0]["items"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn era_collects_following_lists_until_next_era() {
        let role =
            role_value("**A · 2015 – 2016**\n\n- a1\n\n* a2\n\nAfter.\n\n- loose\n\n**B · 2016 – 2017**\n\n- b1\n");
        let body = role["body"].as_array().cloned().unwrap_or_default();
        let kinds: Vec<_> = body.iter().map(|b| b["kind"].as_str().unwrap_or("")).collect();
        assert_eq!(kinds, ["era", "paragraph", "bullets", "era"]);
        assert_eq!(
            body[0]["items"].as_array().map(Vec::len),
            Some(2),
            "adjacent lists merge into the era"
        );
    }

    #[test]
    fn era_outside_role_is_error() {
        let markdown = format!("{DOC}**Phase · 2015 – 2016**\n\n- one\n");
        assert_eq!(codes(&markdown), vec![DiagnosticCode::EraOutsideRole]);
        let value = model(&markdown);
        assert_eq!(
            value["sections"][0]["blocks"][0]["kind"],
            json!("paragraph"),
            "the text is kept"
        );
    }

    #[test]
    fn broken_role_body_does_not_cascade_era_outside_role() {
        let markdown = format!("{DOC}### Not a role\n\n**Phase · 2015 – 2016**\n\n- one\n");
        assert_eq!(codes(&markdown), vec![DiagnosticCode::RoleLineShape]);
    }

    #[test]
    fn bullet_label_is_extracted_without_colon() {
        let role = role_value("- **Languages:** Rust, Java\n- plain\n");
        let items = &role["body"][0]["items"];
        assert_eq!(
            items[0],
            json!({"label": [text("Languages")], "content": [text("Rust, Java")]})
        );
        assert_eq!(items[1]["label"], Value::Null);
    }

    #[test]
    fn tight_and_loose_lists_map_identically() {
        assert_eq!(model(&role_doc("- a\n- b\n")), model(&role_doc("- a\n\n- b\n")));
    }

    #[test]
    fn multi_paragraph_bullet_is_error() {
        let markdown = role_doc("- first\n\n  second\n");
        assert_eq!(codes(&markdown), vec![DiagnosticCode::UnsupportedMarkdown]);
        let items = &model(&markdown)["sections"][0]["blocks"][0]["body"][0]["items"];
        assert_eq!(items[0]["content"], json!([text("first second")]));
    }

    #[test]
    fn nested_items_are_flattened_in_order() {
        let markdown = role_doc("- parent\n  - child one\n    - grandchild\n  - child two\n- sibling\n");
        assert_eq!(
            codes(&markdown),
            vec![DiagnosticCode::NestedList, DiagnosticCode::NestedList]
        );
        let items = model(&markdown)["sections"][0]["blocks"][0]["body"][0]["items"].clone();
        let contents: Vec<_> = items
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|i| i["content"][0]["text"].clone())
            .collect();
        assert_eq!(
            contents,
            [
                json!("parent"),
                json!("child one"),
                json!("grandchild"),
                json!("child two"),
                json!("sibling")
            ]
        );
    }

    #[test]
    fn ordered_list_is_error_but_keeps_items() {
        let markdown = role_doc("1. one\n2. two\n");
        assert_eq!(codes(&markdown), vec![DiagnosticCode::UnsupportedMarkdown]);
        let items = &model(&markdown)["sections"][0]["blocks"][0]["body"][0]["items"];
        assert_eq!(items.as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn page_break_marker_becomes_block() {
        let markdown = format!(
            "{DOC}### Acme · 2015 – 2017\n\n- a\n\n<div class=\"page-break\"></div>\n\n### Beta · 2013 – 2015\n\n- b\n"
        );
        assert_eq!(codes(&markdown), vec![]);
        let kinds: Vec<_> = model(&markdown)["sections"][0]["blocks"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|b| b["kind"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(kinds, ["role", "page-break", "role"]);
    }

    #[test]
    fn page_break_at_section_end_closes_the_role() {
        let markdown =
            format!("{DOC}### Acme · 2015 – 2017\n\n- a\n\n<div class=\"page-break\"></div>\n\n## Next\n\ntext\n");
        assert_eq!(codes(&markdown), vec![]);
    }

    #[test]
    fn page_break_before_any_role_is_a_block() {
        let markdown = format!("{DOC}<div class=\"page-break\"></div>\n\ntext\n");
        assert_eq!(codes(&markdown), vec![]);
        assert_eq!(
            model(&markdown)["sections"][0]["blocks"][0],
            json!({"kind": "page-break"})
        );
    }

    #[test]
    fn page_break_inside_role_body_is_error() {
        let markdown = role_doc("- a\n\n<div class=\"page-break\"></div>\n\n- b\n");
        assert_eq!(codes(&markdown), vec![DiagnosticCode::UnsupportedMarkdown]);
    }

    #[test]
    fn page_break_without_blank_line_after_is_error() {
        let markdown =
            format!("{DOC}text\n\n<div class=\"page-break\"></div>\nswallowed line\n\n### Acme · 2015 – 2017\n");
        assert_eq!(codes(&markdown), vec![DiagnosticCode::UnsupportedHtml]);
    }

    #[test]
    fn comments_are_dropped() {
        let markdown =
            format!("{DOC}<!-- one -->\n\n### Acme · 2015 – 2017\n\n<!-- two -->\n\nBlurb <!-- inline --> text.\n");
        assert_eq!(codes(&markdown), vec![]);
        let role = &model(&markdown)["sections"][0]["blocks"][0];
        assert_eq!(role["blurb"], json!([text("Blurb text.")]));
    }

    #[test]
    fn unterminated_comment_is_error() {
        let markdown = format!("{DOC}text\n\n<!-- never closed\n\n### Acme · 2015 – 2017\n");
        let diagnostics = diagnostics(&markdown);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::UnsupportedHtml);
        assert!(
            diagnostics[0].message.contains("everything after line 9 is hidden"),
            "{}",
            diagnostics[0].message
        );
    }

    #[test]
    fn comment_with_trailing_content_is_error() {
        let markdown = format!("{DOC}<!-- note --> visible?\n");
        assert_eq!(codes(&markdown), vec![DiagnosticCode::UnsupportedHtml]);
    }

    #[test]
    fn raw_html_other_than_page_break_is_error() {
        let markdown = format!("{DOC}<div class=\"x\">hi</div>\n\n<table><tr><td>x</td></tr></table>\n");
        assert_eq!(
            codes(&markdown),
            vec![DiagnosticCode::UnsupportedHtml, DiagnosticCode::UnsupportedHtml]
        );
    }

    #[test]
    fn unsupported_blocks_are_errors() {
        let cases = [
            "> quoted\n",
            "```\ncode\n```\n",
            "    indented code\n",
            "a | b\n--|--\n1 | 2\n",
            "para[^1]\n\n[^1]: note\n",
            "para\n\n---\n",
        ];
        for body in cases {
            let markdown = format!("{DOC}{body}");
            let found = codes(&markdown);
            assert!(
                found.contains(&DiagnosticCode::UnsupportedMarkdown),
                "{body:?} gave {found:?}"
            );
            assert!(parsed(&markdown).has_errors(), "{body:?}");
        }
    }

    #[test]
    fn out_of_order_roles_warn() {
        let markdown = format!("{DOC}### Old · 2010 – 2012\n\n- a\n\n### New · 2018 – 2020\n\n- b\n");
        let found = diagnostics(&markdown);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].code, found[0].severity),
            (DiagnosticCode::RoleOrder, Severity::Warning)
        );
    }

    #[test]
    fn eras_do_not_participate_in_role_order() {
        let role = "**Early · 2010 – 2012**\n\n- a\n\n**Late · 2018 – 2020**\n\n- b\n";
        assert_eq!(codes(&role_doc(role)), vec![]);
    }

    #[test]
    fn invalid_role_line_drops_only_that_role() {
        let markdown = format!("{DOC}### Broken\n\n- lost\n\n### Acme · 2015 – 2017\n\n- kept\n");
        assert_eq!(codes(&markdown), vec![DiagnosticCode::RoleLineShape]);
        let blocks = model(&markdown)["sections"][0]["blocks"].clone();
        assert_eq!(blocks.as_array().map(Vec::len), Some(1));
        assert_eq!(blocks[0]["company"], json!("Acme"));
    }

    #[test]
    fn mid_sentence_bold_in_a_bullet_warns() {
        let markdown = role_doc("- shipped **fast** and well\n");
        assert_eq!(codes(&markdown), vec![DiagnosticCode::MidSentenceBold]);
    }

    #[test]
    fn crlf_and_lf_models_are_equal() {
        let lf = role_doc("Blurb.\n\n**Era · 2015 – 2016**\n\n- **L:** one\n  - nested\n");
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(model(&lf), model(&crlf));
        assert_eq!(codes(&lf), codes(&crlf));
    }

    #[test]
    fn empty_and_dash_comments_are_complete_comments() {
        for comment in ["<!-->", "<!--->", "<!---->", "<!-- a --> <!--> <!--->"] {
            let markdown = format!("{DOC}{comment}\n\ntext after\n");
            assert_eq!(codes(&markdown), vec![], "{comment}");
            let blocks = model(&markdown)["sections"][0]["blocks"].clone();
            assert_eq!(
                blocks,
                json!([{"kind": "paragraph", "content": [text("text after")]}]),
                "{comment}"
            );
        }
    }

    #[test]
    fn heading_marker_without_a_space_warns() {
        let cases = [
            format!("{DOC}###Acme · Eng · 2019 – 2021\n\n- a\n"),
            format!("{DOC}##Skills\n\n- a\n"),
            "# Ada\n\n#hashtag\n\n## S\n\ntext\n".to_string(),
            format!("{DOC}{ROLE_LINE}\n\n#1 priority was delivery\n"),
        ];
        for markdown in cases {
            let found = diagnostics(&markdown);
            let warnings: Vec<_> = found
                .iter()
                .filter(|d| d.code == DiagnosticCode::HeadingMarkerSpacing)
                .collect();
            assert_eq!(warnings.len(), 1, "{markdown:?}: {found:?}");
            let warning = warnings[0];
            assert_eq!(warning.severity, Severity::Warning);
            assert_eq!(
                warning.fix, None,
                "turning text into a heading is not meaning-preserving"
            );
            let range = assert_some!(warning.span.as_ref()).range.clone();
            assert!(markdown[range].chars().all(|c| c == '#'), "{markdown:?}");
        }
    }

    #[test]
    fn text_that_merely_contains_hashes_is_not_flagged() {
        for body in ["C# and F# developer", "a #hashtag in the middle", "Level #1 support"] {
            let markdown = format!("{DOC}{body}\n");
            assert_eq!(codes(&markdown), vec![], "{body}");
        }
    }

    #[test]
    fn names_and_role_parts_collapse_whitespace_left_by_dropped_comments() {
        let markdown = "# Ada <!-- c --> Lovelace\n\nHere\n\n## S\n\n### Acme <!-- c --> Co · 2015 – 2017\n";
        let value = model(markdown);
        assert_eq!(value["name"], json!("Ada Lovelace"));
        assert_eq!(value["sections"][0]["blocks"][0]["company"], json!("Acme Co"));
    }

    #[test]
    fn contact_line_has_no_stray_edge_spaces() {
        let markdown = "# Ada\n\n![me](me.png) Seattle <!-- c -->\n\n## S\n\ntext\n";
        assert_eq!(model(markdown)["contact"], json!([text("Seattle")]));
        assert_eq!(codes(markdown), vec![DiagnosticCode::Image]);
    }

    #[test]
    fn bold_prose_with_a_middle_dot_is_only_a_bold_warning() {
        let markdown = role_doc("**Python · Rust** are my favourite languages.\n");
        assert_eq!(codes(&markdown), vec![DiagnosticCode::MidSentenceBold]);
    }

    #[test]
    fn a_bullet_that_is_only_a_comment_is_dropped_like_the_comment() {
        let markdown = role_doc("- <!-- note -->\n- real\n");
        assert_eq!(codes(&markdown), vec![]);
        let items = &model(&markdown)["sections"][0]["blocks"][0]["body"][0]["items"];
        assert_eq!(items, &json!([{"label": null, "content": [text("real")]}]));
    }

    #[test]
    fn numbered_nested_list_is_reported_like_a_top_level_one() {
        let markdown = role_doc("- parent\n  1. first\n  2. second\n");
        assert_eq!(
            codes(&markdown),
            vec![DiagnosticCode::NestedList, DiagnosticCode::UnsupportedMarkdown]
        );
        let items = &model(&markdown)["sections"][0]["blocks"][0]["body"][0]["items"];
        assert_eq!(items.as_array().map(Vec::len), Some(3), "the items are kept");
    }

    #[test]
    fn html_inside_a_bullet_is_reported_unless_it_is_a_comment() {
        let page_break = role_doc("- item\n\n  <div class=\"page-break\"></div>\n");
        assert_eq!(codes(&page_break), vec![DiagnosticCode::UnsupportedMarkdown]);
        let raw = role_doc("- item\n\n  <div>x</div>\n");
        assert_eq!(codes(&raw), vec![DiagnosticCode::UnsupportedHtml]);
        let comment = role_doc("- item\n\n  <!-- note -->\n");
        assert_eq!(codes(&comment), vec![]);
    }

    #[test]
    fn the_unclosed_comment_span_is_the_opening_marker() {
        let markdown = format!("{DOC}text\n\n<!-- never closed\n");
        let found = diagnostics(&markdown);
        let range = assert_some!(found[0].span.as_ref()).range.clone();
        assert_eq!(&markdown[range], "<!--");
    }
}

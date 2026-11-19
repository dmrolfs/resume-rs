//! ABOUTME: Phase 1: lowers pulldown-cmark offset events into a small located syntax tree, iteratively.
//! ABOUTME: Enforces nesting caps and maps every text segment back to bytes in the original source.

use std::ops::Range;

use pulldown_cmark::{BrokenLink, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
use strum::Display;

use super::{Finding, MAX_INLINE_DEPTH, MAX_LIST_DEPTH, Sink};
use crate::diagnostics::DiagnosticCode;

/// The located syntax tree of a whole document.
#[derive(Debug)]
pub(crate) struct Document {
    pub blocks: Vec<BlockNode>,
}

/// A block-level node.
#[derive(Debug)]
pub(crate) enum BlockNode {
    Heading(HeadingNode),
    Paragraph(ParagraphNode),
    List(ListNode),
    Html(HtmlNode),
    Unsupported(UnsupportedNode),
}

impl BlockNode {
    /// The source range of the whole block, without its trailing line ending.
    pub fn range(&self) -> Range<usize> {
        match self {
            Self::Heading(n) => n.range.clone(),
            Self::Paragraph(n) => n.range.clone(),
            Self::List(n) => n.range.clone(),
            Self::Html(n) => n.range.clone(),
            Self::Unsupported(n) => n.range.clone(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct HeadingNode {
    pub level: u8,
    /// True for `Title` over `===` or `---`, which the model cannot represent.
    pub setext: bool,
    pub range: Range<usize>,
    pub content: Vec<InlineNode>,
}

#[derive(Debug)]
pub(crate) struct ParagraphNode {
    pub range: Range<usize>,
    pub content: Vec<InlineNode>,
}

#[derive(Debug)]
pub(crate) struct ListNode {
    pub range: Range<usize>,
    pub ordered: bool,
    pub items: Vec<ItemNode>,
}

#[derive(Debug)]
pub(crate) struct ItemNode {
    /// Tight items hold their inline run as one implicit paragraph, so tight and loose lists look alike.
    pub blocks: Vec<BlockNode>,
}

#[derive(Debug)]
pub(crate) struct HtmlNode {
    pub range: Range<usize>,
    pub text: String,
}

#[derive(Debug)]
pub(crate) struct UnsupportedNode {
    pub range: Range<usize>,
    pub construct: Construct,
}

/// Markdown constructs the model cannot represent; `Display` gives the user-facing name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub(crate) enum Construct {
    #[strum(serialize = "block quote")]
    BlockQuote,
    #[strum(serialize = "fenced code block")]
    CodeBlock,
    #[strum(serialize = "indented code block")]
    IndentedCode,
    #[strum(serialize = "table")]
    Table,
    #[strum(serialize = "footnote")]
    Footnote,
    #[strum(serialize = "thematic break")]
    ThematicBreak,
    #[strum(serialize = "unsupported construct")]
    Other,
}

/// An inline node.
#[derive(Debug)]
pub(crate) enum InlineNode {
    Text(TextNode),
    Strong {
        range: Range<usize>,
        children: Vec<InlineNode>,
    },
    Emph {
        range: Range<usize>,
        children: Vec<InlineNode>,
    },
    Link {
        range: Range<usize>,
        dest: String,
        email: bool,
        children: Vec<InlineNode>,
    },
    Code {
        range: Range<usize>,
        text: String,
    },
    Html {
        range: Range<usize>,
        text: String,
    },
    Image {
        range: Range<usize>,
    },
    Break {
        range: Range<usize>,
        hard: bool,
    },
    Unsupported {
        range: Range<usize>,
        construct: Construct,
    },
}

impl InlineNode {
    /// The source range of the node.
    pub fn range(&self) -> Range<usize> {
        match self {
            Self::Text(t) => t.range(),
            Self::Strong { range, .. }
            | Self::Emph { range, .. }
            | Self::Link { range, .. }
            | Self::Code { range, .. }
            | Self::Html { range, .. }
            | Self::Image { range }
            | Self::Break { range, .. }
            | Self::Unsupported { range, .. } => range.clone(),
        }
    }
}

/// Merged text with a map from text offsets to source ranges.
///
/// The text is what pulldown-cmark produced (smart punctuation applied, entities decoded, line
/// endings never included), so meaning is read from `text` while every span or edit is mapped back
/// to the original bytes through the segments.
#[derive(Debug, Default, Clone)]
pub(crate) struct TextNode {
    pub text: String,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
struct Segment {
    text_start: usize,
    source: Range<usize>,
    /// True when the segment's text is byte-for-byte the source slice.
    verbatim: bool,
}

impl TextNode {
    /// Appends text that came from `source`; `verbatim` says the two are identical.
    pub fn push(&mut self, text: &str, source: Range<usize>, verbatim: bool) {
        if let Some(last) = self.segments.last_mut()
            && last.verbatim
            && verbatim
            && last.source.end == source.start
        {
            last.source.end = source.end;
        } else {
            self.segments.push(Segment {
                text_start: self.text.len(),
                source,
                verbatim,
            });
        }
        self.text.push_str(text);
    }

    /// Joins several text nodes into one, keeping their source mapping.
    pub fn concat<'a>(nodes: impl IntoIterator<Item = &'a TextNode>) -> Self {
        let mut joined = Self::default();
        for node in nodes {
            let offset = joined.text.len();
            joined.text.push_str(&node.text);
            joined.segments.extend(node.segments.iter().map(|s| Segment {
                text_start: s.text_start + offset,
                ..s.clone()
            }));
        }
        joined
    }

    /// Source range of the whole node.
    pub fn range(&self) -> Range<usize> {
        match (self.segments.first(), self.segments.last()) {
            (Some(first), Some(last)) => first.source.start..last.source.end,
            _ => 0..0,
        }
    }

    /// Source range covering `text_range`: exact inside verbatim segments, the whole segment otherwise.
    pub fn source_range(&self, text_range: Range<usize>) -> Range<usize> {
        let start = self.map_start(text_range.start);
        let end = if text_range.end > text_range.start {
            self.map_end(text_range.end)
        } else {
            start
        };
        start..end.max(start)
    }

    fn segment_at(&self, text_pos: usize) -> Option<&Segment> {
        let index = self
            .segments
            .partition_point(|s| s.text_start <= text_pos)
            .saturating_sub(1);
        self.segments.get(index)
    }

    fn map_start(&self, pos: usize) -> usize {
        match self.segment_at(pos) {
            Some(s) if s.verbatim => (s.source.start + pos - s.text_start).min(s.source.end),
            Some(s) => s.source.start,
            None => 0,
        }
    }

    fn map_end(&self, end: usize) -> usize {
        match self.segment_at(end.saturating_sub(1)) {
            Some(s) if s.verbatim => (s.source.start + end - s.text_start).min(s.source.end),
            Some(s) => s.source.end,
            None => 0,
        }
    }
}

/// pulldown-cmark options: smart punctuation, plus tables and footnotes so they can be diagnosed.
///
/// Math, strikethrough, YAML metadata, heading attributes, task lists, and definition lists stay off:
/// a resume writes `$39K` and `~5`, and `---` is not front matter.
pub(super) fn options() -> Options {
    Options::ENABLE_SMART_PUNCTUATION | Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES
}

/// Lowers `text` (already past any BOM) into a tree; every range is shifted by `base`.
pub(crate) fn lower(text: &str, base: usize, sink: &mut Sink<'_>) -> Document {
    let mut broken: Vec<(Range<usize>, LinkType)> = Vec::new();
    let mut lowerer = Lowerer::new(text, base, sink);
    {
        let callback = |link: BrokenLink<'_>| {
            broken.push((link.span, link.link_type));
            None
        };
        let parser = Parser::new_with_broken_link_callback(text, options(), Some(callback));
        for (event, range) in parser.into_offset_iter() {
            lowerer.event(event, range);
        }
    }
    let document = lowerer.finish();
    report_broken_links(&broken, base, sink);
    document
}

/// Undefined `[text][ref]` and `[text][]` references stay literal text, so report them here;
/// an undefined shortcut `[word]` is ordinary bracketed text.
fn report_broken_links(broken: &[(Range<usize>, LinkType)], base: usize, sink: &mut Sink<'_>) {
    for (span, link_type) in broken {
        if matches!(
            link_type,
            LinkType::Reference | LinkType::ReferenceUnknown | LinkType::Collapsed | LinkType::CollapsedUnknown
        ) {
            let range = span.start + base..span.end + base;
            let finding = Finding::new(DiagnosticCode::InvalidLink, range, "link reference is not defined")
                .hint("write the link inline: [text](https://example.com)");
            sink.report(finding);
        }
    }
}

enum InlineKind {
    Strong,
    Emph,
    Link { dest: String, email: bool },
}

enum Frame {
    Root(Vec<BlockNode>),
    List {
        range: Range<usize>,
        ordered: bool,
        items: Vec<ItemNode>,
    },
    Item {
        blocks: Vec<BlockNode>,
        tight: Vec<InlineNode>,
    },
    Paragraph {
        range: Range<usize>,
        content: Vec<InlineNode>,
    },
    Heading {
        range: Range<usize>,
        level: u8,
        setext: bool,
        content: Vec<InlineNode>,
    },
    Html {
        range: Range<usize>,
        text: String,
    },
    Inline {
        kind: InlineKind,
        range: Range<usize>,
        children: Vec<InlineNode>,
    },
}

struct Lowerer<'t, 's, 'p> {
    text: &'t str,
    base: usize,
    stack: Vec<Frame>,
    /// Depth inside an unsupported container whose events are ignored until it closes.
    skip_depth: usize,
    /// Inline starts beyond the depth cap, each awaiting its end event.
    overflow: usize,
    inline_depth: usize,
    list_depth: usize,
    inline_cap_reported: bool,
    sink: &'s mut Sink<'p>,
}

impl<'t, 's, 'p> Lowerer<'t, 's, 'p> {
    fn new(text: &'t str, base: usize, sink: &'s mut Sink<'p>) -> Self {
        Self {
            text,
            base,
            stack: vec![Frame::Root(Vec::new())],
            skip_depth: 0,
            overflow: 0,
            inline_depth: 0,
            list_depth: 0,
            inline_cap_reported: false,
            sink,
        }
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        if self.skip_depth > 0 {
            match event {
                Event::Start(_) => self.skip_depth += 1,
                Event::End(_) => self.skip_depth -= 1,
                _ => {}
            }
            return;
        }
        let range = self.block_range(range);
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(end) => self.end(end),
            Event::Text(text) => self.push_text(&text, range),
            Event::Code(text) => self.push_inline(InlineNode::Code {
                range,
                text: text.into_string(),
            }),
            Event::Html(html) => self.push_html(&html),
            Event::InlineHtml(html) => self.push_inline(InlineNode::Html {
                range,
                text: html.into_string(),
            }),
            Event::SoftBreak => self.push_inline(InlineNode::Break { range, hard: false }),
            Event::HardBreak => self.push_inline(InlineNode::Break { range, hard: true }),
            Event::Rule => self.push_block(BlockNode::Unsupported(UnsupportedNode {
                range,
                construct: Construct::ThematicBreak,
            })),
            Event::FootnoteReference(_) => {
                self.push_inline(InlineNode::Unsupported {
                    range,
                    construct: Construct::Footnote,
                });
            }
            Event::TaskListMarker(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {
                self.push_inline(InlineNode::Unsupported {
                    range,
                    construct: Construct::Other,
                });
            }
        }
    }

    /// Shifts a range into original coordinates and drops a trailing line ending.
    fn block_range(&self, range: Range<usize>) -> Range<usize> {
        let visible = self
            .text
            .get(range.clone())
            .map_or(range.len(), |text| text.trim_end_matches(['\n', '\r']).len());
        range.start + self.base..range.start + visible + self.base
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        match tag {
            Tag::Paragraph => {
                self.inline_cap_reported = false;
                self.stack.push(Frame::Paragraph {
                    range,
                    content: Vec::new(),
                });
            }
            Tag::Heading { level, .. } => {
                self.inline_cap_reported = false;
                let setext = !self.is_atx(range.start);
                self.stack.push(Frame::Heading {
                    range,
                    level: level as u8,
                    setext,
                    content: Vec::new(),
                });
            }
            Tag::HtmlBlock => self.stack.push(Frame::Html {
                range,
                text: String::new(),
            }),
            Tag::List(first) => self.start_list(first.is_some(), range),
            Tag::Item => self.stack.push(Frame::Item {
                blocks: Vec::new(),
                tight: Vec::new(),
            }),
            Tag::Emphasis => self.start_inline(InlineKind::Emph, range),
            Tag::Strong => self.start_inline(InlineKind::Strong, range),
            Tag::Link {
                link_type, dest_url, ..
            } => {
                let email = link_type == LinkType::Email;
                self.start_inline(
                    InlineKind::Link {
                        dest: dest_url.into_string(),
                        email,
                    },
                    range,
                );
            }
            Tag::Image { .. } => {
                self.push_inline(InlineNode::Image { range });
                self.skip_depth = 1;
            }
            Tag::BlockQuote(_) => self.start_unsupported(Construct::BlockQuote, range),
            Tag::CodeBlock(CodeBlockKind::Indented) => self.start_unsupported(Construct::IndentedCode, range),
            Tag::CodeBlock(CodeBlockKind::Fenced(_)) => self.start_unsupported(Construct::CodeBlock, range),
            Tag::Table(_) => self.start_unsupported(Construct::Table, range),
            Tag::FootnoteDefinition(_) => self.start_unsupported(Construct::Footnote, range),
            // Unreachable with our options; handled defensively so nothing is dropped silently.
            _ => self.start_unsupported(Construct::Other, range),
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::HtmlBlock => self.end_leaf_block(),
            TagEnd::List(_) => self.end_list(),
            TagEnd::Item => self.end_item(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link => self.end_inline(),
            _ => {}
        }
    }

    fn start_unsupported(&mut self, construct: Construct, range: Range<usize>) {
        self.push_block(BlockNode::Unsupported(UnsupportedNode { range, construct }));
        self.skip_depth = 1;
    }

    fn start_list(&mut self, ordered: bool, range: Range<usize>) {
        if self.list_depth >= MAX_LIST_DEPTH {
            let message = format!("lists are nested deeper than {MAX_LIST_DEPTH} levels");
            let finding = Finding::new(DiagnosticCode::InputLimit, range, message).hint("flatten the list");
            self.sink.report(finding);
            self.skip_depth = 1;
            return;
        }
        self.flush_tight();
        self.list_depth += 1;
        self.stack.push(Frame::List {
            range,
            ordered,
            items: Vec::new(),
        });
    }

    fn end_list(&mut self) {
        if let Some(Frame::List { range, ordered, items }) = self.stack.pop() {
            self.list_depth -= 1;
            self.push_block(BlockNode::List(ListNode { range, ordered, items }));
        }
    }

    fn end_item(&mut self) {
        self.flush_tight();
        if let Some(Frame::Item { blocks, .. }) = self.stack.pop()
            && let Some(Frame::List { items, .. }) = self.stack.last_mut()
        {
            items.push(ItemNode { blocks });
        }
    }

    fn end_leaf_block(&mut self) {
        let node = match self.stack.pop() {
            Some(Frame::Paragraph { range, content }) => BlockNode::Paragraph(ParagraphNode { range, content }),
            Some(Frame::Heading {
                range,
                level,
                setext,
                content,
            }) => BlockNode::Heading(HeadingNode {
                level,
                setext,
                range,
                content,
            }),
            Some(Frame::Html { range, text }) => BlockNode::Html(HtmlNode { range, text }),
            _ => return,
        };
        self.push_block(node);
    }

    fn start_inline(&mut self, kind: InlineKind, range: Range<usize>) {
        if self.inline_depth >= MAX_INLINE_DEPTH {
            self.overflow += 1;
            self.report_inline_cap(range);
            return;
        }
        self.inline_depth += 1;
        self.stack.push(Frame::Inline {
            kind,
            range,
            children: Vec::new(),
        });
    }

    fn end_inline(&mut self) {
        if self.overflow > 0 {
            self.overflow -= 1;
            return;
        }
        if let Some(Frame::Inline { kind, range, children }) = self.stack.pop() {
            self.inline_depth -= 1;
            let node = match kind {
                InlineKind::Strong => InlineNode::Strong { range, children },
                InlineKind::Emph => InlineNode::Emph { range, children },
                InlineKind::Link { dest, email } => InlineNode::Link {
                    range,
                    dest,
                    email,
                    children,
                },
            };
            self.push_inline(node);
        }
    }

    /// One `input-limit` finding per paragraph, however many starts overflow.
    fn report_inline_cap(&mut self, range: Range<usize>) {
        if std::mem::replace(&mut self.inline_cap_reported, true) {
            return;
        }
        let message = format!("emphasis, strong, and links are nested deeper than {MAX_INLINE_DEPTH} levels");
        let finding = Finding::new(DiagnosticCode::InputLimit, range, message)
            .hint("deeper markup is flattened into its parent; remove the excess nesting");
        self.sink.report(finding);
    }

    /// True when the block starting at `start` is an ATX heading (`#` to `######`), not a setext one.
    fn is_atx(&self, start: usize) -> bool {
        let line = self.text.get(start - self.base..).unwrap_or("");
        let rest = line.trim_start_matches(' ');
        let hashes = rest.bytes().take_while(|b| *b == b'#').count();
        (1..=6).contains(&hashes) && matches!(rest.as_bytes().get(hashes), None | Some(b' ' | b'\t' | b'\n' | b'\r'))
    }

    fn push_text(&mut self, text: &str, range: Range<usize>) {
        let verbatim = self.text.get(range.start - self.base..range.end - self.base) == Some(text);
        let Some(target) = self.inline_target() else { return };
        if let Some(InlineNode::Text(node)) = target.last_mut() {
            node.push(text, range, verbatim);
        } else {
            let mut node = TextNode::default();
            node.push(text, range, verbatim);
            target.push(InlineNode::Text(node));
        }
    }

    fn push_inline(&mut self, node: InlineNode) {
        if let Some(target) = self.inline_target() {
            target.push(node);
        }
    }

    fn push_html(&mut self, html: &str) {
        if let Some(Frame::Html { text, .. }) = self.stack.last_mut() {
            text.push_str(html);
        }
    }

    /// The inline run being built: a paragraph, heading, inline frame, or a tight item.
    fn inline_target(&mut self) -> Option<&mut Vec<InlineNode>> {
        match self.stack.last_mut()? {
            Frame::Paragraph { content, .. } | Frame::Heading { content, .. } => Some(content),
            Frame::Inline { children, .. } => Some(children),
            Frame::Item { tight, .. } => Some(tight),
            _ => None,
        }
    }

    fn push_block(&mut self, node: BlockNode) {
        self.flush_tight();
        if let Some(Frame::Root(blocks) | Frame::Item { blocks, .. }) = self.stack.last_mut() {
            blocks.push(node);
        }
    }

    /// Wraps a tight item's inline run into an implicit paragraph.
    fn flush_tight(&mut self) {
        let Some(Frame::Item { blocks, tight, .. }) = self.stack.last_mut() else {
            return;
        };
        if tight.is_empty() {
            return;
        }
        let content = std::mem::take(tight);
        let start = content.first().map_or(0, |n| n.range().start);
        let end = content.last().map_or(0, |n| n.range().end);
        blocks.push(BlockNode::Paragraph(ParagraphNode {
            range: start..end,
            content,
        }));
    }

    fn finish(mut self) -> Document {
        // Pulldown-cmark's events are balanced, so only the root remains; anything else is dropped.
        self.stack.truncate(1);
        match self.stack.pop() {
            Some(Frame::Root(blocks)) => Document { blocks },
            _ => Document { blocks: Vec::new() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostic;
    use claims::{assert_none, assert_some};
    use pretty_assertions::assert_eq;
    use std::path::Path;

    fn lowered(text: &str) -> (Document, Vec<Diagnostic>) {
        let mut sink = Sink::new(Path::new("r.md"));
        let doc = lower(text, 0, &mut sink);
        (doc, sink.finish())
    }

    fn only_paragraph(text: &str) -> Vec<InlineNode> {
        let (doc, _) = lowered(text);
        match doc.blocks.into_iter().next() {
            Some(BlockNode::Paragraph(p)) => p.content,
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    fn text_of(node: &InlineNode) -> &TextNode {
        match node {
            InlineNode::Text(t) => t,
            other => panic!("expected text, got {other:?}"),
        }
    }

    fn depth(nodes: &[InlineNode]) -> usize {
        nodes
            .iter()
            .map(|n| match n {
                InlineNode::Strong { children, .. }
                | InlineNode::Emph { children, .. }
                | InlineNode::Link { children, .. } => 1 + depth(children),
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }

    fn list_depth(blocks: &[BlockNode]) -> usize {
        blocks
            .iter()
            .map(|b| match b {
                BlockNode::List(l) => 1 + l.items.iter().map(|i| list_depth(&i.blocks)).max().unwrap_or(0),
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn text_events_merge_with_segments() {
        let nodes = only_paragraph("Ada &amp; Bob");
        assert_eq!(nodes.len(), 1);
        let text = text_of(&nodes[0]);
        assert_eq!(text.text, "Ada & Bob");
        assert_eq!(text.range(), 0..13);
    }

    #[test]
    fn smart_punctuation_segments_map_to_source() {
        let nodes = only_paragraph("A -- B and don't");
        let text = text_of(&nodes[0]);
        assert_eq!(text.text, "A – B and don’t");
        let dash = text.text.find('–').unwrap_or(0);
        // The en dash is 3 bytes of text but came from the 2 source bytes `--`.
        assert_eq!(text.source_range(dash..dash + 3), 2..4);
        let quote = text.text.find('’').unwrap_or(0);
        assert_eq!(text.source_range(quote..quote + 3), 14..15);
        // Verbatim stretches map exactly.
        assert_eq!(text.source_range(0..1), 0..1);
        assert_eq!(text.source_range(dash + 3..dash + 5), 4..6);
    }

    #[test]
    fn crlf_text_excludes_carriage_returns() {
        let nodes = only_paragraph("line one\r\nline two\r\n");
        assert_eq!(nodes.len(), 3);
        assert_eq!(text_of(&nodes[0]).text, "line one");
        assert!(matches!(nodes[1], InlineNode::Break { hard: false, .. }));
        assert_eq!(text_of(&nodes[2]).text, "line two");
        let (doc, _) = lowered("# Name\r\n\r\ntext\r\n");
        assert!(doc.blocks.iter().all(|b| {
            let r = b.range();
            !"# Name\r\n\r\ntext\r\n"[..r.end].ends_with(['\r', '\n'])
        }));
    }

    #[test]
    fn tight_items_get_implicit_paragraph() {
        let (doc, _) = lowered("- a\n- b\n");
        let BlockNode::List(list) = &doc.blocks[0] else {
            panic!("expected a list")
        };
        assert!(!list.ordered);
        assert_eq!(list.items.len(), 2);
        for item in &list.items {
            assert!(matches!(item.blocks.as_slice(), [BlockNode::Paragraph(_)]));
        }
    }

    #[test]
    fn loose_items_lower_like_tight_items() {
        let (tight, _) = lowered("- a\n- b\n");
        let (loose, _) = lowered("- a\n\n- b\n");
        let shape = |doc: &Document| match &doc.blocks[0] {
            BlockNode::List(l) => l.items.iter().map(|i| i.blocks.len()).collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        assert_eq!(shape(&tight), shape(&loose));
    }

    #[test]
    fn setext_heading_is_flagged() {
        let (doc, _) = lowered("Title\n=====\n\n# Atx\n");
        let flags: Vec<_> = doc
            .blocks
            .iter()
            .filter_map(|b| {
                if let BlockNode::Heading(h) = b {
                    Some((h.level, h.setext))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(flags, vec![(1, true), (1, false)]);
    }

    #[test]
    fn deep_emphasis_is_capped_and_reported_once() {
        let delimiter = |i: usize| if i.is_multiple_of(2) { "*" } else { "_" };
        let opens: String = (0..20).map(|i| format!("{}w ", delimiter(i))).collect();
        let closes: String = (0..20).rev().map(|i| format!(" w{}", delimiter(i))).collect();
        let (doc, diagnostics) = lowered(&format!("{opens}x{closes}"));
        let BlockNode::Paragraph(p) = &doc.blocks[0] else {
            panic!("expected a paragraph")
        };
        assert_eq!(depth(&p.content), MAX_INLINE_DEPTH);
        let limits = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::InputLimit)
            .count();
        assert_eq!(limits, 1);
        assert!(format!("{:?}", p.content).contains("w x w"));
    }

    #[test]
    fn deep_lists_are_capped() {
        let text: String = (0..12).map(|i| format!("{}- item {i}\n", "  ".repeat(i))).collect();
        let (doc, diagnostics) = lowered(&text);
        assert_eq!(list_depth(&doc.blocks), MAX_LIST_DEPTH);
        assert!(diagnostics.iter().any(|d| d.code == DiagnosticCode::InputLimit));
    }

    #[test]
    fn blockquote_contents_are_skipped() {
        let (doc, _) = lowered("> quote *x*\n> - item\n\npara\n");
        assert_eq!(doc.blocks.len(), 2);
        assert!(matches!(
            &doc.blocks[0],
            BlockNode::Unsupported(UnsupportedNode {
                construct: Construct::BlockQuote,
                ..
            })
        ));
        assert!(matches!(&doc.blocks[1], BlockNode::Paragraph(_)));
    }

    #[test]
    fn unsupported_blocks_are_recognized() {
        let cases = [
            ("```\ncode\n```\n", Construct::CodeBlock),
            ("    indented\n", Construct::IndentedCode),
            ("a | b\n--|--\n1 | 2\n", Construct::Table),
            ("text[^1]\n\n[^1]: note\n", Construct::Footnote),
            ("para\n\n---\n", Construct::ThematicBreak),
        ];
        for (text, expected) in cases {
            let (doc, _) = lowered(text);
            let found = doc
                .blocks
                .iter()
                .any(|b| matches!(b, BlockNode::Unsupported(u) if u.construct == expected));
            assert!(found, "{expected} not found in {doc:?}");
        }
    }

    #[test]
    fn html_block_text_is_kept() {
        let (doc, _) = lowered("<!-- note -->\n\npara\n");
        let BlockNode::Html(html) = &doc.blocks[0] else {
            panic!("expected html")
        };
        assert_eq!(html.text.trim(), "<!-- note -->");
    }

    #[test]
    fn links_images_and_inline_html_are_nodes() {
        let nodes = only_paragraph("[a](https://x.y) <a@b.co> ![img](p.png) <b>");
        let kinds: Vec<_> = nodes
            .iter()
            .map(|n| match n {
                InlineNode::Link { dest, email, .. } => format!("link:{dest}:{email}"),
                InlineNode::Image { .. } => "image".into(),
                InlineNode::Html { text, .. } => format!("html:{text}"),
                InlineNode::Text(_) => "text".into(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "link:https://x.y:false",
                "text",
                "link:a@b.co:true",
                "text",
                "image",
                "text",
                "html:<b>"
            ]
        );
    }

    #[test]
    fn broken_reference_links_are_collected() {
        let (_, diagnostics) = lowered("[text][nope] and [short] and [ok](https://x.y)\n");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::InvalidLink);
        let span = assert_some!(diagnostics[0].span.as_ref());
        assert_eq!(span.range, 0..12);
    }

    #[test]
    fn base_shifts_every_range() {
        let mut sink = Sink::new(Path::new("r.md"));
        let doc = lower("# A\n", 3, &mut sink);
        assert_eq!(doc.blocks[0].range(), 3..6);
    }

    #[test]
    fn text_node_maps_empty_and_missing_ranges() {
        let empty = TextNode::default();
        assert_eq!(empty.range(), 0..0);
        assert_eq!(empty.source_range(0..0), 0..0);
        assert_none!(empty.segment_at(0));
    }

    #[test]
    fn verbatim_segments_merge_only_when_contiguous_in_the_source() {
        let mut node = TextNode::default();
        node.push("ab", 0..2, true);
        node.push("cd", 2..4, true);
        node.push("ef", 10..12, true); // a gap in the source: markup was between them
        assert_eq!(node.text, "abcdef");
        assert_eq!(node.source_range(0..4), 0..4);
        assert_eq!(node.source_range(4..6), 10..12, "text after a gap maps past the gap");
        assert_eq!(node.range(), 0..12);
    }

    #[test]
    fn concat_keeps_each_part_mapped_to_its_own_source() {
        let (mut first, mut second) = (TextNode::default(), TextNode::default());
        first.push("one ", 0..4, true);
        second.push("two", 20..23, true);
        let joined = TextNode::concat([&first, &second]);
        assert_eq!(joined.text, "one two");
        assert_eq!(joined.source_range(0..3), 0..3);
        assert_eq!(
            joined.source_range(4..7),
            20..23,
            "the second part keeps its own offsets"
        );
    }

    #[test]
    fn an_entity_maps_to_the_whole_entity() {
        let nodes = only_paragraph("A &amp; B");
        let text = text_of(&nodes[0]);
        assert_eq!(text.text, "A & B");
        // The decoded `&` is one byte of text but five bytes of source (`&amp;`).
        assert_eq!(text.source_range(2..3), 2..7);
        assert_eq!(text.source_range(0..2), 0..2, "text before the entity maps exactly");
    }

    #[test]
    fn broken_link_ranges_are_shifted_by_the_base() {
        let mut sink = Sink::new(Path::new("r.md"));
        let _ = lower("[t][nope]\n", 3, &mut sink);
        let diagnostics = sink.finish();
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 3..12);
    }

    #[test]
    fn many_sibling_lists_do_not_count_toward_the_nesting_cap() {
        // Closing a list must give its depth back: 20 lists in a row are 20 siblings, not 20 levels.
        let text: String = (0..20).map(|i| format!("paragraph {i}\n\n- item {i}\n\n")).collect();
        let (doc, diagnostics) = lowered(&text);
        assert_eq!(diagnostics, vec![]);
        assert_eq!(
            doc.blocks.iter().filter(|b| matches!(b, BlockNode::List(_))).count(),
            20
        );
    }

    #[test]
    fn many_sibling_emphasis_spans_do_not_count_toward_the_nesting_cap() {
        let text = (0..20)
            .map(|i| format!("*a{i}* **b{i}** [c{i}](https://x.y)"))
            .collect::<Vec<_>>()
            .join(" ");
        let (_, diagnostics) = lowered(&text);
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn a_hash_without_a_space_is_not_an_atx_heading() {
        let (doc, _) = lowered("#hashtag\n========\n\n#7 priority\n-----\n\n# real\n");
        let flags: Vec<_> = doc
            .blocks
            .iter()
            .filter_map(|b| {
                if let BlockNode::Heading(h) = b {
                    Some((h.level, h.setext))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(flags, vec![(1, true), (2, true), (1, false)]);
    }

    #[test]
    fn a_range_inside_a_synthesized_segment_maps_to_the_whole_segment() {
        // Some entities decode to several characters; none of them has its own source bytes.
        let mut node = TextNode::default();
        node.push("ab", 10..15, false);
        assert_eq!(node.source_range(0..1), 10..15);
        assert_eq!(
            node.source_range(1..2),
            10..15,
            "the second character still maps to the whole entity"
        );
        assert_eq!(node.source_range(0..2), 10..15);
    }
}

//! ABOUTME: Strict letter interpreter over the shared located Markdown tree.
//! ABOUTME: Explicit section frames and error-gated models prevent silently lost letter content.

use super::{
    Finding, Sink, Source, annotation, blocks, letter_inline, letter_safety,
    tree::{BlockNode, Document, InlineNode},
};
use crate::{
    diagnostics::{Diagnostic, DiagnosticCode as Code, Severity},
    letter::{AlternateGroup, Item, LetterSource, Piece, SourceItem},
    model::Name,
};
use annotation::{GroupItem, ItemKind, Marked};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Parse outcome; only `valid` exposes a letter, and only when no error was reported.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct ParsedLetter {
    letter: Option<LetterSource>,
    diagnostics: Vec<Diagnostic>,
}
impl ParsedLetter {
    #[must_use]
    pub fn valid(&self) -> Option<&LetterSource> {
        self.letter.as_ref().filter(|_| !self.has_errors())
    }
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
    /// Splits into an error-gated model and all findings.
    #[must_use]
    pub fn into_parts(self) -> (Option<LetterSource>, Vec<Diagnostic>) {
        let model = if self.has_errors() { None } else { self.letter };
        (model, self.diagnostics)
    }
}

/// Parses one cover letter; `path` labels diagnostics and performs no filesystem I/O.
pub fn parse_letter(path: impl AsRef<Path>, text: &str) -> ParsedLetter {
    let mut sink = Sink::new(path.as_ref());
    let letter = super::lower_document(text, &mut sink).and_then(|doc| {
        let base = if text.starts_with(super::BOM) {
            super::BOM.len_utf8()
        } else {
            0
        };
        let normalized = super::normalize_lone_cr(&text[base..]);
        let (letter, owned) = interpret(&doc, &Source { text }, &mut sink);
        // After the interpreter, so that a comment it has already judged is not judged twice.
        letter_safety::inspect(&normalized, base, &owned, &mut sink);
        letter
    });
    let mut diagnostics = sink.finish();
    super::enforce_span_contract(&mut diagnostics, text);
    ParsedLetter { letter, diagnostics }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::Display)]
enum Section {
    Greeting,
    Body,
    Closing,
}
impl Section {
    fn index(self) -> usize {
        match self {
            Self::Greeting => 0,
            Self::Body => 1,
            Self::Closing => 2,
        }
    }
}

/// Interprets the document; the second value is where every annotation comment it judged starts.
///
/// A comment is judged when a shared annotation helper has looked at it: a comment line (`leading`), the end of a
/// paragraph (`trailing`), or anywhere else inside a block the letter reads (`reject_in`). The safety pass then
/// reports only the annotation-shaped comments nobody looked at, which sit where the tree cannot show them.
fn interpret(doc: &Document, src: &Source<'_>, sink: &mut Sink<'_>) -> (Option<LetterSource>, HashSet<usize>) {
    let mut above = annotation::leading(
        &doc.blocks,
        src,
        "put the comment directly above the paragraph it marks, or delete it",
        sink,
    );
    let mut state = State::default();
    for (index, block) in doc.blocks.iter().enumerate() {
        if blocks::is_comment(block) {
            state.own_comment_block(block, src);
            continue;
        }
        state.block(block, above[index].take(), src, sink);
    }
    let owned = std::mem::take(&mut state.owned);
    (state.finish(src.text.len(), sink), owned)
}

#[derive(Default)]
struct State {
    name: Option<Name>,
    contact: Vec<crate::model::Inline>,
    contact_seen: bool,
    current: Option<Section>,
    /// Counts the section headings seen, so a repeated heading does not join the group before it.
    container: usize,
    headings: [Option<std::ops::Range<usize>>; 3],
    items: [Vec<Item>; 3],
    /// Every paragraph in a section, as the group check sees it; the payload is its place in `items`.
    candidates: Vec<GroupItem<(usize, usize)>>,
    /// Paragraphs that show nothing, per section. Each is reported once and kept out of `items`, as the resume
    /// parser drops such a paragraph, so the wordings on either side of it stay adjacent; they still count toward
    /// a frame's one paragraph so that an empty greeting is one error and not two.
    empty: [usize; 3],
    owned: HashSet<usize>,
}
impl State {
    /// Notes every comment of a comment-only block as judged: `leading` has already reported what is wrong.
    fn own_comment_block(&mut self, block: &BlockNode, src: &Source<'_>) {
        self.owned.extend(annotation::judged_in_block(block, src));
    }

    /// Notes every comment among `nodes`, at any depth, as judged by `trailing` or `reject_in`.
    fn own_inline(&mut self, nodes: &[InlineNode]) {
        self.owned.extend(annotation::judged_in(nodes));
    }

    /// A run that takes no annotation: the name, a heading, or the contact line.
    fn forbid_in(&mut self, above: Option<Marked>, why: &str, nodes: &[InlineNode], sink: &mut Sink<'_>) {
        annotation::unclaimed(above, why, sink);
        self.own_inline(nodes);
        annotation::reject_in(nodes, sink);
    }

    fn block(&mut self, block: &BlockNode, above: Option<Marked>, src: &Source<'_>, sink: &mut Sink<'_>) {
        match block {
            BlockNode::Heading(h) if h.level == 1 => {
                self.forbid_in(above, "the name takes no annotation", &h.content, sink);
                letter_inline::forbidden(&h.content, sink);
                if self.name.is_some() || self.current.is_some() {
                    report(
                        Code::DuplicateName,
                        h.range.clone(),
                        "a letter has only one name heading",
                        sink,
                    );
                } else {
                    self.name = blocks::name(h, sink);
                }
            }
            BlockNode::Heading(h) if h.level == 2 => {
                self.forbid_in(above, "a letter heading takes no annotation", &h.content, sink);
                letter_inline::forbidden(&h.content, sink);
                let title = blocks::heading_text(h, sink);
                match title.parse::<Section>() {
                    Ok(section) => self.section(section, h.range.clone(), sink),
                    Err(_) => {
                        report(
                            Code::LetterUnknownSection,
                            h.range.clone(),
                            "expected Greeting, Body or Closing",
                            sink,
                        );
                        self.current = None;
                    }
                }
            }
            BlockNode::Paragraph(p) => match self.current {
                Some(section) => self.paragraph(section, p, above, src, sink),
                None if self.name.is_some() && !self.contact_seen && self.headings.iter().all(Option::is_none) => {
                    self.forbid_in(above, "the contact line takes no annotation", &p.content, sink);
                    self.contact = letter_inline::contact(&p.content, src, sink);
                    self.contact_seen = true;
                }
                None => {
                    self.forbid_in(
                        above,
                        "this paragraph is outside every letter section",
                        &p.content,
                        sink,
                    );
                    report(
                        Code::ContentBeforeSection,
                        p.range.clone(),
                        "paragraph outside a letter section",
                        sink,
                    );
                    letter_inline::convert(&p.content, src, sink);
                }
            },
            _ => {
                annotation::unclaimed(above, "a letter takes annotations only on its paragraphs", sink);
                letter_inline::unsupported(block.range(), sink);
            }
        }
    }

    /// One paragraph of a section, with its annotation from the end of its line or the line above it.
    fn paragraph(
        &mut self,
        section: Section,
        p: &crate::markdown::tree::ParagraphNode,
        above: Option<Marked>,
        src: &Source<'_>,
        sink: &mut Sink<'_>,
    ) {
        self.own_inline(&p.content);
        let trailing = annotation::trailing(&p.content, sink);
        let marked = annotation::combine(above, trailing, sink);
        let content = letter_inline::convert(&p.content, src, sink);
        if !letter_inline::has_content(&content) && super::inline::flatten_text(&p.content).trim().is_empty() {
            annotation::unclaimed(marked, annotation::SHOWS_NOTHING, sink);
            report(
                Code::LetterSectionShape,
                p.range.clone(),
                "letter paragraph has no visible content",
                sink,
            );
            self.empty[section.index()] += 1;
            return;
        }
        if section != Section::Body {
            frame_takes_only_alternates(marked.as_ref(), section, sink);
        }
        let annotation_range = marked.as_ref().map(|m| m.range.clone());
        let annotations = annotation::annotations_of(marked);
        let index = self.items[section.index()].len();
        self.candidates.push(GroupItem {
            alternate: annotations.alternate.clone(),
            priority: annotations.priority,
            container: self.container,
            kind: ItemKind::Paragraph,
            range: annotation_range.clone().unwrap_or_else(|| p.range.clone()),
            payload: (section.index(), index),
        });
        self.items[section.index()].push(Item {
            piece: Piece {
                content,
                range: p.range.clone(),
            },
            annotations,
            annotation_range,
        });
    }

    fn section(&mut self, section: Section, range: std::ops::Range<usize>, sink: &mut Sink<'_>) {
        let i = section.index();
        if self.headings[i].is_some() {
            report(
                Code::LetterDuplicateSection,
                range.clone(),
                "repeated letter section",
                sink,
            );
        }
        let expected = self.headings.iter().filter(|h| h.is_some()).count();
        if i != expected {
            report(
                Code::LetterSectionOrder,
                range.clone(),
                "letter sections must be Greeting, Body, Closing",
                sink,
            );
        }
        if self.name.is_none() {
            report(
                Code::ContentBeforeSection,
                range.clone(),
                "letter section precedes the name",
                sink,
            );
        }
        self.headings[i] = Some(range);
        self.current = Some(section);
        self.container += 1;
    }

    fn finish(mut self, end: usize, sink: &mut Sink<'_>) -> Option<LetterSource> {
        if self.name.is_none() {
            report(Code::MissingName, 0..0, "letter has no valid # Name heading", sink);
        }
        // One check for the whole letter, so a group name used in two sections is seen.
        let groups = annotation::group(&self.candidates, sink);
        let mut sections = std::mem::take(&mut self.items).map(Some);
        let mut built: [Vec<SourceItem>; 3] = Default::default();
        for (section, items) in sections.iter_mut().enumerate() {
            built[section] = source_items(items.take().unwrap_or_default(), section, &groups);
        }
        for section in [Section::Greeting, Section::Body, Section::Closing] {
            let i = section.index();
            match &self.headings[i] {
                None => report(
                    Code::LetterMissingSection,
                    end..end,
                    &format!("missing ## {section}"),
                    sink,
                ),
                Some(range)
                    if built[i].len() + self.empty[i] == 0 || (i != 1 && built[i].len() + self.empty[i] != 1) =>
                {
                    report(
                        Code::LetterSectionShape,
                        range.clone(),
                        "greeting/closing need one paragraph; body needs at least one",
                        sink,
                    );
                }
                _ => {}
            }
        }
        let [greeting, body, closing] = built;
        Some(LetterSource {
            name: self.name?,
            contact: self.contact,
            greeting: greeting.into_iter().next()?,
            body,
            closing: closing.into_iter().next()?,
        })
    }
}

/// Greeting and closing hold one wording of one paragraph, so they take `=group.variant` and nothing else.
fn frame_takes_only_alternates(marked: Option<&Marked>, section: Section, sink: &mut Sink<'_>) {
    let Some(marked) = marked else { return };
    let said = &marked.annotations;
    if said.priority.is_some() || said.status.is_some() || !said.tags.is_empty() {
        sink.report(
            Finding::new(
                Code::AnnotationMisplaced,
                marked.range.clone(),
                format!("the {section} takes no priority, status, or tags"),
            )
            .hint("keep only `=group.variant` here; state priority, status, and tags on body paragraphs"),
        );
    }
}

/// Moves one section's items into source items in authored order; each valid group becomes one item.
///
/// A group is checked before it gets here, so its members are the consecutive paragraphs that share its name.
fn source_items(items: Vec<Item>, section: usize, groups: &[Vec<(usize, usize)>]) -> Vec<SourceItem> {
    let group_of: HashMap<usize, usize> = groups
        .iter()
        .enumerate()
        .flat_map(|(group, members)| members.iter().filter(|m| m.0 == section).map(move |m| (m.1, group)))
        .collect();
    let mut items: Vec<Option<Item>> = items.into_iter().map(Some).collect();
    let mut built = Vec::with_capacity(items.len());
    let mut done = HashSet::new();
    for index in 0..items.len() {
        match group_of.get(&index) {
            Some(&group) if done.insert(group) => {
                let variants = groups[group]
                    .iter()
                    .filter_map(|member| items.get_mut(member.1)?.take())
                    .collect();
                built.push(SourceItem::Alternates(AlternateGroup { variants }));
            }
            Some(_) => {}
            None => built.extend(items[index].take().map(SourceItem::Paragraph)),
        }
    }
    built
}
fn report(code: Code, range: std::ops::Range<usize>, message: &str, sink: &mut Sink<'_>) {
    sink.report(Finding::new(code, range, message).hint(code.advice().fix));
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_some;
    use pretty_assertions::assert_eq;

    fn body(text: &str) -> String {
        format!("# Alex Example\n\n## Greeting\n\nHi.\n\n## Body\n\n{text}\n\n## Closing\n\nBye.\n")
    }

    fn codes(input: &str) -> Vec<String> {
        parse_letter("letter.md", input)
            .diagnostics()
            .iter()
            .map(|d| d.code.to_string())
            .collect()
    }

    /// The model the parser holds even though the letter has an error, for tests that look inside it.
    fn model(input: &str) -> LetterSource {
        assert_some!(parse_letter("letter.md", input).letter).clone()
    }

    #[test]
    fn variants_either_side_of_a_paragraph_that_shows_nothing_stay_one_group() {
        // Dropping the empty paragraph, as the resume parser does, leaves the wordings next to each other.
        let input = body("A. <!-- =x.default -->\n\n![](x.png)\n\nB. <!-- =x.short -->");
        let source = model(&input);
        assert_eq!(source.body.len(), 1);
        let SourceItem::Alternates(group) = &source.body[0] else {
            panic!("a group expected")
        };
        assert_eq!(group.variants().len(), 2);
        assert_eq!(codes(&input), ["letter-unsupported-block", "letter-section-shape"]);
    }

    #[test]
    fn a_paragraph_whose_only_link_is_invalid_has_one_error_and_does_not_count_as_showing_nothing() {
        // The link is the one fault; its words are still on the page as far as the author is concerned.
        let input = body("[words](relative/path)");
        assert_eq!(codes(&input), ["invalid-link"]);
        // It keeps its place, so it still stands between the wordings on either side of it.
        let between = body("A. <!-- =x.default -->\n\n[words](relative/path)\n\nB. <!-- =x.short -->");
        assert_eq!(codes(&between), ["invalid-link", "alternate-not-adjacent"]);
    }

    #[test]
    fn a_paragraph_that_shows_nothing_is_reported_once_whatever_its_section() {
        // Greeting and closing want exactly one paragraph; the empty one counts as that one and adds no second error.
        for (input, expected) in [
            (
                body("B.").replace("Hi.", "**<!-- ordinary -->**"),
                vec!["letter-section-shape"],
            ),
            (
                body("B.").replace("Bye.", "![](x.png)"),
                vec!["letter-unsupported-block", "letter-section-shape"],
            ),
            (
                body("![](x.png)"),
                vec!["letter-unsupported-block", "letter-section-shape"],
            ),
            (
                body("B.").replace("Hi.", "Hi.\n\n![](x.png)"),
                vec![
                    "letter-section-shape",
                    "letter-unsupported-block",
                    "letter-section-shape",
                ],
            ),
        ] {
            assert_eq!(codes(&input), expected, "{input}");
        }
    }
}

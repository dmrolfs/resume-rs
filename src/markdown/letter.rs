//! ABOUTME: Strict letter interpreter over the shared located Markdown tree.
//! ABOUTME: Explicit section frames and error-gated models prevent silently lost letter content.

use super::{
    Finding, Sink, Source, blocks, letter_inline, letter_safety,
    tree::{BlockNode, Document},
};
use crate::{
    diagnostics::{Diagnostic, DiagnosticCode as Code, Severity},
    letter::{LetterSource, Piece},
    model::Name,
};
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
        letter_safety::inspect(&normalized, base, &mut sink);
        interpret(&doc, &Source { text }, &mut sink)
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

fn interpret(doc: &Document, src: &Source<'_>, sink: &mut Sink<'_>) -> Option<LetterSource> {
    let mut state = State::default();
    for block in &doc.blocks {
        if blocks::is_comment(block) {
            continue;
        }
        state.block(block, src, sink);
    }
    state.finish(src.text.len(), sink)
}

#[derive(Default)]
struct State {
    name: Option<Name>,
    contact: Vec<crate::model::Inline>,
    contact_seen: bool,
    current: Option<Section>,
    headings: [Option<std::ops::Range<usize>>; 3],
    pieces: [Vec<Piece>; 3],
}
impl State {
    fn block(&mut self, block: &BlockNode, src: &Source<'_>, sink: &mut Sink<'_>) {
        match block {
            BlockNode::Heading(h) if h.level == 1 => {
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
            BlockNode::Paragraph(p) => {
                if let Some(section) = self.current {
                    let content = letter_inline::convert(&p.content, src, sink);
                    if !letter_inline::has_content(&content)
                        && super::inline::flatten_text(&p.content).trim().is_empty()
                    {
                        report(
                            Code::LetterSectionShape,
                            p.range.clone(),
                            "letter paragraph has no visible content",
                            sink,
                        );
                    }
                    self.pieces[section.index()].push(Piece {
                        content,
                        range: p.range.clone(),
                    });
                } else if self.name.is_some() && !self.contact_seen && self.headings.iter().all(Option::is_none) {
                    self.contact = letter_inline::contact(&p.content, src, sink);
                    self.contact_seen = true;
                } else {
                    report(
                        Code::ContentBeforeSection,
                        p.range.clone(),
                        "paragraph outside a letter section",
                        sink,
                    );
                    letter_inline::convert(&p.content, src, sink);
                }
            }
            _ => letter_inline::unsupported(block.range(), sink),
        }
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
    }

    fn finish(mut self, end: usize, sink: &mut Sink<'_>) -> Option<LetterSource> {
        if self.name.is_none() {
            report(Code::MissingName, 0..0, "letter has no valid # Name heading", sink);
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
                Some(range) if self.pieces[i].is_empty() || (i != 1 && self.pieces[i].len() != 1) => {
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
        Some(LetterSource {
            name: self.name?,
            contact: self.contact,
            greeting: self.pieces[0].pop()?,
            body: std::mem::take(&mut self.pieces[1]),
            closing: self.pieces[2].pop()?,
        })
    }
}
fn report(code: Code, range: std::ops::Range<usize>, message: &str, sink: &mut Sink<'_>) {
    sink.report(Finding::new(code, range, message).hint(code.advice().fix));
}

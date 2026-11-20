//! ABOUTME: Located letter source nodes, kept separate from the serialized Inline contract.
//! ABOUTME: Selection preserves original placeholder byte ranges for resolution diagnostics.

use crate::model::{Inline, LinkUrl, Name};
use std::ops::Range;

/// Closed vocabulary of application fields; recognized by the future letter parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Placeholder {
    Company,
    Title,
    Recipient,
}

/// Source inline content, including unresolved placeholders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LetterInline {
    Text(String),
    Strong(Vec<LetterInline>),
    Emph(Vec<LetterInline>),
    Link {
        url: LinkUrl,
        content: Vec<LetterInline>,
    },
    Code(String),
    /// Original UTF-8 byte range, not an offset into transformed inline text.
    Placeholder {
        key: Placeholder,
        range: Range<usize>,
    },
}

/// One complete paragraph with its original source range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub content: Vec<LetterInline>,
    pub range: Range<usize>,
}

/// Authored paragraphs before selection; annotation groups are added by the tailoring task.
///
/// The parser establishes section shape and valid source ranges before constructing this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterSource {
    pub name: Name,
    pub contact: Vec<Inline>,
    pub greeting: Piece,
    pub body: Vec<Piece>,
    pub closing: Piece,
}

/// Paragraphs retained by selection. Only these pieces need application field values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedLetter {
    pub name: Name,
    pub contact: Vec<Inline>,
    pub greeting: Piece,
    pub body: Vec<Piece>,
    pub closing: Piece,
}

impl From<LetterSource> for SelectedLetter {
    fn from(source: LetterSource) -> Self {
        Self {
            name: source.name,
            contact: source.contact,
            greeting: source.greeting,
            body: source.body,
            closing: source.closing,
        }
    }
}

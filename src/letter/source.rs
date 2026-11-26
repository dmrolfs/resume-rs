//! ABOUTME: Located letter source nodes, kept separate from the serialized Inline contract.
//! ABOUTME: Selection preserves original placeholder byte ranges for resolution diagnostics.

use crate::model::{Annotations, Inline, LinkUrl, Name};
use snafu::Snafu;
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

/// One authored paragraph with what its annotation comment said, if it had one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub piece: Piece,
    pub annotations: Annotations,
    /// The annotation comment's original byte range; `piece.range` stays the paragraph's own.
    pub annotation_range: Option<Range<usize>>,
}

impl Item {
    /// A paragraph that carries no annotation.
    #[must_use]
    pub fn plain(piece: Piece) -> Self {
        Self {
            piece,
            annotations: Annotations::default(),
            annotation_range: None,
        }
    }
}

/// Adjacent wordings of one paragraph (`=group.variant`), in authored order.
///
/// The parser builds it only for a checked group: one group name, unique variant names, one `default`, one
/// priority, all in one section. It keeps every variant; choosing one belongs to selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlternateGroup {
    pub(crate) variants: Vec<Item>,
}

impl AlternateGroup {
    #[must_use]
    pub fn variants(&self) -> &[Item] {
        &self.variants
    }
}

/// What a letter section holds in one place: a paragraph, or a group of alternate wordings of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceItem {
    Paragraph(Item),
    Alternates(AlternateGroup),
}

impl SourceItem {
    /// The paragraph, unless this is a group of alternates.
    #[must_use]
    pub fn paragraph(&self) -> Option<&Piece> {
        match self {
            Self::Paragraph(item) => Some(&item.piece),
            Self::Alternates(_) => None,
        }
    }
}

impl From<Piece> for SourceItem {
    fn from(piece: Piece) -> Self {
        Self::Paragraph(Item::plain(piece))
    }
}

/// Authored paragraphs before selection, annotations and every alternate wording kept.
///
/// The parser establishes section shape and valid source ranges before constructing this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterSource {
    pub name: Name,
    pub contact: Vec<Inline>,
    pub greeting: SourceItem,
    pub body: Vec<SourceItem>,
    pub closing: SourceItem,
}

impl LetterSource {
    /// The annotation of every body paragraph and every variant of a body group, in document order.
    ///
    /// A greeting or closing is a frame, not an item a position spec can address, so it is not here.
    pub(crate) fn body_annotations_mut(&mut self) -> Vec<&mut Annotations> {
        self.body
            .iter_mut()
            .flat_map(|item| match item {
                SourceItem::Paragraph(item) => std::slice::from_mut(item).iter_mut(),
                SourceItem::Alternates(group) => group.variants.iter_mut(),
            })
            .map(|item| &mut item.annotations)
            .collect()
    }
}

/// A letter with annotations cannot become a [`SelectedLetter`] until selection has chosen its paragraphs.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(display("the letter has annotations, so selection must choose its paragraphs first"))]
pub struct SelectionRequired {
    /// The first annotation comment, or the paragraph it marks when its comment has no location.
    pub range: Range<usize>,
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

/// The bridge for a letter that uses no annotations: every paragraph is kept as written.
///
/// Anything annotated, including a group with only a `default`, is refused rather than guessed at: choosing
/// among wordings and drafts is selection's job, and a conversion that copied them all would publish the ones
/// selection would have dropped.
impl TryFrom<LetterSource> for SelectedLetter {
    type Error = SelectionRequired;

    fn try_from(source: LetterSource) -> Result<Self, Self::Error> {
        let LetterSource {
            name,
            contact,
            greeting,
            body,
            closing,
        } = source;
        let one = |item: SourceItem| -> Result<Piece, SelectionRequired> {
            let refuse = |item: &Item| SelectionRequired {
                range: item
                    .annotation_range
                    .clone()
                    .unwrap_or_else(|| item.piece.range.clone()),
            };
            match item {
                SourceItem::Paragraph(item) if item.annotations.is_empty() && item.annotation_range.is_none() => {
                    Ok(item.piece)
                }
                SourceItem::Paragraph(item) => Err(refuse(&item)),
                SourceItem::Alternates(group) => {
                    Err(group.variants.first().map_or(SelectionRequired { range: 0..0 }, refuse))
                }
            }
        };
        Ok(Self {
            name,
            contact,
            greeting: one(greeting)?,
            body: body.into_iter().map(one).collect::<Result<_, _>>()?,
            closing: one(closing)?,
        })
    }
}

//! ABOUTME: Located cover-letter content and literal application-field resolution.
//! ABOUTME: Only resolved letters reach renderers; the resume JSON contract stays unchanged.

mod model;
mod output;
pub use output::LetterOutput;
mod plain;
pub use plain::{LengthUnit, LinkStyle, PlainLetter, TextCounts, TextLimit, TextLimitError, to_plain};
#[cfg(test)]
mod plain_tests;
mod resolve;
mod source;

pub use model::{
    FieldValue, FieldValueError, LETTER_SCHEMA_VERSION, Letter, LetterDate, LetterDateError, LetterFields, LetterKind,
};
pub use resolve::resolve;
pub use source::{
    AlternateGroup, Item, LetterInline, LetterSource, Piece, Placeholder, SelectedLetter, SelectionRequired, SourceItem,
};

#[cfg(test)]
mod tests;

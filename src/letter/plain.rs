//! ABOUTME: Canonical website letter text, exact Unicode counts and nonmutating submission limits.
//! ABOUTME: Rendering has no I/O or Typst dependency; limits inspect the final bytes without truncating.

use super::Letter;
use crate::model::Inline;
use snafu::{Snafu, ensure};

/// How link labels and destinations appear in pasted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum LinkStyle {
    #[default]
    TextAndUrl,
    TextOnly,
}

/// An explicit counting unit; UTF-16 is the default, not a promise about every website's counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display)]
pub enum LengthUnit {
    #[default]
    #[strum(serialize = "utf16")]
    Utf16,
    #[strum(serialize = "scalars")]
    Scalars,
    #[strum(serialize = "bytes")]
    Bytes,
    #[strum(serialize = "words")]
    Words,
}

/// Counts of the exact string supplied, including its trailing LF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCounts {
    pub utf16: usize,
    pub scalars: usize,
    pub bytes: usize,
    /// Unicode-whitespace-separated words, not language-specific segmentation.
    pub words: usize,
    /// LF characters only; Unicode line separators are not rewritten or counted here.
    pub line_breaks: usize,
}
impl TextCounts {
    #[must_use]
    pub fn of(text: &str) -> Self {
        Self {
            utf16: text.encode_utf16().count(),
            scalars: text.chars().count(),
            bytes: text.len(),
            words: text.split_whitespace().count(),
            line_breaks: text.bytes().filter(|b| *b == b'\n').count(),
        }
    }
    #[must_use]
    pub const fn get(self, unit: LengthUnit) -> usize {
        match unit {
            LengthUnit::Utf16 => self.utf16,
            LengthUnit::Scalars => self.scalars,
            LengthUnit::Bytes => self.bytes,
            LengthUnit::Words => self.words,
        }
    }
}

/// Immutable output and counts derived from the same final bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainLetter {
    text: String,
    counts: TextCounts,
}
impl PlainLetter {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
    #[must_use]
    pub const fn counts(&self) -> TextCounts {
        self.counts
    }
    #[must_use]
    pub fn into_string(self) -> String {
        self.text
    }
}

/// An inclusive maximum; zero is valid. Omitting a limit is the caller's policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextLimit {
    pub max: usize,
    pub unit: LengthUnit,
}

/// The selected count exceeds the maximum; the letter text remains unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum TextLimitError {
    #[snafu(display("letter text is {actual} {unit}, over the limit of {max} {unit}"))]
    OverLimit {
        actual: usize,
        max: usize,
        unit: LengthUnit,
    },
}
impl TextLimit {
    /// Checks only; never edits or emits the letter.
    ///
    /// # Errors
    /// Returns `OverLimit` if the final count in the requested unit exceeds `max`.
    pub fn check(self, text: &PlainLetter) -> Result<(), TextLimitError> {
        let actual = text.counts.get(self.unit);
        ensure!(
            actual <= self.max,
            OverLimitSnafu {
                actual,
                max: self.max,
                unit: self.unit
            }
        );
        Ok(())
    }
}

/// Produces greeting, body, closing and signature, with no postal header/contact/date.
///
/// CRLF, lone CR and LF inside joined paragraphs become spaces; all other characters are preserved.
/// Source validation and placeholder resolution belong to the upstream parser and resolver.
#[must_use]
pub fn to_plain(letter: &Letter, links: LinkStyle) -> PlainLetter {
    let mut text = String::new();
    for paragraph in std::iter::once(&letter.greeting).chain(letter.body.iter()) {
        text.push_str(&paragraph_text(paragraph, links));
        text.push_str("\n\n");
    }
    text.push_str(&paragraph_text(&letter.closing, links));
    text.push('\n');
    text.push_str(letter.name.as_ref());
    text.push('\n');
    let counts = TextCounts::of(&text);
    PlainLetter { text, counts }
}

fn paragraph_text(nodes: &[Inline], links: LinkStyle) -> String {
    let mut raw = String::new();
    flatten(nodes, links, &mut raw);
    let mut output = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                output.push(' ');
            }
            '\n' => output.push(' '),
            _ => output.push(c),
        }
    }
    output
}

fn flatten(nodes: &[Inline], links: LinkStyle, output: &mut String) {
    for node in nodes {
        match node {
            Inline::Text { text } | Inline::Code { text } => output.push_str(text),
            Inline::Strong { content } | Inline::Emph { content } => flatten(content, links, output),
            Inline::Link { url, content } => {
                let mut label = String::new();
                flatten(content, links, &mut label);
                output.push_str(&label);
                let url: &url::Url = url.as_ref();
                let target = if url.scheme() == "mailto" && url.query().is_none() && url.fragment().is_none() {
                    url.path()
                } else {
                    url.as_str()
                };
                if links == LinkStyle::TextAndUrl && label != target && label != url.as_str() {
                    output.push_str(" (");
                    output.push_str(target);
                    output.push(')');
                }
            }
        }
    }
}

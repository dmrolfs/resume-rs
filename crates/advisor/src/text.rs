//! ABOUTME: Python-compatible Unicode character offsets into the unchanged text of a job description.
//! ABOUTME: Every `start`, `end` and `source_start` the spike saved counts code points; Rust strings index by byte.

use std::ops::Range;

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, Snafu};

/// A position counted in Unicode scalar values from the start of a text: what Python's `str` index means.
///
/// It is neither a byte offset nor a grapheme index: `e` plus a combining accent is two, and a woman, a zero-width
/// joiner and a second woman is three, though they draw as one emoji. A `CharOffset` carries no text, so it proves
/// nothing about any particular job description until [`IndexedText::slice`] accepts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CharOffset(usize);

impl CharOffset {
    pub const fn new(characters: usize) -> Self {
        Self(characters)
    }

    pub const fn get(self) -> usize {
        self.0
    }

    /// The offset `characters` further on, or `None` if that overflows.
    pub fn checked_add(self, characters: usize) -> Option<Self> {
        self.0.checked_add(characters).map(Self)
    }
}

/// Why a range does not name text in a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Snafu)]
pub enum TextError {
    #[snafu(display("the range {start}..{end} ends before it starts"))]
    Reversed { start: usize, end: usize },
    #[snafu(display("the range {start}..{end} runs past the {length} characters of the text"))]
    PastEnd { start: usize, end: usize, length: usize },
}

/// Unchanged UTF-8 text with the byte position of every character, so a character range slices in constant time.
///
/// The text is never normalised, trimmed or newline-translated: `\r\n` is two characters, as it is in Python when the
/// file is read as bytes and decoded.
#[derive(Debug, Clone)]
pub struct IndexedText<'a> {
    text: &'a str,
    /// The byte offset of each character, then the byte length of the whole text.
    boundaries: Vec<usize>,
}

impl<'a> IndexedText<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut boundaries: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
        boundaries.push(text.len());
        Self { text, boundaries }
    }

    pub fn as_str(&self) -> &'a str {
        self.text
    }

    /// The number of characters.
    pub fn len(&self) -> CharOffset {
        CharOffset(self.boundaries.len() - 1)
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The text between two character offsets. An empty range inside the text, or at its end, is valid.
    ///
    /// # Errors
    /// [`TextError`] when the range is reversed or runs past the end.
    pub fn slice(&self, range: Range<CharOffset>) -> Result<&'a str, TextError> {
        let (start, end) = (range.start.0, range.end.0);
        let length = self.len().0;
        snafu::ensure!(start <= end, ReversedSnafu { start, end });
        snafu::ensure!(end <= length, PastEndSnafu { start, end, length });
        Ok(&self.text[self.boundaries[start]..self.boundaries[end]])
    }

    /// The `characters` characters from `start`.
    ///
    /// # Errors
    /// [`TextError`] when the range runs past the end, including when the count overflows.
    pub fn slice_with_len(&self, start: CharOffset, characters: usize) -> Result<&'a str, TextError> {
        let end = start.checked_add(characters).context(PastEndSnafu {
            start: start.0,
            end: usize::MAX,
            length: self.len().0,
        })?;
        self.slice(start..end)
    }
}

/// The characters Python's `str.strip()` removes: `str.isspace()` over the whole of Unicode.
///
/// Spelled out, because Rust's `char::is_whitespace` differs: it leaves out U+001C to U+001F, which Python strips.
const fn is_python_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | '\u{1c}'..='\u{20}'
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// The number of characters left after stripping Python whitespace from both ends, without changing the text.
///
/// This is the length the boundary rule (1 to 1200) is applied to; the rule itself belongs to the validators.
pub fn trimmed_char_count(text: &str) -> usize {
    text.trim_matches(is_python_space).chars().count()
}

/// How many times `needle` starts in `haystack`, counting overlaps (`aa` occurs twice in `aaa`). An empty needle
/// occurs zero times, as in the spike's `requirements.occurrences`.
pub fn occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack
        .char_indices()
        .filter(|&(at, _)| haystack[at..].starts_with(needle))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;

    #[test]
    fn slices_count_scalars_not_bytes_or_graphemes() {
        let raw = "naïve 😀 café ñ\r\nx";
        let text = IndexedText::new(raw);
        assert_eq!(text.len(), CharOffset::new(17));
        assert_eq!(assert_ok!(text.slice(CharOffset::new(8)..CharOffset::new(12))), "café");
        assert_eq!(assert_ok!(text.slice_with_len(CharOffset::new(16), 1)), "x");
        assert_eq!(IndexedText::new("e\u{301}").len(), CharOffset::new(2));
        assert_eq!(IndexedText::new("👩‍👩").len(), CharOffset::new(3));
        assert_eq!(text.as_str(), raw);
    }

    #[test]
    fn valid_empty_and_terminal_slices_work_and_bad_ranges_are_refused() {
        let text = IndexedText::new("猫\r\n犬\r");
        assert_eq!(assert_ok!(text.slice(CharOffset::new(1)..CharOffset::new(3))), "\r\n");
        assert_eq!(assert_ok!(text.slice(CharOffset::new(5)..CharOffset::new(5))), "");
        assert_err!(text.slice(CharOffset::new(3)..CharOffset::new(2)));
        assert_err!(text.slice(CharOffset::new(0)..CharOffset::new(6)));
        assert_err!(text.slice_with_len(CharOffset::new(1), usize::MAX));
        assert_eq!(
            assert_ok!(IndexedText::new("").slice(CharOffset::new(0)..CharOffset::new(0))),
            ""
        );
    }

    #[test]
    fn emptiness_follows_the_text_not_the_range() {
        assert!(IndexedText::new("").is_empty());
        assert!(!IndexedText::new("a").is_empty());
        assert_eq!(IndexedText::new("").len(), CharOffset::new(0));
    }

    #[test]
    fn occurrences_overlap_and_empty_needle_does_not_match() {
        assert_eq!(occurrences("aaa", "aa"), 2);
        assert_eq!(occurrences("猫猫猫", "猫猫"), 2);
        assert_eq!(occurrences("a😀a😀", "a😀"), 2);
        assert_eq!(occurrences("a", ""), 0);
        assert_eq!(occurrences("", "a"), 0);
    }

    #[test]
    fn trimming_matches_python_without_changing_text() {
        let ranges = [
            9..=13,
            28..=32,
            133..=133,
            160..=160,
            5760..=5760,
            8192..=8202,
            8232..=8233,
            8239..=8239,
            8287..=8287,
            12288..=12288,
        ];
        let whitespace: String = ranges.into_iter().flatten().filter_map(char::from_u32).collect();
        assert_eq!(whitespace.chars().count(), 29);
        assert_eq!(trimmed_char_count(&whitespace), 0);
        for c in whitespace.chars() {
            assert_eq!(trimmed_char_count(&format!("{c}猫{c}")), 1, "U+{:04X}", c as u32);
        }
        assert_eq!(trimmed_char_count("\u{200b}hi\u{feff}"), 4);
        for count in [0, 1, 1200, 1201] {
            let raw = format!("\u{001c}{}\u{3000}", "😀".repeat(count));
            assert_eq!(trimmed_char_count(&raw), count);
        }
    }

    #[test]
    fn offset_wire_form_is_only_a_nonnegative_integer() {
        for raw in ["0", "12"] {
            let offset: CharOffset = assert_ok!(serde_json::from_str(raw));
            assert_eq!(assert_ok!(serde_json::to_string(&offset)), raw);
        }
        for raw in ["-1", "1.0", "1e0", "true", "\"1\"", "null"] {
            assert_err!(serde_json::from_str::<CharOffset>(raw), "{raw}");
        }
    }
}

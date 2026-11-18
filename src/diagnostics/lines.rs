//! ABOUTME: Line index over source text: LF, CRLF, and lone-CR endings, a leading BOM, 1-based char columns.
//! ABOUTME: Shared by the renderer and the Markdown parser so every output agrees on what a line is.

use std::ops::Range;

/// The UTF-8 byte order mark some editors put at the start of a file.
const BOM: char = '\u{FEFF}';

/// A 1-based line and column; the column counts Unicode scalar values from the start of the line's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Location {
    pub line: usize,
    pub column: usize,
}

/// Line start offsets, built once in O(n). Finding the line of an offset is O(log n) and a line's content
/// range is O(1); [`LineIndex::location`] also counts the characters before the offset on its line, so it is
/// linear in the column (about a millisecond for a column deep into a 16 MiB line).
///
/// A line ends at `\n`, `\r\n`, or a lone `\r`. A leading byte order mark belongs to no line: the first
/// line starts after it, so the first visible character is column 1.
#[derive(Debug, Clone)]
pub(crate) struct LineIndex {
    starts: Vec<usize>,
    ends: Vec<usize>,
    len: usize,
}

impl LineIndex {
    /// Indexes `text`.
    pub(crate) fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let first = if text.starts_with(BOM) { BOM.len_utf8() } else { 0 };
        let (mut starts, mut ends) = (vec![first], Vec::new());
        for (i, &byte) in bytes.iter().enumerate() {
            let next_start = match byte {
                b'\n' => i + 1,
                // A `\r` before `\n` belongs to the CRLF the `\n` ends; only a lone `\r` ends a line itself.
                b'\r' if bytes.get(i + 1) != Some(&b'\n') => i + 1,
                _ => continue,
            };
            let line_start = starts.last().copied().unwrap_or(first);
            let crlf = byte == b'\n' && i > line_start && bytes[i - 1] == b'\r';
            ends.push(if crlf { i - 1 } else { i });
            starts.push(next_start);
        }
        ends.push(text.len());
        Self {
            starts,
            ends,
            len: text.len(),
        }
    }

    /// Number of lines, at least 1; text ending in a line terminator ends with an empty line.
    #[cfg(test)]
    pub(crate) fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// The 0-based line containing `offset`, clamped to the text; an offset inside a terminator belongs
    /// to the line it ends.
    pub(crate) fn line_of(&self, offset: usize) -> usize {
        let offset = offset.min(self.len);
        self.starts.partition_point(|&start| start <= offset).saturating_sub(1)
    }

    /// Byte range of a 0-based line's content without its terminator; `None` past the last line.
    pub(crate) fn content(&self, line: usize) -> Option<Range<usize>> {
        Some(*self.starts.get(line)?..*self.ends.get(line)?)
    }

    /// The location of `offset` in `text`, the text this index was built from.
    ///
    /// An offset inside a terminator is the end of its line; one inside a character is its start.
    pub(crate) fn location(&self, text: &str, offset: usize) -> Location {
        debug_assert_eq!(text.len(), self.len, "the index was built from other text");
        let line = self.line_of(offset);
        let (start, end) = (self.starts[line], self.ends[line]);
        let at = text.floor_char_boundary(offset.clamp(start, end));
        Location {
            line: line + 1,
            column: text[start..at].chars().count() + 1,
        }
    }

    /// Snaps `range` to what can be shown: in bounds, ordered, out of the BOM, on character boundaries, and
    /// without trailing line terminators (a span that ends at the next line's start would look multi-line).
    pub(crate) fn snap(&self, text: &str, range: Range<usize>) -> Range<usize> {
        let range = snap_to_boundaries(text, range);
        let start = range.start.max(self.starts[0]);
        let end = range.end.max(start);
        // Walks back over the trailing terminators only, so it costs the length of that run: nothing for
        // ordinary text, and bounded by the text for a span that ends in a very long run of blank lines.
        let kept = text[start..end].trim_end_matches(['\n', '\r']).len();
        start..start + kept
    }
}

/// Clamps `range` into `text`, orders it, and moves its start back and its end forward onto character
/// boundaries, so the result covers at least what the range named.
pub(crate) fn snap_to_boundaries(text: &str, range: Range<usize>) -> Range<usize> {
    let start = text.floor_char_boundary(range.start);
    let end = text.ceil_char_boundary(range.end);
    start..end.max(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_some};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    const BOM: &str = "\u{FEFF}";

    fn lines_of(text: &str) -> Vec<&str> {
        let index = LineIndex::new(text);
        (0..index.line_count())
            .filter_map(|n| index.content(n))
            .map(|r| &text[r])
            .collect()
    }

    fn at(line: usize, column: usize) -> Location {
        Location { line, column }
    }

    #[test]
    fn line_index_splits_lf_crlf_and_lone_cr() {
        assert_eq!(lines_of("a\nb\r\nc\rd"), vec!["a", "b", "c", "d"]);
        assert_eq!(lines_of("a\r\rb"), vec!["a", "", "b"], "two lone CRs are two endings");
        assert_eq!(lines_of("a\r\r\nb"), vec!["a", "", "b"], "a lone CR then CRLF");
        assert_eq!(lines_of("a\n\nb"), vec!["a", "", "b"]);
    }

    #[test]
    fn content_excludes_terminators() {
        let text = "x\r\ny\nz\r";
        let index = LineIndex::new(text);
        assert_eq!(index.content(0), Some(0..1));
        assert_eq!(index.content(1), Some(3..4));
        assert_eq!(index.content(2), Some(5..6));
        assert_eq!(
            index.content(3),
            Some(7..7),
            "text ending in a terminator ends with an empty line"
        );
        assert_none!(index.content(4));
        assert_eq!(index.line_count(), 4);
    }

    #[test]
    fn empty_and_unterminated_text_have_the_expected_lines() {
        assert_eq!(LineIndex::new("").line_count(), 1);
        assert_eq!(LineIndex::new("").content(0), Some(0..0));
        assert_eq!(lines_of("no newline"), vec!["no newline"]);
        assert_eq!(lines_of("\n"), vec!["", ""]);
    }

    #[test]
    fn first_line_starts_after_bom() {
        let text = format!("{BOM}ab\ncd");
        let index = LineIndex::new(&text);
        assert_eq!(index.content(0), Some(3..5));
        assert_eq!(
            index.location(&text, 3),
            at(1, 1),
            "the first visible character is column 1"
        );
        assert_eq!(index.location(&text, 4), at(1, 2));
        assert_eq!(
            index.location(&text, 0),
            at(1, 1),
            "an offset inside the BOM is the line start"
        );
        assert_eq!(LineIndex::new(BOM).content(0), Some(3..3));
        assert_eq!(index.location(&text, 6), at(2, 1));
    }

    #[test]
    fn a_bom_after_the_start_is_ordinary_text() {
        let text = format!("a{BOM}b");
        assert_eq!(LineIndex::new(&text).location(&text, 4), at(1, 3));
    }

    #[test]
    fn column_counts_scalar_values_and_a_tab_as_one() {
        let text = "\t日本語é𝄞x";
        let index = LineIndex::new(text);
        assert_eq!(index.location(text, 0), at(1, 1));
        assert_eq!(index.location(text, 1), at(1, 2), "after the tab");
        assert_eq!(index.location(text, assert_some!(text.find('é'))), at(1, 5));
        assert_eq!(index.location(text, assert_some!(text.find('x'))), at(1, 7));
        let combining = "e\u{301}x";
        assert_eq!(
            LineIndex::new(combining).location(combining, 3),
            at(1, 3),
            "a combining mark is its own column"
        );
    }

    #[test]
    fn an_offset_inside_a_terminator_is_the_end_of_its_line() {
        let text = "ab\r\ncd";
        let index = LineIndex::new(text);
        assert_eq!(index.line_of(2), 0, "the CR of a CRLF");
        assert_eq!(index.line_of(3), 0, "the LF of a CRLF");
        assert_eq!(index.line_of(4), 1);
        assert_eq!(index.location(text, 2), at(1, 3));
        assert_eq!(index.location(text, 3), at(1, 3));
        assert_eq!(index.location(text, 4), at(2, 1));
    }

    #[test]
    fn an_offset_inside_a_character_is_the_start_of_that_character() {
        let text = "a–b";
        assert_eq!(LineIndex::new(text).location(text, 2), at(1, 2));
        assert_eq!(LineIndex::new(text).location(text, 3), at(1, 2));
    }

    #[test]
    fn offsets_past_the_end_clamp_to_the_end() {
        let text = "ab\ncd";
        let index = LineIndex::new(text);
        assert_eq!(index.line_of(999), 1);
        assert_eq!(index.location(text, 999), at(2, 3));
    }

    #[test]
    fn snap_clamps_orders_and_moves_to_character_boundaries() {
        let text = "a–b"; // the en dash occupies bytes 1..4
        let index = LineIndex::new(text);
        assert_eq!(index.snap(text, 2..3), 1..4);
        assert_eq!(index.snap(text, 1..4), 1..4);
        assert_eq!(index.snap(text, 0..99), 0..5);
        let reversed = Range { start: 4, end: 2 };
        assert_eq!(index.snap(text, reversed), 4..4);
        assert_eq!(LineIndex::new("").snap("", 3..9), 0..0);
    }

    #[test]
    fn snap_leaves_the_bom() {
        let text = format!("{BOM}ab");
        let index = LineIndex::new(&text);
        assert_eq!(index.snap(&text, 0..0), 3..3);
        assert_eq!(index.snap(&text, 1..5), 3..5);
        assert_eq!(index.snap(&text, 0..4), 3..4);
    }

    #[test]
    fn snap_drops_trailing_line_terminators() {
        let text = "ab\r\ncd\n";
        let index = LineIndex::new(text);
        assert_eq!(
            index.snap(text, 0..4),
            0..2,
            "a span ending after the CRLF stops before it"
        );
        assert_eq!(index.snap(text, 0..3), 0..2);
        assert_eq!(index.snap(text, 4..7), 4..6);
        assert_eq!(
            index.snap(text, 2..4),
            2..2,
            "a span of only a terminator becomes empty at its start"
        );
        assert_eq!(
            index.snap(text, 0..2),
            0..2,
            "a span that already stops at the content is unchanged"
        );
        let multi = "a\nb\nc";
        assert_eq!(
            LineIndex::new(multi).snap(multi, 0..4),
            0..3,
            "an inner terminator is kept"
        );
    }

    #[test]
    fn snap_to_boundaries_matches_the_documented_rules() {
        let text = "a–b";
        assert_eq!(snap_to_boundaries(text, 2..3), 1..4);
        assert_eq!(snap_to_boundaries(text, 0..99), 0..5);
        let reversed = Range { start: 4, end: 2 };
        assert_eq!(snap_to_boundaries(text, reversed), 4..4);
        assert_eq!(snap_to_boundaries("", 3..9), 0..0);
        let kept = assert_some!(Some(snap_to_boundaries("abc", 1..2)));
        assert_eq!(kept, 1..2);
    }

    /// Reference implementation: walk the characters one at a time.
    fn naive_location(text: &str, offset: usize) -> Location {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let (mut line, mut column) = (1, 1);
        for (n, &(i, c)) in chars.iter().enumerate() {
            if i >= offset {
                break;
            }
            let next = chars.get(n + 1).map(|&(_, c)| c);
            match c {
                '\n' => (line, column) = (line + 1, 1),
                '\r' if next == Some('\n') => {}
                '\r' => (line, column) = (line + 1, 1),
                '\u{FEFF}' if i == 0 => {}
                _ => column += 1,
            }
        }
        Location { line, column }
    }

    proptest! {
        #[test]
        fn line_index_matches_the_naive_reference(
            bom in any::<bool>(),
            pieces in prop::collection::vec(
                prop::sample::select(vec!["a", "bc", "\n", "\r\n", "\r", "\t", "é", "𝄞", "中", " ", "\u{301}"]),
                0..40,
            ),
        ) {
            let text = format!("{}{}", if bom { BOM } else { "" }, pieces.concat());
            let index = LineIndex::new(&text);
            for offset in (0..=text.len()).filter(|&o| text.is_char_boundary(o)) {
                prop_assert_eq!(index.location(&text, offset), naive_location(&text, offset), "offset {} in {:?}", offset, text);
            }
            for line in 0..index.line_count() {
                let content = assert_some!(index.content(line));
                prop_assert!(!text[content].contains(['\n', '\r']), "line {} holds a terminator in {:?}", line, text);
            }
        }

        #[test]
        fn snap_always_yields_a_showable_range(
            pieces in prop::collection::vec(prop::sample::select(vec!["a", "\n", "\r\n", "\r", "é", "中"]), 0..30),
            start in 0usize..80,
            end in 0usize..80,
        ) {
            let text = pieces.concat();
            let index = LineIndex::new(&text);
            let snapped = index.snap(&text, start..end);
            prop_assert!(snapped.start <= snapped.end && snapped.end <= text.len());
            prop_assert!(text.is_char_boundary(snapped.start) && text.is_char_boundary(snapped.end));
            prop_assert!(!text[snapped].ends_with(['\n', '\r']), "{:?} ends in a terminator", text);
        }
    }
}

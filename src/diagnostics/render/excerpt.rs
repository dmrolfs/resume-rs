//! ABOUTME: A bounded, sanitized copy of the lines a finding points at, for codespan to draw.
//! ABOUTME: Labels are recomputed in the copy's coordinates; line and column report the true values.

use std::borrow::Cow;
use std::ops::Range;

use codespan_reporting::files::{Error, Files};

use super::sources::Located;
use crate::diagnostics::lines::Location;

/// Most source lines shown for one finding.
const MAX_EXCERPT_LINES: usize = 6;
/// Most characters shown from one source line.
const WINDOW_CHARS: usize = 120;
/// Characters kept before the anchor when a line is cut.
const LEFT_CONTEXT: usize = 30;
/// Marks where a line was cut.
pub(super) const ELLIPSIS: &str = "...";

/// A bounded, sanitized copy of the shown lines; reports true line and column to codespan.
#[derive(Debug)]
pub(super) struct Excerpt {
    name: String,
    text: String,
    line_starts: Vec<usize>,
    first_line: usize,
    start: Location,
    pub(super) label: Range<usize>,
    pub(super) shortened: bool,
}

impl Excerpt {
    /// Builds the windowed view for one located span.
    pub(super) fn build(located: &Located<'_>) -> Self {
        let first = located.start.line;
        let last = located.end.line.min(first + MAX_EXCERPT_LINES - 1);
        let mut text = String::new();
        let mut line_starts = Vec::new();
        let mut label = 0..0;
        let mut windowed = false;
        for line in first..=last {
            let shown = shown_line(located, line, first, last);
            windowed |= shown.windowed;
            if line > first {
                text.push('\n');
            }
            line_starts.push(text.len());
            if line == first {
                label.start = text.len() + shown.label.start;
            }
            if line == last {
                label.end = text.len() + shown.label.end;
            }
            text.push_str(&shown.text);
        }
        Self {
            name: located.name.clone(),
            text,
            line_starts,
            first_line: first,
            start: located.start,
            label,
            shortened: windowed || last < located.end.line,
        }
    }
}

/// One shown source line, sanitized, with the label's extent inside it.
struct ShownLine {
    text: String,
    label: Range<usize>,
    windowed: bool,
}

/// Builds source line number `line` (1-based) of an excerpt that shows lines `first` to `last`.
fn shown_line(located: &Located<'_>, line: usize, first: usize, last: usize) -> ShownLine {
    let content = located.index.content(line - 1).unwrap_or_default();
    let raw = &located.text[content.clone()];
    let relative = |offset: usize| offset.clamp(content.start, content.end) - content.start;
    let start = if line == first {
        relative(located.range.start)
    } else {
        0
    };
    // A clipped excerpt ends mid-span, so its last line is labelled to the end.
    let end = if line == located.end.line {
        relative(located.range.end)
    } else {
        raw.len()
    };
    let anchor = if line == first {
        start
    } else if line == last {
        end
    } else {
        0
    };

    let window = window(raw, anchor);
    let start = start.clamp(window.range.start, window.range.end);
    let end = end.clamp(start, window.range.end);
    let mut text = String::new();
    if window.cut_left {
        text.push_str(ELLIPSIS);
    }
    push_sanitized(&mut text, &raw[window.range.start..start]);
    let label_start = text.len();
    push_sanitized(&mut text, &raw[start..end]);
    let label_end = text.len();
    push_sanitized(&mut text, &raw[end..window.range.end]);
    if window.cut_right {
        text.push_str(ELLIPSIS);
    }
    ShownLine {
        text,
        label: label_start..label_end,
        windowed: window.cut_left || window.cut_right,
    }
}

fn push_sanitized(out: &mut String, text: &str) {
    out.push_str(&sanitize(text, false));
}

/// The part of a line that is shown, in bytes of that line.
struct Window {
    range: Range<usize>,
    cut_left: bool,
    cut_right: bool,
}

/// A window of [`WINDOW_CHARS`] characters around `anchor`, a byte offset into `line`.
///
/// The window keeps [`LEFT_CONTEXT`] characters before the anchor and moves left when the line
/// ends sooner, so a window is only short when the whole line is. The work is proportional to the
/// window, not to the line.
fn window(line: &str, anchor: usize) -> Window {
    let anchor = line.floor_char_boundary(anchor);
    let mut start = step_back(line, anchor, LEFT_CONTEXT);
    let end = step_forward(line, start, WINDOW_CHARS);
    if end == line.len() {
        start = step_back(line, end, WINDOW_CHARS);
    }
    Window {
        range: start..end,
        cut_left: start > 0,
        cut_right: end < line.len(),
    }
}

/// The byte offset `chars` characters after `from`, or the end of `line`.
fn step_forward(line: &str, from: usize, chars: usize) -> usize {
    line[from..]
        .char_indices()
        .nth(chars)
        .map_or(line.len(), |(offset, _)| from + offset)
}

/// The byte offset `chars` characters before `from`, or the start of `line`.
fn step_back(line: &str, from: usize, chars: usize) -> usize {
    line[..from]
        .char_indices()
        .rev()
        .take(chars)
        .last()
        .map_or(from, |(offset, _)| offset)
}

impl<'a> Files<'a> for Excerpt {
    type FileId = ();
    type Name = &'a str;
    type Source = &'a str;

    fn name(&'a self, (): ()) -> Result<&'a str, Error> {
        Ok(&self.name)
    }

    fn source(&'a self, (): ()) -> Result<&'a str, Error> {
        Ok(&self.text)
    }

    fn line_index(&'a self, (): (), byte_index: usize) -> Result<usize, Error> {
        Ok(self.line_starts.partition_point(|&start| start <= byte_index) - 1)
    }

    fn line_number(&'a self, (): (), line_index: usize) -> Result<usize, Error> {
        Ok(self.first_line + line_index)
    }

    /// The true column of the span's start. The excerpt has one label, which starts on its first line,
    /// so that is the only column codespan asks for (for the `-->` header).
    fn column_number(&'a self, (): (), _line_index: usize, _byte_index: usize) -> Result<usize, Error> {
        Ok(self.start.column)
    }

    fn line_range(&'a self, (): (), line_index: usize) -> Result<Range<usize>, Error> {
        let start = *self.line_starts.get(line_index).ok_or(Error::LineTooLarge {
            given: line_index,
            max: self.line_starts.len() - 1,
        })?;
        let end = self
            .line_starts
            .get(line_index + 1)
            .map_or(self.text.len(), |next| next - 1);
        Ok(start..end)
    }
}

/// Escapes control and bidi characters as ASCII `\u{..}`; keeps tabs (and newlines when `multiline`).
///
/// With `multiline`, `\r\n` and a lone `\r` are first read as `\n`.
pub(super) fn sanitize(text: &str, multiline: bool) -> Cow<'_, str> {
    if !text.chars().any(|c| c == '\r' && multiline || escapes(c, multiline)) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' && multiline {
            chars.next_if_eq(&'\n');
            out.push('\n');
        } else if escapes(c, multiline) {
            out.push_str(&format!("\\u{{{:x}}}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// True for the characters that could drive a terminal or reorder the text around them.
fn escapes(c: char, multiline: bool) -> bool {
    let kept = c == '\t' || (multiline && c == '\n');
    (c.is_control() && !kept) || is_bidi_control(c)
}

/// The bidirectional formatting characters, which can make source read differently from how it is stored.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::render::sources::{Resolved, Sources};
    use crate::diagnostics::{SourceKind, Span};
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;

    fn excerpt(text: &str, range: Range<usize>) -> Excerpt {
        let mut sources = Sources::new(None);
        sources.insert("t.md", text);
        let span = Span {
            file: "t.md".into(),
            kind: SourceKind::Markdown,
            range,
        };
        match sources.resolve(Some(&span)) {
            Resolved::Located(located) => Excerpt::build(&located),
            other => panic!("expected a located span, got {other:?}"),
        }
    }

    fn quoted(excerpt: &Excerpt) -> &str {
        &excerpt.text[excerpt.label.clone()]
    }

    #[test]
    fn a_short_line_is_shown_whole() {
        let shown = excerpt("hello world\nnext\n", 6..11);
        assert_eq!(shown.text, "hello world");
        assert_eq!(quoted(&shown), "world");
        assert!(!shown.shortened);
    }

    #[test]
    fn the_excerpt_reports_the_true_line_and_column() {
        let shown = excerpt("one\ntwo\nthree four\n", 14..18);
        assert_eq!(assert_ok!(shown.line_number((), 0)), 3);
        assert_eq!(assert_ok!(shown.column_number((), 0, shown.label.start)), 7);
        assert_eq!(assert_ok!(shown.name(())), "t.md");
    }

    #[test]
    fn a_long_line_is_windowed_around_the_label_with_the_true_column() {
        let text = format!("{}TARGET{}", "x".repeat(300), "y".repeat(300));
        let shown = excerpt(&text, 300..306);
        assert!(shown.text.starts_with("...x"));
        assert!(shown.text.ends_with("y..."));
        assert_eq!(shown.text.chars().count(), WINDOW_CHARS + 2 * ELLIPSIS.len());
        assert_eq!(quoted(&shown), "TARGET");
        assert_eq!(shown.label.start, ELLIPSIS.len() + LEFT_CONTEXT);
        assert_eq!(assert_ok!(shown.column_number((), 0, shown.label.start)), 301);
        assert!(shown.shortened);
    }

    #[test]
    fn a_line_of_exactly_the_window_is_not_cut_but_one_more_is() {
        let exact = excerpt(&"a".repeat(WINDOW_CHARS), 0..1);
        assert_eq!(exact.text, "a".repeat(WINDOW_CHARS));
        assert!(!exact.shortened);

        let longer = excerpt(&"a".repeat(WINDOW_CHARS + 1), 0..1);
        assert_eq!(longer.text, format!("{}{ELLIPSIS}", "a".repeat(WINDOW_CHARS)));
        assert!(longer.shortened);
    }

    #[test]
    fn a_label_at_the_end_of_a_long_line_keeps_a_full_window_on_the_left() {
        let text = "a".repeat(WINDOW_CHARS + 1);
        let shown = excerpt(&text, WINDOW_CHARS..WINDOW_CHARS + 1);
        assert_eq!(shown.text, format!("{ELLIPSIS}{}", "a".repeat(WINDOW_CHARS)));
        assert_eq!(quoted(&shown), "a");
        assert_eq!(shown.label.start, shown.text.len() - 1);
    }

    #[test]
    fn a_label_wider_than_the_window_is_clipped_to_it() {
        let text = "b".repeat(500);
        let shown = excerpt(&text, 0..500);
        assert_eq!(quoted(&shown), "b".repeat(WINDOW_CHARS));
        assert!(shown.shortened);
    }

    #[test]
    fn a_window_counts_characters_not_bytes() {
        let text = "é".repeat(WINDOW_CHARS + 5);
        let shown = excerpt(&text, 0..2);
        assert_eq!(shown.text.chars().count(), WINDOW_CHARS + ELLIPSIS.len());
        assert_eq!(quoted(&shown), "é");
    }

    #[test]
    fn a_multi_line_span_is_shown_across_its_lines() {
        let shown = excerpt("ab\ncd\nef", 1..7);
        assert_eq!(shown.text, "ab\ncd\nef");
        assert_eq!(quoted(&shown), "b\ncd\ne");
        assert!(!shown.shortened);
    }

    #[test]
    fn a_tall_span_is_clipped_to_the_line_limit() {
        let lines: Vec<String> = (1..=10).map(|n| format!("line{n}")).collect();
        let text = lines.join("\n");
        let shown = excerpt(&text, 0..text.len());
        assert_eq!(shown.text, lines[..MAX_EXCERPT_LINES].join("\n"));
        assert_eq!(shown.label, 0..shown.text.len());
        assert!(shown.shortened);
    }

    #[test]
    fn the_line_limit_is_exact() {
        let six = (1..=6).map(|n| format!("l{n}")).collect::<Vec<_>>().join("\n");
        assert!(!excerpt(&six, 0..six.len()).shortened);
        let seven = format!("{six}\nl7");
        assert!(excerpt(&seven, 0..seven.len()).shortened);
    }

    #[test]
    fn control_and_bidi_characters_are_escaped_and_tabs_kept() {
        let text = "a\u{1b}[31mb\u{202e}c\td";
        let shown = excerpt(text, 0..text.len());
        assert_eq!(shown.text, "a\\u{1b}[31mb\\u{202e}c\td");
        assert!(!shown.text.contains('\u{1b}'));
    }

    #[test]
    fn label_offsets_follow_the_escaped_text() {
        let text = "x\u{1b}y";
        let shown = excerpt(text, 1..2);
        assert_eq!(quoted(&shown), "\\u{1b}");
        assert_eq!(shown.text, "x\\u{1b}y");
    }

    #[test]
    fn a_label_inside_a_crlf_is_clamped_to_the_line_content() {
        let shown = excerpt("ab\r\ncd", 3..4);
        assert_eq!(shown.text, "ab");
        assert_eq!(shown.label, 2..2);
    }

    #[test]
    fn a_zero_width_label_on_an_empty_line_is_empty() {
        let shown = excerpt("a\n\nb", 2..2);
        assert_eq!(shown.text, "");
        assert_eq!(shown.label, 0..0);
        assert_eq!(assert_ok!(shown.line_number((), 0)), 2);
    }

    #[test]
    fn line_ranges_exclude_the_separator_and_stop_after_the_last_line() {
        let shown = excerpt("ab\ncd\nef", 0..8);
        assert_eq!(assert_ok!(shown.line_range((), 0)), 0..2);
        assert_eq!(assert_ok!(shown.line_range((), 1)), 3..5);
        assert_eq!(assert_ok!(shown.line_range((), 2)), 6..8);
        let err = assert_err!(shown.line_range((), 3));
        assert!(matches!(err, Error::LineTooLarge { given: 3, max: 2 }), "{err:?}");
    }

    #[test]
    fn each_line_of_a_multi_line_span_is_windowed_around_its_own_anchor() {
        // The first line is anchored at the span's start, a middle line at its start, and the last
        // line at the span's end, so the end stays visible however long that line is.
        let text = format!("start\n{}M\n{}END", "x".repeat(300), "y".repeat(300));
        let shown = excerpt(&text, 0..text.len());
        let lines: Vec<&str> = shown.text.split('\n').collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "start");
        assert_eq!(lines[1], format!("{}{ELLIPSIS}", "x".repeat(WINDOW_CHARS)));
        assert_eq!(lines[2], format!("{ELLIPSIS}{}END", "y".repeat(WINDOW_CHARS - 3)));
        assert_eq!(&shown.text[shown.label.end - 3..shown.label.end], "END");
        assert!(shown.shortened);
    }

    #[test]
    fn line_index_maps_bytes_to_lines_and_clamps_past_the_end() {
        let shown = excerpt("ab\ncd\nef", 0..8);
        let lines: Vec<usize> = (0..=9).map(|b| assert_ok!(shown.line_index((), b))).collect();
        assert_eq!(lines, [0, 0, 0, 1, 1, 1, 2, 2, 2, 2]);
    }

    #[test]
    fn numbering_continues_from_the_first_shown_line() {
        let shown = excerpt("a\nb\nc\nd", 4..7);
        assert_eq!(assert_ok!(shown.line_number((), 0)), 3);
        assert_eq!(assert_ok!(shown.line_number((), 1)), 4);
    }

    #[test]
    fn sanitize_escapes_every_control_and_bidi_character() {
        let controls = ['\0', '\u{7}', '\u{1b}', '\u{7f}', '\u{85}', '\u{9f}'];
        let bidi = [
            '\u{61c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202e}', '\u{2066}', '\u{2069}',
        ];
        for c in controls.into_iter().chain(bidi) {
            let text = format!("[{c}]");
            assert_eq!(sanitize(&text, false), format!("[\\u{{{:x}}}]", u32::from(c)), "{c:?}");
        }
    }

    #[test]
    fn sanitize_leaves_printable_text_borrowed_and_keeps_tabs() {
        assert!(matches!(sanitize("plain é 中 𝄞\ttab", false), Cow::Borrowed(_)));
        assert_eq!(sanitize("a\tb", false), "a\tb");
    }

    #[test]
    fn sanitize_does_not_escape_the_neighbours_of_the_bidi_ranges() {
        for c in [
            '\u{61b}', '\u{61d}', '\u{200d}', '\u{2010}', '\u{2029}', '\u{202f}', '\u{2065}', '\u{206a}',
        ] {
            let text = c.to_string();
            assert!(matches!(sanitize(&text, false), Cow::Borrowed(_)), "{c:?}");
        }
    }

    #[test]
    fn sanitize_single_line_escapes_newlines() {
        assert_eq!(sanitize("a\nb\rc", false), "a\\u{a}b\\u{d}c");
    }

    #[test]
    fn sanitize_multiline_normalizes_endings_and_keeps_newlines() {
        assert_eq!(sanitize("a\r\nb\rc\nd", true), "a\nb\nc\nd");
        assert!(matches!(sanitize("a\nb", true), Cow::Borrowed(_)));
        assert_eq!(sanitize("a\u{1b}\nb", true), "a\\u{1b}\nb");
    }
}

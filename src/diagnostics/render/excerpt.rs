//! ABOUTME: A bounded, sanitized copy of the lines a finding points at, for codespan to draw.
//! ABOUTME: Labels are recomputed in the copy's coordinates; line and column report the true values.

use std::ops::Range;

use codespan_reporting::files::{Error, Files};
use unicode_width::UnicodeWidthChar;

use super::sanitize::{ELLIPSIS, display_text, sanitize};
use super::sources::Located;
use crate::diagnostics::lines::Location;

/// Most source lines shown for one finding.
const MAX_EXCERPT_LINES: usize = 6;
/// Most characters shown from one source line.
const WINDOW_CHARS: usize = 120;
/// Characters kept before the anchor when a line is cut.
const LEFT_CONTEXT: usize = 30;

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
    /// The span has no visible width, so the caret marks where it sits instead of covering it.
    pub(super) invisible: bool,
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
        let (label, invisible) = if first == last {
            visible_label(&text, label)
        } else {
            (label, false)
        };
        Self {
            name: display_text(&located.name, false),
            text,
            line_starts,
            first_line: first,
            start: located.start,
            label,
            shortened: windowed || last < located.end.line,
            invisible,
        }
    }
}

/// Moves a label that would draw no caret to the next visible character; the flag says it moved.
///
/// codespan draws a caret as wide as the text under it, and widens a zero-length label to the character
/// at its start. A label over only zero-width characters (a zero-width space, combining marks, a variation
/// selector) would therefore draw nothing, and those are the findings where the caret matters most. With
/// no visible character after it, the caret goes past the end of the line.
fn visible_label(line: &str, label: Range<usize>) -> (Range<usize>, bool) {
    let drawn_over = if label.is_empty() {
        line[label.start..]
            .chars()
            .next()
            .map(|c| &line[label.start..label.start + c.len_utf8()])
    } else {
        Some(&line[label.clone()])
    };
    if drawn_over.is_none_or(|text| text.chars().any(is_visible)) {
        return (label, false);
    }
    let moved =
        line[label.end..]
            .char_indices()
            .find(|&(_, c)| is_visible(c))
            .map_or(line.len()..line.len(), |(offset, c)| {
                let start = label.end + offset;
                start..start + c.len_utf8()
            });
    (moved, true)
}

/// True when codespan gives `c` a column: a tab always does, other characters by their display width.
fn is_visible(c: char) -> bool {
    c == '\t' || c.width().unwrap_or(0) > 0
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

#[cfg(test)]
mod tests {
    use super::super::sanitize::MAX_TEXT_CHARS;
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
    fn a_span_of_only_zero_width_characters_points_at_the_next_visible_one() {
        // codespan draws a caret as wide as the text under it, so zero-width text would get none.
        let zero_width_space = excerpt("ab\u{200b}\u{200b}cd", 2..8);
        assert_eq!(quoted(&zero_width_space), "c");
        assert!(zero_width_space.invisible);
        assert_eq!(
            assert_ok!(zero_width_space.column_number((), 0, 0)),
            3,
            "the header keeps the true column"
        );

        let combining = excerpt("ab\u{301}\u{301}cd", 2..6);
        assert_eq!(quoted(&combining), "c");
        assert!(combining.invisible);

        let selector = excerpt("a\u{2764}\u{fe0f}", 4..7);
        assert_eq!(
            quoted(&selector),
            "",
            "a selector at the end of the line points past it"
        );
        assert!(selector.invisible);
    }

    #[test]
    fn a_zero_length_span_before_a_zero_width_character_skips_it() {
        let shown = excerpt("ab\u{200b}cd", 2..2);
        assert_eq!(quoted(&shown), "c");
        assert!(shown.invisible);
    }

    #[test]
    fn an_invisible_span_at_the_end_of_the_line_points_past_the_end() {
        let shown = excerpt("ab\u{200b}", 2..5);
        assert_eq!(shown.label, shown.text.len()..shown.text.len());
        assert!(shown.invisible);
    }

    #[test]
    fn a_run_of_zero_width_characters_is_skipped_to_the_visible_one() {
        let shown = excerpt("a\u{200b}\u{200b}\u{200b}b", 1..4);
        assert_eq!(quoted(&shown), "b");
    }

    #[test]
    fn spans_with_any_visible_width_are_left_alone() {
        let mixed = excerpt("a\u{200b}b", 1..5);
        assert_eq!(quoted(&mixed), "\u{200b}b");
        assert!(!mixed.invisible);

        let tab = excerpt("a\tb", 1..2);
        assert_eq!(quoted(&tab), "\t");
        assert!(!tab.invisible, "a tab has width");

        let empty = excerpt("abc", 1..1);
        assert_eq!(empty.label, 1..1);
        assert!(
            !empty.invisible,
            "a zero-length span before a visible character is drawn by codespan"
        );

        let at_end = excerpt("abc", 3..3);
        assert!(!at_end.invisible);

        let wide = excerpt("a中b", 1..4);
        assert_eq!(quoted(&wide), "中");
        assert!(!wide.invisible);
    }

    #[test]
    fn multi_line_spans_are_not_moved() {
        let shown = excerpt("ab\n\u{200b}", 0..6);
        assert_eq!(shown.label.start, 0);
        assert!(!shown.invisible);
    }

    #[test]
    fn the_file_name_is_escaped_and_capped_in_the_excerpt() {
        let mut sources = Sources::new(None);
        let long = format!("{}.md", "n".repeat(MAX_TEXT_CHARS + 10));
        for (name, shown) in [
            ("a\u{1b}[31m.md".to_string(), "a\\u{1b}[31m.md".to_string()),
            (long.clone(), format!("{}{ELLIPSIS}", "n".repeat(MAX_TEXT_CHARS))),
        ] {
            sources.insert(name.as_str(), "abc");
            let span = Span {
                file: name.as_str().into(),
                kind: SourceKind::Markdown,
                range: 0..1,
            };
            let excerpt = match sources.resolve(Some(&span)) {
                Resolved::Located(located) => Excerpt::build(&located),
                other => panic!("expected a located span, got {other:?}"),
            };
            assert_eq!(assert_ok!(excerpt.name(())), shown);
        }
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
}

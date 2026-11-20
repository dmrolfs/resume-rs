//! ABOUTME: Makes untrusted text safe to print: control and bidi characters become ASCII escapes, long text is cut.
//! ABOUTME: Used for everything a terminal could be driven by: excerpts, messages, hints, and file names.

use std::borrow::Cow;

/// Marks where text was cut.
pub(super) const ELLIPSIS: &str = "...";
/// Longest message, hint, or file name shown in human output; JSON stays complete.
pub(super) const MAX_TEXT_CHARS: usize = 500;

/// Cuts `text` to [`MAX_TEXT_CHARS`] characters, marking the cut.
pub(super) fn cap(text: &str) -> Cow<'_, str> {
    match text.char_indices().nth(MAX_TEXT_CHARS) {
        Some((end, _)) => Cow::Owned(format!("{}{ELLIPSIS}", &text[..end])),
        None => Cow::Borrowed(text),
    }
}

/// `text` as shown to a person: cut to `MAX_TEXT_CHARS` characters, then sanitized.
pub fn display_text(text: &str, multiline: bool) -> String {
    sanitize(&cap(text), multiline).into_owned()
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
            out.extend(c.escape_unicode());
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
    use pretty_assertions::assert_eq;

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

    #[test]
    fn text_of_exactly_the_cap_is_kept_whole_and_one_more_is_cut() {
        let exact = "é".repeat(MAX_TEXT_CHARS);
        assert_eq!(cap(&exact), exact);
        assert!(matches!(cap(&exact), Cow::Borrowed(_)));
        let longer = "é".repeat(MAX_TEXT_CHARS + 1);
        assert_eq!(cap(&longer), format!("{exact}{ELLIPSIS}"));
    }

    #[test]
    fn display_text_cuts_before_it_escapes() {
        let text = format!("{}\u{1b}", "m".repeat(MAX_TEXT_CHARS));
        assert_eq!(
            display_text(&text, false),
            format!("{}{ELLIPSIS}", "m".repeat(MAX_TEXT_CHARS))
        );
        assert_eq!(display_text("a\u{1b}b", false), "a\\u{1b}b");
    }
}

//! ABOUTME: Turns one finding into a codespan diagnostic: header, caret label, and notes.
//! ABOUTME: ASCII frame only; message and hints are capped and sanitized before codespan prints them.

use std::borrow::Cow;

use codespan_reporting::diagnostic::{Diagnostic as Drawn, Label, Severity as DrawnSeverity};
use codespan_reporting::term::{Chars, Config};

use super::excerpt::{ELLIPSIS, Excerpt, sanitize};
use super::sources::Resolved;
use crate::diagnostics::{Diagnostic, Severity};

/// Longest message or hint shown in human output; JSON stays complete.
const MAX_TEXT_CHARS: usize = 500;
/// The caret label of a finding that carries an Edit.
const FIXABLE: &str = "fixable";

/// The codespan configuration: ASCII characters, tab width 4.
pub(super) fn config() -> Config {
    Config {
        chars: Chars::ascii(),
        ..Config::default()
    }
}

/// Converts one finding (with its excerpt, if located) to a codespan diagnostic.
pub(super) fn to_codespan(d: &Diagnostic, resolved: &Resolved<'_>, excerpt: Option<&Excerpt>) -> Drawn<()> {
    let drawn = Drawn::new(severity(d.severity))
        .with_code(d.code)
        .with_message(sanitize(&cap(&d.message), false))
        .with_notes(notes(d, resolved, excerpt));
    match excerpt {
        Some(excerpt) => {
            let marker = if d.fix.is_some() { FIXABLE } else { "" };
            drawn.with_label(Label::primary((), excerpt.label.clone()).with_message(marker))
        }
        None => drawn,
    }
}

fn severity(severity: Severity) -> DrawnSeverity {
    match severity {
        Severity::Note => DrawnSeverity::Note,
        Severity::Warning => DrawnSeverity::Warning,
        Severity::Error => DrawnSeverity::Error,
    }
}

/// Truncation, unlocated, hint, and fixable notes, sanitized and capped.
///
/// A finding with no excerpt has no caret to carry the `fixable` marker, so it gets a note instead.
fn notes(d: &Diagnostic, resolved: &Resolved<'_>, excerpt: Option<&Excerpt>) -> Vec<String> {
    let mut notes = Vec::new();
    match resolved {
        Resolved::Located(located) if excerpt.is_some_and(|shown| shown.shortened) => notes.push(format!(
            "note: excerpt shortened; the span runs from {}:{} to {}:{}",
            located.start.line, located.start.column, located.end.line, located.end.column
        )),
        Resolved::Unlocated { name, reason } => notes.push(format!("note: in `{name}`; no excerpt: {reason}")),
        Resolved::Located(_) | Resolved::NoSpan => {}
    }
    notes.extend(
        d.hints
            .iter()
            .filter(|hint| !hint.is_empty())
            .map(|hint| format!("help: {}", sanitize(&cap(hint), true))),
    );
    if excerpt.is_none() && d.fix.is_some() {
        notes.push("note: fixable".into());
    }
    notes
}

/// Cuts `text` to [`MAX_TEXT_CHARS`] characters, marking the cut.
fn cap(text: &str) -> Cow<'_, str> {
    match text.char_indices().nth(MAX_TEXT_CHARS) {
        Some((end, _)) => Cow::Owned(format!("{}{ELLIPSIS}", &text[..end])),
        None => Cow::Borrowed(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::render::sources::Sources;
    use crate::diagnostics::{DiagnosticCode, Edit, SourceKind, Span};
    use pretty_assertions::assert_eq;

    fn finding(range: Option<std::ops::Range<usize>>, hints: &[&str], fixable: bool) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code: DiagnosticCode::PeriodHyphen,
            message: "dates are separated by `-`".into(),
            span: range.map(|range| Span {
                file: "t.md".into(),
                kind: SourceKind::Markdown,
                range,
            }),
            hints: hints.iter().map(ToString::to_string).collect(),
            fix: fixable.then(|| Edit {
                range: 0..0,
                replacement: String::new(),
            }),
        }
    }

    fn convert(d: &Diagnostic, text: Option<&str>) -> Drawn<()> {
        let mut sources = Sources::new(None);
        if let Some(text) = text {
            sources.insert("t.md", text);
        }
        let resolved = sources.resolve(d.span.as_ref());
        let excerpt = match &resolved {
            Resolved::Located(located) => Some(Excerpt::build(located)),
            _ => None,
        };
        to_codespan(d, &resolved, excerpt.as_ref())
    }

    #[test]
    fn severity_maps_one_to_one() {
        assert_eq!(severity(Severity::Note), DrawnSeverity::Note);
        assert_eq!(severity(Severity::Warning), DrawnSeverity::Warning);
        assert_eq!(severity(Severity::Error), DrawnSeverity::Error);
    }

    #[test]
    fn the_code_and_message_form_the_header() {
        let drawn = convert(&finding(Some(0..1), &[], false), Some("abc"));
        assert_eq!(drawn.severity, DrawnSeverity::Warning);
        assert_eq!(drawn.code.as_deref(), Some("period-hyphen"));
        assert_eq!(drawn.message, "dates are separated by `-`");
    }

    #[test]
    fn a_located_finding_has_one_primary_label_over_the_excerpt() {
        let drawn = convert(&finding(Some(1..3), &[], false), Some("abcd"));
        assert_eq!(drawn.labels.len(), 1);
        assert_eq!(drawn.labels[0].range, 1..3);
        assert_eq!(drawn.labels[0].message, "");
    }

    #[test]
    fn the_label_says_fixable_only_when_there_is_an_edit() {
        let drawn = convert(&finding(Some(0..1), &[], true), Some("abc"));
        assert_eq!(drawn.labels[0].message, "fixable");
        assert!(
            drawn.notes.is_empty(),
            "a located fixable finding needs no note: {:?}",
            drawn.notes
        );
    }

    #[test]
    fn hints_become_help_notes_in_order_and_empty_hints_are_skipped() {
        let drawn = convert(&finding(Some(0..1), &["first", "", "second"], false), Some("abc"));
        assert_eq!(drawn.notes, ["help: first", "help: second"]);
    }

    #[test]
    fn a_finding_without_a_span_has_no_label() {
        let drawn = convert(&finding(None, &["h"], false), None);
        assert!(drawn.labels.is_empty());
        assert_eq!(drawn.notes, ["help: h"]);
    }

    #[test]
    fn a_fixable_finding_without_an_excerpt_says_so() {
        let no_span = convert(&finding(None, &[], true), None);
        assert_eq!(no_span.notes, ["note: fixable"]);
        let unlocated = convert(&finding(Some(0..1), &["h"], true), None);
        assert_eq!(unlocated.notes[1..], ["help: h", "note: fixable"]);
    }

    #[test]
    fn an_unlocated_finding_names_its_file_and_the_reason() {
        let missing = convert(&finding(Some(0..1), &[], false), None);
        assert_eq!(missing.notes, ["note: in `t.md`; no excerpt: source not available"]);
        let past_end = convert(&finding(Some(9..12), &[], false), Some("abc"));
        assert_eq!(
            past_end.notes,
            ["note: in `t.md`; no excerpt: span 9..12 is past the end (3 bytes)"]
        );
        assert!(past_end.labels.is_empty());
    }

    #[test]
    fn a_shortened_excerpt_notes_the_true_extents() {
        let text = (1..=10).map(|n| format!("line{n}")).collect::<Vec<_>>().join("\n");
        let drawn = convert(&finding(Some(0..text.len()), &["h"], false), Some(&text));
        assert_eq!(
            drawn.notes,
            ["note: excerpt shortened; the span runs from 1:1 to 10:7", "help: h"]
        );
    }

    #[test]
    fn a_whole_excerpt_has_no_shortened_note() {
        let drawn = convert(&finding(Some(0..3), &[], false), Some("abc"));
        assert!(drawn.notes.is_empty());
    }

    #[test]
    fn message_and_hints_are_capped_with_a_marker() {
        let long = "m".repeat(MAX_TEXT_CHARS + 50);
        let mut d = finding(None, &[], false);
        d.message = long.clone();
        d.hints = vec![long];
        let drawn = convert(&d, None);
        let expected = format!("{}{ELLIPSIS}", "m".repeat(MAX_TEXT_CHARS));
        assert_eq!(drawn.message, expected);
        assert_eq!(drawn.notes, [format!("help: {expected}")]);
    }

    #[test]
    fn text_of_exactly_the_cap_is_kept_whole() {
        let exact = "é".repeat(MAX_TEXT_CHARS);
        assert_eq!(cap(&exact), exact);
        assert!(matches!(cap(&exact), Cow::Borrowed(_)));
    }

    #[test]
    fn message_and_hints_are_sanitized() {
        let mut d = finding(None, &[], false);
        d.message = "a\u{1b}[31m\nb".into();
        d.hints = vec!["x\r\ny\u{202e}".into()];
        let drawn = convert(&d, None);
        assert_eq!(drawn.message, "a\\u{1b}[31m\\u{a}b");
        assert_eq!(drawn.notes, ["help: x\ny\\u{202e}"]);
    }

    #[test]
    fn the_frame_is_ascii() {
        let chars = config().chars;
        assert_eq!(chars.snippet_start, "-->");
        assert_eq!(chars.source_border_left, '|');
        assert!(chars.source_border_left_break.is_ascii());
    }
}

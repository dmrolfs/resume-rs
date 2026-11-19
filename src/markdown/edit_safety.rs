//! ABOUTME: Proves that deleting a hard-break marker cannot change how a resume is read, or withholds the Edit.
//! ABOUTME: Two proofs: a local re-parse of the lines around the break, then a re-parse of the whole document.

use std::cmp::Reverse;
use std::ops::Range;

use pulldown_cmark::{Event, Parser};

use super::tree::options;
use super::{BOM, Parsed, normalize_lone_cr};
use crate::diagnostics::{Diagnostic, DiagnosticCode, Edit, Severity};

/// What a hard-break finding says when its Edit is withheld.
pub(super) const REWRITE_HINT: &str = "rewrite or delete this line: removing only the marker would change how the \
                                       document is read, for example by leaving a blank line or a line of markup";

/// The first proof: deleting the marker at `at` leaves the lines around it reading the same.
///
/// It is cheap and decides each break on its own, so one odd break does not cost the others their Edit, but it
/// only sees three lines. It reads the text the parser lowers (no BOM, a lone `\r` as a line end), so a document
/// and its CR twin get the same answer. The second proof, [`verify_hard_break_edits`], is the one that settles it.
pub(super) fn local_proof(text: &str, at: &Range<usize>) -> bool {
    let window = lines_around(text, at);
    // The parser never sees a BOM, and a BOM in front of `-` or `#` would hide the markup from this check.
    let first = window.start.max(if text.starts_with(BOM) { BOM.len_utf8() } else { 0 });
    let (Some(around), Some(start)) = (text.get(first..window.end), at.start.checked_sub(first)) else {
        return false;
    };
    deletion_keeps_structure(&normalize_lone_cr(around), start..at.end - first)
}

/// True when deleting `delete` from `window` leaves it parsing to the same blocks and the same inline
/// content, counting a hard break and a soft break as one thing.
///
/// Removing a line-end marker can leave a blank line, which is a paragraph break, or a line that is nothing
/// but `-`, `---`, `=`, `#`, or `>`, which becomes a setext underline, an empty heading, or a block quote.
/// Re-parsing with the parser's own options means the answer cannot drift from the real parse.
fn deletion_keeps_structure(window: &str, delete: Range<usize>) -> bool {
    let (Some(head), Some(tail)) = (window.get(..delete.start), window.get(delete.end..)) else {
        return false;
    };
    let edited = format!("{head}{tail}");
    shape(window) == shape(&edited)
}

/// The parse events of `text`, with every hard break read as a soft one.
fn shape(text: &str) -> Vec<Event<'_>> {
    Parser::new_ext(text, options())
        .map(|event| match event {
            Event::HardBreak => Event::SoftBreak,
            other => other,
        })
        .collect()
}

/// The source line holding `at`, with the line before it and the line after it.
fn lines_around(text: &str, at: &Range<usize>) -> Range<usize> {
    let line_start = text[..at.start].rfind(['\n', '\r']).map_or(0, |i| i + 1);
    let before = &text[..line_start];
    let before = before
        .strip_suffix("\r\n")
        .or_else(|| before.strip_suffix(['\n', '\r']))
        .unwrap_or(before);
    let start = before.rfind(['\n', '\r']).map_or(0, |i| i + 1);

    let end_of_line = text[at.end..].find(['\n', '\r']).map_or(text.len(), |i| at.end + i);
    let next_start = end_of_line + if text[end_of_line..].starts_with("\r\n") { 2 } else { 1 };
    let end = text
        .get(next_start..)
        .and_then(|rest| rest.find(['\n', '\r']))
        .map_or(text.len(), |i| next_start + i);
    start..end
}

/// The second proof, and the one that settles it: apply every surviving hard-break Edit together, parse the
/// whole document again, and withdraw them all unless the model and every other finding are unchanged.
///
/// Three lines cannot show what lies beyond them (an emphasis closer, a tag's end, a link definition, a
/// construct before the window) or how Edits applied in the same pass interact (two lines that become a table
/// together). The whole document can. A failure withdraws every hard-break Edit, so the check fails closed: the
/// findings stay, with a hint to rewrite the text. `analyze` is the parse without this step, so it cannot recurse.
pub(super) fn verify_hard_break_edits(parsed: &mut Parsed, text: &str, analyze: impl FnOnce(&str) -> Parsed) {
    let mut edits: Vec<&Edit> = parsed
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::HardBreak)
        .filter_map(|d| d.fix.as_ref())
        .collect();
    if edits.is_empty() {
        return;
    }
    edits.sort_by_key(|edit| Reverse(edit.range.start));
    let mut fixed = text.to_string();
    for edit in edits {
        fixed.replace_range(edit.range.clone(), &edit.replacement);
    }
    let after = analyze(&fixed);
    if parsed.resume == after.resume && other_findings(&parsed.diagnostics) == other_findings(&after.diagnostics) {
        return;
    }
    for diagnostic in &mut parsed.diagnostics {
        if diagnostic.code == DiagnosticCode::HardBreak && diagnostic.fix.is_some() {
            diagnostic.fix = None;
            diagnostic.hints = vec![REWRITE_HINT.to_string()];
        }
    }
}

/// The findings that are not hard breaks, as sorted `(severity, code, message)`: positions move when text is
/// deleted, so spans are left out.
fn other_findings(diagnostics: &[Diagnostic]) -> Vec<(Severity, &'static str, &str)> {
    let mut findings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code != DiagnosticCode::HardBreak)
        .map(|d| (d.severity, <&'static str>::from(d.code), d.message.as_str()))
        .collect();
    findings.sort_unstable();
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse;
    use claims::{assert_none, assert_some};
    use pretty_assertions::assert_eq;

    /// `deletion_keeps_structure` over `window` with the first occurrence of `marker` deleted.
    fn keeps(window: &str, marker: &str) -> bool {
        let start = assert_some!(window.find(marker), "{marker:?} is not in {window:?}");
        deletion_keeps_structure(window, start..start + marker.len())
    }

    /// `lines_around` for the first occurrence of `marker` in `text`, as the text it selects.
    fn around<'t>(text: &'t str, marker: &str) -> &'t str {
        let start = assert_some!(text.find(marker), "{marker:?} is not in {text:?}");
        &text[lines_around(text, &(start..start + marker.len()))]
    }

    /// `local_proof` for the first occurrence of `marker`.
    fn locally_safe(text: &str, marker: &str) -> bool {
        let start = assert_some!(text.find(marker), "{marker:?} is not in {text:?}");
        local_proof(text, &(start..start + marker.len()))
    }

    #[test]
    fn deleting_a_break_marker_after_text_keeps_the_structure() {
        assert!(keeps("one\\\ntwo", "\\"), "a backslash break becomes a soft break");
        assert!(keeps("one  \ntwo", "  "), "so do trailing spaces");
        assert!(keeps("one\r\ntwo  \r\nthree", "  "));
        assert!(keeps("- item  \n  more", "  "), "a list item keeps its shape");
        assert!(keeps("a\n# b  \nc", "  "), "even beside a heading");
    }

    #[test]
    fn deleting_a_marker_that_leaves_a_blank_line_changes_the_structure() {
        assert!(!keeps("one\n\\\ntwo", "\\"), "a paragraph break");
        assert!(!keeps("one\n  \\\ntwo", "\\"), "an indented one is blank too");
    }

    #[test]
    fn deleting_a_marker_that_leaves_only_markup_changes_the_structure() {
        for (window, marker) in [
            ("one\n-\\\ntwo", "\\"),   // a setext underline
            ("one\n---\\\ntwo", "\\"), // a setext underline too
            ("one\n=\\\ntwo", "\\"),   // setext level 1
            ("one\n#\\\ntwo", "\\"),   // an empty heading
            ("one\n>\\\ntwo", "\\"),   // an empty block quote
        ] {
            assert!(!keeps(window, marker), "{window:?}");
        }
    }

    #[test]
    fn an_unusable_range_is_not_a_proof() {
        assert!(!deletion_keeps_structure("one\\\ntwo", 3..99), "past the end");
        assert!(!deletion_keeps_structure("é\\\nx", 1..2), "inside a character");
    }

    #[test]
    fn lines_around_is_the_previous_the_current_and_the_next_line() {
        assert_eq!(around("a\nb\nc  \nd\ne", "  "), "b\nc  \nd");
    }

    #[test]
    fn lines_around_stops_at_the_edges_of_the_text() {
        assert_eq!(around("x  \ny\nz", "  "), "x  \ny", "no line before the first");
        assert_eq!(around("w\nx  \ny", "  "), "w\nx  \ny", "the last line is the next one");
    }

    #[test]
    fn lines_around_reaches_the_end_of_the_line_from_a_span_in_the_middle_of_it() {
        // A hard break's span always ends its line, but the function does not assume that.
        assert_eq!(around("a\nb c d\ne\nf", " c"), "a\nb c d\ne");
        assert_eq!(around("a\r\nb c d\r\ne\r\nf", " c"), "a\r\nb c d\r\ne");
    }

    #[test]
    fn lines_around_handles_every_line_ending() {
        assert_eq!(around("a\r\nb\r\nc  \r\nd\r\ne", "  "), "b\r\nc  \r\nd", "CRLF");
        assert_eq!(around("a\rb\rc  \rd\re", "  "), "b\rc  \rd", "a lone CR");
        assert_eq!(around("a\n\nc  \nd", "  "), "\nc  \nd", "a blank line before is a line");
    }

    #[test]
    fn the_local_proof_ignores_a_leading_bom() {
        // `*` alone after the heading would become an empty list item; a BOM glued to the first line must not
        // hide that line's markup from the check.
        let plain = "# Ada\n*\\\nLondon\n";
        let with_bom = format!("\u{feff}{plain}");
        assert!(!locally_safe(plain, "\\"));
        assert!(!locally_safe(&with_bom, "\\"), "the BOM changed the answer");
        let first_line = "\u{feff}-\\\nfoo\n";
        assert_eq!(locally_safe(first_line, "\\"), locally_safe("-\\\nfoo\n", "\\"));
    }

    #[test]
    fn the_local_proof_gives_a_document_and_its_lone_cr_twin_the_same_answer() {
        for text in [
            "# Ada\n\nLondon\n    Seattle\nPortland  \nAustin\n",
            "# Ada\n\nLondon\n-\\\nSeattle\n",
            "# Ada\n\nLondon\n\\\nSeattle\n",
            "# Ada\n\nLondon\\\nSeattle\n",
        ] {
            let marker = if text.contains("  \n") { "  " } else { "\\" };
            let start = assert_some!(text.rfind(marker));
            let cr = text.replace('\n', "\r");
            assert_eq!(
                local_proof(&cr, &(start..start + marker.len())),
                local_proof(text, &(start..start + marker.len())),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_break_the_local_proof_cannot_judge_is_withdrawn_by_the_whole_document() {
        // `[foo][\` + `bar]` has no live reference (the label contains a backslash), but once the backslash is
        // gone it names the definition below, which is outside any three-line window.
        let text = "# Ada\n\nSee [foo][\\\nbar] now\n\n[bar]: https://x.y\n";
        let at = assert_some!(text.find('\\'));
        assert!(local_proof(text, &(at..at + 1)), "the window cannot see the definition");
        let parsed = parse("r.md", text);
        let found: Vec<_> = parsed
            .diagnostics()
            .iter()
            .filter(|d| d.code == DiagnosticCode::HardBreak)
            .collect();
        assert_eq!(found.len(), 1);
        assert_none!(found[0].fix.as_ref());
        assert_eq!(found[0].hints, [REWRITE_HINT]);
    }

    #[test]
    fn one_unsafe_break_withdraws_every_hard_break_edit_in_the_document() {
        // The first break is harmless on its own; the second makes a link live. The check fails closed.
        let text = "# Ada\n\nLondon  \nSeattle\n\n## S\n\n### Acme · Eng · 2015 – 2017\n\nSee [foo][\\\nbar] now\n\n[bar]: https://x.y\n";
        let parsed = parse("r.md", text);
        let found: Vec<_> = parsed
            .diagnostics()
            .iter()
            .filter(|d| d.code == DiagnosticCode::HardBreak)
            .collect();
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|d| d.fix.is_none()), "{found:?}");
    }

    /// A finding as the parser reports it, optionally carrying an Edit that deletes `range`.
    fn finding(code: DiagnosticCode, message: &str, edit: Option<Range<usize>>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code,
            message: message.to_string(),
            span: None,
            hints: vec!["a hint".to_string()],
            fix: edit.map(|range| Edit {
                range,
                replacement: String::new(),
            }),
        }
    }

    /// Runs the whole-document proof on `text` with the re-parse stubbed to return `after`.
    fn verified(text: &str, before: Vec<Diagnostic>, after: Vec<Diagnostic>) -> Vec<Diagnostic> {
        let mut parsed = Parsed::new(None, before);
        verify_hard_break_edits(&mut parsed, text, |_| Parsed::new(None, after));
        parsed.diagnostics
    }

    #[test]
    fn a_difference_in_the_other_findings_alone_withdraws_the_edits() {
        // The model is the same (none in both); a finding that was there is gone after the Edit.
        let break_with_edit = finding(DiagnosticCode::HardBreak, "break", Some(1..3));
        let other = finding(DiagnosticCode::MidSentenceBold, "bold inside a sentence", None);
        let result = verified("a  \nb", vec![break_with_edit, other.clone()], vec![]);
        let hard = &result[0];
        assert_none!(hard.fix.as_ref());
        assert_eq!(hard.hints, [REWRITE_HINT]);
        assert_eq!(result[1], other, "other findings are left alone");
    }

    #[test]
    fn withdrawing_hard_break_edits_leaves_other_findings_and_their_edits_alone() {
        // The period Edit belongs to another finding: only hard-break Edits are ours to withdraw.
        let period = finding(DiagnosticCode::PeriodHyphen, "use an en dash", Some(10..11));
        let unfixable_break = finding(DiagnosticCode::HardBreak, "unfixable break", None);
        let before = vec![
            finding(DiagnosticCode::HardBreak, "break", Some(1..3)),
            period.clone(),
            unfixable_break.clone(),
        ];
        let result = verified("a  \nb with a long enough text", before, vec![]);
        assert_none!(result[0].fix.as_ref());
        assert_eq!(result[0].hints, [REWRITE_HINT]);
        assert_eq!(result[1], period, "another finding keeps its Edit and its hint");
        assert_eq!(
            result[2], unfixable_break,
            "a break that never had an Edit is untouched"
        );
    }

    #[test]
    fn a_different_message_is_a_difference() {
        let before = vec![
            finding(DiagnosticCode::HardBreak, "break", Some(1..3)),
            finding(DiagnosticCode::MidSentenceBold, "one", None),
        ];
        let after = vec![finding(DiagnosticCode::MidSentenceBold, "two", None)];
        assert_none!(verified("a  \nb", before, after)[0].fix.as_ref());
    }

    #[test]
    fn identical_other_findings_keep_the_edits_whatever_happens_to_the_hard_breaks() {
        let other = finding(DiagnosticCode::MidSentenceBold, "bold inside a sentence", None);
        // The fixed document has no hard-break finding at all (the Edit removed it), and another break that
        // was never fixable stays in both: neither counts as a difference.
        let before = vec![
            finding(DiagnosticCode::HardBreak, "break", Some(1..3)),
            finding(DiagnosticCode::HardBreak, "unfixable break", None),
            other.clone(),
        ];
        let after = vec![finding(DiagnosticCode::HardBreak, "unfixable break", None), other];
        let result = verified("a  \nb", before, after);
        assert_some!(result[0].fix.as_ref());
        assert_eq!(result[0].hints, ["a hint"], "the hint is untouched");
    }

    #[test]
    fn the_order_of_the_other_findings_is_not_a_difference() {
        let first = finding(DiagnosticCode::MidSentenceBold, "one", None);
        let second = finding(DiagnosticCode::Image, "two", None);
        let before = vec![
            finding(DiagnosticCode::HardBreak, "break", Some(1..3)),
            first.clone(),
            second.clone(),
        ];
        let result = verified("a  \nb", before, vec![second, first]);
        assert_some!(result[0].fix.as_ref());
    }

    #[test]
    fn a_safe_break_keeps_its_edit_after_the_whole_document_is_checked() {
        let parsed = parse("r.md", "# Ada\n\nLondon  \nSeattle\n");
        let found: Vec<_> = parsed
            .diagnostics()
            .iter()
            .filter(|d| d.code == DiagnosticCode::HardBreak)
            .collect();
        assert_eq!(found.len(), 1);
        assert_some!(found[0].fix.as_ref());
        assert!(found[0].hints[0].starts_with("remove the trailing spaces or backslash"));
    }
}

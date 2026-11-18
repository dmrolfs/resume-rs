//! ABOUTME: Property tests for the renderers: hostile text and arbitrary spans never panic, never leak control
//! ABOUTME: bytes, stay bounded, and give the human header and the JSON record the same line and column.

use claims::assert_ok;
use proptest::prelude::*;
use resume::diagnostics::termcolor::Buffer;
use resume::diagnostics::{
    Diagnostic, DiagnosticCode, Edit, Severity, SourceKind, Sources, Span, emit_human, emit_json,
};
use serde::Deserialize;

/// Pieces that stress line splitting, columns, windows, and escaping: every kind of line ending, a BOM
/// anywhere, multi-byte and wide characters, tabs, controls, and bidi overrides.
const PIECES: &[&str] = &[
    "a",
    "bc",
    "é",
    "中",
    "𝄞",
    "·",
    "–",
    " ",
    "\t",
    "\n",
    "\r\n",
    "\r",
    "\u{FEFF}",
    "\u{1b}[31m",
    "\0",
    "\u{85}",
    "\u{202e}",
    "\u{2028}",
    "#",
    "```",
    "<!--",
];

/// Long runs, so lines pass the 120-character window and spans pass the 6-line limit.
const LONG: &[&str] = &[
    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    "中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中中",
    "\n\n\n\n\n\n\n\n\n\n",
];

/// The part of a JSON record this test reads.
#[derive(Debug, Deserialize)]
struct Record {
    file: Option<String>,
    line: Option<usize>,
    column: Option<usize>,
    end_line: Option<usize>,
    end_column: Option<usize>,
}

fn text_strategy() -> impl Strategy<Value = String> {
    let piece = prop_oneof![9 => prop::sample::select(PIECES), 1 => prop::sample::select(LONG)];
    prop::collection::vec(piece, 0..80).prop_map(|pieces| pieces.concat())
}

/// A finding with an arbitrary byte range (possibly reversed, past the end, or inside a character).
fn finding_strategy(max_offset: usize) -> impl Strategy<Value = Diagnostic> {
    let range = (0..max_offset + 8, 0..max_offset + 8);
    (
        prop::option::weighted(0.85, range),
        prop::option::weighted(0.1, Just("missing.md")),
        prop::sample::select(&[Severity::Note, Severity::Warning, Severity::Error][..]),
        prop::collection::vec(prop::sample::select(PIECES), 0..6).prop_map(|p| p.concat()),
        prop::collection::vec(prop::sample::select(PIECES), 0..6).prop_map(|p| p.concat()),
        any::<bool>(),
    )
        .prop_map(|(range, other_file, severity, message, hint, fixable)| Diagnostic {
            severity,
            code: DiagnosticCode::UnsupportedHtml,
            message,
            span: range.map(|(start, end)| Span {
                file: other_file.unwrap_or("t.md").into(),
                kind: SourceKind::Markdown,
                range: start..end,
            }),
            hints: vec![hint],
            fix: fixable.then(|| Edit {
                range: 0..0,
                replacement: String::new(),
            }),
        })
}

fn case_strategy() -> impl Strategy<Value = (String, Vec<Diagnostic>)> {
    text_strategy().prop_flat_map(|text| {
        let longest = text.len();
        (Just(text), prop::collection::vec(finding_strategy(longest), 0..6))
    })
}

fn human(sources: &Sources<'_>, diagnostics: &[Diagnostic]) -> Vec<u8> {
    let mut out = Buffer::no_color();
    assert_ok!(emit_human(&mut out, sources, diagnostics));
    out.into_inner()
}

fn json(sources: &Sources<'_>, diagnostics: &[Diagnostic]) -> Vec<u8> {
    let mut out = Vec::new();
    assert_ok!(emit_json(&mut out, sources, diagnostics));
    out
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    #[test]
    fn render_never_panics_on_arbitrary_text_and_ranges((text, diagnostics) in case_strategy()) {
        let mut sources = Sources::new(None);
        sources.insert("t.md", &text);

        let shown = human(&sources, &diagnostics);
        let lines = json(&sources, &diagnostics);

        prop_assert!(String::from_utf8(shown.clone()).is_ok(), "human output is UTF-8");
        prop_assert!(!shown.contains(&0x1b), "no raw escape reaches the terminal");
        prop_assert!(!shown.contains(&b'\r'), "no raw carriage return");
        prop_assert!(!lines.contains(&b'\r'));
        prop_assert!(lines.is_empty() || lines.ends_with(b"\n"));
        let lines = String::from_utf8(lines).unwrap_or_default();
        prop_assert_eq!(lines.matches('\n').count(), diagnostics.len(), "one line per finding");
        for line in lines.lines() {
            prop_assert!(serde_json::from_str::<Record>(line).is_ok(), "{}", line);
        }
    }

    #[test]
    fn human_header_and_json_agree_for_every_finding((text, diagnostics) in case_strategy()) {
        let mut sources = Sources::new(None);
        sources.insert("t.md", &text);
        for d in &diagnostics {
            let one = std::slice::from_ref(d);
            let shown = String::from_utf8(human(&sources, one)).unwrap_or_default();
            let line = String::from_utf8(json(&sources, one)).unwrap_or_default();
            let record: Record = assert_ok!(serde_json::from_str(line.trim_end()));

            let header = shown.lines().find_map(|l| l.trim_start().strip_prefix("--> ")).map(str::to_string);
            let expected = match (&record.file, record.line, record.column) {
                (Some(file), Some(line), Some(column)) => Some(format!("{file}:{line}:{column}")),
                _ => None,
            };
            prop_assert_eq!(header, expected);

            // A located record is internally consistent.
            if let (Some(l), Some(c), Some(el), Some(ec)) =
                (record.line, record.column, record.end_line, record.end_column)
            {
                prop_assert!(l >= 1 && c >= 1 && el >= l, "{:?}", record);
                prop_assert!(l < el || c <= ec, "the end is not before the start: {:?}", record);
            } else {
                prop_assert!(record.line.is_none() && record.column.is_none());
                prop_assert!(record.end_line.is_none() && record.end_column.is_none());
            }
        }
    }

    #[test]
    fn located_exactly_when_the_span_starts_inside_the_registered_text((text, diagnostics) in case_strategy()) {
        let mut sources = Sources::new(None);
        sources.insert("t.md", &text);
        for d in &diagnostics {
            let line = String::from_utf8(json(&sources, std::slice::from_ref(d))).unwrap_or_default();
            let record: Record = assert_ok!(serde_json::from_str(line.trim_end()));
            let expected = d.span.as_ref().is_some_and(|s| s.file.ends_with("t.md") && s.range.start <= text.len());
            prop_assert_eq!(record.line.is_some(), expected, "{:?} in {:?}", d.span, text);
        }
    }

    #[test]
    fn output_per_finding_is_bounded_whatever_the_text((text, diagnostics) in case_strategy()) {
        let mut sources = Sources::new(None);
        sources.insert("t.md", &text);
        for d in &diagnostics {
            let shown = human(&sources, std::slice::from_ref(d));
            // 6 excerpt lines of at most ~130 columns of up-to-8-byte escapes, plus header and notes.
            prop_assert!(shown.len() < 16 * 1024, "{} bytes", shown.len());
        }
    }
}

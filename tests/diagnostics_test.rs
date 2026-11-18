//! ABOUTME: Whole-pipeline tests for diagnostic rendering: parsed fixtures and hand-built findings in, text out.
//! ABOUTME: Unit tests for the pieces (line index, excerpt, color, summary) live beside the code.

use std::path::{Path, PathBuf};
use std::{env, fs};

use claims::{assert_ok, assert_some};
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use resume::diagnostics::termcolor::Buffer;
use resume::diagnostics::{
    Diagnostic, DiagnosticCode, Edit, Severity, SourceKind, Sources, Span, emit_human, emit_json,
};
use resume::markdown::parse;
use serde::Deserialize;

fn fixtures_dir() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.extend(["tests", "fixtures"]);
    path
}

/// Renders `diagnostics` as plain text (no color), the way a log or a pipe would see it.
fn render(sources: &Sources<'_>, diagnostics: &[Diagnostic]) -> String {
    let mut out = Buffer::no_color();
    assert_ok!(emit_human(&mut out, sources, diagnostics));
    assert_ok!(String::from_utf8(out.into_inner()))
}

/// A finding of `code` over `range` of `t.md`.
fn finding(severity: Severity, message: &str, range: Option<std::ops::Range<usize>>) -> Diagnostic {
    Diagnostic {
        severity,
        code: DiagnosticCode::UnsupportedHtml,
        message: message.into(),
        span: range.map(|range| Span {
            file: "t.md".into(),
            kind: SourceKind::Markdown,
            range,
        }),
        hints: Vec::new(),
        fix: None,
    }
}

fn with_hint(mut d: Diagnostic, hint: &str) -> Diagnostic {
    d.hints.push(hint.into());
    d
}

fn fixable(mut d: Diagnostic) -> Diagnostic {
    d.fix = Some(Edit {
        range: 0..0,
        replacement: String::new(),
    });
    d
}

/// Renders one finding over `needle`'s first occurrence in `text`.
fn render_over(text: &str, needle: &str) -> String {
    let start = assert_some!(text.find(needle), "{needle:?} not in the text");
    let mut sources = Sources::new(None);
    sources.insert("t.md", text);
    let d = finding(Severity::Error, "problem", Some(start..start + needle.len()));
    render(&sources, &[d])
}

#[test]
fn human_error_shows_excerpt_and_hint() {
    let name = "errors-role.md";
    let path = fixtures_dir().join(name);
    let text = assert_ok!(fs::read_to_string(&path));
    let parsed = parse(&path, &text);
    let found = assert_some!(
        parsed
            .diagnostics()
            .iter()
            .find(|d| d.code == DiagnosticCode::RoleSeparator && d.message.ends_with("not `|`")),
        "the `|` separator finding"
    );
    let mut sources = Sources::new(Some(fixtures_dir()));
    sources.insert(path, &text);

    let shown = render(&sources, std::slice::from_ref(found));

    assert!(shown.starts_with("error[role-separator]: "), "{shown}");
    assert!(shown.contains("--> errors-role.md:12:5"), "file:line:column\n{shown}");
    assert!(
        shown.contains("12 | ### Acme | Engineer | 2015 – 2017"),
        "excerpt\n{shown}"
    );
    assert!(shown.contains("^ fixable"), "caret and marker\n{shown}");
    assert!(
        shown.contains("= help: did you mean `### Acme · Engineer · 2015 – 2017`?"),
        "hint\n{shown}"
    );
    assert_snapshot!("human_error_shows_excerpt_and_hint", shown);
}

#[test]
fn a_finding_without_a_span_prints_its_header_and_notes_only() {
    let sources = Sources::new(None);
    let d = fixable(with_hint(
        finding(Severity::Warning, "the resume has no name", None),
        "add a `# Name` line",
    ));
    assert_snapshot!("no_span", render(&sources, &[d]));
}

#[test]
fn a_zero_width_span_at_the_start_still_gets_a_caret() {
    assert_snapshot!("zero_width_at_start", {
        let mut sources = Sources::new(None);
        sources.insert("t.md", "First line\nsecond\n");
        let d = finding(Severity::Error, "missing name", Some(0..0));
        render(&sources, &[d])
    });
}

#[test]
fn a_zero_width_span_at_the_end_of_the_file_gets_a_caret() {
    let mut sources = Sources::new(None);
    sources.insert("t.md", "abc");
    let shown = render(&sources, &[finding(Severity::Error, "unexpected end", Some(3..3))]);
    assert!(shown.contains("--> t.md:1:4"), "{shown}");
    assert_snapshot!("zero_width_at_end", shown);
}

#[test]
fn a_multi_line_span_is_drawn_across_its_lines() {
    let text = "# Ada\n\n```\nline a\nline b\n```\n\nafter\n";
    let start = 7;
    let end = text.find("after").unwrap_or(0) - 2;
    let mut sources = Sources::new(None);
    sources.insert("t.md", text);
    let d = finding(Severity::Error, "code blocks are not supported", Some(start..end));
    assert_snapshot!("multi_line", render(&sources, &[d]));
}

#[test]
fn a_tall_span_is_clipped_and_says_so() {
    let text = (1..=20).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
    let mut sources = Sources::new(None);
    sources.insert("t.md", &text);
    let d = finding(Severity::Error, "block is too tall", Some(0..text.len()));
    let shown = render(&sources, &[d]);
    assert!(
        shown.contains("= note: excerpt shortened; the span runs from 1:1 to 20:8"),
        "{shown}"
    );
    assert!(!shown.contains("line 7"), "{shown}");
    assert_snapshot!("tall_span", shown);
}

#[test]
fn a_long_line_is_windowed_around_the_finding() {
    let text = format!("{}TARGET{}", "x".repeat(300), "y".repeat(300));
    let shown = render_over(&text, "TARGET");
    assert!(shown.contains("--> t.md:1:301"), "the true column\n{shown}");
    assert!(shown.len() < 1000, "{} bytes", shown.len());
    assert_snapshot!("long_line", shown);
}

#[test]
fn wide_characters_and_tabs_keep_the_caret_under_the_span() {
    let text = "a\t中文 x · target tail\n";
    assert_snapshot!("wide_and_tab", render_over(text, "target"));
}

#[test]
fn an_invisible_span_still_gets_a_caret_and_a_note() {
    let text = "Jane\u{200b}Doe\n";
    let mut sources = Sources::new(None);
    sources.insert("t.md", text);
    let d = fixable(finding(
        Severity::Warning,
        "a zero-width space is in the name",
        Some(4..7),
    ));
    let shown = render(&sources, &[d]);
    assert!(shown.contains("--> t.md:1:5"), "the true column\n{shown}");
    assert!(shown.contains("^ fixable"), "a caret and the marker\n{shown}");
    assert!(shown.contains("= note: the span has no visible width"), "{shown}");
    // The zero-width space is shown as `<ZWSP>` only so the snapshot is readable; in the real output it
    // has no width, which is why the caret sits under the `D` after it.
    assert_snapshot!("invisible_span", shown.replace('\u{200b}', "<ZWSP>"));
}

#[test]
fn a_finding_whose_source_is_missing_names_the_file_and_says_why() {
    let sources = Sources::new(None);
    let d = finding(Severity::Error, "problem", Some(0..4));
    let shown = render(&sources, &[d]);
    assert!(
        shown.contains("= note: in `t.md`; no excerpt: source not available"),
        "{shown}"
    );
    assert_snapshot!("missing_source", shown);
}

#[test]
fn a_span_past_the_end_is_unlocated_rather_than_misplaced() {
    let mut sources = Sources::new(None);
    sources.insert("t.md", "abc");
    let shown = render(&sources, &[finding(Severity::Error, "problem", Some(50..60))]);
    assert!(shown.contains("span 50..60 is past the end (3 bytes)"), "{shown}");
    assert!(!shown.contains("-->"), "no false location\n{shown}");
}

#[test]
fn never_and_no_color_emit_no_escape_bytes() {
    // The source itself carries an escape sequence: a terminal must never see it raw.
    let text = "# Ada\n\u{1b}[31mred\u{1b}[0m and a lone\rcarriage return\n";
    let start = assert_some!(text.find('\u{1b}'));
    let mut sources = Sources::new(None);
    sources.insert("t.md", text);
    let d = with_hint(
        finding(Severity::Error, "bad \u{1b}[1mbold\u{1b}[0m", Some(start..start + 9)),
        "hint \u{1b}[2J",
    );
    let mut out = Buffer::no_color();
    assert_ok!(emit_human(&mut out, &sources, [&d]));
    let bytes = out.into_inner();
    assert!(!bytes.contains(&0x1b), "{}", String::from_utf8_lossy(&bytes));
    assert!(!bytes.contains(&b'\r'), "{}", String::from_utf8_lossy(&bytes));
    let shown = String::from_utf8_lossy(&bytes);
    assert!(shown.contains("\\u{1b}[31mred"), "{shown}");
}

#[test]
fn always_emits_ansi_but_still_neutralizes_the_source() {
    let text = "a\u{1b}[31mb\n";
    let mut sources = Sources::new(None);
    sources.insert("t.md", text);
    let d = finding(Severity::Error, "problem", Some(0..text.len() - 1));
    let mut out = Buffer::ansi();
    assert_ok!(emit_human(&mut out, &sources, [&d]));
    let shown = String::from_utf8_lossy(out.as_slice()).into_owned();
    assert!(shown.contains("\u{1b}["), "styled output has escape sequences");
    assert!(
        shown.contains("\\u{1b}[31mb"),
        "the source's escape is shown, not sent\n{shown}"
    );
}

#[test]
fn bom_does_not_shift_columns() {
    let plain = "# Ada\n";
    let with_bom = "\u{FEFF}# Ada\n";
    let (mut a, mut b) = (Sources::new(None), Sources::new(None));
    a.insert("t.md", plain);
    b.insert("t.md", with_bom);
    let over_ada = |offset: usize| finding(Severity::Error, "problem", Some(offset + 2..offset + 5));

    let (without, with) = (render(&a, &[over_ada(0)]), render(&b, &[over_ada(3)]));

    assert!(without.contains("--> t.md:1:3"), "{without}");
    assert_eq!(with, without);
}

#[test]
fn a_span_that_starts_inside_the_bom_does_not_panic_or_misplace() {
    let mut sources = Sources::new(None);
    sources.insert("t.md", "\u{FEFF}# Ada\n");
    let shown = render(&sources, &[finding(Severity::Error, "problem", Some(0..5))]);
    assert!(shown.contains("--> t.md:1:1"), "{shown}");
}

#[test]
fn lf_crlf_and_cr_render_identical_excerpts() {
    let document =
        |ending: &str| format!("# Ada{ending}{ending}Seattle · ada@example.com{ending}## Experience{ending}");
    let render_ending = |ending: &str| {
        let text = document(ending);
        render_over(&text, "ada@example.com")
    };
    let lf = render_ending("\n");
    assert!(lf.contains("--> t.md:3:11"), "{lf}");
    assert_eq!(render_ending("\r\n"), lf);
    assert_eq!(render_ending("\r"), lf);
}

#[test]
fn several_findings_render_in_the_order_given() {
    let mut sources = Sources::new(None);
    sources.insert("t.md", "one\ntwo\nthree\n");
    let diagnostics = [
        finding(Severity::Warning, "second line", Some(4..7)),
        finding(Severity::Error, "first line", Some(0..3)),
    ];
    let shown = render(&sources, &diagnostics);
    let (second, first) = (
        assert_some!(shown.find("second line")),
        assert_some!(shown.find("first line")),
    );
    assert!(second < first, "{shown}");
    assert_eq!(shown.matches("-->").count(), 2);
}

#[test]
fn the_frame_is_ascii_for_ascii_source() {
    let shown = render_over("# Ada\nplain text here\n", "plain");
    assert!(shown.is_ascii(), "{shown}");
}

#[test]
fn rendering_a_fixture_never_needs_the_file_system() {
    // Sources hold the text; a path that does not exist on disk is fine.
    let path = Path::new("no-such-dir").join("resume.md");
    let text = "# Ada\n\n<div>x</div>\n";
    let parsed = parse(&path, text);
    let mut sources = Sources::new(None);
    sources.insert(path, text);
    assert!(!render(&sources, parsed.diagnostics()).is_empty());
}

/// The documented JSON record, as a consumer would declare it. `deny_unknown_fields` makes any key the
/// documentation does not mention a test failure.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    #[serde(rename = "type")]
    kind: String,
    severity: String,
    code: String,
    category: String,
    message: String,
    file: Option<String>,
    line: Option<usize>,
    column: Option<usize>,
    end_line: Option<usize>,
    end_column: Option<usize>,
    hints: Vec<String>,
    fixable: bool,
}

/// Every key of a record, in documented order.
const RECORD_KEYS: [&str; 12] = [
    "type",
    "severity",
    "code",
    "category",
    "message",
    "file",
    "line",
    "column",
    "end_line",
    "end_column",
    "hints",
    "fixable",
];

fn emit_json_bytes(sources: &Sources<'_>, diagnostics: &[Diagnostic]) -> Vec<u8> {
    let mut out = Vec::new();
    assert_ok!(emit_json(&mut out, sources, diagnostics));
    out
}

fn records(bytes: &[u8]) -> Vec<Record> {
    let text = assert_ok!(std::str::from_utf8(bytes));
    text.lines()
        .map(|line| assert_ok!(serde_json::from_str(line), "{line}"))
        .collect()
}

/// Parses a fixture and registers its text, the way the CLI will.
fn parsed_fixture(name: &str) -> (String, Vec<Diagnostic>, PathBuf) {
    let path = fixtures_dir().join(name);
    let text = assert_ok!(fs::read_to_string(&path));
    let diagnostics = parse(&path, &text).diagnostics().to_vec();
    (text, diagnostics, path)
}

#[test]
fn json_lines_one_object_per_diagnostic() {
    let (text, diagnostics, path) = parsed_fixture("errors-role.md");
    let mut sources = Sources::new(Some(fixtures_dir()));
    sources.insert(path, &text);

    let bytes = emit_json_bytes(&sources, &diagnostics);

    assert!(diagnostics.len() > 10, "the fixture should have many findings");
    assert_eq!(bytes.last(), Some(&b'\n'), "every record ends with a newline");
    assert!(!bytes.contains(&b'\r'), "records are separated by LF alone");
    let text_out = assert_ok!(String::from_utf8(bytes.clone()));
    assert_eq!(
        text_out.matches('\n').count(),
        diagnostics.len(),
        "one line per finding, no blank lines"
    );
    for line in text_out.lines() {
        assert!(line.starts_with(r#"{"type":"diagnostic","severity":""#), "{line}");
        let value: serde_json::Value = assert_ok!(serde_json::from_str(line));
        let mut keys: Vec<&str> = assert_some!(value.as_object()).keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected = RECORD_KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected, "every key is always present\n{line}");
    }

    let parsed = records(&bytes);
    assert_eq!(parsed.len(), diagnostics.len());
    for (record, d) in parsed.iter().zip(&diagnostics) {
        assert_eq!(record.kind, "diagnostic");
        assert_eq!(record.severity, d.severity.to_string());
        assert_eq!(record.code, d.code.to_string());
        assert_eq!(record.category, d.code.category().to_string());
        assert_eq!(record.message, d.message);
        assert_eq!(record.hints, d.hints);
        assert_eq!(record.fixable, d.fix.is_some());
        assert_eq!(record.file.as_deref(), Some("errors-role.md"));
    }
}

#[test]
fn json_line_and_column_match_an_independent_count() {
    let (text, diagnostics, path) = parsed_fixture("warnings-fixable.md");
    let mut sources = Sources::new(Some(fixtures_dir()));
    sources.insert(path, &text);

    let parsed = records(&emit_json_bytes(&sources, &diagnostics));

    assert!(!parsed.is_empty());
    for (record, d) in parsed.iter().zip(&diagnostics) {
        let span = assert_some!(d.span.as_ref());
        let before = &text[..span.range.start];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = text[line_start..span.range.start].chars().count() + 1;
        assert_eq!(
            (record.line, record.column),
            (Some(line), Some(column)),
            "{}",
            d.message
        );
        let end_before = &text[..span.range.end];
        assert_eq!(record.end_line, Some(end_before.matches('\n').count() + 1));
    }
}

#[test]
fn json_unknown_location_is_null() {
    let sources = Sources::new(None);
    let diagnostics = [
        finding(Severity::Error, "no span", None),
        finding(Severity::Error, "span but no source", Some(0..4)),
    ];

    let parsed = records(&emit_json_bytes(&sources, &diagnostics));

    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].file, None);
    assert_eq!(parsed[1].file.as_deref(), Some("t.md"));
    for record in &parsed {
        assert_eq!(
            (record.line, record.column, record.end_line, record.end_column),
            (None, None, None, None)
        );
    }
}

#[test]
fn json_with_a_hostile_source_stays_one_clean_line_per_finding() {
    let text = "# Ada\r\n\u{1b}[31mred\u{0}\u{2028} \u{202e}\r\n";
    let mut sources = Sources::new(None);
    sources.insert("t.md", text);
    let d = finding(Severity::Error, "bad \u{1b}[1m\r\nbold", Some(7..12));

    let bytes = emit_json_bytes(&sources, std::slice::from_ref(&d));

    assert_eq!(bytes.iter().filter(|&&b| b == b'\n').count(), 1);
    assert!(!bytes.contains(&0x1b) && !bytes.contains(&b'\r'));
    let parsed = records(&bytes);
    assert_eq!(parsed[0].message, d.message, "the message round-trips exactly");
    assert_eq!(parsed[0].line, Some(2));
}

#[test]
fn human_header_and_json_agree_on_line_and_column() {
    let mut checked = 0;
    for entry in assert_ok!(fs::read_dir(fixtures_dir())) {
        let path = assert_ok!(entry).path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let text = assert_ok!(fs::read_to_string(&path));
        let diagnostics = parse(&path, &text).diagnostics().to_vec();
        let mut sources = Sources::new(Some(fixtures_dir()));
        sources.insert(path.clone(), &text);
        for d in &diagnostics {
            let human = render(&sources, std::slice::from_ref(d));
            let json = records(&emit_json_bytes(&sources, std::slice::from_ref(d)));
            let (line, column) = (json[0].line, json[0].column);
            let header = human.lines().find_map(|l| l.trim_start().strip_prefix("--> "));
            let expected = match (json[0].file.as_deref(), line, column) {
                (Some(file), Some(line), Some(column)) => Some(format!("{file}:{line}:{column}")),
                _ => None,
            };
            assert_eq!(
                header.map(str::to_string),
                expected,
                "{}: {}\n{human}",
                path.display(),
                d.message
            );
            checked += 1;
        }
    }
    assert!(checked > 30, "only {checked} findings were compared");
}

/// Renders both formats for `diagnostics` over `text`; returns the byte counts and the time taken.
fn render_both(text: &str, diagnostics: &[Diagnostic]) -> (usize, usize, std::time::Duration) {
    let mut sources = Sources::new(None);
    sources.insert("t.md", text);
    let started = std::time::Instant::now();
    let human = render(&sources, diagnostics);
    let json = emit_json_bytes(&sources, diagnostics);
    (human.len(), json.len(), started.elapsed())
}

#[test]
fn tall_and_long_spans_output_is_bounded() {
    // 500 findings over a single 400 KiB line. Printed whole, each would repeat the line (about 400 MB).
    let long_line = "x".repeat(400 * 1024);
    let over_line: Vec<_> = (0..500)
        .map(|i| finding(Severity::Error, "problem", Some(i * 800..i * 800 + 300_000)))
        .collect();
    let (human, json, took) = render_both(&long_line, &over_line);
    assert!(human < 1 << 20, "human output was {human} bytes");
    assert!(json < 1 << 20, "json output was {json} bytes");
    assert!(took < std::time::Duration::from_secs(10), "{took:?}");

    // 500 findings over a 1 MiB block of one-byte lines, each spanning from its start to the end.
    let many_lines = "a\n".repeat(512 * 1024);
    let over_block: Vec<_> = (0..500)
        .map(|i| finding(Severity::Error, "problem", Some(i * 1000..many_lines.len())))
        .collect();
    let (human, json, took) = render_both(&many_lines, &over_block);
    assert!(human < 4 << 20, "human output was {human} bytes");
    assert!(json < 1 << 20, "json output was {json} bytes");
    assert!(took < std::time::Duration::from_secs(10), "{took:?}");
}

#[test]
fn every_fixture_renders_in_both_formats() {
    let mut rendered = 0;
    let mut names: Vec<_> = assert_ok!(fs::read_dir(fixtures_dir()))
        .map(|entry| assert_ok!(entry).path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    names.sort();
    for path in names {
        let text = assert_ok!(fs::read_to_string(&path));
        let diagnostics = parse(&path, &text).diagnostics().to_vec();
        let mut sources = Sources::new(Some(fixtures_dir()));
        sources.insert(path.clone(), &text);

        let shown = render(&sources, &diagnostics);
        let json = emit_json_bytes(&sources, &diagnostics);

        assert_eq!(records(&json).len(), diagnostics.len(), "{}", path.display());
        let headers = shown.lines().filter(|l| l.trim_start().starts_with("--> ")).count();
        assert_eq!(headers, diagnostics.len(), "{}", path.display());
        let summary = resume::diagnostics::Summary::of(&diagnostics);
        let name = assert_some!(path.file_name()).to_string_lossy().into_owned();
        insta::with_settings!({ snapshot_suffix => name }, {
            assert_snapshot!("human", format!("{shown}{summary}\n"));
        });
        rendered += 1;
    }
    assert!(rendered >= 15, "only {rendered} fixtures were rendered");
}

#[test]
fn color_never_and_no_color_emit_no_ansi_end_to_end() {
    use resume::diagnostics::termcolor::ColorChoice;
    use resume::diagnostics::{ColorEnv, Summary, resolve_color};
    use resume::model::options::ColorMode;
    use std::ffi::OsStr;

    let xterm = Some(OsStr::new("xterm-256color"));
    let terminal = ColorEnv::from_vars(true, None, xterm);
    let cases = [
        (ColorMode::Never, terminal, false),
        (
            ColorMode::Auto,
            ColorEnv::from_vars(true, Some(OsStr::new("1")), xterm),
            false,
        ),
        (ColorMode::Auto, ColorEnv::from_vars(false, None, xterm), false),
        (
            ColorMode::Auto,
            ColorEnv::from_vars(true, None, Some(OsStr::new("dumb"))),
            false,
        ),
        (ColorMode::Auto, terminal, true),
        (
            ColorMode::Auto,
            ColorEnv::from_vars(true, Some(OsStr::new("")), xterm),
            true,
        ),
        (
            ColorMode::Always,
            ColorEnv::from_vars(false, Some(OsStr::new("1")), None),
            true,
        ),
    ];
    let mut sources = Sources::new(None);
    sources.insert("t.md", "# Ada\n");
    let d = fixable(with_hint(finding(Severity::Error, "problem", Some(2..5)), "a hint"));

    for (mode, env, expect_ansi) in cases {
        let mut out = match resolve_color(mode, env) {
            ColorChoice::Always => Buffer::ansi(),
            _ => Buffer::no_color(),
        };
        assert_ok!(emit_human(&mut out, &sources, [&d]));
        assert_eq!(out.as_slice().contains(&0x1b), expect_ansi, "{mode} with {env:?}");
    }

    let summary = Summary::of([&d]).to_string();
    assert_eq!(summary, "1 error, 0 warnings; 1 fixable");
    assert!(summary.is_ascii());
}

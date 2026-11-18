//! ABOUTME: Whole-document tests for the Markdown parser: fixtures, snapshots, line endings, and properties.
//! ABOUTME: Per-convention and per-code unit tests live beside the code in `src/markdown`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use claims::{assert_none, assert_ok, assert_some};
use insta::assert_json_snapshot;
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use resume::diagnostics::{Category, Diagnostic, DiagnosticCode, Edit, Severity};
use resume::markdown::{MAX_INPUT_BYTES, Parsed, parse};
use serde::Serialize;
use strum::IntoEnumIterator;

mod common;
use common::{apply_edits, edits_of, subject_matches};

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.extend(["tests", "fixtures", name]);
    path
}

fn read_fixture(name: &str) -> String {
    assert_ok!(fs::read_to_string(fixture_path(name)), "{name}")
}

fn parse_fixture(name: &str) -> (String, Parsed) {
    let text = read_fixture(name);
    let parsed = parse(fixture_path(name), &text);
    (text, parsed)
}

/// Fixtures that must produce findings, in a stable order.
const FINDING_FIXTURES: [&str; 14] = [
    "errors-missing-name.md",
    "errors-duplicate-name.md",
    "errors-content-before-section.md",
    "errors-role.md",
    "errors-era.md",
    "errors-period.md",
    "errors-html.md",
    "errors-unclosed-comment.md",
    "errors-links.md",
    "errors-length.md",
    "errors-unsupported.md",
    "warnings-format.md",
    "warnings-missing-contact.md",
    "warnings-fixable.md",
];

/// A serializable view of a diagnostic, with the source it points at quoted for review.
#[derive(Debug, Serialize)]
struct View {
    code: String,
    severity: String,
    range: [usize; 2],
    source: String,
    message: String,
    hints: Vec<String>,
    fix: Option<FixView>,
}

#[derive(Debug, Serialize)]
struct FixView {
    range: [usize; 2],
    replaces: String,
    with: String,
}

fn views(text: &str, diagnostics: &[Diagnostic]) -> Vec<View> {
    diagnostics
        .iter()
        .map(|d| {
            let range = assert_some!(d.span.as_ref(), "{}: every finding is located", d.code)
                .range
                .clone();
            View {
                code: d.code.to_string(),
                severity: d.severity.to_string(),
                range: [range.start, range.end],
                source: text[range.clone()].to_string(),
                message: d.message.clone(),
                hints: d.hints.clone(),
                fix: d.fix.as_ref().map(|f| FixView {
                    range: [f.range.start, f.range.end],
                    replaces: text[f.range.clone()].to_string(),
                    with: f.replacement.clone(),
                }),
            }
        })
        .collect()
}

fn to_crlf(text: &str) -> String {
    text.replace('\n', "\r\n")
}

// ---------------------------------------------------------------------------------------------
// Fixtures and snapshots
// ---------------------------------------------------------------------------------------------

#[test]
fn full_fixture_has_no_diagnostics() {
    let (text, parsed) = parse_fixture("full.md");
    assert_eq!(
        views(&text, parsed.diagnostics()).len(),
        0,
        "{:#?}",
        parsed.diagnostics()
    );
    assert_some!(parsed.valid());
}

#[test]
fn full_fixture_snapshot() {
    let (_, parsed) = parse_fixture("full.md");
    assert_json_snapshot!(assert_some!(parsed.valid()));
}

#[test]
fn finding_fixtures_snapshots() {
    for name in FINDING_FIXTURES {
        let (text, parsed) = parse_fixture(name);
        assert!(!parsed.diagnostics().is_empty(), "{name} must produce findings");
        insta::with_settings!({snapshot_suffix => name}, {
            assert_json_snapshot!("diagnostics", views(&text, parsed.diagnostics()));
        });
    }
}

#[test]
fn error_fixtures_block_the_build_and_warning_fixtures_do_not() {
    for name in FINDING_FIXTURES {
        let (_, parsed) = parse_fixture(name);
        if name.starts_with("errors-") {
            assert!(parsed.has_errors(), "{name}");
            assert_none!(parsed.valid(), "{name}");
        } else {
            assert!(!parsed.has_errors(), "{name}: {:#?}", parsed.diagnostics());
            assert_some!(parsed.valid(), "{name}");
        }
    }
}

#[test]
fn every_finding_is_located_hinted_and_in_bounds() {
    for name in FINDING_FIXTURES {
        let (text, parsed) = parse_fixture(name);
        for d in parsed.diagnostics() {
            let span = assert_some!(d.span.as_ref(), "{name}: {}", d.code);
            assert!(
                span.range.start <= span.range.end && span.range.end <= text.len(),
                "{name}: {}",
                d.code
            );
            assert!(text.is_char_boundary(span.range.start) && text.is_char_boundary(span.range.end));
            assert_eq!(span.file, fixture_path(name));
            assert!(!d.hints.is_empty(), "{name}: {} has no hint", d.code);
        }
    }
}

#[test]
fn every_finding_quotes_what_its_code_names() {
    for name in FINDING_FIXTURES {
        let (text, parsed) = parse_fixture(name);
        for d in parsed.diagnostics() {
            let range = assert_some!(d.span.as_ref()).range.clone();
            assert!(
                subject_matches(d.code, &text[range.clone()]),
                "{name}: {} quotes {:?}",
                d.code,
                &text[range]
            );
        }
    }
}

#[test]
fn every_format_code_is_demonstrated_by_a_fixture() {
    let mut seen: Vec<DiagnosticCode> = FINDING_FIXTURES
        .iter()
        .flat_map(|name| {
            parse_fixture(name)
                .1
                .diagnostics()
                .iter()
                .map(|d| d.code)
                .collect::<Vec<_>>()
        })
        .collect();
    seen.push(DiagnosticCode::InputLimit); // the size and nesting caps are exercised below
    for code in DiagnosticCode::iter().filter(|c| c.category() == Category::Format) {
        assert!(seen.contains(&code), "no fixture produces `{code}`");
    }
}

#[test]
fn warnings_fixable_edits_apply_cleanly() {
    let (text, parsed) = parse_fixture("warnings-fixable.md");
    let edits = edits_of(parsed.diagnostics());
    assert!(edits.len() >= 8, "every finding in this fixture is mechanical");
    assert_eq!(parsed.diagnostics().iter().filter(|d| d.fix.is_none()).count(), 0);
    let fixed = apply_edits(&text, &edits);
    assert_eq!(parse("fixed.md", &fixed).diagnostics(), &[], "{fixed}");
    insta::assert_snapshot!("warnings_fixable_after_edits", fixed);
}

// ---------------------------------------------------------------------------------------------
// Line endings and the BOM (Windows portability)
// ---------------------------------------------------------------------------------------------

fn all_fixtures() -> Vec<&'static str> {
    let mut names = vec!["full.md"];
    names.extend(FINDING_FIXTURES);
    names
}

#[test]
fn lf_and_crlf_inputs_parse_to_equal_models() {
    for name in all_fixtures() {
        let lf = read_fixture(name);
        let (a, b) = (parse("a.md", &lf), parse("b.md", &to_crlf(&lf)));
        assert_eq!(a.best_effort(), b.best_effort(), "{name}");
        let codes = |p: &Parsed| {
            p.diagnostics()
                .iter()
                .map(|d| (d.code, d.message.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(codes(&a), codes(&b), "{name}");
    }
}

#[test]
fn crlf_spans_point_into_original() {
    for name in all_fixtures() {
        let lf = read_fixture(name);
        let crlf = to_crlf(&lf);
        let (a, b) = (parse("a.md", &lf), parse("b.md", &crlf));
        assert_eq!(a.diagnostics().len(), b.diagnostics().len(), "{name}");
        for (da, db) in a.diagnostics().iter().zip(b.diagnostics()) {
            let (sa, sb) = (assert_some!(da.span.as_ref()), assert_some!(db.span.as_ref()));
            let slice = |text: &str, r: &std::ops::Range<usize>| text[r.clone()].replace("\r\n", "\n");
            assert_eq!(slice(&crlf, &sb.range), slice(&lf, &sa.range), "{name}: {}", da.code);
            assert!(crlf.is_char_boundary(sb.range.start) && crlf.is_char_boundary(sb.range.end));
            assert!(
                !crlf[..sb.range.end].ends_with('\r'),
                "{name}: {} ends between CR and LF",
                da.code
            );
            assert!(
                !crlf[sb.range.start..].starts_with('\n') || sb.range.is_empty(),
                "{name}: {}",
                da.code
            );
        }
        let fixed_lf = apply_edits(&lf, &edits_of(a.diagnostics()));
        let fixed_crlf = apply_edits(&crlf, &edits_of(b.diagnostics()));
        assert_eq!(
            fixed_crlf.replace("\r\n", "\n"),
            fixed_lf,
            "{name}: edits must agree across line endings"
        );
    }
}

#[test]
fn mixed_line_endings_parse_like_lf() {
    for name in all_fixtures() {
        let lf = read_fixture(name);
        let mixed: String = lf
            .split_inclusive('\n')
            .enumerate()
            .map(|(i, line)| {
                if i % 2 == 0 {
                    line.replace('\n', "\r\n")
                } else {
                    line.to_string()
                }
            })
            .collect();
        assert_eq!(
            parse("a.md", &lf).best_effort(),
            parse("b.md", &mixed).best_effort(),
            "{name}"
        );
    }
}

#[test]
fn lone_cr_line_endings_parse_like_lf() {
    let lf = read_fixture("full.md");
    let cr = lf.replace('\n', "\r");
    assert_eq!(parse("a.md", &lf).best_effort(), parse("b.md", &cr).best_effort());
}

#[test]
fn final_line_without_newline_parses() {
    let lf = read_fixture("full.md");
    let trimmed = lf.trim_end_matches('\n');
    assert_eq!(parse("a.md", &lf).best_effort(), parse("b.md", trimmed).best_effort());
    let no_eol = "# Ada\n\nHere\n\n## S\n\n### Acme · 2015 - 2017";
    let parsed = parse("c.md", no_eol);
    let fix = assert_some!(parsed.diagnostics()[0].fix.as_ref());
    assert_eq!(
        apply_edits(no_eol, &[fix]),
        "# Ada\n\nHere\n\n## S\n\n### Acme · 2015 – 2017"
    );
}

#[test]
fn bom_is_ignored_for_the_model_and_spans_shift() {
    for name in ["full.md", "warnings-fixable.md", "errors-role.md"] {
        let plain = read_fixture(name);
        let with_bom = format!("\u{FEFF}{plain}");
        let (a, b) = (parse("a.md", &plain), parse("b.md", &with_bom));
        assert_eq!(a.best_effort(), b.best_effort(), "{name}");
        for (da, db) in a.diagnostics().iter().zip(b.diagnostics()) {
            let (sa, sb) = (assert_some!(da.span.as_ref()), assert_some!(db.span.as_ref()));
            assert_eq!(&with_bom[sb.range.clone()], &plain[sa.range.clone()], "{name}");
            assert!(sb.range.start >= 3, "{name}: no span may include the BOM");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

const TOKENS: &[&str] = &[
    ":",
    "Label",
    "**Label**:",
    "<!-->",
    "<!--->",
    "###Acme",
    "\\·",
    "&ndash;",
    "Aug",
    "Spring",
    "![img](a.png)",
    "\u{ff65}",
    " – ",
    "2015 – 2017",
    "\n- ",
    "\n### ",
    "Present – 2019",
    "# ",
    "## ",
    "### ",
    "#### ",
    " · ",
    "·",
    " - ",
    "-",
    "–",
    "—",
    "--",
    "|",
    "•",
    "**",
    "*",
    "_",
    "`",
    "[",
    "](",
    ")",
    "https://x.y",
    "ftp://a",
    "<!--",
    "-->",
    "<div class=\"page-break\"></div>",
    "<b>",
    "![a](b)",
    "\n",
    "\r\n",
    "\r",
    "\n\n",
    "    ",
    "> ",
    "1. ",
    "- ",
    "2020",
    "Aug 2017",
    "Present",
    "Current",
    "Spring",
    "1949",
    "2101",
    "é",
    "日本",
    "👩‍💻",
    "e\u{301}",
    "\u{2028}",
    "\u{FEFF}",
    "\u{A0}",
    "\t",
    "\\",
    "&amp;",
    "&",
    "'",
    "\"",
    "...",
    "~",
    "$",
    "|---|",
    "[^1]",
    "word",
    "Acme",
    "Engineer",
    " ",
    "  ",
];

fn structured_markdown() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(TOKENS), 0..200).prop_map(|tokens| tokens.concat())
}

fn check_invariants(text: &str, parsed: &Parsed) -> Result<(), TestCaseError> {
    let mut edits: Vec<&Edit> = Vec::new();
    for d in parsed.diagnostics() {
        let Some(span) = d.span.as_ref() else {
            return Err(TestCaseError::fail(format!("{} has no span", d.code)));
        };
        prop_assert!(
            span.range.start <= span.range.end && span.range.end <= text.len(),
            "{}: {:?}",
            d.code,
            span.range
        );
        prop_assert!(
            text.is_char_boundary(span.range.start) && text.is_char_boundary(span.range.end),
            "{}",
            d.code
        );
        prop_assert!(!d.hints.is_empty(), "{} has no hint", d.code);
        prop_assert!(
            subject_matches(d.code, &text[span.range.clone()]),
            "{} quotes {:?}",
            d.code,
            &text[span.range.clone()]
        );
        if let Some(edit) = &d.fix {
            prop_assert!(edit.range.end <= text.len(), "{}: {:?}", d.code, edit.range);
            prop_assert!(text.is_char_boundary(edit.range.start) && text.is_char_boundary(edit.range.end));
            edits.push(edit);
        }
    }
    edits.sort_by_key(|e| e.range.start);
    for pair in edits.windows(2) {
        prop_assert!(
            pair[0].range.end <= pair[1].range.start,
            "edits overlap: {:?} and {:?}",
            pair[0],
            pair[1]
        );
    }
    Ok(())
}

proptest! {
    #[test]
    fn parse_never_panics_on_arbitrary_text(text in any::<String>()) {
        let parsed = parse("r.md", &text);
        check_invariants(&text, &parsed)?;
    }

    #[test]
    fn parse_never_panics_on_structured_markdown(text in structured_markdown()) {
        let parsed = parse("r.md", &text);
        check_invariants(&text, &parsed)?;
    }

    #[test]
    fn spans_are_in_bounds(text in structured_markdown()) {
        let parsed = parse("r.md", &text);
        check_invariants(&text, &parsed)?;
        let crlf = to_crlf(&text.replace("\r\n", "\n").replace('\r', "\n"));
        check_invariants(&crlf, &parse("r.md", &crlf))?;
    }

    #[test]
    fn resume_is_present_unless_errors(text in structured_markdown()) {
        let parsed = parse("r.md", &text);
        if !parsed.has_errors() {
            prop_assert!(parsed.best_effort().is_some());
        }
        if parsed.best_effort().is_none() {
            prop_assert!(parsed.has_errors());
        }
        prop_assert_eq!(parsed.valid().is_some(), parsed.best_effort().is_some() && !parsed.has_errors());
    }

    #[test]
    fn applying_edits_clears_fixable_findings(text in structured_markdown()) {
        let parsed = parse("r.md", &text);
        let fixed = apply_edits(&text, &edits_of(parsed.diagnostics()));
        let again = parse("r.md", &fixed);
        prop_assert!(
            again.diagnostics().iter().all(|d| d.fix.is_none()),
            "fixable findings remain after applying edits\nbefore: {:?}\nafter: {:?}",
            text,
            fixed
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Pathological input
// ---------------------------------------------------------------------------------------------

/// Runs `work` on a thread with a deliberately small stack, so any recursion that grows with the
/// input overflows here instead of in production.
fn on_small_stack(name: &str, work: impl FnOnce() + Send + 'static) {
    let handle = assert_ok!(
        thread::Builder::new()
            .name(name.into())
            .stack_size(256 * 1024)
            .spawn(work)
    );
    assert_ok!(
        handle
            .join()
            .map_err(|_| format!("{name} panicked or overflowed its stack"))
    );
}

/// `depth` genuinely nested emphasis spans, alternating `*` and `_` so each one opens a new level.
fn nested_emphasis(depth: usize) -> String {
    let delimiter = |i: usize| if i.is_multiple_of(2) { "*" } else { "_" };
    let opens: String = (0..depth).map(|i| format!("{}w ", delimiter(i))).collect();
    let closes: String = (0..depth).rev().map(|i| format!(" w{}", delimiter(i))).collect();
    format!("{opens}x{closes}")
}

fn pathological_inputs() -> Vec<(&'static str, String)> {
    let line = |s: &str| format!("# Ada\n\nHere\n\n## S\n\n{s}\n");
    vec![
        ("deep block quotes", line(&">".repeat(20_000))),
        ("many emphasis markers", line(&"*".repeat(50_000))),
        ("many brackets", line(&"[".repeat(50_000))),
        ("nested emphasis", line(&nested_emphasis(5_000))),
        (
            "flat emphasis runs",
            line(&format!("{}x{}", "*_".repeat(5_000), "_*".repeat(5_000))),
        ),
        (
            "deep nested lists",
            line(
                &(0..1_000)
                    .map(|i| format!("{}- item\n", " ".repeat((i * 2).min(600))))
                    .collect::<String>(),
            ),
        ),
        ("comment blocks", line(&"<!--x-->\n\n".repeat(90_000))),
        ("unclosed comment", line(&format!("<!--{}", "x".repeat(500 * 1024)))),
        (
            "huge role line",
            format!("# Ada\n\nHere\n\n## S\n\n### {}\n", "a - ".repeat(100_000)),
        ),
        ("huge era line", line(&format!("**{}**", "a - ".repeat(100_000)))),
        ("huge contact line", format!("# Ada\n\n{}\n", "a · ".repeat(100_000))),
        ("many findings", line(&"<b>x</b> ".repeat(60_000))),
    ]
}

#[test]
fn pathological_inputs_do_not_overflow_a_small_stack() {
    for (name, text) in pathological_inputs() {
        assert!(
            text.len() <= MAX_INPUT_BYTES,
            "{name} must be under the size cap to exercise the parser"
        );
        on_small_stack(name, move || {
            let parsed = parse("r.md", &text);
            // Serializing the model recurses over inlines too; the depth caps must keep it shallow.
            assert_ok!(serde_json::to_vec(&parsed.best_effort()));
            assert!(parsed.diagnostics().len() <= 501, "diagnostics are capped");
        });
    }
}

/// Parses `text` and fails if it takes longer than `limit`, so a quadratic regression fails crisply.
fn assert_parses_within(name: &str, text: &str, limit: Duration) {
    let started = Instant::now();
    let parsed = parse("r.md", text);
    let elapsed = started.elapsed();
    assert!(
        elapsed < limit,
        "{name}: parsing {} bytes took {elapsed:?} (limit {limit:?})",
        text.len()
    );
    assert!(parsed.diagnostics().len() <= 501, "{name}: diagnostics are capped");
}

#[test]
fn many_html_blocks_parse_in_linear_time() {
    // A run of comment blocks was quadratic: every block rescanned all the blocks after it.
    let head = "# Ada\n\nHere\n\n## S\n\n";
    for (name, block) in [
        ("comment blocks", "<!--x-->\n\n"),
        ("empty comments", "<!-->\n\n"),
        ("page breaks", "<div class=\"page-break\"></div>\n\n"),
        ("unsupported html", "<div>x</div>\n\n"),
    ] {
        let text = format!("{head}{}", block.repeat(20_000));
        assert_parses_within(name, &text, Duration::from_secs(5));
    }
}

#[test]
fn nesting_beyond_the_cap_is_reported_not_crashed_on() {
    let delimiter = |i: usize| if i.is_multiple_of(2) { "*" } else { "_" };
    let opens: String = (0..30).map(|i| format!("{}w ", delimiter(i))).collect();
    let closes: String = (0..30).rev().map(|i| format!(" w{}", delimiter(i))).collect();
    let text = format!("# Ada\n\nHere\n\n## S\n\n{opens}x{closes}\n");
    let parsed = parse("r.md", &text);
    assert!(
        parsed
            .diagnostics()
            .iter()
            .any(|d| d.code == DiagnosticCode::InputLimit)
    );
    assert!(parsed.has_errors());
}

#[test]
fn oversized_input_is_one_error_and_no_model() {
    let parsed = parse("r.md", &"a".repeat(MAX_INPUT_BYTES + 1));
    assert_eq!(parsed.diagnostics().len(), 1);
    assert_eq!(parsed.diagnostics()[0].code, DiagnosticCode::InputLimit);
    assert_eq!(parsed.diagnostics()[0].severity, Severity::Error);
    assert_none!(parsed.best_effort());
}

// ---------------------------------------------------------------------------------------------
// Real baselines (ignored: they live outside the repository)
// ---------------------------------------------------------------------------------------------

/// Parses the two real baselines and expects only the contact-count warning the book calls for.
///
/// Run with `RESUME_BASELINES=~/Documents/vrm/my/job/drafts cargo test --test parse_test -- --ignored`.
#[test]
#[ignore = "reads the real resumes from RESUME_BASELINES"]
fn baselines_parse_without_errors() {
    let Some(dir) = env::var_os("RESUME_BASELINES") else {
        panic!(
            "set RESUME_BASELINES to the directory holding Resume-Principal-Architect.md and Resume-Org-Leader.md, \
             for example: RESUME_BASELINES=~/Documents/vrm/my/job/drafts cargo test --test parse_test -- --ignored"
        );
    };
    for name in ["Resume-Principal-Architect.md", "Resume-Org-Leader.md"] {
        let path = Path::new(&dir).join(name);
        let text = assert_ok!(fs::read_to_string(&path), "{}", path.display());
        let parsed = parse(&path, &text);
        assert!(
            !parsed.has_errors(),
            "{name}: {:#?}",
            views(&text, parsed.diagnostics())
        );
        let codes: Vec<_> = parsed.diagnostics().iter().map(|d| d.code).collect();
        assert_eq!(
            codes,
            vec![DiagnosticCode::TooManyContactItems],
            "{name}: the only expected finding is the contact count"
        );
        assert_some!(parsed.valid(), "{name}");
    }
}

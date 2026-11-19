//! ABOUTME: Helpers shared by the parser's integration tests: applying Edits and checking what a span quotes.
//! ABOUTME: Lives in `common/mod.rs` so cargo does not compile it as a test target of its own.

// Each integration test is its own crate and uses a different subset of these helpers.
#![allow(dead_code)]

use resume::diagnostics::{Diagnostic, DiagnosticCode, Edit};
use resume::markdown::{Parsed, parse};

/// Applies non-overlapping edits from the end of the text backwards, so earlier offsets stay valid.
pub fn apply_edits(text: &str, edits: &[&Edit]) -> String {
    let mut sorted = edits.to_vec();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));
    let mut out = text.to_string();
    for edit in sorted {
        out.replace_range(edit.range.clone(), &edit.replacement);
    }
    out
}

/// The findings other than hard breaks, as sorted `severity|code|message` keys: what a hard-break Edit must
/// not change (positions move when text is deleted, so spans are left out).
pub fn other_findings(parsed: &Parsed) -> Vec<String> {
    let mut keys: Vec<String> = parsed
        .diagnostics()
        .iter()
        .filter(|d| d.code != DiagnosticCode::HardBreak)
        .map(|d| format!("{}|{}|{}", d.severity, d.code, d.message))
        .collect();
    keys.sort();
    keys
}

/// Applies ONLY the hard-break Edits of `text` and checks the claim the parser makes for them: the model and
/// the other findings are exactly what they were. `Err` describes the difference.
pub fn hard_break_edits_change_nothing(text: &str) -> Result<(), String> {
    let before = parse("r.md", text);
    let edits: Vec<&Edit> = before
        .diagnostics()
        .iter()
        .filter(|d| d.code == DiagnosticCode::HardBreak)
        .filter_map(|d| d.fix.as_ref())
        .collect();
    let fixed = apply_edits(text, &edits);
    let after = parse("r.md", &fixed);
    if before.best_effort() != after.best_effort() {
        return Err(format!("the model changed\nbefore: {text:?}\nafter:  {fixed:?}"));
    }
    if other_findings(&before) != other_findings(&after) {
        return Err(format!(
            "the other findings changed: {:?} -> {:?}\nbefore: {text:?}\nafter:  {fixed:?}",
            other_findings(&before),
            other_findings(&after)
        ));
    }
    Ok(())
}

/// The Edits carried by `diagnostics`.
pub fn edits_of(diagnostics: &[Diagnostic]) -> Vec<&Edit> {
    diagnostics.iter().filter_map(|d| d.fix.as_ref()).collect()
}

/// True when `source`, the text a finding's span quotes, is the kind of thing its code names.
///
/// A span that points at the wrong thing (a neighbouring line, half a dash) is a bug the range checks
/// alone cannot see.
pub fn subject_matches(code: DiagnosticCode, source: &str) -> bool {
    use DiagnosticCode::*;
    let is_pair = |delimiter: &str| {
        source.len() >= 2 * delimiter.len() && source.starts_with(delimiter) && source.ends_with(delimiter)
    };
    match code {
        PeriodHyphen => {
            !source.is_empty()
                && source
                    .chars()
                    .all(|c| c.is_whitespace() || matches!(c, '-' | '–' | '—'))
        }
        PeriodPresentSpelling => matches!(source.to_lowercase().as_str(), "present" | "current" | "now"),
        HardBreak => !source.is_empty() && source.chars().all(|c| matches!(c, ' ' | '\t' | '\\')),
        Image => source.starts_with("!["),
        HeadingMarkerSpacing => !source.is_empty() && source.chars().all(|c| c == '#'),
        MidSentenceBold => is_pair("**") || is_pair("__"),
        UnsupportedHtml => source.starts_with('<'),
        InvalidLink | UnsupportedLink => source.starts_with(['[', '<']),
        NestedList => source.starts_with(['-', '*', '+']) || source.starts_with(|c: char| c.is_ascii_digit()),
        _ => true,
    }
}

/// What a child test process did.
pub struct ChildRun {
    pub success: bool,
    pub stderr: String,
}

/// Runs `command`, killing it if it takes longer than `timeout`, and captures its stderr.
///
/// A hung child must fail the test, not hang the suite, and a crashed one must say why.
pub fn run_child(mut command: std::process::Command, timeout: std::time::Duration) -> ChildRun {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::Instant;

    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the child process starts");
    let mut stderr_pipe = child.stderr.take().expect("stderr is piped");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr_pipe.read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("the child can be polled") {
            break Some(status);
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let mut stderr = reader.join().unwrap_or_default();
    if status.is_none() {
        stderr.push_str(&format!("\n[killed after {timeout:?}]"));
    }
    ChildRun {
        success: status.is_some_and(|s| s.success()),
        stderr,
    }
}

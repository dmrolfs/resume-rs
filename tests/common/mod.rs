//! ABOUTME: Helpers shared by the parser's integration tests: applying Edits and checking what a span quotes.
//! ABOUTME: Lives in `common/mod.rs` so cargo does not compile it as a test target of its own.

// Each integration test is its own crate and uses a different subset of these helpers.
#![allow(dead_code)]

use resume::diagnostics::{Diagnostic, DiagnosticCode, Edit};

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

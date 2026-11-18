//! ABOUTME: Helpers shared by the parser's unit tests: finding codes, valid years, and applying Edits.
//! ABOUTME: Compiled only for tests, so the modules' test code does not carry private copies.

use claims::assert_ok;

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::model::Year;

/// The codes of `diagnostics`, in order.
pub(crate) fn codes(diagnostics: &[Diagnostic]) -> Vec<DiagnosticCode> {
    diagnostics.iter().map(|d| d.code).collect()
}

/// A year known to be in range.
pub(crate) fn year(y: u16) -> Year {
    assert_ok!(Year::try_new(y))
}

/// `text` with every Edit of `diagnostics` applied from the end backwards, so earlier offsets stay valid.
pub(crate) fn apply_fixes(text: &str, diagnostics: &[Diagnostic]) -> String {
    let mut edits: Vec<_> = diagnostics.iter().filter_map(|d| d.fix.as_ref()).collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
    let mut out = text.to_string();
    for edit in edits {
        out.replace_range(edit.range.clone(), &edit.replacement);
    }
    out
}

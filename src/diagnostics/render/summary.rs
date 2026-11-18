//! ABOUTME: The one-line summary of a run's findings: errors, warnings, notes, and how many are fixable.
//! ABOUTME: ASCII only, so it reads the same on legacy Windows consoles, in logs, and in pasted text.

use std::fmt;

use crate::diagnostics::{Diagnostic, Severity};

/// Counts for the summary line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    /// Findings with severity `Error`.
    pub errors: usize,
    /// Findings with severity `Warning`.
    pub warnings: usize,
    /// Findings with severity `Note`.
    pub notes: usize,
    /// Findings of any severity that carry an Edit.
    pub fixable: usize,
}

impl Summary {
    /// Counts findings by severity and by whether they carry an Edit.
    pub fn of<'d>(diagnostics: impl IntoIterator<Item = &'d Diagnostic>) -> Self {
        diagnostics.into_iter().fold(Self::default(), |mut counts, diagnostic| {
            match diagnostic.severity {
                Severity::Error => counts.errors += 1,
                Severity::Warning => counts.warnings += 1,
                Severity::Note => counts.notes += 1,
            }
            counts.fixable += usize::from(diagnostic.fix.is_some());
            counts
        })
    }

    /// True when any finding is an error.
    #[must_use]
    pub const fn has_errors(&self) -> bool {
        self.errors > 0
    }
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}, {}",
            count(self.errors, "error"),
            count(self.warnings, "warning")
        )?;
        if self.notes > 0 {
            write!(f, ", {}", count(self.notes, "note"))?;
        }
        if self.fixable > 0 {
            write!(f, "; {} fixable", self.fixable)?;
        }
        Ok(())
    }
}

/// "1 error", "2 errors": the noun is pluralized unless the count is exactly one.
fn count(n: usize, noun: &str) -> String {
    let plural = if n == 1 { "" } else { "s" };
    format!("{n} {noun}{plural}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagnosticCode, Edit};
    use pretty_assertions::assert_eq;

    fn finding(severity: Severity, fixable: bool) -> Diagnostic {
        Diagnostic {
            severity,
            code: DiagnosticCode::Image,
            message: String::new(),
            span: None,
            hints: Vec::new(),
            fix: fixable.then(|| Edit {
                range: 0..0,
                replacement: String::new(),
            }),
        }
    }

    fn summary(findings: &[(Severity, bool)]) -> Summary {
        let diagnostics: Vec<_> = findings.iter().map(|&(s, f)| finding(s, f)).collect();
        Summary::of(&diagnostics)
    }

    #[test]
    fn summary_counts_errors_warnings_and_fixable() {
        let counted = summary(&[
            (Severity::Error, false),
            (Severity::Error, true),
            (Severity::Warning, true),
            (Severity::Note, true),
        ]);
        assert_eq!(
            counted,
            Summary {
                errors: 2,
                warnings: 1,
                notes: 1,
                fixable: 3
            }
        );
        assert_eq!(counted.to_string(), "2 errors, 1 warning, 1 note; 3 fixable");
    }

    #[test]
    fn summary_pluralizes_and_hides_zero_notes_and_zero_fixable() {
        assert_eq!(summary(&[(Severity::Error, false)]).to_string(), "1 error, 0 warnings");
        assert_eq!(
            summary(&[(Severity::Warning, false), (Severity::Warning, false)]).to_string(),
            "0 errors, 2 warnings"
        );
        assert_eq!(
            summary(&[(Severity::Note, false), (Severity::Note, false)]).to_string(),
            "0 errors, 0 warnings, 2 notes"
        );
        assert_eq!(
            summary(&[(Severity::Warning, true)]).to_string(),
            "0 errors, 1 warning; 1 fixable"
        );
    }

    #[test]
    fn a_clean_run_says_so() {
        assert_eq!(summary(&[]).to_string(), "0 errors, 0 warnings");
        assert!(!summary(&[]).has_errors());
    }

    #[test]
    fn has_errors_is_true_only_for_errors() {
        assert!(summary(&[(Severity::Error, false)]).has_errors());
        assert!(!summary(&[(Severity::Warning, true), (Severity::Note, false)]).has_errors());
    }

    #[test]
    fn the_summary_is_ascii() {
        let all = summary(&[
            (Severity::Error, true),
            (Severity::Warning, true),
            (Severity::Note, true),
        ]);
        assert!(all.to_string().is_ascii(), "{all}");
    }
}

//! ABOUTME: Diagnostic types shared by the parser, doctor, and renderer: severity, span, and edits.
//! ABOUTME: One type covers Markdown and Typst findings so a single renderer can display both.

mod advice;
pub mod code;
pub(crate) mod lines;
mod render;

use std::ops::Range;
use std::path::PathBuf;

use serde::Serialize;
use strum::{AsRefStr, Display, EnumString};

pub use self::code::{Advice, Category, DiagnosticCode};
pub use self::render::{
    ColorEnv, RenderError, Sources, Summary, display_text, emit_human, emit_human_with_notes, emit_json,
    emit_json_record, resolve_color, termcolor,
};

/// How serious a finding is; ordered so `Error` is the greatest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Display, AsRefStr, EnumString)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Note,
    Warning,
    Error,
}

/// Which kind of file a [`Span`] points into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Markdown,
    Style,
}

/// A byte range in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub file: PathBuf,
    pub kind: SourceKind,
    pub range: Range<usize>,
}

/// A mechanical, meaning-preserving replacement of a byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub replacement: String,
}

impl Edit {
    /// `text` with every edit applied, from the end, so no edit moves another's range.
    ///
    /// The one applier for both the hard-break proof and `doctor --fix`, so what was proven is what is applied.
    /// The edits must not overlap; `doctor::fix` checks that before calling this.
    #[must_use]
    pub fn apply_all(text: &str, edits: &[&Edit]) -> String {
        let mut ordered: Vec<&Edit> = edits.to_vec();
        ordered.sort_by_key(|edit| std::cmp::Reverse(edit.range.start));
        let mut fixed = text.to_string();
        for edit in ordered {
            fixed.replace_range(edit.range.clone(), &edit.replacement);
        }
        fixed
    }
}

/// A located finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    /// Specific to this occurrence.
    pub message: String,
    pub span: Option<Span>,
    /// Occurrence-specific hints, e.g. the corrected line.
    pub hints: Vec<String>,
    /// Present when the fix is mechanical and meaning-preserving.
    pub fix: Option<Edit>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use pretty_assertions::assert_eq;

    #[test]
    fn severity_orders_error_highest() {
        assert!(Severity::Note < Severity::Warning && Severity::Warning < Severity::Error);
    }

    #[test]
    fn severity_displays_and_parses_lowercase() {
        assert_eq!(Severity::Warning.to_string(), "warning");
        assert_eq!(assert_ok!("error".parse::<Severity>()), Severity::Error);
        assert_eq!(assert_ok!(serde_json::to_string(&Severity::Note)), r#""note""#);
    }
}

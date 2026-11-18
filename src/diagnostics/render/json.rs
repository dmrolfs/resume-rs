//! ABOUTME: One JSON object per finding, one per line, for editors, CI, and scripts.
//! ABOUTME: A private record type fixes the contract: every key is always present, `null` when unknown.

use std::io;

use serde::Serialize;
use snafu::ResultExt;

use super::sources::{Resolved, Sources};
use super::{JsonSnafu, RenderError, WriteSnafu};
use crate::diagnostics::{Category, Diagnostic, DiagnosticCode, Severity};

/// The documented JSON-lines record.
///
/// Serde writes fields in declaration order, so the key order is part of the contract. New keys may
/// be added at the end; existing keys never change meaning.
#[derive(Debug, Serialize)]
struct JsonDiagnostic<'a> {
    /// Always `"diagnostic"`; lets later line types share the stream.
    #[serde(rename = "type")]
    kind: &'static str,
    severity: Severity,
    code: DiagnosticCode,
    category: Category,
    message: &'a str,
    /// The path as shown to people; `null` for a finding with no span.
    file: Option<String>,
    /// 1-based line of the span's start; `null` when the finding cannot be located.
    line: Option<usize>,
    /// 1-based column, counted in Unicode scalar values; `null` when unlocated.
    column: Option<usize>,
    /// Line just past the span (exclusive end); `null` when unlocated.
    end_line: Option<usize>,
    /// Column just past the span (exclusive end); `null` when unlocated.
    end_column: Option<usize>,
    hints: &'a [String],
    fixable: bool,
}

impl<'a> JsonDiagnostic<'a> {
    fn new(d: &'a Diagnostic, resolved: &Resolved<'_>) -> Self {
        let (file, position) = match resolved {
            Resolved::NoSpan => (None, None),
            Resolved::Unlocated { name, .. } => (Some(name.clone()), None),
            Resolved::Located(located) => (Some(located.name.clone()), Some((located.start, located.end))),
        };
        Self {
            kind: "diagnostic",
            severity: d.severity,
            code: d.code,
            category: d.code.category(),
            message: &d.message,
            file,
            line: position.map(|(start, _)| start.line),
            column: position.map(|(start, _)| start.column),
            end_line: position.map(|(_, end)| end.line),
            end_column: position.map(|(_, end)| end.column),
            hints: &d.hints,
            fixable: d.fix.is_some(),
        }
    }
}

/// Writes one record and its `\n`.
///
/// The terminator is `\n` on every OS, so the stream reads the same through a pipe on Windows.
pub(super) fn write_line(out: &mut dyn io::Write, sources: &Sources<'_>, d: &Diagnostic) -> Result<(), RenderError> {
    let resolved = sources.resolve(d.span.as_ref());
    let mut line = serde_json::to_vec(&JsonDiagnostic::new(d, &resolved)).context(JsonSnafu)?;
    line.push(b'\n');
    out.write_all(&line).context(WriteSnafu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Edit, SourceKind, Span};
    use claims::{assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use serde_json::Value;

    fn finding(range: Option<std::ops::Range<usize>>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code: DiagnosticCode::PeriodHyphen,
            message: "dates are separated by `-`".into(),
            span: range.map(|range| Span {
                file: "t.md".into(),
                kind: SourceKind::Markdown,
                range,
            }),
            hints: vec!["write `2015 – 2017`".into()],
            fix: None,
        }
    }

    fn record(d: &Diagnostic, text: Option<&str>) -> String {
        let mut sources = Sources::new(None);
        if let Some(text) = text {
            sources.insert("t.md", text);
        }
        let mut out = Vec::new();
        assert_ok!(write_line(&mut out, &sources, d));
        assert_ok!(String::from_utf8(out))
    }

    fn parsed(line: &str) -> Value {
        assert_ok!(serde_json::from_str(line.trim_end_matches('\n')))
    }

    #[test]
    fn a_located_finding_writes_every_documented_field_in_order() {
        let line = record(&finding(Some(5..8)), Some("2015 - 2017\n"));
        assert_eq!(
            line,
            concat!(
                r#"{"type":"diagnostic","severity":"error","code":"period-hyphen","category":"format","#,
                r#""message":"dates are separated by `-`","file":"t.md","line":1,"column":6,"#,
                r#""end_line":1,"end_column":9,"hints":["write `2015 – 2017`"],"fixable":false}"#,
                "\n"
            )
        );
    }

    #[test]
    fn the_record_ends_with_exactly_one_lf_and_no_cr() {
        let line = record(&finding(Some(0..1)), Some("x"));
        assert!(line.ends_with("}\n"), "{line:?}");
        assert!(!line.contains('\r'));
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[test]
    fn end_position_is_exclusive_and_follows_a_multi_line_span() {
        let text = "ab\ncd\nef\n";
        let value = parsed(&record(&finding(Some(1..7)), Some(text)));
        assert_eq!(value["line"], 1);
        assert_eq!(value["column"], 2);
        assert_eq!(value["end_line"], 3);
        assert_eq!(value["end_column"], 2);
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // 日 and 本 are three bytes each, so `é` starts at byte 7 but is the fourth character.
        let value = parsed(&record(&finding(Some(7..9)), Some("日本 é x\n")));
        assert_eq!(
            (value["column"].as_u64(), value["end_column"].as_u64()),
            (Some(4), Some(5))
        );
    }

    #[test]
    fn an_unlocated_finding_keeps_its_file_and_nulls_the_position() {
        let value = parsed(&record(&finding(Some(40..50)), Some("short")));
        assert_eq!(value["file"], "t.md");
        for key in ["line", "column", "end_line", "end_column"] {
            assert_eq!(value[key], Value::Null, "{key}");
        }
        let missing = parsed(&record(&finding(Some(0..1)), None));
        assert_eq!(missing["file"], "t.md");
        assert_eq!(missing["line"], Value::Null);
    }

    #[test]
    fn a_finding_without_a_span_has_a_null_file_and_position_but_every_key() {
        let value = parsed(&record(&finding(None), None));
        let object = assert_some!(value.as_object());
        assert_eq!(object.len(), 12);
        assert_eq!(value["file"], Value::Null);
        assert_eq!(value["line"], Value::Null);
        assert_eq!(value["category"], "format");
    }

    #[test]
    fn fixable_follows_the_edit_and_hints_are_kept_in_order() {
        let mut d = finding(None);
        d.hints = vec!["one".into(), "two".into()];
        d.fix = Some(Edit {
            range: 0..0,
            replacement: String::new(),
        });
        let value = parsed(&record(&d, None));
        assert_eq!(value["fixable"], true);
        assert_eq!(value["hints"], serde_json::json!(["one", "two"]));
    }

    #[test]
    fn json_keeps_the_full_message_and_escapes_control_characters_losslessly() {
        let mut d = finding(None);
        d.message = format!("esc \u{1b}[31m, tab \t, newline \n, {}", "m".repeat(2000));
        let line = record(&d, None);
        assert!(!line.contains('\u{1b}'));
        assert_eq!(line.matches('\n').count(), 1, "the newline in the message is escaped");
        assert_eq!(parsed(&line)["message"], d.message.as_str());
    }

    #[test]
    fn a_failing_writer_is_a_write_error() {
        struct Broken;
        impl io::Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("closed"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let err = write_line(&mut Broken, &Sources::new(None), &finding(None));
        assert!(matches!(err, Err(RenderError::Write { .. })), "{err:?}");
    }
}

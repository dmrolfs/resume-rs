//! ABOUTME: Renders diagnostics for people (rustc-style excerpts) and for tools (JSON lines).
//! ABOUTME: Bounded, sanitized output; color resolution and the summary line live here too.

mod color;
mod excerpt;
mod human;
mod json;
mod sanitize;
mod sources;
mod summary;

use codespan_reporting::files::SimpleFile;
use codespan_reporting::term;
use snafu::{IntoError, Snafu};

use self::excerpt::Excerpt;
use self::sources::Resolved;
use crate::diagnostics::Diagnostic;

pub use codespan_reporting::term::termcolor;

pub use self::color::{ColorEnv, resolve_color};
pub(crate) use self::sanitize::has_unsafe_multiline_characters;
pub use self::sanitize::{display_text, has_unsafe_characters};
pub use self::sources::{SourceConflict, SourcePosition, Sources};
pub use self::summary::Summary;

/// Failure to write rendered diagnostics.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(super)))]
pub enum RenderError {
    /// codespan could not draw a finding. Never an I/O failure (those are [`RenderError::Write`]), so it
    /// means a bug in how an excerpt was built.
    #[snafu(display("could not draw a diagnostic: {source}"))]
    Human { source: codespan_reporting::files::Error },
    /// A finding could not be serialized.
    #[snafu(display("could not serialize a diagnostic as JSON: {source}"))]
    Json { source: serde_json::Error },
    /// The output could not be written.
    #[snafu(display("could not write diagnostics: {source}"))]
    Write { source: std::io::Error },
}

/// Sorts a codespan failure: an I/O error is a write failure whatever codespan wrapped it in, so a
/// caller can treat `BrokenPipe` the same way for both formats; anything else is a rendering failure.
fn from_codespan(error: codespan_reporting::files::Error) -> RenderError {
    match error {
        codespan_reporting::files::Error::Io(source) => WriteSnafu.into_error(source),
        source => HumanSnafu.into_error(source),
    }
}

/// Writes each diagnostic as an ASCII excerpt with hints, in the given order; no summary.
///
/// # Errors
/// - [`RenderError::Write`] - When `out` cannot be written.
/// - [`RenderError::Human`] - When codespan cannot draw a finding (a bug: the excerpt is always valid).
pub fn emit_human<'d>(
    out: &mut dyn termcolor::WriteColor,
    sources: &Sources<'_>,
    diagnostics: impl IntoIterator<Item = &'d Diagnostic>,
) -> Result<(), RenderError> {
    emit_human_with_notes(out, sources, diagnostics.into_iter().map(|d| (d, &[][..])))
}

/// Like [`emit_human`], with extra notes after each finding's own, such as the advice for its code.
///
/// # Errors
/// - [`RenderError::Write`] - When `out` cannot be written.
/// - [`RenderError::Human`] - When codespan cannot draw a finding (a bug: the excerpt is always valid).
pub fn emit_human_with_notes<'d, 'n>(
    out: &mut dyn termcolor::WriteColor,
    sources: &Sources<'_>,
    diagnostics: impl IntoIterator<Item = (&'d Diagnostic, &'n [String])>,
) -> Result<(), RenderError> {
    let config = human::config();
    // A finding with no excerpt has no labels, so codespan never looks anything up in this file.
    let no_file = SimpleFile::new("", "");
    for (diagnostic, extra) in diagnostics {
        let resolved = sources.resolve(diagnostic.span.as_ref());
        let excerpt = match &resolved {
            Resolved::Located(located) => Some(Excerpt::build(located)),
            Resolved::NoSpan | Resolved::Unlocated { .. } => None,
        };
        let drawn = human::to_codespan(diagnostic, &resolved, excerpt.as_ref(), extra);
        match &excerpt {
            Some(excerpt) => term::emit_to_write_style(out, &config, excerpt, &drawn),
            None => term::emit_to_write_style(out, &config, &no_file, &drawn),
        }
        .map_err(from_codespan)?;
    }
    Ok(())
}

/// Writes one JSON object per diagnostic, each terminated by `\n`, in the given order.
///
/// # Errors
/// - [`RenderError::Json`] - When a finding cannot be serialized.
/// - [`RenderError::Write`] - When `out` cannot be written.
pub fn emit_json<'d>(
    out: &mut dyn std::io::Write,
    sources: &Sources<'_>,
    diagnostics: impl IntoIterator<Item = &'d Diagnostic>,
) -> Result<(), RenderError> {
    diagnostics
        .into_iter()
        .try_for_each(|diagnostic| json::write_line(out, sources, diagnostic))
}

/// Writes one non-diagnostic JSON-lines record, such as a build's `selection` record, with the escaping every
/// record on the stream shares. The record must carry its own `type` key.
///
/// # Errors
/// - [`RenderError::Json`] - When the record cannot be serialized.
/// - [`RenderError::Write`] - When `out` cannot be written.
pub fn emit_json_record(out: &mut dyn std::io::Write, record: &impl serde::Serialize) -> Result<(), RenderError> {
    json::write_record(out, record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagnosticCode, Severity, SourceKind, Span};
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;
    use std::io;
    use termcolor::NoColor;

    /// A writer that always fails with `kind`.
    struct Failing(io::ErrorKind);

    impl io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn finding(range: Option<std::ops::Range<usize>>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code: DiagnosticCode::UnsupportedHtml,
            message: "problem".into(),
            span: range.map(|range| Span {
                file: "t.md".into(),
                kind: SourceKind::Markdown,
                range,
            }),
            hints: Vec::new(),
            fix: None,
        }
    }

    fn write_kind(error: RenderError) -> io::ErrorKind {
        match error {
            RenderError::Write { source } => source.kind(),
            other => panic!("expected a write error, got {other:?}"),
        }
    }

    #[test]
    fn a_write_failure_in_human_output_is_a_write_error_that_keeps_its_kind() {
        let mut sources = Sources::new(None);
        sources.insert("t.md", "abc");
        for range in [Some(0..1), None] {
            let mut out = NoColor::new(Failing(io::ErrorKind::BrokenPipe));
            let error = assert_err!(emit_human(&mut out, &sources, [&finding(range.clone())]));
            assert_eq!(write_kind(error), io::ErrorKind::BrokenPipe, "{range:?}");
        }
    }

    #[test]
    fn a_write_failure_in_json_output_is_a_write_error_that_keeps_its_kind() {
        let sources = Sources::new(None);
        let error = assert_err!(emit_json(
            &mut Failing(io::ErrorKind::BrokenPipe),
            &sources,
            [&finding(None)]
        ));
        assert_eq!(write_kind(error), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn nothing_is_written_when_there_is_nothing_to_report() {
        let mut out = termcolor::Buffer::no_color();
        assert_ok!(emit_human(&mut out, &Sources::new(None), []));
        let mut json = Vec::new();
        assert_ok!(emit_json(&mut json, &Sources::new(None), []));
        assert!(out.as_slice().is_empty() && json.is_empty());
    }
}

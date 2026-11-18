//! ABOUTME: Renders diagnostics for people (rustc-style excerpts) and for tools (JSON lines).
//! ABOUTME: Bounded, sanitized output; color resolution and the summary line live here too.

mod color;
mod excerpt;
mod human;
mod json;
mod sources;
mod summary;

use codespan_reporting::files::SimpleFile;
use codespan_reporting::term;
use snafu::{ResultExt, Snafu};

use self::excerpt::Excerpt;
use self::sources::Resolved;
use crate::diagnostics::Diagnostic;

pub use codespan_reporting::term::termcolor;

pub use self::color::{ColorEnv, resolve_color};
pub use self::sources::Sources;
pub use self::summary::Summary;

/// Failure to write rendered diagnostics.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(super)))]
pub enum RenderError {
    /// The excerpt renderer failed, usually because the output could not be written.
    #[snafu(display("could not write diagnostics: {source}"))]
    Human { source: codespan_reporting::files::Error },
    /// A finding could not be serialized.
    #[snafu(display("could not serialize a diagnostic as JSON: {source}"))]
    Json { source: serde_json::Error },
    /// The output could not be written.
    #[snafu(display("could not write diagnostics: {source}"))]
    Write { source: std::io::Error },
}

/// Writes each diagnostic as an ASCII excerpt with hints, in the given order; no summary.
///
/// # Errors
/// - [`RenderError::Human`] - When `out` cannot be written.
pub fn emit_human<'d>(
    out: &mut dyn termcolor::WriteColor,
    sources: &Sources<'_>,
    diagnostics: impl IntoIterator<Item = &'d Diagnostic>,
) -> Result<(), RenderError> {
    let config = human::config();
    // A finding with no excerpt has no labels, so codespan never looks anything up in this file.
    let no_file = SimpleFile::new("", "");
    for diagnostic in diagnostics {
        let resolved = sources.resolve(diagnostic.span.as_ref());
        let excerpt = match &resolved {
            Resolved::Located(located) => Some(Excerpt::build(located)),
            Resolved::NoSpan | Resolved::Unlocated { .. } => None,
        };
        let drawn = human::to_codespan(diagnostic, &resolved, excerpt.as_ref());
        match &excerpt {
            Some(excerpt) => term::emit_to_write_style(out, &config, excerpt, &drawn),
            None => term::emit_to_write_style(out, &config, &no_file, &drawn),
        }
        .context(HumanSnafu)?;
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

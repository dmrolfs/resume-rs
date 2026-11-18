//! ABOUTME: Operational errors: style lookup, thread and serialization failures, Typst compile and export.
//! ABOUTME: Content problems are `Diagnostic`s; `Compile` and `ExportPdf` carry them so one run reports all.

use std::path::PathBuf;

use snafu::Snafu;
use strum::Display;

use crate::diagnostics::{Diagnostic, Severity};
use crate::model::StyleName;
use crate::typeset::fonts::FontOrigin;

/// A style source file as Typst read it, so a renderer can show excerpts of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleFile {
    /// The path to show users.
    pub path: PathBuf,
    /// The decoded text; byte ranges in diagnostics index into this.
    pub text: String,
}

/// The style source files read during one compile, ordered by path.
pub type StyleSources = Vec<StyleFile>;

/// The pipeline stage that was running when a panic was caught.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Stage {
    Compile,
    ExportPdf,
}

/// Operational failures of the typesetting pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    #[snafu(display("unknown style `{name}`: no directory `{name}` in {}", searched.display()))]
    UnknownStyle { name: StyleName, searched: PathBuf },

    #[snafu(display("style directory {} has no style.typ", dir.display()))]
    StyleMissingMain { dir: PathBuf },

    #[snafu(display("style directory {} resolves outside the styles directory (a symlink?)", dir.display()))]
    StyleOutsideRoot { dir: PathBuf },

    #[snafu(display("cannot open styles directory {}: {source}", path.display()))]
    StyleRoot { path: PathBuf, source: std::io::Error },

    #[snafu(display("cannot serialize the resume for the style: {source}"))]
    SerializeResume { source: serde_json::Error },

    #[snafu(display("cannot read font directory {}: {source}", path.display()))]
    FontDirectory { path: PathBuf, source: std::io::Error },

    #[snafu(display("scanning {origin} fonts panicked: {message}"))]
    FontScanPanicked { origin: FontOrigin, message: String },

    #[snafu(display("cannot start the typesetting thread: {source}"))]
    SpawnTypesetThread { source: std::io::Error },

    #[snafu(display("typesetting panicked during {stage}: {message}"))]
    TypesetPanicked { stage: Stage, message: String },

    #[snafu(display("the style failed to compile ({} errors)", errors_in(diagnostics)))]
    Compile {
        diagnostics: Vec<Diagnostic>,
        sources: StyleSources,
    },

    #[snafu(display("PDF export failed ({} errors)", errors_in(diagnostics)))]
    ExportPdf {
        diagnostics: Vec<Diagnostic>,
        sources: StyleSources,
    },
}

fn errors_in(diagnostics: &[Diagnostic]) -> usize {
    diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
}

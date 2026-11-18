//! ABOUTME: Operational errors: style lookup, thread and serialization failures, Typst compile and export.
//! ABOUTME: Content problems are `Diagnostic`s; `Compile` and `ExportPdf` carry them so one run reports all.

use std::path::PathBuf;

use snafu::Snafu;
use strum::Display;

use crate::diagnostics::Diagnostic;
use crate::model::StyleName;

/// The text of every style file Typst read, keyed by display path, so a renderer can show excerpts.
pub type StyleSources = Vec<(PathBuf, String)>;

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

    #[snafu(display("cannot open styles directory {}: {source}", path.display()))]
    StyleRoot { path: PathBuf, source: std::io::Error },

    #[snafu(display("cannot serialize the resume for the style: {source}"))]
    SerializeResume { source: serde_json::Error },

    #[snafu(display("cannot start the typesetting thread: {source}"))]
    SpawnTypesetThread { source: std::io::Error },

    #[snafu(display("typesetting panicked during {stage}: {message}"))]
    TypesetPanicked { stage: Stage, message: String },

    #[snafu(display("the style failed to compile ({} diagnostics)", diagnostics.len()))]
    Compile {
        diagnostics: Vec<Diagnostic>,
        sources: StyleSources,
    },

    #[snafu(display("PDF export failed ({} diagnostics)", diagnostics.len()))]
    ExportPdf {
        diagnostics: Vec<Diagnostic>,
        sources: StyleSources,
    },
}

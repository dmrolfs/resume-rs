//! ABOUTME: Operational errors: style lookup, writing output, thread failures, Typst compile and export.
//! ABOUTME: Content problems are `Diagnostic`s; `Compile` and `ExportPdf` carry them so one run reports all.

use std::path::PathBuf;

use snafu::Snafu;
use strum::Display;

use crate::diagnostics::{Diagnostic, Severity};
use crate::model::StyleName;
use crate::model::options::FontOrigin;

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
    #[snafu(display(
        "unknown style `{name}`: no directory `{name}` in {}{}",
        searched.display(),
        available_text(available)
    ))]
    UnknownStyle {
        name: StyleName,
        searched: PathBuf,
        /// The styles that do exist, so the message can say what to type instead; empty when not known.
        available: Vec<StyleName>,
    },

    #[snafu(display("style directory {} has no style.typ", dir.display()))]
    StyleMissingMain { dir: PathBuf },

    #[snafu(display("style directory {} resolves outside the styles directory (a symlink?)", dir.display()))]
    StyleOutsideRoot { dir: PathBuf },

    #[snafu(display("cannot open styles directory {}: {source}", path.display()))]
    StyleRoot { path: PathBuf, source: std::io::Error },

    #[snafu(display("cannot list the style directory {}: {source}", path.display()))]
    StyleDirectory { path: PathBuf, source: std::io::Error },

    #[snafu(display("cannot read the style file {}: {source}", path.display()))]
    StyleFile {
        path: PathBuf,
        source: typst::diag::FileError,
    },

    #[snafu(display("style has no letter entry {}; use a style with letter.typ (such as pragmatic)", path.display()))]
    StyleMissingLetter { path: PathBuf },

    #[snafu(display("cannot serialize the letter for the style"))]
    SerializeLetter { source: serde_json::Error },

    #[snafu(display("cannot serialize the resume for the style: {source}"))]
    SerializeResume { source: serde_json::Error },

    #[snafu(display("cannot read font directory {}: {source}", path.display()))]
    FontDirectory { path: PathBuf, source: std::io::Error },

    #[snafu(display("scanning {origin} fonts panicked: {message}"))]
    FontScanPanicked { origin: FontOrigin, message: String },

    #[snafu(display("the style's `<{label}>` metadata is malformed: {detail}"))]
    Probe { label: &'static str, detail: String },

    #[snafu(display("cannot start the typesetting thread: {source}"))]
    SpawnTypesetThread { source: std::io::Error },

    #[snafu(display("typesetting panicked during {stage}: {message}"))]
    TypesetPanicked { stage: Stage, message: String },

    #[snafu(display("cannot read {}: {source}", path.display()))]
    ReadInput { path: PathBuf, source: std::io::Error },

    #[snafu(display("cannot read {} as UTF-8: {source}", path.display()))]
    InputUtf8 {
        path: PathBuf,
        source: std::string::FromUtf8Error,
    },

    #[snafu(display("cannot create the output directory {}: {source}", path.display()))]
    CreateOutputDir { path: PathBuf, source: std::io::Error },

    #[snafu(display("cannot write {}: {source}", path.display()))]
    WriteOutput { path: PathBuf, source: std::io::Error },

    /// The target is open in another program, which on Windows stops it being replaced.
    #[snafu(display("{} is in use by another program: {source}", path.display()))]
    OutputLocked { path: PathBuf, source: std::io::Error },

    #[snafu(display("{} is read-only", path.display()))]
    OutputReadOnly { path: PathBuf },

    #[snafu(display("{} is a directory", path.display()))]
    OutputIsDirectory { path: PathBuf },

    #[snafu(display("{} already exists", path.display()))]
    OutputExists { path: PathBuf },

    /// A file being edited no longer holds what was read from it.
    #[snafu(display("{} changed while it was being fixed", path.display()))]
    InputChanged { path: PathBuf },

    /// The operating system would not report changes to a path, so `watch` cannot see edits there.
    #[snafu(display("cannot watch {} for changes: {source}", path.display()))]
    Watch { path: PathBuf, source: notify::Error },

    /// The path to watch is a file system root or otherwise has no directory to watch beside it.
    #[snafu(display("{} is not a file that can be watched: it has no directory or no name", path.display()))]
    WatchInput { path: PathBuf },

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

impl Error {
    /// What the user can do about this error, when there is something specific.
    #[must_use]
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::OutputLocked { .. } => Some("close the PDF viewer or use -o <DIR>"),
            Self::OutputReadOnly { .. } => Some("make the file writable or delete it, or use -o <DIR>"),
            Self::OutputIsDirectory { .. } => Some("remove it or use -o <DIR> to write elsewhere"),
            Self::OutputExists { .. } => Some("use --force to replace it"),
            Self::InputChanged { .. } => Some("the file was edited meanwhile; nothing was changed, run doctor again"),
            _ => None,
        }
    }
}

/// `; available styles: mono, pragmatic`, or nothing when no style is known.
fn available_text(available: &[StyleName]) -> String {
    if available.is_empty() {
        return String::new();
    }
    let names: Vec<&str> = available.iter().map(AsRef::as_ref).collect();
    format!("; available styles: {}", names.join(", "))
}

fn errors_in(diagnostics: &[Diagnostic]) -> usize {
    diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_some;

    fn path() -> PathBuf {
        PathBuf::from("out/Resume.pdf")
    }

    fn io() -> std::io::Error {
        std::io::Error::other("the disk said no")
    }

    #[test]
    fn a_locked_output_says_to_close_the_viewer_or_write_elsewhere() {
        let hint = assert_some!(
            Error::OutputLocked {
                path: path(),
                source: io()
            }
            .hint()
        );
        assert!(hint.contains("close the PDF viewer") && hint.contains("-o"), "{hint}");
    }

    #[test]
    fn an_output_that_cannot_be_replaced_says_what_to_do_about_it() {
        let read_only = assert_some!(Error::OutputReadOnly { path: path() }.hint());
        assert!(
            read_only.contains("read-only") || read_only.contains("writable"),
            "{read_only}"
        );
        let directory = assert_some!(Error::OutputIsDirectory { path: path() }.hint());
        assert!(directory.contains("-o"), "{directory}");
        let exists = assert_some!(Error::OutputExists { path: path() }.hint());
        assert!(exists.contains("--force"), "{exists}");
    }

    #[test]
    fn a_failure_with_nothing_specific_to_do_has_no_hint() {
        assert!(
            Error::WriteOutput {
                path: path(),
                source: io()
            }
            .hint()
            .is_none()
        );
        assert!(
            Error::CreateOutputDir {
                path: path(),
                source: io()
            }
            .hint()
            .is_none()
        );
    }

    #[test]
    fn output_errors_name_the_path_and_carry_the_cause() {
        let locked = Error::OutputLocked {
            path: path(),
            source: io(),
        }
        .to_string();
        assert!(
            locked.contains("Resume.pdf") && locked.contains("the disk said no"),
            "{locked}"
        );
        let write = Error::WriteOutput {
            path: path(),
            source: io(),
        }
        .to_string();
        assert!(
            write.contains("Resume.pdf") && write.contains("the disk said no"),
            "{write}"
        );
        let dir = Error::CreateOutputDir {
            path: path(),
            source: io(),
        }
        .to_string();
        assert!(
            dir.contains("output directory") && dir.contains("the disk said no"),
            "{dir}"
        );
    }
}

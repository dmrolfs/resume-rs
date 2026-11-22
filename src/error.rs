//! ABOUTME: Operational errors: style lookup, writing output, thread failures, Typst compile and export.
//! ABOUTME: Content problems are `Diagnostic`s; `Compile` and `ExportPdf` carry them so one run reports all.

use std::path::PathBuf;

use snafu::Snafu;
use strum::Display;

use crate::diagnostics::{Diagnostic, Severity};
use crate::model::StyleName;
use crate::model::options::FontOrigin;

/// Why a spec's file name cannot be the name and the identity of its PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum StemProblem {
    #[strum(to_string = "it has no file name")]
    NoName,
    #[strum(to_string = "its name is not valid UTF-8")]
    NotUtf8,
    #[strum(to_string = "its name starts with a dot, so the PDF would be a hidden file")]
    Hidden,
    #[strum(to_string = "its name ends in `.drafts`, which is how the preview of the spec before it is named")]
    DraftsSuffix,
}

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

    #[snafu(display("{} is not a regular file; a resume is read from a file, not a directory or a pipe", path.display()))]
    InputNotAFile { path: PathBuf },

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

    /// A position spec or a profile could not be loaded or resolved.
    #[snafu(display("{source}"))]
    Spec {
        #[snafu(source(from(crate::spec::SpecError, Box::new)))]
        source: Box<crate::spec::SpecError>,
    },

    /// Settings typed beside `--spec` that the policy refuses. The command line is checked first, so this is
    /// reached only by a caller that built its arguments without that check.
    #[snafu(display("{source}"))]
    OverridesRefused { source: crate::spec::Refused },

    /// A spec's file name cannot name its PDF.
    #[snafu(display("{} cannot name a PDF: {problem}", path.display()))]
    SpecName { path: PathBuf, problem: StemProblem },

    /// A spec's `content` file could not be read. `at` is the `content =` line, because the path was not typed.
    #[snafu(display("{at}: the `content` file cannot be used: {source}"))]
    SpecContent {
        at: crate::spec::Located,
        #[snafu(source(from(Error, Box::new)))]
        source: Box<Error>,
    },

    /// A choice written in a spec or a profile led to this error; `at` is every place that wrote the choice.
    #[snafu(display("{source} (chosen at {})", located_list(at)))]
    TracedToChoice {
        at: Vec<crate::spec::Located>,
        #[snafu(source(from(Error, Box::new)))]
        source: Box<Error>,
    },

    /// The operating system would not report changes to a path, so `watch` cannot see edits there.
    #[snafu(display("cannot watch {} for changes: {source}", path.display()))]
    Watch { path: PathBuf, source: notify::Error },

    /// The path to watch is a file system root or otherwise has no directory to watch beside it.
    #[snafu(display("{} is not a file that can be watched: it has no directory or no name", path.display()))]
    WatchInput { path: PathBuf },

    /// The selection asked for something the resume cannot give, such as a wording that is still a draft.
    #[snafu(display("{source}"))]
    Select { source: crate::select::SelectError },

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
            Self::Select { .. } => Some("pick a ready wording, finish the draft, or build with --drafts to preview it"),
            Self::Spec { source } => source.hint(),
            Self::SpecContent { source, .. } | Self::TracedToChoice { source, .. } => source.hint(),
            Self::SpecName { .. } => Some("rename the spec file"),
            Self::OverridesRefused { .. } => Some("add --allow-overrides to replace the spec's choices"),
            _ => None,
        }
    }
}

/// `positions/acme.toml:9:5, profiles/base.toml:2:8`: every place a choice was written.
fn located_list(at: &[crate::spec::Located]) -> String {
    at.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
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
    fn a_spec_error_prints_as_itself_and_lends_its_hint() {
        let inner = crate::spec::SpecError::MissingProfile {
            at: crate::spec::Located {
                path: PathBuf::from("positions/acme.toml").into(),
                line: 4,
                column: 1,
            },
            name: "nope".into(),
            searched: PathBuf::from("profiles"),
            available: vec!["principal".into()],
        };
        let text = inner.to_string();
        let error = Error::Spec {
            source: Box::new(inner),
        };
        assert_eq!(error.to_string(), text);
        assert_some!(error.hint());
    }

    #[test]
    fn the_errors_that_trace_a_spec_say_what_to_do() {
        let at = crate::spec::Located {
            path: PathBuf::from("positions/acme.toml").into(),
            line: 4,
            column: 9,
        };
        let name = Error::SpecName {
            path: PathBuf::from("positions/.toml"),
            problem: StemProblem::Hidden,
        };
        assert_eq!(name.hint(), Some("rename the spec file"));
        let refused = Error::OverridesRefused {
            source: crate::spec::Refused {
                flags: vec![crate::spec::OverrideFlag::Tags],
            },
        };
        assert_eq!(
            refused.hint(),
            Some("add --allow-overrides to replace the spec's choices")
        );
        // A wrapper lends the hint of the error it wraps, and names the places in its own text.
        let wrapped = Error::TracedToChoice {
            at: vec![at.clone(), at],
            source: Box::new(Error::OutputExists {
                path: PathBuf::from("x.pdf"),
            }),
        };
        assert_eq!(wrapped.hint(), Some("use --force to replace it"));
        assert!(
            wrapped
                .to_string()
                .ends_with("(chosen at positions/acme.toml:4:9, positions/acme.toml:4:9)"),
            "{wrapped}"
        );
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

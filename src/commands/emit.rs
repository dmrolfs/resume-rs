//! ABOUTME: Writes findings, summary lines, and errors to the injected streams.
//! ABOUTME: Human text goes to stderr and JSON lines to stdout; a closed pipe ends the output quietly.

use std::io;
use std::path::Path;

use crate::diagnostics::termcolor::{Color, ColorSpec};
use crate::diagnostics::{Diagnostic, RenderError, Sources, Summary, emit_human, emit_human_with_notes, emit_json};
use crate::error::Error;
use crate::model::options::OutputFormat;
use crate::typeset::fonts::FontResolution;

use super::Io;
use super::report::{BuildLine, FontsLine};

/// Everything a command says, in the format and to the stream the user asked for.
pub(super) struct Reporter<'a, 'io> {
    format: OutputFormat,
    cwd: &'a Path,
    io: &'a mut Io<'io>,
}

impl<'a, 'io> Reporter<'a, 'io> {
    pub(super) fn new(format: OutputFormat, cwd: &'a Path, io: &'a mut Io<'io>) -> Self {
        Self { format, cwd, io }
    }

    /// An empty source database whose paths are shown relative to the current directory.
    pub(super) fn sources<'t>(&self) -> Sources<'t> {
        Sources::new(Some(self.cwd.to_path_buf()))
    }

    /// One summary line, with its marker in color when color is on. A failure to write to stderr has nowhere
    /// to be reported, so it is dropped.
    pub(super) fn summary(&mut self, line: &BuildLine<'_>) {
        let color = if line.marker() == "ok" {
            Color::Green
        } else {
            Color::Red
        };
        let stderr = &mut *self.io.stderr;
        let _ = stderr.set_color(ColorSpec::new().set_fg(Some(color)).set_bold(true));
        let _ = write!(stderr, "{}", line.marker());
        let _ = stderr.reset();
        let _ = writeln!(stderr, " {line}");
    }

    /// What a command prints as its product (the list of `resume styles`): to stdout, stopping quietly when the
    /// reader has closed the pipe.
    pub(super) fn product(&mut self, text: &str) {
        if let Err(error) = self.io.stdout.write_all(text.as_bytes())
            && error.kind() != io::ErrorKind::BrokenPipe
        {
            let _ = writeln!(self.io.stderr, "error: cannot write to stdout: {error}");
        }
    }

    /// Sends what has been written so far, for a command that runs for a long time: the real streams are
    /// buffered and flushed when the process ends, so a loop that does not call this shows nothing until then.
    /// A closed pipe is the reader's choice; a failure has nowhere to be reported, as for the other lines.
    ///
    /// Returns `false` when stderr's reader has gone (a closed pipe), so a command that would otherwise run
    /// unseen can stop. Any other failure has nowhere to be reported, and the command goes on.
    pub(super) fn flush(&mut self) -> bool {
        let _ = self.io.stdout.flush();
        !matches!(self.io.stderr.flush(), Err(error) if error.kind() == io::ErrorKind::BrokenPipe)
    }

    /// One non-diagnostic JSON-lines record on stdout, such as a build's `selection` record.
    pub(super) fn record(&mut self, record: &impl serde::Serialize) {
        if let Err(error) = crate::diagnostics::emit_json_record(&mut *self.io.stdout, record) {
            self.render_failed(&error);
        }
    }

    /// One line of progress or result, on stderr like the summary lines.
    pub(super) fn line(&mut self, text: &str) {
        let _ = writeln!(self.io.stderr, "{text}");
    }

    /// The fonts line, indented under the summary; nothing for a style that declares no font roles.
    pub(super) fn fonts(&mut self, fonts: &[FontResolution]) {
        let text = FontsLine(fonts).to_string();
        if !text.is_empty() {
            let _ = writeln!(self.io.stderr, "  {text}");
        }
    }

    /// The findings, as excerpts on stderr or as JSON lines on stdout.
    pub(super) fn diagnostics(&mut self, sources: &Sources<'_>, diagnostics: &[&Diagnostic]) {
        let rendered = match self.format {
            OutputFormat::Human => emit_human(&mut *self.io.stderr, sources, diagnostics.iter().copied()),
            OutputFormat::Json => emit_json(&mut *self.io.stdout, sources, diagnostics.iter().copied()),
        };
        if let Err(error) = rendered {
            self.render_failed(&error);
        }
    }

    /// One line of the doctor report, such as a category header, on stderr.
    pub(super) fn report_line(&mut self, text: &str) {
        let _ = writeln!(self.io.stderr, "{text}");
    }

    /// The closing line of one file's report: a marker, the counts, and what `--fix` could do about them.
    pub(super) fn doctor_line(&mut self, summary: Summary, note: super::doctor::FixableNote, failed: bool) {
        let stderr = &mut *self.io.stderr;
        let (marker, color) = if failed {
            ("error", Color::Red)
        } else {
            ("ok", Color::Green)
        };
        let _ = writeln!(stderr);
        let _ = stderr.set_color(ColorSpec::new().set_fg(Some(color)).set_bold(true));
        let _ = write!(stderr, "{marker}");
        let _ = stderr.reset();
        let how = if summary.fixable > 0 && note == super::doctor::FixableNote::Offer {
            " with `resume doctor --fix`"
        } else {
            ""
        };
        let _ = writeln!(stderr, " {summary}{how}");
    }

    /// A category header, in bold when color is on.
    pub(super) fn header(&mut self, text: &str) {
        let stderr = &mut *self.io.stderr;
        let _ = stderr.set_color(ColorSpec::new().set_bold(true));
        let _ = write!(stderr, "{text}");
        let _ = stderr.reset();
        let _ = writeln!(stderr);
    }

    /// Findings with the advice notes after each, as excerpts on stderr; in JSON mode the findings only, as
    /// JSON lines on stdout, because a consumer groups by `category` and reads the advice from `--explain`.
    pub(super) fn diagnostics_with_notes(&mut self, sources: &Sources<'_>, shown: &[(&Diagnostic, &[String])]) {
        let rendered = match self.format {
            OutputFormat::Human => emit_human_with_notes(&mut *self.io.stderr, sources, shown.iter().copied()),
            OutputFormat::Json => emit_json(&mut *self.io.stdout, sources, shown.iter().map(|(d, _)| *d)),
        };
        if let Err(error) = rendered {
            self.render_failed(&error);
        }
    }

    /// The text of a command's product on stdout, such as a unified diff or an explained code.
    pub(super) fn stderr_text(&mut self, text: &str) {
        let _ = write!(self.io.stderr, "{text}");
    }

    /// An operational error. A compile or export failure carries findings, which are shown with the style
    /// files they point into; any other error is one line, with its hint.
    pub(super) fn error(&mut self, error: &Error) {
        match error {
            Error::Compile { diagnostics, sources } | Error::ExportPdf { diagnostics, sources } => {
                let mut known = self.sources();
                for file in sources {
                    known.insert(&file.path, &file.text);
                }
                let findings: Vec<&Diagnostic> = diagnostics.iter().collect();
                self.diagnostics(&known, &findings);
            }
            other => {
                let _ = writeln!(self.io.stderr, "error: {other}");
                if let Some(hint) = other.hint() {
                    let _ = writeln!(self.io.stderr, "  = help: {hint}");
                }
            }
        }
    }

    /// A closed pipe (`resume ... | head`) is the reader's choice, not a failure; anything else is reported.
    fn render_failed(&mut self, error: &RenderError) {
        let closed = matches!(error, RenderError::Write { source } if source.kind() == io::ErrorKind::BrokenPipe);
        if !closed {
            let _ = writeln!(self.io.stderr, "error: {error}");
        }
    }
}

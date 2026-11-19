//! ABOUTME: Writes findings, summary lines, and errors to the injected streams.
//! ABOUTME: Human text goes to stderr and JSON lines to stdout; a closed pipe ends the output quietly.

use std::io;
use std::path::Path;

use crate::diagnostics::termcolor::{Color, ColorSpec};
use crate::diagnostics::{Diagnostic, RenderError, Sources, emit_human, emit_json};
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

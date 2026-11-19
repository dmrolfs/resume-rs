//! ABOUTME: The text of a build's summary lines, as pure functions of their data.
//! ABOUTME: ASCII only, so legacy Windows consoles, logs, and pasted text read the same; elapsed time is passed in.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use crate::diagnostics::Summary;
use crate::typeset::fonts::FontResolution;

/// What happened to one input file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Built<'a> {
    /// A PDF was written.
    Wrote {
        /// Where it went.
        path: &'a Path,
        /// The style it was built with.
        style: &'a str,
        /// How many pages it has.
        pages: usize,
    },
    /// A dry run: the PDF would be written.
    WouldWrite {
        /// Where it would go.
        path: &'a Path,
        /// The style it was built with.
        style: &'a str,
        /// How many pages it has.
        pages: usize,
        /// A file at the target would be replaced.
        replaces: bool,
        /// The directory that would be created.
        creates_dir: Option<&'a Path>,
    },
    /// The input did not build: no PDF was written.
    NoPdf {
        /// The input file.
        input: &'a Path,
    },
}

/// The one summary line of an input file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildLine<'a> {
    /// What happened.
    pub built: Built<'a>,
    /// The findings counted, as the user sees them.
    pub summary: Summary,
    /// How long it took, from reading the input to the end of the write.
    pub elapsed: Duration,
}

impl BuildLine<'_> {
    /// `error` when the PDF could not be produced or any finding is an error.
    #[must_use]
    pub fn marker(&self) -> &'static str {
        if matches!(self.built, Built::NoPdf { .. }) || self.summary.has_errors() {
            "error"
        } else {
            "ok"
        }
    }

    /// The time as a person reads it: whole milliseconds, at least one (`84ms`, `1s 234ms`).
    fn time(&self) -> impl fmt::Display {
        let millis = u64::try_from(self.elapsed.as_millis()).unwrap_or(u64::MAX).max(1);
        humantime::format_duration(Duration::from_millis(millis))
    }
}

impl fmt::Display for BuildLine<'_> {
    /// The line without its marker, so a caller can color the marker.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let summary = &self.summary;
        match self.built {
            Built::Wrote { path, style, pages } => write!(
                f,
                "{}  {style}  {}  {summary}  built in {}",
                path.display(),
                pages_of(pages),
                self.time()
            ),
            Built::WouldWrite {
                path,
                style,
                pages,
                replaces,
                creates_dir,
            } => {
                let effect = match (creates_dir, replaces) {
                    (Some(dir), _) => format!("creates {}", dir.display()),
                    (None, true) => "overwrites existing".to_string(),
                    (None, false) => "new file".to_string(),
                };
                write!(
                    f,
                    "would write {} ({}, {effect})  {style}  {summary}  checked in {}",
                    path.display(),
                    pages_of(pages),
                    self.time()
                )
            }
            Built::NoPdf { input } => write!(f, "{}  {summary}  no PDF written", input.display()),
        }
    }
}

fn pages_of(pages: usize) -> String {
    format!("{pages} page{}", if pages == 1 { "" } else { "s" })
}

/// The fonts each role resolved to, on one line; empty when the style declares no font roles.
#[derive(Debug, Clone, Copy)]
pub struct FontsLine<'a>(pub &'a [FontResolution]);

impl fmt::Display for FontsLine<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return Ok(());
        }
        f.write_str("fonts: ")?;
        for (position, font) in self.0.iter().enumerate() {
            if position > 0 {
                f.write_str("; ")?;
            }
            match font {
                FontResolution::Preferred { role, family, origin } => write!(f, "{role} {family} ({origin})")?,
                FontResolution::Fallback {
                    role,
                    preferred,
                    family,
                    ..
                } => write!(f, "{role} {family} (fallback for {preferred})")?,
                FontResolution::Missing {
                    role,
                    preferred,
                    fallback,
                } => write!(f, "{role} MISSING (neither {preferred} nor {fallback})")?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::options::FontOrigin;
    use crate::typeset::fonts::Metrics;
    use pretty_assertions::assert_eq;

    const WARNING_ONLY: Summary = Summary {
        errors: 0,
        warnings: 1,
        notes: 0,
        fixable: 0,
    };
    const ONE_ERROR: Summary = Summary {
        errors: 1,
        warnings: 0,
        notes: 0,
        fixable: 0,
    };

    fn line<'a>(built: Built<'a>, summary: Summary, ms: u64) -> BuildLine<'a> {
        BuildLine {
            built,
            summary,
            elapsed: Duration::from_millis(ms),
        }
    }

    fn wrote(pages: usize) -> Built<'static> {
        Built::Wrote {
            path: Path::new("out/Resume-Org-Leader.pdf"),
            style: "pragmatic",
            pages,
        }
    }

    fn shown(line: &BuildLine<'_>) -> String {
        format!("{} {line}", line.marker())
    }

    // ========================================================================
    // The summary line
    // ========================================================================

    #[test]
    fn a_written_pdf_is_one_line_with_its_pages_findings_and_time() {
        assert_eq!(
            shown(&line(wrote(2), WARNING_ONLY, 84)),
            "ok out/Resume-Org-Leader.pdf  pragmatic  2 pages  0 errors, 1 warning  built in 84ms"
        );
    }

    #[test]
    fn one_page_is_singular() {
        assert!(shown(&line(wrote(1), WARNING_ONLY, 84)).contains("  1 page  "));
    }

    #[test]
    fn a_pdf_written_with_an_error_is_marked_error() {
        // A layout error still writes the PDF (the owner's decision), and the line says it failed.
        assert_eq!(
            shown(&line(wrote(4), ONE_ERROR, 90)),
            "error out/Resume-Org-Leader.pdf  pragmatic  4 pages  1 error, 0 warnings  built in 90ms"
        );
    }

    #[test]
    fn an_input_that_did_not_build_says_no_pdf_was_written() {
        let built = Built::NoPdf {
            input: Path::new("drafts/Resume.md"),
        };
        let errors = Summary {
            errors: 2,
            ..Summary::default()
        };
        assert_eq!(
            shown(&line(built, errors, 3)),
            "error drafts/Resume.md  2 errors, 0 warnings  no PDF written"
        );
    }

    #[test]
    fn a_dry_run_says_would_write_and_what_it_would_replace_or_create() {
        let would = |replaces, creates_dir| Built::WouldWrite {
            path: Path::new("out/X.pdf"),
            style: "pragmatic",
            pages: 2,
            replaces,
            creates_dir,
        };
        let shown = |built| shown(&line(built, WARNING_ONLY, 80));
        assert_eq!(
            shown(would(true, None)),
            "ok would write out/X.pdf (2 pages, overwrites existing)  pragmatic  0 errors, 1 warning  checked in 80ms"
        );
        assert_eq!(
            shown(would(false, None)),
            "ok would write out/X.pdf (2 pages, new file)  pragmatic  0 errors, 1 warning  checked in 80ms"
        );
        assert_eq!(
            shown(would(false, Some(Path::new("out")))),
            "ok would write out/X.pdf (2 pages, creates out)  pragmatic  0 errors, 1 warning  checked in 80ms"
        );
    }

    #[test]
    fn build_time_is_whole_milliseconds_and_at_least_one() {
        let time = |elapsed: Duration| {
            let built = BuildLine {
                built: wrote(1),
                summary: Summary::default(),
                elapsed,
            };
            shown(&built)
                .rsplit("built in ")
                .next()
                .map(str::to_string)
                .unwrap_or_default()
        };
        assert_eq!(time(Duration::new(0, 84_123_456)), "84ms");
        assert_eq!(time(Duration::from_micros(40)), "1ms");
        assert_eq!(time(Duration::ZERO), "1ms");
        assert_eq!(time(Duration::from_millis(1_234)), "1s 234ms");
    }

    // ========================================================================
    // The fonts line
    // ========================================================================

    fn preferred(role: &str, family: &str, origin: FontOrigin) -> FontResolution {
        FontResolution::Preferred {
            role: role.into(),
            family: family.into(),
            origin,
        }
    }

    fn fallback(role: &str, preferred: &str, family: &str) -> FontResolution {
        FontResolution::Fallback {
            role: role.into(),
            preferred: preferred.into(),
            family: family.into(),
            origin: FontOrigin::Bundled,
            metrics: Metrics::Compatible,
        }
    }

    #[test]
    fn the_fonts_line_says_where_each_role_got_its_font() {
        let fonts = [
            preferred("body", "Calibri", FontOrigin::System),
            fallback("display", "Georgia", "Gelasio"),
            FontResolution::Missing {
                role: "mono".into(),
                preferred: "Arial".into(),
                fallback: "Arimo".into(),
            },
        ];
        assert_eq!(
            FontsLine(&fonts).to_string(),
            "fonts: body Calibri (system); display Gelasio (fallback for Georgia); mono MISSING (neither Arial nor Arimo)"
        );
    }

    #[test]
    fn a_font_from_a_font_path_or_bundled_is_named_by_its_origin() {
        let fonts = [
            preferred("body", "Calibri", FontOrigin::FontPath),
            preferred("mono", "Arimo", FontOrigin::Bundled),
        ];
        assert_eq!(
            FontsLine(&fonts).to_string(),
            "fonts: body Calibri (font-path); mono Arimo (bundled)"
        );
    }

    #[test]
    fn a_style_with_no_font_roles_has_no_fonts_line() {
        assert_eq!(FontsLine(&[]).to_string(), "");
    }

    #[test]
    fn the_lines_are_ascii_when_their_inputs_are() {
        let fonts = [fallback("body", "Calibri", "Carlito")];
        let text = format!("{}{}", shown(&line(wrote(2), WARNING_ONLY, 84)), FontsLine(&fonts));
        assert!(text.contains("built in") && text.contains("fonts:"), "{text}");
        assert!(text.is_ascii(), "{text}");
    }
}

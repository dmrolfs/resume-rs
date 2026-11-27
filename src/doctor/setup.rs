//! ABOUTME: Setup checks: the style, the fonts, and the output location, reported as findings.
//! ABOUTME: Reads only; the output check is the Writer's own dry run, so nothing is created to test it.

use std::path::Path;

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::error::Error;
use crate::output::{Overwrite, Planned, WriteMode, Writer, Written};

/// A Setup finding for an error the style, fonts, or output check raised, or `None` when the error is not a
/// setup problem a person can act on through advice (a panic, say), which the caller reports as an error line.
///
/// One mapping, exhaustive over the setup errors, so doctor and build cannot disagree about what a problem is.
#[must_use]
pub fn finding_of(error: &Error) -> Option<Diagnostic> {
    let code = match error {
        Error::UnknownStyle { .. } => DiagnosticCode::UnknownStyle,
        Error::StyleMissingMain { .. } => DiagnosticCode::StyleMissingMain,
        Error::StyleRoot { .. }
        | Error::StyleOutsideRoot { .. }
        | Error::StyleDirectory { .. }
        | Error::StyleFile { .. } => DiagnosticCode::StylesDirUnusable,
        Error::FontDirectory { .. } => DiagnosticCode::FontPathUnusable,
        Error::CreateOutputDir { .. }
        | Error::OutputIsDirectory { .. }
        | Error::OutputReadOnly { .. }
        | Error::WriteOutput { .. }
        | Error::OutputLocked { .. }
        | Error::OutputExists { .. } => DiagnosticCode::OutputNotWritable,
        Error::ReportSource { .. }
        | Error::OutputInputCollision { .. }
        | Error::ReadInput { .. }
        | Error::InputUtf8 { .. }
        | Error::InputNotAFile { .. }
        | Error::SpecContent { .. }
        | Error::SpecName { .. }
        | Error::OverridesRefused { .. }
        | Error::TracedToChoice { .. }
        | Error::InputChanged { .. }
        | Error::Spec { .. }
        | Error::Watch { .. }
        | Error::WatchInput { .. }
        | Error::SerializeResume { .. }
        | Error::SerializeLetter { .. }
        | Error::StyleMissingLetter { .. }
        | Error::FontScanPanicked { .. }
        | Error::Probe { .. }
        | Error::SpawnTypesetThread { .. }
        | Error::TypesetPanicked { .. }
        | Error::Compile { .. }
        | Error::ExportPdf { .. }
        | Error::LetterPicks { .. }
        | Error::Select { .. } => return None,
    };
    Some(Diagnostic {
        severity: code.default_severity(),
        code,
        message: error.to_string(),
        span: None,
        hints: error.hint().map(str::to_string).into_iter().collect(),
        fix: None,
    })
}

/// What the output check found, for the Setup line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// A PDF would be written; this says where and what that does to what is there.
    Usable(String),
    /// The location cannot be written, as a finding.
    Unusable(Box<Diagnostic>),
}

/// Checks that `target` could be written, by the Writer's dry run: the same checks as a real write, no bytes.
///
/// The check cannot see permissions or free space, so it says "usable", not "writable": only a write can tell.
#[must_use]
pub fn output(target: &Path, shown: &str) -> Output {
    let planned = Planned {
        target,
        bytes: &[],
        overwrite: Overwrite::Replace,
    };
    match Writer::new(WriteMode::DryRun).commit(planned) {
        Ok(Written::WouldWrite { replaces, creates_dir }) => Output::Usable(match (creates_dir, replaces) {
            (Some(dir), _) => format!("{} would be created", dir.display()),
            (None, true) => format!("{shown} would be replaced"),
            (None, false) => format!("{shown} is free"),
        }),
        Ok(Written::Wrote { .. }) => unreachable!("a dry run never writes"),
        Err(error) => match finding_of(&error) {
            Some(finding) => Output::Unusable(Box::new(finding)),
            None => Output::Unusable(Box::new(Diagnostic {
                severity: DiagnosticCode::OutputNotWritable.default_severity(),
                code: DiagnosticCode::OutputNotWritable,
                message: error.to_string(),
                span: None,
                hints: Vec::new(),
                fix: None,
            })),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Severity;
    use crate::model::StyleName;
    use claims::{assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn io() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::PermissionDenied)
    }

    #[test]
    fn each_setup_error_maps_to_its_code_as_an_error() {
        let name = assert_ok!(StyleName::try_new("nope"));
        let cases = [
            (
                Error::UnknownStyle {
                    name,
                    searched: PathBuf::from("styles"),
                    available: Vec::new(),
                },
                DiagnosticCode::UnknownStyle,
            ),
            (
                Error::StyleMissingMain {
                    dir: PathBuf::from("styles/x"),
                },
                DiagnosticCode::StyleMissingMain,
            ),
            (
                Error::StyleRoot {
                    path: PathBuf::from("styles"),
                    source: io(),
                },
                DiagnosticCode::StylesDirUnusable,
            ),
            (
                Error::StyleOutsideRoot {
                    dir: PathBuf::from("styles/x"),
                },
                DiagnosticCode::StylesDirUnusable,
            ),
            (
                Error::FontDirectory {
                    path: PathBuf::from("fonts"),
                    source: io(),
                },
                DiagnosticCode::FontPathUnusable,
            ),
            (
                Error::OutputIsDirectory {
                    path: PathBuf::from("out/a.pdf"),
                },
                DiagnosticCode::OutputNotWritable,
            ),
        ];
        for (error, code) in cases {
            let finding = assert_some!(finding_of(&error), "{error}");
            assert_eq!(finding.code, code, "{error}");
            assert_eq!(
                finding.severity,
                Severity::Error,
                "an unusable setup fails the run: {error}"
            );
            assert_eq!(finding.message, error.to_string());
            assert!(finding.span.is_none() && finding.fix.is_none());
        }
    }

    #[test]
    fn an_output_error_keeps_its_hint() {
        let error = Error::OutputIsDirectory {
            path: PathBuf::from("out/a.pdf"),
        };
        let finding = assert_some!(finding_of(&error));
        assert_eq!(finding.hints, ["remove it or use -o <DIR> to write elsewhere"]);
    }

    #[test]
    fn a_panic_or_an_unreadable_input_is_not_a_setup_finding() {
        let error = Error::ReadInput {
            path: PathBuf::from("r.md"),
            source: io(),
        };
        assert!(finding_of(&error).is_none());
        let panic = Error::FontScanPanicked {
            origin: crate::model::options::FontOrigin::System,
            message: "boom".into(),
        };
        assert!(finding_of(&panic).is_none());
    }

    #[test]
    fn a_new_output_location_is_usable_and_says_what_would_happen() {
        let dir = assert_ok!(tempfile::tempdir());
        let target = dir.path().join("out").join("r.pdf");
        let found = output(&target, "out/r.pdf");
        assert_eq!(
            found,
            Output::Usable(format!("{} would be created", dir.path().join("out").display()))
        );
        assert!(!dir.path().join("out").exists(), "the check created a directory");
    }

    #[test]
    fn an_existing_and_a_free_target_say_so() {
        let dir = assert_ok!(tempfile::tempdir());
        let target = dir.path().join("r.pdf");
        assert_eq!(output(&target, "r.pdf"), Output::Usable("r.pdf is free".into()));
        assert_ok!(std::fs::write(&target, b"old"));
        assert_eq!(
            output(&target, "r.pdf"),
            Output::Usable("r.pdf would be replaced".into())
        );
        assert_eq!(assert_ok!(std::fs::read(&target)), b"old");
    }

    #[test]
    fn an_output_directory_that_is_a_file_is_unusable() {
        let dir = assert_ok!(tempfile::tempdir());
        let file = dir.path().join("out");
        assert_ok!(std::fs::write(&file, b"x"));
        let Output::Unusable(finding) = output(&file.join("r.pdf"), "out/r.pdf") else {
            panic!("a file in the way must be unusable");
        };
        assert_eq!(finding.code, DiagnosticCode::OutputNotWritable);
        assert_eq!(finding.severity, Severity::Error);
    }

    #[test]
    fn a_directory_where_the_pdf_goes_is_unusable() {
        let dir = assert_ok!(tempfile::tempdir());
        let target = dir.path().join("r.pdf");
        assert_ok!(std::fs::create_dir(&target));
        assert!(matches!(output(&target, "r.pdf"), Output::Unusable(_)));
    }
}

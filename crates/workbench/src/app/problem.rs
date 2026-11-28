//! Display rows retain engine locations, without parsing human error messages.
use resume::{
    diagnostics::{Diagnostic, DiagnosticCode, Severity},
    spec::{SpecError, error::Located},
};
use std::{
    ops::Range,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    Bytes(Range<usize>),
    LineColumn { line: usize, column: usize },
    File,
}

impl Location {
    pub fn offset(&self, text: &str) -> Option<usize> {
        let offset = match self {
            Self::Bytes(range) => range.start.min(text.len()),
            Self::LineColumn { line, column } => {
                resume::diagnostics::SourceText::new(PathBuf::new(), resume::diagnostics::SourceKind::Spec, text)
                    .byte_offset(*line, *column)
            }
            Self::File => return None,
        };
        Some(text.floor_char_boundary(offset.min(text.len())))
    }

    pub fn position(&self, text: &str) -> lsp_types::Position {
        let offset = self.offset(text).unwrap_or(0);
        let prefix = &text[..offset];
        lsp_types::Position::new(
            prefix.bytes().filter(|b| *b == b'\n').count() as u32,
            prefix.rsplit('\n').next().unwrap_or("").chars().count() as u32,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub file: PathBuf,
    pub severity: Severity,
    pub code: Option<DiagnosticCode>,
    pub message: String,
    pub location: Location,
}

impl Problem {
    pub fn file(path: &Path, message: impl Into<String>) -> Self {
        Self {
            file: path.to_owned(),
            severity: Severity::Error,
            code: None,
            message: message.into(),
            location: Location::File,
        }
    }

    pub fn diagnostic(fallback: &Path, d: &Diagnostic) -> Self {
        Self {
            file: d.span.as_ref().map_or_else(|| fallback.to_owned(), |s| s.file.clone()),
            severity: d.severity,
            code: Some(d.code),
            message: d.message.clone(),
            location: d
                .span
                .as_ref()
                .map_or(Location::File, |s| Location::Bytes(s.range.clone())),
        }
    }

    pub fn spec(path: &Path, error: &SpecError) -> Vec<Self> {
        if let SpecError::AdditionValidation {
            diagnostics, errors, ..
        } = error
        {
            return diagnostics
                .iter()
                .map(|d| Self::diagnostic(path, d))
                .chain(errors.iter().flat_map(|e| Self::spec(path, e)))
                .collect();
        }
        if let Some((snapshot, findings)) = error.content_findings() {
            return findings.iter().map(|d| Self::diagnostic(snapshot.path(), d)).collect();
        }
        if let SpecError::UnknownPicks { problems } = error {
            return problems
                .iter()
                .map(|p| Self::located(&p.at, p.kind.to_string()))
                .collect();
        }
        use SpecError::*;
        let at = match error {
            AdditionPlace { at }
            | Syntax { at, .. }
            | UnknownKey { at, .. }
            | WrongType { at, .. }
            | TextLength { at, .. }
            | UnsafeText { at, .. }
            | InvalidStyle { at, .. }
            | InvalidPaper { at, .. }
            | InvalidLevel { at, .. }
            | InvalidDate { at, .. }
            | InvalidUrl { at, .. }
            | RefusedUrl { at, .. }
            | MissingKey { at, .. }
            | InvalidName { at, .. }
            | EmptyOverride { at, .. }
            | InvalidPath { at, .. }
            | ForbiddenInProfile { at, .. }
            | InvalidKeyword { at, .. }
            | RequiresKey { at, .. }
            | DuplicateEntry { at, .. }
            | MissingProfile { at, .. }
            | ProfileOutsideRoot { at, .. }
            | ProfileNameCase { at, .. }
            | Cycle { at, .. }
            | ExtendsOrderConflict { at, .. }
            | TooDeep { at }
            | TooManyProfiles { at } => Some(at),
            ScopeConflict { core_only_at, .. } => Some(core_only_at),
            _ => None,
        };
        vec![if let Some(at) = at {
            Self::located(at, error.to_string())
        } else {
            let path = match error {
                Read { path, .. }
                | NotAFile { path }
                | TooLarge { path, .. }
                | NotUtf8 { path, .. }
                | NotASpec { path }
                | ConflictingSources { path } => path,
                ProfilesDirectoryMissing { spec, .. } => spec,
                ProfilesPathNotADirectory { given } => given,
                _ => path,
            };
            Self::file(path, error.to_string())
        }]
    }

    fn located(at: &Located, message: String) -> Self {
        Self {
            location: Location::LineColumn {
                line: at.line,
                column: at.column,
            },
            ..Self::file(&at.path, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Location, Problem};
    use claims::assert_err;
    use pretty_assertions::assert_eq;
    use std::path::Path;

    #[test]
    fn byte_and_character_locations_map_to_editor_scalar_columns_without_splitting_characters() {
        let text = "first\r\né😀x\r\n";
        assert_eq!(Location::Bytes(13..14).position(text), lsp_types::Position::new(1, 2));
        assert_eq!(
            Location::LineColumn { line: 2, column: 3 }.position(text),
            lsp_types::Position::new(1, 2)
        );
        assert_eq!(Location::Bytes(10..11).position(text), lsp_types::Position::new(1, 1));
    }

    #[test]
    fn bad_spec_has_real_location_without_invented_code() {
        let path = Path::new("position.toml");
        let error = assert_err!(resume::spec::layer::SpecLayer::from_text(
            path,
            "[build]\ncontent = 'a.md'\n[select]\nwrong = true\n"
        ));
        let problems = Problem::spec(path, &error);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].file, path);
        assert!(problems[0].code.is_none());
        assert!(matches!(
            problems[0].location,
            Location::LineColumn { line: 4, column: 1 }
        ));
    }
}

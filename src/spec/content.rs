//! ABOUTME: Validated addition content derived only from a literal in a retained spec snapshot.
//! ABOUTME: Findings point at the whole literal; decoded offsets are explanatory context, never edits.

use std::ops::Range;
use std::sync::Arc;

use serde::Deserialize;
use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::diagnostics::lines::LineIndex;
use crate::diagnostics::{Diagnostic, ItemOrigin, SourceKind, SourceText, display_text};
use crate::letter::LetterInline;
use crate::markdown::fragment::{self, FragmentIssue, Lowered, ResumeFragment, ResumeFragmentKind};

/// A rejected source reference, invalid TOML literal, or invalid Markdown content.
#[derive(Debug, Snafu)]
pub enum ContentError {
    #[snafu(display("addition content requires a spec snapshot, not {kind:?}"))]
    NotSpec { kind: SourceKind },
    #[snafu(display("addition source exceeds the {limit}-byte spec limit"))]
    TooLarge { limit: u64 },
    #[snafu(display("addition literal range {range:?} is outside the source or splits a UTF-8 character"))]
    Range { range: Range<usize> },
    #[snafu(display("addition content range must contain exactly one TOML string literal"))]
    NotLiteral,
    #[snafu(display("invalid addition TOML string: {}", display_text(source.message(), false)))]
    Decode { source: toml::de::Error },
    #[snafu(display("{}: invalid addition Markdown ({} findings)", snapshot.path().display(), diagnostics.len()))]
    Findings {
        snapshot: SourceText,
        diagnostics: Vec<Diagnostic>,
    },
}

impl ContentError {
    /// Findings and their original text, even when validation failed before a spec could be resolved.
    #[must_use]
    pub fn findings(&self) -> Option<(&SourceText, &[Diagnostic])> {
        match self {
            Self::Findings { snapshot, diagnostics } => Some((snapshot, diagnostics)),
            _ => None,
        }
    }
}

/// Validated content with its decoded authored text, source ownership, and warnings.
/// Construction derives all of these from one literal; none can be changed independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecContent<T> {
    value: T,
    markdown: Arc<str>,
    origin: ItemOrigin,
    source: SourceText,
    warnings: Vec<Diagnostic>,
}

impl SpecContent<ResumeFragment> {
    /// Reads and validates exactly one resume leaf from a TOML literal in `source`.
    pub fn resume(source: &SourceText, kind: ResumeFragmentKind, literal: Range<usize>) -> Result<Self, ContentError> {
        Self::parse(source, literal, |text| fragment::resume_fragment(kind, text))
    }
}

impl SpecContent<Vec<LetterInline>> {
    /// Reads and validates one letter body paragraph, preserving decoded placeholder ranges.
    pub fn letter(source: &SourceText, literal: Range<usize>) -> Result<Self, ContentError> {
        Self::parse(source, literal, fragment::letter_paragraph)
    }
}

impl<T> SpecContent<T> {
    #[must_use]
    pub fn value(&self) -> &T {
        &self.value
    }
    #[must_use]
    pub fn markdown(&self) -> &str {
        &self.markdown
    }
    #[must_use]
    pub fn origin(&self) -> &ItemOrigin {
        &self.origin
    }
    #[must_use]
    pub fn source(&self) -> &SourceText {
        &self.source
    }
    #[must_use]
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    fn parse(
        source: &SourceText,
        literal: Range<usize>,
        lower: impl FnOnce(&str) -> Result<Lowered<T>, Vec<FragmentIssue>>,
    ) -> Result<Self, ContentError> {
        let (markdown, origin) = decode(source, literal)?;
        match lower(&markdown) {
            Ok(lowered) => Ok(Self {
                warnings: locate(&markdown, &origin, lowered.warnings),
                value: lowered.value,
                markdown: markdown.into(),
                origin,
                source: source.clone(),
            }),
            Err(issues) => {
                assert!(!issues.is_empty(), "invalid Markdown always carries an error finding");
                FindingsSnafu {
                    snapshot: source.clone(),
                    diagnostics: locate(&markdown, &origin, issues),
                }
                .fail()
            }
        }
    }
}

fn decode(source: &SourceText, literal: Range<usize>) -> Result<(String, ItemOrigin), ContentError> {
    if source.kind() != SourceKind::Spec {
        return NotSpecSnafu { kind: source.kind() }.fail();
    }
    if source.text().len() as u64 > super::error::MAX_FILE_BYTES {
        return TooLargeSnafu {
            limit: super::error::MAX_FILE_BYTES,
        }
        .fail();
    }
    let origin = ItemOrigin::spec(source, literal.clone()).context(RangeSnafu { range: literal.clone() })?;
    let slice = &source.text()[literal];
    if !matches!(slice.as_bytes().first(), Some(b'\'' | b'"')) {
        return NotLiteralSnafu.fail();
    }
    // ValueDeserializer discards spans; a TOML envelope proves the whole slice is one literal.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Literal {
        value: toml::Spanned<String>,
    }
    const PREFIX: &str = "value = ";
    let parsed: Literal = toml::from_str(&format!("{PREFIX}{slice}")).context(DecodeSnafu)?;
    if parsed.value.span() != (PREFIX.len()..PREFIX.len() + slice.len()) {
        return NotLiteralSnafu.fail();
    }
    Ok((parsed.value.into_inner(), origin))
}

fn locate(markdown: &str, origin: &ItemOrigin, issues: Vec<FragmentIssue>) -> Vec<Diagnostic> {
    let lines = LineIndex::new(markdown);
    issues
        .into_iter()
        .map(|issue| {
            let at = lines.location(markdown, issue.decoded.start);
            let excerpt = lines.content(at.line - 1).map_or("", |range| &markdown[range]);
            let context = format!(
                "in decoded Markdown at {}:{}: {}",
                at.line,
                at.column,
                display_text(excerpt, false)
            );
            Diagnostic {
                severity: issue.severity,
                code: issue.code,
                message: issue.message,
                span: Some(origin.inline_span(issue.decoded)),
                hints: std::iter::once(context).chain(issue.hints).collect(),
                fix: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;

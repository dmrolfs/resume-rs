//! ABOUTME: Stable diagnostic identifiers, their categories, and the advice entry shape.
//! ABOUTME: Advice text is filled in per code by the parser and doctor tasks.

use serde::Serialize;

use super::Severity;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// What kind of finding a diagnostic is, and which stage reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Display, EnumIter)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Category {
    /// How the Markdown is written; reported by the parser.
    Format,
    /// What the resume says; reported by doctor.
    Content,
    /// How the rendered document looks; reported after render.
    Layout,
    /// The environment: styles, fonts, output location.
    Setup,
}

/// Stable, documented identifier for every finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Display, EnumString, IntoStaticStr, EnumIter)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCode {
    // Format (parser)
    MissingName,
    DuplicateName,
    ContentBeforeSection,
    RoleLineShape,
    RoleSeparator,
    EmptyRolePart,
    EraShape,
    EraOutsideRole,
    PeriodUnparseable,
    PeriodEndBeforeStart,
    PeriodHyphen,
    UnsupportedHtml,
    InvalidLink,
    UnsupportedLink,
    NestedList,
    RoleOrder,
    MidSentenceBold,
    Image,
    TooManyContactItems,
    MissingContact,
    PeriodPresentSpelling,
    UnsupportedMarkdown,
    HardBreak,
    TextTooLong,
    EmptyHeading,
    InputLimit,
    HeadingMarkerSpacing,
    PlaceholderUnresolved,
    AnnotationUnknownKeyword,
    AnnotationMalformed,
    AnnotationConflict,
    AnnotationDuplicate,
    AnnotationWithoutTarget,
    AnnotationDetached,
    AnnotationMisplaced,
    AlternateNotAllowed,
    AlternateMissingDefault,
    AlternateNotAdjacent,
    AlternateGroupReused,
    AlternateVariantRepeated,
    AlternatePriorityConflict,
    LetterMissingSection,
    LetterUnknownSection,
    LetterDuplicateSection,
    LetterSectionOrder,
    LetterSectionShape,
    LetterUnsupportedBlock,
    PlaceholderUnknown,
    PlaceholderMalformed,
    PlaceholderPosition,
    TextLimitExceeded,
    LetterSelectionRequired,
    // Content (doctor)
    LongBullet,
    UnquantifiedBullet,
    WeakOpener,
    FirstPerson,
    SloppyPhrase,
    LongSummary,
    EducationNotLast,
    OldRoleDetail,
    AlternateFactsDiffer,
    // Layout (render)
    PageLimitExceeded,
    HeadingNearPageBottom,
    SparseLastPage,
    ContentOverflowsPage,
    FontFallbackMetric,
    FontFallbackLayout,
    FontMissing,
    // Setup (doctor)
    UnknownStyle,
    StyleMissingMain,
    StylesDirUnusable,
    FontPathUnusable,
    OutputNotWritable,
    TypstError,
    TypstWarning,
    UnknownTag,
    UnknownWording,
}

impl DiagnosticCode {
    /// The category this code belongs to.
    #[must_use]
    pub const fn category(self) -> Category {
        use DiagnosticCode::*;
        match self {
            MissingName
            | DuplicateName
            | ContentBeforeSection
            | RoleLineShape
            | RoleSeparator
            | EmptyRolePart
            | EraShape
            | EraOutsideRole
            | PeriodUnparseable
            | PeriodEndBeforeStart
            | PeriodHyphen
            | UnsupportedHtml
            | InvalidLink
            | UnsupportedLink
            | NestedList
            | RoleOrder
            | MidSentenceBold
            | Image
            | TooManyContactItems
            | MissingContact
            | PeriodPresentSpelling
            | UnsupportedMarkdown
            | HardBreak
            | TextTooLong
            | EmptyHeading
            | InputLimit
            | HeadingMarkerSpacing
            | PlaceholderUnresolved
            | AnnotationUnknownKeyword
            | AnnotationMalformed
            | AnnotationConflict
            | AnnotationDuplicate
            | AnnotationWithoutTarget
            | AnnotationDetached
            | AnnotationMisplaced
            | AlternateNotAllowed
            | AlternateMissingDefault
            | AlternateNotAdjacent
            | AlternateGroupReused
            | AlternateVariantRepeated
            | AlternatePriorityConflict
            | LetterMissingSection
            | LetterUnknownSection
            | LetterDuplicateSection
            | LetterSectionOrder
            | LetterSectionShape
            | LetterUnsupportedBlock
            | PlaceholderUnknown
            | PlaceholderMalformed
            | PlaceholderPosition
            | TextLimitExceeded
            | LetterSelectionRequired => Category::Format,
            LongBullet | UnquantifiedBullet | WeakOpener | FirstPerson | SloppyPhrase | LongSummary
            | EducationNotLast | OldRoleDetail | AlternateFactsDiffer => Category::Content,
            PageLimitExceeded
            | HeadingNearPageBottom
            | SparseLastPage
            | ContentOverflowsPage
            | FontFallbackMetric
            | FontFallbackLayout
            | FontMissing => Category::Layout,
            UnknownStyle | StyleMissingMain | StylesDirUnusable | FontPathUnusable | OutputNotWritable | TypstError
            | TypstWarning | UnknownTag | UnknownWording => Category::Setup,
        }
    }

    /// How serious a finding with this code is, unless the finder knows better.
    ///
    /// One table for every code, exhaustive with no wildcard, so a new code cannot silently default to a
    /// warning. Format errors block `build`; content advice never fails a run; a layout code is an error when the
    /// document cannot be used as it is; a Setup code is an error when the run cannot do its job.
    #[must_use]
    pub const fn default_severity(self) -> Severity {
        use DiagnosticCode::*;
        match self {
            MissingName
            | DuplicateName
            | ContentBeforeSection
            | RoleLineShape
            | RoleSeparator
            | EmptyRolePart
            | EraShape
            | EraOutsideRole
            | PeriodUnparseable
            | PeriodEndBeforeStart
            | UnsupportedHtml
            | InvalidLink
            | UnsupportedLink
            | UnsupportedMarkdown
            | TextTooLong
            | EmptyHeading
            | InputLimit
            | PlaceholderUnresolved
            | AnnotationUnknownKeyword
            | AnnotationMalformed
            | AnnotationConflict
            | AnnotationDuplicate
            | AnnotationWithoutTarget
            | AnnotationDetached
            | AnnotationMisplaced
            | AlternateNotAllowed
            | AlternateMissingDefault
            | AlternateNotAdjacent
            | AlternateGroupReused
            | AlternateVariantRepeated
            | AlternatePriorityConflict
            | LetterMissingSection
            | LetterUnknownSection
            | LetterDuplicateSection
            | LetterSectionOrder
            | LetterSectionShape
            | LetterUnsupportedBlock
            | PlaceholderUnknown
            | PlaceholderMalformed
            | PlaceholderPosition
            | TextLimitExceeded
            | LetterSelectionRequired => Severity::Error,
            PeriodHyphen
            | PeriodPresentSpelling
            | NestedList
            | RoleOrder
            | MidSentenceBold
            | Image
            | TooManyContactItems
            | MissingContact
            | HardBreak
            | HeadingMarkerSpacing => Severity::Warning,
            LongBullet | UnquantifiedBullet | WeakOpener | FirstPerson | SloppyPhrase | LongSummary
            | EducationNotLast | OldRoleDetail | AlternateFactsDiffer => Severity::Warning,
            PageLimitExceeded | ContentOverflowsPage | FontMissing => Severity::Error,
            HeadingNearPageBottom | SparseLastPage | FontFallbackLayout => Severity::Warning,
            FontFallbackMetric => Severity::Note,
            UnknownStyle | StyleMissingMain | StylesDirUnusable | FontPathUnusable | OutputNotWritable | TypstError => {
                Severity::Error
            }
            TypstWarning | UnknownTag | UnknownWording => Severity::Warning,
        }
    }
}

/// The explanation attached to a [`DiagnosticCode`], shown by `resume doctor --explain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advice {
    /// One line, shown with every finding.
    pub summary: &'static str,
    /// The rationale, citing the book where applicable.
    pub why: &'static str,
    /// What to change.
    pub fix: &'static str,
    /// A `(before, after)` pair.
    pub example: Option<(&'static str, &'static str)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;
    use strum::IntoEnumIterator;

    #[test]
    fn diagnostic_code_ids_are_kebab_case_and_unique() {
        let mut seen = HashSet::new();
        for code in DiagnosticCode::iter() {
            let id: &'static str = code.into();
            assert!(
                !id.is_empty()
                    && id
                        .split('-')
                        .all(|part| !part.is_empty()
                            && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())),
                "{id:?} is not kebab-case"
            );
            assert!(seen.insert(id), "duplicate id {id}");
            assert_eq!(code.to_string(), id);
            assert_eq!(assert_ok!(id.parse::<DiagnosticCode>()), code);
        }
    }

    #[test]
    fn diagnostic_code_serializes_as_its_id() {
        for code in DiagnosticCode::iter() {
            let json = assert_ok!(serde_json::to_value(code));
            assert_eq!(json.as_str(), Some(<&str>::from(code)));
        }
    }

    #[test]
    fn every_category_has_codes() {
        for category in Category::iter() {
            assert!(
                DiagnosticCode::iter().any(|c| c.category() == category),
                "{category} is empty"
            );
        }
    }

    #[test]
    fn category_matches_documented_groups() {
        assert_eq!(DiagnosticCode::MissingName.category(), Category::Format);
        assert_eq!(DiagnosticCode::TooManyContactItems.category(), Category::Format);
        assert_eq!(DiagnosticCode::LongBullet.category(), Category::Content);
        assert_eq!(DiagnosticCode::OldRoleDetail.category(), Category::Content);
        assert_eq!(DiagnosticCode::PageLimitExceeded.category(), Category::Layout);
        assert_eq!(DiagnosticCode::FontMissing.category(), Category::Layout);
        assert_eq!(DiagnosticCode::UnknownStyle.category(), Category::Setup);
        assert_eq!(DiagnosticCode::TypstError.category(), Category::Setup);
    }

    #[test]
    fn every_setup_code_is_an_error_but_a_typst_warning_and_a_content_code_is_never_one() {
        for code in DiagnosticCode::iter() {
            let severity = code.default_severity();
            match code.category() {
                // A Typst warning and a name the resume does not use leave the run usable, so they only warn.
                Category::Setup
                    if matches!(
                        code,
                        DiagnosticCode::TypstWarning | DiagnosticCode::UnknownTag | DiagnosticCode::UnknownWording
                    ) =>
                {
                    assert_eq!(severity, Severity::Warning);
                }
                Category::Setup => assert_eq!(severity, Severity::Error, "{code}: an unusable setup must fail the run"),
                Category::Content => assert_eq!(severity, Severity::Warning, "{code}: advice never fails a run"),
                Category::Format | Category::Layout => {}
            }
        }
        assert_eq!(DiagnosticCode::FontFallbackMetric.default_severity(), Severity::Note);
        assert_eq!(DiagnosticCode::PageLimitExceeded.default_severity(), Severity::Error);
        assert_eq!(DiagnosticCode::SparseLastPage.default_severity(), Severity::Warning);
    }

    #[test]
    fn the_two_setup_codes_for_unusable_directories_have_stable_ids() {
        assert_eq!(DiagnosticCode::StylesDirUnusable.to_string(), "styles-dir-unusable");
        assert_eq!(DiagnosticCode::FontPathUnusable.to_string(), "font-path-unusable");
        assert_eq!(DiagnosticCode::StylesDirUnusable.category(), Category::Setup);
        assert_eq!(DiagnosticCode::FontPathUnusable.category(), Category::Setup);
    }

    #[test]
    fn typst_warning_is_a_setup_code_with_a_stable_id() {
        assert_eq!(DiagnosticCode::TypstWarning.category(), Category::Setup);
        assert_eq!(DiagnosticCode::TypstWarning.to_string(), "typst-warning");
        assert_eq!(DiagnosticCode::TypstError.to_string(), "typst-error");
    }

    #[test]
    fn content_overflows_page_is_a_layout_code_with_a_stable_id() {
        assert_eq!(DiagnosticCode::ContentOverflowsPage.category(), Category::Layout);
        assert_eq!(
            DiagnosticCode::ContentOverflowsPage.to_string(),
            "content-overflows-page"
        );
    }

    #[test]
    fn parser_codes_added_in_the_markdown_task_are_format_codes_with_stable_ids() {
        let added = [
            (DiagnosticCode::MissingContact, "missing-contact"),
            (DiagnosticCode::PeriodPresentSpelling, "period-present-spelling"),
            (DiagnosticCode::UnsupportedMarkdown, "unsupported-markdown"),
            (DiagnosticCode::HardBreak, "hard-break"),
            (DiagnosticCode::TextTooLong, "text-too-long"),
            (DiagnosticCode::EmptyHeading, "empty-heading"),
            (DiagnosticCode::InputLimit, "input-limit"),
            (DiagnosticCode::HeadingMarkerSpacing, "heading-marker-spacing"),
        ];
        for (code, id) in added {
            assert_eq!(code.category(), Category::Format);
            assert_eq!(<&str>::from(code), id);
        }
    }
}

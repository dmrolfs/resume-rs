//! ABOUTME: Stable diagnostic identifiers, their categories, and the advice entry shape.
//! ABOUTME: Advice text is filled in per code by the parser and doctor tasks.

use serde::Serialize;
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
    // Content (doctor)
    LongBullet,
    UnquantifiedBullet,
    WeakOpener,
    FirstPerson,
    SloppyPhrase,
    LongSummary,
    EducationNotLast,
    OldRoleDetail,
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
    OutputNotWritable,
    TypstError,
    TypstWarning,
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
            | HeadingMarkerSpacing => Category::Format,
            LongBullet | UnquantifiedBullet | WeakOpener | FirstPerson | SloppyPhrase | LongSummary
            | EducationNotLast | OldRoleDetail => Category::Content,
            PageLimitExceeded
            | HeadingNearPageBottom
            | SparseLastPage
            | ContentOverflowsPage
            | FontFallbackMetric
            | FontFallbackLayout
            | FontMissing => Category::Layout,
            UnknownStyle | StyleMissingMain | OutputNotWritable | TypstError | TypstWarning => Category::Setup,
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

//! ABOUTME: The layout report: page-limit, stranded-heading, sparse-page, overflow, and font findings for a compiled resume.
//! ABOUTME: The checks are pure functions over measured page geometry, probes, and font resolutions, so they test without Typst.

use std::num::NonZeroUsize;

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::error::Error;
use crate::model::options::{FontsMode, PageLimit};
use crate::typeset::Compiled;
use crate::typeset::fonts::{FontResolution, FontSet};
use crate::typeset::geometry::PageGeometry;
use crate::typeset::probe::{Probe, ProbeKind};

/// A heading whose top is in the bottom 8% of the page body is reported as stranded.
pub const HEADING_ZONE: f64 = 0.08;

/// A last page with less than 15% of the body filled is reported as sparse.
pub const SPARSE_FILL: f64 = 0.15;

/// Content may pass the body bottom by this much before it counts as an overflow, in points: the gap
/// between the body and the footer absorbs a baseline that sits a hair below the body edge.
const OVERFLOW_TOLERANCE_PT: f64 = 0.5;

/// Where one role starts and ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePlacement {
    /// The role's position in the document, from 0.
    pub index: usize,
    /// The company, school, or project.
    pub company: String,
    /// The job title; `None` for a role without one.
    pub title: Option<String>,
    /// The page the role's heading is on.
    pub first_page: NonZeroUsize,
    /// The last page of the role. Exact between two roles; when something other than a role follows the
    /// last role, it can be one page too high.
    pub last_page: NonZeroUsize,
}

/// Everything the layout checks found in one compiled resume.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutReport {
    /// The number of pages.
    pub pages: usize,
    /// Where each role starts and ends, in document order.
    pub roles: Vec<RolePlacement>,
    /// What each font role resolved to; the summary line names them.
    pub fonts: Vec<FontResolution>,
    /// The findings: page limit, overflow, stranded headings, a sparse last page, then fonts.
    pub diagnostics: Vec<Diagnostic>,
}

/// What the checks read; every field is data, so a test can build one without Typst.
#[derive(Debug, Clone, Copy)]
pub struct LayoutInput<'a> {
    /// The measured geometry of each page.
    pub pages: &'a [PageGeometry],
    /// Every role and era heading with its page and position.
    pub probes: &'a [Probe],
    /// What each font role resolved to.
    pub fonts: &'a [FontResolution],
    /// The font mode, which only changes the hint on a font finding.
    pub fonts_mode: FontsMode,
    /// The `--max-pages` limit, if one was given.
    pub max_pages: Option<PageLimit>,
}

impl LayoutReport {
    /// Runs every check over `input`, in a fixed order: page limit, overflow, headings, sparse page, fonts.
    #[must_use]
    pub fn from_input(input: &LayoutInput<'_>) -> Self {
        let mut diagnostics = Vec::new();
        diagnostics.extend(page_limit(input.pages.len(), input.max_pages));
        diagnostics.extend(overflows(input.pages, input.probes));
        diagnostics.extend(headings_near_bottom(input.pages, input.probes));
        diagnostics.extend(sparse_last_page(input.pages));
        diagnostics.extend(input.fonts.iter().filter_map(|font| font.diagnostic(input.fonts_mode)));
        Self {
            pages: input.pages.len(),
            roles: role_placements(input.pages, input.probes),
            fonts: input.fonts.to_vec(),
            diagnostics,
        }
    }

    /// Measures `compiled` and runs the checks.
    ///
    /// # Errors
    /// `Error::Probe` when the style's metadata does not have the documented shape.
    pub fn of(compiled: &Compiled, fonts: &FontSet, max_pages: Option<PageLimit>) -> Result<Self, Error> {
        let pages = compiled.geometry();
        let probes = compiled.probes()?;
        let resolutions = fonts.resolve(&compiled.font_roles()?);
        Ok(Self::from_input(&LayoutInput {
            pages: &pages,
            probes: &probes,
            fonts: &resolutions,
            fonts_mode: fonts.mode(),
            max_pages,
        }))
    }
}

/// A finding with no span: a layout finding names a page, not a place in the Markdown.
fn finding(severity: Severity, code: DiagnosticCode, message: String, hint: String) -> Diagnostic {
    Diagnostic {
        severity,
        code,
        message,
        span: None,
        hints: vec![hint],
        fix: None,
    }
}

/// The company of a role heading; `None` for an era heading.
fn role_company(probe: &Probe) -> Option<&str> {
    match &probe.kind {
        ProbeKind::Role { company, .. } => Some(company),
        ProbeKind::Era { .. } => None,
    }
}

/// What a heading is called in a message: the company of a role, the name of an era.
fn heading_label(probe: &Probe) -> &str {
    match &probe.kind {
        ProbeKind::Role { company, .. } => company,
        ProbeKind::Era { name, .. } => name,
    }
}

/// `page-limit-exceeded`: more pages than `--max-pages` allows.
fn page_limit(pages: usize, limit: Option<PageLimit>) -> Option<Diagnostic> {
    let limit = limit?;
    let over = pages
        .checked_sub(usize::from(limit.into_inner()))
        .filter(|over| *over > 0)?;
    let plural = if over == 1 { "" } else { "s" };
    Some(finding(
        Severity::Error,
        DiagnosticCode::PageLimitExceeded,
        format!("{pages} pages, limit is {limit}"),
        format!("cut or tighten about {over} page{plural} of text, or raise --max-pages"),
    ))
}

/// `content-overflows-page`: one error per page whose content runs past the body, naming the role placed
/// most recently before it. Bullets cannot break across pages, so a bullet taller than a page overflows
/// silently, and the text past the bottom is cut from the PDF.
fn overflows(pages: &[PageGeometry], probes: &[Probe]) -> Vec<Diagnostic> {
    pages
        .iter()
        .enumerate()
        .filter_map(|(index, page)| {
            let over = page.content_bottom? - page.body_bottom;
            (over > OVERFLOW_TOLERANCE_PT).then(|| overflow(index + 1, over, probes))
        })
        .collect()
}

fn overflow(page: usize, over: f64, probes: &[Probe]) -> Diagnostic {
    let role = probes
        .iter()
        .rev()
        .filter(|probe| probe.page.get() <= page)
        .find_map(role_company);
    let hint = match role {
        Some(company) => {
            format!("split the bullet under \"{company}\" into shorter bullets; a bullet must fit on one page")
        }
        None => "split the long bullet into shorter bullets; a bullet must fit on one page".to_string(),
    };
    finding(
        Severity::Error,
        DiagnosticCode::ContentOverflowsPage,
        format!("content on page {page} runs {over:.0}pt past the bottom of the page body"),
        hint,
    )
}

/// `heading-near-page-bottom`: a role or era heading whose top is in the bottom 8% of the page body, measured
/// against the body and not the page so the answer does not depend on the margins.
fn headings_near_bottom(pages: &[PageGeometry], probes: &[Probe]) -> Vec<Diagnostic> {
    probes
        .iter()
        .filter_map(|probe| {
            let page = pages.get(probe.page.get() - 1)?;
            let span = page.body_bottom - page.body_top;
            let depth = (probe.y - page.body_top) / span;
            (span > 0.0 && depth > 1.0 - HEADING_ZONE).then(|| {
                finding(
                    Severity::Warning,
                    DiagnosticCode::HeadingNearPageBottom,
                    format!(
                        "{} starts in the bottom {:.0}% of page {}",
                        heading_label(probe),
                        HEADING_ZONE * 100.0,
                        probe.page
                    ),
                    "add a page break before it, or tighten the text above so it moves up".to_string(),
                )
            })
        })
        .collect()
}

/// `sparse-last-page`: the last page of a resume of two or more pages holds less than 15% of a body.
fn sparse_last_page(pages: &[PageGeometry]) -> Option<Diagnostic> {
    let last = pages.last().filter(|_| pages.len() > 1)?;
    let span = last.body_bottom - last.body_top;
    let message = match last.content_bottom {
        None => "the last page is empty".to_string(),
        Some(bottom) => {
            let fill = (bottom - last.body_top) / span;
            if span <= 0.0 || fill >= SPARSE_FILL {
                return None;
            }
            format!("the last page is {:.0}% full", fill * 100.0)
        }
    };
    Some(finding(
        Severity::Warning,
        DiagnosticCode::SparseLastPage,
        message,
        "trim or tighten the text to save a page".to_string(),
    ))
}

/// Where each role starts and ends. A role ends on the page before the next role when that role opens its
/// page, and otherwise shares the page with it; the last role ends on the last page. Eras belong to their
/// role and are not placed here.
fn role_placements(pages: &[PageGeometry], probes: &[Probe]) -> Vec<RolePlacement> {
    let roles: Vec<(&Probe, usize, &str, Option<&str>)> = probes
        .iter()
        .filter_map(|probe| match &probe.kind {
            ProbeKind::Role {
                index, company, title, ..
            } => Some((probe, *index, company.as_str(), title.as_deref())),
            ProbeKind::Era { .. } => None,
        })
        .collect();
    roles
        .iter()
        .enumerate()
        .map(|(position, (probe, index, company, title))| {
            let first_page = probe.page;
            let last_page = match roles.get(position + 1) {
                Some((next, ..)) => {
                    let opens_page = pages
                        .get(next.page.get() - 1)
                        .is_some_and(|page| next.y - page.body_top < 1.0);
                    let end = if opens_page {
                        next.page.get() - 1
                    } else {
                        next.page.get()
                    };
                    NonZeroUsize::new(end.max(first_page.get())).unwrap_or(first_page)
                }
                None => NonZeroUsize::new(pages.len()).map_or(first_page, |last| last.max(first_page)),
            };
            RolePlacement {
                index: *index,
                company: (*company).to_string(),
                title: title.map(str::to_string),
                first_page,
                last_page,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::fonts::Metrics;
    use claims::{assert_none, assert_ok, assert_some};
    use pretty_assertions::assert_eq;

    /// A Letter page as the pragmatic style lays it out: 36 pt above the body, the footer from 759.24 pt.
    fn page(content_bottom: Option<f64>) -> PageGeometry {
        PageGeometry {
            height: 792.0,
            body_top: 36.0,
            body_bottom: 759.24,
            content_bottom,
        }
    }

    fn full_page() -> PageGeometry {
        page(Some(740.0))
    }

    fn at(page: usize) -> NonZeroUsize {
        assert_some!(NonZeroUsize::new(page))
    }

    fn role(index: usize, company: &str, page: usize, y: f64) -> Probe {
        Probe {
            kind: ProbeKind::Role {
                index,
                company: company.to_string(),
                title: None,
                period: "2020 – Present".to_string(),
            },
            page: at(page),
            y,
            page_height: 792.0,
        }
    }

    fn era(role: usize, name: &str, page: usize, y: f64) -> Probe {
        Probe {
            kind: ProbeKind::Era {
                role,
                index: 0,
                name: name.to_string(),
                period: "2020".to_string(),
            },
            page: at(page),
            y,
            page_height: 792.0,
        }
    }

    fn report(pages: &[PageGeometry], probes: &[Probe]) -> LayoutReport {
        LayoutReport::from_input(&LayoutInput {
            pages,
            probes,
            fonts: &[],
            fonts_mode: FontsMode::Auto,
            max_pages: None,
        })
    }

    fn limited(pages: &[PageGeometry], limit: u8) -> LayoutReport {
        LayoutReport::from_input(&LayoutInput {
            pages,
            probes: &[],
            fonts: &[],
            fonts_mode: FontsMode::Auto,
            max_pages: Some(assert_ok!(PageLimit::try_new(limit))),
        })
    }

    fn with_code(report: &LayoutReport, code: DiagnosticCode) -> Vec<&Diagnostic> {
        report.diagnostics.iter().filter(|d| d.code == code).collect()
    }

    // ========================================================================
    // Page limit
    // ========================================================================

    #[test]
    fn page_limit_exceeded_names_the_count_and_the_limit() {
        let pages = [full_page(), full_page(), full_page(), full_page()];
        let report = limited(&pages, 3);
        assert_eq!(report.pages, 4);
        let found = with_code(&report, DiagnosticCode::PageLimitExceeded);
        assert_eq!(found.len(), 1, "{:?}", report.diagnostics);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].message, "4 pages, limit is 3");
        assert_none!(found[0].span.as_ref());
        assert!(!found[0].hints.is_empty());
    }

    #[test]
    fn the_page_limit_hint_says_how_many_pages_to_cut() {
        let hint = |pages: usize, limit: u8| {
            let report = limited(&vec![full_page(); pages], limit);
            let found = with_code(&report, DiagnosticCode::PageLimitExceeded);
            assert_eq!(found.len(), 1, "{:?}", report.diagnostics);
            found[0].hints.join(" | ")
        };
        assert_eq!(hint(4, 3), "cut or tighten about 1 page of text, or raise --max-pages");
        assert_eq!(hint(5, 3), "cut or tighten about 2 pages of text, or raise --max-pages");
    }

    #[test]
    fn no_limit_or_a_page_count_at_the_limit_gives_no_page_finding() {
        let pages = [full_page(), full_page(), full_page()];
        assert_eq!(
            with_code(&report(&pages, &[]), DiagnosticCode::PageLimitExceeded).len(),
            0
        );
        assert_eq!(
            with_code(&limited(&pages, 3), DiagnosticCode::PageLimitExceeded).len(),
            0
        );
    }

    // ========================================================================
    // Sparse last page
    // ========================================================================

    #[test]
    fn sparse_last_page_warns_below_15_percent_of_the_body() {
        // The body is 723.24 pt tall, so 15% ends at 144.49 pt: content down to 120 pt is 11.6% full.
        let report = report(&[full_page(), page(Some(120.0))], &[]);
        let found = with_code(&report, DiagnosticCode::SparseLastPage);
        assert_eq!(found.len(), 1, "{:?}", report.diagnostics);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].message, "the last page is 12% full");
    }

    #[test]
    fn a_last_page_above_15_percent_is_not_sparse() {
        let report = report(&[full_page(), page(Some(150.0))], &[]);
        assert_eq!(with_code(&report, DiagnosticCode::SparseLastPage).len(), 0);
    }

    #[test]
    fn a_one_page_resume_is_never_sparse() {
        let report = report(&[page(Some(60.0))], &[]);
        assert_eq!(with_code(&report, DiagnosticCode::SparseLastPage).len(), 0);
    }

    #[test]
    fn an_empty_last_page_is_sparse() {
        let report = report(&[full_page(), page(None)], &[]);
        let found = with_code(&report, DiagnosticCode::SparseLastPage);
        assert_eq!(found.len(), 1, "{:?}", report.diagnostics);
        assert_eq!(found[0].message, "the last page is empty");
    }

    // ========================================================================
    // Headings near the bottom of a page
    // ========================================================================

    #[test]
    fn a_heading_in_the_bottom_8_percent_of_the_body_warns() {
        // (710 - 36) / 723.24 = 93.2% of the way down the body.
        let probes = [role(0, "Acme", 1, 710.0)];
        let report = report(&[full_page(), full_page()], &probes);
        let found = with_code(&report, DiagnosticCode::HeadingNearPageBottom);
        assert_eq!(found.len(), 1, "{:?}", report.diagnostics);
        assert_eq!(found[0].severity, Severity::Warning);
        assert!(
            found[0].message.contains("Acme") && found[0].message.contains("page 1"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn a_heading_above_the_zone_is_quiet() {
        // 90.4% and 91.95% of the way down: both above the 92% line.
        let probes = [role(0, "Acme", 1, 690.0), role(1, "Beta", 2, 701.0)];
        let report = report(&[full_page(), full_page()], &probes);
        assert_eq!(with_code(&report, DiagnosticCode::HeadingNearPageBottom).len(), 0);
    }

    #[test]
    fn a_heading_is_judged_against_its_own_page() {
        // Page 1's body ends at 700 pt and page 2's at 759.24 pt: a heading at 650 pt is 92.5% of the way down
        // the first body and only 84.9% down the second.
        let short = PageGeometry {
            height: 792.0,
            body_top: 36.0,
            body_bottom: 700.0,
            content_bottom: Some(690.0),
        };
        let probes = [role(0, "Acme", 1, 650.0)];
        let report = report(&[short, full_page()], &probes);
        assert_eq!(with_code(&report, DiagnosticCode::HeadingNearPageBottom).len(), 1);
    }

    #[test]
    fn a_page_without_a_body_gets_no_heading_finding() {
        let flat = PageGeometry {
            height: 792.0,
            body_top: 36.0,
            body_bottom: 36.0,
            content_bottom: None,
        };
        let report = report(&[flat], &[role(0, "Acme", 1, 100.0)]);
        assert_eq!(with_code(&report, DiagnosticCode::HeadingNearPageBottom).len(), 0);
    }

    #[test]
    fn an_era_heading_in_the_zone_warns_by_its_name() {
        let probes = [role(0, "Acme", 1, 100.0), era(0, "Programming languages", 1, 715.0)];
        let report = report(&[full_page(), full_page()], &probes);
        let found = with_code(&report, DiagnosticCode::HeadingNearPageBottom);
        assert_eq!(found.len(), 1, "{:?}", report.diagnostics);
        assert!(
            found[0].message.contains("Programming languages"),
            "{}",
            found[0].message
        );
    }

    // ========================================================================
    // Content that overflows a page
    // ========================================================================

    #[test]
    fn overflow_is_one_error_per_page_naming_the_role() {
        // Page 2's deepest baseline is 504 pt below the body bottom; Acme is the role placed most recently
        // before it (Beta starts on page 3).
        let pages = [full_page(), page(Some(759.24 + 504.0)), full_page()];
        let probes = [role(0, "Acme", 1, 100.0), role(1, "Beta", 3, 36.0)];
        let report = report(&pages, &probes);
        let found = with_code(&report, DiagnosticCode::ContentOverflowsPage);
        assert_eq!(found.len(), 1, "{:?}", report.diagnostics);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(
            found[0].message,
            "content on page 2 runs 504pt past the bottom of the page body"
        );
        assert!(
            found[0].hints.iter().any(|h| h.contains("Acme")),
            "{:?}",
            found[0].hints
        );
    }

    #[test]
    fn a_baseline_a_hair_below_the_body_is_not_an_overflow() {
        let inside = report(&[page(Some(759.24 + 0.4))], &[]);
        assert_eq!(with_code(&inside, DiagnosticCode::ContentOverflowsPage).len(), 0);
        let outside = report(&[page(Some(759.24 + 0.6))], &[]);
        assert_eq!(with_code(&outside, DiagnosticCode::ContentOverflowsPage).len(), 1);
    }

    #[test]
    fn content_exactly_at_the_tolerance_is_not_an_overflow() {
        let page = PageGeometry {
            height: 800.0,
            body_top: 0.0,
            body_bottom: 700.0,
            content_bottom: Some(700.5),
        };
        assert_eq!(
            with_code(&report(&[page], &[]), DiagnosticCode::ContentOverflowsPage).len(),
            0
        );
    }

    // ========================================================================
    // Where each role starts and ends
    // ========================================================================

    #[test]
    fn role_placement_ends_before_a_role_that_opens_a_page() {
        let probes = [role(0, "Acme", 1, 36.0), role(1, "Beta", 2, 36.0)];
        let roles = report(&[full_page(), full_page()], &probes).roles;
        assert_eq!((roles[0].first_page, roles[0].last_page), (at(1), at(1)));
    }

    #[test]
    fn role_placement_shares_the_page_when_the_next_role_starts_mid_page() {
        let probes = [role(0, "Acme", 1, 36.0), role(1, "Beta", 2, 300.0)];
        let roles = report(&[full_page(), full_page()], &probes).roles;
        assert_eq!((roles[0].first_page, roles[0].last_page), (at(1), at(2)));
        assert_eq!((roles[1].first_page, roles[1].last_page), (at(2), at(2)));
    }

    #[test]
    fn a_role_one_point_below_the_body_top_does_not_open_its_page() {
        let probes = [role(0, "Acme", 1, 36.0), role(1, "Beta", 2, 37.0)];
        let roles = report(&[full_page(), full_page()], &probes).roles;
        assert_eq!(roles[0].last_page, at(2));
    }

    #[test]
    fn the_last_role_ends_on_the_last_page() {
        let probes = [role(0, "Acme", 1, 36.0), role(1, "Beta", 2, 300.0)];
        let roles = report(&[full_page(), full_page(), page(Some(200.0))], &probes).roles;
        assert_eq!(roles[1].last_page, at(3));
    }

    #[test]
    fn eras_do_not_create_placements() {
        let probes = [role(0, "Acme", 1, 36.0), era(0, "Languages", 1, 200.0)];
        let roles = report(&[full_page()], &probes).roles;
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].company, "Acme");
        assert_eq!(roles[0].index, 0);
    }

    // ========================================================================
    // Fonts, and the order of the findings
    // ========================================================================

    fn fallback(role: &str, metrics: Metrics) -> FontResolution {
        FontResolution::Fallback {
            role: role.to_string(),
            preferred: "Calibri".to_string(),
            family: "Carlito".to_string(),
            origin: crate::model::options::FontOrigin::Bundled,
            metrics,
        }
    }

    #[test]
    fn font_resolutions_join_the_report_with_their_severities() {
        let fonts = [
            FontResolution::Preferred {
                role: "mono".to_string(),
                family: "Arial".to_string(),
                origin: crate::model::options::FontOrigin::System,
            },
            fallback("body", Metrics::Compatible),
            fallback("labels", Metrics::Different),
            FontResolution::Missing {
                role: "display".to_string(),
                preferred: "Georgia".to_string(),
                fallback: "Gelasio".to_string(),
            },
        ];
        let report = LayoutReport::from_input(&LayoutInput {
            pages: &[full_page()],
            probes: &[],
            fonts: &fonts,
            fonts_mode: FontsMode::Auto,
            max_pages: None,
        });
        let found: Vec<_> = report.diagnostics.iter().map(|d| (d.code, d.severity)).collect();
        assert_eq!(
            found,
            [
                (DiagnosticCode::FontFallbackMetric, Severity::Note),
                (DiagnosticCode::FontFallbackLayout, Severity::Warning),
                (DiagnosticCode::FontMissing, Severity::Error),
            ]
        );
    }

    #[test]
    fn the_report_keeps_the_font_resolutions_for_the_summary() {
        let fonts = [fallback("body", Metrics::Compatible)];
        let report = LayoutReport::from_input(&LayoutInput {
            pages: &[full_page()],
            probes: &[],
            fonts: &fonts,
            fonts_mode: FontsMode::Auto,
            max_pages: None,
        });
        assert_eq!(report.fonts, fonts);
    }

    #[test]
    fn findings_come_in_a_fixed_order() {
        let pages = [full_page(), page(Some(759.24 + 100.0)), page(Some(60.0))];
        let probes = [role(0, "Acme", 1, 715.0)];
        let fonts = [fallback("body", Metrics::Different)];
        let report = LayoutReport::from_input(&LayoutInput {
            pages: &pages,
            probes: &probes,
            fonts: &fonts,
            fonts_mode: FontsMode::Auto,
            max_pages: Some(assert_ok!(PageLimit::try_new(2))),
        });
        let codes: Vec<_> = report.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(
            codes,
            [
                DiagnosticCode::PageLimitExceeded,
                DiagnosticCode::ContentOverflowsPage,
                DiagnosticCode::HeadingNearPageBottom,
                DiagnosticCode::SparseLastPage,
                DiagnosticCode::FontFallbackLayout,
            ]
        );
    }
}

//! ABOUTME: Groups a file's findings by category, with the advice for each code, ready to print.
//! ABOUTME: Pure: it decides order, counts, and notes, and leaves the printing to the command.

use std::collections::HashSet;

use crate::diagnostics::{Category, Diagnostic, DiagnosticCode, display_text};
use crate::layout::RolePlacement;

/// One finding with the notes printed after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown<'d> {
    pub diagnostic: &'d Diagnostic,
    /// The `why:` and `fix:` of its code on the first finding of that code, a pointer on the later ones.
    pub notes: Vec<String>,
}

/// One category of one file's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group<'d> {
    pub category: Category,
    /// Lines printed under the header, before the findings.
    pub before: Vec<String>,
    pub shown: Vec<Shown<'d>>,
    /// What is true of the category besides its findings, such as the style and fonts for Setup.
    pub facts: Vec<String>,
}

impl Group<'_> {
    /// `Format   2 findings`, or `Setup    ok (style pragmatic; ...)` when nothing was found.
    #[must_use]
    pub fn header(&self) -> String {
        let name = format!("{:<8}", capitalized(self.category));
        match self.shown.len() {
            0 if self.facts.is_empty() => format!("{name}ok"),
            0 => format!("{name}ok ({})", self.facts.join("; ")),
            1 => format!("{name}1 finding"),
            n => format!("{name}{n} findings"),
        }
    }

    /// The facts as one line, when findings are listed and the header could not carry them.
    #[must_use]
    pub fn facts_line(&self) -> Option<String> {
        (!self.shown.is_empty() && !self.facts.is_empty()).then(|| self.facts.join("; "))
    }
}

fn capitalized(category: Category) -> String {
    let name = category.to_string();
    let mut chars = name.chars();
    chars
        .next()
        .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// What a report says about a file besides its findings.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// Where each role starts, under the Layout header.
    pub roles: Option<String>,
    /// The Setup facts: the style, the fonts, the output.
    pub setup: Vec<String>,
}

/// Groups `findings` by category, in `Category` order, every category present even when empty.
///
/// Format and Content findings are sorted by position (unlocated last), then code and message. Layout keeps the
/// order the checks produce, which is already the meaningful one. In human output the notes about a font
/// fallback with the same widths are left out, because the Setup line names the fonts; JSON keeps them.
#[must_use]
pub fn group<'d>(findings: &'d [Diagnostic], facts: &Facts, human: bool) -> Vec<Group<'d>> {
    let mut seen: HashSet<DiagnosticCode> = HashSet::new();
    use strum::IntoEnumIterator;
    Category::iter()
        .map(|category| {
            let mut shown: Vec<&Diagnostic> = findings
                .iter()
                .filter(|d| d.code.category() == category)
                .filter(|d| !(human && d.code == DiagnosticCode::FontFallbackMetric))
                .collect();
            if matches!(category, Category::Format | Category::Content) {
                shown.sort_by(|a, b| {
                    position(a)
                        .cmp(&position(b))
                        .then_with(|| (a.code as u8).cmp(&(b.code as u8)))
                        .then_with(|| a.message.cmp(&b.message))
                });
            }
            Group {
                category,
                before: match category {
                    Category::Layout => facts.roles.iter().map(|roles| format!("roles: {roles}")).collect(),
                    _ => Vec::new(),
                },
                shown: shown
                    .into_iter()
                    .map(|diagnostic| Shown {
                        diagnostic,
                        notes: notes(diagnostic.code, &mut seen),
                    })
                    .collect(),
                facts: if category == Category::Setup {
                    facts.setup.clone()
                } else {
                    Vec::new()
                },
            }
        })
        .collect()
}

/// A finding's place for sorting: start of its span, with unlocated findings after every located one.
fn position(diagnostic: &Diagnostic) -> usize {
    diagnostic.span.as_ref().map_or(usize::MAX, |span| span.range.start)
}

/// The advice notes for a finding: in full the first time its code appears in a file, a pointer after that.
fn notes(code: DiagnosticCode, seen: &mut HashSet<DiagnosticCode>) -> Vec<String> {
    if seen.insert(code) {
        let advice = code.advice();
        vec![format!("why: {}", advice.why), format!("fix: {}", advice.fix)]
    } else {
        vec![format!("the why and fix are under the first `{code}` above")]
    }
}

/// Where each role starts, for the Layout header: `Acme p1-2; Globex p2-3; Initech p3`.
///
/// A role's range ends on the page before the next role opens a page or on the next role's page, which is
/// exact. The last role would end on the last page of the document, which is one too many when something that
/// is not a role follows it, so the last role shows only the page it starts on. Nothing for a one-page resume.
#[must_use]
pub fn role_map(roles: &[RolePlacement], pages: usize) -> Option<String> {
    if pages < 2 || roles.is_empty() {
        return None;
    }
    let shown: Vec<String> = roles
        .iter()
        .enumerate()
        .map(|(at, role)| {
            let name = display_text(&role.company, false);
            let first = role.first_page.get();
            let last = role.last_page.get();
            if at + 1 == roles.len() || last == first {
                format!("{name} p{first}")
            } else {
                format!("{name} p{first}-{last}")
            }
        })
        .collect();
    Some(shown.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Severity, SourceKind, Span};
    use claims::assert_some;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroUsize;

    fn finding(code: DiagnosticCode, at: Option<usize>, message: &str) -> Diagnostic {
        Diagnostic {
            severity: code.default_severity(),
            code,
            message: message.to_string(),
            span: at.map(|start| Span {
                file: "r.md".into(),
                kind: SourceKind::Markdown,
                range: start..start + 1,
            }),
            hints: Vec::new(),
            fix: None,
        }
    }

    fn page(n: usize) -> NonZeroUsize {
        assert_some!(NonZeroUsize::new(n))
    }

    fn role(company: &str, first: usize, last: usize) -> RolePlacement {
        RolePlacement {
            index: 0,
            company: company.to_string(),
            title: None,
            first_page: page(first),
            last_page: page(last),
        }
    }

    #[test]
    fn every_category_is_present_in_a_fixed_order_even_when_empty() {
        let groups = group(&[], &Facts::default(), true);
        let names: Vec<String> = groups.iter().map(|g| g.header()).collect();
        assert_eq!(names, ["Format  ok", "Content ok", "Layout  ok", "Setup   ok"]);
    }

    #[test]
    fn a_header_counts_its_findings_and_singularizes_one() {
        let findings = [
            finding(DiagnosticCode::PeriodHyphen, Some(5), "a"),
            finding(DiagnosticCode::HardBreak, Some(9), "b"),
            finding(DiagnosticCode::LongBullet, Some(1), "c"),
        ];
        let groups = group(&findings, &Facts::default(), true);
        assert_eq!(groups[0].header(), "Format  2 findings");
        assert_eq!(groups[1].header(), "Content 1 finding");
    }

    #[test]
    fn format_and_content_findings_sort_by_position_with_unlocated_last() {
        let findings = [
            finding(DiagnosticCode::HardBreak, None, "unlocated"),
            finding(DiagnosticCode::PeriodHyphen, Some(30), "late"),
            finding(DiagnosticCode::HardBreak, Some(2), "early"),
        ];
        let groups = group(&findings, &Facts::default(), true);
        let order: Vec<&str> = groups[0].shown.iter().map(|s| s.diagnostic.message.as_str()).collect();
        assert_eq!(order, ["early", "late", "unlocated"]);
    }

    #[test]
    fn layout_keeps_the_order_the_checks_produced() {
        let findings = [
            finding(DiagnosticCode::SparseLastPage, None, "sparse"),
            finding(DiagnosticCode::PageLimitExceeded, None, "limit"),
        ];
        let groups = group(&findings, &Facts::default(), true);
        let order: Vec<&str> = groups[2].shown.iter().map(|s| s.diagnostic.message.as_str()).collect();
        assert_eq!(order, ["sparse", "limit"]);
    }

    #[test]
    fn a_metric_compatible_font_note_is_hidden_from_people_and_kept_for_json() {
        let findings = [finding(DiagnosticCode::FontFallbackMetric, None, "Carlito")];
        assert_eq!(group(&findings, &Facts::default(), true)[2].shown.len(), 0);
        assert_eq!(group(&findings, &Facts::default(), false)[2].shown.len(), 1);
    }

    #[test]
    fn the_first_finding_of_a_code_carries_the_advice_and_later_ones_point_to_it() {
        let findings = [
            finding(DiagnosticCode::UnquantifiedBullet, Some(1), "a"),
            finding(DiagnosticCode::UnquantifiedBullet, Some(2), "b"),
        ];
        let groups = group(&findings, &Facts::default(), true);
        let first = &groups[1].shown[0].notes;
        assert!(
            first[0].starts_with("why: ") && first[1].starts_with("fix: "),
            "{first:?}"
        );
        let second = &groups[1].shown[1].notes;
        assert_eq!(second.len(), 1);
        assert!(second[0].contains("`unquantified-bullet`"), "{second:?}");
    }

    #[test]
    fn setup_facts_ride_the_header_when_ok_and_a_line_when_there_are_findings() {
        let facts = Facts {
            roles: None,
            setup: vec!["style pragmatic (built-in)".into(), "out/r.pdf is free".into()],
        };
        let ok = group(&[], &facts, true);
        assert_eq!(
            ok[3].header(),
            "Setup   ok (style pragmatic (built-in); out/r.pdf is free)"
        );
        assert_eq!(ok[3].facts_line(), None);
        let bad = [finding(DiagnosticCode::OutputNotWritable, None, "x")];
        let groups = group(&bad, &facts, true);
        assert_eq!(groups[3].header(), "Setup   1 finding");
        assert_eq!(
            groups[3].facts_line().as_deref(),
            Some("style pragmatic (built-in); out/r.pdf is free")
        );
    }

    #[test]
    fn the_role_map_goes_under_the_layout_header() {
        let facts = Facts {
            roles: Some("Acme p1".into()),
            setup: Vec::new(),
        };
        assert_eq!(group(&[], &facts, true)[2].before, ["roles: Acme p1"]);
    }

    #[test]
    fn role_map_gives_ranges_but_only_the_start_page_of_the_last_role() {
        let roles = [role("Acme", 1, 2), role("Globex", 2, 2), role("Initech", 2, 3)];
        assert_eq!(role_map(&roles, 3).as_deref(), Some("Acme p1-2; Globex p2; Initech p2"));
    }

    #[test]
    fn role_map_is_empty_for_one_page_or_no_roles_and_is_sanitized() {
        assert_eq!(role_map(&[role("Acme", 1, 1)], 1), None);
        assert_eq!(role_map(&[], 3), None);
        let map = assert_some!(role_map(&[role("A\u{1b}[31m", 1, 2), role("B", 2, 2)], 2));
        assert!(!map.contains('\u{1b}'), "{map:?}");
    }

    #[test]
    fn severity_does_not_change_which_group_a_finding_is_in() {
        let mut f = finding(DiagnosticCode::UnknownStyle, None, "x");
        f.severity = Severity::Warning;
        assert_eq!(group(&[f], &Facts::default(), true)[3].shown.len(), 1);
    }
}

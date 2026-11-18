//! ABOUTME: Format lints computed during interpretation, where located tree nodes still carry source ranges.
//! ABOUTME: Nested lists, role chronology, mid-sentence bold, images, and contact item count (book-guided).

use std::ops::Range;

use super::tree::{InlineNode, ListNode};
use super::{Finding, Sink};
use crate::diagnostics::{DiagnosticCode, Edit};
use crate::model::PeriodEnd;

/// Contact lines with more items than this get a warning.
const MAX_CONTACT_ITEMS: usize = 4;

/// One role's chronology data and where to point.
#[derive(Debug, Clone)]
pub(crate) struct RoleKey {
    pub end: PeriodEnd,
    pub range: Range<usize>,
    pub company: String,
}

/// Warns about a nested list; the caller flattens its items into the parent list.
pub(crate) fn nested_list(list: &ListNode, sink: &mut Sink<'_>) {
    let finding = Finding::new(
        DiagnosticCode::NestedList,
        list.range.clone(),
        "nested lists are flattened to one level",
    )
    .hint("keep bullets to one level: fold the detail into the parent bullet or make it a sibling");
    sink.report(finding);
}

/// Warns for each role that certainly ended later than the role listed before it.
///
/// Roles are listed newest first. A role is out of order only when the earliest date its end can
/// denote is after the latest date the previous role's end can denote, so `2017` after `Aug 2017`
/// is never flagged (the year might be earlier in the year).
pub(crate) fn role_order(roles: &[RoleKey], sink: &mut Sink<'_>) {
    for pair in roles.windows(2) {
        let [previous, current] = pair else { continue };
        if earliest_end(&current.end) > latest_end(&previous.end) {
            let message = format!("`{}` ended after the role above it", current.company);
            let finding = Finding::new(DiagnosticCode::RoleOrder, current.range.clone(), message)
                .hint(format!("list roles newest first: move it above `{}`", previous.company));
            sink.report(finding);
        }
    }
}

/// Orders ends chronologically with `Present` after every date.
type EndKey = (bool, u16, u8);

fn earliest_end(end: &PeriodEnd) -> EndKey {
    match end {
        PeriodEnd::Present => (true, 0, 0),
        PeriodEnd::Date(date) => {
            let (year, month) = date.sort_key();
            (false, year.into(), month)
        }
    }
}

fn latest_end(end: &PeriodEnd) -> EndKey {
    match end {
        PeriodEnd::Present => (true, 0, 0),
        PeriodEnd::Date(date) => {
            let (year, month) = date.latest_key();
            (false, year.into(), month)
        }
    }
}

/// Warns about bold inside a sentence; `colon_fix` moves a trailing colon inside the bold.
pub(crate) fn mid_sentence_bold(range: Range<usize>, colon_fix: Option<Edit>, sink: &mut Sink<'_>) {
    let mut finding = Finding::new(DiagnosticCode::MidSentenceBold, range, "bold inside a sentence").hint(
        "bold only titles, companies, dates, and headings; for a lead-in write `**Label:** text`, with the colon inside",
    );
    if let Some(edit) = colon_fix {
        finding = finding.fix(edit.range, edit.replacement);
    }
    sink.report(finding);
}

/// Warns that an image is not rendered.
pub(crate) fn image(range: Range<usize>, sink: &mut Sink<'_>) {
    let finding = Finding::new(DiagnosticCode::Image, range, "images are not rendered on a resume")
        .hint("remove the image; a resume has no photos or graphics");
    sink.report(finding);
}

/// Warns above four contact items, counted over the contact paragraph's top-level inline nodes.
pub(crate) fn contact_items(nodes: &[InlineNode], range: Range<usize>, sink: &mut Sink<'_>) {
    let count = count_contact_items(nodes);
    if count > MAX_CONTACT_ITEMS {
        let message = format!("the contact line has {count} items; at most {MAX_CONTACT_ITEMS} are recommended");
        let finding = Finding::new(DiagnosticCode::TooManyContactItems, range, message)
            .hint("keep the contact line to location, email, phone, and one or two links");
        sink.report(finding);
    }
}

/// Counts items separated by `·`, `|`, or `•`; link text is never split.
fn count_contact_items(nodes: &[InlineNode]) -> usize {
    let mut items = 0;
    let mut in_item = false;
    for node in nodes {
        match node {
            InlineNode::Text(text) => {
                let mut pieces = text.text.split(['·', '|', '•']);
                if let Some(first) = pieces.next() {
                    in_item |= !first.trim().is_empty();
                }
                for piece in pieces {
                    items += usize::from(in_item);
                    in_item = !piece.trim().is_empty();
                }
            }
            InlineNode::Break { .. } => {}
            InlineNode::Html { text, .. } if text.starts_with("<!--") => {}
            _ => in_item = true,
        }
    }
    items + usize::from(in_item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostic;
    use crate::markdown::tree::{self, BlockNode};
    use crate::model::{PartialDate, Season, Year};
    use claims::assert_some;
    use pretty_assertions::assert_eq;
    use std::path::Path;
    use time::Month;

    fn year(y: u16) -> Year {
        assert_some!(Year::try_new(y).ok())
    }

    fn month_end(y: u16, m: Month) -> PeriodEnd {
        PeriodEnd::Date(PartialDate::Month {
            year: year(y),
            month: m,
        })
    }

    fn year_end(y: u16) -> PeriodEnd {
        PeriodEnd::Date(PartialDate::Year { year: year(y) })
    }

    fn role(company: &str, end: PeriodEnd, at: usize) -> RoleKey {
        RoleKey {
            end,
            range: at..at + 1,
            company: company.to_string(),
        }
    }

    fn order(roles: &[RoleKey]) -> Vec<Diagnostic> {
        let mut sink = Sink::new(Path::new("r.md"));
        role_order(roles, &mut sink);
        sink.finish()
    }

    fn contact_diagnostics(markdown: &str) -> Vec<Diagnostic> {
        let mut sink = Sink::new(Path::new("r.md"));
        let doc = tree::lower(markdown, 0, &mut sink);
        let Some(BlockNode::Paragraph(p)) = doc.blocks.first() else {
            return Vec::new();
        };
        contact_items(&p.content, p.range.clone(), &mut sink);
        sink.finish()
    }

    #[test]
    fn out_of_order_roles_warn() {
        let roles = [
            role("Old Co", month_end(2015, Month::June), 0),
            role("New Co", month_end(2020, Month::June), 10),
        ];
        let diagnostics = order(&roles);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::RoleOrder);
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 10..11);
        assert_eq!(
            diagnostics[0].hints,
            vec!["list roles newest first: move it above `Old Co`"]
        );
    }

    #[test]
    fn newest_first_roles_do_not_warn() {
        let roles = [
            role("New", PeriodEnd::Present, 0),
            role("Mid", month_end(2020, Month::June), 10),
            role("Old", year_end(2015), 20),
        ];
        assert_eq!(order(&roles), vec![]);
    }

    #[test]
    fn present_roles_tie_without_warning() {
        let roles = [role("A", PeriodEnd::Present, 0), role("B", PeriodEnd::Present, 10)];
        assert_eq!(order(&roles), vec![]);
        let roles = [
            role("A", month_end(2020, Month::May), 0),
            role("B", PeriodEnd::Present, 10),
        ];
        assert_eq!(
            order(&roles).len(),
            1,
            "a Present role below a dated one is out of order"
        );
    }

    #[test]
    fn year_end_ties_month_end_in_same_year() {
        let first_year = [
            role("A", month_end(2017, Month::August), 0),
            role("B", year_end(2017), 10),
        ];
        assert_eq!(order(&first_year), vec![], "2017 might be earlier than August");
        let first_bare = [
            role("A", year_end(2017), 0),
            role("B", month_end(2017, Month::August), 10),
        ];
        assert_eq!(order(&first_bare), vec![]);
        let later_year = [
            role("A", month_end(2017, Month::August), 0),
            role("B", year_end(2018), 10),
        ];
        assert_eq!(order(&later_year).len(), 1);
    }

    #[test]
    fn season_ends_compare_by_their_span() {
        let spring = PeriodEnd::Date(PartialDate::Season {
            year: year(2020),
            season: Season::Spring,
        });
        let roles = [role("A", spring, 0), role("B", month_end(2020, Month::June), 10)];
        assert_eq!(order(&roles).len(), 1, "June is after the end of spring");
        let roles = [role("A", spring, 0), role("B", month_end(2020, Month::April), 10)];
        assert_eq!(order(&roles), vec![], "April is inside spring");
    }

    #[test]
    fn nested_list_warns() {
        let mut sink = Sink::new(Path::new("r.md"));
        let doc = tree::lower("- a\n  - b\n", 0, &mut sink);
        let Some(BlockNode::List(outer)) = doc.blocks.first() else {
            panic!("expected a list")
        };
        let Some(BlockNode::List(inner)) = outer.items[0].blocks.get(1) else {
            panic!("expected a nested list")
        };
        nested_list(inner, &mut sink);
        let diagnostics = sink.finish();
        assert_eq!(diagnostics[0].code, DiagnosticCode::NestedList);
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, inner.range);
        assert!(!diagnostics[0].hints.is_empty());
    }

    #[test]
    fn mid_sentence_bold_warns() {
        let mut sink = Sink::new(Path::new("r.md"));
        mid_sentence_bold(3..9, None, &mut sink);
        let diagnostics = sink.finish();
        assert_eq!(
            (diagnostics[0].code, diagnostics[0].fix.is_none()),
            (DiagnosticCode::MidSentenceBold, true)
        );
    }

    #[test]
    fn mid_sentence_bold_carries_the_colon_edit() {
        let mut sink = Sink::new(Path::new("r.md"));
        mid_sentence_bold(
            0..9,
            Some(Edit {
                range: 7..10,
                replacement: ":**".into(),
            }),
            &mut sink,
        );
        let fix = assert_some!(sink.finish().remove(0).fix);
        assert_eq!((fix.range, fix.replacement.as_str()), (7..10, ":**"));
    }

    #[test]
    fn image_warns() {
        let mut sink = Sink::new(Path::new("r.md"));
        image(2..8, &mut sink);
        let diagnostics = sink.finish();
        assert_eq!(diagnostics[0].code, DiagnosticCode::Image);
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 2..8);
    }

    #[test]
    fn too_many_contact_items_warns() {
        let line = "Seattle, WA · 206-555-0100 · a@b.co · [LinkedIn](https://l.in/x) · [GitHub](https://g.h/y)";
        let diagnostics = contact_diagnostics(line);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::TooManyContactItems);
        assert_eq!(
            diagnostics[0].message,
            "the contact line has 5 items; at most 4 are recommended"
        );
    }

    #[test]
    fn four_contact_items_do_not_warn() {
        let line = "Seattle, WA · a@b.co · [LinkedIn](https://l.in/x) · [GitHub](https://g.h/y)";
        assert_eq!(contact_diagnostics(line), vec![]);
        assert_eq!(contact_diagnostics("Just one item"), vec![]);
    }

    #[test]
    fn separator_inside_link_text_is_not_counted() {
        let line = "A · B · C · D · [Docs · Blog](https://x.y)";
        assert_eq!(
            contact_diagnostics(line).len(),
            1,
            "5 items; the dot inside the link text is not a separator"
        );
        let four = "A · B · C · [Docs · Blog](https://x.y)";
        assert_eq!(contact_diagnostics(four), vec![]);
    }

    #[test]
    fn alternative_separators_count_as_items() {
        let line = "A | B | C • D | E";
        assert_eq!(contact_diagnostics(line).len(), 1);
    }

    #[test]
    fn trailing_and_leading_separators_do_not_create_items() {
        let line = "· A · B · C · D ·";
        assert_eq!(contact_diagnostics(line), vec![]);
    }
}

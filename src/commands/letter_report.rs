//! Letter selection explanations contain identities and locations, never authored prose or field values.
use crate::{
    letter::{LetterRef, LetterReport},
    select::{ChosenBy, Scope, Selection},
};
use serde::Serialize;

#[cfg(test)]
#[path = "letter_report_tests.rs"]
mod tests;

#[derive(Serialize)]
pub(super) struct LetterSelectionRecord {
    #[serde(rename = "type")]
    kind: &'static str,
    file: String,
    scope: &'static str,
    tags: Vec<String>,
    prefer: Vec<String>,
    drafts: bool,
    kept: Vec<Location>,
    dropped: Vec<Dropped>,
    chosen: Vec<Chosen>,
    verify: Vec<Location>,
    skipped_drafts: Vec<String>,
    drafts_left_out: usize,
    drafts_included: usize,
}
#[derive(Serialize)]
struct Location {
    #[serde(skip)]
    same_file: bool,
    part: String,
    file: String,
    column: usize,
    line: usize,
}
#[derive(Serialize)]
struct Dropped {
    #[serde(flatten)]
    location: Location,
    reason: String,
}
#[derive(Serialize)]
struct Chosen {
    group: String,
    variant: String,
    by: String,
    not_chosen: Vec<String>,
}

// Matches SelectionSummary::is_silent; the two reports use different location types.
pub(super) fn is_silent(report: &LetterReport, selection: &Selection) -> bool {
    selection == &Selection::default()
        && report.dropped.is_empty()
        && report.verify.is_empty()
        && report.skipped_drafts.is_empty()
        && report.unknown_picks.is_empty()
        && report.drafts_left_out == 0
        && report.chosen.iter().all(|c| c.by == ChosenBy::Default)
}

impl LetterSelectionRecord {
    pub(super) fn new(
        file: String,
        input: &std::path::Path,
        sources: &crate::diagnostics::Sources<'_>,
        report: &LetterReport,
        selection: &Selection,
    ) -> Self {
        let location = |item: &LetterRef| {
            let found = sources
                .position(&item.origin.span())
                .expect("selected letter origins have registered sources");
            Location {
                same_file: item.origin.span().file == input,
                part: item.part.to_string(),
                file: found.file,
                line: found.line,
                column: found.column,
            }
        };
        Self {
            kind: "letter-selection",
            file,
            scope: match selection.scope {
                Scope::All => "all",
                Scope::CoreOnly => "core-only",
                Scope::Tags(_) => "tags",
            },
            tags: match &selection.scope {
                Scope::Tags(tags) => tags.as_set().iter().map(ToString::to_string).collect(),
                _ => vec![],
            },
            prefer: selection.prefer.iter().map(ToString::to_string).collect(),
            drafts: selection.drafts,
            kept: report.kept.iter().map(location).collect(),
            dropped: report
                .dropped
                .iter()
                .map(|d| Dropped {
                    location: location(&d.item),
                    reason: d.reason.to_string(),
                })
                .collect(),
            chosen: report
                .chosen
                .iter()
                .map(|c| Chosen {
                    group: c.group.to_string(),
                    variant: c.variant.to_string(),
                    by: c.by.to_string(),
                    not_chosen: c.not_chosen.iter().map(ToString::to_string).collect(),
                })
                .collect(),
            verify: report.verify.iter().map(location).collect(),
            skipped_drafts: report
                .skipped_drafts
                .iter()
                .map(|a| format!("{}.{}", a.group, a.variant))
                .collect(),
            drafts_left_out: report.drafts_left_out,
            drafts_included: report.drafts_included,
        }
    }
    pub(super) fn lines(&self, selection: &Selection) -> Vec<String> {
        let mut lines = vec![
            crate::select::selection_line(selection),
            format!("letter: {} kept, {} dropped", self.kept.len(), self.dropped.len()),
        ];
        for chosen in &self.chosen {
            lines.push(format!("wording {}.{}: {}", chosen.group, chosen.variant, chosen.by));
        }
        let at = |item: &Location| {
            if item.same_file {
                format!("line {}", item.line)
            } else {
                format!(
                    "{}:{}:{}",
                    crate::diagnostics::display_text(&item.file, false),
                    item.line,
                    item.column
                )
            }
        };
        for item in &self.kept {
            lines.push(format!("kept: {} at {}", item.part, at(item)));
        }
        for item in &self.dropped {
            lines.push(format!(
                "dropped: {} at {} ({})",
                item.location.part,
                at(&item.location),
                item.reason
            ));
        }
        for item in &self.verify {
            lines.push(format!("to verify: {} at {}", item.part, at(item)));
        }
        for name in &self.skipped_drafts {
            lines.push(format!("skipped draft wording: {name}"));
        }
        if self.drafts_left_out > 0 {
            lines.push(format!("drafts: {} left out", self.drafts_left_out));
        }
        if selection.drafts {
            lines.push(format!("drafts: {} included for preview", self.drafts_included));
        }
        lines
    }
}

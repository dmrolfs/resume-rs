//! ABOUTME: What a build tells a person about its selection: counts, the surprises one by one, core pages.
//! ABOUTME: One value feeds both the human lines and the JSON `selection` record, so the two cannot disagree.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{NodeKind, Resume};

use super::{ChosenBy, DropReason, Scope, Selection, SelectionReport};

/// How many nodes of one kind the baseline had, and how many the selection kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct KindCount {
    pub baseline: usize,
    pub kept: usize,
}

/// The pages of the core content alone, measured by compiling the core-only selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum CorePages {
    Measured {
        pages: usize,
    },
    /// Not measured, and why: the selection is already core only, or nothing is `@core`.
    Skipped {
        reason: CoreSkip,
    },
    /// The core-only compile failed; the build reports its error and fails.
    Failed,
}

/// Why the core-only page count was not measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum CoreSkip {
    /// The build is core only, so its own page count is the answer.
    AlreadyCoreOnly,
    /// Nothing in the resume is `@core`.
    NoCore,
    /// The style or the fonts could not be set up, so nothing could be compiled.
    NoSetup,
}

/// What a selection did, ready to print: the facts a person needs to catch an unintended cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionSummary {
    pub selection: Selection,
    /// Per kind, in the order sections, roles, eras, blurbs, paragraphs, bullets.
    pub counts: BTreeMap<NodeKind, KindCount>,
    pub report: SelectionReport,
    pub core: Option<CorePages>,
}

impl SelectionSummary {
    /// The summary of selecting `tailored` from `baseline` with `selection`, which produced `report`.
    #[must_use]
    pub fn new(baseline: &Resume, tailored: &Resume, selection: &Selection, report: &SelectionReport) -> Self {
        let mut counts: BTreeMap<NodeKind, KindCount> = BTreeMap::new();
        for node in baseline.nodes() {
            counts.entry(node.kind).or_default().baseline += 1;
        }
        for node in tailored.nodes() {
            counts.entry(node.kind).or_default().kept += 1;
        }
        Self {
            selection: selection.clone(),
            counts,
            report: report.clone(),
            core: None,
        }
    }

    /// True when the summary has nothing to say: the default selection kept everything and chose only defaults.
    ///
    /// Then a build prints nothing new, so a resume without annotations builds exactly as it did before them.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        let report = &self.report;
        self.selection == Selection::default()
            && report.dropped.is_empty()
            && report.verify.is_empty()
            && report.skipped_drafts.is_empty()
            && report.unknown_picks.is_empty()
            && report.drafts_left_out == 0
            && report.chosen.iter().all(|c| c.by == ChosenBy::Default)
    }

    /// The lines a person reads under the build line, without indentation; empty when [`Self::is_silent`].
    ///
    /// Routine events are counted; those that can surprise are listed one by one. `line_of` names a node's
    /// source line from its index in the baseline's nodes.
    #[must_use]
    pub fn lines(&self, line_of: &dyn Fn(usize) -> Option<usize>) -> Vec<String> {
        if self.is_silent() {
            return Vec::new();
        }
        let mut out = vec![self.selection_line(), self.counts_line()];
        if let Some(line) = self.wordings_line() {
            out.push(line);
        }
        let report = &self.report;
        let at = |node: &super::NodeRef| {
            let line = line_of(node.index).map_or_else(String::new, |l| format!(" at line {l}"));
            format!("{}{line} \"{}\"", node.kind, node.label)
        };
        for chosen in report.chosen.iter().filter(|c| c.by == ChosenBy::FirstReady) {
            out.push(format!(
                "wording {}.{}: its default is a draft, so the first ready wording is used",
                chosen.group, chosen.variant
            ));
        }
        for skipped in &report.skipped_drafts {
            out.push(format!("skipped draft wording: {}.{}", skipped.group, skipped.variant));
        }
        for dropped in &report.dropped {
            match dropped.reason {
                DropReason::InheritedTagsOnly => {
                    out.push(format!("dropped by an inherited tag: {}", at(&dropped.node)))
                }
                DropReason::Empty => out.push(format!("dropped as emptied: {}", at(&dropped.node))),
                _ => {}
            }
        }
        for node in &report.verify {
            out.push(format!("to verify: {}", at(node)));
        }
        if let Some(line) = self.drafts_line() {
            out.push(line);
        }
        if let Some(core) = self.core {
            out.push(match core {
                CorePages::Measured { pages } => {
                    format!("core alone: {pages} page{}", if pages == 1 { "" } else { "s" })
                }
                CorePages::Skipped { reason } => format!("core alone: not measured ({reason})"),
                CorePages::Failed => "core alone: not measured (the core-only compile failed)".to_string(),
            });
        }
        out
    }

    fn selection_line(&self) -> String {
        let selection = &self.selection;
        let mut parts = vec![match &selection.scope {
            Scope::All => "everything".to_string(),
            Scope::CoreOnly => "core only".to_string(),
            Scope::Tags(tags) => {
                let names: Vec<String> = tags.as_set().iter().map(ToString::to_string).collect();
                format!("tags {}", names.join(", "))
            }
        }];
        if !selection.prefer.is_empty() {
            let names: Vec<String> = selection.prefer.iter().map(ToString::to_string).collect();
            parts.push(format!("prefer {}", names.join(", ")));
        }
        if selection.drafts {
            parts.push("with drafts".to_string());
        }
        format!("selection: {}", parts.join("; "))
    }

    fn counts_line(&self) -> String {
        let plural = |kind: NodeKind, n: usize| {
            let name = kind.to_string();
            if n == 1 {
                format!("{n} {name}")
            } else {
                format!("{n} {name}s")
            }
        };
        let kept: Vec<String> = self.counts.iter().map(|(kind, c)| plural(*kind, c.kept)).collect();
        let dropped: Vec<String> = self
            .counts
            .iter()
            .filter(|(_, c)| c.baseline > c.kept)
            .map(|(kind, c)| plural(*kind, c.baseline - c.kept))
            .collect();
        let dropped = if dropped.is_empty() {
            "nothing".to_string()
        } else {
            dropped.join(", ")
        };
        // A wording not chosen is not content lost, so it is counted apart from what was dropped.
        let unchosen: usize = self.report.chosen.iter().map(|c| c.not_chosen.len()).sum();
        let others = if unchosen == 0 {
            String::new()
        } else {
            format!(
                " (of which {unchosen} other wording{})",
                if unchosen == 1 { "" } else { "s" }
            )
        };
        format!("kept: {}; dropped: {dropped}{others}", kept.join(", "))
    }

    /// One line counting the wordings by how they were chosen; `None` when the resume has no groups.
    fn wordings_line(&self) -> Option<String> {
        let chosen = &self.report.chosen;
        if chosen.is_empty() {
            return None;
        }
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        for c in chosen {
            *by.entry(c.by.to_string()).or_default() += 1;
        }
        let how: Vec<String> = by.iter().map(|(name, n)| format!("{n} {name}")).collect();
        Some(format!("wordings: {} chosen ({})", chosen.len(), how.join(", ")))
    }

    fn drafts_line(&self) -> Option<String> {
        let report = &self.report;
        match (self.selection.drafts, report.drafts_left_out, report.drafts_included) {
            (true, _, n) => Some(format!("drafts: {n} included (preview)")),
            (false, 0, _) => None,
            (false, n, _) => Some(format!("drafts left out: {n}")),
        }
    }
}

/// The JSON-lines `selection` record: one per build of a file whose summary is not silent.
///
/// Every key is always present, in this order; the contract is additive, like a finding's.
#[derive(Debug, Serialize)]
pub struct SelectionRecord<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    /// The input, as shown to people.
    file: &'a str,
    scope: &'static str,
    tags: Vec<String>,
    prefer: Vec<String>,
    drafts: bool,
    counts: BTreeMap<NodeKind, KindCount>,
    chosen: Vec<ChosenRecord>,
    dropped: Vec<DroppedRecord>,
    verify: Vec<NodeRecord>,
    skipped_drafts: Vec<String>,
    drafts_left_out: usize,
    drafts_included: usize,
    core: Option<CorePages>,
}

#[derive(Debug, Serialize)]
struct NodeRecord {
    kind: NodeKind,
    /// The 1-based source line where the node starts; `null` when it is not known.
    line: Option<usize>,
    label: String,
}

#[derive(Debug, Serialize)]
struct DroppedRecord {
    #[serde(flatten)]
    node: NodeRecord,
    reason: String,
}

#[derive(Debug, Serialize)]
struct ChosenRecord {
    group: String,
    variant: String,
    by: String,
    not_chosen: Vec<String>,
}

impl<'a> SelectionRecord<'a> {
    /// The record of `summary` for the input shown as `file`; `line_of` maps a node index to its source line.
    #[must_use]
    pub fn new(summary: &SelectionSummary, file: &'a str, line_of: &dyn Fn(usize) -> Option<usize>) -> Self {
        let selection = &summary.selection;
        let report = &summary.report;
        let node = |n: &super::NodeRef| NodeRecord {
            kind: n.kind,
            line: line_of(n.index),
            label: n.label.clone(),
        };
        let names = |v: &[crate::model::VariantName]| v.iter().map(ToString::to_string).collect();
        Self {
            kind: "selection",
            file,
            scope: match selection.scope {
                Scope::All => "all",
                Scope::Tags(_) => "tags",
                Scope::CoreOnly => "core-only",
            },
            tags: match &selection.scope {
                Scope::Tags(tags) => tags.as_set().iter().map(ToString::to_string).collect(),
                Scope::All | Scope::CoreOnly => Vec::new(),
            },
            prefer: names(&selection.prefer),
            drafts: selection.drafts,
            counts: summary.counts.clone(),
            chosen: report
                .chosen
                .iter()
                .map(|c| ChosenRecord {
                    group: c.group.to_string(),
                    variant: c.variant.to_string(),
                    by: c.by.to_string(),
                    not_chosen: names(&c.not_chosen),
                })
                .collect(),
            dropped: report
                .dropped
                .iter()
                .map(|d| DroppedRecord {
                    node: node(&d.node),
                    reason: d.reason.to_string(),
                })
                .collect(),
            verify: report.verify.iter().map(node).collect(),
            skipped_drafts: report
                .skipped_drafts
                .iter()
                .map(|a| format!("{}.{}", a.group, a.variant))
                .collect(),
            drafts_left_out: report.drafts_left_out,
            drafts_included: report.drafts_included,
            core: summary.core,
        }
    }
}

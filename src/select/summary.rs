//! ABOUTME: What a build tells a person about its selection: counts, the surprises one by one, core pages.
//! ABOUTME: One value feeds both the human lines and the JSON `selection` record, so the two cannot disagree.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{NodeKind, Resume};

use super::{ChosenBy, DropReason, Scope, Selection, SelectionReport};

/// A prepared node's report position, with source identity compared before display formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportLocation {
    pub position: crate::diagnostics::SourcePosition,
    pub same_input: bool,
}

impl ReportLocation {
    fn human(&self) -> String {
        if self.same_input {
            format!("line {}", self.position.line)
        } else {
            format!(
                "{}:{}:{}",
                crate::diagnostics::display_text(&self.position.file, false),
                self.position.line,
                self.position.column
            )
        }
    }
}

/// How many nodes of one kind the prepared input had, and how many the selection kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct KindCount {
    /// Selection input count, including spec additions; the wire key remains `baseline`.
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
    /// Routine events are counted; surprises are listed individually. The callback resolves prepared-input
    /// node indexes, never selected-output indexes or the original baseline's line vector.
    #[must_use]
    pub fn lines(&self, location_of: &dyn Fn(usize) -> Option<ReportLocation>) -> Vec<String> {
        if self.is_silent() {
            return Vec::new();
        }
        let mut out = vec![selection_line(&self.selection), self.counts_line()];
        if let Some(line) = self.wordings_line() {
            out.push(line);
        }
        let report = &self.report;
        let at = |node: &super::NodeRef| {
            let line = location_of(node.index).map_or_else(String::new, |l| format!(" at {}", l.human()));
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
        for node in &report.excluded_core {
            out.push(format!("core group excluded by explicit group list: {}", at(node)));
        }
        for node in &report.verify {
            out.push(format!("to verify: {}", at(node)));
        }
        out.extend(self.moved_up_lines(location_of, &at));
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

    /// One line per list in which `promote` moved something up, naming the risen items' lines top first; one line
    /// saying nothing moved when a promote list is set, so a typo or an already-first list is not taken for a bug.
    fn moved_up_lines(
        &self,
        location_of: &dyn Fn(usize) -> Option<ReportLocation>,
        at: &dyn Fn(&super::NodeRef) -> String,
    ) -> Vec<String> {
        if self.selection.promote.is_empty() {
            return Vec::new();
        }
        if self.report.reordered.is_empty() {
            return vec!["moved up: nothing".to_string()];
        }
        self.report
            .reordered
            .iter()
            .map(|r| {
                let locations: Option<Vec<ReportLocation>> =
                    r.moved_up.iter().map(|m| location_of(m.node.index)).collect();
                let lines = locations.map_or_else(String::new, |locations| {
                    if locations.iter().all(|l| l.same_input) {
                        let lines: Vec<_> = locations.iter().map(|l| l.position.line.to_string()).collect();
                        format!(
                            " ({} {})",
                            if lines.len() == 1 { "line" } else { "lines" },
                            lines.join(", ")
                        )
                    } else {
                        format!(
                            " ({})",
                            locations
                                .iter()
                                .map(ReportLocation::human)
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                });
                let what = if r.kept == 1 {
                    r.kind.to_string()
                } else {
                    format!("{}s", r.kind)
                };
                format!(
                    "moved up in {}: {} of {} {what}{lines}",
                    at(&r.container),
                    r.moved_up.len(),
                    r.kept
                )
            })
            .collect()
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
    groups: Option<Vec<String>>,
    prefer: Vec<String>,
    /// The tags whose items rise within their own list, most promoted first; empty unless a spec promotes.
    promote: Vec<String>,
    /// The wording each named group was told to render, by group name; empty unless a spec picked.
    picks: BTreeMap<String, String>,
    drafts: bool,
    counts: BTreeMap<NodeKind, KindCount>,
    chosen: Vec<ChosenRecord>,
    dropped: Vec<DroppedRecord>,
    verify: Vec<NodeRecord>,
    /// Each list in which `promote` moved something up, in document order.
    reordered: Vec<ReorderedRecord>,
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
    file: Option<String>,
    column: Option<usize>,
    label: String,
}

#[derive(Debug, Serialize)]
struct DroppedRecord {
    #[serde(flatten)]
    node: NodeRecord,
    reason: String,
}

#[derive(Debug, Serialize)]
struct ReorderedRecord {
    container: NodeRecord,
    /// What the list holds: `bullet` or `paragraph`.
    kind: NodeKind,
    kept: usize,
    moved_up: Vec<MovedUpRecord>,
}

#[derive(Debug, Serialize)]
struct MovedUpRecord {
    #[serde(flatten)]
    node: NodeRecord,
    /// 1-based positions among the list's kept items.
    from: usize,
    to: usize,
}

#[derive(Debug, Serialize)]
struct ChosenRecord {
    group: String,
    variant: String,
    by: String,
    not_chosen: Vec<String>,
}

pub(crate) fn selection_line(selection: &Selection) -> String {
    let mut parts = vec![match &selection.scope {
        Scope::All => "everything".to_string(),
        Scope::CoreOnly => "core only".to_string(),
        Scope::Tags(tags) => format!(
            "tags {}",
            tags.as_set()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }];
    if let Some(groups) = &selection.groups {
        parts[0] = format!("ungrouped: {}", parts[0]);
        let names = if groups.is_empty() {
            "none".to_string()
        } else {
            groups.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
        };
        parts.insert(0, format!("groups {names}"));
    }
    if !selection.prefer.is_empty() {
        parts.push(format!(
            "prefer {}",
            selection
                .prefer
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !selection.promote.is_empty() {
        let promote: Vec<String> = selection.promote.iter().map(ToString::to_string).collect();
        parts.push(format!("promote {}", promote.join(", ")));
    }
    if !selection.picks.is_empty() {
        let picks: Vec<String> = selection.picks.iter().map(|(g, v)| format!("{g}.{v}")).collect();
        parts.push(format!("picks {}", picks.join(", ")));
    }
    if selection.drafts {
        parts.push("with drafts".to_string());
    }
    format!("selection: {}", parts.join("; "))
}

impl<'a> SelectionRecord<'a> {
    /// The record of `summary` for `file`; the callback resolves prepared-input node origins.
    #[must_use]
    pub fn new(
        summary: &SelectionSummary,
        file: &'a str,
        location_of: &dyn Fn(usize) -> Option<ReportLocation>,
    ) -> Self {
        let selection = &summary.selection;
        let report = &summary.report;
        let node = |n: &super::NodeRef| {
            let location = location_of(n.index);
            NodeRecord {
                kind: n.kind,
                line: location.as_ref().map(|l| l.position.line),
                file: location.as_ref().map(|l| l.position.file.clone()),
                column: location.as_ref().map(|l| l.position.column),
                label: n.label.clone(),
            }
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
            groups: selection
                .groups
                .as_ref()
                .map(|groups| groups.iter().map(ToString::to_string).collect()),
            prefer: names(&selection.prefer),
            promote: selection.promote.iter().map(ToString::to_string).collect(),
            picks: selection
                .picks
                .iter()
                .map(|(g, v)| (g.to_string(), v.to_string()))
                .collect(),
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
            reordered: report
                .reordered
                .iter()
                .map(|r| ReorderedRecord {
                    container: node(&r.container),
                    kind: r.kind,
                    kept: r.kept,
                    moved_up: r
                        .moved_up
                        .iter()
                        .map(|m| MovedUpRecord {
                            node: node(&m.node),
                            from: m.from,
                            to: m.to,
                        })
                        .collect(),
                })
                .collect(),
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

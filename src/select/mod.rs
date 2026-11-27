//! ABOUTME: Tailoring: turns one annotated baseline into the document for one application, with a report.
//! ABOUTME: `rules` holds the keep and wording rules letters share; `resume` walks a resume with them.

use std::collections::{BTreeMap, BTreeSet};

use snafu::Snafu;

use crate::model::{Alternate, GroupName, Resume, Tag, VariantName};

#[cfg(test)]
mod group_tests;
mod inventory;
mod order;
mod resume;
pub mod rules;
pub(crate) use self::summary::selection_line;
#[cfg(test)]
mod order_tests;
mod summary;
#[cfg(test)]
mod tests;

pub use self::inventory::{GroupUse, Inventory, KeywordUse, TagUse, VariantUse};
pub use self::resume::{Chosen, Dropped, MovedUp, NodeRef, Reordered, SelectionReport, UnknownPick, select};
pub use self::rules::{ChosenBy, DropReason};
pub use self::summary::{CorePages, CoreSkip, KindCount, ReportLocation, SelectionRecord, SelectionSummary};
pub use crate::model::NodeKind;

/// Shared unknown-pick policy; each document supplies its own group inventory.
pub(crate) fn unknown_picks(groups: &BTreeMap<GroupName, Vec<VariantName>>, selection: &Selection) -> Vec<UnknownPick> {
    selection
        .picks
        .iter()
        .filter_map(|(group, variant)| match groups.get(group) {
            None => Some(UnknownPick::Group(group.clone())),
            Some(found) if !found.contains(variant) => Some(UnknownPick::Variant(Alternate {
                group: group.clone(),
                variant: variant.clone(),
            })),
            Some(_) => None,
        })
        .collect()
}

/// What to keep and which wordings to use. The default keeps everything except drafts and renders each
/// group's `default`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub scope: Scope,
    /// Exact resume wording-group membership; `None` uses ordinary scope rules.
    pub groups: Option<BTreeSet<GroupName>>,
    /// Put every draft back: a preview of what the document will say once the drafts are ready.
    pub drafts: bool,
    /// Variant names to prefer, most preferred first.
    pub prefer: Vec<VariantName>,
    /// The variant each named group renders, overriding `prefer`.
    pub picks: BTreeMap<GroupName, VariantName>,
    /// Tags whose items rise within their own list (a list of bullets, or a run of adjacent paragraphs), most
    /// promoted first; the first place among an item's own and inherited tags decides its rank. Only a spec
    /// sets it, and a letter's selection leaves it empty: letters keep their authored order.
    pub promote: Vec<Tag>,
}

/// Which items a selection keeps, before drafts and wordings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Scope {
    /// Everything.
    #[default]
    All,
    /// `@core` items, items tagged with one of these tags, and untagged items.
    Tags(SelectedTags),
    /// `@core` items only.
    CoreOnly,
}

/// The tags a selection asks for: never empty, because an empty set would keep only untagged items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedTags(BTreeSet<Tag>);

impl SelectedTags {
    /// The tags, or `None` for an empty set.
    #[must_use]
    pub fn new(tags: BTreeSet<Tag>) -> Option<Self> {
        (!tags.is_empty()).then_some(Self(tags))
    }

    #[must_use]
    pub fn as_set(&self) -> &BTreeSet<Tag> {
        &self.0
    }
}

/// The names a selection asks for that the resume does not use, with what it does use, so a typo is caught
/// before a build quietly selects nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnknownNames {
    /// Tags asked for that no node carries, in the order asked.
    pub tags: Vec<Tag>,
    /// Every tag the resume uses, sorted.
    pub known_tags: Vec<Tag>,
    /// Preferred wordings that no group has, in the order asked.
    pub wordings: Vec<VariantName>,
    /// Every wording name the resume's groups use, sorted.
    pub known_wordings: Vec<VariantName>,
    /// Promote tags that no node carries, in promote order, each once.
    pub promote: Vec<Tag>,
}

impl UnknownNames {
    /// Checks `selection`'s tags, preferred wordings, and promote tags against the names `resume` uses, drafts included.
    #[must_use]
    pub fn of(resume: &Resume, selection: &Selection) -> Self {
        let nodes = resume.nodes();
        Self::from_annotations(nodes.iter().map(|n| n.annotations), selection)
    }

    /// Checks all authored annotations, including drafts and unselected variants.
    #[must_use]
    pub fn from_annotations<'a>(
        annotations: impl Iterator<Item = &'a crate::model::Annotations>,
        selection: &Selection,
    ) -> Self {
        let annotations: Vec<_> = annotations.collect();
        let known_tags: BTreeSet<&Tag> = annotations.iter().flat_map(|a| &a.tags).collect();
        let known_wordings: BTreeSet<&VariantName> = annotations
            .iter()
            .filter_map(|a| a.alternate.as_ref())
            .map(|a| &a.variant)
            .collect();
        let asked: Vec<&Tag> = match &selection.scope {
            Scope::Tags(tags) => tags.as_set().iter().collect(),
            Scope::All | Scope::CoreOnly => Vec::new(),
        };
        let mut wordings: Vec<VariantName> = Vec::new();
        for name in selection.prefer.iter().filter(|n| !known_wordings.contains(n)) {
            if !wordings.contains(name) {
                wordings.push(name.clone());
            }
        }
        let mut promote: Vec<Tag> = Vec::new();
        for tag in selection.promote.iter().filter(|t| !known_tags.contains(t)) {
            if !promote.contains(tag) {
                promote.push(tag.clone());
            }
        }
        Self {
            promote,
            tags: asked.into_iter().filter(|t| !known_tags.contains(t)).cloned().collect(),
            known_tags: known_tags.into_iter().cloned().collect(),
            wordings,
            known_wordings: known_wordings.into_iter().cloned().collect(),
        }
    }

    /// True when every name asked for is used.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.wordings.is_empty() && self.promote.is_empty()
    }
}

/// A selection that cannot be met as written.
#[derive(Debug, Clone, Snafu, PartialEq, Eq)]
pub enum SelectError {
    /// Picks name wordings that are still drafts, and drafts are off. Every such pick, in document order.
    #[snafu(display(
        "{} picked {} still a draft: {}; finish {}, pick another wording, or build with --drafts to preview",
        if picks.len() == 1 { "a" } else { "the" },
        if picks.len() == 1 { "wording is" } else { "wordings are" },
        listed(picks),
        if picks.len() == 1 { "it" } else { "them" }
    ))]
    PickedDraft { picks: Vec<Alternate> },
    #[snafu(display("unknown requested groups: {}", groups.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")))]
    UnknownGroups { groups: Vec<GroupName> },
    #[snafu(display("requested groups cannot render: {}", groups.iter().map(|g| format!("{} ({})", g.group, g.reason)).collect::<Vec<_>>().join(", ")))]
    UnavailableGroups { groups: Vec<UnavailableGroup> },
}

/// A known requested group excluded by draft gating or lack of ready wording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableGroup {
    pub group: GroupName,
    pub reason: DropReason,
}

pub(crate) fn listed(picks: &[Alternate]) -> String {
    picks
        .iter()
        .map(|a| format!("`{}.{}`", a.group, a.variant))
        .collect::<Vec<_>>()
        .join(", ")
}

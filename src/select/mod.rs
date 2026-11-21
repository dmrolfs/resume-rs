//! ABOUTME: Tailoring: turns one annotated baseline into the document for one application, with a report.
//! ABOUTME: `rules` holds the keep and wording rules letters share; `resume` walks a resume with them.

use std::collections::{BTreeMap, BTreeSet};

use snafu::Snafu;

use crate::model::{Alternate, GroupName, Tag, VariantName};

mod resume;
pub mod rules;
#[cfg(test)]
mod tests;

pub use self::resume::{Chosen, Dropped, NodeKind, NodeRef, SelectionReport, UnknownPick, select};
pub use self::rules::{ChosenBy, DropReason};

/// What to keep and which wordings to use. The default keeps everything except drafts and renders each
/// group's `default`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub scope: Scope,
    /// Put every draft back: a preview of what the document will say once the drafts are ready.
    pub drafts: bool,
    /// Variant names to prefer, most preferred first.
    pub prefer: Vec<VariantName>,
    /// The variant each named group renders, overriding `prefer`.
    pub picks: BTreeMap<GroupName, VariantName>,
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

/// A selection that cannot be met as written.
#[derive(Debug, Snafu, PartialEq, Eq)]
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
}

fn listed(picks: &[Alternate]) -> String {
    picks
        .iter()
        .map(|a| format!("`{}.{}`", a.group, a.variant))
        .collect::<Vec<_>>()
        .join(", ")
}

//! Source-owned content and a projection of shared-engine decisions, never selection rules.
use ::resume::{
    diagnostics::ItemOrigin,
    model::{Annotations, Tag},
    select::{ChosenBy, DropReason},
};
use std::collections::BTreeSet;

mod letter;
mod render;
#[cfg(test)]
mod render_tests;
mod resume;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, strum::Display)]
pub enum Outcome {
    Unselected,
    Unavailable,
    Kept,
    #[strum(serialize = "Dropped: {0}")]
    Dropped(DropReason),
    #[strum(serialize = "Chosen: {0}")]
    Chosen(ChosenBy),
    #[strum(serialize = "Not chosen")]
    NotChosen,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, strum::Display)]
pub enum Filter {
    #[default]
    All,
    Kept,
    Dropped,
}
impl Filter {
    pub fn includes(self, outcome: &Outcome) -> bool {
        match self {
            Self::All => true,
            Self::Kept => matches!(outcome, Outcome::Kept | Outcome::Chosen(_)),
            Self::Dropped => matches!(outcome, Outcome::Dropped(_) | Outcome::NotChosen),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentItem {
    pub origin: ItemOrigin,
    pub annotations: Annotations,
    pub group_tags: BTreeSet<Tag>,
    pub inherited_tags: BTreeSet<Tag>,
    pub depth: usize,
    pub text: String,
    pub outcome: Outcome,
    pub promoted: Option<(usize, usize)>,
    pub mandatory: bool,
}
impl ContentItem {
    pub fn chips(&self) -> Vec<String> {
        let a = &self.annotations;
        let mut chips = Vec::new();
        chips.extend(a.id.as_ref().map(|id| format!("&{id}")));
        chips.extend(a.priority.map(|p| format!("@{p}")));
        chips.extend(a.status.map(|s| format!("@{s}")));
        chips.extend(a.tags.iter().map(|t| format!("#{t}")));
        chips.extend(
            self.inherited_tags
                .difference(&a.tags)
                .map(|t| format!("#{t} (inherited)")),
        );
        chips.extend(self.group_tags.difference(&a.tags).map(|t| format!("#{t} (group)")));
        chips.extend(a.alternate.as_ref().map(|a| format!("={}.{}", a.group, a.variant)));
        if self.mandatory {
            chips.push("Mandatory frame".into());
        }
        if matches!(self.origin, ItemOrigin::SpecContent { .. }) {
            chips.push("Spec addition".into());
        }
        chips.push(self.outcome.to_string());
        if let Some((from, to)) = self.promoted {
            chips.push(format!("Promoted {from} → {to}"));
        }
        chips
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Content {
    pub problems: Vec<super::problem::Problem>,
    pub items: Vec<ContentItem>,
    /// Shared-engine selected/resolved output, or all authored content with no spec.
    pub rendered: Option<String>,
    pub assessed: bool,
}
impl Content {
    /// Ancestors remain as context, without changing their actual outcome.
    pub fn visible(&self, filter: Filter) -> Vec<(usize, bool)> {
        let mut visible = vec![false; self.items.len()];
        let mut parents = Vec::<usize>::new();
        for (index, item) in self.items.iter().enumerate() {
            while parents.last().is_some_and(|&i| self.items[i].depth >= item.depth) {
                parents.pop();
            }
            if filter.includes(&item.outcome) {
                visible[index] = true;
                for &parent in &parents {
                    visible[parent] = true;
                }
            }
            parents.push(index);
        }
        visible
            .into_iter()
            .enumerate()
            .filter(|(_, yes)| *yes)
            .map(|(i, _)| (i, !filter.includes(&self.items[i].outcome)))
            .collect()
    }

    pub fn unavailable(&mut self) {
        self.assessed = false;
        self.rendered = None;
        for item in &mut self.items {
            item.outcome = Outcome::Unavailable;
            item.promoted = None;
        }
    }
}

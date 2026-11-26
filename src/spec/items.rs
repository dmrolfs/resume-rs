//! ABOUTME: Applies a position spec's per-item overrides to a copy of the parsed baseline, before selection.
//! ABOUTME: Pure: no files, no CLI, no selection. Targets are checked first, so a missing one never half-applies.

use std::collections::{BTreeMap, BTreeSet};

use snafu::Snafu;

use super::error::Located;
use super::merge::{ChoiceKey, ItemOverride};
use crate::letter::LetterSource;
use crate::model::{Annotations, ItemId, Resume, Tag};

/// The item overrides of one document, with where each field was written.
///
/// A resume and a letter each have their own, because `[select]` and `[letter.select]` choose the document; the
/// private fields mean the two cannot be paired by mistake.
#[derive(Debug, Clone, Copy)]
pub struct ItemOverrides<'a> {
    overrides: &'a BTreeMap<ItemId, ItemOverride>,
    origins: &'a BTreeMap<ChoiceKey, Located>,
}

impl<'a> ItemOverrides<'a> {
    pub(crate) fn new(
        overrides: &'a BTreeMap<ItemId, ItemOverride>,
        origins: &'a BTreeMap<ChoiceKey, Located>,
    ) -> Self {
        Self { overrides, origins }
    }

    /// True when the spec changes no item.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty()
    }
}

/// One override whose target is not in the document, with every line that wrote a field of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingTarget {
    pub id: ItemId,
    /// The places that wrote the tags and the priority of this target, in file order then line order.
    pub at: Vec<Located>,
}

/// Why a spec's item overrides cannot be applied to a document.
#[derive(Debug, Clone, Snafu, PartialEq, Eq)]
pub enum PrepareError {
    /// An override names an `&id` the document does not have. Every such target, in id order.
    #[snafu(display("{} names nothing in the content file:\n  {}", describe(missing), lines(missing)))]
    UnknownTargets { missing: Vec<MissingTarget> },
}

fn describe(missing: &[MissingTarget]) -> String {
    match missing {
        [one] => format!("override of `{}`", one.id),
        many => format!("{} overrides", many.len()),
    }
}

fn lines(missing: &[MissingTarget]) -> String {
    missing
        .iter()
        .map(|m| {
            let at: Vec<String> = m.at.iter().map(ToString::to_string).collect();
            format!("{}: the content has no item `&{}`", at.join(", "), m.id)
        })
        .collect::<Vec<_>>()
        .join("\n  ")
}

/// A copy of `baseline` with `overrides` applied, or the targets that are not there.
///
/// Every target is checked before anything is changed, so a missing one is reported even when the item it meant
/// would have been dropped by the selection.
///
/// # Errors
/// [`PrepareError::UnknownTargets`] listing every override whose `&id` the baseline does not have.
pub fn prepare_resume(baseline: &Resume, overrides: ItemOverrides<'_>) -> Result<Resume, PrepareError> {
    let mut prepared = baseline.clone();
    if overrides.is_empty() {
        return Ok(prepared);
    }
    let mut slots = prepared.node_annotations_mut();
    check_targets(slots.iter().map(|a| &**a), overrides)?;
    apply(&mut slots, overrides);
    Ok(prepared)
}

/// A copy of `baseline` with `overrides` applied to its body paragraphs.
///
/// # Errors
/// [`PrepareError::UnknownTargets`] for an id no body paragraph or group has.
pub fn prepare_letter(baseline: &LetterSource, overrides: ItemOverrides<'_>) -> Result<LetterSource, PrepareError> {
    let mut prepared = baseline.clone();
    if overrides.is_empty() {
        return Ok(prepared);
    }
    let mut slots = prepared.body_annotations_mut();
    check_targets(slots.iter().map(|a| &**a), overrides)?;
    apply(&mut slots, overrides);
    Ok(prepared)
}

/// The ids an override may name: an explicit `&id`, or the group name of any variant.
fn name_of(annotations: &Annotations) -> Option<ItemId> {
    annotations.id.as_deref().cloned().or_else(|| {
        annotations
            .alternate
            .as_ref()
            .and_then(|a| ItemId::try_new(a.group.as_ref().to_string()).ok())
    })
}

fn check_targets<'a>(
    nodes: impl Iterator<Item = &'a Annotations>,
    overrides: ItemOverrides<'_>,
) -> Result<(), PrepareError> {
    let present: BTreeSet<ItemId> = nodes.filter_map(name_of).collect();
    let missing: Vec<MissingTarget> = overrides
        .overrides
        .keys()
        .filter(|id| !present.contains(*id))
        .map(|id| MissingTarget {
            id: id.clone(),
            at: [
                ChoiceKey::OverrideTags(id.clone()),
                ChoiceKey::OverridePriority(id.clone()),
            ]
            .iter()
            .filter_map(|key| overrides.origins.get(key).cloned())
            .collect(),
        })
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        UnknownTargetsSnafu { missing }.fail()
    }
}

/// Writes each override into every node it names: the one node an `&id` marks, or every variant of a group.
fn apply(slots: &mut [&mut Annotations], overrides: ItemOverrides<'_>) {
    for slot in slots {
        let Some(name) = name_of(slot) else { continue };
        let Some(change) = overrides.overrides.get(&name) else {
            continue;
        };
        if let Some(tags) = &change.tags {
            slot.tags = tags.iter().cloned().collect::<BTreeSet<Tag>>();
        }
        if let Some(priority) = change.priority {
            slot.priority = priority.applied();
        }
    }
}

#[cfg(test)]
mod tests;

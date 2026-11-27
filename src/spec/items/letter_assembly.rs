//! ABOUTME: Binds letter additions to immutable body gaps, then merges them into an overridden baseline copy.
//! ABOUTME: Groups are logical items, sources travel with pieces, and additions never become override targets.

use super::{BindingProblem, BindingReason, ItemOverrides, PrepareError};
use crate::letter::{Item, LetterSource, Piece, SourceItem};
use crate::model::{Annotations, ItemId};
use crate::spec::additions::{LetterAddition, LetterPlace};
use crate::spec::layer::Sourced;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
enum Target {
    Body(usize),
    Frame,
}

/// Assembles additions into a fresh copy before normal letter selection.
///
/// `baseline` must be a valid parsed letter; `baseline_bytes` is the original Markdown byte length,
/// including comments and drafts. The input budget and snapshot consistency are checked first,
/// then binding errors are reported together, followed by ordinary override errors. No failure modifies the baseline or returns a partial document.
/// All callers must register addition snapshots before reporting the assembled letter.
pub fn assemble_letter(
    baseline: &LetterSource,
    baseline_bytes: usize,
    additions: &[LetterAddition],
    overrides: Option<ItemOverrides<'_>>,
) -> Result<LetterSource, PrepareError> {
    check_budget(baseline_bytes, additions)?;
    let targets = index(baseline);
    let mut problems = Vec::new();
    let ids = check_ids(additions, &targets, &mut problems);
    let mut gaps: Vec<Vec<&LetterAddition>> = vec![Vec::new(); baseline.body.len() + 1];
    for addition in additions {
        if let Some(gap) = bind(addition.place(), baseline.body.len(), &targets, &ids, &mut problems) {
            gaps[gap].push(addition);
        }
    }
    if let Some(overrides) = overrides {
        for id in overrides.overrides.keys().filter(|id| ids.contains(*id)) {
            for at in overrides.written_at(id) {
                problems.push(BindingProblem {
                    id: id.clone(),
                    at,
                    reason: BindingReason::AdditionTarget,
                });
            }
        }
    }
    if !problems.is_empty() {
        return Err(PrepareError::AdditionBinding { problems });
    }
    let mut prepared = match overrides {
        Some(overrides) => super::prepare_letter(baseline, overrides)?,
        None => baseline.clone(),
    };
    let body = std::mem::take(&mut prepared.body);
    for (gap, next) in gaps
        .into_iter()
        .zip(body.into_iter().map(Some).chain(std::iter::once(None)))
    {
        prepared.body.extend(gap.into_iter().map(item));
        prepared.body.extend(next);
    }
    Ok(prepared)
}

fn names(annotations: &Annotations) -> impl Iterator<Item = ItemId> + '_ {
    annotations.id.as_deref().cloned().into_iter().chain(
        annotations
            .alternate
            .as_ref()
            .map(|a| ItemId::try_new(a.group.as_ref()).expect("group names and item ids have the same grammar")),
    )
}

fn items(source: &SourceItem) -> &[Item] {
    match source {
        SourceItem::Paragraph(item) => std::slice::from_ref(item),
        SourceItem::Alternates(group) => group.variants(),
    }
}

fn index(baseline: &LetterSource) -> BTreeMap<ItemId, Target> {
    let mut targets = BTreeMap::new();
    for source in [&baseline.greeting, &baseline.closing] {
        for item in items(source) {
            for name in names(&item.annotations) {
                targets.insert(name, Target::Frame);
            }
        }
    }
    for (i, source) in baseline.body.iter().enumerate() {
        for item in items(source) {
            for name in names(&item.annotations) {
                targets.insert(name, Target::Body(i));
            }
        }
    }
    targets
}

fn check_ids(
    additions: &[LetterAddition],
    targets: &BTreeMap<ItemId, Target>,
    problems: &mut Vec<BindingProblem>,
) -> BTreeSet<ItemId> {
    let mut ids = BTreeSet::new();
    for addition in additions {
        let id = addition.id();
        if !ids.insert(id.value.clone()) {
            problem(id, BindingReason::DuplicateAddition, problems);
        }
        if targets.contains_key(&id.value) {
            problem(id, BindingReason::BaselineCollision, problems);
        }
    }
    ids.retain(|id| !targets.contains_key(id));
    ids
}

fn bind(
    place: &LetterPlace,
    end: usize,
    targets: &BTreeMap<ItemId, Target>,
    ids: &BTreeSet<ItemId>,
    problems: &mut Vec<BindingProblem>,
) -> Option<usize> {
    let (anchor, after) = match place {
        LetterPlace::Start(_) => return Some(0),
        LetterPlace::End(_) => return Some(end),
        LetterPlace::Before(anchor) => (anchor, false),
        LetterPlace::After(anchor) => (anchor, true),
    };
    if ids.contains(&anchor.value) {
        problem(anchor, BindingReason::AdditionTarget, problems);
        return None;
    }
    match targets.get(&anchor.value) {
        Some(Target::Body(index)) => Some(index + usize::from(after)),
        Some(Target::Frame) => {
            problem(anchor, BindingReason::FrameTarget, problems);
            None
        }
        None => {
            problem(anchor, BindingReason::MissingReference, problems);
            None
        }
    }
}

fn problem(id: &Sourced<ItemId>, reason: BindingReason, problems: &mut Vec<BindingProblem>) {
    problems.push(BindingProblem {
        id: id.value.clone(),
        at: id.at.clone(),
        reason,
    });
}

fn check_budget(baseline_bytes: usize, additions: &[LetterAddition]) -> Result<(), PrepareError> {
    let added_bytes = additions.iter().try_fold(0usize, |bytes, addition| {
        bytes.checked_add(addition.content().markdown().len())
    });
    let total = added_bytes.and_then(|added| baseline_bytes.checked_add(added));
    if total.is_none_or(|bytes| bytes > crate::markdown::MAX_INPUT_BYTES) {
        return Err(PrepareError::PreparedContentLimit {
            baseline_bytes,
            added_bytes,
        });
    }
    let mut sources = BTreeMap::new();
    for addition in additions {
        let source = addition.content().source();
        if let Some(old) = sources.insert(source.path(), source.text())
            && old != source.text()
        {
            return Err(PrepareError::ConflictingAdditionSources {
                path: source.path().to_owned(),
            });
        }
    }
    Ok(())
}

fn item(addition: &LetterAddition) -> SourceItem {
    let content = addition.content();
    SourceItem::Paragraph(Item {
        piece: Piece {
            content: content.value().clone(),
            range: 0..content.markdown().len(),
            origin: content.origin().clone(),
        },
        annotations: addition.metadata().annotations(&addition.id().value),
        annotation_range: None,
    })
}

#[cfg(test)]
mod tests;

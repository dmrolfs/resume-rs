//! ABOUTME: Binds resume additions to immutable, model-shaped gaps and rebuilds with preorder origins.
//! ABOUTME: Container-edge aliases share one bucket; groups and pre-existing lists are never split by placement.

mod bind;
mod emit;
#[cfg(test)]
mod tests;

use super::{BindingProblem, BindingReason, ItemOverrides, PrepareError};
use crate::diagnostics::ItemOrigin;
use crate::model::{ItemId, Resume};
use crate::pipeline::PreparedResume;
use crate::spec::additions::ResumeAddition;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Owner {
    Section(usize),
    Role(usize, usize),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct List {
    owner: Owner,
    block: usize,
    era: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Gap {
    Blocks(Owner, usize),
    Items(List, usize),
    Blurb(Owner),
}

/// Inserts checked leaves into a valid parsed baseline without modifying it.
/// `baseline_origins` must describe that baseline in node preorder; `baseline_bytes` is its original
/// Markdown byte length. Budget, source consistency and binding are checked before cloning or emission.
/// The caller registers the baseline and addition snapshots when reporting the prepared result.
pub fn assemble_resume(
    baseline: &Resume,
    baseline_origins: &[ItemOrigin],
    baseline_bytes: usize,
    additions: &[ResumeAddition],
    overrides: Option<ItemOverrides<'_>>,
) -> Result<PreparedResume, PrepareError> {
    check_budget(baseline_bytes, additions)?;
    assert_eq!(
        baseline.nodes().len(),
        baseline_origins.len(),
        "baseline origins follow parsed preorder"
    );
    let index = bind::Index::new(baseline);
    let mut problems = Vec::new();
    let ids = check_ids(additions, &index, &mut problems);
    let mut gaps: BTreeMap<Gap, Vec<usize>> = BTreeMap::new();
    for (i, addition) in additions.iter().enumerate() {
        if let Some(gap) = index.bind(addition, &ids, &mut problems) {
            let entries = gaps.entry(gap).or_default();
            if matches!(gap, Gap::Blurb(_)) && !entries.is_empty() {
                problems.push(BindingProblem {
                    id: addition.id().value.clone(),
                    at: addition.id().at.clone(),
                    reason: BindingReason::OccupiedBlurb,
                });
            }
            entries.push(i);
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
    let prepared = match overrides {
        Some(overrides) => super::prepare_resume(baseline, overrides)?,
        None => baseline.clone(),
    };
    Ok(emit::assemble(prepared, baseline_origins, additions, gaps))
}

fn check_ids(
    additions: &[ResumeAddition],
    index: &bind::Index,
    problems: &mut Vec<BindingProblem>,
) -> BTreeSet<ItemId> {
    let mut ids = BTreeSet::new();
    for addition in additions {
        let id = addition.id();
        if !ids.insert(id.value.clone()) {
            problems.push(BindingProblem {
                id: id.value.clone(),
                at: id.at.clone(),
                reason: BindingReason::DuplicateAddition,
            });
        }
        if index.contains(&id.value) {
            problems.push(BindingProblem {
                id: id.value.clone(),
                at: id.at.clone(),
                reason: BindingReason::BaselineCollision,
            });
        }
    }
    ids.retain(|id| !index.contains(id));
    ids
}

fn check_budget(baseline_bytes: usize, additions: &[ResumeAddition]) -> Result<(), PrepareError> {
    let added_bytes = additions
        .iter()
        .try_fold(0usize, |n, a| n.checked_add(a.content().markdown().len()));
    if added_bytes
        .and_then(|n| baseline_bytes.checked_add(n))
        .is_none_or(|n| n > crate::markdown::MAX_INPUT_BYTES)
    {
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

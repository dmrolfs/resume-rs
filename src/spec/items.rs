//! ABOUTME: Applies a position spec's per-item overrides to a copy of the parsed baseline, before selection.
//! ABOUTME: Pure: no files, no CLI, no selection. Targets are checked first, so a missing one never half-applies.

use std::collections::{BTreeMap, BTreeSet};

use snafu::Snafu;

use super::error::Located;
use super::merge::{ChoiceKey, ItemOverride};
use crate::letter::LetterSource;
use crate::model::{Annotations, ItemId, Resume, Tag};

mod letter_assembly;
pub use letter_assembly::assemble_letter;
mod resume_assembly;
pub use resume_assembly::assemble_resume;

/// A declaration or reference that could not bind to the unchanged baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingProblem {
    pub id: ItemId,
    pub at: Located,
    pub reason: BindingReason,
}

#[derive(Debug, Clone, PartialEq, Eq, strum::Display)]
pub enum BindingReason {
    #[strum(serialize = "placement is incompatible with this baseline item kind")]
    WrongTargetKind,
    #[strum(serialize = "the role blurb slot is already occupied")]
    OccupiedBlurb,
    #[strum(serialize = "addition id is declared more than once")]
    DuplicateAddition,
    #[strum(serialize = "addition id is already used by a baseline item or group")]
    BaselineCollision,
    #[strum(serialize = "additions are edited directly and cannot be anchors or override targets")]
    AdditionTarget,
    #[strum(serialize = "reference names no baseline item or group")]
    MissingReference,
    #[strum(serialize = "letter greeting and closing are not editable by this operation")]
    FrameTarget,
}

/// The item overrides of one document, with where each field was written.
///
/// A resume and a letter each have their own, because `[select]` and `[letter.select]` choose the document. The
/// fields are private, so a view always pairs its overrides with their own origins; which document it belongs to
/// is the caller's to keep straight: use `ResolvedSpec::item_overrides` for a resume and
/// `ResolvedSpec::letter_item_overrides` for a letter.
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

    /// The places that wrote a field of `id`, in the order they were written: a less specific file first, and within
    /// one file by line. Not tags before priority, which would put a later line first.
    fn written_at(&self, id: &ItemId) -> Vec<Located> {
        let mut at: Vec<Located> = [
            ChoiceKey::OverrideTags(id.clone()),
            ChoiceKey::OverridePriority(id.clone()),
        ]
        .iter()
        .filter_map(|key| self.origins.get(key).cloned())
        .collect();
        at.sort_by(|a, b| (a.path.as_ref(), a.line, a.column).cmp(&(b.path.as_ref(), b.line, b.column)));
        at
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
    /// The places that wrote the tags and the priority of this target, in the order they were written: the less
    /// specific file first, and within a file by line.
    pub at: Vec<Located>,
}

/// An override that names a letter's greeting or closing wording group, which exists but is not an item a spec can
/// change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameTarget {
    pub id: ItemId,
    /// `greeting` or `closing`.
    pub section: &'static str,
    pub at: Vec<Located>,
}

/// Why a spec's item overrides cannot be applied to a document.
#[derive(Debug, Clone, Snafu, PartialEq, Eq)]
pub enum PrepareError {
    #[snafu(display("cannot place additions:\n{}", binding_lines(problems)))]
    AdditionBinding { problems: Vec<BindingProblem> },

    #[snafu(display("prepared content exceeds the {}-byte input limit (baseline: {baseline_bytes} bytes; additions: {})", crate::markdown::MAX_INPUT_BYTES, added_bytes.map_or_else(|| "byte count overflow".to_owned(), |n| format!("{n} bytes"))))]
    PreparedContentLimit {
        baseline_bytes: usize,
        added_bytes: Option<usize>,
    },

    #[snafu(display("{} has different loaded texts among addition sources", path.display()))]
    ConflictingAdditionSources { path: std::path::PathBuf },
    /// An override names an `&id` the document does not have. Every such target, in id order.
    #[snafu(display("{} names nothing in the content file:\n  {}", describe(missing), lines(missing)))]
    UnknownTargets { missing: Vec<MissingTarget> },

    /// An override names a wording group of a letter's greeting or closing. Every such target, in id order.
    #[snafu(display("{}", frame_lines(frames)))]
    FrameTargets { frames: Vec<FrameTarget> },

    /// Both kinds at once, so a spec with two different mistakes is told about both.
    #[snafu(display(
        "{}\n{} names nothing in the content file:\n  {}",
        frame_lines(frames),
        describe(missing),
        lines(missing)
    ))]
    Targets {
        frames: Vec<FrameTarget>,
        missing: Vec<MissingTarget>,
    },
}

fn binding_lines(problems: &[BindingProblem]) -> String {
    problems
        .iter()
        .map(|p| format!("  {}: `{}`: {}", p.at, p.id, p.reason))
        .collect::<Vec<_>>()
        .join("\n")
}

fn describe(missing: &[MissingTarget]) -> String {
    match missing {
        [one] => format!("override of `{}`", one.id),
        many => format!("{} overrides", many.len()),
    }
}

fn places(at: &[Located]) -> String {
    at.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

fn lines(missing: &[MissingTarget]) -> String {
    missing
        .iter()
        .map(|m| format!("{}: the content has no item `&{}`", places(&m.at), m.id))
        .collect::<Vec<_>>()
        .join("\n  ")
}

fn frame_lines(frames: &[FrameTarget]) -> String {
    let each: Vec<String> = frames
        .iter()
        .map(|f| {
            format!(
                "{}: `{}` is a wording group of the letter's {}, which is not an item a spec can change",
                places(&f.at),
                f.id,
                f.section
            )
        })
        .collect();
    format!(
        "{}\n  (choose its wording with `pick`; only body paragraphs take overrides)",
        each.join("\n")
    )
}

/// A copy of `baseline` with `overrides` applied, or the targets that are not there.
///
/// Every target is checked before anything is changed, so a missing one is reported even when the item it meant
/// would have been dropped by the selection.
///
/// `baseline` must be a valid parse ([`crate::markdown::Parsed::valid`]), in which ids and group names are unique
/// in one namespace. Given a best-effort model with a duplicate id, every node of that name changes, and a node
/// with both an id and a group variant is addressed by its id. [`Session::assess_with`](crate::pipeline::Session)
/// parses and checks first; a caller that edits the model itself must keep that invariant.
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
    let frames = frame_targets(baseline, overrides);
    let mut slots = prepared.body_annotations_mut();
    let missing = missing_targets(slots.iter().map(|a| &**a), overrides, &frames);
    if !frames.is_empty() || !missing.is_empty() {
        return Err(match (frames.is_empty(), missing.is_empty()) {
            (false, true) => PrepareError::FrameTargets { frames },
            (true, false) => PrepareError::UnknownTargets { missing },
            _ => PrepareError::Targets { frames, missing },
        });
    }
    apply(&mut slots, overrides);
    Ok(prepared)
}

/// The overrides that name a greeting or closing group, in id order.
fn frame_targets(letter: &LetterSource, overrides: ItemOverrides<'_>) -> Vec<FrameTarget> {
    let mut found: Vec<FrameTarget> = letter
        .frame_groups()
        .into_iter()
        .filter_map(|(section, group)| {
            let id = ItemId::try_new(group.as_ref().to_string()).ok()?;
            overrides.overrides.contains_key(&id).then(|| FrameTarget {
                section,
                at: overrides.written_at(&id),
                id,
            })
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
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
    let missing = missing_targets(nodes, overrides, &[]);
    if missing.is_empty() {
        Ok(())
    } else {
        UnknownTargetsSnafu { missing }.fail()
    }
}

/// The overrides whose target no node has, leaving out those already known to be frame groups, in id order.
fn missing_targets<'a>(
    nodes: impl Iterator<Item = &'a Annotations>,
    overrides: ItemOverrides<'_>,
    frames: &[FrameTarget],
) -> Vec<MissingTarget> {
    let present: BTreeSet<ItemId> = nodes.filter_map(name_of).collect();
    overrides
        .overrides
        .keys()
        .filter(|id| !present.contains(*id) && !frames.iter().any(|f| &f.id == *id))
        .map(|id| MissingTarget {
            id: id.clone(),
            at: overrides.written_at(id),
        })
        .collect()
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

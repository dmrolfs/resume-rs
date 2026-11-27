//! Selects a flat authored letter with the same keep and alternate rules as resumes.
//! Frames are mandatory; scope filters body paragraphs, never greeting or closing.
use super::{Item, LetterSource, Piece, SelectedLetter, SourceItem};
use crate::diagnostics::ItemOrigin;
use crate::{
    model::{Alternate, Status},
    select::{
        Chosen, DropReason, Selection, UnknownPick,
        rules::{self, Facts, GroupFacts, Resolution, Variant, Verdict},
    },
};
use snafu::Snafu;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum LetterPart {
    Greeting,
    Body,
    Closing,
}

/// A report location in the original source, before selection or placeholder resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterRef {
    pub part: LetterPart,
    pub origin: ItemOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterDrop {
    pub item: LetterRef,
    pub reason: DropReason,
}

/// Kept/dropped locations are in authored order; a dropped group counts once at its first paragraph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LetterReport {
    pub kept: Vec<LetterRef>,
    pub dropped: Vec<LetterDrop>,
    pub chosen: Vec<Chosen>,
    pub verify: Vec<LetterRef>,
    pub skipped_drafts: Vec<Alternate>,
    pub unknown_picks: Vec<UnknownPick>,
    /// Counts authored draft wordings, including unchosen and scope-filtered ones, just as resume reports do.
    pub drafts_left_out: usize,
    pub drafts_included: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum LetterSelectError {
    #[snafu(display("explicit group selection is supported only for resumes"))]
    GroupsUnsupported,
    #[snafu(display(
        "letter picks name draft wordings: {}; finish them, pick another wording, or enable drafts to preview",
        crate::select::listed(picks)
    ))]
    PickedDraft { picks: Vec<Alternate> },
    #[snafu(display("the letter {part} has no ready wording"))]
    FrameNotReady { part: LetterPart, origin: ItemOrigin },
    #[snafu(display("selection leaves the letter body empty"))]
    EmptyBody { report: Box<LetterReport> },
}

/// Selects from a letter that parsed without errors, preserving original pieces and placeholder ranges.
///
/// Groups must satisfy the parser's invariants: nonempty, one unique name per document, unique variants,
/// a default, and shared priority. Selection never repairs malformed source or resolves fields.
/// All explicit draft picks error, even for groups excluded by scope. Unknown picks are reported.
/// Greeting/closing retain their chosen wording under every scope. Publicly constructed draft frames
/// fail with drafts off rather than disappearing or leaking into output.
///
/// # Errors
/// Explicit resume group membership, draft picks, an unavailable frame, or an empty selected body.
pub fn select(
    source: &LetterSource,
    selection: &Selection,
) -> Result<(SelectedLetter, LetterReport), LetterSelectError> {
    if selection.groups.is_some() {
        return GroupsUnsupportedSnafu.fail();
    }
    let items: Vec<_> = std::iter::once((LetterPart::Greeting, &source.greeting))
        .chain(source.body.iter().map(|item| (LetterPart::Body, item)))
        .chain(std::iter::once((LetterPart::Closing, &source.closing)))
        .collect();
    let prepared: Vec<_> = items.iter().map(|(_, item)| prepare(item, selection)).collect();
    let picks: Vec<_> = prepared.iter().filter_map(|r| r.as_ref().err().cloned()).collect();
    if !picks.is_empty() {
        return Err(LetterSelectError::PickedDraft { picks });
    }
    let groups = items
        .iter()
        .filter_map(|(_, item)| match item {
            SourceItem::Alternates(group) => {
                let names: Vec<_> = group.variants().iter().map(alternate).collect();
                Some((
                    names[0].group.clone(),
                    names.iter().map(|a| a.variant.clone()).collect(),
                ))
            }
            SourceItem::Paragraph(_) => None,
        })
        .collect();
    let mut report = LetterReport {
        unknown_picks: crate::select::unknown_picks(&groups, selection),
        ..LetterReport::default()
    };
    let mut pieces = Vec::new();
    for ((part, item), prepared) in items.into_iter().zip(prepared) {
        let resolution = prepared.expect("all draft pick errors were collected above");
        if let Some(piece) = choose(part, item, resolution, selection, &mut report)? {
            pieces.push((part, piece));
        }
    }
    let mut body = Vec::new();
    let mut greeting = None;
    let mut closing = None;
    for (part, piece) in pieces {
        match part {
            LetterPart::Greeting => greeting = Some(piece),
            LetterPart::Body => body.push(piece),
            LetterPart::Closing => closing = Some(piece),
        }
    }
    if body.is_empty() {
        return Err(LetterSelectError::EmptyBody {
            report: Box::new(report),
        });
    }
    Ok((
        SelectedLetter {
            name: source.name.clone(),
            contact: source.contact.clone(),
            greeting: greeting.expect("mandatory greeting was checked"),
            body,
            closing: closing.expect("mandatory closing was checked"),
        },
        report,
    ))
}

fn alternate(item: &Item) -> &Alternate {
    item.annotations
        .alternate
        .as_ref()
        .expect("parsed group variants carry alternate identity")
}

fn prepare<'a>(item: &'a SourceItem, selection: &Selection) -> Result<Option<Resolution<'a>>, Alternate> {
    match item {
        SourceItem::Paragraph(_) => Ok(None),
        SourceItem::Alternates(group) => {
            let variants: Vec<_> = group
                .variants()
                .iter()
                .map(|item| Variant {
                    name: &alternate(item).variant,
                    status: item.annotations.status,
                })
                .collect();
            let first = group.variants().first().expect("parsed groups are nonempty");
            rules::resolve(&alternate(first).group, &variants, selection).map(Some)
        }
    }
}

fn choose(
    part: LetterPart,
    source: &SourceItem,
    resolution: Option<Resolution<'_>>,
    selection: &Selection,
    report: &mut LetterReport,
) -> Result<Option<Piece>, LetterSelectError> {
    let empty = BTreeSet::new();
    let (items, verdict): (&[Item], _) = match source {
        SourceItem::Paragraph(item) => (
            std::slice::from_ref(item),
            rules::judge(
                &Facts {
                    priority: item.annotations.priority,
                    status: item.annotations.status,
                    own_tags: &item.annotations.tags,
                    inherited: &empty,
                },
                selection,
            ),
        ),
        SourceItem::Alternates(group) => (
            group.variants(),
            rules::judge_group(
                &GroupFacts::of(group.variants().iter().map(|i| &i.annotations)),
                &empty,
                None,
                selection,
            ),
        ),
    };
    for item in items {
        if item.annotations.status == Some(Status::Draft) {
            if selection.drafts {
                report.drafts_included += 1;
            } else {
                report.drafts_left_out += 1;
            }
        }
    }
    let first = items.first().expect("parsed groups are nonempty");
    let location = |item: &Item| LetterRef {
        part,
        origin: item.piece.origin.clone(),
    };
    let unavailable = || LetterSelectError::FrameNotReady {
        part,
        origin: first.piece.origin.clone(),
    };
    if part == LetterPart::Body {
        if let Verdict::Drop(reason) = verdict {
            report.dropped.push(LetterDrop {
                item: location(first),
                reason,
            });
            return Ok(None);
        }
    } else if resolution.is_none() && first.annotations.status == Some(Status::Draft) && !selection.drafts {
        return Err(unavailable());
    }
    let item = match resolution {
        None => first,
        Some(Resolution::NoReady) if part != LetterPart::Body => return Err(unavailable()),
        Some(Resolution::NoReady) => {
            report.dropped.push(LetterDrop {
                item: location(first),
                reason: DropReason::NoReadyVariant,
            });
            return Ok(None);
        }
        Some(Resolution::Chosen { index, by, skipped }) => {
            let item = &items[index];
            let chosen = alternate(item);
            report.chosen.push(Chosen {
                group: chosen.group.clone(),
                variant: chosen.variant.clone(),
                by,
                not_chosen: items
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .map(|(_, i)| alternate(i).variant.clone())
                    .collect(),
            });
            report
                .skipped_drafts
                .extend(skipped.into_iter().map(|variant| Alternate {
                    group: chosen.group.clone(),
                    variant: variant.clone(),
                }));
            item
        }
    };
    report.kept.push(location(item));
    if item.annotations.status == Some(Status::Verify) {
        report.verify.push(location(item));
    }
    Ok(Some(item.piece.clone()))
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;

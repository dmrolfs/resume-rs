//! Validates both documents' additions before a spec can be used or printed.
use super::additions::*;
use super::content::ContentError;
use super::layer::Sourced;
use super::raw::Text;
use super::raw_additions::*;
use super::read::Source;
use super::{Kind, SpecError};
use crate::diagnostics::{Collector, Diagnostic, Severity};
use crate::markdown::fragment::ResumeFragmentKind;
use crate::model::{ItemId, Priority, Status, Tag};
use std::collections::BTreeSet;
use std::str::FromStr;
use toml::Spanned;

pub(super) type LoadedAdditions = (Vec<ResumeAddition>, Vec<LetterAddition>, Vec<Diagnostic>);

pub(super) fn load(
    source: &Source,
    kind: Kind,
    resume: Option<Spanned<Vec<RawResumeAddition>>>,
    letter: Option<Spanned<Vec<RawLetterAddition>>>,
) -> Result<LoadedAdditions, SpecError> {
    if kind == Kind::Profile {
        let forbidden = resume
            .as_ref()
            .map(|a| (a.span().start, "add"))
            .into_iter()
            .chain(letter.as_ref().map(|a| (a.span().start, "letter.add")))
            .min();
        if let Some((offset, key)) = forbidden {
            return Err(SpecError::ForbiddenInProfile {
                at: source.at(offset),
                key,
                reason: "added content belongs only in the final position spec",
            });
        }
    }
    let mut findings = Collector::new(crate::markdown::MAX_DIAGNOSTICS);
    let mut resumed = Vec::new();
    let mut errors = Vec::new();
    let mut ids = BTreeSet::new();
    for raw in resume.into_iter().flat_map(Spanned::into_inner) {
        let result = (|| {
            let id = name(source, &raw.id, "add.id")?;
            unique(&id, &mut ids, "add.id")?;
            let kind = keyword::<ResumeFragmentKind>(source, &raw.kind, "add.kind", &["bullet", "paragraph", "blurb"])?;
            let place = resume_place(source, &raw.place, kind.value)?;
            let metadata = metadata(source, raw.tags, raw.priority, raw.status, false)?;
            ResumeAddition::new(id, place, metadata, source.snapshot(), raw.content.span()).map_err(|source| {
                SpecError::AdditionContent {
                    source: Box::new(source),
                }
            })
        })();
        match result {
            Ok(addition) => {
                for d in addition.content().warnings() {
                    findings.push(d.clone());
                }
                resumed.push(addition);
            }
            Err(error) => collect(error, &mut findings, &mut errors),
        }
    }
    let mut letters = Vec::new();
    ids.clear();
    for raw in letter.into_iter().flat_map(Spanned::into_inner) {
        let result = (|| {
            let id = name(source, &raw.id, "letter.add.id")?;
            unique(&id, &mut ids, "letter.add.id")?;
            let place = letter_place(source, &raw.place)?;
            let metadata = metadata(source, raw.tags, raw.priority, raw.status, true)?;
            LetterAddition::new(id, place, metadata, source.snapshot(), raw.content.span()).map_err(|source| {
                SpecError::AdditionContent {
                    source: Box::new(source),
                }
            })
        })();
        match result {
            Ok(addition) => {
                for d in addition.content().warnings() {
                    findings.push(d.clone());
                }
                letters.push(addition);
            }
            Err(error) => collect(error, &mut findings, &mut errors),
        }
    }
    let findings = findings.finish(None);
    if !errors.is_empty() {
        if findings.is_empty() && errors.len() == 1 {
            return Err(errors.pop().expect("one error"));
        }
        return Err(SpecError::AdditionValidation {
            snapshot: source.snapshot().clone(),
            diagnostics: findings,
            errors,
        });
    }
    if findings.iter().any(|d| d.severity == Severity::Error) {
        return Err(SpecError::AdditionContent {
            source: Box::new(ContentError::Findings {
                snapshot: source.snapshot().clone(),
                diagnostics: findings,
            }),
        });
    }
    Ok((resumed, letters, findings))
}
fn unique(id: &Sourced<ItemId>, seen: &mut BTreeSet<ItemId>, key: &str) -> Result<(), SpecError> {
    if seen.insert(id.value.clone()) {
        Ok(())
    } else {
        Err(SpecError::DuplicateEntry {
            at: id.at.clone(),
            key: key.into(),
            value: id.value.to_string(),
        })
    }
}
fn collect(error: SpecError, findings: &mut Collector, errors: &mut Vec<SpecError>) {
    match error {
        SpecError::AdditionContent { source } => match *source {
            ContentError::Findings { diagnostics, .. } => {
                for d in diagnostics {
                    findings.push(d);
                }
            }
            other => errors.push(SpecError::AdditionContent {
                source: Box::new(other),
            }),
        },
        other => errors.push(other),
    }
}
fn name(source: &Source, text: &Text, key: &str) -> Result<Sourced<ItemId>, SpecError> {
    let at = source.at(text.span().start);
    let value = ItemId::try_new(text.get_ref().clone()).map_err(|_| SpecError::InvalidName {
        at: at.clone(),
        key: key.into(),
        kind: super::error::NameKind::Item,
        value: super::error::shown(text.get_ref()),
    })?;
    Ok(Sourced { value, at })
}
fn keyword<T: FromStr>(
    source: &Source,
    text: &Text,
    key: &'static str,
    allowed: &[&str],
) -> Result<Sourced<T>, SpecError> {
    let at = source.at(text.span().start);
    let value = text.get_ref().parse().map_err(|_| SpecError::InvalidKeyword {
        at: at.clone(),
        key,
        value: super::error::shown(text.get_ref()),
        allowed: allowed.iter().map(|s| (*s).into()).collect(),
    })?;
    Ok(Sourced { value, at })
}
fn metadata(
    source: &Source,
    tags: Option<Spanned<Vec<Text>>>,
    priority: Option<Text>,
    status: Option<Text>,
    letter: bool,
) -> Result<AdditionMetadata, SpecError> {
    let tags_key = if letter { "letter.add.tags" } else { "add.tags" };
    let priority_key = if letter { "letter.add.priority" } else { "add.priority" };
    let status_key = if letter { "letter.add.status" } else { "add.status" };
    let tags = tags
        .map(|tags| {
            let at = source.at(tags.span().start);
            let mut value = BTreeSet::new();
            for text in tags.get_ref() {
                let tag = Tag::try_new(text.get_ref().clone()).map_err(|_| SpecError::InvalidName {
                    at: source.at(text.span().start),
                    key: tags_key.into(),
                    kind: super::error::NameKind::Tag,
                    value: super::error::shown(text.get_ref()),
                })?;
                if !value.insert(tag) {
                    return Err(SpecError::DuplicateEntry {
                        at: source.at(text.span().start),
                        key: tags_key.into(),
                        value: text.get_ref().clone(),
                    });
                }
            }
            Ok(Sourced { value, at })
        })
        .transpose()?;
    Ok(AdditionMetadata {
        tags,
        priority: priority
            .as_ref()
            .map(|t| keyword::<Priority>(source, t, priority_key, &["core", "extra"]))
            .transpose()?,
        status: status
            .as_ref()
            .map(|t| keyword::<Status>(source, t, status_key, &["draft", "verify"]))
            .transpose()?,
    })
}
fn invalid_place(source: &Source, raw: &Spanned<RawPlace>) -> SpecError {
    SpecError::AdditionPlace {
        at: source.at(raw.span().start),
    }
}
fn resume_place(source: &Source, raw: &Spanned<RawPlace>, kind: ResumeFragmentKind) -> Result<ResumePlace, SpecError> {
    let p = raw.get_ref();
    let leaf = match (&p.before, &p.after, &p.parent, &p.at) {
        (Some(a), None, None, None) => LeafPlace::Before(name(source, a, "add.place.before")?),
        (None, Some(a), None, None) => LeafPlace::After(name(source, a, "add.place.after")?),
        (None, None, Some(parent), Some(at)) => {
            let parent = name(source, parent, "add.place.parent")?;
            match at.get_ref().as_str() {
                "blurb" if kind == ResumeFragmentKind::Blurb => return Ok(ResumePlace::Blurb { role: parent }),
                "start" => LeafPlace::Start(parent),
                "end" => LeafPlace::End(parent),
                _ => return Err(invalid_place(source, raw)),
            }
        }
        _ => return Err(invalid_place(source, raw)),
    };
    match kind {
        ResumeFragmentKind::Bullet => Ok(ResumePlace::Bullet(leaf)),
        ResumeFragmentKind::Paragraph => Ok(ResumePlace::Paragraph(leaf)),
        ResumeFragmentKind::Blurb => Err(invalid_place(source, raw)),
    }
}
fn letter_place(source: &Source, raw: &Spanned<RawPlace>) -> Result<LetterPlace, SpecError> {
    let p = raw.get_ref();
    match (&p.before, &p.after, &p.parent, &p.at) {
        (Some(a), None, None, None) => Ok(LetterPlace::Before(name(source, a, "letter.add.place.before")?)),
        (None, Some(a), None, None) => Ok(LetterPlace::After(name(source, a, "letter.add.place.after")?)),
        (None, None, None, Some(at)) => match at.get_ref().as_str() {
            "start" => Ok(LetterPlace::Start(source.at(at.span().start))),
            "end" => Ok(LetterPlace::End(source.at(at.span().start))),
            _ => Err(invalid_place(source, raw)),
        },
        _ => Err(invalid_place(source, raw)),
    }
}

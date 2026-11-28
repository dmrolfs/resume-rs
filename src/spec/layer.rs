//! ABOUTME: One validated file of a chain: a profile or a position spec, with the place of every value it holds.
//! ABOUTME: Raw TOML becomes typed names, paths, and position data here, and the rules for each kind of file apply.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use clap::ValueEnum;
use std::str::FromStr as _;

use snafu::ResultExt;
use toml::Spanned;

use super::error::{
    DateProblem, DuplicateEntrySnafu, EmptyOverrideSnafu, ForbiddenInProfileSnafu, InvalidDateSnafu, InvalidLevelSnafu,
    InvalidNameSnafu, InvalidPaperSnafu, InvalidPathSnafu, InvalidStyleSnafu, InvalidUrlSnafu, Located,
    MissingKeySnafu, NameKind, NotASpecSnafu, RefusedUrlSnafu, SpecError, TextLengthSnafu, UnsafeTextSnafu, UrlProblem,
    WrongTypeSnafu, shown,
};
use super::letter::{self, LetterSettings};
use super::names::{DeclaredPath, Level, PositionLabel, ProfileName};
use super::raw::{RawBuild, RawEmphasis, RawFile, RawOverride, RawPosition, RawSelect, Text};
use super::read::{Source, parse, read_source, read_source_shown, text_source};
use crate::diagnostics::has_unsafe_characters;
use crate::model::options::Paper;
use crate::model::{Alternate, Company, GroupName, ItemId, PriorityEdit, StyleName, Tag, Title, VariantName};

/// A value and where the file wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sourced<T> {
    pub value: T,
    pub at: Located,
}

/// Which rules a file is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A reusable set of choices: no `[position]`, no `content`.
    Profile,
    /// One application: it names the content file and may describe the position.
    Spec,
}

/// How strongly a position stresses each axis. An axis a file does not mention is `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Emphasis {
    pub business: Option<Level>,
    pub org: Option<Level>,
    pub technical: Option<Level>,
}

/// What a spec says about the position it applies for. Nothing reads these fields yet except the summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Position {
    pub company: Option<Company>,
    pub title: Option<Title>,
    pub level: Option<PositionLabel>,
    pub domain: Option<PositionLabel>,
    pub audience: Option<PositionLabel>,
    pub emphasis: Emphasis,
    pub date: Option<time::Date>,
    pub posting: Option<url::Url>,
}

/// What one file says about one item's own tags and priority: each field is there or it is not.
///
/// A field that is absent leaves the less specific file's value in force; a field that is present replaces it, an
/// empty tag set and `none` included. Each carries the place it was written, so a finding names a line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemOverrideLayer {
    /// Replaces the item's own tags as a whole; empty clears them.
    pub tags: Option<Sourced<BTreeSet<Tag>>>,
    pub priority: Option<Sourced<PriorityEdit>>,
}

/// The `[select]` table of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectLayer {
    pub groups: Option<Sourced<Vec<Sourced<GroupName>>>>,
    pub tags: Vec<Sourced<Tag>>,
    pub prefer: Vec<Sourced<VariantName>>,
    pub promote: Vec<Sourced<Tag>>,
    pub pick: BTreeMap<GroupName, Sourced<VariantName>>,
    pub core_only: Option<Sourced<bool>>,
    /// `[select.override.<id>]`: changes to one item's own tags or priority, by the item's `&id`.
    pub overrides: BTreeMap<ItemId, ItemOverrideLayer>,
}

/// One file of the chain, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub kind: Kind,
    /// The profile name this file was found under; `None` for a spec.
    pub name: Option<ProfileName>,
    /// Immutable text accepted by the loader, retained for later diagnostics.
    pub(super) source: crate::diagnostics::SourceText,
    pub extends: Vec<Sourced<ProfileName>>,
    pub position: Option<Position>,
    pub content: Option<Sourced<DeclaredPath>>,
    pub style: Option<Sourced<StyleName>>,
    pub paper: Option<Sourced<Paper>>,
    pub select: SelectLayer,
    /// The `[letter]` settings of a spec that has the table. A profile never has them.
    pub(super) letter: Option<LetterTable>,
    /// The `[letter.select]` table of any file; it merges apart from `select`.
    pub letter_select: SelectLayer,
    pub(crate) additions: Vec<super::additions::ResumeAddition>,
    pub(super) content_warnings: Vec<crate::diagnostics::Diagnostic>,
}

/// The `[letter]` table of a spec: the letter content it builds from, and the letter's own settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterTable {
    pub content: Sourced<DeclaredPath>,
    pub settings: LetterSettings,
    pub(crate) additions: Vec<super::additions::LetterAddition>,
}

impl LetterTable {
    pub fn additions(&self) -> &[super::additions::LetterAddition] {
        &self.additions
    }
}

/// A position spec: a [`Layer`] that is known to be one, so a chain can end in it without a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecLayer(Layer);

impl SpecLayer {
    /// Reads, parses, and validates the position spec at `path`.
    ///
    /// # Errors
    /// A [`SpecError`] naming the file and, where it can, the line, column, and key.
    pub fn load(path: &Path) -> Result<Self, SpecError> {
        Layer::load(Kind::Spec, path).map(Self)
    }

    /// Parses and validates `text` as the position spec at `path`, without reading the file.
    ///
    /// For text that is being edited and is not saved: `path` names the file in every error and is what a relative
    /// `content` or profile is resolved against. It gives the same layer, and the same located errors, as
    /// [`Self::load`] gives for a file holding `text`.
    ///
    /// # Errors
    /// A [`SpecError`] naming `path` and, where it can, the line, column, and key; [`SpecError::TooLarge`] past the
    /// size a file may be.
    pub fn from_text(path: &Path, text: &str) -> Result<Self, SpecError> {
        Layer::from_source(Kind::Spec, &text_source(path, text)?).map(Self)
    }

    /// `layer`, if it is a spec.
    ///
    /// # Errors
    /// [`SpecError::NotASpec`] for a profile.
    pub fn try_from_layer(layer: Layer) -> Result<Self, SpecError> {
        if layer.kind == Kind::Spec {
            Ok(Self(layer))
        } else {
            NotASpecSnafu {
                path: layer.path().to_owned(),
            }
            .fail()
        }
    }

    /// The spec as a layer.
    #[must_use]
    pub fn into_layer(self) -> Layer {
        self.0
    }

    /// The spec as a layer, borrowed.
    #[must_use]
    pub fn layer(&self) -> &Layer {
        &self.0
    }
}

impl Layer {
    /// Validates a spec or profile snapshot without rereading the file.
    ///
    /// # Errors
    /// The same size, syntax and validation errors as [`Self::load`].
    pub fn from_text(kind: Kind, path: &Path, text: &str) -> Result<Self, SpecError> {
        Self::from_source(kind, &text_source(path, text)?)
    }
    pub fn letter(&self) -> Option<&LetterTable> {
        self.letter.as_ref()
    }
    pub fn additions(&self) -> &[super::additions::ResumeAddition] {
        &self.additions
    }
    /// The source path as loaded, never canonicalized or independently mutable.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.source.path()
    }

    /// The exact text accepted by the loader, including unsaved editor changes.
    #[must_use]
    pub fn source(&self) -> &crate::diagnostics::SourceText {
        &self.source
    }

    /// Reads, parses, and validates the file at `path`.
    ///
    /// # Errors
    /// A [`SpecError`] naming the file and, where it can, the line, column, and key.
    pub fn load(kind: Kind, path: &Path) -> Result<Self, SpecError> {
        Self::from_source(kind, &read_source(path)?)
    }

    /// Reads the file at `open`, which a check has already vetted, and shows it as `path` in every error.
    pub(super) fn load_shown(kind: Kind, open: &Path, path: &Path) -> Result<Self, SpecError> {
        Self::from_source(kind, &read_source_shown(open, path)?)
    }

    pub(super) fn from_source(kind: Kind, source: &Source) -> Result<Self, SpecError> {
        let raw = parse(source)?;
        Self::from_raw(kind, source, raw)
    }

    fn from_raw(kind: Kind, source: &Source, mut raw: RawFile) -> Result<Self, SpecError> {
        let (additions, letter_additions, content_warnings) = super::load_additions::load(
            source,
            kind,
            raw.add.take(),
            raw.letter.as_mut().and_then(|l| l.add.take()),
        )?;
        let result = (|| {
            let extends = list(source, "extends", NameKind::Profile, raw.extends.as_deref(), |s| {
                ProfileName::try_new(s).ok()
            })?;
            let position = match (kind, &raw.position) {
                (Kind::Profile, Some(table)) => {
                    // The header is the natural place; a table written as `position.company = ..` has none, so its
                    // first value is used, and an empty `[position]` always has a header.
                    let offset = table_header(source.text(), "[position")
                        .or_else(|| table.first_offset())
                        .unwrap_or(0);
                    return ForbiddenInProfileSnafu {
                        at: source.at(offset),
                        key: "position",
                        reason: "it describes one application, so it belongs in a position spec",
                    }
                    .fail();
                }
                (Kind::Spec, Some(table)) => Some(position(source, table)?),
                (_, None) => None,
            };
            let build = raw.build.unwrap_or(RawBuild {
                content: None,
                style: None,
                paper: None,
            });
            let content = content(kind, source, build.content.as_ref())?;
            let style = build.style.as_ref().map(|t| style(source, t)).transpose()?;
            let paper = build.paper.as_ref().map(|t| paper(source, t)).transpose()?;
            let select = select(source, "select", raw.select)?;
            let (mut letter, letter_select) = letter::from_raw(kind, source, raw.letter)?;
            if let Some(letter) = &mut letter {
                letter.additions = letter_additions;
            }
            Ok(Self {
                kind,
                name: None,
                source: source.snapshot().clone(),
                extends,
                position,
                content,
                style,
                paper,
                select,
                letter,
                letter_select,
                additions,
                content_warnings: Vec::new(),
            })
        })();
        match result {
            Ok(mut layer) => {
                layer.content_warnings = content_warnings;
                Ok(layer)
            }
            Err(error) if content_warnings.is_empty() => Err(error),
            Err(error) => Err(SpecError::AdditionValidation {
                snapshot: source.snapshot().clone(),
                diagnostics: content_warnings,
                errors: vec![error],
            }),
        }
    }

    /// Every pick this file makes, as the alternate it names, with where it was written.
    #[must_use]
    pub fn picks(&self) -> Vec<(Alternate, &Located)> {
        self.select
            .pick
            .iter()
            .map(|(group, variant)| {
                (
                    Alternate {
                        group: group.clone(),
                        variant: variant.value.clone(),
                    },
                    &variant.at,
                )
            })
            .collect()
    }
}

/// Where a line that opens the table `[name` (or a sub-table of it) starts, if the file has one.
pub(super) fn table_header(text: &str, name: &str) -> Option<usize> {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with(name) {
            return Some(at + (line.len() - trimmed.len()));
        }
        at += line.len();
    }
    None
}

fn content(kind: Kind, source: &Source, text: Option<&Text>) -> Result<Option<Sourced<DeclaredPath>>, SpecError> {
    match (kind, text) {
        (Kind::Profile, Some(text)) => ForbiddenInProfileSnafu {
            at: source.at(text.span().start),
            key: "build.content",
            reason: "only a position spec names the content file",
        }
        .fail(),
        (Kind::Spec, None) => MissingKeySnafu {
            at: source.at(0),
            key: "build.content",
        }
        .fail(),
        (_, None) => Ok(None),
        (Kind::Spec, Some(text)) => sourced(source, text, |at, raw| {
            DeclaredPath::parse(raw).map_err(|rule| {
                InvalidPathSnafu {
                    at,
                    key: "build.content",
                    value: shown(raw),
                    rule,
                }
                .build()
            })
        })
        .map(Some),
    }
}

/// Turns `text` into a value with `make`, which is told where the text is, and remembers the place.
pub(super) fn sourced<T>(
    source: &Source,
    text: &Text,
    make: impl FnOnce(Located, &str) -> Result<T, SpecError>,
) -> Result<Sourced<T>, SpecError> {
    let at = source.at(text.span().start);
    let value = make(at.clone(), text.get_ref())?;
    Ok(Sourced { value, at })
}

pub(super) fn style(source: &Source, text: &Text) -> Result<Sourced<StyleName>, SpecError> {
    sourced(source, text, |at, raw| {
        StyleName::try_new(raw).map_err(|_| InvalidStyleSnafu { at, value: shown(raw) }.build())
    })
}

pub(super) fn paper(source: &Source, text: &Text) -> Result<Sourced<Paper>, SpecError> {
    sourced(source, text, |at, raw| {
        Paper::from_str(raw, false).map_err(|_| {
            let allowed: Vec<String> = Paper::value_variants()
                .iter()
                .filter_map(ValueEnum::to_possible_value)
                .map(|v| v.get_name().to_string())
                .collect();
            InvalidPaperSnafu {
                at,
                value: shown(raw),
                allowed,
            }
            .build()
        })
    })
}

/// A list of names, each valid, none repeated.
fn list<T>(
    source: &Source,
    key: &str,
    kind: NameKind,
    items: Option<&[Text]>,
    make: impl Fn(String) -> Option<T>,
) -> Result<Vec<Sourced<T>>, SpecError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in items.unwrap_or_default() {
        let at = source.at(item.span().start);
        let text = item.get_ref();
        let Some(value) = make(text.clone()) else {
            return InvalidNameSnafu {
                at,
                key,
                kind,
                value: shown(text),
            }
            .fail();
        };
        if !seen.insert(text.clone()) {
            return DuplicateEntrySnafu {
                at,
                key,
                value: shown(text),
            }
            .fail();
        }
        out.push(Sourced { value, at });
    }
    Ok(out)
}

pub(super) fn select(source: &Source, table: &str, raw: Option<RawSelect>) -> Result<SelectLayer, SpecError> {
    let Some(raw) = raw else {
        return Ok(SelectLayer::default());
    };
    let groups = raw
        .groups
        .as_ref()
        .map(|items| {
            list(
                source,
                &format!("{table}.groups"),
                NameKind::Group,
                Some(items.get_ref()),
                |s| GroupName::try_new(s).ok(),
            )
            .map(|value| Sourced {
                value,
                at: source.at(items.span().start),
            })
        })
        .transpose()?;
    let tags = list(
        source,
        &format!("{table}.tags"),
        NameKind::Tag,
        raw.tags.as_deref(),
        |s| Tag::try_new(s).ok(),
    )?;
    let prefer = list(
        source,
        &format!("{table}.prefer"),
        NameKind::Variant,
        raw.prefer.as_deref(),
        |s| VariantName::try_new(s).ok(),
    )?;
    let promote = list(
        source,
        &format!("{table}.promote"),
        NameKind::Tag,
        raw.promote.as_deref(),
        |s| Tag::try_new(s).ok(),
    )?;
    let mut pick = BTreeMap::new();
    for (group, variant) in raw.pick.iter().flatten() {
        // The group is a plain map key with no position of its own, so both names are located at the wording.
        let at = source.at(variant.span().start);
        let Ok(group_name) = GroupName::try_new(group.clone()) else {
            return InvalidNameSnafu {
                at,
                key: format!("{table}.pick"),
                kind: NameKind::Group,
                value: shown(group),
            }
            .fail();
        };
        let Ok(variant_name) = VariantName::try_new(variant.get_ref().clone()) else {
            return InvalidNameSnafu {
                at,
                key: format!("{table}.pick"),
                kind: NameKind::Variant,
                value: shown(variant.get_ref()),
            }
            .fail();
        };
        pick.insert(
            group_name,
            Sourced {
                value: variant_name,
                at,
            },
        );
    }
    let core_only = raw.core_only.map(|flag| Sourced {
        value: *flag.get_ref(),
        at: source.at(flag.span().start),
    });
    Ok(SelectLayer {
        groups,
        tags,
        prefer,
        promote,
        pick,
        core_only,
        overrides: overrides(source, table, raw.overrides.as_ref())?,
    })
}

/// `[<table>.override.<id>]`: each item's own tags and priority, every field with the line that wrote it.
///
/// An override that writes neither field is an error at its table, because it promises an edit and changes nothing.
fn overrides(
    source: &Source,
    table: &str,
    raw: Option<&BTreeMap<String, Spanned<RawOverride>>>,
) -> Result<BTreeMap<ItemId, ItemOverrideLayer>, SpecError> {
    let key = format!("{table}.override");
    let mut out = BTreeMap::new();
    for (name, body) in raw.into_iter().flatten() {
        let at = source.at(body.span().start);
        let Ok(id) = ItemId::try_new(name.clone()) else {
            return InvalidNameSnafu {
                at,
                key: key.clone(),
                kind: NameKind::Item,
                value: shown(name),
            }
            .fail();
        };
        let body = body.get_ref();
        if body.tags.is_none() && body.priority.is_none() {
            return EmptyOverrideSnafu {
                at,
                table: table.to_string(),
                id: name.clone(),
            }
            .fail();
        }
        let tags = body
            .tags
            .as_ref()
            .map(|written| {
                let items = written.get_ref().as_slice();
                let tags = list(source, &format!("{key}.{name}.tags"), NameKind::Tag, Some(items), |s| {
                    Tag::try_new(s).ok()
                })?;
                // The parser's own span of the value, so an inline table, a quoted key or a table with comments is
                // located exactly; an empty list has no item to point at.
                Ok::<_, SpecError>(Sourced {
                    value: tags.into_iter().map(|t| t.value).collect(),
                    at: source.at(written.span().start),
                })
            })
            .transpose()?;
        let priority = body
            .priority
            .as_ref()
            .map(|text| priority_edit(source, &format!("{key}.{name}.priority"), text))
            .transpose()?;
        out.insert(id, ItemOverrideLayer { tags, priority });
    }
    Ok(out)
}

fn priority_edit(source: &Source, key: &str, text: &Text) -> Result<Sourced<PriorityEdit>, SpecError> {
    let at = source.at(text.span().start);
    match PriorityEdit::from_str(text.get_ref()) {
        Ok(value) => Ok(Sourced { value, at }),
        Err(_) => WrongTypeSnafu {
            at,
            key,
            message: format!(
                "`{}` is not a priority; write `core`, `extra`, or `none`",
                shown(text.get_ref())
            ),
        }
        .fail(),
    }
}

/// Free text of a position: no control or direction characters, then the length rule of its type.
fn free_text<T>(
    source: &Source,
    text: Option<&Text>,
    key: &str,
    max: usize,
    make: impl FnOnce(&str) -> Option<T>,
) -> Result<Option<T>, SpecError> {
    let Some(text) = text else {
        return Ok(None);
    };
    let at = source.at(text.span().start);
    if has_unsafe_characters(text.get_ref()) {
        return UnsafeTextSnafu { at, key }.fail();
    }
    match make(text.get_ref()) {
        Some(value) => Ok(Some(value)),
        None => TextLengthSnafu { at, key, max }.fail(),
    }
}

fn position(source: &Source, raw: &RawPosition) -> Result<Position, SpecError> {
    let label =
        |key: &str, text: &Option<Text>| free_text(source, text.as_ref(), key, 80, |s| PositionLabel::try_new(s).ok());
    Ok(Position {
        company: free_text(source, raw.company.as_ref(), "position.company", 120, |s| {
            Company::try_new(s).ok()
        })?,
        title: free_text(source, raw.title.as_ref(), "position.title", 120, |s| {
            Title::try_new(s).ok()
        })?,
        level: label("position.level", &raw.level)?,
        domain: label("position.domain", &raw.domain)?,
        audience: label("position.audience", &raw.audience)?,
        emphasis: emphasis(source, raw.emphasis.as_ref())?,
        date: date(source, raw.date.as_ref(), "position.date")?.map(|d| d.value),
        posting: posting(source, raw.posting.as_ref())?,
    })
}

fn emphasis(source: &Source, raw: Option<&RawEmphasis>) -> Result<Emphasis, SpecError> {
    let level = |axis: &str, text: &Option<Text>| -> Result<Option<Level>, SpecError> {
        text.as_ref()
            .map(|t| {
                sourced(source, t, |at, raw| {
                    raw.parse::<Level>().map_err(|_| {
                        InvalidLevelSnafu {
                            at,
                            key: format!("position.emphasis.{axis}"),
                            value: shown(raw),
                        }
                        .build()
                    })
                })
                .map(|s| s.value)
            })
            .transpose()
    };
    let Some(raw) = raw else {
        return Ok(Emphasis::default());
    };
    Ok(Emphasis {
        business: level("business", &raw.business)?,
        org: level("org", &raw.org)?,
        technical: level("technical", &raw.technical)?,
    })
}

pub(super) fn date(
    source: &Source,
    raw: Option<&toml::Spanned<toml::value::Datetime>>,
    key: &'static str,
) -> Result<Option<Sourced<time::Date>>, SpecError> {
    let Some(spanned) = raw else {
        return Ok(None);
    };
    let at = source.at(spanned.span().start);
    let datetime = spanned.get_ref();
    let (Some(day), None, None) = (&datetime.date, &datetime.time, &datetime.offset) else {
        return InvalidDateSnafu {
            at,
            key,
            problem: DateProblem::NotJustADate,
        }
        .fail();
    };
    // The TOML reader refuses a month or day out of range itself (as a syntax error), so these two are a guard
    // for the conversion, not a path a file can take.
    let Ok(month) = time::Month::try_from(day.month) else {
        return InvalidDateSnafu {
            at,
            key,
            problem: DateProblem::Month,
        }
        .fail();
    };
    match time::Date::from_calendar_date(i32::from(day.year), month, day.day) {
        Ok(value) => Ok(Some(Sourced { value, at })),
        Err(_) => InvalidDateSnafu {
            at,
            key,
            problem: DateProblem::Day,
        }
        .fail(),
    }
}

fn posting(source: &Source, text: Option<&Text>) -> Result<Option<url::Url>, SpecError> {
    let Some(text) = text else {
        return Ok(None);
    };
    let at = source.at(text.span().start);
    let url = url::Url::parse(text.get_ref()).context(InvalidUrlSnafu { at: at.clone() })?;
    if !matches!(url.scheme(), "http" | "https") {
        return RefusedUrlSnafu {
            at,
            problem: UrlProblem::NotWeb,
        }
        .fail();
    }
    if !url.username().is_empty() || url.password().is_some() {
        return RefusedUrlSnafu {
            at,
            problem: UrlProblem::Credentials,
        }
        .fail();
    }
    Ok(Some(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok, assert_some};
    use pretty_assertions::assert_eq;

    fn load(kind: Kind, text: &str) -> Result<Layer, SpecError> {
        Layer::from_source(kind, &Source::new(PathBuf::from("f.toml"), text.to_string()))
    }

    const SPEC: &str = "[build]\ncontent = \"../baselines/baseline.md\"\n";

    fn spec_with(position: &str) -> String {
        format!("{position}\n[build]\ncontent = \"a.md\"\n")
    }

    #[test]
    fn a_minimal_spec_has_only_a_content_path() {
        let layer = assert_ok!(load(Kind::Spec, SPEC));
        assert_eq!(
            assert_some!(layer.content.as_ref()).value.as_str(),
            "../baselines/baseline.md"
        );
        assert!(layer.extends.is_empty() && layer.position.is_none() && layer.style.is_none());
        assert_eq!(layer.select, SelectLayer::default());
    }

    #[test]
    fn a_spec_without_content_is_missing_it() {
        assert!(matches!(
            assert_err!(load(Kind::Spec, "[select]\ntags = [\"ai\"]\n")),
            SpecError::MissingKey {
                key: "build.content",
                ..
            }
        ));
    }

    #[test]
    fn a_profile_may_not_have_a_position_or_a_content_path() {
        match assert_err!(load(Kind::Profile, "\n[position]\ncompany = \"Acme\"\n")) {
            SpecError::ForbiddenInProfile { at, key, .. } => assert_eq!((at.line, key), (2, "position")),
            other => panic!("{other:?}"),
        }
        match assert_err!(load(Kind::Profile, SPEC)) {
            SpecError::ForbiddenInProfile { at, key, .. } => assert_eq!((at.line, key), (2, "build.content")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_position_in_a_profile_is_refused_however_toml_spells_it() {
        for text in [
            "[position]\n",
            "[position.emphasis]\nbusiness = \"low\"\n",
            "position.company = \"A\"\n",
            "[position]\nemphasis = { business = \"low\" }\n",
        ] {
            assert!(
                matches!(
                    assert_err!(load(Kind::Profile, text)),
                    SpecError::ForbiddenInProfile { key: "position", .. }
                ),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_spec_may_spell_its_position_table_in_any_valid_toml_way() {
        for position in [
            "[position.emphasis]\nbusiness = \"low\"\n",
            "position.company = \"A\"\n",
            "[position]\nemphasis.business = \"low\"\n",
        ] {
            assert_ok!(load(Kind::Spec, &spec_with(position)), "{position:?}");
        }
    }

    #[test]
    fn a_profile_with_choices_loads_without_content() {
        let layer = assert_ok!(load(
            Kind::Profile,
            "extends = [\"base\"]\n[build]\nstyle = \"Mono\"\npaper = \"a4\"\n[select]\ntags = [\"ai\"]\n"
        ));
        assert_eq!(assert_some!(layer.style.as_ref()).value.as_ref(), "mono");
        assert_eq!(assert_some!(layer.paper.as_ref()).value, Paper::A4);
        assert_eq!(layer.extends.len(), 1);
        assert!(layer.content.is_none());
    }

    #[test]
    fn a_spec_describes_its_position_with_typed_values() {
        let layer = assert_ok!(load(
            Kind::Spec,
            "[position]\ncompany = \" Acme  Corp \"\ntitle = \"Principal\"\nlevel = \"principal\"\naudience = \"technical\"\nemphasis = { business = \"low\", technical = \"high\" }\ndate = 2026-11-19\nposting = \"https://example.com/jobs/1\"\n[build]\ncontent = \"a.md\"\n"
        ));
        let position = assert_some!(layer.position);
        assert_eq!(assert_some!(position.company).as_ref(), "Acme Corp");
        assert_eq!(position.emphasis.business, Some(Level::Low));
        assert_eq!(position.emphasis.org, None);
        assert_eq!(position.emphasis.technical, Some(Level::High));
        assert_eq!(
            assert_some!(position.date),
            assert_ok!(time::Date::from_calendar_date(2026, time::Month::November, 19))
        );
        assert_eq!(assert_some!(position.posting).host_str(), Some("example.com"));
    }

    #[test]
    fn each_position_label_is_read_from_its_own_key() {
        let layer = assert_ok!(load(
            Kind::Spec,
            &spec_with("[position]\nlevel = \"l\"\ndomain = \"d\"\naudience = \"a\"")
        ));
        let position = assert_some!(layer.position);
        assert_eq!(assert_some!(position.level).as_ref(), "l");
        assert_eq!(assert_some!(position.domain).as_ref(), "d");
        assert_eq!(assert_some!(position.audience).as_ref(), "a");
    }

    #[test]
    fn a_position_value_that_is_not_usable_is_a_typed_error_naming_its_key() {
        let err = |position: &str| assert_err!(load(Kind::Spec, &spec_with(position)));
        match err("[position]\nemphasis = { technical = \"extreme\" }") {
            SpecError::InvalidLevel { key, value, .. } => assert_eq!(
                (key.as_str(), value.as_str()),
                ("position.emphasis.technical", "extreme")
            ),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            err("[position]\nposting = \"ftp://example.com/x\""),
            SpecError::RefusedUrl {
                problem: UrlProblem::NotWeb,
                ..
            }
        ));
        assert!(matches!(
            err("[position]\nposting = \"https://u:secret@example.com/j?token=1\""),
            SpecError::RefusedUrl {
                problem: UrlProblem::Credentials,
                ..
            }
        ));
        assert!(matches!(
            err("[position]\nposting = \"not a url\""),
            SpecError::InvalidUrl { .. }
        ));
        match err("[position]\ncompany = \"  \"") {
            SpecError::TextLength { key, max, .. } => assert_eq!((key.as_str(), max), ("position.company", 120)),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            err(&format!("[position]\ntitle = \"{}\"", "t".repeat(121))),
            SpecError::TextLength { max: 120, .. }
        ));
        assert!(matches!(
            err("[position]\nlevel = \"\""),
            SpecError::TextLength { max: 80, .. }
        ));
        assert!(matches!(
            err("[position]\ndate = 2026-11-19T10:00:00"),
            SpecError::InvalidDate {
                problem: DateProblem::NotJustADate,
                ..
            }
        ));
        assert!(matches!(
            err("[position]\ndate = 10:00:00"),
            SpecError::InvalidDate {
                problem: DateProblem::NotJustADate,
                ..
            }
        ));
    }

    #[test]
    fn a_plain_http_posting_is_allowed() {
        for url in ["http://example.com/a", "https://example.com/a"] {
            assert_ok!(load(
                Kind::Spec,
                &spec_with(&format!("[position]\nposting = \"{url}\""))
            ));
        }
    }

    #[test]
    fn free_text_with_a_control_or_direction_character_is_refused_in_every_field() {
        for key in ["company", "title", "level", "domain", "audience"] {
            for bad in ["a\\u001b[2Jb", "a \\u202e x", "a\\u200fb"] {
                let text = spec_with(&format!("[position]\n{key} = \"{bad}\""));
                match assert_err!(load(Kind::Spec, &text)) {
                    SpecError::UnsafeText { key: got, .. } => assert_eq!(got, format!("position.{key}"), "{bad}"),
                    other => panic!("{key} {bad}: {other:?}"),
                }
            }
        }
    }

    #[test]
    fn an_impossible_date_is_a_located_syntax_error_from_the_toml_reader() {
        match assert_err!(load(Kind::Spec, &spec_with("[position]\ndate = 2026-02-30"))) {
            SpecError::Syntax { at, .. } => assert_eq!(at.line, 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_paper_is_spelled_the_way_the_command_line_spells_it_and_in_lowercase() {
        for (text, want) in [("letter", Paper::Letter), ("a4", Paper::A4)] {
            let layer = assert_ok!(load(Kind::Profile, &format!("[build]\npaper = \"{text}\"\n")));
            assert_eq!(assert_some!(layer.paper).value, want);
        }
        for bad in ["us-letter", "A4", "Letter", "LETTER"] {
            match assert_err!(load(Kind::Profile, &format!("[build]\npaper = \"{bad}\"\n"))) {
                SpecError::InvalidPaper { allowed, value, .. } => {
                    assert_eq!(
                        (allowed, value.as_str()),
                        (vec!["letter".to_string(), "a4".to_string()], bad)
                    );
                }
                other => panic!("{bad}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_bad_style_name_is_refused() {
        assert!(matches!(
            assert_err!(load(Kind::Profile, "[build]\nstyle = \"my style\"\n")),
            SpecError::InvalidStyle { .. }
        ));
    }

    #[test]
    fn a_name_the_parser_would_reject_is_a_spec_error_naming_the_kind_and_key() {
        let cases = [
            ("[select]\ntags = [\"AI\"]", "select.tags", NameKind::Tag, "AI"),
            (
                "[select]\nprefer = [\"a--b\"]",
                "select.prefer",
                NameKind::Variant,
                "a--b",
            ),
            ("[select]\npromote = [\"2x\"]", "select.promote", NameKind::Tag, "2x"),
            (
                "[select.pick]\nGateway = \"long\"",
                "select.pick",
                NameKind::Group,
                "Gateway",
            ),
            (
                "[select.pick]\ngateway = \"Long\"",
                "select.pick",
                NameKind::Variant,
                "Long",
            ),
            ("extends = [\"../x\"]", "extends", NameKind::Profile, "../x"),
            ("extends = [\"Principal\"]", "extends", NameKind::Profile, "Principal"),
            ("extends = [\"con\"]", "extends", NameKind::Profile, "con"),
        ];
        for (text, want_key, want_kind, want_value) in cases {
            match assert_err!(load(Kind::Profile, text)) {
                SpecError::InvalidName { key, kind, value, .. } => {
                    assert_eq!(
                        (key.as_str(), kind, value.as_str()),
                        (want_key, want_kind, want_value),
                        "{text}"
                    );
                }
                other => panic!("{text}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_name_repeated_in_one_list_is_refused_at_its_second_use() {
        match assert_err!(load(Kind::Profile, "[select]\ntags = [\"ai\",\n \"llm\", \"ai\"]\n")) {
            SpecError::DuplicateEntry { at, key, value } => {
                assert_eq!((at.line, key.as_str(), value.as_str()), (3, "select.tags", "ai"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            assert_err!(load(Kind::Profile, "extends = [\"a\", \"a\"]")),
            SpecError::DuplicateEntry { .. }
        ));
    }

    #[test]
    fn a_name_repeated_is_refused_in_every_list() {
        for key in ["tags", "prefer", "promote"] {
            let text = format!("[select]\n{key} = [\"a\", \"b\", \"a\"]\n");
            match assert_err!(load(Kind::Profile, &text)) {
                SpecError::DuplicateEntry { key: got, value, .. } => {
                    assert_eq!((got, value.as_str()), (format!("select.{key}"), "a"));
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn a_content_path_that_is_not_portable_names_the_rule() {
        match assert_err!(load(Kind::Spec, "[build]\ncontent = \"C:\\\\x.md\"\n")) {
            SpecError::InvalidPath { rule, key, .. } => {
                assert_eq!((rule, key), (super::super::names::PathRule::Backslash, "build.content"));
            }
            other => panic!("{other:?}"),
        }
        for bad in ["a\\u202eb.md", "a\\u001bb.md", "."] {
            assert!(
                matches!(
                    assert_err!(load(Kind::Spec, &format!("[build]\ncontent = \"{bad}\"\n"))),
                    SpecError::InvalidPath { .. }
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn picks_are_reported_as_alternates_with_their_place() {
        let layer = assert_ok!(load(
            Kind::Profile,
            "[select.pick]\ngateway = \"short\"\nsummary = \"ai\"\n"
        ));
        let picks = layer.picks();
        assert_eq!(picks.len(), 2);
        assert_eq!(picks[0].0.group.as_ref(), "gateway");
        assert_eq!((picks[1].1.line, picks[1].0.variant.as_ref()), (3, "ai"));
    }

    #[test]
    fn a_pick_is_located_where_its_wording_was_written() {
        let layer = assert_ok!(load(Kind::Profile, "[select.pick]\ngateway = \"short\"\n"));
        let picks = layer.picks();
        assert_eq!((picks[0].1.line, picks[0].1.column), (2, 11));
    }

    #[test]
    fn a_value_echoed_by_any_error_has_its_control_characters_escaped() {
        let esc = "\\u001b[31m";
        let cases = [
            (Kind::Profile, format!("[select]\ntags = [\"a{esc}b\"]\n")),
            (Kind::Spec, format!("[build]\ncontent = \"a{esc}.md\"\n")),
            (Kind::Profile, format!("[build]\npaper = \"{esc}\"\n")),
            (Kind::Profile, format!("[build]\nstyle = \"{esc}\"\n")),
            (Kind::Profile, format!("[select.pick]\n\"g{esc}\" = \"x\"\n")),
            (
                Kind::Spec,
                format!("[position]\nemphasis = {{ technical = \"{esc}\" }}\n[build]\ncontent = \"a.md\"\n"),
            ),
        ];
        for (kind, text) in cases {
            // The escape really is in the input and shows up escaped, so the check is not vacuous.
            let message = assert_err!(load(kind, &text)).to_string();
            assert!(message.contains("\\u{1b}"), "{text:?} -> {message:?}");
            assert!(!message.contains('\u{1b}'), "{text:?} -> {message:?}");
        }
    }

    #[test]
    fn a_spec_layer_is_a_spec_and_only_a_spec() {
        let spec = assert_ok!(load(Kind::Spec, SPEC));
        assert_ok!(SpecLayer::try_from_layer(spec));
        let profile = assert_ok!(load(Kind::Profile, "[select]\ntags = [\"ai\"]\n"));
        assert!(matches!(
            assert_err!(SpecLayer::try_from_layer(profile)),
            SpecError::NotASpec { .. }
        ));
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;
    use claims::{assert_err, assert_ok, assert_some};
    use pretty_assertions::assert_eq;

    fn load(kind: Kind, text: &str) -> Result<Layer, SpecError> {
        Layer::from_source(kind, &Source::new(PathBuf::from("f.toml"), text.to_string()))
    }

    fn spec(select: &str) -> String {
        format!("[build]\ncontent = \"a.md\"\n{select}")
    }

    fn id(name: &str) -> ItemId {
        assert_ok!(ItemId::try_new(name))
    }

    #[test]
    fn an_override_table_is_read_with_the_line_of_each_field() {
        let text = spec(
            "[select.override.gateway]\ntags = [\"ai\", \"platform\"]\npriority = \"core\"\n\n[select.override.cache]\ntags = []\npriority = \"none\"\n",
        );
        let layer = assert_ok!(load(Kind::Spec, &text));
        let gateway = &layer.select.overrides[&id("gateway")];
        let tags = assert_some!(gateway.tags.as_ref());
        assert_eq!(
            tags.value.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["ai", "platform"]
        );
        assert_eq!(tags.at.line, 4, "the line of `tags`");
        let priority = assert_some!(gateway.priority.as_ref());
        assert_eq!((priority.value, priority.at.line), (PriorityEdit::Core, 5));
        let cache = &layer.select.overrides[&id("cache")];
        assert_eq!(
            assert_some!(cache.tags.as_ref()).value.len(),
            0,
            "`tags = []` is a choice"
        );
        assert_eq!(assert_some!(cache.priority.as_ref()).value, PriorityEdit::None);
    }

    #[test]
    fn a_table_with_only_one_field_leaves_the_other_unsaid() {
        let layer = assert_ok!(load(Kind::Spec, &spec("[select.override.a]\ntags = [\"ai\"]\n")));
        let a = &layer.select.overrides[&id("a")];
        assert!(a.tags.is_some() && a.priority.is_none());
        let layer = assert_ok!(load(Kind::Spec, &spec("[select.override.a]\npriority = \"extra\"\n")));
        let a = &layer.select.overrides[&id("a")];
        assert!(a.tags.is_none() && a.priority.is_some());
    }

    #[test]
    fn an_empty_override_table_is_an_error_at_its_header() {
        let error = assert_err!(load(Kind::Spec, &spec("[select.override.a]\n")));
        match error {
            SpecError::EmptyOverride { at, id, .. } => assert_eq!((at.line, id.as_str()), (3, "a")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_override_may_not_change_anything_but_tags_and_priority() {
        for field in [
            "status = \"draft\"",
            "text = \"x\"",
            "id = \"b\"",
            "variant = \"short\"",
            "order = 1",
        ] {
            let error = assert_err!(load(
                Kind::Spec,
                &spec(&format!("[select.override.a]\ntags = [\"ai\"]\n{field}\n"))
            ));
            match error {
                SpecError::UnknownKey { key, expected, .. } => {
                    assert!(key.starts_with("select.override.a."), "{key}");
                    assert_eq!(expected, ["tags", "priority"]);
                }
                other => panic!("{field}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_bad_priority_is_a_located_error_naming_the_choices() {
        for bad in ["\"high\"", "\"Core\"", "\"@core\"", "1", "true"] {
            let error = assert_err!(load(
                Kind::Spec,
                &spec(&format!("[select.override.a]\npriority = {bad}\n"))
            ));
            let text = error.to_string();
            assert!(text.contains("f.toml:4:"), "{bad}: {text}");
            assert!(text.contains("priority"), "{bad}: {text}");
        }
    }

    #[test]
    fn an_invalid_id_or_tag_is_a_located_name_error() {
        match assert_err!(load(Kind::Spec, &spec("[select.override.Bad_Id]\ntags = [\"ai\"]\n"))) {
            SpecError::InvalidName { at, key, kind, value } => {
                assert_eq!(
                    (at.line, key.as_str(), kind, value.as_str()),
                    (3, "select.override", NameKind::Item, "Bad_Id")
                );
            }
            other => panic!("{other:?}"),
        }
        match assert_err!(load(Kind::Spec, &spec("[select.override.a]\ntags = [\"Not A Tag\"]\n"))) {
            SpecError::InvalidName { at, kind, .. } => assert_eq!((at.line, kind), (4, NameKind::Tag)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_tag_twice_in_one_override_is_a_duplicate() {
        let error = assert_err!(load(
            Kind::Spec,
            &spec("[select.override.a]\ntags = [\"ai\", \"ai\"]\n")
        ));
        assert!(matches!(error, SpecError::DuplicateEntry { .. }), "{error:?}");
    }

    #[test]
    fn a_profile_may_hold_overrides() {
        let layer = assert_ok!(load(Kind::Profile, "[select.override.a]\ntags = [\"ai\"]\n"));
        assert!(layer.select.overrides.contains_key(&id("a")));
    }

    #[test]
    fn the_letter_has_its_own_override_table_apart_from_the_resumes() {
        let text = "[build]\ncontent = \"a.md\"\n[select.override.one]\ntags = [\"ai\"]\n\n[letter]\ncontent = \"l.md\"\n\n[letter.select.override.two]\npriority = \"core\"\n";
        let layer = assert_ok!(load(Kind::Spec, text));
        assert!(layer.select.overrides.contains_key(&id("one")));
        assert!(!layer.select.overrides.contains_key(&id("two")));
        assert!(layer.letter_select.overrides.contains_key(&id("two")));
        assert!(!layer.letter_select.overrides.contains_key(&id("one")));
    }

    #[test]
    fn the_same_id_in_the_resume_and_the_letter_is_legal() {
        let text = "[build]\ncontent = \"a.md\"\n[select.override.same]\ntags = [\"ai\"]\n\n[letter]\ncontent = \"l.md\"\n\n[letter.select.override.same]\ntags = [\"data\"]\n";
        let layer = assert_ok!(load(Kind::Spec, text));
        assert_eq!(layer.select.overrides.len(), 1);
        assert_eq!(layer.letter_select.overrides.len(), 1);
    }

    #[test]
    fn an_override_table_inline_or_dotted_reads_the_same_as_a_header() {
        let header = assert_ok!(load(Kind::Spec, &spec("[select.override.a]\ntags = [\"ai\"]\n")));
        let inline = assert_ok!(load(
            Kind::Spec,
            &spec("[select]\noverride = { a = { tags = [\"ai\"] } }\n")
        ));
        let values = |l: &Layer| l.select.overrides[&id("a")].tags.as_ref().map(|t| t.value.clone());
        assert_eq!(values(&header), values(&inline));
    }
}

#[cfg(test)]
mod origin_tests {
    use super::*;
    use claims::{assert_ok, assert_some};
    use pretty_assertions::assert_eq;

    fn load(text: &str) -> Layer {
        assert_ok!(Layer::from_source(
            Kind::Spec,
            &Source::new(PathBuf::from("f.toml"), text.to_string())
        ))
    }

    fn tags_line(layer: &Layer, id: &str) -> usize {
        let id = assert_ok!(ItemId::try_new(id));
        assert_some!(layer.select.overrides[&id].tags.as_ref()).at.line
    }

    #[test]
    fn an_inline_override_locates_its_tags_on_its_own_line_not_in_a_later_table() {
        let text = "[build]\ncontent = \"a.md\"\n[select.override]\nmissing = { tags = [] }\n[letter]\ncontent = \"l.md\"\n[letter.select]\ntags = [\"elsewhere\"]\n";
        assert_eq!(tags_line(&load(text), "missing"), 4);
    }

    #[test]
    fn a_quoted_key_still_locates_the_field_it_names() {
        let text = "[build]\ncontent = \"a.md\"\n[select.override.a]\n\"tags\" = [\"ai\"]\n[letter]\ncontent = \"l.md\"\n[letter.select]\ntags = [\"x\"]\n";
        assert_eq!(tags_line(&load(text), "a"), 4);
    }

    #[test]
    fn each_field_is_on_its_own_line_whichever_order_they_are_written_in() {
        let text = "[build]\ncontent = \"a.md\"\n[select.override.a]\npriority = \"core\"\ntags = []\n";
        let layer = load(text);
        let a = &layer.select.overrides[&assert_ok!(ItemId::try_new("a"))];
        assert_eq!(assert_some!(a.priority.as_ref()).at.line, 4);
        assert_eq!(assert_some!(a.tags.as_ref()).at.line, 5);
    }

    #[test]
    fn an_empty_tag_list_in_a_table_is_located_at_its_key() {
        let text = "[build]\ncontent = \"a.md\"\n[select.override.a]\n\n# a note about tags = here\ntags = []\n[letter]\ncontent = \"l.md\"\n[letter.select]\ntags = [\"x\"]\n";
        assert_eq!(tags_line(&load(text), "a"), 6);
    }

    #[test]
    fn two_overrides_with_empty_tags_are_each_located_at_their_own_key() {
        let text = "[build]\ncontent = \"a.md\"\n[select.override.a]\ntags = []\n[select.override.b]\ntags = []\n";
        let layer = load(text);
        assert_eq!((tags_line(&layer, "a"), tags_line(&layer, "b")), (4, 6));
    }
}

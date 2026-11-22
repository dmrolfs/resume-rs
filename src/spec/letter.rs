//! ABOUTME: The `[letter]` table of a spec and `[letter.select]` of any file: validated here, resolved apart from the resume.
//! ABOUTME: Shape and keywords are checked for every command; letter field values only when a letter is built.

use std::collections::BTreeMap;

use clap::ValueEnum;

use super::error::{
    DateProblem, ForbiddenInProfileSnafu, InvalidDateSnafu, InvalidKeywordSnafu, Located, MissingKeySnafu,
    NoLetterTableSnafu, RequiresKeySnafu, SpecError, TextLengthSnafu, UnsafeTextSnafu, shown,
};
use super::layer::{Kind, LetterTable, SelectLayer, Sourced, date, paper, select, sourced, style, table_header};
use super::merge::{ChoiceKey, SelectChoices};
use super::names::DeclaredPath;
use super::raw::RawLetter;
use super::read::Source;
use super::resolved::{ContentRef, ResolvedSpec};
use crate::diagnostics::has_unsafe_characters;
use crate::letter::{FieldValue, FieldValueError, LengthUnit, LetterDate, LetterFields, LetterOutput, LinkStyle};
use crate::model::StyleName;
use crate::model::options::Paper;
use crate::select::{Scope, SelectedTags, Selection, UnknownPick};

/// What `[letter]` says besides its content file. Every value is optional: a letter command has its own defaults.
///
/// The recipient stays text until a letter is built, so a resume build never applies a letter field rule to it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LetterSettings {
    pub recipient: Option<Sourced<String>>,
    pub date: Option<Sourced<time::Date>>,
    pub style: Option<Sourced<StyleName>>,
    pub paper: Option<Sourced<Paper>>,
    pub output_format: Option<Sourced<LetterOutput>>,
    pub max_length: Option<Sourced<usize>>,
    pub length_unit: Option<Sourced<LengthUnit>>,
    pub links: Option<Sourced<LinkStyle>>,
}

/// Validates the `[letter]` table of one file into its settings (spec only) and its select layer (any file).
///
/// A profile may hold `letter.select` and nothing else of the table. A spec that has the table must name its
/// content file, as a spec must name `build.content`.
pub(super) fn from_raw(
    kind: Kind,
    source: &Source,
    raw: Option<RawLetter>,
) -> Result<(Option<LetterTable>, SelectLayer), SpecError> {
    let Some(raw) = raw else {
        return Ok((None, SelectLayer::default()));
    };
    let RawLetter {
        content,
        recipient,
        date: letter_date,
        style: letter_style,
        paper: letter_paper,
        output_format,
        max_length,
        length_unit,
        links,
        select: raw_select,
    } = raw;
    let select = select(source, "letter.select", raw_select.map(Into::into))?;
    let written = {
        let forbidden = [
            (content.as_ref().map(|t| t.span().start), "letter.content"),
            (recipient.as_ref().map(|t| t.span().start), "letter.recipient"),
            (letter_date.as_ref().map(|t| t.span().start), "letter.date"),
            (letter_style.as_ref().map(|t| t.span().start), "letter.style"),
            (letter_paper.as_ref().map(|t| t.span().start), "letter.paper"),
            (output_format.as_ref().map(|t| t.span().start), "letter.output-format"),
            (max_length.as_ref().map(|t| t.span().start), "letter.max-length"),
            (length_unit.as_ref().map(|t| t.span().start), "letter.length-unit"),
            (links.as_ref().map(|t| t.span().start), "letter.links"),
        ];
        // The first one written, so an error points at the line a reader reaches first.
        forbidden
            .into_iter()
            .filter_map(|(at, key)| at.map(|at| (at, key)))
            .min()
    };
    if kind == Kind::Profile {
        return match written {
            Some((offset, key)) => ForbiddenInProfileSnafu {
                at: source.at(offset),
                key,
                reason: "only `letter.select` is shared; the rest describes one application",
            }
            .fail(),
            None => Ok((None, select)),
        };
    }
    let Some(content) = content else {
        return MissingKeySnafu {
            at: source.at(table_header(&source.text, "[letter")
                .or_else(|| written.map(|(offset, _)| offset))
                .unwrap_or(0)),
            key: "letter.content",
        }
        .fail();
    };
    let content = sourced(source, &content, |at, raw| {
        DeclaredPath::parse(raw).map_err(|rule| {
            super::error::InvalidPathSnafu {
                at,
                key: "letter.content",
                value: shown(raw),
                rule,
            }
            .build()
        })
    })?;
    let settings = LetterSettings {
        recipient: recipient
            .as_ref()
            .map(|text| sourced(source, text, |_, raw| Ok(raw.to_string())))
            .transpose()?,
        date: date(source, letter_date.as_ref(), "letter.date")?,
        style: letter_style.as_ref().map(|t| style(source, t)).transpose()?,
        paper: letter_paper.as_ref().map(|t| paper(source, t)).transpose()?,
        output_format: output_format
            .as_ref()
            .map(|t| keyword::<LetterOutput>(source, t, "letter.output-format"))
            .transpose()?,
        max_length: max_length.map(|n| Sourced {
            value: *n.get_ref(),
            at: source.at(n.span().start),
        }),
        length_unit: length_unit
            .as_ref()
            .map(|t| keyword::<LengthUnit>(source, t, "letter.length-unit"))
            .transpose()?,
        links: links
            .as_ref()
            .map(|t| keyword::<LinkStyle>(source, t, "letter.links"))
            .transpose()?,
    };
    if let (Some(unit), None) = (&settings.length_unit, &settings.max_length) {
        return RequiresKeySnafu {
            at: unit.at.clone(),
            key: "letter.length-unit",
            needs: "letter.max-length",
        }
        .fail();
    }
    Ok((Some(LetterTable { content, settings }), select))
}

/// A value that is one of an enum's command-line spellings, as `--links` and the others take it.
fn keyword<T: ValueEnum>(source: &Source, text: &super::raw::Text, key: &'static str) -> Result<Sourced<T>, SpecError> {
    sourced(source, text, |at, raw| {
        T::from_str(raw, false).map_err(|_| {
            let allowed: Vec<String> = T::value_variants()
                .iter()
                .filter_map(ValueEnum::to_possible_value)
                .map(|v| v.get_name().to_string())
                .collect();
            InvalidKeywordSnafu {
                at,
                key,
                value: shown(raw),
                allowed,
            }
            .build()
        })
    })
}

/// What a spec says about its letter, resolved over the same chain as the resume but apart from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLetter {
    /// The letter content file, joined onto the directory of the spec that named it; never opened here.
    pub content: ContentRef,
    /// `letter.select` of every file of the chain, merged by the rules `select` follows.
    pub choices: SelectChoices,
    pub settings: LetterSettings,
    company: Option<crate::model::Company>,
    title: Option<crate::model::Title>,
    origins: BTreeMap<ChoiceKey, Located>,
}

impl ResolvedLetter {
    pub(super) fn new(
        table: LetterTable,
        spec_dir: &std::path::Path,
        position: Option<&super::layer::Position>,
        (choices, origins): (SelectChoices, BTreeMap<ChoiceKey, Located>),
    ) -> Self {
        let LetterTable { content, settings } = table;
        let (declared, at) = (content.value, content.at);
        Self {
            content: ContentRef {
                path: declared.resolve_against(spec_dir),
                declared,
                at,
            },
            choices,
            settings,
            company: position.and_then(|p| p.company.clone()),
            title: position.and_then(|p| p.title.clone()),
            origins,
        }
    }

    /// The file and place that decided `key` of `letter.select`, if anything did.
    #[must_use]
    pub fn origin_of(&self, key: &ChoiceKey) -> Option<&Located> {
        self.origins.get(key)
    }

    /// Every `letter.select` choice and the place that decided it, in key order.
    pub fn origins(&self) -> impl Iterator<Item = (&ChoiceKey, &Located)> {
        self.origins.iter()
    }

    /// The company, title, recipient, and date a letter is built with, each checked as a letter field.
    ///
    /// Called only when a letter is built, so a resume build never meets a letter field rule. The company and the
    /// title come from `[position]` and passed its checks, which are stricter than a letter field's.
    ///
    /// # Errors
    /// [`SpecError::UnsafeText`] or [`SpecError::TextLength`] for a recipient a letter cannot print.
    /// Company and title passed `[position]`'s stricter checks at load, so they are not the cause in practice.
    pub fn fields(&self) -> Result<LetterFields, SpecError> {
        let here = &self.content.at;
        let value = |text: &str, key: &str, at: &Located| {
            if has_unsafe_characters(text) {
                return UnsafeTextSnafu { at: at.clone(), key }.fail();
            }
            FieldValue::try_new(text).map_err(|refused| match refused {
                // Line and paragraph separators pass the unsafe check but a letter field is one line.
                FieldValueError::PredicateViolated => UnsafeTextSnafu { at: at.clone(), key }.build(),
                _ => TextLengthSnafu {
                    at: at.clone(),
                    key,
                    max: 120_usize,
                }
                .build(),
            })
        };
        Ok(LetterFields {
            company: self
                .company
                .as_ref()
                .map(|c| value(c.as_ref(), "position.company", here))
                .transpose()?,
            title: self
                .title
                .as_ref()
                .map(|t| value(t.as_ref(), "position.title", here))
                .transpose()?,
            recipient: self
                .settings
                .recipient
                .as_ref()
                .map(|r| value(&r.value, "letter.recipient", &r.at))
                .transpose()?,
            date: self
                .settings
                .date
                .as_ref()
                .map(|d| {
                    LetterDate::try_from(d.value).map_err(|_| {
                        InvalidDateSnafu {
                            at: d.at.clone(),
                            key: "letter.date",
                            problem: DateProblem::Year,
                        }
                        .build()
                    })
                })
                .transpose()?,
        })
    }
}

impl ResolvedSpec {
    /// The letter selection this spec asks for, by the rules of [`ResolvedSpec::to_selection`]: drafts off, and
    /// `core-only` with tags a conflict.
    ///
    /// # Errors
    /// [`SpecError::NoLetterTable`] for a spec without `[letter]`; [`SpecError::ScopeConflict`] as for a resume.
    pub fn to_letter_selection(&self) -> Result<Selection, SpecError> {
        let Some(letter) = &self.letter else {
            return NoLetterTableSnafu.fail();
        };
        let select = &letter.choices;
        let scope = match (select.core_only, SelectedTags::new(select.tags.clone())) {
            (Some(true), Some(_)) => return self.scope_conflict_of(select, |key| letter.origin_of(key)),
            (Some(true), None) => Scope::CoreOnly,
            (_, Some(tags)) => Scope::Tags(tags),
            (_, None) => Scope::All,
        };
        Ok(Selection {
            scope,
            drafts: false,
            prefer: select.prefer.clone(),
            picks: select.pick.clone(),
        })
    }

    /// Fails when a letter pick names a group or wording the letter does not have, naming the line that wrote it.
    ///
    /// `unknown` is the `unknown_picks` of the letter report. The lines come from `letter.select` alone, so a
    /// misspelled letter pick is never looked up among the resume's picks, where it would have no line to name.
    ///
    /// # Errors
    /// [`SpecError::NoLetterTable`] without `[letter]`; [`SpecError::UnknownPicks`] listing every such pick.
    pub fn check_letter_picks(&self, unknown: &[UnknownPick]) -> Result<(), SpecError> {
        let Some(letter) = &self.letter else {
            return NoLetterTableSnafu.fail();
        };
        self.check_picks_with(unknown, |key| letter.origin_of(key))
    }
}

//! ABOUTME: The shape of a spec or profile file as TOML delivers it: strings with their positions, nothing checked yet.
//! ABOUTME: One struct serves both kinds of file, so unknown keys are refused the same way everywhere.

use std::collections::BTreeMap;

use super::raw_additions::{RawLetterAddition, RawResumeAddition};
use serde::Deserialize;
use toml::Spanned;
use toml::value::Datetime;

/// A string leaf with the byte range it was written at.
pub(super) type Text = Spanned<String>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawFile {
    pub extends: Option<Vec<Text>>,
    pub position: Option<RawPosition>,
    pub build: Option<RawBuild>,
    pub select: Option<RawSelect>,
    pub letter: Option<RawLetter>,
    pub add: Option<Spanned<Vec<RawResumeAddition>>>,
}

/// The `[position]` table. It is not wrapped in `Spanned`: a table written as `[position.emphasis]` or as
/// `position.company = ..` has no span of its own, so [`RawPosition::first_offset`] finds a place from its leaves.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawPosition {
    pub company: Option<Text>,
    pub title: Option<Text>,
    pub level: Option<Text>,
    pub domain: Option<Text>,
    pub audience: Option<Text>,
    pub emphasis: Option<RawEmphasis>,
    pub date: Option<Spanned<Datetime>>,
    pub posting: Option<Text>,
}

impl RawPosition {
    /// Where the table starts being written: the earliest of its values, or `None` for an empty table.
    pub(super) fn first_offset(&self) -> Option<usize> {
        let texts = [
            &self.company,
            &self.title,
            &self.level,
            &self.domain,
            &self.audience,
            &self.posting,
        ];
        let mut starts: Vec<usize> = texts
            .iter()
            .filter_map(|t| t.as_ref())
            .map(|t| t.span().start)
            .collect();
        starts.extend(self.date.iter().map(|d| d.span().start));
        if let Some(emphasis) = &self.emphasis {
            let axes = [&emphasis.business, &emphasis.org, &emphasis.technical];
            starts.extend(axes.iter().filter_map(|t| t.as_ref()).map(|t| t.span().start));
        }
        starts.into_iter().min()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawEmphasis {
    pub business: Option<Text>,
    pub org: Option<Text>,
    pub technical: Option<Text>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawBuild {
    pub content: Option<Text>,
    pub style: Option<Text>,
    pub paper: Option<Text>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawSelect {
    pub tags: Option<Vec<Text>>,
    pub prefer: Option<Vec<Text>>,
    pub promote: Option<Vec<Text>>,
    /// The group is a plain `String`: a map key wrapped in `Spanned` has no readable path when its value is wrong.
    pub pick: Option<BTreeMap<String, Text>>,
    pub core_only: Option<Spanned<bool>>,
    /// `[select.override.<id>]`. The id is a plain map key, as `pick`'s group is: a key in `Spanned` has no position.
    #[serde(rename = "override")]
    pub overrides: Option<BTreeMap<String, Spanned<RawOverride>>>,
}

/// One item's override: only its own tags and priority. Nothing else about an item can be changed from a spec.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawOverride {
    /// `Spanned` as a whole, so an empty list still has the place `tags =` was written.
    pub tags: Option<Spanned<Vec<Text>>>,
    pub priority: Option<Text>,
}

/// The `[letter]` table. Every key is optional here; which are allowed depends on the kind of file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawLetter {
    pub content: Option<Text>,
    pub recipient: Option<Text>,
    pub date: Option<Spanned<Datetime>>,
    pub style: Option<Text>,
    pub paper: Option<Text>,
    pub output_format: Option<Text>,
    pub max_length: Option<Spanned<usize>>,
    pub length_unit: Option<Text>,
    pub links: Option<Text>,
    pub select: Option<RawLetterSelect>,
    pub add: Option<Spanned<Vec<RawLetterAddition>>>,
}

/// `[letter.select]`: the resume's `[select]` without `promote`, because a letter keeps its written order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawLetterSelect {
    pub tags: Option<Vec<Text>>,
    pub prefer: Option<Vec<Text>>,
    pub pick: Option<BTreeMap<String, Text>>,
    pub core_only: Option<Spanned<bool>>,
    #[serde(rename = "override")]
    pub overrides: Option<BTreeMap<String, Spanned<RawOverride>>>,
}

impl From<RawLetterSelect> for RawSelect {
    fn from(raw: RawLetterSelect) -> Self {
        Self {
            tags: raw.tags,
            prefer: raw.prefer,
            promote: None,
            pick: raw.pick,
            core_only: raw.core_only,
            overrides: raw.overrides,
        }
    }
}

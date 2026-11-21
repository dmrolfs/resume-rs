//! ABOUTME: The shape of a spec or profile file as TOML delivers it: strings with their positions, nothing checked yet.
//! ABOUTME: One struct serves both kinds of file, so unknown keys are refused the same way everywhere.

use std::collections::BTreeMap;

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
}

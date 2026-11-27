//! Strict TOML shapes for final-spec leaf additions.
use super::raw::Text;
use serde::Deserialize;
use toml::Spanned;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawPlace {
    pub before: Option<Text>,
    pub after: Option<Text>,
    pub parent: Option<Text>,
    pub at: Option<Text>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawResumeAddition {
    pub id: Text,
    pub kind: Text,
    pub place: Spanned<RawPlace>,
    pub content: Text,
    pub tags: Option<Spanned<Vec<Text>>>,
    pub priority: Option<Text>,
    pub status: Option<Text>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawLetterAddition {
    pub id: Text,
    pub place: Spanned<RawPlace>,
    pub content: Text,
    pub tags: Option<Spanned<Vec<Text>>>,
    pub priority: Option<Text>,
    pub status: Option<Text>,
}

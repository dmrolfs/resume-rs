//! ABOUTME: The outcome of resolving a spec: its position, content file, merged choices, and the chain behind them.
//! ABOUTME: Printable as one flat TOML spec that, saved beside the original, resolves to the same choices.

use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::ValueEnum;
use serde::Serialize;

use super::error::Located;
use super::layer::{Kind, Layer, Position};
use super::merge::{ChoiceKey, Choices, merge};
use super::names::{DeclaredPath, ProfileName};

/// One file of the chain, in the order it was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainLink {
    /// The profile's name; `None` for the spec itself.
    pub name: Option<ProfileName>,
    /// The file as found.
    pub path: PathBuf,
    /// What the file says it extends, in the order it wrote it.
    pub declared_extends: Vec<ProfileName>,
}

/// The content file a spec names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentRef {
    /// The path as the spec wrote it.
    pub declared: DeclaredPath,
    /// The path joined onto the spec's directory: where to read it from. Not canonicalized, not opened.
    pub path: PathBuf,
}

/// Everything a build needs from a spec and its profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSpec {
    pub position: Option<Position>,
    pub content: ContentRef,
    pub choices: Choices,
    /// Every file that contributed, least specific first, the spec last.
    pub chain: Vec<ChainLink>,
    origins: BTreeMap<ChoiceKey, Located>,
}

impl ResolvedSpec {
    /// Resolves a chain ordered least specific first, whose last layer is the spec.
    ///
    /// The chain comes from [`super::chain::order`], which always ends in the spec the caller loaded as a
    /// [`super::layer::SpecLayer`], so this is private to the module family: nothing outside can hand it a profile.
    pub(super) fn from_chain(layers: Vec<Layer>) -> Self {
        let merged = merge(&layers);
        let chain = layers
            .iter()
            .map(|layer| ChainLink {
                name: layer.name.clone(),
                path: layer.path.clone(),
                declared_extends: layer.extends.iter().map(|e| e.value.clone()).collect(),
            })
            .collect();
        let spec = layers.into_iter().next_back().filter(|l| l.kind == Kind::Spec);
        let Some(spec) = spec else {
            unreachable!("a chain ends in its spec");
        };
        let Some(declared) = spec.content.map(|c| c.value) else {
            unreachable!("loading a spec requires its content path");
        };
        let dir = spec.path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        Self {
            position: spec.position,
            content: ContentRef {
                path: declared.resolve_against(&dir),
                declared,
            },
            choices: merged.choices,
            chain,
            origins: merged.origins,
        }
    }

    /// Every choice and the file and place that decided it, in key order.
    pub fn origins(&self) -> impl Iterator<Item = (&ChoiceKey, &Located)> {
        self.origins.iter()
    }

    /// The file and place that decided `key`, if anything did.
    #[must_use]
    pub fn origin_of(&self, key: &ChoiceKey) -> Option<&Located> {
        self.origins.get(key)
    }

    /// The choices as one flat spec, with no `extends`: the same TOML a user could write by hand.
    ///
    /// Saved beside the original spec, it resolves to the same choices, because `content` stays relative to the
    /// spec's directory. Scalars come before tables and every map is sorted, so the text is the same on every run.
    ///
    /// # Panics
    /// Never in practice: the values are strings, numbers, and dates, which TOML can always write.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let chain: Vec<String> = self
            .chain
            .iter()
            .map(|link| {
                link.name
                    .as_ref()
                    .map_or_else(|| "the spec".to_string(), ToString::to_string)
            })
            .collect();
        let mut out = format!("# Resolved from: {}\n\n", chain.join(", "));
        let body = toml::to_string(&Printed::from(self));
        match body {
            Ok(text) => out.push_str(&text),
            Err(e) => unreachable!("a resolved spec is always writable as TOML: {e}"),
        }
        out
    }
}

// ---- printing ----------------------------------------------------------------------------------------------

#[derive(Serialize)]
struct Printed<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<PrintedPosition<'a>>,
    build: PrintedBuild<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    select: Option<PrintedSelect<'a>>,
}

#[derive(Serialize)]
struct PrintedPosition<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    company: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audience: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<toml::value::Datetime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    posting: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    emphasis: Option<PrintedEmphasis>,
}

#[derive(Serialize)]
struct PrintedEmphasis {
    #[serde(skip_serializing_if = "Option::is_none")]
    business: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    technical: Option<String>,
}

#[derive(Serialize)]
struct PrintedBuild<'a> {
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    paper: Option<String>,
}

#[derive(Serialize)]
struct PrintedSelect<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    prefer: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    promote: Vec<&'a str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pick: BTreeMap<&'a str, &'a str>,
    #[serde(rename = "core-only", skip_serializing_if = "Option::is_none")]
    core_only: Option<bool>,
}

impl<'a> From<&'a ResolvedSpec> for Printed<'a> {
    fn from(spec: &'a ResolvedSpec) -> Self {
        let choices = &spec.choices;
        let select = &choices.select;
        let printed_select = PrintedSelect {
            tags: select.tags.iter().map(AsRef::as_ref).collect(),
            prefer: select.prefer.iter().map(AsRef::as_ref).collect(),
            promote: select.promote.iter().map(AsRef::as_ref).collect(),
            pick: select.pick.iter().map(|(g, v)| (g.as_ref(), v.as_ref())).collect(),
            core_only: select.core_only,
        };
        let has_select = !printed_select.tags.is_empty()
            || !printed_select.prefer.is_empty()
            || !printed_select.promote.is_empty()
            || !printed_select.pick.is_empty()
            || printed_select.core_only.is_some();
        Self {
            position: spec.position.as_ref().map(PrintedPosition::from),
            build: PrintedBuild {
                content: spec.content.declared.as_str(),
                style: choices.style.as_ref().map(AsRef::as_ref),
                paper: choices
                    .paper
                    .and_then(|p| p.to_possible_value())
                    .map(|v| v.get_name().to_string()),
            },
            select: has_select.then_some(printed_select),
        }
    }
}

impl<'a> From<&'a Position> for PrintedPosition<'a> {
    fn from(p: &'a Position) -> Self {
        let level = |l: Option<super::names::Level>| l.map(|l| l.to_string());
        let emphasis = &p.emphasis;
        let any_emphasis = emphasis.business.is_some() || emphasis.org.is_some() || emphasis.technical.is_some();
        Self {
            company: p.company.as_ref().map(AsRef::as_ref),
            title: p.title.as_ref().map(AsRef::as_ref),
            level: p.level.as_ref().map(AsRef::as_ref),
            domain: p.domain.as_ref().map(AsRef::as_ref),
            audience: p.audience.as_ref().map(AsRef::as_ref),
            date: p.date.and_then(|d| d.to_string().parse().ok()),
            posting: p.posting.as_ref().map(url::Url::as_str),
            emphasis: any_emphasis.then(|| PrintedEmphasis {
                business: level(emphasis.business),
                org: level(emphasis.org),
                technical: level(emphasis.technical),
            }),
        }
    }
}

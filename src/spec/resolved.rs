//! ABOUTME: The outcome of resolving a spec: its position, content file, merged choices, and the chain behind them.
//! ABOUTME: Printable as one flat TOML spec that, saved beside the original, resolves to the same choices.

use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::ValueEnum;
use serde::Serialize;

use super::error::Located;
use super::layer::{Kind, Layer, Position};
use super::letter::ResolvedLetter;
use super::merge::{ChoiceKey, Choices, merge, merge_select};
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
    /// Where the spec wrote `content =`, so an error about the file can point at the line that named it.
    pub at: Located,
}

/// Everything a build needs from a spec and its profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSpec {
    pub position: Option<Position>,
    pub content: ContentRef,
    pub choices: Choices,
    /// Every file that contributed, least specific first, the spec last.
    pub chain: Vec<ChainLink>,
    /// The letter the spec builds, resolved apart from the resume; `None` for a spec with no `[letter]`.
    pub letter: Option<ResolvedLetter>,
    origins: BTreeMap<ChoiceKey, Located>,
}

impl ResolvedSpec {
    /// Resolves a chain ordered least specific first, whose last layer is the spec.
    ///
    /// The chain comes from [`super::chain::order`], which always ends in the spec the caller loaded as a
    /// [`super::layer::SpecLayer`], so this is private to the module family: nothing outside can hand it a profile.
    pub(super) fn from_chain(layers: Vec<Layer>) -> Self {
        let merged = merge(&layers);
        let spec_has_letter = layers.last().is_some_and(|layer| layer.letter.is_some());
        let letter_merged = spec_has_letter.then(|| merge_select(layers.iter().map(|layer| &layer.letter_select)));
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
        let Some(content) = spec.content else {
            unreachable!("loading a spec requires its content path");
        };
        let (declared, at) = (content.value, content.at);
        let dir = spec.path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        let letter = spec
            .letter
            .zip(letter_merged)
            .map(|(table, merged)| ResolvedLetter::new(table, &dir, spec.position.as_ref(), merged));
        Self {
            position: spec.position,
            content: ContentRef {
                path: declared.resolve_against(&dir),
                declared,
                at,
            },
            choices: merged.choices,
            chain,
            letter,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    letter: Option<PrintedLetter<'a>>,
}

#[derive(Serialize)]
struct PrintedLetter<'a> {
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    recipient: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<toml::value::Datetime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    paper: Option<String>,
    #[serde(rename = "output-format", skip_serializing_if = "Option::is_none")]
    output_format: Option<String>,
    #[serde(rename = "max-length", skip_serializing_if = "Option::is_none")]
    max_length: Option<usize>,
    #[serde(rename = "length-unit", skip_serializing_if = "Option::is_none")]
    length_unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    links: Option<String>,
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
    /// `[select.override.<id>]`, sorted by id. Printed last, so the plain keys above stay in one table.
    #[serde(rename = "override", skip_serializing_if = "BTreeMap::is_empty")]
    overrides: BTreeMap<&'a str, PrintedOverride<'a>>,
}

/// One item's overrides. A field nobody wrote is left out; one that was written is always printed, an empty tag
/// list and `none` included, because they are choices and not the absence of one.
#[derive(Serialize)]
struct PrintedOverride<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<&'a str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<&'static str>,
}

impl<'a> PrintedSelect<'a> {
    /// The table, or `None` when the choices say nothing, so an empty table is never printed.
    fn of(select: &'a super::merge::SelectChoices) -> Option<Self> {
        let printed = Self {
            tags: select.tags.iter().map(AsRef::as_ref).collect(),
            prefer: select.prefer.iter().map(AsRef::as_ref).collect(),
            promote: select.promote.iter().map(AsRef::as_ref).collect(),
            pick: select.pick.iter().map(|(g, v)| (g.as_ref(), v.as_ref())).collect(),
            core_only: select.core_only,
            overrides: select
                .overrides
                .iter()
                .map(|(id, o)| {
                    let printed = PrintedOverride {
                        tags: o.tags.as_ref().map(|t| t.iter().map(AsRef::as_ref).collect()),
                        priority: o.priority.map(Into::into),
                    };
                    (id.as_ref(), printed)
                })
                .collect(),
        };
        let says_something = !printed.tags.is_empty()
            || !printed.prefer.is_empty()
            || !printed.promote.is_empty()
            || !printed.pick.is_empty()
            || printed.core_only.is_some()
            || !printed.overrides.is_empty();
        says_something.then_some(printed)
    }
}

impl<'a> From<&'a ResolvedLetter> for PrintedLetter<'a> {
    fn from(letter: &'a ResolvedLetter) -> Self {
        let s = &letter.settings;
        let name = |value: &dyn ValueEnumName| value.name().unwrap_or_default();
        Self {
            content: letter.content.declared.as_str(),
            recipient: s.recipient.as_ref().map(|r| r.value.as_str()),
            date: s.date.as_ref().and_then(|d| d.value.to_string().parse().ok()),
            style: s.style.as_ref().map(|v| v.value.as_ref()),
            paper: s.paper.as_ref().map(|v| name(&v.value)),
            output_format: s.output_format.as_ref().map(|v| name(&v.value)),
            max_length: s.max_length.as_ref().map(|v| v.value),
            length_unit: s.length_unit.as_ref().map(|v| name(&v.value)),
            links: s.links.as_ref().map(|v| name(&v.value)),
            select: PrintedSelect::of(&letter.choices),
        }
    }
}

/// The spelling a file and a flag use for a value.
trait ValueEnumName {
    fn name(&self) -> Option<String>;
}

impl<T: ValueEnum> ValueEnumName for T {
    fn name(&self) -> Option<String> {
        self.to_possible_value().map(|v| v.get_name().to_string())
    }
}

impl<'a> From<&'a ResolvedSpec> for Printed<'a> {
    fn from(spec: &'a ResolvedSpec) -> Self {
        let choices = &spec.choices;
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
            select: PrintedSelect::of(&choices.select),
            letter: spec.letter.as_ref().map(PrintedLetter::from),
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use claims::{assert_ok, assert_some};
    use pretty_assertions::assert_eq;

    use crate::model::{ItemId, PriorityEdit, Tag};
    use crate::spec::chain::ProfileSource;
    use crate::spec::error::{Located, SpecError};
    use crate::spec::layer::{ItemOverrideLayer, Kind, Layer, SelectLayer, Sourced, SpecLayer};
    use crate::spec::names::{DeclaredPath, ProfileName};
    use crate::spec::resolve_with;

    fn at(file: &str, line: usize) -> Located {
        Located {
            path: PathBuf::from(file).into(),
            line,
            column: 1,
        }
    }

    fn item(name: &str) -> ItemId {
        assert_ok!(ItemId::try_new(name))
    }

    fn tags(names: &[&str]) -> Option<Sourced<BTreeSet<Tag>>> {
        Some(Sourced {
            value: names.iter().map(|n| assert_ok!(Tag::try_new(*n))).collect(),
            at: at("x.toml", 1),
        })
    }

    fn priority(edit: PriorityEdit) -> Option<Sourced<PriorityEdit>> {
        Some(Sourced {
            value: edit,
            at: at("x.toml", 2),
        })
    }

    fn layer(kind: Kind, name: Option<&str>, file: &str, overrides: BTreeMap<ItemId, ItemOverrideLayer>) -> Layer {
        Layer {
            kind,
            name: name.map(|n| assert_ok!(ProfileName::try_new(n))),
            path: PathBuf::from(file),
            extends: vec![],
            position: None,
            content: (kind == Kind::Spec).then(|| Sourced {
                value: assert_ok!(DeclaredPath::parse("baseline.md")),
                at: at(file, 1),
            }),
            style: None,
            paper: None,
            select: SelectLayer {
                overrides,
                ..SelectLayer::default()
            },
            letter: None,
            letter_select: SelectLayer::default(),
        }
    }

    struct NoProfiles;
    impl ProfileSource for NoProfiles {
        fn find(&self, name: &ProfileName, at: &Located) -> Result<Layer, SpecError> {
            panic!("unexpected profile {name} asked for at {at}")
        }
    }

    fn resolved(overrides: BTreeMap<ItemId, ItemOverrideLayer>) -> super::ResolvedSpec {
        let spec = layer(Kind::Spec, None, "acme.toml", overrides);
        let spec = assert_ok!(SpecLayer::try_from_layer(spec));
        assert_ok!(resolve_with(spec, &NoProfiles))
    }

    fn printed(overrides: BTreeMap<ItemId, ItemOverrideLayer>) -> String {
        resolved(overrides).to_toml()
    }

    #[test]
    fn a_spec_with_no_overrides_prints_no_override_table() {
        let text = printed(BTreeMap::new());
        assert!(!text.contains("override"), "{text}");
    }

    #[test]
    fn overrides_print_with_their_tags_and_priority_sorted_by_id() {
        let text = printed(BTreeMap::from([
            (
                item("gateway"),
                ItemOverrideLayer {
                    tags: tags(&["platform", "ai"]),
                    priority: priority(PriorityEdit::Core),
                },
            ),
            (
                item("cache"),
                ItemOverrideLayer {
                    tags: None,
                    priority: priority(PriorityEdit::Extra),
                },
            ),
        ]));
        let cache = assert_some!(text.find("[select.override.cache]"), "{text}");
        let gateway = assert_some!(text.find("[select.override.gateway]"), "{text}");
        assert!(cache < gateway, "sorted by id: {text}");
        assert!(
            text.contains("tags = [\"ai\", \"platform\"]"),
            "tags are sorted: {text}"
        );
        assert!(text.contains("priority = \"core\""), "{text}");
        assert!(text.contains("priority = \"extra\""), "{text}");
    }

    #[test]
    fn an_explicit_clear_prints_and_is_not_dropped_as_empty() {
        let text = printed(BTreeMap::from([(
            item("gateway"),
            ItemOverrideLayer {
                tags: tags(&[]),
                priority: priority(PriorityEdit::None),
            },
        )]));
        assert!(text.contains("tags = []"), "{text}");
        assert!(text.contains("priority = \"none\""), "{text}");
    }

    #[test]
    fn a_field_nobody_wrote_is_not_printed() {
        let text = printed(BTreeMap::from([(
            item("gateway"),
            ItemOverrideLayer {
                tags: tags(&["ai"]),
                priority: None,
            },
        )]));
        assert!(text.contains("tags = [\"ai\"]"), "{text}");
        assert!(!text.contains("priority"), "{text}");
    }

    #[test]
    fn an_override_is_printed_under_select_so_the_spec_resolves_to_the_same_choices_again() {
        let spec = resolved(BTreeMap::from([(
            item("gateway"),
            ItemOverrideLayer {
                tags: tags(&[]),
                priority: priority(PriorityEdit::None),
            },
        )]));
        let text = spec.to_toml();
        let value: toml::Value = assert_ok!(text.parse());
        let table = &value["select"]["override"]["gateway"];
        assert_eq!(table["tags"].as_array().map(Vec::len), Some(0));
        assert_eq!(table["priority"].as_str(), Some("none"));
    }
}

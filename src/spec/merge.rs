//! ABOUTME: Folds the layers of a chain, least specific first, into one set of choices and the place each came from.
//! ABOUTME: Pure and total: the same layers give the same choices, whatever the files were called or where they sat.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::Hash;

use super::error::Located;
use super::layer::{Layer, SelectLayer};
use crate::model::options::Paper;
use crate::model::{GroupName, StyleName, Tag, VariantName};

/// The `[select]` choices after merging.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectChoices {
    pub groups: Option<BTreeSet<GroupName>>,
    pub tags: BTreeSet<Tag>,
    /// Most preferred first.
    pub prefer: Vec<VariantName>,
    /// The tags whose items rise within their own list, most promoted first (`Selection::promote`).
    pub promote: Vec<Tag>,
    pub pick: BTreeMap<GroupName, VariantName>,
    pub core_only: Option<bool>,
}

/// Every choice a chain makes. `None` means no file said anything, so the command's own default applies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choices {
    pub style: Option<StyleName>,
    pub paper: Option<Paper>,
    pub select: SelectChoices,
}

/// Names one value in [`Choices`], to ask where it came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChoiceKey {
    Groups,
    Group(GroupName),
    Tag(Tag),
    Prefer(VariantName),
    Promote(Tag),
    Pick(GroupName),
    Style,
    Paper,
    CoreOnly,
}

/// The choices and, for each, the file and place that decided it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Merged {
    pub choices: Choices,
    pub origins: BTreeMap<ChoiceKey, Located>,
}

/// `more` first, then what `less` lists that `more` does not, each name once.
fn specific_first<T: Clone + Eq + Hash>(less: &[T], more: impl Iterator<Item = T>) -> Vec<T> {
    let mut merged: Vec<T> = more.collect();
    let named: HashSet<T> = merged.iter().cloned().collect();
    merged.extend(less.iter().filter(|item| !named.contains(item)).cloned());
    merged
}

/// Merges the `[select]` tables of `selects`, ordered from the least to the most specific, with the place that
/// decided each value.
///
/// This is the part of merging that does not depend on what kind of document is built: a second table, such as
/// the letter's, is merged by the same function over the same chain.
#[must_use]
pub fn merge_select<'a>(
    selects: impl IntoIterator<Item = &'a SelectLayer>,
) -> (SelectChoices, BTreeMap<ChoiceKey, Located>) {
    let mut choices = SelectChoices::default();
    let mut origins = BTreeMap::new();
    for select in selects {
        if let Some(groups) = &select.groups {
            choices.groups = Some(groups.value.iter().map(|g| g.value.clone()).collect());
            origins.retain(|key, _| !matches!(key, ChoiceKey::Group(_)));
            origins.insert(ChoiceKey::Groups, groups.at.clone());
            for group in &groups.value {
                origins.insert(ChoiceKey::Group(group.value.clone()), group.at.clone());
            }
        }
        for tag in &select.tags {
            choices.tags.insert(tag.value.clone());
            origins.insert(ChoiceKey::Tag(tag.value.clone()), tag.at.clone());
        }
        choices.prefer = specific_first(&choices.prefer, select.prefer.iter().map(|v| v.value.clone()));
        for item in &select.prefer {
            origins.insert(ChoiceKey::Prefer(item.value.clone()), item.at.clone());
        }
        choices.promote = specific_first(&choices.promote, select.promote.iter().map(|t| t.value.clone()));
        for item in &select.promote {
            origins.insert(ChoiceKey::Promote(item.value.clone()), item.at.clone());
        }
        for (group, variant) in &select.pick {
            choices.pick.insert(group.clone(), variant.value.clone());
            origins.insert(ChoiceKey::Pick(group.clone()), variant.at.clone());
        }
        if let Some(flag) = &select.core_only {
            choices.core_only = Some(flag.value);
            origins.insert(ChoiceKey::CoreOnly, flag.at.clone());
        }
    }
    (choices, origins)
}

/// Merges `layers`, ordered from the least to the most specific.
///
/// Tags are a union. `prefer` and `promote` list the most specific file's names first, then those of the less
/// specific files that were not named already. A pick and a scalar are decided by the most specific file that
/// has one. A name listed twice in one file never reaches this function (loading refuses it), which is what
/// makes merging associative.
#[must_use]
pub fn merge<'a>(layers: impl IntoIterator<Item = &'a Layer>) -> Merged {
    let layers: Vec<&Layer> = layers.into_iter().collect();
    let (select, mut origins) = merge_select(layers.iter().map(|layer| &layer.select));
    let mut choices = Choices {
        select,
        ..Choices::default()
    };
    for layer in layers {
        if let Some(style) = &layer.style {
            choices.style = Some(style.value.clone());
            origins.insert(ChoiceKey::Style, style.at.clone());
        }
        if let Some(paper) = &layer.paper {
            choices.paper = Some(paper.value);
            origins.insert(ChoiceKey::Paper, paper.at.clone());
        }
    }
    Merged { choices, origins }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::layer::{Kind, Layer, SelectLayer, Sourced};
    use claims::assert_ok;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::path::PathBuf;

    fn at(file: &str, line: usize) -> Located {
        Located {
            path: PathBuf::from(file).into(),
            line,
            column: 1,
        }
    }

    fn tag(s: &str) -> Tag {
        assert_ok!(Tag::try_new(s))
    }
    fn variant(s: &str) -> VariantName {
        assert_ok!(VariantName::try_new(s))
    }
    fn group(s: &str) -> GroupName {
        assert_ok!(GroupName::try_new(s))
    }

    /// A layer with the given `[select]` and scalars; every value is "written" in `file` at its index.
    fn layer(
        file: &str,
        tags: &[&str],
        prefer: &[&str],
        pick: &[(&str, &str)],
        style: Option<&str>,
        paper: Option<Paper>,
    ) -> Layer {
        fn sourced<T>(file: &str, i: usize, value: T) -> Sourced<T> {
            Sourced {
                value,
                at: at(file, i + 1),
            }
        }
        Layer {
            kind: Kind::Profile,
            name: None,
            path: PathBuf::from(file),
            extends: vec![],
            position: None,
            content: None,
            style: style.map(|s| sourced(file, 0, assert_ok!(StyleName::try_new(s)))),
            paper: paper.map(|p| sourced(file, 0, p)),
            letter: None,
            letter_select: SelectLayer::default(),
            select: SelectLayer {
                groups: None,
                tags: tags.iter().enumerate().map(|(i, t)| sourced(file, i, tag(t))).collect(),
                prefer: prefer
                    .iter()
                    .enumerate()
                    .map(|(i, v)| sourced(file, i, variant(v)))
                    .collect(),
                promote: vec![],
                pick: pick
                    .iter()
                    .enumerate()
                    .map(|(i, (g, v))| (group(g), sourced(file, i, variant(v))))
                    .collect(),
                core_only: None,
            },
        }
    }

    fn worked_example() -> [Layer; 3] {
        [
            layer(
                "principal.toml",
                &["leadership"],
                &["technical", "short"],
                &[("gateway", "long")],
                Some("pragmatic"),
                None,
            ),
            layer(
                "ai-platform.toml",
                &["ai", "platform"],
                &["business"],
                &[("gateway", "short"), ("summary", "ai")],
                None,
                Some(Paper::A4),
            ),
            layer(
                "acme.toml",
                &["iceberg"],
                &["short"],
                &[("summary", "acme")],
                Some("mono"),
                None,
            ),
        ]
    }

    #[test]
    fn two_profiles_and_a_spec_merge_as_the_design_says() {
        let merged = merge(&worked_example());
        let c = &merged.choices;
        assert_eq!(
            c.select.tags.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["ai", "iceberg", "leadership", "platform"]
        );
        assert_eq!(
            c.select.prefer.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["short", "business", "technical"]
        );
        assert_eq!(c.select.pick.get(&group("gateway")), Some(&variant("short")));
        assert_eq!(c.select.pick.get(&group("summary")), Some(&variant("acme")));
        assert_eq!(c.style.as_ref().map(AsRef::as_ref), Some("mono"));
        assert_eq!(c.paper, Some(Paper::A4));
        assert_eq!(c.select.core_only, None);
    }

    #[test]
    fn each_value_remembers_the_file_that_decided_it() {
        let merged = merge(&worked_example());
        let file = |key: ChoiceKey| merged.origins[&key].path.display().to_string();
        assert_eq!(file(ChoiceKey::Pick(group("gateway"))), "ai-platform.toml");
        assert_eq!(file(ChoiceKey::Pick(group("summary"))), "acme.toml");
        assert_eq!(file(ChoiceKey::Style), "acme.toml");
        assert_eq!(file(ChoiceKey::Paper), "ai-platform.toml");
        assert_eq!(file(ChoiceKey::Prefer(variant("short"))), "acme.toml");
        assert_eq!(file(ChoiceKey::Prefer(variant("technical"))), "principal.toml");
        assert_eq!(file(ChoiceKey::Tag(tag("leadership"))), "principal.toml");
    }

    #[test]
    fn nothing_to_merge_is_nothing_chosen() {
        assert_eq!(merge(std::iter::empty()), Merged::default());
        let empty = layer("a.toml", &[], &[], &[], None, None);
        assert_eq!(merge([&empty]).choices, Choices::default());
    }

    #[test]
    fn a_later_layer_wins_a_contested_pick_and_the_order_matters() {
        let a = layer("a.toml", &[], &[], &[("gateway", "long")], None, None);
        let b = layer("b.toml", &[], &[], &[("gateway", "short")], None, None);
        assert_eq!(merge([&a, &b]).choices.select.pick[&group("gateway")], variant("short"));
        assert_eq!(merge([&b, &a]).choices.select.pick[&group("gateway")], variant("long"));
    }

    #[test]
    fn promote_merges_like_prefer() {
        let mut a = layer("a.toml", &[], &[], &[], None, None);
        let mut b = layer("b.toml", &[], &[], &[], None, None);
        let sourced = |t: &str| Sourced {
            value: tag(t),
            at: at("x.toml", 1),
        };
        a.select.promote = vec![sourced("people"), sourced("scale")];
        b.select.promote = vec![sourced("leadership"), sourced("people")];
        let merged = merge([&a, &b]);
        assert_eq!(
            merged
                .choices
                .select
                .promote
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            ["leadership", "people", "scale"]
        );
    }

    #[test]
    fn core_only_is_a_scalar_the_most_specific_file_decides() {
        let mut a = layer("a.toml", &[], &[], &[], None, None);
        let mut b = layer("b.toml", &[], &[], &[], None, None);
        a.select.core_only = Some(Sourced {
            value: true,
            at: at("a.toml", 1),
        });
        assert_eq!(merge([&a, &b]).choices.select.core_only, Some(true));
        b.select.core_only = Some(Sourced {
            value: false,
            at: at("b.toml", 1),
        });
        assert_eq!(merge([&a, &b]).choices.select.core_only, Some(false));
    }

    // ---- properties over generated layers ------------------------------------------------------------------

    const NAMES: [&str; 5] = ["ai", "llm", "platform", "short", "long"];

    fn unique_names() -> impl Strategy<Value = Vec<&'static str>> {
        proptest::sample::subsequence(NAMES.to_vec(), 0..=NAMES.len()).prop_shuffle()
    }

    fn arbitrary_layer() -> impl Strategy<Value = Layer> {
        (
            (unique_names(), unique_names(), unique_names()),
            proptest::collection::btree_map(0..3usize, 0..NAMES.len(), 0..3),
            proptest::option::of(prop_oneof![Just("pragmatic"), Just("mono")]),
            proptest::option::of(prop_oneof![Just(Paper::Letter), Just(Paper::A4)]),
            proptest::option::of(any::<bool>()),
        )
            .prop_map(|((tags, prefer, promote), picks, style, paper, core_only)| {
                let groups = ["gateway", "summary", "scale"];
                let pick: Vec<(&str, &str)> = picks.iter().map(|(g, v)| (groups[*g], NAMES[*v])).collect();
                let mut l = layer("f.toml", &tags, &prefer, &pick, style, paper);
                l.select.promote = promote
                    .iter()
                    .enumerate()
                    .map(|(i, t)| Sourced {
                        value: tag(t),
                        at: at("f.toml", i + 1),
                    })
                    .collect();
                l.select.core_only = core_only.map(|value| Sourced {
                    value,
                    at: at("f.toml", 1),
                });
                l
            })
    }

    #[test]
    fn explicit_group_replacement_is_associative_including_empty() {
        let s = |name| Sourced {
            value: group(name),
            at: at("a.toml", 1),
        };
        let names = [None, Some(Vec::new()), Some(vec![s("a")]), Some(vec![s("b")])];
        for a in &names {
            for b in &names {
                for c in &names {
                    let layer = |groups: &Option<Vec<Sourced<GroupName>>>| SelectLayer {
                        groups: groups.clone().map(|value| Sourced {
                            value,
                            at: at("a.toml", 1),
                        }),
                        ..SelectLayer::default()
                    };
                    let (a, b, c) = (layer(a), layer(b), layer(c));
                    let direct = merge_select([&a, &b, &c]).0.groups;
                    let prefix = if b.groups.is_some() { b } else { a };
                    assert_eq!(merge_select([&prefix, &c]).0.groups, direct);
                }
            }
        }
    }

    proptest! {
        #[test]
        fn merging_is_deterministic(layers in proptest::collection::vec(arbitrary_layer(), 0..5)) {
            prop_assert_eq!(merge(&layers), merge(&layers));
        }

        #[test]
        fn merging_one_layer_after_nothing_loses_nothing(l in arbitrary_layer()) {
            let alone = merge([&l]);
            let after_empty = merge([&layer("e.toml", &[], &[], &[], None, None), &l]);
            prop_assert_eq!(alone.choices, after_empty.choices);
        }

        #[test]
        fn merging_the_same_layer_twice_changes_nothing(l in arbitrary_layer()) {
            prop_assert_eq!(merge([&l]).choices, merge([&l, &l]).choices);
        }

        #[test]
        fn grouping_does_not_change_the_result(a in arbitrary_layer(), b in arbitrary_layer(), c in arbitrary_layer()) {
            // Associativity: merging (a, b) first and then c equals merging a with (b, c). The merged choices are
            // fed back as a layer so the same function folds them.
            let as_layer = |m: &Merged| {
                let mut l = layer("m.toml", &[], &[], &[], None, None);
                fn s<T>(value: T) -> Sourced<T> {
                    Sourced { value, at: at("m.toml", 1) }
                }
                l.select.tags = m.choices.select.tags.iter().cloned().map(s).collect();
                l.select.prefer = m.choices.select.prefer.iter().cloned().map(s).collect();
                l.select.promote = m.choices.select.promote.iter().cloned().map(s).collect();
                l.select.core_only = m.choices.select.core_only.map(s);
                l.select.pick = m.choices.select.pick.iter().map(|(g, v)| (g.clone(), s(v.clone()))).collect();
                l.style = m.choices.style.clone().map(s);
                l.paper = m.choices.paper.map(s);
                l
            };
            let left = merge([&as_layer(&merge([&a, &b])), &c]);
            let right = merge([&a, &as_layer(&merge([&b, &c]))]);
            prop_assert_eq!(left.choices, right.choices);
        }

        #[test]
        fn the_most_specific_non_empty_prefer_list_comes_first(a in arbitrary_layer(), b in arbitrary_layer()) {
            let merged = merge([&a, &b]);
            let wanted: Vec<&str> = b.select.prefer.iter().map(|v| v.value.as_ref()).collect();
            let got: Vec<&str> = merged.choices.select.prefer.iter().map(AsRef::as_ref).take(wanted.len()).collect();
            prop_assert_eq!(got, wanted);
        }

        #[test]
        fn the_last_layer_decides_every_pick_it_makes(a in arbitrary_layer(), b in arbitrary_layer()) {
            let merged = merge([&a, &b]);
            for (group, variant) in &b.select.pick {
                prop_assert_eq!(&merged.choices.select.pick[group], &variant.value);
            }
        }

        #[test]
        fn tags_do_not_depend_on_the_order_of_layers(a in arbitrary_layer(), b in arbitrary_layer()) {
            prop_assert_eq!(merge([&a, &b]).choices.select.tags, merge([&b, &a]).choices.select.tags);
        }
    }

    #[test]
    fn paper_and_style_are_decided_by_the_most_specific_layer_that_has_one() {
        let a = layer("a.toml", &[], &[], &[], Some("one"), Some(Paper::A4));
        let b = layer("b.toml", &[], &[], &[], Some("two"), Some(Paper::Letter));
        let c = layer("c.toml", &[], &[], &[], None, None);
        let merged = merge([&a, &b, &c]);
        assert_eq!(merged.choices.paper, Some(Paper::Letter));
        assert_eq!(merged.choices.style.as_ref().map(AsRef::as_ref), Some("two"));
        assert_eq!(merged.origins[&ChoiceKey::Paper].path.display().to_string(), "b.toml");
        assert_eq!(merged.origins[&ChoiceKey::Style].path.display().to_string(), "b.toml");
    }

    #[test]
    fn a_name_given_by_two_files_remembers_the_more_specific_one() {
        let a = layer("a.toml", &["ai"], &[], &[], None, None);
        let b = layer("b.toml", &["ai"], &[], &[], None, None);
        let merged = merge([&a, &b]);
        assert_eq!(
            merged.origins[&ChoiceKey::Tag(tag("ai"))].path.display().to_string(),
            "b.toml"
        );
    }

    #[test]
    fn promote_and_core_only_remember_their_files() {
        let mut a = layer("a.toml", &[], &[], &[], None, None);
        let mut b = layer("b.toml", &[], &[], &[], None, None);
        let sourced = |t: &str, file: &str| Sourced {
            value: tag(t),
            at: at(file, 1),
        };
        a.select.promote = vec![sourced("people", "a.toml")];
        b.select.promote = vec![sourced("people", "b.toml"), sourced("scale", "b.toml")];
        a.select.core_only = Some(Sourced {
            value: true,
            at: at("a.toml", 2),
        });
        let merged = merge([&a, &b]);
        let file = |key: ChoiceKey| merged.origins[&key].path.display().to_string();
        assert_eq!(file(ChoiceKey::Promote(tag("people"))), "b.toml");
        assert_eq!(file(ChoiceKey::Promote(tag("scale"))), "b.toml");
        assert_eq!(file(ChoiceKey::CoreOnly), "a.toml");
    }

    #[test]
    fn the_origins_are_listed_in_key_order() {
        let merged_spec = crate::spec::merge::merge(&worked_example());
        let keys: Vec<&ChoiceKey> = merged_spec.origins.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn eight_profiles_of_thousands_of_distinct_names_merge_in_time_that_grows_with_their_size() {
        // A hostile pack within the file limit: 8 files of 6,000 names each, all different. A merge that searched the
        // growing list for every name took 20 seconds in a debug build; this one takes a few milliseconds.
        let layers: Vec<Layer> = (0..8)
            .map(|file| {
                let mut l = layer(&format!("f{file}.toml"), &[], &[], &[], None, None);
                l.select.prefer = (0..6_000)
                    .map(|i| Sourced {
                        value: variant(&format!("v{}", number_name(file * 6_000 + i))),
                        at: at("f.toml", i + 1),
                    })
                    .collect();
                l
            })
            .collect();
        let started = std::time::Instant::now();
        let merged = merge(&layers);
        assert_eq!(merged.choices.select.prefer.len(), 48_000);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    /// 0 -> "a", 1 -> "b", ..., 26 -> "ba": letters only, so it is a valid variant name after a `v` prefix.
    fn number_name(mut n: usize) -> String {
        let mut out = String::new();
        loop {
            out.insert(0, char::from(b'a' + u8::try_from(n % 26).unwrap_or(0)));
            n /= 26;
            if n == 0 {
                return out;
            }
        }
    }

    proptest! {
        #[test]
        fn the_choices_do_not_depend_on_what_the_files_were_called(layers in proptest::collection::vec(arbitrary_layer(), 0..5)) {
            let mut moved = layers.clone();
            for (i, l) in moved.iter_mut().enumerate() {
                l.path = PathBuf::from(format!("elsewhere/x{i}.toml"));
            }
            prop_assert_eq!(merge(&layers).choices, merge(&moved).choices);
        }

        #[test]
        fn an_empty_layer_after_any_layer_changes_nothing(l in arbitrary_layer()) {
            let empty = layer("e.toml", &[], &[], &[], None, None);
            prop_assert_eq!(merge([&l]).choices, merge([&l, &empty]).choices);
        }
    }
}

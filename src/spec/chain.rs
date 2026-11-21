//! ABOUTME: Finds the profiles a spec extends and puts the whole chain in one order, least specific first.
//! ABOUTME: Depth first, each profile once; a cycle, a missing profile, a contradiction, or a limit is a typed error.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};
use std::path::{Path, PathBuf};

use snafu::ResultExt;

use super::error::{
    CycleSnafu, ExtendsOrderConflictSnafu, Located, MAX_DEPTH, MAX_PROFILES, MissingProfileSnafu, ProfileNameCaseSnafu,
    ProfileOutsideRootSnafu, ReadSnafu, SpecError, TooDeepSnafu, TooManyProfilesSnafu, shown,
};
use super::layer::{Kind, Layer, Sourced, SpecLayer};
use super::names::ProfileName;

/// Where profiles come from. A directory in the product; a map in the tests.
pub trait ProfileSource {
    /// The profile called `name`, which an `extends` entry at `at` asked for.
    ///
    /// # Errors
    /// [`SpecError::MissingProfile`] when there is none, or any error of loading it.
    fn find(&self, name: &ProfileName, at: &Located) -> Result<Layer, SpecError>;
}

/// A directory of `<name>.toml` files.
#[derive(Debug, Clone)]
pub struct DirSource {
    root: PathBuf,
}

impl DirSource {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The directory profiles are looked for when none is given: `profiles` beside the spec's own directory.
    #[must_use]
    pub fn default_for(spec: &Path) -> PathBuf {
        let dir = spec
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        dir.join("..").join("profiles")
    }

    /// The names of the profiles in the directory, for an error message.
    fn available(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let stem = path.file_stem()?.to_str()?.to_string();
                (path.extension()? == "toml" && ProfileName::try_new(stem.as_str()).is_ok()).then_some(stem)
            })
            .collect();
        names.sort();
        names
    }
}

impl ProfileSource for DirSource {
    fn find(&self, name: &ProfileName, at: &Located) -> Result<Layer, SpecError> {
        let path = self.root.join(format!("{name}.toml"));
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return MissingProfileSnafu {
                    at: at.clone(),
                    name: name.to_string(),
                    searched: super::error::searched_path(&self.root),
                    available: self.available(),
                }
                .fail();
            }
            Err(source) => return Err(source).context(ReadSnafu { path }),
        }
        // The name cannot hold a separator, so only a link can lead out of the directory. A check that cannot be
        // made (a dangling or looping link, an unreadable directory) refuses the profile, so it never fails open.
        let root = std::fs::canonicalize(&self.root).context(ReadSnafu { path: &self.root })?;
        let file = std::fs::canonicalize(&path).context(ReadSnafu { path: &path })?;
        if !file.starts_with(&root) {
            return ProfileOutsideRootSnafu {
                at: at.clone(),
                name: name.to_string(),
                path,
            }
            .fail();
        }
        // On a file system that ignores case, `Principal.toml` answers to `principal`; on Linux it does not, so the
        // same tree would resolve on one machine and not on another. The name on disk must be the name asked for.
        let on_disk = file.file_name().map(|n| n.to_string_lossy().into_owned());
        if on_disk.as_deref() != Some(&format!("{name}.toml")) {
            return ProfileNameCaseSnafu {
                at: at.clone(),
                name: name.to_string(),
                found: shown(&on_disk.unwrap_or_default()),
            }
            .fail();
        }
        // The file that was checked is the file that is read, so a link swapped in between cannot be followed.
        Layer::load_shown(Kind::Profile, &file, &path)
    }
}

struct Walk<'a> {
    source: &'a dyn ProfileSource,
    stack: Vec<ProfileName>,
    done: BTreeMap<ProfileName, usize>,
    layers: Vec<Layer>,
}

impl Walk<'_> {
    fn visit(&mut self, extends: &[Sourced<ProfileName>], depth: usize) -> Result<(), SpecError> {
        for entry in extends {
            let name = &entry.value;
            if let Some(start) = self.stack.iter().position(|n| n == name) {
                let mut path: Vec<String> = self.stack[start..].iter().map(ToString::to_string).collect();
                path.push(name.to_string());
                return CycleSnafu {
                    at: entry.at.clone(),
                    path,
                }
                .fail();
            }
            if self.done.contains_key(name) {
                continue;
            }
            if depth >= MAX_DEPTH {
                return TooDeepSnafu { at: entry.at.clone() }.fail();
            }
            // Counts the profiles being read up the stack as well as those finished, so the limit is the limit.
            if self.layers.len() + self.stack.len() >= MAX_PROFILES {
                return TooManyProfilesSnafu { at: entry.at.clone() }.fail();
            }
            let mut layer = self.source.find(name, &entry.at)?;
            layer.name = Some(name.clone());
            self.stack.push(name.clone());
            self.visit(&layer.extends.clone(), depth + 1)?;
            self.stack.pop();
            self.done.insert(name.clone(), self.layers.len());
            self.layers.push(layer);
        }
        Ok(())
    }
}

/// Orders the chain of `spec`: every profile it needs, once, least specific first, then the spec.
///
/// Two kinds of fact fix the order. A file is applied after every profile it extends, and an `extends` list runs
/// from the least to the most specific, so an entry is applied before the entry listed after it. When some
/// order satisfies all of them, the chain uses it; where the facts leave two profiles unordered, the one found
/// first in a depth-first walk comes first. When none does, a file lists two profiles the other files order the
/// other way, which is [`SpecError::ExtendsOrderConflict`]: for example `extends = ["b", "base"]` where `b`
/// extends `base`.
///
/// # Errors
/// A cycle, a missing profile, a profile outside its directory, a chain over the limits, or a contradiction.
pub(super) fn order(spec: SpecLayer, source: &dyn ProfileSource) -> Result<Vec<Layer>, SpecError> {
    let spec = spec.into_layer();
    let mut walk = Walk {
        source,
        stack: Vec::new(),
        done: BTreeMap::new(),
        layers: Vec::new(),
    };
    walk.visit(&spec.extends, 0)?;
    let mut layers = walk.layers;
    layers.push(spec);
    let constraints = Constraints::new(&layers, &walk.done);
    match constraints.sorted() {
        Some(sequence) => {
            let mut slots: Vec<Option<Layer>> = layers.into_iter().map(Some).collect();
            Ok(sequence.into_iter().filter_map(|i| slots[i].take()).collect())
        }
        None => Err(constraints.conflict(&layers, &walk.done)),
    }
}

/// What must come before what, over the layers' indexes; the last layer is the spec.
struct Constraints {
    /// `before[i]` holds every `j` that must be applied before `i`.
    before: Vec<Vec<usize>>,
    /// The same facts the other way round.
    after: Vec<Vec<usize>>,
}

impl Constraints {
    fn new(layers: &[Layer], discovered: &BTreeMap<ProfileName, usize>) -> Self {
        let mut before = vec![Vec::new(); layers.len()];
        let mut after = vec![Vec::new(); layers.len()];
        let mut add = |first: usize, then: usize| {
            if !before[then].contains(&first) {
                before[then].push(first);
                after[first].push(then);
            }
        };
        for (file, layer) in layers.iter().enumerate() {
            let entries: Vec<usize> = layer.extends.iter().map(|e| discovered[&e.value]).collect();
            for entry in &entries {
                add(*entry, file);
            }
            for pair in entries.windows(2) {
                add(pair[0], pair[1]);
            }
        }
        Self { before, after }
    }

    /// A deterministic order that satisfies every fact, or `None`. Among the layers that may come next, the one
    /// the walk found first does.
    fn sorted(&self) -> Option<Vec<usize>> {
        let mut missing: Vec<usize> = self.before.iter().map(Vec::len).collect();
        let mut ready: BinaryHeap<Reverse<usize>> = missing
            .iter()
            .enumerate()
            .filter(|(_, count)| **count == 0)
            .map(|(i, _)| Reverse(i))
            .collect();
        let mut sequence = Vec::with_capacity(missing.len());
        while let Some(Reverse(next)) = ready.pop() {
            sequence.push(next);
            for &later in &self.after[next] {
                missing[later] -= 1;
                if missing[later] == 0 {
                    ready.push(Reverse(later));
                }
            }
        }
        (sequence.len() == missing.len()).then_some(sequence)
    }

    /// The facts that make `to` come after `from`, as the layers along the way, if there are such facts.
    fn path(&self, from: usize, to: usize) -> Option<Vec<usize>> {
        let mut parent: BTreeMap<usize, usize> = BTreeMap::new();
        let mut queue = VecDeque::from([from]);
        while let Some(at) = queue.pop_front() {
            if at == to {
                let mut path = vec![to];
                while let Some(&p) = parent.get(path.last().unwrap_or(&to)) {
                    path.push(p);
                }
                path.reverse();
                return Some(path);
            }
            for &next in &self.after[at] {
                if next != from && !parent.contains_key(&next) {
                    parent.insert(next, at);
                    queue.push_back(next);
                }
            }
        }
        None
    }

    /// The first listed pair, file by file and entry by entry, that the other facts order the other way.
    fn conflict(&self, layers: &[Layer], discovered: &BTreeMap<ProfileName, usize>) -> SpecError {
        let name_of = |i: usize| {
            layers[i]
                .name
                .as_ref()
                .map_or_else(|| "the spec".to_string(), ToString::to_string)
        };
        // The spec is the file the user edits, so its listing is the one reported when it is part of the conflict.
        for layer in layers.iter().rev() {
            for pair in layer.extends.windows(2) {
                let (first, then) = (discovered[&pair[0].value], discovered[&pair[1].value]);
                if let Some(path) = self.path(then, first) {
                    return ExtendsOrderConflictSnafu {
                        at: pair[1].at.clone(),
                        listed_first: pair[0].value.to_string(),
                        listed_after: pair[1].value.to_string(),
                        must_come_first: path.into_iter().map(name_of).collect::<Vec<_>>(),
                    }
                    .build();
                }
            }
        }
        // The walk refused every cycle of `extends` alone, so a cycle here runs through a listed pair.
        unreachable!("facts that cannot be ordered contain a listed pair that closes a cycle")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::read::Source;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::collections::HashMap;

    /// Profiles held as text, so a chain can be built without files.
    struct MapSource(HashMap<String, String>);

    impl MapSource {
        fn of(profiles: &[(&str, &str)]) -> Self {
            Self(
                profiles
                    .iter()
                    .map(|(n, t)| ((*n).to_string(), (*t).to_string()))
                    .collect(),
            )
        }
    }

    impl ProfileSource for MapSource {
        fn find(&self, name: &ProfileName, at: &Located) -> Result<Layer, SpecError> {
            let Some(text) = self.0.get(name.as_ref() as &str) else {
                return Err(SpecError::MissingProfile {
                    at: at.clone(),
                    name: name.to_string(),
                    searched: PathBuf::from("profiles"),
                    available: vec![],
                });
            };
            Layer::from_source(
                Kind::Profile,
                &Source::new(PathBuf::from(format!("profiles/{name}.toml")), text.clone()),
            )
        }
    }

    fn spec(extends: &str) -> SpecLayer {
        let text = format!("extends = {extends}\n[build]\ncontent = \"a.md\"\n");
        let layer = assert_ok!(Layer::from_source(
            Kind::Spec,
            &Source::new(PathBuf::from("acme.toml"), text)
        ));
        assert_ok!(SpecLayer::try_from_layer(layer))
    }

    fn names(layers: &[Layer]) -> Vec<String> {
        layers
            .iter()
            .map(|l| l.name.as_ref().map_or("spec".to_string(), ToString::to_string))
            .collect()
    }

    #[test]
    fn a_spec_with_no_profiles_is_a_chain_of_one() {
        let layers = assert_ok!(order(spec("[]"), &MapSource::of(&[])));
        assert_eq!(names(&layers), ["spec"]);
    }

    #[test]
    fn profiles_come_in_listed_order_before_the_spec() {
        let s = MapSource::of(&[("principal", ""), ("ai-platform", "")]);
        let layers = assert_ok!(order(spec("[\"principal\", \"ai-platform\"]"), &s));
        assert_eq!(names(&layers), ["principal", "ai-platform", "spec"]);
    }

    #[test]
    fn a_profile_comes_after_the_profiles_it_extends() {
        let s = MapSource::of(&[("c", "extends = [\"b\"]"), ("b", "extends = [\"a\"]"), ("a", "")]);
        let layers = assert_ok!(order(spec("[\"c\"]"), &s));
        assert_eq!(names(&layers), ["a", "b", "c", "spec"]);
    }

    #[test]
    fn a_diamond_applies_the_shared_base_once_and_first() {
        let s = MapSource::of(&[
            ("left", "extends = [\"base\"]"),
            ("right", "extends = [\"base\"]"),
            ("base", ""),
        ]);
        let layers = assert_ok!(order(spec("[\"left\", \"right\"]"), &s));
        assert_eq!(names(&layers), ["base", "left", "right", "spec"]);
    }

    #[test]
    fn a_cycle_is_named_with_its_path() {
        let s = MapSource::of(&[
            ("a", "extends = [\"b\"]"),
            ("b", "extends = [\"c\"]"),
            ("c", "extends = [\"a\"]"),
        ]);
        match assert_err!(order(spec("[\"a\"]"), &s)) {
            SpecError::Cycle { path, .. } => assert_eq!(path, ["a", "b", "c", "a"]),
            other => panic!("{other:?}"),
        }
        let alone = MapSource::of(&[("a", "extends = [\"a\"]")]);
        match assert_err!(order(spec("[\"a\"]"), &alone)) {
            SpecError::Cycle { path, .. } => assert_eq!(path, ["a", "a"]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_diamond_is_not_mistaken_for_a_cycle() {
        let s = MapSource::of(&[
            ("a", "extends = [\"b\", \"c\"]"),
            ("b", "extends = [\"d\"]"),
            ("c", "extends = [\"d\"]"),
            ("d", ""),
        ]);
        assert_ok!(order(spec("[\"a\"]"), &s));
    }

    #[test]
    fn a_missing_profile_is_located_at_the_entry_that_asked_for_it() {
        match assert_err!(order(spec("[\"nope\"]"), &MapSource::of(&[]))) {
            SpecError::MissingProfile { at, name, .. } => assert_eq!((at.line, name.as_str()), (1, "nope")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_contradictory_order_is_refused_with_the_reason_and_the_consistent_one_is_accepted() {
        let s = MapSource::of(&[("b", "extends = [\"base\"]"), ("base", "")]);
        match assert_err!(order(spec("[\"b\", \"base\"]"), &s)) {
            SpecError::ExtendsOrderConflict {
                listed_first,
                listed_after,
                at,
                must_come_first,
            } => {
                assert_eq!(
                    (listed_first.as_str(), listed_after.as_str(), at.path.to_str()),
                    ("b", "base", Some("acme.toml"))
                );
                assert_eq!(must_come_first, ["base", "b"]);
            }
            other => panic!("{other:?}"),
        }
        let layers = assert_ok!(order(spec("[\"base\", \"b\"]"), &s));
        assert_eq!(names(&layers), ["base", "b", "spec"]);
    }

    #[test]
    fn a_contradiction_inside_a_profile_is_refused_too() {
        let s = MapSource::of(&[
            ("top", "extends = [\"b\", \"base\"]"),
            ("b", "extends = [\"base\"]"),
            ("base", ""),
        ]);
        match assert_err!(order(spec("[\"top\"]"), &s)) {
            SpecError::ExtendsOrderConflict { at, .. } => assert!(at.path.ends_with("top.toml"), "{at}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn two_files_that_order_a_pair_oppositely_contradict_each_other_without_either_extending_the_other() {
        // The spec lists x before y; the profile y2 lists y before x. No profile extends another.
        let s = MapSource::of(&[("x", ""), ("y", ""), ("both", "extends = [\"y\", \"x\"]")]);
        match assert_err!(order(spec("[\"x\", \"y\", \"both\"]"), &s)) {
            SpecError::ExtendsOrderConflict {
                listed_first,
                listed_after,
                ..
            } => {
                assert_eq!((listed_first.as_str(), listed_after.as_str()), ("x", "y"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_listing_that_only_looks_contradictory_in_walk_order_is_accepted() {
        // `ai-platform` lists technical before principal; the spec has already walked into principal. Some order
        // satisfies every fact (technical, principal, ai-platform), so this is not a contradiction.
        let s = MapSource::of(&[
            ("principal", ""),
            ("technical", ""),
            ("ai-platform", "extends = [\"technical\", \"principal\"]"),
        ]);
        let layers = assert_ok!(order(spec("[\"principal\", \"ai-platform\"]"), &s));
        assert_eq!(names(&layers), ["technical", "principal", "ai-platform", "spec"]);
    }

    #[test]
    fn profiles_the_facts_leave_unordered_keep_the_order_the_walk_found_them_in() {
        let s = MapSource::of(&[
            ("a", "extends = [\"x\"]"),
            ("b", "extends = [\"y\"]"),
            ("x", ""),
            ("y", ""),
        ]);
        let layers = assert_ok!(order(spec("[\"a\", \"b\"]"), &s));
        assert_eq!(names(&layers), ["x", "a", "y", "b", "spec"]);
    }

    fn deep(levels: usize) -> MapSource {
        let mut map = HashMap::new();
        for i in 0..levels {
            map.insert(format!("p{i}"), format!("extends = [\"p{}\"]", i + 1));
        }
        map.insert(format!("p{levels}"), String::new());
        MapSource(map)
    }

    #[test]
    fn eight_levels_are_allowed_and_nine_are_not() {
        assert_ok!(order(spec("[\"p0\"]"), &deep(7)));
        assert!(matches!(
            assert_err!(order(spec("[\"p0\"]"), &deep(8))),
            SpecError::TooDeep { .. }
        ));
    }

    #[test]
    fn more_than_thirty_two_profiles_are_refused() {
        let all: Vec<String> = (0..33).map(|i| format!("\"p{i}\"")).collect();
        let list = format!("[{}]", all.join(", "));
        let map: HashMap<String, String> = (0..33).map(|i| (format!("p{i}"), String::new())).collect();
        assert!(matches!(
            assert_err!(order(spec(&list), &MapSource(map))),
            SpecError::TooManyProfiles { .. }
        ));
    }

    #[test]
    fn the_default_profiles_directory_is_beside_the_specs_directory() {
        assert_eq!(
            DirSource::default_for(Path::new("positions/acme.toml")),
            Path::new("positions/../profiles")
        );
        assert_eq!(
            DirSource::default_for(Path::new("acme.toml")),
            Path::new("./../profiles")
        );
        assert_eq!(
            DirSource::default_for(Path::new("/w/positions/acme.toml")),
            Path::new("/w/positions/../profiles")
        );
    }

    // ---- against an independent reference ------------------------------------------------------------------

    /// A random graph: `n` profiles `p0..`, each extending a list of others, and a spec extending a list.
    fn graph() -> impl Strategy<Value = (usize, Vec<Vec<usize>>, Vec<usize>)> {
        (1usize..=5).prop_flat_map(|n| {
            let list = || proptest::sample::subsequence((0..n).collect::<Vec<_>>(), 0..=n).prop_shuffle();
            (Just(n), proptest::collection::vec(list(), n), list())
        })
    }

    fn quoted(list: &[usize]) -> String {
        format!(
            "[{}]",
            list.iter().map(|i| format!("\"p{i}\"")).collect::<Vec<_>>().join(", ")
        )
    }

    /// True when `from` can reach `to` through the extends lists (a path of one or more steps).
    fn reaches(extends: &[Vec<usize>], from: usize, to: usize) -> bool {
        let mut seen = vec![false; extends.len()];
        let mut stack: Vec<usize> = extends[from].clone();
        while let Some(at) = stack.pop() {
            if at == to {
                return true;
            }
            if !std::mem::replace(&mut seen[at], true) {
                stack.extend(extends[at].iter().copied());
            }
        }
        false
    }

    /// Every ordering of the reachable profiles that satisfies the facts, found by trying all of them.
    fn some_order_exists(extends: &[Vec<usize>], spec: &[usize], reachable: &[usize]) -> bool {
        fn permute(items: &mut Vec<usize>, from: usize, ok: &dyn Fn(&[usize]) -> bool) -> bool {
            if from == items.len() {
                return ok(items);
            }
            for i in from..items.len() {
                items.swap(from, i);
                if permute(items, from + 1, ok) {
                    return true;
                }
                items.swap(from, i);
            }
            false
        }
        let satisfies = |order: &[usize]| {
            let at = |p: usize| order.iter().position(|x| *x == p);
            // A profile the spec does not reach is not part of the chain, so what it lists does not count.
            let files: Vec<(Option<usize>, &[usize])> = std::iter::once((None, spec))
                .chain(
                    extends
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| reachable.contains(i))
                        .map(|(i, l)| (Some(i), l.as_slice())),
                )
                .collect();
            files.iter().all(|(file, list)| {
                let applied = |p: usize| at(p);
                let listed_in_order = list.windows(2).all(|w| match (applied(w[0]), applied(w[1])) {
                    (Some(a), Some(b)) => a < b,
                    _ => true,
                });
                let after_what_it_extends = list.iter().all(|e| match (file, applied(*e)) {
                    (Some(f), Some(a)) => applied(*f).is_none_or(|fa| a < fa),
                    _ => true,
                });
                listed_in_order && after_what_it_extends
            })
        };
        let mut items = reachable.to_vec();
        permute(&mut items, 0, &satisfies)
    }

    proptest! {
        #[test]
        fn the_chain_agrees_with_an_exhaustive_search_of_all_orders((n, extends, spec_list) in graph()) {
            let profiles: Vec<(String, String)> = (0..n)
                .map(|i| (format!("p{i}"), format!("extends = {}", quoted(&extends[i]))))
                .collect();
            let refs: Vec<(&str, &str)> = profiles.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let source = MapSource::of(&refs);
            let result = order(spec(&quoted(&spec_list)), &source);

            let reachable: Vec<usize> = (0..n)
                .filter(|p| spec_list.contains(p) || spec_list.iter().any(|s| reaches(&extends, *s, *p)))
                .collect();
            let has_cycle = reachable.iter().any(|p| reaches(&extends, *p, *p));
            match result {
                Err(SpecError::Cycle { .. }) => prop_assert!(has_cycle, "a cycle was reported without one"),
                Err(SpecError::ExtendsOrderConflict { .. }) => {
                    prop_assert!(!has_cycle);
                    prop_assert!(!some_order_exists(&extends, &spec_list, &reachable), "a valid order existed");
                }
                Err(other) => prop_assert!(false, "{other:?}"),
                Ok(layers) => {
                    prop_assert!(!has_cycle, "a cycle was accepted");
                    let applied = names(&layers);
                    prop_assert_eq!(applied.last().map(String::as_str), Some("spec"));
                    let mut want: Vec<String> = reachable.iter().map(|p| format!("p{p}")).collect();
                    let mut got: Vec<String> = applied[..applied.len() - 1].to_vec();
                    want.sort();
                    got.sort();
                    prop_assert_eq!(got, want, "each reachable profile exactly once");
                    let order: Vec<usize> = applied[..applied.len() - 1].iter().map(|s| s[1..].parse().unwrap_or(0)).collect();
                    prop_assert!(some_order_exists(&extends, &spec_list, &reachable));
                    let at = |p: usize| order.iter().position(|x| *x == p);
                    for (file, list) in std::iter::once((None, &spec_list)).chain(extends.iter().enumerate().map(|(i, l)| (Some(i), l))) {
                        if file.is_some_and(|f| at(f).is_none()) { continue; }
                        for w in list.windows(2) {
                            prop_assert!(at(w[0]) < at(w[1]), "listed order broken: {list:?}");
                        }
                        for e in list {
                            if let Some(f) = file { prop_assert!(at(*e) < at(f), "p{f} applied before p{e} which it extends"); }
                        }
                    }
                }
            }
        }

        #[test]
        fn resolving_twice_gives_the_same_chain((n, extends, spec_list) in graph()) {
            let profiles: Vec<(String, String)> = (0..n)
                .map(|i| (format!("p{i}"), format!("extends = {}", quoted(&extends[i]))))
                .collect();
            let refs: Vec<(&str, &str)> = profiles.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let source = MapSource::of(&refs);
            let first = order(spec(&quoted(&spec_list)), &source).map(|l| names(&l)).map_err(|e| e.to_string());
            let second = order(spec(&quoted(&spec_list)), &source).map(|l| names(&l)).map_err(|e| e.to_string());
            prop_assert_eq!(first, second);
        }
    }

    #[test]
    fn a_cycle_path_starts_where_the_cycle_starts_and_is_located_at_the_entry_that_closes_it() {
        let s = MapSource::of(&[
            ("x", "extends = [\"a\"]"),
            ("a", "extends = [\"b\"]"),
            ("b", "\n\nextends = [\"a\"]"),
        ]);
        match assert_err!(order(spec("[\"x\"]"), &s)) {
            SpecError::Cycle { at, path } => {
                assert_eq!(path, ["a", "b", "a"]);
                assert_eq!((at.line, at.path.to_str()), (3, Some("profiles/b.toml")));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn exactly_thirty_two_profiles_are_allowed_however_they_nest() {
        let all: Vec<String> = (0..32).map(|i| format!("\"p{i}\"")).collect();
        let list = format!("[{}]", all.join(", "));
        let map: HashMap<String, String> = (0..32).map(|i| (format!("p{i}"), String::new())).collect();
        assert_eq!(assert_ok!(order(spec(&list), &MapSource(map))).len(), 33);
    }

    #[test]
    fn the_profile_limit_counts_the_profiles_being_read_as_well_as_the_finished_ones() {
        // 31 leaves, then a chain of 8 nested profiles: 39 in all, which must be refused.
        let mut map: HashMap<String, String> = HashMap::new();
        let mut names: Vec<String> = (0..31).map(|i| format!("leaf{i}")).collect();
        for name in &names {
            map.insert(name.clone(), String::new());
        }
        for i in 0..8 {
            let next = if i == 7 {
                String::new()
            } else {
                format!("extends = [\"deep{}\"]", i + 1)
            };
            map.insert(format!("deep{i}"), next);
        }
        names.push("deep0".to_string());
        let list = format!(
            "[{}]",
            names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ")
        );
        assert!(matches!(
            assert_err!(order(spec(&list), &MapSource(map))),
            SpecError::TooManyProfiles { .. }
        ));
    }

    #[test]
    fn a_conflict_is_located_at_the_entry_listed_second() {
        let s = MapSource::of(&[("b", "extends = [\"base\"]"), ("base", "")]);
        match assert_err!(order(spec("[\"b\",\n \"base\"]"), &s)) {
            SpecError::ExtendsOrderConflict { at, .. } => assert_eq!((at.line, at.column), (2, 2)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn where_the_facts_leave_a_choice_the_earliest_found_profile_that_is_ready_goes_first() {
        // `x` was found first and the spec lists it first, but it must wait for `z`; `y`, found later through
        // `r`, is ready at once. The order is still the one that satisfies every fact.
        let s = MapSource::of(&[
            ("x", ""),
            ("r", "extends = [\"y\"]"),
            ("y", ""),
            ("q", "extends = [\"z\", \"x\"]"),
            ("z", ""),
        ]);
        let layers = assert_ok!(order(spec("[\"x\", \"r\", \"q\"]"), &s));
        assert_eq!(names(&layers), ["y", "z", "x", "r", "q", "spec"]);
    }

    fn dir_with(files: &[&str]) -> tempfile::TempDir {
        let dir = assert_ok!(tempfile::tempdir());
        for name in files {
            assert_ok!(std::fs::write(dir.path().join(name), ""));
        }
        dir
    }

    fn here() -> Located {
        Located {
            path: PathBuf::from("s.toml").into(),
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn a_directory_lists_only_its_valid_profile_files_in_order() {
        let dir = dir_with(&["zeta.toml", "alpha.toml", "notes.txt", "Bad Name.toml", "README.md"]);
        assert_ok!(std::fs::create_dir(dir.path().join("sub")));
        let name = assert_ok!(ProfileName::try_new("nope"));
        match assert_err!(DirSource::new(dir.path().to_path_buf()).find(&name, &here())) {
            SpecError::MissingProfile { available, .. } => assert_eq!(available, ["alpha", "zeta"]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_profile_found_on_disk_is_shown_under_the_name_the_user_gave() {
        let dir = dir_with(&["p.toml"]);
        let name = assert_ok!(ProfileName::try_new("p"));
        let layer = assert_ok!(DirSource::new(dir.path().to_path_buf()).find(&name, &here()));
        assert_eq!(layer.path, dir.path().join("p.toml"));
    }

    #[test]
    fn a_file_whose_case_differs_from_the_profile_name_is_refused_or_missing_never_loaded() {
        // On a file system that ignores case (macOS, Windows) the file answers to the name and is refused for its
        // case; on one that does not, it is simply missing. It is never loaded, so the tree means the same everywhere.
        let dir = dir_with(&["Principal.toml"]);
        let name = assert_ok!(ProfileName::try_new("principal"));
        match assert_err!(DirSource::new(dir.path().to_path_buf()).find(&name, &here())) {
            SpecError::ProfileNameCase { found, name, .. } => {
                assert_eq!((found.as_str(), name.as_str()), ("Principal.toml", "principal"))
            }
            SpecError::MissingProfile { .. } => {}
            other => panic!("{other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_or_looping_link_is_an_error_about_the_link_not_a_missing_profile() {
        let dir = assert_ok!(tempfile::tempdir());
        assert_ok!(std::os::unix::fs::symlink(
            dir.path().join("gone.toml"),
            dir.path().join("dangling.toml")
        ));
        assert_ok!(std::os::unix::fs::symlink(
            dir.path().join("loop.toml"),
            dir.path().join("loop.toml")
        ));
        for name in ["dangling", "loop"] {
            let name = assert_ok!(ProfileName::try_new(name));
            assert!(
                matches!(
                    assert_err!(DirSource::new(dir.path().to_path_buf()).find(&name, &here())),
                    SpecError::Read { .. }
                ),
                "{name}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_profiles_directory_fails_closed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir_with(&["p.toml"]);
        let root = dir.path().to_path_buf();
        assert_ok!(std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o000)));
        let name = assert_ok!(ProfileName::try_new("p"));
        let result = DirSource::new(root.clone()).find(&name, &here());
        assert_ok!(std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)));
        // A user who is root can read it regardless; everyone else is refused with an error, not let through.
        if let Err(error) = result {
            assert!(
                matches!(error, SpecError::Read { .. } | SpecError::MissingProfile { .. }),
                "{error:?}"
            );
        }
    }

    // ---- a generator that reaches the interesting cases ------------------------------------------------------

    /// A graph with no cycle: profile `i` extends only profiles below `i`, and the spec lists any shuffled subset.
    fn acyclic_graph() -> impl Strategy<Value = (usize, Vec<Vec<usize>>, Vec<usize>)> {
        (2usize..=6).prop_flat_map(|n| {
            let below = |i: usize| proptest::sample::subsequence((0..i).collect::<Vec<_>>(), 0..=i).prop_shuffle();
            let lists: Vec<_> = (0..n).map(below).collect();
            let spec = proptest::sample::subsequence((0..n).collect::<Vec<_>>(), 0..=n).prop_shuffle();
            (Just(n), lists, spec)
        })
    }

    fn source_of(n: usize, extends: &[Vec<usize>]) -> MapSource {
        let profiles: Vec<(String, String)> = (0..n)
            .map(|i| (format!("p{i}"), format!("extends = {}", quoted(&extends[i]))))
            .collect();
        let refs: Vec<(&str, &str)> = profiles.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        MapSource::of(&refs)
    }

    proptest! {
        #[test]
        fn on_graphs_without_cycles_the_chain_agrees_with_an_exhaustive_search((n, extends, spec_list) in acyclic_graph()) {
            let result = order(spec(&quoted(&spec_list)), &source_of(n, &extends));
            let reachable: Vec<usize> = (0..n)
                .filter(|p| spec_list.contains(p) || spec_list.iter().any(|s| reaches(&extends, *s, *p)))
                .collect();
            let possible = some_order_exists(&extends, &spec_list, &reachable);
            match result {
                Ok(layers) => {
                    prop_assert!(possible, "an order was produced where none exists");
                    let applied = names(&layers);
                    prop_assert_eq!(applied.last().map(String::as_str), Some("spec"));
                }
                Err(SpecError::ExtendsOrderConflict { .. }) => prop_assert!(!possible, "a valid order existed"),
                Err(other) => prop_assert!(false, "{other:?}"),
            }
        }
    }
}

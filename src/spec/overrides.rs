//! ABOUTME: The rule for command-line flags typed beside a spec: refuse them, or let each replace one setting.
//! ABOUTME: Pure (no clap, no I/O), so `build` and the cover-letter command apply one policy and share its tests.

use snafu::Snafu;
use strum::Display;

use super::error::{Located, SpecError};
use super::merge::ChoiceKey;
use super::resolved::ResolvedSpec;
use crate::model::options::Paper;
use crate::model::{StyleName, VariantName};
use crate::select::{Scope, SelectedTags, Selection};

/// A flag that chooses what a spec chooses, so typing it beside `--spec` changes what the spec rebuilds.
///
/// Flags about the machine (`--fonts`, `-o`, and the like) are not here: they are allowed beside a spec.
/// `--drafts` is not here either: it is a preview switch, never a choice a file can make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display)]
pub enum OverrideFlag {
    #[strum(to_string = "--tags")]
    Tags,
    #[strum(to_string = "--core-only")]
    CoreOnly,
    #[strum(to_string = "--prefer")]
    Prefer,
    #[strum(to_string = "--style")]
    Style,
    #[strum(to_string = "--paper")]
    Paper,
}

/// What a typed scope flag asks to keep. `--tags` and `--core-only` exclude each other, so there is one scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeFlag {
    Tags(SelectedTags),
    CoreOnly,
}

impl ScopeFlag {
    fn flag(&self) -> OverrideFlag {
        match self {
            Self::Tags(_) => OverrideFlag::Tags,
            Self::CoreOnly => OverrideFlag::CoreOnly,
        }
    }
}

/// The settings the user typed, and only those: a value the user did not type is `None`, whatever the flag's
/// default is, so a default can never be mistaken for a choice and clobber a value from a spec.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Explicit {
    pub style: Option<StyleName>,
    pub paper: Option<Paper>,
    pub scope: Option<ScopeFlag>,
    pub prefer: Option<Vec<VariantName>>,
}

impl Explicit {
    /// The flags that were typed, in a fixed order.
    #[must_use]
    pub fn flags(&self) -> Vec<OverrideFlag> {
        let mut flags: Vec<OverrideFlag> = [
            self.scope.as_ref().map(ScopeFlag::flag),
            self.prefer.as_ref().map(|_| OverrideFlag::Prefer),
            self.style.as_ref().map(|_| OverrideFlag::Style),
            self.paper.as_ref().map(|_| OverrideFlag::Paper),
        ]
        .into_iter()
        .flatten()
        .collect();
        flags.sort();
        flags
    }

    /// Lets the settings through, or refuses them, according to `policy`. Typing nothing is always allowed.
    ///
    /// # Errors
    /// [`Refused`] naming every typed flag, when `policy` refuses and any was typed.
    pub fn admit(self, policy: OverridePolicy) -> Result<Admitted, Refused> {
        let flags = self.flags();
        if policy == OverridePolicy::Refuse && !flags.is_empty() {
            return Err(Refused { flags });
        }
        Ok(Admitted(self))
    }
}

/// Whether settings typed beside a spec are let through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverridePolicy {
    /// The default: a spec is the whole recipe, so a flag that would change it is a usage error.
    Refuse,
    /// `--allow-overrides`: each typed setting replaces the spec's, and the build says so.
    Allow,
}

/// Settings that passed the policy. Only [`Explicit::admit`] makes one, so applying settings to a spec without
/// going through the policy does not compile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Admitted(Explicit);

impl Admitted {
    /// Nothing typed: the spec alone. Only the tests need it: a build admits what it parsed.
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn explicit(&self) -> &Explicit {
        &self.0
    }
}

/// Flags typed beside a spec that the policy refuses, every one of them, in a fixed order.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(display(
    "{} cannot be used beside --spec: the spec is the whole recipe, so a flag would silently change what it \
     rebuilds; put the choice in the spec or a profile, or add --allow-overrides to experiment",
    listed(flags)
))]
pub struct Refused {
    pub flags: Vec<OverrideFlag>,
}

fn listed(flags: &[OverrideFlag]) -> String {
    let names: Vec<String> = flags.iter().map(ToString::to_string).collect();
    match names.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The kind of choice a typed flag replaced, as a spec spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Setting {
    Tags,
    CoreOnly,
    Prefer,
    Style,
    Paper,
}

/// What a typed flag replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replaced {
    /// The spec chose nothing, so the built-in default would have applied.
    Default,
    /// What the spec chose: the kind of choice, its values (none for `core-only`), and every place that chose it.
    Spec {
        setting: Setting,
        value: Vec<String>,
        at: Vec<Located>,
    },
}

/// One flag typed beside a spec, what it set, and what it replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    pub flag: OverrideFlag,
    /// The values typed; none for a switch.
    pub set: Vec<String>,
    pub replaced: Replaced,
}

/// A spec with the typed settings applied: what to select and what to render.
///
/// `style` and `paper` are `None` when neither the flags nor the spec chose, so the built-in default applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub style: Option<StyleName>,
    pub paper: Option<Paper>,
    /// Drafts are off: whether to preview them is the command line's choice.
    pub selection: Selection,
    pub overrides: Vec<Override>,
}

/// Applies `flags` to `spec`: each typed setting replaces the spec's, and the scope is replaced as one unit.
///
/// The spec is checked alone first, so a flag cannot make a spec build that would not build without it.
/// Picks come only from the spec; no flag sets one.
///
/// # Errors
/// [`SpecError::ScopeConflict`] when the spec alone asks for core-only and tags.
pub fn apply(spec: &ResolvedSpec, flags: &Admitted) -> Result<Applied, SpecError> {
    let mut selection = spec.to_selection()?;
    let explicit = flags.explicit();
    let mut overrides = Vec::new();
    if let Some(scope) = &explicit.scope {
        overrides.push(Override {
            flag: scope.flag(),
            set: scope_values(scope),
            replaced: replaced_scope(spec),
        });
        selection.scope = match scope {
            ScopeFlag::Tags(tags) => Scope::Tags(tags.clone()),
            ScopeFlag::CoreOnly => Scope::CoreOnly,
        };
    }
    if let Some(prefer) = &explicit.prefer {
        overrides.push(Override {
            flag: OverrideFlag::Prefer,
            set: names(prefer),
            replaced: replaced_prefer(spec),
        });
        selection.prefer.clone_from(prefer);
    }
    if let Some(style) = &explicit.style {
        overrides.push(Override {
            flag: OverrideFlag::Style,
            set: vec![style.to_string()],
            replaced: replaced_one(
                spec,
                Setting::Style,
                spec.choices.style.as_ref().map(ToString::to_string),
            ),
        });
    }
    if let Some(paper) = explicit.paper {
        overrides.push(Override {
            flag: OverrideFlag::Paper,
            set: vec![paper_name(paper)],
            replaced: replaced_one(spec, Setting::Paper, spec.choices.paper.map(paper_name)),
        });
    }
    Ok(Applied {
        style: explicit.style.clone().or_else(|| spec.choices.style.clone()),
        paper: explicit.paper.or(spec.choices.paper),
        selection,
        overrides,
    })
}

fn names<T: ToString>(items: &[T]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

/// The paper as `--paper` and a spec spell it: `letter` or `a4`.
#[must_use]
pub fn paper_name(paper: Paper) -> String {
    use clap::ValueEnum as _;
    paper
        .to_possible_value()
        .map_or_else(|| paper.to_string(), |value| value.get_name().to_string())
}

fn scope_values(scope: &ScopeFlag) -> Vec<String> {
    match scope {
        ScopeFlag::Tags(tags) => names(&tags.as_set().iter().collect::<Vec<_>>()),
        ScopeFlag::CoreOnly => Vec::new(),
    }
}

/// The place that decided each of `keys`, the first in each file, from the least specific file to the most.
///
/// A list such as the tags is decided by several lines of one file; naming the file once, at its first line, is
/// enough to find them.
fn decided_at(spec: &ResolvedSpec, keys: impl IntoIterator<Item = ChoiceKey>) -> Vec<Located> {
    let mut found: Vec<Located> = keys
        .into_iter()
        .filter_map(|key| spec.origin_of(&key))
        .cloned()
        .collect();
    let rank = |at: &Located| spec.chain.iter().position(|link| *link.path == *at.path);
    found.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| (a.line, a.column).cmp(&(b.line, b.column)))
    });
    found.dedup_by(|later, earlier| *later.path == *earlier.path);
    found
}

fn replaced_scope(spec: &ResolvedSpec) -> Replaced {
    let select = &spec.choices.select;
    if select.core_only == Some(true) {
        return Replaced::Spec {
            setting: Setting::CoreOnly,
            value: Vec::new(),
            at: decided_at(spec, [ChoiceKey::CoreOnly]),
        };
    }
    if select.tags.is_empty() {
        return Replaced::Default;
    }
    Replaced::Spec {
        setting: Setting::Tags,
        value: names(&select.tags.iter().collect::<Vec<_>>()),
        at: decided_at(spec, select.tags.iter().cloned().map(ChoiceKey::Tag)),
    }
}

fn replaced_prefer(spec: &ResolvedSpec) -> Replaced {
    let prefer = &spec.choices.select.prefer;
    if prefer.is_empty() {
        return Replaced::Default;
    }
    Replaced::Spec {
        setting: Setting::Prefer,
        value: names(prefer),
        at: decided_at(spec, prefer.iter().cloned().map(ChoiceKey::Prefer)),
    }
}

/// What a spec chose for a single value (`style` or `paper`), which has one place.
fn replaced_one(spec: &ResolvedSpec, setting: Setting, value: Option<String>) -> Replaced {
    let key = if setting == Setting::Style {
        ChoiceKey::Style
    } else {
        ChoiceKey::Paper
    };
    value.map_or(Replaced::Default, |value| Replaced::Spec {
        setting,
        value: vec![value],
        at: decided_at(spec, [key]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Tag;
    use crate::spec::resolve;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;

    /// A spec in a temporary directory, extending nothing, with `body` after its `[build]` table.
    fn spec_text(build: &str, body: &str) -> (tempfile::TempDir, ResolvedSpec) {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("acme.toml");
        assert_ok!(std::fs::write(
            &path,
            format!("[build]\ncontent = \"a.md\"\n{build}\n{body}")
        ));
        let spec = assert_ok!(resolve(&path, None));
        (dir, spec)
    }

    fn tags(names: &[&str]) -> ScopeFlag {
        let set = names.iter().map(|n| assert_ok!(Tag::try_new(*n))).collect();
        ScopeFlag::Tags(SelectedTags::new(set).expect("a name was given"))
    }

    fn prefer(names: &[&str]) -> Vec<VariantName> {
        names.iter().map(|n| assert_ok!(VariantName::try_new(*n))).collect()
    }

    fn allowed(explicit: Explicit) -> Admitted {
        assert_ok!(explicit.admit(OverridePolicy::Allow))
    }

    #[test]
    fn no_flags_means_no_overrides_and_the_specs_own_choices() {
        let (_dir, spec) = spec_text(
            "style = \"mono\"\npaper = \"a4\"",
            "[select]\ntags = [\"ai\"]\nprefer = [\"short\"]\n",
        );
        let applied = assert_ok!(apply(&spec, &Admitted::none()));
        assert_eq!(applied.overrides, []);
        assert_eq!(applied.style, Some(assert_ok!(StyleName::try_new("mono"))));
        assert_eq!(applied.paper, Some(Paper::A4));
        assert_eq!(applied.selection, assert_ok!(spec.to_selection()));
    }

    #[test]
    fn a_flag_replaces_the_spec_value_and_records_what_it_replaced() {
        let (_dir, spec) = spec_text("style = \"mono\"\npaper = \"a4\"", "");
        let flags = allowed(Explicit {
            style: Some(assert_ok!(StyleName::try_new("pragmatic"))),
            paper: Some(Paper::Letter),
            ..Explicit::default()
        });
        let applied = assert_ok!(apply(&spec, &flags));
        assert_eq!(applied.style, Some(assert_ok!(StyleName::try_new("pragmatic"))));
        assert_eq!(applied.paper, Some(Paper::Letter));
        let [style, paper] = applied.overrides.as_slice() else {
            panic!("two overrides expected: {:?}", applied.overrides)
        };
        assert_eq!(
            (style.flag, style.set.as_slice()),
            (OverrideFlag::Style, &["pragmatic".to_string()][..])
        );
        assert!(matches!(
            &style.replaced,
            Replaced::Spec { setting: Setting::Style, value, at } if value == &["mono"] && at.len() == 1
        ));
        assert_eq!(
            (paper.flag, paper.set.as_slice()),
            (OverrideFlag::Paper, &["letter".to_string()][..])
        );
        assert!(matches!(
            &paper.replaced,
            Replaced::Spec { setting: Setting::Paper, value, .. } if value == &["a4"]
        ));
    }

    #[test]
    fn a_style_flag_over_a_spec_without_a_style_replaces_the_default() {
        let (_dir, spec) = spec_text("", "");
        let flags = allowed(Explicit {
            style: Some(assert_ok!(StyleName::try_new("mono"))),
            ..Explicit::default()
        });
        let applied = assert_ok!(apply(&spec, &flags));
        assert_eq!(applied.overrides[0].replaced, Replaced::Default);
    }

    #[test]
    fn a_scope_flag_replaces_the_whole_scope_so_it_can_narrow() {
        let (_dir, spec) = spec_text("", "[select]\ntags = [\"ai\", \"platform\"]\n");
        let flags = allowed(Explicit {
            scope: Some(tags(&["ai"])),
            ..Explicit::default()
        });
        let applied = assert_ok!(apply(&spec, &flags));
        let Scope::Tags(kept) = &applied.selection.scope else {
            panic!("tags expected")
        };
        assert_eq!(kept.as_set().len(), 1, "a union would keep both");
        assert!(matches!(
            &applied.overrides[0].replaced,
            Replaced::Spec { setting: Setting::Tags, value, .. } if value == &["ai", "platform"]
        ));
    }

    #[test]
    fn core_only_replaces_the_specs_tags_instead_of_conflicting_with_them() {
        let (_dir, spec) = spec_text("", "[select]\ntags = [\"ai\"]\n");
        let flags = allowed(Explicit {
            scope: Some(ScopeFlag::CoreOnly),
            ..Explicit::default()
        });
        let applied = assert_ok!(apply(&spec, &flags));
        assert_eq!(applied.selection.scope, Scope::CoreOnly);
        assert_eq!(
            (applied.overrides[0].flag, applied.overrides[0].set.clone()),
            (OverrideFlag::CoreOnly, Vec::<String>::new()),
            "a switch sets no value"
        );
    }

    #[test]
    fn tags_replace_a_specs_core_only() {
        let (_dir, spec) = spec_text("", "[select]\ncore-only = true\n");
        let flags = allowed(Explicit {
            scope: Some(tags(&["ai"])),
            ..Explicit::default()
        });
        let applied = assert_ok!(apply(&spec, &flags));
        assert!(matches!(applied.selection.scope, Scope::Tags(_)));
        assert!(matches!(
            &applied.overrides[0].replaced,
            Replaced::Spec { setting: Setting::CoreOnly, value, at } if value.is_empty() && at.len() == 1
        ));
    }

    #[test]
    fn prefer_replaces_the_list_and_does_not_prepend_to_it() {
        let (_dir, spec) = spec_text("", "[select]\nprefer = [\"short\", \"business\"]\n");
        let flags = allowed(Explicit {
            prefer: Some(prefer(&["technical"])),
            ..Explicit::default()
        });
        let applied = assert_ok!(apply(&spec, &flags));
        assert_eq!(applied.selection.prefer, prefer(&["technical"]));
        assert!(matches!(
            &applied.overrides[0].replaced,
            Replaced::Spec { setting: Setting::Prefer, value, .. } if value == &["short", "business"]
        ));
    }

    #[test]
    fn picks_come_only_from_the_spec() {
        let (_dir, spec) = spec_text("", "[select]\npick = { gateway = \"short\" }\n");
        let flags = allowed(Explicit {
            prefer: Some(prefer(&["long"])),
            scope: Some(ScopeFlag::CoreOnly),
            ..Explicit::default()
        });
        let applied = assert_ok!(apply(&spec, &flags));
        assert_eq!(applied.selection.picks, assert_ok!(spec.to_selection()).picks);
        assert_eq!(applied.selection.picks.len(), 1);
    }

    #[test]
    fn a_flag_equal_to_the_specs_value_is_still_listed() {
        let (_dir, spec) = spec_text("paper = \"a4\"", "");
        let flags = allowed(Explicit {
            paper: Some(Paper::A4),
            ..Explicit::default()
        });
        assert_eq!(assert_ok!(apply(&spec, &flags)).overrides.len(), 1);
    }

    #[test]
    fn a_flag_over_nothing_the_spec_chose_replaces_the_default() {
        let (_dir, spec) = spec_text("", "");
        for explicit in [
            Explicit {
                prefer: Some(prefer(&["short"])),
                ..Explicit::default()
            },
            Explicit {
                scope: Some(tags(&["ai"])),
                ..Explicit::default()
            },
            Explicit {
                scope: Some(ScopeFlag::CoreOnly),
                ..Explicit::default()
            },
            Explicit {
                paper: Some(Paper::A4),
                ..Explicit::default()
            },
        ] {
            let applied = assert_ok!(apply(&spec, &allowed(explicit.clone())));
            assert_eq!(applied.overrides.len(), 1, "{explicit:?}");
            assert_eq!(applied.overrides[0].replaced, Replaced::Default, "{explicit:?}");
        }
    }

    /// A profile and a spec in the layout a user has: `profiles/p.toml` beside `positions/acme.toml`.
    fn spec_with_profile(profile: &str, spec: &str) -> (tempfile::TempDir, ResolvedSpec) {
        let dir = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir_all(dir.path().join("profiles")));
        assert_ok!(std::fs::create_dir_all(dir.path().join("positions")));
        assert_ok!(std::fs::write(dir.path().join("profiles/p.toml"), profile));
        let path = dir.path().join("positions/acme.toml");
        assert_ok!(std::fs::write(
            &path,
            format!("extends = [\"p\"]\n[build]\ncontent = \"a.md\"\n{spec}")
        ));
        let resolved = assert_ok!(resolve(&path, None));
        (dir, resolved)
    }

    #[test]
    fn the_places_that_decided_a_list_are_the_first_line_of_each_file_least_specific_first() {
        let (_dir, spec) = spec_with_profile("[select]\ntags = [\"a\", \"b\"]\n", "[select]\ntags = [\"c\"]\n");
        let flags = allowed(Explicit {
            scope: Some(ScopeFlag::CoreOnly),
            ..Explicit::default()
        });
        let applied = assert_ok!(apply(&spec, &flags));
        let Replaced::Spec { at, .. } = &applied.overrides[0].replaced else {
            panic!("the spec chose tags: {:?}", applied.overrides)
        };
        let places: Vec<(String, usize)> = at
            .iter()
            .map(|l| {
                (
                    l.path.file_name().expect("a file").to_string_lossy().into_owned(),
                    l.line,
                )
            })
            .collect();
        assert_eq!(
            places,
            [("p.toml".to_string(), 2), ("acme.toml".to_string(), 5)],
            "one place per file, the profile first, each at its first line"
        );
    }

    #[test]
    fn a_scope_conflict_in_the_spec_is_refused_even_when_a_flag_would_hide_it() {
        let (_dir, spec) = spec_text("", "[select]\ntags = [\"ai\"]\ncore-only = true\n");
        let flags = allowed(Explicit {
            scope: Some(tags(&["ai"])),
            ..Explicit::default()
        });
        assert!(matches!(
            assert_err!(apply(&spec, &flags)),
            SpecError::ScopeConflict { .. }
        ));
    }

    #[test]
    fn the_policy_refuses_every_typed_flag_at_once_and_names_them_in_a_fixed_order() {
        let explicit = Explicit {
            style: Some(assert_ok!(StyleName::try_new("mono"))),
            paper: Some(Paper::A4),
            scope: Some(tags(&["ai"])),
            prefer: Some(prefer(&["short"])),
        };
        let refused = assert_err!(explicit.admit(OverridePolicy::Refuse));
        assert_eq!(
            refused.flags,
            [
                OverrideFlag::Tags,
                OverrideFlag::Prefer,
                OverrideFlag::Style,
                OverrideFlag::Paper
            ]
        );
        let text = refused.to_string();
        assert!(
            text.starts_with("--tags, --prefer, --style and --paper cannot be used beside --spec"),
            "{text}"
        );
        assert!(text.contains("--allow-overrides"), "{text}");
    }

    #[test]
    fn one_refused_flag_is_named_alone() {
        let explicit = Explicit {
            scope: Some(ScopeFlag::CoreOnly),
            ..Explicit::default()
        };
        let text = assert_err!(explicit.admit(OverridePolicy::Refuse)).to_string();
        assert!(text.starts_with("--core-only cannot be used beside --spec"), "{text}");
    }

    #[test]
    fn typing_nothing_is_allowed_under_either_policy_and_allow_admits_everything() {
        assert_ok!(Explicit::default().admit(OverridePolicy::Refuse));
        let explicit = Explicit {
            paper: Some(Paper::A4),
            ..Explicit::default()
        };
        assert_ok!(explicit.admit(OverridePolicy::Allow));
    }
}

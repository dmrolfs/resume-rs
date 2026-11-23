//! ABOUTME: The rule for command-line flags typed beside a spec: refuse them, or let each replace one setting.
//! ABOUTME: Pure (no clap, no I/O), so `build` and the cover-letter command apply one policy and share its tests.

use snafu::Snafu;
use strum::Display;

use super::error::{Located, NoLetterTableSnafu, SpecError};
use super::layer::Sourced;
use super::letter::ResolvedLetter;
use super::merge::{ChoiceKey, SelectChoices};
use super::resolved::{ChainLink, ResolvedSpec};
use crate::letter::{FieldValue, LengthUnit, LetterDate, LetterFields, LetterOutput, LinkStyle};
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
    // The rest are letter flags: a letter spec can say each of them, so each is refused beside `--spec` too.
    #[strum(to_string = "--output-format")]
    OutputFormat,
    #[strum(to_string = "--max-length")]
    MaxLength,
    #[strum(to_string = "--length-unit")]
    LengthUnit,
    #[strum(to_string = "--links")]
    Links,
    #[strum(to_string = "--company")]
    Company,
    #[strum(to_string = "--title")]
    Title,
    #[strum(to_string = "--recipient")]
    Recipient,
    #[strum(to_string = "--date")]
    Date,
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
        policy.check(self.flags())?;
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

impl OverridePolicy {
    /// Refuses `flags` when this policy refuses and any was typed.
    pub(super) fn check(self, flags: Vec<OverrideFlag>) -> Result<(), Refused> {
        if self == Self::Refuse && !flags.is_empty() {
            return Err(Refused { flags });
        }
        Ok(())
    }
}

/// Settings that passed the policy. Only [`Explicit::admit`] and `LetterExplicit::admit` make one, so applying
/// settings to a spec without going through the policy does not compile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Admitted<T = Explicit>(pub(super) T);

impl<T> Admitted<T> {
    /// Nothing typed: the spec alone. Only the tests need it: a build admits what it parsed.
    #[cfg(test)]
    pub(crate) fn none() -> Self
    where
        T: Default,
    {
        Self(T::default())
    }

    #[must_use]
    pub fn explicit(&self) -> &T {
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
    OutputFormat,
    MaxLength,
    LengthUnit,
    Links,
    Company,
    Title,
    Recipient,
    Date,
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

/// Where the choices of one scope (the resume's `[select]`, or the letter's `[letter.select]`) came from.
///
/// The two scopes are resolved apart over one chain, so a flag that replaces a letter choice must name the lines of
/// `[letter.select]` and never the resume's.
pub(super) struct ScopeView<'a> {
    select: &'a SelectChoices,
    chain: &'a [ChainLink],
    origins: Origins<'a>,
}

/// Which document's origins a [`ScopeView`] reads.
#[derive(Clone, Copy)]
enum Origins<'a> {
    Resume(&'a ResolvedSpec),
    Letter(&'a ResolvedLetter),
}

impl<'a> ScopeView<'a> {
    pub(super) fn of_resume(spec: &'a ResolvedSpec) -> Self {
        Self {
            select: &spec.choices.select,
            chain: &spec.chain,
            origins: Origins::Resume(spec),
        }
    }

    /// The letter's own scope: `letter.select` and the lines that decided it, over the same chain.
    pub(super) fn of_letter(spec: &'a ResolvedSpec, letter: &'a ResolvedLetter) -> Self {
        Self {
            select: &letter.choices,
            chain: &spec.chain,
            origins: Origins::Letter(letter),
        }
    }

    fn origin_of(&self, key: &ChoiceKey) -> Option<&'a Located> {
        match self.origins {
            Origins::Resume(spec) => spec.origin_of(key),
            Origins::Letter(letter) => letter.origin_of(key),
        }
    }

    /// The place that decided each of `keys`, the first in each file, from the least specific file to the most.
    ///
    /// A list such as the tags is decided by several lines of one file; naming the file once, at its first line, is
    /// enough to find them.
    pub(super) fn decided_at(&self, keys: impl IntoIterator<Item = ChoiceKey>) -> Vec<Located> {
        let mut found: Vec<Located> = keys
            .into_iter()
            .filter_map(|key| self.origin_of(&key))
            .cloned()
            .collect();
        let rank = |at: &Located| self.chain.iter().position(|link| *link.path == *at.path);
        found.sort_by(|a, b| {
            rank(a)
                .cmp(&rank(b))
                .then_with(|| (a.line, a.column).cmp(&(b.line, b.column)))
        });
        found.dedup_by(|later, earlier| *later.path == *earlier.path);
        found
    }

    /// What typing a scope flag replaced.
    pub(super) fn replaced_scope(&self) -> Replaced {
        if self.select.core_only == Some(true) {
            return Replaced::Spec {
                setting: Setting::CoreOnly,
                value: Vec::new(),
                at: self.decided_at([ChoiceKey::CoreOnly]),
            };
        }
        if self.select.tags.is_empty() {
            return Replaced::Default;
        }
        Replaced::Spec {
            setting: Setting::Tags,
            value: names(&self.select.tags.iter().collect::<Vec<_>>()),
            at: self.decided_at(self.select.tags.iter().cloned().map(ChoiceKey::Tag)),
        }
    }

    /// What typing `--prefer` replaced.
    pub(super) fn replaced_prefer(&self) -> Replaced {
        let prefer = &self.select.prefer;
        if prefer.is_empty() {
            return Replaced::Default;
        }
        Replaced::Spec {
            setting: Setting::Prefer,
            value: names(prefer),
            at: self.decided_at(prefer.iter().cloned().map(ChoiceKey::Prefer)),
        }
    }
}

/// Overlays the scope and `--prefer` flags on `selection`, recording what each replaced. Shared by both documents.
pub(super) fn overlay_select(
    explicit: &Explicit,
    view: &ScopeView<'_>,
    selection: &mut Selection,
    overrides: &mut Vec<Override>,
) {
    if let Some(scope) = &explicit.scope {
        overrides.push(Override {
            flag: scope.flag(),
            set: scope_values(scope),
            replaced: view.replaced_scope(),
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
            replaced: view.replaced_prefer(),
        });
        selection.prefer.clone_from(prefer);
    }
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
    overlay_select(explicit, &ScopeView::of_resume(spec), &mut selection, &mut overrides);
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

pub(super) fn names<T: ToString>(items: &[T]) -> Vec<String> {
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
        at: ScopeView::of_resume(spec).decided_at([key]),
    })
}

// ---------------------------------------------------------------------------------------------------------------
// The letter
// ---------------------------------------------------------------------------------------------------------------

/// The letter settings the user typed, and only those: a value not typed is `None`, whatever a flag's default is.
///
/// `base` holds what a letter shares with a resume: the scope, `--prefer`, `--style`, and `--paper`. The rest are
/// what only a letter has. Every one of them is something a spec can say, so each is refused beside `--spec` unless
/// `--allow-overrides` is given; the flags about the machine are not here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LetterExplicit {
    pub base: Explicit,
    pub output: Option<LetterOutput>,
    pub max_length: Option<usize>,
    pub length_unit: Option<LengthUnit>,
    pub links: Option<LinkStyle>,
    pub company: Option<FieldValue>,
    pub title: Option<FieldValue>,
    pub recipient: Option<FieldValue>,
    pub date: Option<LetterDate>,
}

impl LetterExplicit {
    /// The flags that were typed, in a fixed order.
    #[must_use]
    pub fn flags(&self) -> Vec<OverrideFlag> {
        let mut flags = self.base.flags();
        flags.extend(
            [
                self.output.map(|_| OverrideFlag::OutputFormat),
                self.max_length.map(|_| OverrideFlag::MaxLength),
                self.length_unit.map(|_| OverrideFlag::LengthUnit),
                self.links.map(|_| OverrideFlag::Links),
                self.company.as_ref().map(|_| OverrideFlag::Company),
                self.title.as_ref().map(|_| OverrideFlag::Title),
                self.recipient.as_ref().map(|_| OverrideFlag::Recipient),
                self.date.map(|_| OverrideFlag::Date),
            ]
            .into_iter()
            .flatten(),
        );
        flags.sort();
        flags
    }

    /// Lets the settings through, or refuses them, according to `policy`. Typing nothing is always allowed.
    ///
    /// # Errors
    /// [`Refused`] naming every typed flag, when `policy` refuses and any was typed.
    pub fn admit(self, policy: OverridePolicy) -> Result<Admitted<Self>, Refused> {
        policy.check(self.flags())?;
        Ok(Admitted(self))
    }
}

/// Where an effective letter setting came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Typed on the command line.
    Flag,
    /// Written in `[letter]` or `[letter.select]`, at this place.
    Spec(Located),
    /// Not written for the letter: the spec's `build.style` or `build.paper`, at this place, was used.
    Build(Located),
    /// Written in `[position]`, whose lines are not recorded: this is the spec's file.
    Position(std::sync::Arc<std::path::Path>),
    /// Nobody chose it: the built-in default.
    Default,
}

/// A value and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen<T> {
    pub value: T,
    pub origin: Origin,
}

impl<T> Chosen<T> {
    fn new(value: T, origin: Origin) -> Self {
        Self { value, origin }
    }

    /// The first of a flag, the spec's value, and a fallback that is set, then the default.
    fn first(flag: Option<T>, spec: Option<(T, Origin)>, default: T) -> Self {
        match (flag, spec) {
            (Some(value), _) => Self::new(value, Origin::Flag),
            (None, Some((value, origin))) => Self::new(value, origin),
            (None, None) => Self::new(default, Origin::Default),
        }
    }
}

/// Every effective letter setting, the built-in defaults applied once and after every other layer.
///
/// Only [`LetterChoices::from_flags`] and [`apply_letter`] make one, so no setting is filled from a default by a
/// caller that has not asked the spec first: `non_exhaustive` stops a struct literal outside this crate, and the
/// constructors are the only way in.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LetterChoices {
    pub output: Chosen<LetterOutput>,
    pub links: Chosen<LinkStyle>,
    pub max_length: Option<Chosen<usize>>,
    pub length_unit: Chosen<LengthUnit>,
    pub company: Option<Chosen<FieldValue>>,
    pub title: Option<Chosen<FieldValue>>,
    pub recipient: Option<Chosen<FieldValue>>,
    pub date: Option<Chosen<LetterDate>>,
    /// Used only when a PDF is made.
    pub style: Chosen<StyleName>,
    /// Used only when a PDF is made.
    pub paper: Chosen<Paper>,
}

/// The style a letter is made in when nothing chose one.
fn default_style() -> StyleName {
    StyleName::try_new("pragmatic").expect("the built-in style name is valid")
}

impl LetterChoices {
    /// The flags alone: a letter built from a file, with no spec.
    #[must_use]
    pub fn from_flags(flags: &LetterExplicit) -> Self {
        let field = |value: &Option<FieldValue>| value.clone().map(|v| Chosen::new(v, Origin::Flag));
        Self {
            output: Chosen::first(flags.output, None, LetterOutput::default()),
            links: Chosen::first(flags.links, None, LinkStyle::default()),
            max_length: flags.max_length.map(|n| Chosen::new(n, Origin::Flag)),
            length_unit: Chosen::first(flags.length_unit, None, LengthUnit::default()),
            company: field(&flags.company),
            title: field(&flags.title),
            recipient: field(&flags.recipient),
            date: flags.date.map(|d| Chosen::new(d, Origin::Flag)),
            style: Chosen::first(flags.base.style.clone(), None, default_style()),
            paper: Chosen::first(flags.base.paper, None, Paper::default()),
        }
    }

    /// The company, title, recipient, and date the letter is built with.
    #[must_use]
    pub fn fields(&self) -> LetterFields {
        LetterFields {
            company: self.company.as_ref().map(|c| c.value.clone()),
            title: self.title.as_ref().map(|c| c.value.clone()),
            recipient: self.recipient.as_ref().map(|c| c.value.clone()),
            date: self.date.as_ref().map(|c| c.value),
        }
    }
}

/// A spec's letter with the typed settings applied: what to select and every effective setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedLetter {
    pub choices: LetterChoices,
    /// Drafts are off: whether to preview them is the command line's choice.
    pub selection: Selection,
    pub overrides: Vec<Override>,
}

/// What a flag replaced, when the spec's `[letter]` said a value at `sourced`.
fn replaced_from<T: ToString>(setting: Setting, sourced: Option<&Sourced<T>>) -> Replaced {
    sourced.map_or(Replaced::Default, |s| Replaced::Spec {
        setting,
        value: vec![s.value.to_string()],
        at: vec![s.at.clone()],
    })
}

/// Applies `flags` to the letter of `spec`: each typed setting replaces the spec's, over the letter's own scope.
///
/// The spec is checked alone first (its selection, and its recipient and date as letter fields), so a flag cannot
/// make a spec build that would not build without it. A letter's style and paper fall back to the spec's
/// `build.style` and `build.paper` when `[letter]` says none, so the letter and the resume of one job match.
/// Picks come only from the spec; no flag sets one.
///
/// # Errors
/// [`SpecError::NoLetterTable`] for a spec without `[letter]`; [`SpecError::ScopeConflict`] when the letter's
/// scope asks for core-only and tags; the errors of `ResolvedLetter::fields` for a recipient or date a letter
/// cannot print.
pub fn apply_letter(spec: &ResolvedSpec, flags: &Admitted<LetterExplicit>) -> Result<AppliedLetter, SpecError> {
    let Some(letter) = &spec.letter else {
        return NoLetterTableSnafu.fail();
    };
    let mut selection = spec.to_letter_selection()?;
    let from_spec = letter.fields()?;
    let explicit = flags.explicit();
    let settings = &letter.settings;
    let mut overrides = Vec::new();
    overlay_select(
        &explicit.base,
        &ScopeView::of_letter(spec, letter),
        &mut selection,
        &mut overrides,
    );

    // `style` and `paper` are the build's when the letter says none; the line named is the one that chose.
    let style_spec = settings
        .style
        .as_ref()
        .map(|s| (s.value.clone(), Origin::Spec(s.at.clone())))
        .or_else(|| {
            let at = spec.origin_of(&ChoiceKey::Style)?.clone();
            Some((spec.choices.style.clone()?, Origin::Build(at)))
        });
    let paper_spec = settings
        .paper
        .as_ref()
        .map(|p| (p.value, Origin::Spec(p.at.clone())))
        .or_else(|| {
            let at = spec.origin_of(&ChoiceKey::Paper)?.clone();
            Some((spec.choices.paper?, Origin::Build(at)))
        });
    let origin_place = |origin: &Origin| match origin {
        Origin::Spec(at) | Origin::Build(at) => vec![at.clone()],
        _ => Vec::new(),
    };
    if let Some(style) = &explicit.base.style {
        overrides.push(Override {
            flag: OverrideFlag::Style,
            set: vec![style.to_string()],
            replaced: style_spec
                .as_ref()
                .map_or(Replaced::Default, |(value, origin)| Replaced::Spec {
                    setting: Setting::Style,
                    value: vec![value.to_string()],
                    at: origin_place(origin),
                }),
        });
    }
    if let Some(paper) = explicit.base.paper {
        overrides.push(Override {
            flag: OverrideFlag::Paper,
            set: vec![paper_name(paper)],
            replaced: paper_spec
                .as_ref()
                .map_or(Replaced::Default, |(value, origin)| Replaced::Spec {
                    setting: Setting::Paper,
                    value: vec![paper_name(*value)],
                    at: origin_place(origin),
                }),
        });
    }
    if let Some(output) = explicit.output {
        overrides.push(Override {
            flag: OverrideFlag::OutputFormat,
            set: vec![output.to_string()],
            replaced: replaced_from(Setting::OutputFormat, settings.output_format.as_ref()),
        });
    }
    if let Some(max) = explicit.max_length {
        overrides.push(Override {
            flag: OverrideFlag::MaxLength,
            set: vec![max.to_string()],
            replaced: replaced_from(Setting::MaxLength, settings.max_length.as_ref()),
        });
    }
    if let Some(unit) = explicit.length_unit {
        overrides.push(Override {
            flag: OverrideFlag::LengthUnit,
            set: vec![unit.to_string()],
            replaced: replaced_from(Setting::LengthUnit, settings.length_unit.as_ref()),
        });
    }
    if let Some(links) = explicit.links {
        overrides.push(Override {
            flag: OverrideFlag::Links,
            set: vec![links.to_string()],
            replaced: replaced_from(Setting::Links, settings.links.as_ref()),
        });
    }
    // `[position]` records no lines, so what a company or title flag replaced is named without a place.
    let position = |setting: Setting, value: Option<String>| {
        value.map_or(Replaced::Default, |value| Replaced::Spec {
            setting,
            value: vec![value],
            at: Vec::new(),
        })
    };
    if let Some(company) = &explicit.company {
        overrides.push(Override {
            flag: OverrideFlag::Company,
            set: vec![company.to_string()],
            replaced: position(Setting::Company, from_spec.company.as_ref().map(ToString::to_string)),
        });
    }
    if let Some(title) = &explicit.title {
        overrides.push(Override {
            flag: OverrideFlag::Title,
            set: vec![title.to_string()],
            replaced: position(Setting::Title, from_spec.title.as_ref().map(ToString::to_string)),
        });
    }
    if let Some(recipient) = &explicit.recipient {
        overrides.push(Override {
            flag: OverrideFlag::Recipient,
            set: vec![recipient.to_string()],
            replaced: replaced_from(Setting::Recipient, settings.recipient.as_ref()),
        });
    }
    if let Some(date) = explicit.date {
        overrides.push(Override {
            flag: OverrideFlag::Date,
            set: vec![date.to_string()],
            replaced: replaced_from(Setting::Date, settings.date.as_ref()),
        });
    }

    let spec_file = letter.content.at.path.clone();
    let field = |flag: &Option<FieldValue>, spec: Option<FieldValue>, origin: Origin| match (flag, spec) {
        (Some(value), _) => Some(Chosen::new(value.clone(), Origin::Flag)),
        (None, Some(value)) => Some(Chosen::new(value, origin)),
        (None, None) => None,
    };
    let choices = LetterChoices {
        output: Chosen::first(
            explicit.output,
            settings
                .output_format
                .as_ref()
                .map(|v| (v.value, Origin::Spec(v.at.clone()))),
            LetterOutput::default(),
        ),
        links: Chosen::first(
            explicit.links,
            settings.links.as_ref().map(|v| (v.value, Origin::Spec(v.at.clone()))),
            LinkStyle::default(),
        ),
        max_length: match (explicit.max_length, &settings.max_length) {
            (Some(n), _) => Some(Chosen::new(n, Origin::Flag)),
            (None, Some(n)) => Some(Chosen::new(n.value, Origin::Spec(n.at.clone()))),
            (None, None) => None,
        },
        length_unit: Chosen::first(
            explicit.length_unit,
            settings
                .length_unit
                .as_ref()
                .map(|v| (v.value, Origin::Spec(v.at.clone()))),
            LengthUnit::default(),
        ),
        company: field(
            &explicit.company,
            from_spec.company,
            Origin::Position(spec_file.clone()),
        ),
        title: field(&explicit.title, from_spec.title, Origin::Position(spec_file)),
        // A value is never dropped for want of a place: if `fields()` ever gains a source that `settings` lacks, the
        // letter's `content` line is named rather than the value lost.
        recipient: match (&explicit.recipient, from_spec.recipient) {
            (Some(value), _) => Some(Chosen::new(value.clone(), Origin::Flag)),
            (None, Some(value)) => Some(Chosen::new(
                value,
                Origin::Spec(
                    settings
                        .recipient
                        .as_ref()
                        .map_or_else(|| letter.content.at.clone(), |r| r.at.clone()),
                ),
            )),
            (None, None) => None,
        },
        date: match (explicit.date, from_spec.date) {
            (Some(value), _) => Some(Chosen::new(value, Origin::Flag)),
            (None, Some(value)) => Some(Chosen::new(
                value,
                Origin::Spec(
                    settings
                        .date
                        .as_ref()
                        .map_or_else(|| letter.content.at.clone(), |d| d.at.clone()),
                ),
            )),
            (None, None) => None,
        },
        style: Chosen::first(explicit.base.style.clone(), style_spec, default_style()),
        paper: Chosen::first(explicit.base.paper, paper_spec, Paper::default()),
    };
    Ok(AppliedLetter {
        choices,
        selection,
        overrides,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Tag;
    use crate::spec::resolve;
    use claims::{assert_err, assert_ok, assert_some};
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
    // -----------------------------------------------------------------------------------------------------------
    // The letter
    // -----------------------------------------------------------------------------------------------------------

    /// A spec in a temporary directory with a `[letter]` table; `build` and `letter` are the text after each header.
    fn letter_spec(build: &str, position: &str, letter: &str) -> (tempfile::TempDir, ResolvedSpec) {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("acme.toml");
        assert_ok!(std::fs::write(
            &path,
            format!(
                "[position]\n{position}\n[build]\ncontent = \"a.md\"\n{build}\n[letter]\ncontent = \"l.md\"\n{letter}\n"
            )
        ));
        let spec = assert_ok!(resolve(&path, None));
        (dir, spec)
    }

    fn letter_allowed(explicit: LetterExplicit) -> Admitted<LetterExplicit> {
        assert_ok!(explicit.admit(OverridePolicy::Allow))
    }

    fn field(text: &str) -> FieldValue {
        assert_ok!(FieldValue::try_new(text))
    }

    fn date(text: &str) -> LetterDate {
        assert_ok!(text.parse::<LetterDate>())
    }

    #[test]
    fn a_letter_spec_with_no_flags_gives_its_own_choices_each_with_the_line_that_chose_it() {
        let (_dir, spec) = letter_spec(
            "style = \"mono\"",
            "company = \"Acme\"\ntitle = \"Engineer\"",
            "recipient = \"Hiring Team\"\ndate = 2028-02-29\nstyle = \"pragmatic\"\npaper = \"a4\"\n\
             output-format = \"both\"\nmax-length = 900\nlength-unit = \"scalars\"\nlinks = \"text-only\"",
        );
        let applied = assert_ok!(apply_letter(&spec, &Admitted::none()));
        let c = &applied.choices;
        assert_eq!(applied.overrides, []);
        assert_eq!(c.output.value, LetterOutput::Both);
        assert_eq!(c.links.value, LinkStyle::TextOnly);
        assert_eq!(c.max_length.as_ref().map(|m| m.value), Some(900));
        assert_eq!(c.length_unit.value, LengthUnit::Scalars);
        assert_eq!(c.paper.value, Paper::A4);
        assert_eq!(c.style.value, assert_ok!(StyleName::try_new("pragmatic")));
        assert_eq!(c.fields().recipient, Some(field("Hiring Team")));
        assert_eq!(c.fields().date, Some(date("2028-02-29")));
        assert_eq!(c.fields().company, Some(field("Acme")));
        assert_eq!(c.fields().title, Some(field("Engineer")));
        // Every value written in `[letter]` names a line of the spec.
        for origin in [
            &c.output.origin,
            &c.links.origin,
            &c.length_unit.origin,
            &c.paper.origin,
            &c.style.origin,
        ] {
            assert!(matches!(origin, Origin::Spec(at) if at.line > 0), "{origin:?}");
        }
        assert!(matches!(
            c.company.as_ref().map(|c| &c.origin),
            Some(Origin::Position(_))
        ));
    }

    #[test]
    fn a_letter_the_spec_says_nothing_about_gets_the_built_in_defaults_and_says_so() {
        let (_dir, spec) = letter_spec("", "", "");
        let c = assert_ok!(apply_letter(&spec, &Admitted::none())).choices;
        assert_eq!(
            (c.output.value, c.links.value, c.length_unit.value, c.paper.value),
            (
                LetterOutput::Text,
                LinkStyle::TextAndUrl,
                LengthUnit::Utf16,
                Paper::default()
            )
        );
        assert_eq!(c.style.value, assert_ok!(StyleName::try_new("pragmatic")));
        for origin in [&c.output.origin, &c.links.origin, &c.style.origin, &c.paper.origin] {
            assert_eq!(origin, &Origin::Default);
        }
        assert!(c.max_length.is_none() && c.company.is_none() && c.recipient.is_none() && c.date.is_none());
    }

    #[test]
    fn style_and_paper_fall_back_to_the_builds_then_the_default_and_say_which() {
        let (_dir, spec) = letter_spec("style = \"mono\"\npaper = \"a4\"", "", "");
        let c = assert_ok!(apply_letter(&spec, &Admitted::none())).choices;
        assert_eq!(c.style.value, assert_ok!(StyleName::try_new("mono")));
        assert_eq!(c.paper.value, Paper::A4);
        assert!(
            matches!(&c.style.origin, Origin::Build(at) if at.line == 5),
            "{:?}",
            c.style.origin
        );
        assert!(
            matches!(&c.paper.origin, Origin::Build(at) if at.line == 6),
            "{:?}",
            c.paper.origin
        );
        // The letter's own value wins over the build's, and the build's is not consulted for the other.
        let (_dir, spec) = letter_spec("style = \"mono\"\npaper = \"a4\"", "", "style = \"pragmatic\"");
        let c = assert_ok!(apply_letter(&spec, &Admitted::none())).choices;
        assert_eq!(c.style.value, assert_ok!(StyleName::try_new("pragmatic")));
        assert!(matches!(c.style.origin, Origin::Spec(_)));
        assert_eq!(c.paper.value, Paper::A4);
        assert!(matches!(c.paper.origin, Origin::Build(_)));
    }

    #[test]
    fn a_letter_flag_replaces_the_spec_value_and_names_the_letter_line_it_replaced() {
        let (_dir, spec) = letter_spec(
            "",
            "company = \"Acme\"",
            "recipient = \"Hiring Team\"\ndate = 2028-02-29\noutput-format = \"both\"\nmax-length = 900\n\
             length-unit = \"scalars\"\nlinks = \"text-only\"",
        );
        let flags = letter_allowed(LetterExplicit {
            output: Some(LetterOutput::Text),
            max_length: Some(100),
            length_unit: Some(LengthUnit::Words),
            links: Some(LinkStyle::TextAndUrl),
            company: Some(field("Other")),
            recipient: Some(field("Dana")),
            date: Some(date("2029-01-01")),
            ..LetterExplicit::default()
        });
        let applied = assert_ok!(apply_letter(&spec, &flags));
        let c = &applied.choices;
        assert_eq!(c.output.value, LetterOutput::Text);
        assert_eq!(c.max_length.as_ref().map(|m| m.value), Some(100));
        assert_eq!(c.length_unit.value, LengthUnit::Words);
        assert_eq!(c.links.value, LinkStyle::TextAndUrl);
        assert_eq!(c.fields().company, Some(field("Other")));
        assert_eq!(c.fields().recipient, Some(field("Dana")));
        assert_eq!(c.fields().date, Some(date("2029-01-01")));
        for origin in [&c.output.origin, &c.length_unit.origin, &c.links.origin] {
            assert_eq!(origin, &Origin::Flag);
        }
        let by_flag: Vec<(OverrideFlag, &Replaced)> = applied.overrides.iter().map(|o| (o.flag, &o.replaced)).collect();
        assert_eq!(by_flag.len(), 7, "{by_flag:?}");
        for (flag, replaced) in by_flag {
            match (flag, replaced) {
                (OverrideFlag::Company, Replaced::Spec { value, at, .. }) => {
                    // `[position]` records no line, so only the value is named.
                    assert_eq!((value.as_slice(), at.len()), (&["Acme".to_string()][..], 0));
                }
                (_, Replaced::Spec { at, .. }) => assert_eq!(at.len(), 1, "{flag}: {replaced:?}"),
                (_, Replaced::Default) => panic!("{flag} replaced a value the spec wrote"),
            }
        }
    }

    #[test]
    fn a_letter_flag_over_nothing_the_spec_said_replaces_the_default() {
        let (_dir, spec) = letter_spec("", "", "");
        let flags = letter_allowed(LetterExplicit {
            links: Some(LinkStyle::TextOnly),
            title: Some(field("Engineer")),
            ..LetterExplicit::default()
        });
        let applied = assert_ok!(apply_letter(&spec, &flags));
        assert!(applied.overrides.iter().all(|o| o.replaced == Replaced::Default));
        assert_eq!(applied.overrides.len(), 2);
    }

    #[test]
    fn a_letter_scope_flag_names_the_letter_select_lines_and_never_the_resumes() {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("acme.toml");
        assert_ok!(std::fs::write(
            &path,
            "[build]\ncontent = \"a.md\"\n[select]\ntags = [\"resume-only\"]\nprefer = [\"resume-way\"]\n\
             [letter]\ncontent = \"l.md\"\n[letter.select]\ntags = [\"ai\"]\nprefer = [\"short\"]\n"
        ));
        let spec = assert_ok!(resolve(&path, None));
        let flags = letter_allowed(LetterExplicit {
            base: Explicit {
                scope: Some(tags(&["platform"])),
                prefer: Some(prefer(&["long"])),
                ..Explicit::default()
            },
            ..LetterExplicit::default()
        });
        let applied = assert_ok!(apply_letter(&spec, &flags));
        let [scope, prefer_override] = applied.overrides.as_slice() else {
            panic!("two overrides expected: {:?}", applied.overrides)
        };
        let Replaced::Spec { setting, value, at } = &scope.replaced else {
            panic!("tags were chosen: {scope:?}")
        };
        assert_eq!((*setting, value.as_slice()), (Setting::Tags, &["ai".to_string()][..]));
        assert_eq!(at.iter().map(|a| a.line).collect::<Vec<_>>(), [9]);
        let Replaced::Spec { value, at, .. } = &prefer_override.replaced else {
            panic!("prefer was chosen")
        };
        assert_eq!((value.as_slice(), at[0].line), (&["short".to_string()][..], 10));
        // The resume's own scope is untouched by the same flags.
        let resume = assert_ok!(apply(
            &spec,
            &allowed(Explicit {
                scope: Some(tags(&["platform"])),
                ..Explicit::default()
            })
        ));
        let Replaced::Spec { value, .. } = &resume.overrides[0].replaced else {
            panic!("resume tags")
        };
        assert_eq!(value.as_slice(), ["resume-only"]);
    }

    #[test]
    fn the_policy_refuses_every_letter_flag_at_once_in_a_fixed_order() {
        let all = LetterExplicit {
            base: Explicit {
                style: Some(assert_ok!(StyleName::try_new("mono"))),
                paper: Some(Paper::A4),
                scope: Some(ScopeFlag::CoreOnly),
                prefer: Some(prefer(&["short"])),
            },
            output: Some(LetterOutput::Pdf),
            max_length: Some(1),
            length_unit: Some(LengthUnit::Words),
            links: Some(LinkStyle::TextOnly),
            company: Some(field("A")),
            title: Some(field("B")),
            recipient: Some(field("C")),
            date: Some(date("2028-01-01")),
        };
        let refused = assert_err!(all.clone().admit(OverridePolicy::Refuse));
        let text = refused.to_string();
        assert_eq!(refused.flags.len(), 12, "{text}");
        assert!(
            text.starts_with(
                "--core-only, --prefer, --style, --paper, --output-format, --max-length, --length-unit, --links, \
                 --company, --title, --recipient and --date cannot be used beside --spec"
            ),
            "{text}"
        );
        assert_ok!(all.admit(OverridePolicy::Allow));
        assert_ok!(LetterExplicit::default().admit(OverridePolicy::Refuse));
    }

    #[test]
    fn each_letter_flag_alone_is_refused_naming_only_itself() {
        let cases: Vec<(OverrideFlag, LetterExplicit)> = vec![
            (
                OverrideFlag::OutputFormat,
                LetterExplicit {
                    output: Some(LetterOutput::Pdf),
                    ..Default::default()
                },
            ),
            (
                OverrideFlag::MaxLength,
                LetterExplicit {
                    max_length: Some(0),
                    ..Default::default()
                },
            ),
            (
                OverrideFlag::LengthUnit,
                LetterExplicit {
                    length_unit: Some(LengthUnit::Utf16),
                    ..Default::default()
                },
            ),
            (
                OverrideFlag::Links,
                LetterExplicit {
                    links: Some(LinkStyle::TextOnly),
                    ..Default::default()
                },
            ),
            (
                OverrideFlag::Company,
                LetterExplicit {
                    company: Some(field("A")),
                    ..Default::default()
                },
            ),
            (
                OverrideFlag::Title,
                LetterExplicit {
                    title: Some(field("A")),
                    ..Default::default()
                },
            ),
            (
                OverrideFlag::Recipient,
                LetterExplicit {
                    recipient: Some(field("A")),
                    ..Default::default()
                },
            ),
            (
                OverrideFlag::Date,
                LetterExplicit {
                    date: Some(date("2028-01-01")),
                    ..Default::default()
                },
            ),
        ];
        for (flag, explicit) in cases {
            assert_eq!(explicit.flags(), [flag]);
            let refused = assert_err!(explicit.admit(OverridePolicy::Refuse));
            assert_eq!(refused.flags, [flag]);
        }
    }

    #[test]
    fn a_flag_cannot_make_a_letter_spec_with_a_broken_recipient_build() {
        let (_dir, spec) = letter_spec("", "", "recipient = \"two\\u2028lines\"");
        let flags = letter_allowed(LetterExplicit {
            recipient: Some(field("Dana")),
            ..LetterExplicit::default()
        });
        assert!(matches!(
            assert_err!(apply_letter(&spec, &flags)),
            SpecError::UnsafeText { .. }
        ));
    }

    #[test]
    fn a_spec_without_a_letter_table_is_no_letter_table_whatever_the_flags() {
        let (_dir, spec) = spec_text("", "");
        for flags in [
            Admitted::none(),
            letter_allowed(LetterExplicit {
                links: Some(LinkStyle::TextOnly),
                ..Default::default()
            }),
        ] {
            assert!(matches!(
                assert_err!(apply_letter(&spec, &flags)),
                SpecError::NoLetterTable
            ));
        }
    }

    #[test]
    fn a_letter_that_asks_for_core_only_and_tags_cannot_be_rescued_by_a_flag() {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("acme.toml");
        assert_ok!(std::fs::write(
            &path,
            "[build]\ncontent = \"a.md\"\n[letter]\ncontent = \"l.md\"\n[letter.select]\ncore-only = true\ntags = [\"ai\"]\n"
        ));
        let spec = assert_ok!(resolve(&path, None));
        let flags = letter_allowed(LetterExplicit {
            base: Explicit {
                scope: Some(tags(&["x"])),
                ..Explicit::default()
            },
            ..LetterExplicit::default()
        });
        assert!(matches!(
            assert_err!(apply_letter(&spec, &flags)),
            SpecError::ScopeConflict { .. }
        ));
    }

    #[test]
    fn the_resume_flag_ids_the_build_probe_asks_about_are_unchanged_by_the_letter_flags() {
        // `OverrideFlag` order decides the order `Refused` lists flags; a letter flag sorts after every resume flag.
        let resume = [
            OverrideFlag::Tags,
            OverrideFlag::CoreOnly,
            OverrideFlag::Prefer,
            OverrideFlag::Style,
            OverrideFlag::Paper,
        ];
        assert!(resume.windows(2).all(|w| w[0] < w[1]));
        assert!(OverrideFlag::Paper < OverrideFlag::OutputFormat);
    }
    /// The lines of a generated `letter_spec`, by the key that starts the line, so a test names a line by its key.
    fn line_of(dir: &tempfile::TempDir, key: &str) -> usize {
        let text = assert_ok!(std::fs::read_to_string(dir.path().join("acme.toml")));
        let found = text
            .lines()
            .position(|l| l.starts_with(key))
            .unwrap_or_else(|| panic!("no line starts with `{key}` in:\n{text}"));
        found + 1
    }

    #[test]
    fn each_letter_flag_names_the_setting_the_value_and_the_line_it_replaced() {
        let (dir, spec) = letter_spec(
            "style = \"mono\"\npaper = \"a4\"",
            "company = \"Acme\"\ntitle = \"Engineer\"",
            "recipient = \"Hiring Team\"\ndate = 2028-02-29\nstyle = \"pragmatic\"\npaper = \"letter\"\n\
             output-format = \"both\"\nmax-length = 900\nlength-unit = \"scalars\"\nlinks = \"text-only\"",
        );
        let flags = letter_allowed(LetterExplicit {
            base: Explicit {
                style: Some(assert_ok!(StyleName::try_new("mono"))),
                paper: Some(Paper::A4),
                ..Explicit::default()
            },
            output: Some(LetterOutput::Text),
            max_length: Some(100),
            length_unit: Some(LengthUnit::Words),
            links: Some(LinkStyle::TextAndUrl),
            company: Some(field("Other")),
            title: Some(field("Staff")),
            recipient: Some(field("Dana")),
            date: Some(date("2029-01-01")),
        });
        let applied = assert_ok!(apply_letter(&spec, &flags));
        /// One override as the report states it: the flag, what it set, and what it replaced and where.
        #[derive(Debug, PartialEq, Eq)]
        struct Row(OverrideFlag, Vec<String>, Setting, Vec<String>, Vec<usize>);
        let got: Vec<Row> = applied
            .overrides
            .iter()
            .map(|o| {
                let Replaced::Spec { setting, value, at } = &o.replaced else {
                    panic!("{} replaced nothing: {o:?}", o.flag)
                };
                Row(
                    o.flag,
                    o.set.clone(),
                    *setting,
                    value.clone(),
                    at.iter().map(|a| a.line).collect(),
                )
            })
            .collect();
        let line = |key: &str| vec![line_of(&dir, key)];
        let v = |s: &str| vec![s.to_string()];
        assert_eq!(
            got,
            [
                Row(
                    OverrideFlag::Style,
                    v("mono"),
                    Setting::Style,
                    v("pragmatic"),
                    line("style = \"pragmatic\"")
                ),
                Row(
                    OverrideFlag::Paper,
                    v("a4"),
                    Setting::Paper,
                    v("letter"),
                    line("paper = \"letter\"")
                ),
                Row(
                    OverrideFlag::OutputFormat,
                    v("text"),
                    Setting::OutputFormat,
                    v("both"),
                    line("output-format")
                ),
                Row(
                    OverrideFlag::MaxLength,
                    v("100"),
                    Setting::MaxLength,
                    v("900"),
                    line("max-length")
                ),
                Row(
                    OverrideFlag::LengthUnit,
                    v("words"),
                    Setting::LengthUnit,
                    v("scalars"),
                    line("length-unit")
                ),
                Row(
                    OverrideFlag::Links,
                    v("text-and-url"),
                    Setting::Links,
                    v("text-only"),
                    line("links")
                ),
                // `[position]` records no line, so only the value it replaced is named.
                Row(OverrideFlag::Company, v("Other"), Setting::Company, v("Acme"), vec![]),
                Row(OverrideFlag::Title, v("Staff"), Setting::Title, v("Engineer"), vec![]),
                Row(
                    OverrideFlag::Recipient,
                    v("Dana"),
                    Setting::Recipient,
                    v("Hiring Team"),
                    line("recipient")
                ),
                Row(
                    OverrideFlag::Date,
                    v("2029-01-01"),
                    Setting::Date,
                    v("2028-02-29"),
                    line("date")
                ),
            ]
        );
        let c = &applied.choices;
        assert_eq!(c.fields().title, Some(field("Staff")));
        assert_eq!(c.fields().company, Some(field("Other")));
        assert_eq!(c.style.value, assert_ok!(StyleName::try_new("mono")));
        assert_eq!(c.paper.value, Paper::A4);
    }

    #[test]
    fn a_flag_over_the_builds_style_or_paper_names_the_build_line_and_not_the_letters() {
        let (dir, spec) = letter_spec("style = \"mono\"\npaper = \"a4\"", "", "");
        let flags = letter_allowed(LetterExplicit {
            base: Explicit {
                style: Some(assert_ok!(StyleName::try_new("pragmatic"))),
                paper: Some(Paper::Letter),
                ..Explicit::default()
            },
            ..LetterExplicit::default()
        });
        let applied = assert_ok!(apply_letter(&spec, &flags));
        let at = |o: &Override| match &o.replaced {
            Replaced::Spec { at, .. } => at.iter().map(|a| a.line).collect::<Vec<_>>(),
            Replaced::Default => panic!("a build value was replaced"),
        };
        let [style, paper] = applied.overrides.as_slice() else {
            panic!("two overrides expected: {:?}", applied.overrides)
        };
        assert_eq!(at(style), [line_of(&dir, "style = \"mono\"")]);
        assert_eq!(at(paper), [line_of(&dir, "paper = \"a4\"")]);
    }

    #[test]
    fn every_origin_a_letter_names_is_the_exact_line_that_wrote_it() {
        let (dir, spec) = letter_spec(
            "",
            "",
            "recipient = \"T\"\ndate = 2028-02-29\nstyle = \"pragmatic\"\npaper = \"a4\"\noutput-format = \"pdf\"\n\
             max-length = 900\nlength-unit = \"words\"\nlinks = \"text-only\"",
        );
        let c = assert_ok!(apply_letter(&spec, &Admitted::none())).choices;
        let line = |origin: &Origin| match origin {
            Origin::Spec(at) => at.line,
            other => panic!("expected a spec line, got {other:?}"),
        };
        assert_eq!(line(&c.output.origin), line_of(&dir, "output-format"));
        assert_eq!(line(&c.links.origin), line_of(&dir, "links"));
        assert_eq!(line(&c.length_unit.origin), line_of(&dir, "length-unit"));
        assert_eq!(line(&c.style.origin), line_of(&dir, "style"));
        assert_eq!(line(&c.paper.origin), line_of(&dir, "paper"));
        assert_eq!(
            line(&assert_some!(c.max_length.as_ref()).origin),
            line_of(&dir, "max-length")
        );
        assert_eq!(
            line(&assert_some!(c.recipient.as_ref()).origin),
            line_of(&dir, "recipient")
        );
        assert_eq!(
            line(&assert_some!(c.date.as_ref()).origin),
            line_of(&dir, "date = 2028")
        );
    }

    #[test]
    fn a_letter_override_changes_the_selection_itself_and_not_only_the_report() {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("acme.toml");
        assert_ok!(std::fs::write(
            &path,
            "[build]\ncontent = \"a.md\"\n[letter]\ncontent = \"l.md\"\n[letter.select]\ntags = [\"ai\"]\nprefer = [\"short\"]\n"
        ));
        let spec = assert_ok!(resolve(&path, None));
        let by_tags = assert_ok!(apply_letter(
            &spec,
            &letter_allowed(LetterExplicit {
                base: Explicit {
                    scope: Some(tags(&["platform"])),
                    prefer: Some(prefer(&["long"])),
                    ..Explicit::default()
                },
                ..LetterExplicit::default()
            })
        ));
        assert_eq!(by_tags.selection.scope, tags_scope(&["platform"]));
        assert_eq!(by_tags.selection.prefer, prefer(&["long"]));
        let by_core = assert_ok!(apply_letter(
            &spec,
            &letter_allowed(LetterExplicit {
                base: Explicit {
                    scope: Some(ScopeFlag::CoreOnly),
                    ..Explicit::default()
                },
                ..LetterExplicit::default()
            })
        ));
        assert_eq!(by_core.selection.scope, Scope::CoreOnly);
        let Replaced::Spec { setting, value, .. } = &by_core.overrides[0].replaced else {
            panic!("the letter chose tags")
        };
        assert_eq!((*setting, value.as_slice()), (Setting::Tags, &["ai".to_string()][..]));
        // Nothing typed: the letter's own scope, never the resume's.
        let own = assert_ok!(apply_letter(&spec, &Admitted::none())).selection;
        assert_eq!(own.scope, tags_scope(&["ai"]));
        assert_eq!(own.prefer, prefer(&["short"]));
    }

    fn tags_scope(names: &[&str]) -> Scope {
        let ScopeFlag::Tags(tags) = tags(names) else {
            unreachable!()
        };
        Scope::Tags(tags)
    }
}

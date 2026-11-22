//! ABOUTME: `--spec` and its companions for `build`, and the probe that tells a typed flag from a clap default.
//! ABOUTME: The probe is part of the parse itself, so every way of parsing a `build` line gets override detection.

use std::collections::BTreeSet;
use std::path::PathBuf;

use clap::parser::ValueSource;
use clap::{ArgMatches, Args, Command, FromArgMatches};

use crate::spec::OverrideFlag;

/// Which of the flags that choose what a spec chooses were typed on the command line.
///
/// `--style` and `--paper` have defaults, so their values cannot say whether they were typed. clap can: the
/// value's source is `CommandLine` (or an environment variable) when it was given, and `DefaultValue` when not.
/// Asking is done here, while the line is parsed, and not afterwards, because a derived `from_arg_matches`
/// takes each value out of the matches as it reads it: this must be flattened before the fields it asks about.
/// `SpecArgs::given` is the first field of the first flattened struct in `BuildArgs` for that reason, and a test
/// pins it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Given(BTreeSet<OverrideFlag>);

impl Given {
    /// Whether `flag` was typed.
    #[must_use]
    pub fn has(&self, flag: OverrideFlag) -> bool {
        self.0.contains(&flag)
    }

    /// The clap argument id of `flag`: its field name in `SelectArgs` or `RenderArgs`.
    ///
    /// A flag only a letter has (`--links`, `--recipient`, and the like) has none here: `build` has no such
    /// argument, and the letter command needs no probe because none of its flags has a default.
    fn id(flag: OverrideFlag) -> Option<&'static str> {
        match flag {
            OverrideFlag::Tags => Some("tags"),
            OverrideFlag::CoreOnly => Some("core_only"),
            OverrideFlag::Prefer => Some("prefer"),
            OverrideFlag::Style => Some("style"),
            OverrideFlag::Paper => Some("paper"),
            OverrideFlag::OutputFormat
            | OverrideFlag::MaxLength
            | OverrideFlag::LengthUnit
            | OverrideFlag::Links
            | OverrideFlag::Company
            | OverrideFlag::Title
            | OverrideFlag::Recipient
            | OverrideFlag::Date => None,
        }
    }

    /// Every flag, so the probe asks about all of them.
    const ALL: [OverrideFlag; 5] = [
        OverrideFlag::Tags,
        OverrideFlag::CoreOnly,
        OverrideFlag::Prefer,
        OverrideFlag::Style,
        OverrideFlag::Paper,
    ];

    fn probe(matches: &ArgMatches) -> Self {
        Self(
            Self::ALL
                .into_iter()
                .filter(|flag| {
                    let Some(id) = Self::id(*flag) else { return false };
                    // `value_source` panics for an id the command does not have (`watch` has no `paper`).
                    matches.try_contains_id(id).unwrap_or(false)
                        && matches
                            .value_source(id)
                            .is_some_and(|source| source != ValueSource::DefaultValue)
                })
                .collect(),
        )
    }

    /// The ids the probe asks about, so a test can check that every one exists on `build`.
    #[cfg(test)]
    pub(crate) fn ids() -> Vec<&'static str> {
        Self::ALL.into_iter().filter_map(Self::id).collect()
    }
}

/// The probe adds no argument of its own; it only reads what the arguments flattened after it will be given.
impl Args for Given {
    fn augment_args(command: Command) -> Command {
        command
    }

    fn augment_args_for_update(command: Command) -> Command {
        command
    }
}

impl FromArgMatches for Given {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        Ok(Self::probe(matches))
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        *self = Self::probe(matches);
        Ok(())
    }
}

/// The options that make `build` and `letter` build from a position spec.
#[derive(Debug, Args)]
pub struct SpecArgs {
    /// Which tailoring flags were typed. Must stay first: see [`Given`].
    #[command(flatten)]
    pub given: Given,

    /// Build from this position spec, which names the content, the style, the paper, and what to keep; `build`
    /// names the PDF after the spec, and `letter` names its letter PDF after it too. Do not also give a FILE.
    #[arg(long, value_name = "FILE")]
    pub spec: Option<PathBuf>,

    /// Where to find the profiles the spec extends; without it, `profiles` beside the spec's directory.
    #[arg(long, value_name = "DIR", requires = "spec")]
    pub profiles: Option<PathBuf>,

    /// Let the flags that choose what the spec chooses replace its choices; the summary lists each. Without it
    /// they are refused, so a build is always what the spec says.
    #[arg(long, requires = "spec")]
    pub allow_overrides: bool,

    /// Print the spec as resolved from its profiles, as one TOML file, and build nothing.
    #[arg(long, requires = "spec")]
    pub print_spec: bool,
}

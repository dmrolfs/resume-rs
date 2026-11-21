//! ABOUTME: What the code says, for the guide's checks to compare with: commands, options, styles, components.
//! ABOUTME: Everything is read from the real definitions in-process; nothing is spawned and nothing is typed in twice.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use claims::assert_ok;
use clap::{ArgAction, CommandFactory};
use regex::Regex;
use resume::cli::Cli;

use crate::root;
use crate::theme_keys;

/// One option or positional argument of a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOption {
    /// `out` for `--out`, or `FILE` for the positional `<FILE>...`.
    pub name: String,
    pub positional: bool,
    pub short: Option<char>,
    /// The values it accepts, when it is an enumeration (`letter`, `a4`).
    pub values: Vec<String>,
    pub default: Option<String>,
    /// It may be given more than once.
    pub repeatable: bool,
    /// It takes a value (`--out <DIR>`), as opposed to being a switch.
    pub takes_value: bool,
}

impl CliOption {
    /// How the guide writes it in the first cell of its row's table: `--out`, or `FILE`.
    pub fn key(&self) -> String {
        if self.positional {
            format!("<{}>", self.name)
        } else {
            format!("--{}", self.name)
        }
    }
}

fn option_of(arg: &clap::Arg) -> CliOption {
    let takes_value = arg.get_action().takes_values();
    let positional = arg.is_positional();
    let name = if positional {
        arg.get_value_names()
            .and_then(|names| names.first())
            .map_or_else(|| arg.get_id().to_string().to_uppercase(), ToString::to_string)
    } else {
        arg.get_long().unwrap_or_default().to_string()
    };
    CliOption {
        name,
        positional,
        short: arg.get_short(),
        values: if takes_value {
            arg.get_possible_values()
                .iter()
                .map(|v| v.get_name().to_string())
                .collect()
        } else {
            Vec::new()
        },
        default: arg
            .get_default_values()
            .first()
            .map(|v| v.to_string_lossy().to_string()),
        repeatable: matches!(arg.get_action(), ArgAction::Append)
            || (positional && arg.get_num_args().is_some_and(|n| n.max_values() > 1)),
        takes_value,
    }
}

/// The commands and their own options, keyed by command name. `help` is not a command here.
pub fn commands() -> BTreeMap<String, Vec<CliOption>> {
    Cli::command()
        .get_subcommands()
        .map(|command| {
            let options = command
                .get_arguments()
                .filter(|a| !a.is_global_set())
                .filter(|a| !matches!(a.get_id().as_str(), "help" | "version"))
                .map(option_of)
                .collect();
            (command.get_name().to_string(), options)
        })
        .collect()
}

/// The options every command accepts.
pub fn global_options() -> Vec<CliOption> {
    Cli::command()
        .get_arguments()
        .filter(|a| a.is_global_set())
        .map(option_of)
        .collect()
}

/// Every short option letter of every command and the global options, with `-h` and `-V`.
pub fn every_short_option() -> BTreeSet<char> {
    let mut all: BTreeSet<char> = global_options().iter().filter_map(|o| o.short).collect();
    for options in commands().values() {
        all.extend(options.iter().filter_map(|o| o.short));
    }
    all.extend(['h', 'V']);
    all
}

/// Every `--long` option of every command, global or not.
pub fn every_long_option() -> BTreeSet<String> {
    let mut all: BTreeSet<String> = global_options().iter().map(|o| o.name.clone()).collect();
    for options in commands().values() {
        all.extend(options.iter().filter(|o| !o.positional).map(|o| o.name.clone()));
    }
    all.extend(["help".to_string(), "version".to_string()]);
    all
}

/// The built-in styles, from the directories under `styles/`.
pub fn built_in_styles() -> Vec<String> {
    assert_ok!(resume::style::catalog(None))
        .into_iter()
        .map(|entry| entry.name)
        .collect()
}

/// The key paths each built-in style's theme defines, normalized as the guide writes them.
pub fn theme_keys_by_style() -> BTreeMap<String, BTreeSet<String>> {
    built_in_styles()
        .into_iter()
        .map(|name| {
            let src = std::fs::read_to_string(root().join(format!("styles/{name}/theme.typ")))
                .unwrap_or_else(|e| panic!("styles/{name}/theme.typ: {e}"));
            (name, theme_keys::normalized(&theme_keys::theme_keys(&src)))
        })
        .collect()
}

/// The font roles the themes declare.
pub fn font_roles() -> BTreeSet<String> {
    built_in_styles()
        .into_iter()
        .flat_map(|name| {
            let src = assert_ok!(std::fs::read_to_string(root().join(format!("styles/{name}/theme.typ"))));
            theme_keys::font_roles(&theme_keys::theme_keys(&src))
        })
        .collect()
}

/// The shared components: every public `#let` of `styles/lib/components.typ`.
pub fn components() -> BTreeSet<String> {
    let src = assert_ok!(std::fs::read_to_string(root().join("styles/lib/components.typ")));
    let public = assert_ok!(Regex::new(r"(?m)^#let ([a-z][a-z0-9-]*)"));
    public.captures_iter(&src).map(|c| c[1].to_string()).collect()
}

/// The bundled font families the program carries besides Typst's own: the directories of `fonts/`.
pub fn bundled_families() -> BTreeSet<String> {
    assert_ok!(std::fs::read_dir(root().join("fonts")))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != "typst-assets")
        .collect()
}

/// The text of a file under the repository root.
pub fn read(relative: impl AsRef<Path>) -> String {
    let path: PathBuf = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

//! ABOUTME: Decides whether diagnostics are colored from `--color`, a TTY check, `NO_COLOR`, and `TERM`.
//! ABOUTME: A pure function over explicit facts, so tests never touch the process environment.

use std::env;
use std::ffi::OsStr;

use codespan_reporting::term::termcolor::ColorChoice;

use crate::model::options::ColorMode;

/// The facts that decide `--color auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ColorEnv {
    /// The output is an interactive terminal.
    pub is_terminal: bool,
    /// `NO_COLOR` is set to a non-empty value.
    pub no_color: bool,
    /// `TERM` is `dumb`.
    pub term_dumb: bool,
}

impl ColorEnv {
    /// Builds the facts from explicit values; `NO_COLOR` counts only when non-empty (no-color.org).
    #[must_use]
    pub fn from_vars(is_terminal: bool, no_color: Option<&OsStr>, term: Option<&OsStr>) -> Self {
        Self {
            is_terminal,
            no_color: no_color.is_some_and(|value| !value.is_empty()),
            term_dumb: term == Some(OsStr::new("dumb")),
        }
    }

    /// Reads `NO_COLOR` and `TERM` from the process; the caller says whether the stream is a terminal.
    #[must_use]
    pub fn detect(is_terminal: bool) -> Self {
        Self::from_vars(
            is_terminal,
            env::var_os("NO_COLOR").as_deref(),
            env::var_os("TERM").as_deref(),
        )
    }
}

/// Resolves `--color` to `Always` or `Never`, never `Auto`, so termcolor cannot re-decide from the
/// environment (it does not check for a terminal, and it treats an empty `NO_COLOR` as set).
#[must_use]
pub fn resolve_color(mode: ColorMode, env: ColorEnv) -> ColorChoice {
    match mode {
        ColorMode::Always => ColorChoice::Always,
        ColorMode::Never => ColorChoice::Never,
        ColorMode::Auto if env.is_terminal && !env.no_color && !env.term_dumb => ColorChoice::Always,
        ColorMode::Auto => ColorChoice::Never,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    fn env(is_terminal: bool, no_color: bool, term_dumb: bool) -> ColorEnv {
        ColorEnv {
            is_terminal,
            no_color,
            term_dumb,
        }
    }

    #[test]
    fn resolve_color_truth_table() {
        for mode in ColorMode::iter() {
            for (is_terminal, no_color, term_dumb) in [false, true].into_iter().flat_map(|a| {
                [false, true]
                    .into_iter()
                    .flat_map(move |b| [false, true].map(move |c| (a, b, c)))
            }) {
                let expected = match mode {
                    ColorMode::Always => ColorChoice::Always,
                    ColorMode::Never => ColorChoice::Never,
                    ColorMode::Auto if is_terminal && !no_color && !term_dumb => ColorChoice::Always,
                    ColorMode::Auto => ColorChoice::Never,
                };
                let got = resolve_color(mode, env(is_terminal, no_color, term_dumb));
                assert_eq!(
                    got, expected,
                    "{mode} terminal={is_terminal} no_color={no_color} dumb={term_dumb}"
                );
            }
        }
    }

    #[test]
    fn the_resolver_never_returns_auto() {
        for mode in ColorMode::iter() {
            assert_ne!(resolve_color(mode, ColorEnv::default()), ColorChoice::Auto);
            assert_ne!(resolve_color(mode, env(true, false, false)), ColorChoice::Auto);
        }
    }

    #[test]
    fn always_wins_over_no_color_and_a_dumb_terminal() {
        assert_eq!(
            resolve_color(ColorMode::Always, env(false, true, true)),
            ColorChoice::Always
        );
    }

    #[test]
    fn no_color_counts_only_when_non_empty() {
        let set = ColorEnv::from_vars(true, Some(OsStr::new("1")), Some(OsStr::new("xterm")));
        assert!(set.no_color);
        assert_eq!(resolve_color(ColorMode::Auto, set), ColorChoice::Never);
        let empty = ColorEnv::from_vars(true, Some(OsStr::new("")), Some(OsStr::new("xterm")));
        assert!(!empty.no_color, "an empty NO_COLOR does not disable color");
        assert_eq!(resolve_color(ColorMode::Auto, empty), ColorChoice::Always);
        assert!(!ColorEnv::from_vars(true, None, None).no_color);
    }

    #[test]
    fn a_dumb_terminal_disables_auto_color() {
        let dumb = ColorEnv::from_vars(true, None, Some(OsStr::new("dumb")));
        assert!(dumb.term_dumb);
        assert_eq!(resolve_color(ColorMode::Auto, dumb), ColorChoice::Never);
        assert!(!ColorEnv::from_vars(true, None, Some(OsStr::new("xterm-256color"))).term_dumb);
        assert!(
            !ColorEnv::from_vars(true, None, None).term_dumb,
            "Windows consoles set no TERM"
        );
        assert_eq!(
            resolve_color(ColorMode::Auto, ColorEnv::from_vars(true, None, None)),
            ColorChoice::Always
        );
    }

    #[test]
    fn a_pipe_disables_auto_color() {
        assert_eq!(
            resolve_color(ColorMode::Auto, ColorEnv::from_vars(false, None, None)),
            ColorChoice::Never
        );
    }

    #[test]
    fn detect_keeps_the_terminal_fact() {
        assert!(ColorEnv::detect(true).is_terminal);
        assert!(!ColorEnv::detect(false).is_terminal);
    }
}

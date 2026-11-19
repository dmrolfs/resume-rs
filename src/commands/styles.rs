//! ABOUTME: `resume styles`: lists the built-in styles and the ones found under `--styles-dir`.
//! ABOUTME: One line per style, aligned, with where it lives and why it cannot be used when it cannot.

use std::path::Path;

use super::emit::Reporter;
use super::{Env, Io, Outcome};
use crate::cli::{Cli, StylesArgs};
use crate::style::{self, StyleEntry, StyleLocation};

/// Prints the catalog to stdout; a styles directory that cannot be read is an error.
pub(super) fn run(cli: &Cli, args: &StylesArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    let mut out = Reporter::new(cli.format, &env.cwd, io);
    match style::catalog(args.styles_dir.as_deref()) {
        Ok(entries) => {
            out.product(&listing(&entries, &env.cwd));
            Outcome::Success
        }
        Err(error) => {
            out.error(&error);
            Outcome::Failure
        }
    }
}

/// One line per style: its name, where it lives, and what is notable about it, with the places lined up.
fn listing(entries: &[StyleEntry], cwd: &Path) -> String {
    let width = entries
        .iter()
        .map(|entry| entry.name.chars().count())
        .max()
        .unwrap_or(0);
    entries
        .iter()
        .map(|entry| {
            let place = match &entry.location {
                StyleLocation::BuiltIn => "built-in".to_string(),
                StyleLocation::Directory(path) => path.strip_prefix(cwd).unwrap_or(path).display().to_string(),
            };
            let note = entry.note.as_ref().map(|note| format!(" ({note})")).unwrap_or_default();
            format!("{:<width$}  {place}{note}\n", entry.name)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn entry(name: &str, location: StyleLocation, note: Option<&str>) -> StyleEntry {
        StyleEntry {
            name: name.to_string(),
            location,
            note: note.map(str::to_string),
        }
    }

    #[test]
    fn the_places_are_lined_up_and_notes_follow_in_parentheses() {
        let cwd = Path::new("/work");
        let entries = [
            entry("pragmatic", StyleLocation::BuiltIn, None),
            entry(
                "mine",
                StyleLocation::Directory(PathBuf::from("/work/styles/mine")),
                None,
            ),
            entry(
                "half",
                StyleLocation::Directory(PathBuf::from("/elsewhere/half")),
                Some("invalid: no style.typ"),
            ),
        ];
        assert_eq!(
            listing(&entries, cwd),
            format!(
                "pragmatic  built-in\nmine       {}\nhalf       /elsewhere/half (invalid: no style.typ)\n",
                Path::new("styles").join("mine").display()
            )
        );
    }

    #[test]
    fn no_styles_is_no_output() {
        assert_eq!(listing(&[], Path::new("/work")), "");
    }
}

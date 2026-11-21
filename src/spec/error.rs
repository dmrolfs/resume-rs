//! ABOUTME: Why a position spec or a profile cannot be loaded or resolved: one typed error per cause.
//! ABOUTME: Each message names the file, and where it can the line, the column, and the key.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use snafu::Snafu;
use strum::Display;

use super::names::{NAME_RULE, PathRule};
use crate::diagnostics::display_text;

/// Where in a file something is: the file as the user named it, and a 1-based line and column.
///
/// The path is shared (`Arc`), not owned: a location is copied into every value of a file, and a `PathBuf` is
/// 24 bytes on macOS and 32 on Windows, which made the same error large on one system and not on the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub path: Arc<Path>,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for Located {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Shown without the `..` that led to a profile (`positions/../profiles/a.toml` is `profiles/a.toml`); the path
        // itself is untouched, because it is what was opened. A path with nothing to remove is shown as it was
        // written, so a user's own separators are not rewritten.
        let has_parent = self.path.components().any(|part| matches!(part, Component::ParentDir));
        let shown = if has_parent {
            tidy_path(&self.path)
        } else {
            self.path.to_path_buf()
        };
        write!(f, "{}:{}:{}", shown.display(), self.line, self.column)
    }
}

/// What kind of name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "lowercase")]
pub enum NameKind {
    Tag,
    Group,
    Variant,
    Profile,
}

/// `path` without the `..` that follow a named part (`positions/../profiles` is `profiles`), for showing only.
///
/// Textual: a link in the path is not followed, so this is never used to find a file. A spec's directory must not be
/// a link, which is also why `positions/..` is the spec's parent.
#[must_use]
pub fn tidy_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            // Nothing is above the root, so `/..` is `/`.
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::RootDir)) => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// A directory that was searched, as an error shows it: made absolute (a spec in the working directory has no
/// directory part, and `./../profiles` would not say which) and tidied.
#[must_use]
pub fn searched_path(path: &Path) -> PathBuf {
    tidy_path(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
}

/// A value of a file as it is safe to print: cut short, with control characters escaped.
#[must_use]
pub fn shown(value: &str) -> String {
    display_text(value, false)
}

/// The size a spec or profile file may have, in bytes.
pub const MAX_FILE_BYTES: u64 = 64 * 1024;
/// How many `extends` levels a chain may nest.
pub const MAX_DEPTH: usize = 8;
/// How many distinct profiles one spec may pull in.
pub const MAX_PROFILES: usize = 32;

/// What is wrong with a `position.date`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum DateProblem {
    /// A time of day or an offset came with it.
    NotJustADate,
    /// The month is not 1 to 12.
    Month,
    /// The day is not in the month.
    Day,
}

impl DateProblem {
    fn explain(self) -> &'static str {
        match self {
            Self::NotJustADate => "must be a date such as 2026-11-19, with no time of day",
            Self::Month => "the month is not 1 to 12",
            Self::Day => "that day does not exist in that month",
        }
    }
}

/// What is wrong with a `position.posting` address that parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum UrlProblem {
    /// It is not an `http` or `https` address.
    NotWeb,
    /// It carries a user name or a password, which a file that is shared and printed must not.
    Credentials,
}

impl UrlProblem {
    fn explain(self) -> &'static str {
        match self {
            Self::NotWeb => "must be an http or https address",
            Self::Credentials => "must not contain a user name or a password",
        }
    }
}

/// What is wrong with a pick that names nothing in the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickProblemKind {
    /// The content has no group of this name.
    UnknownGroup { group: String },
    /// The group exists and has no variant of this name.
    UnknownVariant { group: String, variant: String },
}

impl fmt::Display for PickProblemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownGroup { group } => write!(f, "the content has no group `{group}`"),
            Self::UnknownVariant { group, variant } => write!(f, "the group `{group}` has no wording `{variant}`"),
        }
    }
}

/// One pick that names nothing in the content: where the file wrote it, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickProblem {
    pub at: Located,
    pub kind: PickProblemKind,
}

fn did_you_mean(suggestion: &Option<String>) -> String {
    suggestion
        .as_ref()
        .map_or_else(String::new, |s| format!("; did you mean `{s}`?"))
}

fn names_text(names: &[String]) -> String {
    if names.is_empty() {
        String::from("none")
    } else {
        names.join(", ")
    }
}

fn pick_lines(problems: &[PickProblem]) -> String {
    problems
        .iter()
        .map(|p| format!("{}: {}", p.at, p.kind))
        .collect::<Vec<_>>()
        .join("\n  ")
}

/// Why a spec or profile could not be loaded or resolved.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum SpecError {
    #[snafu(display("cannot read {}: {source}", path.display()))]
    Read { path: PathBuf, source: std::io::Error },

    #[snafu(display("{} is not a regular file", path.display()))]
    NotAFile { path: PathBuf },

    #[snafu(display("{} is {len} bytes; a spec or profile may be at most {MAX_FILE_BYTES}", path.display()))]
    TooLarge { path: PathBuf, len: u64 },

    #[snafu(display("{} is not UTF-8 text (the first bad byte is at offset {valid_up_to})", path.display()))]
    NotUtf8 { path: PathBuf, valid_up_to: usize },

    #[snafu(display("{} is a profile, not a position spec", path.display()))]
    NotASpec { path: PathBuf },

    #[snafu(display("{at}: {message}"))]
    Syntax { at: Located, message: String },

    #[snafu(display(
        "{at}: unknown key `{key}`; allowed here: {}{}",
        names_text(expected),
        did_you_mean(suggestion)
    ))]
    UnknownKey {
        at: Located,
        /// The key's full path, such as `select.tag`.
        key: String,
        /// The keys the table allows, by their own names.
        expected: Vec<String>,
        /// The allowed key it is closest to, as a full path.
        suggestion: Option<String>,
    },

    #[snafu(display("{at}: `{key}`: {message}"))]
    WrongType { at: Located, key: String, message: String },

    #[snafu(display("{at}: `{key}` must be 1 to {max} characters of text"))]
    TextLength { at: Located, key: String, max: usize },

    #[snafu(display("{at}: `{key}` must not contain a control or direction-changing character"))]
    UnsafeText { at: Located, key: String },

    #[snafu(display(
        "{at}: `{value}` is not a style name: lowercase letters, digits, and hyphens, starting with a letter"
    ))]
    InvalidStyle { at: Located, value: String },

    #[snafu(display("{at}: `{value}` is not a paper size; use one of: {}", allowed.join(", ")))]
    InvalidPaper {
        at: Located,
        value: String,
        allowed: Vec<String>,
    },

    #[snafu(display("{at}: `{value}` is not a level in `{key}`; use low, medium, or high"))]
    InvalidLevel { at: Located, key: String, value: String },

    #[snafu(display("{at}: `position.date` {}", problem.explain()))]
    InvalidDate { at: Located, problem: DateProblem },

    #[snafu(display("{at}: `position.posting` is not a web address: {source}"))]
    InvalidUrl { at: Located, source: url::ParseError },

    #[snafu(display("{at}: `position.posting` {}", problem.explain()))]
    RefusedUrl { at: Located, problem: UrlProblem },

    #[snafu(display("{at}: a position spec must have `{key}`"))]
    MissingKey { at: Located, key: &'static str },

    #[snafu(display("{at}: `{value}` is not a valid {kind} name in `{key}`: {NAME_RULE}"))]
    InvalidName {
        at: Located,
        key: String,
        kind: NameKind,
        value: String,
    },

    #[snafu(display("{at}: `{value}` is not a usable path in `{key}`: {}", rule.explain()))]
    InvalidPath {
        at: Located,
        key: &'static str,
        value: String,
        rule: PathRule,
    },

    #[snafu(display("{at}: a profile cannot have `{key}`: {reason}"))]
    ForbiddenInProfile {
        at: Located,
        key: &'static str,
        reason: &'static str,
    },

    #[snafu(display("{at}: `{value}` appears more than once in `{key}`"))]
    DuplicateEntry { at: Located, key: String, value: String },

    #[snafu(display("{} has no directory of profiles; pass `--profiles DIR` (looked for {})", spec.display(), searched.display()))]
    ProfilesDirectoryMissing { spec: PathBuf, searched: PathBuf },

    #[snafu(display("`--profiles {}` is not a directory", given.display()))]
    ProfilesPathNotADirectory { given: PathBuf },

    #[snafu(display(
        "{at}: no profile `{name}` in {}; available profiles: {}",
        searched.display(),
        names_text(available)
    ))]
    MissingProfile {
        at: Located,
        name: String,
        searched: PathBuf,
        available: Vec<String>,
    },

    #[snafu(display("{at}: the profile `{name}` is {} and resolves outside the profiles directory", path.display()))]
    ProfileOutsideRoot { at: Located, name: String, path: PathBuf },

    #[snafu(display(
        "{at}: the profile `{name}` is in a file called `{found}`; profile files are named in lowercase, so rename it to `{name}.toml`"
    ))]
    ProfileNameCase { at: Located, name: String, found: String },

    #[snafu(display("{at}: profiles extend each other in a circle: {}", path.join(" -> ")))]
    Cycle {
        /// The entry that closes the circle.
        at: Located,
        path: Vec<String>,
    },

    #[snafu(display(
        "{at}: `{listed_first}` is listed before `{listed_after}` in `extends`, but `{listed_after}` has to be applied first ({}); later entries are more specific, so list `{listed_after}` first",
        must_come_first.join(" then ")
    ))]
    ExtendsOrderConflict {
        at: Located,
        listed_first: String,
        listed_after: String,
        /// The files from `listed_after` to `listed_first`, in the order the facts require them.
        must_come_first: Vec<String>,
    },

    #[snafu(display(
        "{core_only_at}: `core-only` is true, but {tags_at} also chooses tags; a build keeps either the core items or the tagged ones"
    ))]
    ScopeConflict { tags_at: Located, core_only_at: Located },

    #[snafu(display(
        "{} pick{} nothing in the content file:\n  {}",
        problems.len(),
        if problems.len() == 1 { " names" } else { "s name" },
        pick_lines(problems)
    ))]
    UnknownPicks { problems: Vec<PickProblem> },

    #[snafu(display("{at}: `extends` nests more than {MAX_DEPTH} levels deep"))]
    TooDeep { at: Located },

    #[snafu(display("{at}: more than {MAX_PROFILES} profiles are needed"))]
    TooManyProfiles { at: Located },
}

impl SpecError {
    /// What the user can do about this error, when there is something specific.
    #[must_use]
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::TooLarge { .. } => Some("a spec or profile is a short list of choices, not a resume"),
            Self::MissingProfile { .. } | Self::ProfilesDirectoryMissing { .. } => {
                Some("profiles live in a `profiles` directory beside `positions`; use --profiles DIR for another place")
            }
            Self::ProfilesPathNotADirectory { .. } => {
                Some("give --profiles the directory that holds the profile files")
            }
            Self::InvalidName { .. } => Some("names never change case and never contain spaces or underscores"),
            Self::ExtendsOrderConflict { .. } => Some("the list runs from least to most specific"),
            Self::ScopeConflict { .. } => Some("set `core-only = false` in the more specific file, or remove the tags"),
            Self::ProfileNameCase { .. } => Some("a profile name is the file name without `.toml`, in lowercase"),
            Self::UnknownPicks { .. } => Some("fix the name in the profile or spec that wrote the pick"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_parent_part_cancels_the_named_part_before_it_and_only_that() {
        let cases = [
            ("two/positions/../profiles/a.toml", "two/profiles/a.toml"),
            ("a/../../b", "../b"),
            ("../../a", "../../a"),
            ("../a", "../a"),
            ("./a/./b", "a/b"),
            ("a/b/../../c", "c"),
            ("/../a", "/a"),
        ];
        for (path, shown) in cases {
            assert_eq!(tidy_path(Path::new(path)), Path::new(shown), "{path}");
        }
    }

    #[test]
    fn a_location_without_a_parent_part_is_shown_exactly_as_written() {
        // Mixed separators, as a path joined by hand has on Windows: nothing to remove, so nothing is rewritten.
        let place = Located {
            path: Path::new("positions/sub\\acme.toml").into(),
            line: 1,
            column: 2,
        };
        assert_eq!(
            place.to_string(),
            format!("{}:1:2", Path::new("positions/sub\\acme.toml").display())
        );
    }

    #[test]
    fn a_location_is_shown_without_the_parent_part_that_led_to_its_file_and_keeps_its_path() {
        let place = Located {
            path: Path::new("positions/../profiles/a.toml").into(),
            line: 3,
            column: 9,
        };
        assert_eq!(
            place.to_string(),
            format!("{}:3:9", Path::new("profiles").join("a.toml").display())
        );
        assert_eq!(
            place.path.as_ref(),
            Path::new("positions/../profiles/a.toml"),
            "what was opened is untouched"
        );
    }

    fn at() -> Located {
        Located {
            path: Path::new("positions/acme.toml").into(),
            line: 8,
            column: 1,
        }
    }

    #[test]
    fn a_location_prints_as_file_line_column() {
        assert_eq!(at().to_string(), "positions/acme.toml:8:1");
    }

    #[test]
    fn an_unknown_key_names_the_full_path_the_allowed_keys_and_the_close_one() {
        let error = SpecError::UnknownKey {
            at: at(),
            key: "select.tag".into(),
            expected: vec!["tags".into(), "pick".into()],
            suggestion: Some("select.tags".into()),
        };
        assert_eq!(
            error.to_string(),
            "positions/acme.toml:8:1: unknown key `select.tag`; allowed here: tags, pick; did you mean `select.tags`?"
        );
    }

    #[test]
    fn an_unknown_key_at_the_top_has_no_suggestion_when_nothing_is_close() {
        let error = SpecError::UnknownKey {
            at: at(),
            key: "selct".into(),
            expected: vec!["select".into()],
            suggestion: None,
        };
        assert_eq!(
            error.to_string(),
            "positions/acme.toml:8:1: unknown key `selct`; allowed here: select"
        );
    }

    #[test]
    fn a_cycle_is_located_and_prints_the_way_round_and_a_missing_profile_lists_what_exists() {
        let cycle = SpecError::Cycle {
            at: at(),
            path: ["a", "b", "c", "a"].map(String::from).to_vec(),
        };
        assert_eq!(
            cycle.to_string(),
            "positions/acme.toml:8:1: profiles extend each other in a circle: a -> b -> c -> a"
        );
        let missing = SpecError::MissingProfile {
            at: at(),
            name: "nope".into(),
            searched: PathBuf::from("profiles"),
            available: vec![],
        };
        assert!(missing.to_string().ends_with("available profiles: none"), "{missing}");
        assert!(missing.hint().is_some());
    }

    #[test]
    fn echoed_text_cannot_carry_a_control_character_to_the_terminal() {
        assert_eq!(shown("ok\u{1b}[31m"), "ok\\u{1b}[31m");
    }

    #[test]
    fn the_errors_a_person_can_fix_have_a_hint() {
        let l = at();
        let with_hint = [
            SpecError::TooLarge {
                path: PathBuf::from("a"),
                len: 1,
            },
            SpecError::ProfilesDirectoryMissing {
                spec: PathBuf::from("a"),
                searched: PathBuf::from("b"),
            },
            SpecError::ProfilesPathNotADirectory {
                given: PathBuf::from("b"),
            },
            SpecError::InvalidName {
                at: l.clone(),
                key: "k".into(),
                kind: NameKind::Tag,
                value: "V".into(),
            },
            SpecError::ExtendsOrderConflict {
                at: l.clone(),
                listed_first: "b".into(),
                listed_after: "a".into(),
                must_come_first: vec![],
            },
            SpecError::ScopeConflict {
                tags_at: l.clone(),
                core_only_at: l.clone(),
            },
            SpecError::ProfileNameCase {
                at: l,
                name: "a".into(),
                found: "A.toml".into(),
            },
            SpecError::UnknownPicks { problems: vec![] },
        ];
        for error in with_hint {
            assert!(error.hint().is_some(), "{error:?}");
        }
    }

    #[test]
    fn the_error_is_small_enough_to_return_by_value_on_every_platform() {
        // clippy refuses a `Result` whose error is 128 bytes or more. The largest variant holds one `PathBuf`, which is
        // 8 bytes larger on Windows, so the limit here is what keeps the Windows size under the line: a field added
        // on macOS fails this test instead of failing only the Windows build.
        let size = std::mem::size_of::<SpecError>();
        let limit = if cfg!(windows) { 120 } else { 112 };
        assert!(size <= limit, "SpecError is {size} bytes");
    }

    #[test]
    fn a_read_error_keeps_the_cause_the_system_gave() {
        let error = SpecError::Read {
            path: PathBuf::from("p.toml"),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        };
        assert!(error.to_string().starts_with("cannot read p.toml: "), "{error}");
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn a_pick_problem_reads_as_one_line_naming_the_group_or_the_wording() {
        let problems = vec![
            PickProblem {
                at: at(),
                kind: PickProblemKind::UnknownGroup { group: "nope".into() },
            },
            PickProblem {
                at: at(),
                kind: PickProblemKind::UnknownVariant {
                    group: "gateway".into(),
                    variant: "huge".into(),
                },
            },
        ];
        let text = SpecError::UnknownPicks { problems }.to_string();
        assert!(
            text.starts_with("2 picks name nothing in the content file:\n  "),
            "{text}"
        );
        assert!(text.contains("the content has no group `nope`"), "{text}");
        assert!(text.contains("the group `gateway` has no wording `huge`"), "{text}");
    }
}

//! ABOUTME: Validated values of spec and profile files: profile names, position labels, emphasis levels, content paths.
//! ABOUTME: Pure and platform-independent, so a path rule means the same on macOS and Windows.

use nutype::nutype;
use strum::{Display, EnumIter, EnumString};

use crate::diagnostics::has_unsafe_characters;

/// The sentence that says what a tag, group, variant, or profile name may be. Parser errors say the same.
pub const NAME_RULE: &str =
    "names are lowercase letters and digits, starting with a letter, with single hyphens, at most 40 characters";

/// Names Windows reserves for devices: a file called `con.toml` would open the console.
fn is_not_device_name(name: &str) -> bool {
    let reserved = matches!(name, "con" | "prn" | "aux" | "nul");
    // `com1` to `com9`, and the superscript forms Windows reserves as well (`com¹`, `lpt²`).
    let numbered = ["com", "lpt"].iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|rest| {
            let mut chars = rest.chars();
            matches!((chars.next(), chars.next()), (Some('1'..='9' | '¹' | '²' | '³'), None))
        })
    });
    !(reserved || numbered)
}

/// The name of a profile, which is also its file name without `.toml`.
///
/// The grammar is the annotation name grammar. It leaves no room for a separator, a drive letter, a dot or a
/// capital, so a profile name cannot name a path outside the profiles directory or two files on a
/// case-insensitive disk. Windows device names are refused as well.
#[nutype(
    validate(len_char_max = 40, regex = "^[a-z](-?[a-z0-9])*$", predicate = is_not_device_name),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, AsRef, TryFrom)
)]
pub struct ProfileName(String);

/// Trims the ends and turns every run of whitespace into one space.
fn collapse(text: String) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A short free-text label of a position: its level, domain, or audience.
#[nutype(
    sanitize(with = collapse),
    validate(not_empty, len_char_max = 80),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, TryFrom)
)]
pub struct PositionLabel(String);

/// How strongly a position stresses business, organization, or technical matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, EnumString, EnumIter)]
#[strum(serialize_all = "lowercase")]
pub enum Level {
    Low,
    Medium,
    High,
}

/// Why a content path is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum PathRule {
    Empty,
    TooLong,
    Absolute,
    Backslash,
    EmptyComponent,
    ForbiddenCharacter,
    DeviceName,
    TrailingDotOrSpace,
    EndsInDirectory,
}

impl PathRule {
    /// The sentence a user can act on.
    #[must_use]
    pub fn explain(self) -> &'static str {
        match self {
            Self::Empty => "the path is empty",
            Self::TooLong => "the path is longer than 255 characters",
            Self::Absolute => "write the path relative to the file that holds it, such as `../baselines/baseline.md`",
            Self::Backslash => "separate the parts with `/`, which works on every system",
            Self::EmptyComponent => "the path has an empty part (`//` or a trailing `/`)",
            Self::ForbiddenCharacter => "a part contains a character that some systems refuse in file names",
            Self::DeviceName => "a part is a name Windows reserves for devices (`con`, `nul`, `com1`, and so on)",
            Self::TrailingDotOrSpace => "a part ends with a dot or a space, which Windows drops from file names",
            Self::EndsInDirectory => "the path ends in `.` or `..`, which is a directory, not a file",
        }
    }
}

/// A content path as the file declares it: relative, forward slashes only, valid on every system.
///
/// The path is checked as text, not as a file, so a path that works on macOS cannot fail on Windows and the
/// reverse. `..` is allowed, because a spec in `positions/` names a baseline in `baselines/`. The path is never
/// canonicalized and never opened here.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclaredPath {
    text: String,
    parts: Vec<String>,
}

const MAX_PATH_CHARS: usize = 255;

impl DeclaredPath {
    /// Checks `text` against the rules, and returns the first rule it breaks.
    ///
    /// # Errors
    /// The [`PathRule`] that `text` breaks.
    pub fn parse(text: &str) -> Result<Self, PathRule> {
        if text.is_empty() {
            return Err(PathRule::Empty);
        }
        if text.chars().count() > MAX_PATH_CHARS {
            return Err(PathRule::TooLong);
        }
        if text.contains('\\') {
            return Err(PathRule::Backslash);
        }
        let bytes = text.as_bytes();
        let has_drive = bytes.get(1) == Some(&b':') && bytes.first().is_some_and(u8::is_ascii_alphabetic);
        if text.starts_with('/') || has_drive {
            return Err(PathRule::Absolute);
        }
        let mut parts = Vec::new();
        for part in text.split('/') {
            check_part(part)?;
            if part != "." {
                parts.push(part.to_string());
            }
        }
        if matches!(text.rsplit('/').next(), Some("." | "..")) {
            return Err(PathRule::EndsInDirectory);
        }
        Ok(Self {
            text: text.to_string(),
            parts,
        })
    }

    /// The path as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The path joined onto `base`, one part at a time, so no separator is typed by hand and `..` is kept as
    /// written: it is not collapsed, because a link in the path would make that wrong.
    #[must_use]
    pub fn resolve_against(&self, base: &std::path::Path) -> std::path::PathBuf {
        let mut path = base.to_path_buf();
        path.extend(&self.parts);
        path
    }
}

fn check_part(part: &str) -> Result<(), PathRule> {
    if part.is_empty() {
        return Err(PathRule::EmptyComponent);
    }
    if part == "." || part == ".." {
        return Ok(());
    }
    if has_unsafe_characters(part) || part.chars().any(|c| "*?\"<>|:".contains(c)) {
        return Err(PathRule::ForbiddenCharacter);
    }
    if part.ends_with('.') || part.ends_with(' ') {
        return Err(PathRule::TrailingDotOrSpace);
    }
    // A device name is reserved with or without an extension: `con.md` is as bad as `con`.
    let stem = part.split('.').next().unwrap_or(part).to_ascii_lowercase();
    if !is_not_device_name(&stem) {
        return Err(PathRule::DeviceName);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::path::Path;
    use std::str::FromStr;

    #[test]
    fn a_profile_name_follows_the_annotation_name_grammar() {
        for good in ["principal", "ai-platform", "c-suite-2", "a"] {
            assert_ok!(ProfileName::try_new(good), "{good}");
        }
        for bad in [
            "",
            "Principal",
            "ai_platform",
            "2ai",
            "-ai",
            "ai-",
            "ai--ml",
            "a.b",
            "a/b",
            "..",
            "../x",
            "x/y",
            "C:x",
            "café",
        ] {
            assert_err!(ProfileName::try_new(bad), "{bad:?}");
        }
        assert_ok!(ProfileName::try_new("a".repeat(40)));
        assert_err!(ProfileName::try_new("a".repeat(41)));
    }

    #[test]
    fn a_profile_name_is_never_a_windows_device_name() {
        for device in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9"] {
            assert_err!(ProfileName::try_new(device), "{device}");
        }
        for fine in ["com", "com0", "com10", "console", "lpt", "nullable", "auxiliary"] {
            assert_ok!(ProfileName::try_new(fine), "{fine}");
        }
    }

    #[test]
    fn a_position_label_collapses_whitespace_and_must_not_be_empty() {
        assert_eq!(
            assert_ok!(PositionLabel::try_new("  AI \n platform ")).as_ref(),
            "AI platform"
        );
        assert_err!(PositionLabel::try_new("  "));
        assert_err!(PositionLabel::try_new("x".repeat(81)));
    }

    #[test]
    fn an_emphasis_level_is_one_of_three_lowercase_words_in_order() {
        assert_eq!(Level::from_str("low"), Ok(Level::Low));
        assert_eq!(Level::from_str("high"), Ok(Level::High));
        assert_err!(Level::from_str("High"));
        assert_err!(Level::from_str("extreme"));
        assert!(Level::Low < Level::Medium && Level::Medium < Level::High);
    }

    #[test]
    fn a_portable_relative_path_is_accepted() {
        for good in [
            "baseline.md",
            "../baselines/baseline.md",
            "./baseline.md",
            "../../a/b.md",
            "résumé/Ünï.md",
            "my baseline.md",
            "baseline.txt",
            "no-extension",
        ] {
            assert_ok!(DeclaredPath::parse(good), "{good}");
        }
    }

    #[test]
    fn a_path_that_is_not_portable_is_refused_with_the_rule_it_breaks() {
        let cases = [
            ("", PathRule::Empty),
            ("/etc/passwd", PathRule::Absolute),
            ("C:\\x.md", PathRule::Backslash),
            ("C:x.md", PathRule::Absolute),
            ("c:/x.md", PathRule::Absolute),
            ("\\\\h\\s\\x.md", PathRule::Backslash),
            ("a\\b.md", PathRule::Backslash),
            ("a//b.md", PathRule::EmptyComponent),
            ("a/", PathRule::EmptyComponent),
            ("a/b?.md", PathRule::ForbiddenCharacter),
            ("a/b|c.md", PathRule::ForbiddenCharacter),
            ("a/b\u{7}.md", PathRule::ForbiddenCharacter),
            ("a/con.md", PathRule::DeviceName),
            ("a/NUL", PathRule::DeviceName),
            ("com1/x.md", PathRule::DeviceName),
            ("a/b.", PathRule::TrailingDotOrSpace),
            ("a/b /c.md", PathRule::TrailingDotOrSpace),
            ("a/b\u{202e}.md", PathRule::ForbiddenCharacter),
            ("a/b\u{200f}c.md", PathRule::ForbiddenCharacter),
            ("a/com\u{b9}.md", PathRule::DeviceName),
            (".", PathRule::EndsInDirectory),
            ("a/..", PathRule::EndsInDirectory),
            ("../baselines/.", PathRule::EndsInDirectory),
        ];
        for (text, rule) in cases {
            assert_eq!(DeclaredPath::parse(text), Err(rule), "{text:?}");
        }
        assert_eq!(DeclaredPath::parse(&"a".repeat(256)), Err(PathRule::TooLong));
    }

    #[test]
    fn a_path_resolves_against_a_directory_part_by_part_and_keeps_dot_dot() {
        let path = assert_ok!(DeclaredPath::parse("../baselines/./baseline.md"));
        let resolved = path.resolve_against(Path::new("specs/positions"));
        assert_eq!(
            resolved
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["specs", "positions", "..", "baselines", "baseline.md"]
        );
        assert_eq!(path.as_str(), "../baselines/./baseline.md");
    }

    #[test]
    fn every_path_rule_has_a_sentence() {
        for rule in [
            PathRule::Empty,
            PathRule::TooLong,
            PathRule::Absolute,
            PathRule::Backslash,
            PathRule::EmptyComponent,
            PathRule::ForbiddenCharacter,
            PathRule::DeviceName,
            PathRule::TrailingDotOrSpace,
            PathRule::EndsInDirectory,
        ] {
            assert!(!rule.explain().is_empty(), "{rule}");
        }
    }

    #[test]
    fn a_path_may_have_255_characters_counted_as_characters_and_no_more() {
        assert_ok!(DeclaredPath::parse(&"a".repeat(255)));
        assert_ok!(DeclaredPath::parse(&"é".repeat(255)));
        assert_eq!(DeclaredPath::parse(&"é".repeat(256)), Err(PathRule::TooLong));
    }

    #[test]
    fn every_character_windows_refuses_in_a_name_is_refused() {
        for c in ['*', '?', '"', '<', '>', '|', ':'] {
            assert_eq!(
                DeclaredPath::parse(&format!("a/b{c}c.md")),
                Err(PathRule::ForbiddenCharacter),
                "{c:?}"
            );
        }
    }

    #[test]
    fn windows_device_names_with_a_superscript_digit_are_refused_in_paths_and_profile_names() {
        for name in ["com\u{b9}.md", "lpt\u{b2}", "COM\u{b3}.txt"] {
            assert_eq!(
                DeclaredPath::parse(&format!("a/{name}")),
                Err(PathRule::DeviceName),
                "{name}"
            );
        }
        // A profile name is ASCII only, so these are refused by the grammar; the predicate says the same.
        assert!(!is_not_device_name("com\u{b9}"));
        assert!(is_not_device_name("com0") && is_not_device_name("com10") && is_not_device_name("lptx"));
    }

    proptest! {
        #[test]
        fn an_accepted_path_carries_no_windows_or_unix_hazard(text in "[a-zA-Z0-9 ./\\\\:*?<>|\"_é-]{0,40}") {
            if let Ok(path) = DeclaredPath::parse(&text) {
                prop_assert!(!text.contains('\\'));
                prop_assert!(!text.starts_with('/'));
                prop_assert!(!text.as_bytes().get(1).is_some_and(|b| *b == b':'));
                let mut kept = Vec::new();
                for part in text.split('/') {
                    prop_assert!(!part.is_empty());
                    if part != "." && part != ".." {
                        prop_assert!(!part.ends_with('.') && !part.ends_with(' '), "{part:?}");
                        prop_assert!(!part.chars().any(|c| "*?\"<>|:".contains(c) || c.is_control()), "{part:?}");
                        let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
                        prop_assert!(!["con", "prn", "aux", "nul"].contains(&stem.as_str()), "{part:?}");
                    }
                    if part != "." {
                        kept.push(part.to_string());
                    }
                }
                prop_assert!(!matches!(text.rsplit('/').next(), Some("." | "..")));
                let resolved = path.resolve_against(Path::new("base"));
                let tail: Vec<String> = resolved
                    .components()
                    .skip(1)
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                prop_assert_eq!(tail, kept);
            }
        }
    }
}

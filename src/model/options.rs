//! ABOUTME: Option enums and bounded values shared by the CLI and the renderer.
//! ABOUTME: Built-in styles, paper sizes, output formats, and the `--max-pages` limit.

use nutype::nutype;
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// A style embedded in the binary, listed by `resume styles`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Display, EnumString, AsRefStr, EnumIter)]
#[strum(serialize_all = "kebab-case")]
pub enum BuiltinStyle {
    Pragmatic,
    Mono,
    Experienced,
}

/// Paper size; the `AsRefStr` value is Typst's `page(paper: ..)` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Display, AsRefStr)]
pub enum Paper {
    #[default]
    #[strum(serialize = "us-letter")]
    Letter,
    #[strum(serialize = "a4")]
    A4,
}

/// How diagnostics are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Display)]
#[strum(serialize_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

/// The `--max-pages` limit, 1 to 20.
#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 20),
    derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Display, FromStr, TryFrom, Into)
)]
pub struct PageLimit(u8);

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;

    #[test]
    fn paper_as_ref_str_matches_typst_names() {
        assert_eq!(Paper::Letter.as_ref(), "us-letter");
        assert_eq!(Paper::A4.as_ref(), "a4");
        assert_eq!(Paper::default(), Paper::Letter);
    }

    #[test]
    fn builtin_style_round_trips_kebab_case() {
        assert_eq!(BuiltinStyle::Experienced.to_string(), "experienced");
        assert_eq!(assert_ok!("mono".parse::<BuiltinStyle>()), BuiltinStyle::Mono);
    }

    #[test]
    fn output_format_displays_lowercase() {
        assert_eq!(OutputFormat::Json.to_string(), "json");
        assert_eq!(OutputFormat::default(), OutputFormat::Human);
    }

    #[test]
    fn page_limit_rejects_zero() {
        assert_err!(PageLimit::try_new(0));
        assert_err!(PageLimit::try_new(21));
        assert_ok!(PageLimit::try_new(1));
        assert_ok!(PageLimit::try_new(20));
        assert_eq!(assert_ok!("3".parse::<PageLimit>()).into_inner(), 3);
    }
}

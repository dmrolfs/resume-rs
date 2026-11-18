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

/// When diagnostics are colored (`--color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Display, EnumString, AsRefStr, EnumIter)]
#[strum(serialize_all = "lowercase")]
pub enum ColorMode {
    /// Color only when writing to an interactive terminal that allows it.
    #[default]
    Auto,
    Always,
    Never,
}

/// Which fonts a render may use (`--fonts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Display, EnumString, AsRefStr, EnumIter)]
#[strum(serialize_all = "lowercase")]
pub enum FontsMode {
    /// Bundled fonts, then `--font-path`, `RESUME_FONT_PATH`, application fonts, and system fonts.
    #[default]
    Auto,
    /// Only the bundled fonts, so the output does not depend on the machine.
    Bundled,
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
    use claims::{assert_err, assert_ok, assert_some};
    use clap::ValueEnum;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

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
    fn color_mode_names_match_clap_and_strum() {
        for mode in ColorMode::iter() {
            let clap_name = assert_some!(mode.to_possible_value()).get_name().to_string();
            assert_eq!(clap_name, mode.to_string());
            assert_eq!(mode.as_ref(), mode.to_string());
            assert_eq!(assert_ok!(clap_name.parse::<ColorMode>()), mode);
        }
        assert_eq!(ColorMode::default(), ColorMode::Auto);
    }

    #[test]
    fn fonts_mode_names_match_clap_and_strum() {
        for mode in FontsMode::iter() {
            let clap_name = assert_some!(mode.to_possible_value()).get_name().to_string();
            assert_eq!(clap_name, mode.to_string());
            assert_eq!(mode.as_ref(), mode.to_string());
            assert_eq!(assert_ok!(clap_name.parse::<FontsMode>()), mode);
        }
        assert_eq!(FontsMode::default(), FontsMode::Auto);
        assert_eq!(FontsMode::Bundled.to_string(), "bundled");
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

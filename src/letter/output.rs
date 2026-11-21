//! ABOUTME: Letter product modes, distinct from the global diagnostic format.
//! ABOUTME: PDF variants are reserved but rejected by the text-only command until implemented.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum LetterOutput {
    #[default]
    Text,
    #[value(hide = true)]
    Pdf,
    #[value(hide = true)]
    Both,
}

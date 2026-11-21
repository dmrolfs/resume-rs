//! ABOUTME: Letter product modes, distinct from the global diagnostic format.
//! ABOUTME: Text is stdout, PDF is an atomic file, and both commits the PDF before printing text.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum LetterOutput {
    #[default]
    Text,
    Pdf,
    Both,
}

//! ABOUTME: Explicit letter CLI settings, resolved defaults and text-mode usage checks.
//! ABOUTME: Render options are syntax-checked but never applied by the text command.
use crate::letter::{FieldValue, LengthUnit, LetterDate, LetterFields, LetterOutput, LinkStyle, TextLimit};
use crate::model::{
    StyleName,
    options::{FontsMode, OutputFormat, PageLimit, Paper},
};
use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct LetterArgs {
    /// The Markdown letter to print.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,
    /// Product format; resolves to text when omitted.
    #[arg(long, value_enum)]
    pub output_format: Option<LetterOutput>,
    /// PDF directory; resolves to out when omitted, ignored for text.
    #[arg(short, long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Company field, inserted literally into selected text.
    #[arg(long,value_parser=parse_field)]
    pub company: Option<FieldValue>,
    /// Position title field, inserted literally.
    #[arg(long,value_parser=parse_field)]
    pub title: Option<FieldValue>,
    /// Recipient field, inserted literally.
    #[arg(long,value_parser=parse_field)]
    pub recipient: Option<FieldValue>,
    /// Explicit ISO date, retained for PDF output; not printed in website text.
    #[arg(long)]
    pub date: Option<LetterDate>,
    /// How links print; effective default is text-and-url.
    #[arg(long, value_enum)]
    pub links: Option<LinkStyle>,
    /// Inclusive maximum text length; zero is valid.
    #[arg(long)]
    pub max_length: Option<usize>,
    /// Counting unit; effective default is utf16.
    #[arg(long, value_enum, requires = "max_length")]
    pub length_unit: Option<LengthUnit>,
    #[command(flatten)]
    pub render: LetterRenderArgs,
}

#[derive(Debug, Args)]
pub struct LetterRenderArgs {
    /// Style name, ignored for text output.
    #[arg(short, long)]
    pub style: Option<StyleName>,
    /// Style directory, ignored for text output.
    #[arg(long)]
    pub styles_dir: Option<PathBuf>,
    /// Paper size, ignored for text output.
    #[arg(long, value_enum)]
    pub paper: Option<Paper>,
    /// Page limit, ignored for text output.
    #[arg(long)]
    pub max_pages: Option<PageLimit>,
    /// Font mode, ignored for text output.
    #[arg(long, value_enum)]
    pub fonts: Option<FontsMode>,
    /// Font directory, ignored for text output; repeatable.
    #[arg(long = "font-path")]
    pub font_path: Vec<PathBuf>,
}

pub struct LetterTextSettings {
    pub fields: LetterFields,
    pub links: LinkStyle,
    pub limit: Option<TextLimit>,
}
impl LetterArgs {
    /// Checks value contradictions without filesystem or renderer access, including direct library callers.
    pub(crate) fn validate(&self, format: OutputFormat) -> Result<(), &'static str> {
        if self.render.fonts == Some(FontsMode::Bundled) && !self.render.font_path.is_empty() {
            return Err("--font-path cannot be used with --fonts bundled");
        }
        if format == OutputFormat::Json && self.output_format.unwrap_or_default() != LetterOutput::Pdf {
            return Err("--format json cannot share stdout with letter text");
        }
        if self.length_unit.is_some() && self.max_length.is_none() {
            return Err("--length-unit requires --max-length");
        }
        Ok(())
    }
    /// Resolves render values only when PDF is requested; original options remain untouched.
    #[must_use]
    pub fn effective_render(&self) -> super::RenderArgs {
        super::RenderArgs {
            style: self
                .render
                .style
                .clone()
                .unwrap_or_else(|| StyleName::try_new("pragmatic").expect("valid built-in name")),
            styles_dir: self.render.styles_dir.clone(),
            paper: self.render.paper.unwrap_or_default(),
            max_pages: Some(
                self.render
                    .max_pages
                    .unwrap_or_else(|| PageLimit::try_new(1).expect("one page is valid")),
            ),
            fonts: self.render.fonts.unwrap_or_default(),
            font_path: self.render.font_path.clone(),
        }
    }

    /// Resolves defaults without changing the original explicit options.
    #[must_use]
    pub fn effective(&self) -> LetterTextSettings {
        LetterTextSettings {
            fields: LetterFields {
                company: self.company.clone(),
                title: self.title.clone(),
                recipient: self.recipient.clone(),
                date: self.date,
            },
            links: self.links.unwrap_or_default(),
            limit: self.max_length.map(|max| TextLimit {
                max,
                unit: self.length_unit.unwrap_or_default(),
            }),
        }
    }
}
fn parse_field(value: &str) -> Result<FieldValue, crate::letter::FieldValueError> {
    FieldValue::try_new(value)
}

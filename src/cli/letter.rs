//! ABOUTME: Explicit letter CLI settings, resolved defaults and text-mode usage checks.
//! ABOUTME: Render options are syntax-checked but never applied by the text command.
use crate::letter::{FieldValue, LengthUnit, LetterDate, LetterOutput, LinkStyle};
use crate::model::{
    StyleName,
    options::{FontsMode, OutputFormat, PageLimit, Paper},
};
use crate::select::SelectedTags;
use crate::spec::{Explicit, LetterChoices, LetterExplicit, OverridePolicy, ScopeFlag};
use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct LetterArgs {
    /// The options that build from a position spec. First, so its probe is read before what it asks about.
    #[command(flatten)]
    pub spec: super::SpecArgs,
    /// The Markdown letter to print; not needed with `--spec`, which names its own.
    #[arg(value_name = "FILE", required_unless_present = "spec")]
    pub file: Option<PathBuf>,
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
    #[arg(long, value_enum)]
    pub length_unit: Option<LengthUnit>,
    #[command(flatten)]
    pub render: LetterRenderArgs,
    #[command(flatten)]
    pub select: super::SelectArgs,
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

pub use crate::spec::plan::LetterTextSettings;

impl LetterArgs {
    /// The letter to read when it is given as a file: `None` only beside `--spec`, which names its own.
    #[must_use]
    pub fn input(&self) -> Option<&std::path::Path> {
        self.file.as_deref()
    }

    /// The settings typed on the command line that choose what a spec chooses, and only those.
    ///
    /// None of the letter's flags has a clap default, so a typed flag and a present value are the same thing and
    /// no probe is needed: a test pins that no default ever creeps in.
    #[must_use]
    pub fn explicit(&self) -> LetterExplicit {
        let scope = if self.select.core_only {
            Some(ScopeFlag::CoreOnly)
        } else {
            SelectedTags::new(self.select.tags.iter().cloned().collect()).map(ScopeFlag::Tags)
        };
        LetterExplicit {
            base: Explicit {
                style: self.render.style.clone(),
                paper: self.render.paper,
                scope,
                prefer: (!self.select.prefer.is_empty()).then(|| self.select.prefer.clone()),
            },
            output: self.output_format,
            max_length: self.max_length,
            length_unit: self.length_unit,
            links: self.links,
            company: self.company.clone(),
            title: self.title.clone(),
            recipient: self.recipient.clone(),
            date: self.date,
        }
    }

    /// Whether settings typed beside `--spec` are let through.
    #[must_use]
    pub fn override_policy(&self) -> OverridePolicy {
        if self.spec.allow_overrides {
            OverridePolicy::Allow
        } else {
            OverridePolicy::Refuse
        }
    }

    /// Checks value contradictions without filesystem or renderer access, including direct library callers.
    ///
    /// In spec mode the output format is the spec's unless typed, which only the plan can know, so the
    /// `--format json` check is the plan's there.
    pub(crate) fn validate(&self, format: OutputFormat) -> Result<(), String> {
        if self.render.fonts == Some(FontsMode::Bundled) && !self.render.font_path.is_empty() {
            return Err("--font-path cannot be used with --fonts bundled".into());
        }
        let in_spec_mode = self.spec.spec.is_some();
        // Clap requires one of them, but a library caller builds the arguments itself.
        if !in_spec_mode && self.file.is_none() {
            return Err("a letter needs a FILE or --spec".into());
        }
        // A typed output format is known now; one the spec supplies is the plan's to judge.
        let output_known = self.output_format.is_some() || !in_spec_mode;
        if format == OutputFormat::Json && output_known && self.output_format.unwrap_or_default() != LetterOutput::Pdf {
            return Err("--format json cannot share stdout with letter text".into());
        }
        // Beside a spec the limit may come from the spec, so the pair is the plan's to judge.
        if self.length_unit.is_some() && self.max_length.is_none() && !in_spec_mode {
            return Err("--length-unit requires --max-length".into());
        }
        if in_spec_mode {
            self.validate_spec()?;
        }
        Ok(())
    }

    /// What clap cannot say about `--spec`: a file beside it, and settings that would silently change it.
    ///
    /// Out of clap on purpose, for the reason `BuildArgs::validate_spec` gives: a `conflicts_with` between
    /// `--spec` and FILE makes clap stop enforcing the `requires` rules of the other spec options.
    fn validate_spec(&self) -> Result<(), String> {
        if let Some(file) = &self.file {
            return Err(format!(
                "--spec builds the letter the spec names, so {} cannot be given too; build either a spec or a file",
                file.display()
            ));
        }
        let explicit = self.explicit();
        if self.spec.print_spec {
            let flags = explicit.flags();
            if !flags.is_empty() {
                let names: Vec<String> = flags.iter().map(ToString::to_string).collect();
                return Err(format!(
                    "--print-spec shows the spec as written, so {} cannot be given with it; leave the flags out",
                    names.join(", ")
                ));
            }
            return Ok(());
        }
        explicit
            .admit(self.override_policy())
            .map(|_| ())
            .map_err(|refused| refused.to_string())
    }

    /// What is rendered, with the choices' style and paper and the machine flags from the command line.
    #[must_use]
    pub fn render_for(&self, choices: &LetterChoices) -> super::RenderArgs {
        let render = crate::spec::plan::letter_render(&self.machine_flags(), choices);
        super::RenderArgs {
            style: render.style,
            styles_dir: render.styles_dir,
            paper: render.paper,
            max_pages: render.max_pages,
            fonts: render.fonts,
            font_path: render.font_path,
        }
    }

    /// The machine flags as typed, in the shared planner's terms.
    #[must_use]
    pub fn machine_flags(&self) -> crate::spec::plan::LetterMachineFlags {
        crate::spec::plan::LetterMachineFlags {
            styles_dir: self.render.styles_dir.clone(),
            max_pages: self.render.max_pages,
            fonts: self.render.fonts,
            font_path: self.render.font_path.clone(),
        }
    }

    /// Resolves render values for a letter built from a file; the original options remain untouched.
    #[must_use]
    pub fn effective_render(&self) -> super::RenderArgs {
        self.render_for(&LetterChoices::from_flags(&self.explicit()))
    }

    /// Resolves defaults for a letter built from a file; the original options remain untouched.
    #[must_use]
    pub fn effective(&self) -> LetterTextSettings {
        LetterTextSettings::of(&LetterChoices::from_flags(&self.explicit()))
    }
}
fn parse_field(value: &str) -> Result<FieldValue, crate::letter::FieldValueError> {
    FieldValue::try_new(value)
}

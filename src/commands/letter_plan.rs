//! ABOUTME: Turns `letter` into what a run needs: the file to read, the PDF's name, the selection, the settings.
//! ABOUTME: One plan for a letter given as a file and one built from a spec, so both go through one run path.

use std::path::{Path, PathBuf};

use snafu::{IntoError, ResultExt};

use super::Env;
use super::spec_report::LetterFacts;
use crate::cli::{LetterArgs, LetterTextSettings};
use crate::error::{Error, SpecSnafu, TracedToChoiceSnafu};
use crate::letter::LetterOutput;
use crate::model::options::OutputFormat;
use crate::pipeline::{NameOrigins, RenderSettings};
use crate::select::Selection;
use crate::spec::plan::{LetterPlanOptions, plan_letter_spec};
use crate::spec::{self, LetterChoices, Located, ResolvedSpec};

pub(super) use crate::spec::plan::LetterPdf;

/// Set for a letter built from a spec: what to report about it, and what to check the content against.
pub(super) struct LetterSpecRun {
    pub facts: LetterFacts,
    pub resolved: ResolvedSpec,
    /// Where each tag and wording of `letter.select` was written, so a finding names a line and not a flag.
    pub names: NameOrigins,
    /// Where the spec chose the style being built with; `None` when a flag or the default chose it.
    pub style_at: Option<Located>,
}

impl LetterSpecRun {
    /// The letter's `content =` line, for an error that is about the file the spec named.
    pub(super) fn content_at(&self) -> &Located {
        self.facts.content_at()
    }
}

/// Everything one `letter` run needs, resolved before anything is read or written.
pub(super) struct LetterPlan {
    /// The letter to read: the file typed, or the content the spec names.
    pub content: PathBuf,
    /// The letter as findings name it: the same file, written the way a person would (no `..` that led there).
    pub shown: PathBuf,
    pub selection: Selection,
    pub output: LetterOutput,
    pub text: LetterTextSettings,
    /// What a PDF is made with; read only when `output` makes one.
    pub render: RenderSettings,
    pub pdf: LetterPdf,
    /// The directory the PDF is written to.
    pub out_dir: PathBuf,
    pub spec: Option<LetterSpecRun>,
}

impl LetterPlan {
    /// A style the spec chose that cannot be found, naming the line that chose it; any other error as it was.
    ///
    /// A style a flag or the default chose is not traced to the spec, and neither are problems with the styles
    /// directory, which the spec does not name.
    pub(super) fn style_error(&self, error: Error) -> Error {
        let at = self.spec.as_ref().and_then(|spec| spec.style_at.clone());
        match at {
            Some(at) if matches!(error, Error::UnknownStyle { .. } | Error::StyleMissingMain { .. }) => {
                TracedToChoiceSnafu { at: vec![at] }.into_error(error)
            }
            _ => error,
        }
    }
}

/// The plan of a letter given as a file: the flags are the only layer, and the defaults apply to what they leave.
pub(super) fn plan_file(args: &LetterArgs, file: &Path) -> LetterPlan {
    let choices = LetterChoices::from_flags(&args.explicit());
    LetterPlan {
        content: file.to_path_buf(),
        shown: file.to_path_buf(),
        // `selection()` carries `--drafts`: no spec has chosen anything here to leave drafts off.
        selection: args.select.selection(),
        output: choices.output.value,
        text: LetterTextSettings::of(&choices),
        render: RenderSettings::from(&args.render_for(&choices)),
        pdf: LetterPdf::of_file(file),
        out_dir: out_dir(args),
        spec: None,
    }
}

/// Resolves the spec at `spec_path` with the profiles directory the command line names.
///
/// # Errors
/// `Error::Spec` for anything `spec::resolve` refuses.
pub(super) fn resolve(args: &LetterArgs, spec_path: &Path) -> Result<ResolvedSpec, Error> {
    spec::resolve(spec_path, args.spec.profiles.as_deref()).context(SpecSnafu)
}

pub(super) use crate::spec::plan::LetterPlanError as PlanError;

/// The command line as the planner's plain options.
pub(super) fn options_of(args: &LetterArgs, format: OutputFormat) -> LetterPlanOptions {
    LetterPlanOptions {
        explicit: args.explicit(),
        policy: args.override_policy(),
        drafts: args.select.drafts,
        json_output: format == OutputFormat::Json,
        machine: args.machine_flags(),
        out: args.out.clone(),
    }
}

/// Plans the letter of `resolved`, the spec at `spec_path`, under the options of `args`.
///
/// # Errors
/// - `Error::OverridesRefused` - flags beside the spec that the policy refuses.
/// - `Error::Spec` - the spec has no `[letter]`, or its letter alone cannot be built.
/// - `Error::SpecName` - the spec's file name cannot name a PDF.
/// - A [`crate::spec::plan::LetterUsage`] mistake only the effective settings show. Clap has already exited with 2 for everything it can
///   see, so this is an `error:` line and exit code 1, as for any mistake found in a spec's content.
pub(super) fn plan_spec(
    args: &LetterArgs,
    spec_path: &Path,
    resolved: ResolvedSpec,
    format: OutputFormat,
    env: &Env,
) -> Result<LetterPlan, PlanError> {
    let planned = plan_letter_spec(spec_path, resolved, &options_of(args, format), &env.cwd)?;
    let letter = planned
        .resolved
        .letter
        .as_ref()
        .expect("a planned letter spec has a [letter] table");
    let facts = LetterFacts::new(
        &env.cwd,
        spec_path,
        &planned.resolved,
        letter,
        &planned.applied,
        args,
        &planned.out_dir,
    );
    Ok(LetterPlan {
        content: planned.content,
        shown: planned.shown,
        selection: planned.selection,
        output: planned.output,
        text: planned.text,
        render: planned.render,
        pdf: planned.pdf,
        out_dir: planned.out_dir,
        spec: Some(LetterSpecRun {
            facts,
            resolved: planned.resolved,
            names: planned.names,
            style_at: planned.style_at,
        }),
    })
}

/// The directory the PDF is written to, as the facts name it.
fn out_dir(args: &LetterArgs) -> PathBuf {
    args.out.clone().unwrap_or_else(|| PathBuf::from("out"))
}

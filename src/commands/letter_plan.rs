//! ABOUTME: Turns `letter` into what a run needs: the file to read, the PDF's name, the selection, the settings.
//! ABOUTME: One plan for a letter given as a file and one built from a spec, so both go through one run path.

use std::path::{Path, PathBuf};

use snafu::{IntoError, ResultExt};

use super::Env;
use super::build_plan::PdfStem;
use super::spec_report::{LetterFacts, located};
use crate::cli::{BuildArgs, LetterArgs, LetterTextSettings, RenderArgs};
use crate::error::{Error, OverridesRefusedSnafu, SpecSnafu, TracedToChoiceSnafu};
use crate::letter::LetterOutput;
use crate::model::options::OutputFormat;
use crate::pipeline::{NameOrigin, NameOrigins};
use crate::select::{Scope, Selection};
use crate::spec::{self, ChoiceKey, LetterChoices, Located, ResolvedLetter, ResolvedSpec};

/// The name of a letter's PDF without `.pdf`, which is also the identity written into it.
///
/// One value gives both, so the file name and the identity cannot come from different places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LetterPdf {
    /// The PDF's name without `.pdf`, and the identity inside it.
    stem: String,
    /// What the file is called under `--out`.
    file_name: String,
}

impl LetterPdf {
    /// A letter built as a file: `<file-stem>-letter.pdf`, with the file's stem as the identity.
    ///
    /// The identity is the input's stem, as a resume's is, so a letter and a resume of the same file name share
    /// one; changing it would change the bytes of every letter already built.
    pub(super) fn of_file(file: &Path) -> Self {
        let stem = BuildArgs::pdf_stem(file);
        Self {
            file_name: format!("{stem}-letter.pdf"),
            stem,
        }
    }

    /// A letter built from a spec: `<spec-stem>-letter.pdf`, and the same text as the identity, so the resume and
    /// the letter of one spec are two documents to a viewer. A preview of drafts ends in `.drafts.pdf`.
    pub(super) fn of_spec(spec: &Path, drafts: bool) -> Result<Self, Error> {
        let stem = PdfStem::letter_of_spec(spec, drafts)?;
        Ok(Self {
            file_name: stem.file_name(),
            stem: stem.as_str().to_string(),
        })
    }

    /// The identity written into the PDF.
    pub(super) fn ident(&self) -> &str {
        &self.stem
    }

    pub(super) fn file_name(&self) -> &str {
        &self.file_name
    }
}

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
    pub render: RenderArgs,
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
        selection: Selection {
            drafts: args.select.drafts,
            ..args.select.selection()
        },
        output: choices.output.value,
        text: LetterTextSettings::of(&choices),
        render: args.render_for(&choices),
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

/// Why the letter of a spec cannot be planned, apart from the errors every command can give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Usage {
    /// `--format json` needs standard output for itself, so the letter's text cannot be printed there too.
    JsonWithText,
    /// A length unit with no limit to count against.
    UnitWithoutLimit,
}

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JsonWithText => f.write_str(
                "--format json cannot share stdout with letter text; this letter prints text, so use `--output-format pdf`",
            ),
            Self::UnitWithoutLimit => f.write_str("--length-unit needs a max-length, from `--max-length` or the spec"),
        }
    }
}

/// What planning a spec can come to: an error every command shares, or a usage mistake only the whole can show.
pub(super) enum PlanError {
    Error(Error),
    Usage(Usage),
}

impl From<Error> for PlanError {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

/// Plans the letter of `resolved`, the spec at `spec_path`, under the options of `args`.
///
/// # Errors
/// - `Error::OverridesRefused` - flags beside the spec that the policy refuses.
/// - `Error::Spec` - the spec has no `[letter]`, or its letter alone cannot be built.
/// - `Error::SpecName` - the spec's file name cannot name a PDF.
/// - A [`Usage`] mistake only the effective settings show. Clap has already exited with 2 for everything it can
///   see, so this is an `error:` line and exit code 1, as for any mistake found in a spec's content.
pub(super) fn plan_spec(
    args: &LetterArgs,
    spec_path: &Path,
    resolved: ResolvedSpec,
    format: OutputFormat,
    env: &Env,
) -> Result<LetterPlan, PlanError> {
    let admitted = args
        .explicit()
        .admit(args.override_policy())
        .context(OverridesRefusedSnafu)?;
    let applied = spec::apply_letter(&resolved, &admitted).context(SpecSnafu)?;
    let pdf = LetterPdf::of_spec(spec_path, args.select.drafts)?;
    let choices = &applied.choices;
    // The spec alone cannot give a unit without a limit (the loader refuses it), so a flag made it.
    if args.length_unit.is_some() && choices.max_length.is_none() {
        return Err(PlanError::Usage(Usage::UnitWithoutLimit));
    }
    if format == OutputFormat::Json && choices.output.value != LetterOutput::Pdf {
        return Err(PlanError::Usage(Usage::JsonWithText));
    }
    let letter = resolved.letter.as_ref().expect("apply_letter checked the letter table");
    let names = name_origins(&env.cwd, &resolved, letter, &applied.selection, &applied.overrides);
    let style_at = match &choices.style.origin {
        crate::spec::Origin::Spec(at) | crate::spec::Origin::Build(at) => Some(at.clone()),
        _ => None,
    };
    let render = args.render_for(choices);
    let facts = LetterFacts::new(&env.cwd, spec_path, &resolved, letter, &applied, args, &out_dir(args));
    let selection = Selection {
        drafts: args.select.drafts,
        ..applied.selection
    };
    Ok(LetterPlan {
        content: letter.content.path.clone(),
        shown: super::spec_report::shown_path(&env.cwd, &letter.content.path),
        selection,
        output: choices.output.value,
        text: LetterTextSettings::of(choices),
        render,
        pdf,
        out_dir: out_dir(args),
        spec: Some(LetterSpecRun {
            facts,
            resolved,
            names,
            style_at,
        }),
    })
}

/// The directory the PDF is written to, as the facts name it.
fn out_dir(args: &LetterArgs) -> PathBuf {
    args.out.clone().unwrap_or_else(|| PathBuf::from("out"))
}

/// Says where each tag and wording of `selection` was written, so a finding names a line and not a flag.
///
/// A name a flag replaced was typed, which is the default origin, so it is left out. A letter has no `promote`.
fn name_origins(
    cwd: &Path,
    resolved: &ResolvedSpec,
    letter: &ResolvedLetter,
    selection: &Selection,
    overrides: &[spec::Override],
) -> NameOrigins {
    use spec::OverrideFlag;
    let spec_file = resolved.chain.last().map(|link| &link.path);
    let replaced = |flag| overrides.iter().any(|o| o.flag == flag);
    let origin_of = |key: ChoiceKey| {
        let at = letter.origin_of(&key)?;
        let place = located(cwd, at);
        Some(if spec_file.is_some_and(|spec| **spec == *at.path) {
            NameOrigin::Spec(place)
        } else {
            NameOrigin::Profile(place)
        })
    };
    let mut names = NameOrigins::default();
    if !replaced(OverrideFlag::Tags)
        && !replaced(OverrideFlag::CoreOnly)
        && let Scope::Tags(tags) = &selection.scope
    {
        for tag in tags.as_set() {
            if let Some(origin) = origin_of(ChoiceKey::Tag(tag.clone())) {
                names.tag(tag.clone(), origin);
            }
        }
    }
    if !replaced(OverrideFlag::Prefer) {
        for wording in &selection.prefer {
            if let Some(origin) = origin_of(ChoiceKey::Prefer(wording.clone())) {
                names.wording(wording.clone(), origin);
            }
        }
    }
    names
}

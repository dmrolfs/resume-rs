//! ABOUTME: Plans the build of a resolved spec without a command line: the file to read, the PDF's name, the selection.
//! ABOUTME: The one place spec data meets the build, shared by the CLI and the desktop app. Only the resume's own choices reach the typesetter.

use std::path::{Path, PathBuf};

use snafu::ResultExt;

use super::error::SpecError;
use super::overrides::{
    Applied, AppliedLetter, Explicit, LetterChoices, LetterExplicit, Origin, Override, OverrideFlag, OverridePolicy,
    apply, apply_letter,
};
use super::{ChoiceKey, Located, ResolvedLetter, ResolvedSpec};
use crate::error::{Error, OverridesRefusedSnafu, SpecNameSnafu, SpecSnafu, StemProblem};
use crate::letter::{LetterFields, LetterOutput, LinkStyle, TextLimit};
use crate::model::options::{FontsMode, PageLimit};
use crate::pipeline::{NameOrigin, NameOrigins, RenderSettings};
use crate::select::{Scope, Selection};
use crate::spec::error::tidy_path;

/// The suffix the letter of a file or a spec adds to its name: `acme.md` has the letter `acme-letter.pdf`.
pub(crate) const LETTER_SUFFIX: &str = "-letter";

/// The suffix a preview of a spec's drafts adds, so a preview never replaces the PDF built to be sent.
const DRAFTS_SUFFIX: &str = ".drafts";

/// How a path is shown: tidied, and relative to the current directory when it is under it. Never opened.
#[must_use]
pub fn shown_path(cwd: &Path, path: &Path) -> PathBuf {
    let tidy = tidy_path(path);
    tidy.strip_prefix(cwd).map_or(tidy.clone(), Path::to_path_buf)
}

/// `file:line:column` of a place, with the file shown as [`shown_path`] shows it.
#[must_use]
pub fn located(cwd: &Path, at: &Located) -> String {
    format!("{}:{}:{}", shown_path(cwd, &at.path).display(), at.line, at.column)
}

/// The name of a PDF without `.pdf`, which is also the identity written into it.
///
/// One value gives both, so the file name and the identity cannot come from different places: a build from a
/// spec is named after the spec, and a build from a file after the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfStem(String);

impl PdfStem {
    /// The stem of an input file: its file stem, or `resume` when it has none.
    #[must_use]
    pub fn of_input(input: &Path) -> Self {
        Self(file_stem(input))
    }

    /// The stem of the spec at `spec`, with the preview suffix when `drafts` are on.
    ///
    /// # Errors
    /// The spec's file name has no stem, is not UTF-8, starts with a dot (the PDF would be hidden), or ends in
    /// the preview suffix (it would collide with the preview of another spec) or in `-letter` (it would collide
    /// with the letter of another spec, which `letter --spec` names `<stem>-letter.pdf`).
    pub fn of_spec(spec: &Path, drafts: bool) -> Result<Self, Error> {
        let fail = |problem| SpecNameSnafu { path: spec, problem }.build();
        let stem = spec.file_stem().ok_or_else(|| fail(StemProblem::NoName))?;
        let stem = stem.to_str().ok_or_else(|| fail(StemProblem::NotUtf8))?;
        if stem.starts_with('.') {
            return Err(fail(StemProblem::Hidden));
        }
        // Case is ignored: on macOS and Windows `acme.Drafts.pdf` and `acme.drafts.pdf` are one file.
        let lower = stem.to_ascii_lowercase();
        if lower.ends_with(DRAFTS_SUFFIX) {
            return Err(fail(StemProblem::DraftsSuffix));
        }
        if lower.ends_with(LETTER_SUFFIX) {
            return Err(fail(StemProblem::LetterSuffix));
        }
        Ok(Self(if drafts {
            format!("{stem}{DRAFTS_SUFFIX}")
        } else {
            stem.to_string()
        }))
    }

    /// The stem as text: the identity written into the PDF.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The stem of the letter of this spec: `acme` is `acme-letter`, and its preview `acme-letter.drafts`.
    ///
    /// The preview suffix stays last, so every preview of either document ends in `.drafts.pdf`. The stem is also
    /// the identity written into the PDF, so a spec's resume and its letter are not one document to a viewer.
    ///
    /// # Errors
    /// As [`Self::of_spec`].
    pub fn letter_of_spec(spec: &Path, drafts: bool) -> Result<Self, Error> {
        let base = Self::of_spec(spec, false)?;
        Ok(Self(if drafts {
            format!("{}{LETTER_SUFFIX}{DRAFTS_SUFFIX}", base.0)
        } else {
            format!("{}{LETTER_SUFFIX}", base.0)
        }))
    }

    /// The PDF's file name.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.pdf", self.0)
    }
}

/// The file stem of `input`, which names the PDF built from it and is its identity.
#[must_use]
pub fn file_stem(input: &Path) -> String {
    input
        .file_stem()
        .map_or_else(|| "resume".into(), |stem| stem.to_string_lossy().into_owned())
}

/// What a caller asks of the planner: the flags it typed, whether they may replace the spec, and the rendering.
///
/// The command line fills `explicit` only with what was typed (a defaulted flag is `None`), so a default never
/// replaces a choice the spec made. A caller with no command line passes `Explicit::default()`.
#[derive(Debug, Clone)]
pub struct SpecPlanOptions {
    pub explicit: Explicit,
    pub policy: OverridePolicy,
    /// Preview drafts: put every draft back, and name the PDF `<spec>.drafts.pdf`.
    pub drafts: bool,
    /// The render settings before the spec's style and paper are applied to them.
    pub render: RenderSettings,
}

/// Everything one build from a spec needs, resolved before anything is read or written.
#[derive(Debug, Clone)]
pub struct SpecPlan {
    /// The resume to read: the file the spec names.
    pub content: PathBuf,
    pub stem: PdfStem,
    pub selection: Selection,
    /// The render settings with the spec's style and paper applied.
    pub render: RenderSettings,
    /// Where each tag and wording was written, so a finding names a line and not a flag.
    pub names: NameOrigins,
    pub resolved: ResolvedSpec,
    /// What the typed flags replaced, for the facts a command reports. `selection` here has drafts off.
    pub applied: Applied,
    /// Where the spec chose the style being built with; `None` when a flag or the default chose it.
    pub style_at: Option<Located>,
}

/// Plans the build of `resolved`, the spec at `spec_path`, with `options`; paths are shown relative to `cwd`.
///
/// Nothing is read or written: a missing content file, an unknown style and unusable fonts are found when the
/// build runs, so `print-spec` and an editor can plan without them.
///
/// # Errors
/// - `Error::OverridesRefused` - flags beside the spec that the policy refuses.
/// - `Error::Spec` - the spec alone asks for core-only and tags.
/// - `Error::SpecName` - the spec's file name cannot name a PDF.
pub fn plan_spec(
    spec_path: &Path,
    resolved: ResolvedSpec,
    options: &SpecPlanOptions,
    cwd: &Path,
) -> Result<SpecPlan, Error> {
    let admitted = options
        .explicit
        .clone()
        .admit(options.policy)
        .context(OverridesRefusedSnafu)?;
    let applied = apply(&resolved, &admitted).context(SpecSnafu)?;
    let stem = PdfStem::of_spec(spec_path, options.drafts)?;
    let mut render = options.render.clone();
    if let Some(style) = applied.style.clone() {
        render.style = style;
    }
    if let Some(paper) = applied.paper {
        render.paper = paper;
    }
    let overridden = |flag| applied.overrides.iter().any(|o| o.flag == flag);
    let style_at = (!overridden(OverrideFlag::Style) && resolved.choices.style.is_some())
        .then(|| resolved.origin_of(&ChoiceKey::Style).cloned())
        .flatten();
    let names = name_origins(cwd, &resolved, &applied.selection, &overridden);
    let selection = Selection {
        drafts: options.drafts,
        ..applied.selection.clone()
    };
    Ok(SpecPlan {
        content: resolved.content.path.clone(),
        stem,
        selection,
        render,
        names,
        resolved,
        applied,
        style_at,
    })
}

/// Says where each tag and wording of `selection` was written, so a finding names a line and not a flag.
///
/// A name a flag replaced was typed, which is the default origin, so it is left out.
fn name_origins(
    cwd: &Path,
    resolved: &ResolvedSpec,
    selection: &Selection,
    overridden: &dyn Fn(OverrideFlag) -> bool,
) -> NameOrigins {
    let spec_file = resolved.chain.last().map(|link| &link.path);
    let origin_of = |key: ChoiceKey| {
        let at = resolved.origin_of(&key)?;
        let place = located(cwd, at);
        Some(if spec_file.is_some_and(|spec| **spec == *at.path) {
            NameOrigin::Spec(place)
        } else {
            NameOrigin::Profile(place)
        })
    };
    let mut names = NameOrigins::default();
    if !overridden(OverrideFlag::Tags)
        && !overridden(OverrideFlag::CoreOnly)
        && let Scope::Tags(tags) = &selection.scope
    {
        for tag in tags.as_set() {
            if let Some(origin) = origin_of(ChoiceKey::Tag(tag.clone())) {
                names.tag(tag.clone(), origin);
            }
        }
    }
    // No flag replaces `promote`, so every promote tag was written in a file.
    for tag in &selection.promote {
        if let Some(origin) = origin_of(ChoiceKey::Promote(tag.clone())) {
            names.promoted(tag.clone(), origin);
        }
    }
    if !overridden(OverrideFlag::Prefer) {
        for wording in &selection.prefer {
            if let Some(origin) = origin_of(ChoiceKey::Prefer(wording.clone())) {
                names.wording(wording.clone(), origin);
            }
        }
    }
    names
}

// ---------------------------------------------------------------------------------------------------
// Letters
// ---------------------------------------------------------------------------------------------------

/// The name of a letter's PDF without `.pdf`, which is also the identity written into it.
///
/// One value gives both, so the file name and the identity cannot come from different places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterPdf {
    /// The PDF's name without `.pdf`, and the identity inside it.
    stem: String,
    /// What the file is called under the output directory.
    file_name: String,
}

impl LetterPdf {
    /// A letter built as a file: `<file-stem>-letter.pdf`, with the file's stem as the identity.
    ///
    /// The identity is the input's stem, as a resume's is, so a letter and a resume of the same file name share
    /// one; changing it would change the bytes of every letter already built.
    #[must_use]
    pub fn of_file(file: &Path) -> Self {
        let stem = file_stem(file);
        Self {
            file_name: format!("{stem}{LETTER_SUFFIX}.pdf"),
            stem,
        }
    }

    /// A letter built from a spec: `<spec-stem>-letter.pdf`, and the same text as the identity, so the resume and
    /// the letter of one spec are two documents to a viewer. A preview of drafts ends in `.drafts.pdf`.
    ///
    /// # Errors
    /// As [`PdfStem::of_spec`].
    pub fn of_spec(spec: &Path, drafts: bool) -> Result<Self, Error> {
        let stem = PdfStem::letter_of_spec(spec, drafts)?;
        Ok(Self {
            file_name: stem.file_name(),
            stem: stem.as_str().to_string(),
        })
    }

    /// The identity written into the PDF.
    #[must_use]
    pub fn ident(&self) -> &str {
        &self.stem
    }

    /// The file name, with `.pdf`.
    #[must_use]
    pub fn file_name(&self) -> &str {
        &self.file_name
    }
}

/// The text a letter is made of: which fields it fills, how links print, and the limit its text must stay under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterTextSettings {
    pub fields: LetterFields,
    pub links: LinkStyle,
    pub limit: Option<TextLimit>,
}

impl LetterTextSettings {
    /// The text settings of `choices`, whose defaults were applied after every layer.
    #[must_use]
    pub fn of(choices: &LetterChoices) -> Self {
        Self {
            fields: choices.fields(),
            links: choices.links.value,
            limit: choices.max_length.as_ref().map(|max| TextLimit {
                max: max.value,
                unit: choices.length_unit.value,
            }),
        }
    }
}

/// What a caller asks of the letter planner: the flags it typed, whether they may replace the spec, and rendering.
#[derive(Debug, Clone)]
pub struct LetterPlanOptions {
    pub explicit: LetterExplicit,
    pub policy: OverridePolicy,
    /// Preview drafts: put every draft back, and name the PDF `<spec>-letter.drafts.pdf`.
    pub drafts: bool,
    /// `--format json`: stdout is taken by the record, so a letter that prints text is a usage error.
    pub json_output: bool,
    /// The machine flags as typed; the plan applies the letter's own style and paper, and one page unless typed.
    pub machine: LetterMachineFlags,
    /// Where the PDF is written; `out` under the working directory when not typed.
    pub out: Option<PathBuf>,
}

/// Why the letter of a spec cannot be planned, apart from the errors every command can give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LetterUsage {
    /// `--format json` needs standard output for itself, so the letter's text cannot be printed there too.
    JsonWithText,
    /// A length unit with no limit to count against.
    UnitWithoutLimit,
}

impl std::fmt::Display for LetterUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JsonWithText => f.write_str(
                "--format json cannot share stdout with letter text; this letter prints text, so use `--output-format pdf`",
            ),
            Self::UnitWithoutLimit => f.write_str("--length-unit needs a max-length, from `--max-length` or the spec"),
        }
    }
}

/// The render flags of a letter as typed: where styles and fonts are, and the page limit, when given.
///
/// A letter's style and paper come from its choices, and a letter is one page unless a limit was typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LetterMachineFlags {
    pub styles_dir: Option<PathBuf>,
    pub max_pages: Option<PageLimit>,
    pub fonts: Option<FontsMode>,
    pub font_path: Vec<PathBuf>,
}

/// What planning the letter of a spec can come to: an error every command shares, or a usage mistake.
#[derive(Debug)]
pub enum LetterPlanError {
    Error(Error),
    Usage(LetterUsage),
}

impl From<Error> for LetterPlanError {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

/// Everything one letter built from a spec needs, resolved before anything is read or written.
#[derive(Debug, Clone)]
pub struct LetterSpecPlan {
    /// The letter to read: the content the spec names.
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
    pub names: NameOrigins,
    pub resolved: ResolvedSpec,
    /// Where the spec chose the style being built with; `None` when a flag or the default chose it.
    pub style_at: Option<Located>,
    /// What the typed flags replaced and chose, for the facts a command reports. `selection` here has drafts off.
    pub applied: AppliedLetter,
}

/// The directory a PDF goes to when none was typed.
const DEFAULT_OUT_DIR: &str = "out";

/// Plans the letter of `resolved`, the spec at `spec_path`, with `options`; paths are shown relative to `cwd`.
///
/// # Errors
/// - [`LetterPlanError::Error`] - flags the policy refuses, a spec without `[letter]`, a spec name that cannot
///   name a PDF, or a spec that contradicts itself.
/// - [`LetterPlanError::Usage`] - a length unit with no limit, or `--format json` with a letter that prints text.
pub fn plan_letter_spec(
    spec_path: &Path,
    resolved: ResolvedSpec,
    options: &LetterPlanOptions,
    cwd: &Path,
) -> Result<LetterSpecPlan, LetterPlanError> {
    let admitted = options
        .explicit
        .clone()
        .admit(options.policy)
        .context(OverridesRefusedSnafu)?;
    let applied = apply_letter(&resolved, &admitted).context(SpecSnafu)?;
    let pdf = LetterPdf::of_spec(spec_path, options.drafts)?;
    let choices = &applied.choices;
    // The spec alone cannot give a unit without a limit (the loader refuses it), so a flag made it.
    if options.explicit.length_unit.is_some() && choices.max_length.is_none() {
        return Err(LetterPlanError::Usage(LetterUsage::UnitWithoutLimit));
    }
    if options.json_output && choices.output.value != LetterOutput::Pdf {
        return Err(LetterPlanError::Usage(LetterUsage::JsonWithText));
    }
    let Some(letter) = resolved.letter.as_ref() else {
        return Err(Error::Spec {
            source: Box::new(SpecError::NoLetterTable),
        }
        .into());
    };
    let names = letter_name_origins(cwd, &resolved, letter, &applied.selection, &applied.overrides);
    let style_at = match &choices.style.origin {
        Origin::Spec(at) | Origin::Build(at) => Some(at.clone()),
        _ => None,
    };
    let render = letter_render(&options.machine, choices);
    let selection = Selection {
        drafts: options.drafts,
        ..applied.selection.clone()
    };
    Ok(LetterSpecPlan {
        content: letter.content.path.clone(),
        shown: shown_path(cwd, &letter.content.path),
        selection,
        output: choices.output.value,
        text: LetterTextSettings::of(choices),
        render,
        pdf,
        out_dir: options.out.clone().unwrap_or_else(|| PathBuf::from(DEFAULT_OUT_DIR)),
        names,
        resolved,
        style_at,
        applied,
    })
}

/// What a letter is rendered with: the choices' style and paper, the machine flags as typed, one page unless typed.
#[must_use]
pub fn letter_render(machine: &LetterMachineFlags, choices: &LetterChoices) -> RenderSettings {
    RenderSettings {
        style: choices.style.value.clone(),
        styles_dir: machine.styles_dir.clone(),
        paper: choices.paper.value,
        max_pages: Some(
            machine
                .max_pages
                .unwrap_or_else(|| PageLimit::try_new(1).expect("one page is valid")),
        ),
        fonts: machine.fonts.unwrap_or_default(),
        font_path: machine.font_path.clone(),
    }
}

/// Says where each tag and wording of `selection` was written, so a finding names a line and not a flag.
///
/// A name a flag replaced was typed, which is the default origin, so it is left out. A letter has no `promote`.
fn letter_name_origins(
    cwd: &Path,
    resolved: &ResolvedSpec,
    letter: &ResolvedLetter,
    selection: &Selection,
    overrides: &[Override],
) -> NameOrigins {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::StemProblem;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;

    fn stem(path: &str, drafts: bool) -> Result<PdfStem, Error> {
        PdfStem::of_spec(Path::new(path), drafts)
    }

    #[test]
    fn the_stem_is_the_spec_stem_and_never_the_content_stem() {
        let stem = assert_ok!(stem("positions/acme.toml", false));
        assert_eq!(stem.as_str(), "acme");
        assert_eq!(stem.file_name(), "acme.pdf");
    }

    #[test]
    fn a_draft_preview_is_named_apart_from_the_pdf_built_to_be_sent() {
        let stem = assert_ok!(stem("positions/acme.toml", true));
        assert_eq!(
            (stem.as_str(), stem.file_name().as_str()),
            ("acme.drafts", "acme.drafts.pdf")
        );
    }

    #[test]
    fn a_spec_whose_name_ends_in_the_preview_suffix_is_refused_so_two_specs_never_share_a_pdf() {
        for drafts in [false, true] {
            let error = assert_err!(stem("positions/acme.drafts.toml", drafts));
            assert!(
                matches!(
                    error,
                    Error::SpecName {
                        problem: StemProblem::DraftsSuffix,
                        ..
                    }
                ),
                "{error}"
            );
        }
    }

    #[test]
    fn a_preview_suffix_in_any_case_is_refused_because_case_insensitive_disks_name_one_file() {
        for name in ["acme.Drafts.toml", "acme.DRAFTS.toml", "ACME.drafts.toml"] {
            let error = assert_err!(stem(&format!("positions/{name}"), false));
            assert!(
                matches!(
                    error,
                    Error::SpecName {
                        problem: StemProblem::DraftsSuffix,
                        ..
                    }
                ),
                "{name}: {error}"
            );
        }
    }

    #[test]
    fn the_letter_of_a_spec_is_named_after_it_and_a_preview_ends_in_drafts() {
        let spec = Path::new("positions/acme.toml");
        let plain = assert_ok!(PdfStem::letter_of_spec(spec, false));
        assert_eq!(
            (plain.as_str(), plain.file_name().as_str()),
            ("acme-letter", "acme-letter.pdf")
        );
        let preview = assert_ok!(PdfStem::letter_of_spec(spec, true));
        assert_eq!(preview.file_name(), "acme-letter.drafts.pdf");
        // The resume of the spec is a different file in one `-o` directory.
        assert_ne!(
            plain.file_name(),
            assert_ok!(stem("positions/acme.toml", false)).file_name()
        );
    }

    #[test]
    fn a_spec_named_like_the_letter_of_another_is_refused_by_both_commands() {
        for name in [
            "acme-letter.toml",
            "acme-LETTER.toml",
            "ACME-Letter.toml",
            "-letter.toml",
        ] {
            let path = format!("positions/{name}");
            for drafts in [false, true] {
                let error = assert_err!(stem(&path, drafts));
                assert!(
                    matches!(
                        error,
                        Error::SpecName {
                            problem: StemProblem::LetterSuffix,
                            ..
                        }
                    ),
                    "{name}: {error}"
                );
            }
            assert!(matches!(
                assert_err!(PdfStem::letter_of_spec(Path::new(&path), false)),
                Error::SpecName {
                    problem: StemProblem::LetterSuffix,
                    ..
                }
            ));
        }
        // Only a stem that ENDS in the suffix collides.
        assert_ok!(stem("positions/letter.toml", false));
        assert_ok!(stem("positions/my-letters.toml", false));
        assert_ok!(stem("positions/acme-letter-2.toml", false));
    }

    #[test]
    fn a_hidden_or_nameless_spec_cannot_name_a_pdf() {
        let hidden = assert_err!(stem("positions/.toml", false));
        assert!(
            matches!(
                hidden,
                Error::SpecName {
                    problem: StemProblem::Hidden,
                    ..
                }
            ),
            "{hidden}"
        );
        let none = assert_err!(stem("..", false));
        assert!(
            matches!(
                none,
                Error::SpecName {
                    problem: StemProblem::NoName,
                    ..
                }
            ),
            "{none}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_spec_name_that_is_not_utf8_is_refused_and_not_made_lossy() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;
        let path = Path::new(OsStr::from_bytes(b"positions/ac\xffme.toml"));
        let error = assert_err!(PdfStem::of_spec(path, false));
        assert!(
            matches!(
                error,
                Error::SpecName {
                    problem: StemProblem::NotUtf8,
                    ..
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn an_input_keeps_the_stem_it_always_had() {
        assert_eq!(PdfStem::of_input(Path::new("dir/Resume.md")).file_name(), "Resume.pdf");
    }

    #[test]
    fn a_letter_built_from_a_file_keeps_the_name_and_identity_it_always_had() {
        let pdf = LetterPdf::of_file(Path::new("dir/Cover.md"));
        assert_eq!((pdf.ident(), pdf.file_name()), ("Cover", "Cover-letter.pdf"));
    }

    #[test]
    fn a_letter_built_from_a_spec_is_named_and_identified_by_the_specs_stem_and_letter() {
        let spec = Path::new("positions/acme.toml");
        let pdf = assert_ok!(LetterPdf::of_spec(spec, false));
        assert_eq!((pdf.ident(), pdf.file_name()), ("acme-letter", "acme-letter.pdf"));
        let preview = assert_ok!(LetterPdf::of_spec(spec, true));
        assert_eq!(
            (preview.ident(), preview.file_name()),
            ("acme-letter.drafts", "acme-letter.drafts.pdf")
        );
        // The identity is the file name without `.pdf`: one value, so the two cannot come from different places.
        assert_eq!(format!("{}.pdf", pdf.ident()), pdf.file_name());
    }
}

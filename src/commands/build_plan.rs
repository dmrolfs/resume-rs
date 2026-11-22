//! ABOUTME: Turns `build --spec` into what a build needs: the file to read, the PDF's name, the selection, the style.
//! ABOUTME: The one place spec data meets the build. Only the resume's own choices reach the typesetter.

use std::path::{Path, PathBuf};

use snafu::ResultExt;

use super::Env;
use super::spec_report::{SpecFacts, located};
use crate::cli::{BuildArgs, RenderArgs};
use crate::error::{Error, OverridesRefusedSnafu, SpecNameSnafu, SpecSnafu, StemProblem};
use crate::pipeline::{NameOrigin, NameOrigins};
use crate::select::{Scope, Selection};
use crate::spec::{self, ChoiceKey, Located, OverrideFlag, ResolvedSpec};

/// The name of a PDF without `.pdf`, which is also the identity written into it.
///
/// One value gives both, so the file name and the identity cannot come from different places: a build from a
/// spec is named after the spec, and a build from a file after the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PdfStem(String);

/// The suffix a preview of a spec's drafts adds, so a preview never replaces the PDF built to be sent.
const DRAFTS_SUFFIX: &str = ".drafts";

impl PdfStem {
    /// The stem of an input file.
    pub(super) fn of_input(input: &Path) -> Self {
        Self(BuildArgs::pdf_stem(input))
    }

    /// The stem of the spec at `spec`, with the preview suffix when `drafts` are on.
    ///
    /// # Errors
    /// The spec's file name has no stem, is not UTF-8, starts with a dot (the PDF would be hidden), or ends in
    /// the preview suffix (it would collide with the preview of another spec).
    pub(super) fn of_spec(spec: &Path, drafts: bool) -> Result<Self, Error> {
        let fail = |problem| SpecNameSnafu { path: spec, problem }.build();
        let stem = spec.file_stem().ok_or_else(|| fail(StemProblem::NoName))?;
        let stem = stem.to_str().ok_or_else(|| fail(StemProblem::NotUtf8))?;
        if stem.starts_with('.') {
            return Err(fail(StemProblem::Hidden));
        }
        // Case is ignored: on macOS and Windows `acme.Drafts.pdf` and `acme.drafts.pdf` are one file.
        if stem.to_ascii_lowercase().ends_with(DRAFTS_SUFFIX) {
            return Err(fail(StemProblem::DraftsSuffix));
        }
        Ok(Self(if drafts {
            format!("{stem}{DRAFTS_SUFFIX}")
        } else {
            stem.to_string()
        }))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }

    /// The PDF's file name.
    pub(super) fn file_name(&self) -> String {
        format!("{}.pdf", self.0)
    }
}

/// Everything one `build --spec` needs, resolved before anything is read or written.
pub(super) struct SpecPlan {
    /// The resume to read: the file the spec names.
    pub content: PathBuf,
    pub stem: PdfStem,
    pub selection: Selection,
    pub render: RenderArgs,
    pub facts: SpecFacts,
    pub names: NameOrigins,
    pub resolved: ResolvedSpec,
    /// Where the spec chose the style being built with; `None` when a flag or the default chose it.
    pub style_at: Option<Located>,
}

/// Resolves the spec at `spec_path` with the profiles directory the command line names.
///
/// # Errors
/// `Error::Spec` for anything `spec::resolve` refuses.
pub(super) fn resolve(args: &BuildArgs, spec_path: &Path) -> Result<ResolvedSpec, Error> {
    spec::resolve(spec_path, args.spec.profiles.as_deref()).context(SpecSnafu)
}

/// Plans the build of `resolved`, the spec at `spec_path`, under the options of `args`.
///
/// # Errors
/// - `Error::OverridesRefused` - flags beside the spec that the policy refuses.
/// - `Error::Spec` - the spec alone asks for core-only and tags.
/// - `Error::SpecName` - the spec's file name cannot name a PDF.
pub(super) fn plan(args: &BuildArgs, spec_path: &Path, resolved: ResolvedSpec, env: &Env) -> Result<SpecPlan, Error> {
    let admitted = args
        .explicit()
        .admit(args.override_policy())
        .context(OverridesRefusedSnafu)?;
    let applied = spec::apply(&resolved, &admitted).context(SpecSnafu)?;
    let drafts = args.select.drafts;
    let stem = PdfStem::of_spec(spec_path, drafts)?;
    let mut render = args.render.clone();
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
    let names = name_origins(&env.cwd, &resolved, &applied.selection, &overridden);
    let facts = SpecFacts::new(&env.cwd, spec_path, &resolved, &applied, args, &render);
    let selection = Selection {
        drafts,
        ..applied.selection
    };
    Ok(SpecPlan {
        content: resolved.content.path.clone(),
        stem,
        selection,
        render,
        facts,
        names,
        resolved,
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

#[cfg(test)]
mod tests {
    use super::*;
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

    /// The `build` arguments of `line`, parsed without `Cli::validate`, as a library caller could.
    fn unvalidated(line: &[&str]) -> BuildArgs {
        use crate::cli::{Cli, Command};
        use clap::Parser as _;
        let cli = assert_ok!(Cli::try_parse_from(
            std::iter::once("resume").chain(line.iter().copied())
        ));
        match cli.command {
            Command::Build(build) => build,
            other => panic!("expected `build`, got {other:?}"),
        }
    }

    /// The error `plan` gives; the plan itself has no `Debug`, so `assert_err!` cannot be used on it.
    fn plan_error(args: &BuildArgs, spec: &Path, resolved: ResolvedSpec, dir: &Path) -> Error {
        match plan(args, spec, resolved, &env_in(dir)) {
            Err(error) => error,
            Ok(_) => panic!("expected the plan to be refused"),
        }
    }

    fn env_in(dir: &Path) -> Env {
        Env {
            cwd: dir.to_path_buf(),
            resume_font_path: None,
        }
    }

    #[test]
    fn a_library_caller_that_skips_validation_still_meets_the_policy() {
        let dir = assert_ok!(tempfile::tempdir());
        let spec = dir.path().join("acme.toml");
        assert_ok!(std::fs::write(&spec, "[build]\ncontent = \"a.md\"\n"));
        let spec_arg = spec.display().to_string();
        let refused = unvalidated(&["build", "--spec", &spec_arg, "--tags", "ai"]);
        let resolved = assert_ok!(resolve(&refused, &spec));
        let error = plan_error(&refused, &spec, resolved, dir.path());
        assert!(matches!(error, Error::OverridesRefused { .. }), "{error}");
        let allowed = unvalidated(&["build", "--spec", &spec_arg, "--tags", "ai", "--allow-overrides"]);
        let resolved = assert_ok!(resolve(&allowed, &spec));
        assert!(plan(&allowed, &spec, resolved, &env_in(dir.path())).is_ok());
    }
}

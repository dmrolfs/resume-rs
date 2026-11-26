//! ABOUTME: Turns `build --spec` into what a build needs: the shared plan from `spec::plan`, plus what the CLI reports.
//! ABOUTME: The command-line side only: flags become plain options, and the plan's choices become facts to print.

use std::path::{Path, PathBuf};

use snafu::ResultExt;

use super::Env;
use super::spec_report::SpecFacts;
use crate::cli::BuildArgs;
use crate::error::{Error, SpecSnafu};
use crate::pipeline::{NameOrigins, RenderSettings};
use crate::select::Selection;
use crate::spec::plan::{SpecPlanOptions, plan_spec};
use crate::spec::{self, Located, ResolvedSpec};

pub(super) use crate::spec::plan::PdfStem;

/// Everything one `build --spec` needs, resolved before anything is read or written.
pub(super) struct SpecPlan {
    /// The resume to read: the file the spec names.
    pub content: PathBuf,
    pub stem: PdfStem,
    pub selection: Selection,
    pub render: RenderSettings,
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

/// The command line as the planner's plain options: what was typed, the policy, drafts, and the render flags.
fn options_of(args: &BuildArgs) -> SpecPlanOptions {
    SpecPlanOptions {
        explicit: args.explicit(),
        policy: args.override_policy(),
        drafts: args.select.drafts,
        render: RenderSettings::from(&args.render),
    }
}

/// Plans the build of `resolved`, the spec at `spec_path`, under the options of `args`.
///
/// # Errors
/// - `Error::OverridesRefused` - flags beside the spec that the policy refuses.
/// - `Error::Spec` - the spec alone asks for core-only and tags.
/// - `Error::SpecName` - the spec's file name cannot name a PDF.
pub(super) fn plan(args: &BuildArgs, spec_path: &Path, resolved: ResolvedSpec, env: &Env) -> Result<SpecPlan, Error> {
    let planned = plan_spec(spec_path, resolved, &options_of(args), &env.cwd)?;
    let facts = SpecFacts::new(
        &env.cwd,
        spec_path,
        &planned.resolved,
        &planned.applied,
        args,
        &planned.render,
    );
    Ok(SpecPlan {
        content: planned.content,
        stem: planned.stem,
        selection: planned.selection,
        render: planned.render,
        facts,
        names: planned.names,
        resolved: planned.resolved,
        style_at: planned.style_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;

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

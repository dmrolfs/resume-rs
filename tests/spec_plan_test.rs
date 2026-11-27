//! ABOUTME: The spec planners work with plain options and no command line, as the desktop app calls them.
//! ABOUTME: One test per field of the plan for the worked example, then refusals, drafts, and the letter's checks.

use std::path::{Path, PathBuf};

use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;
use resume::error::Error;
use resume::model::options::{FontsMode, Paper};
use resume::model::{StyleName, Tag};
use resume::pipeline::RenderSettings;
use resume::select::Scope;
use resume::spec::plan::{
    LetterMachineFlags, LetterPdf, LetterPlanError, LetterPlanOptions, LetterUsage, PdfStem, SpecPlanOptions,
    plan_letter_spec, plan_spec,
};
use resume::spec::{Explicit, LetterExplicit, OverridePolicy, ScopeFlag, resolve};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/specs")
}

fn spec_of(case: &str) -> PathBuf {
    fixtures().join(case).join("positions/acme.toml")
}

fn render() -> RenderSettings {
    RenderSettings {
        style: assert_ok!(StyleName::try_new("pragmatic")),
        styles_dir: None,
        paper: Paper::Letter,
        max_pages: None,
        fonts: FontsMode::Bundled,
        font_path: Vec::new(),
    }
}

fn options() -> SpecPlanOptions {
    SpecPlanOptions {
        explicit: Explicit::default(),
        policy: OverridePolicy::Refuse,
        drafts: false,
        render: render(),
    }
}

fn plan_of(case: &str, options: &SpecPlanOptions) -> Result<resume::spec::plan::SpecPlan, Error> {
    let spec = spec_of(case);
    let resolved = assert_ok!(resolve(&spec, None));
    plan_spec(&spec, resolved, options, &fixtures())
}

fn tag(name: &str) -> Tag {
    assert_ok!(Tag::try_new(name))
}

#[test]
fn the_worked_example_plans_every_field_from_the_spec_alone() {
    let plan = assert_ok!(plan_of("two-profiles", &options()));
    // The resume to read is the one the spec names, resolved against the spec's own directory.
    assert!(plan.content.ends_with("baseline.md"), "{}", plan.content.display());
    // The PDF is named after the spec, never after the content.
    assert_eq!(plan.stem.as_str(), "acme");
    assert_eq!(plan.stem.file_name(), "acme.pdf");
    // The spec's style wins over the render settings it was given.
    assert_eq!(plan.render.style.as_ref() as &str, "mono");
    assert_eq!(plan.render.styles_dir, None);
    // The paper came from `ai-platform`, and beats the render settings' own.
    assert_eq!(plan.render.paper, Paper::A4);
    assert_eq!(plan.render.fonts, FontsMode::Bundled);
    // The merge worked example: tags from both profiles and the spec itself.
    let Scope::Tags(tags) = &plan.selection.scope else {
        panic!("the spec selects by tags: {:?}", plan.selection.scope)
    };
    let mut found: Vec<String> = tags.as_set().iter().map(ToString::to_string).collect();
    found.sort();
    assert_eq!(found, ["ai", "iceberg", "leadership", "platform"]);
    assert!(!plan.selection.drafts);
    let wordings: Vec<String> = plan.selection.prefer.iter().map(ToString::to_string).collect();
    assert_eq!(wordings, ["short", "business", "technical"]);
    // Each name can be traced to the line that wrote it.
    assert!(plan.style_at.is_some(), "the spec chose the style");
    // `iceberg` was written in the spec itself and `leadership` only in a profile, and each says where.
    let origins = format!("{:?}", plan.names);
    assert!(origins.contains("Spec(") && origins.contains("acme.toml"), "{origins}");
    assert!(
        origins.contains("Profile(") && origins.contains("principal.toml"),
        "{origins}"
    );
    assert!(plan.applied.overrides.is_empty());
}

#[test]
fn a_preview_of_drafts_is_named_apart_and_puts_drafts_back() {
    let plan = assert_ok!(plan_of(
        "two-profiles",
        &SpecPlanOptions {
            drafts: true,
            ..options()
        }
    ));
    assert_eq!(plan.stem.as_str(), "acme.drafts");
    assert!(plan.selection.drafts);
    assert!(
        !plan.applied.selection.drafts,
        "the facts keep drafts off, as a spec chose"
    );
}

#[test]
fn a_typed_flag_beside_the_spec_is_refused_unless_overrides_are_allowed() {
    let typed = Explicit {
        scope: Some(ScopeFlag::CoreOnly),
        ..Explicit::default()
    };
    let refused = assert_err!(plan_of(
        "two-profiles",
        &SpecPlanOptions {
            explicit: typed.clone(),
            ..options()
        }
    ));
    assert!(matches!(refused, Error::OverridesRefused { .. }), "{refused}");
    let allowed = assert_ok!(plan_of(
        "two-profiles",
        &SpecPlanOptions {
            explicit: typed,
            policy: OverridePolicy::Allow,
            ..options()
        }
    ));
    assert_eq!(allowed.selection.scope, Scope::CoreOnly);
    assert_eq!(
        allowed.applied.overrides.len(),
        1,
        "the flag is recorded as an override"
    );
    // A name a flag replaced was typed, so it has no recorded origin: the tags the spec chose are not listed.
    assert!(
        !format!("{:?}", allowed.names).contains("iceberg"),
        "{:?}",
        allowed.names
    );
}

#[test]
fn a_flag_that_repeats_the_specs_value_is_still_a_flag() {
    let typed = Explicit {
        style: Some(assert_ok!(StyleName::try_new("mono"))),
        ..Explicit::default()
    };
    let plan = assert_ok!(plan_of(
        "two-profiles",
        &SpecPlanOptions {
            explicit: typed,
            policy: OverridePolicy::Allow,
            ..options()
        }
    ));
    assert_eq!(plan.applied.overrides.len(), 1);
    assert!(plan.style_at.is_none(), "a flag chose the style, not the spec");
}

#[test]
fn a_spec_that_cannot_name_a_pdf_is_refused() {
    let dir = assert_ok!(tempfile::tempdir());
    let spec = dir.path().join("acme.drafts.toml");
    assert_ok!(std::fs::write(&spec, "[build]\ncontent = \"a.md\"\n"));
    let resolved = assert_ok!(resolve(&spec, None));
    let error = assert_err!(plan_spec(&spec, resolved, &options(), dir.path()));
    assert!(matches!(error, Error::SpecName { .. }), "{error}");
}

#[test]
fn planning_reads_nothing_beyond_the_spec() {
    // The content file named here does not exist: planning is for print-spec and editors, so it must not need it.
    let dir = assert_ok!(tempfile::tempdir());
    let spec = dir.path().join("acme.toml");
    assert_ok!(std::fs::write(
        &spec,
        "[build]\ncontent = \"missing.md\"\nstyle = \"no-such-style\"\n"
    ));
    let resolved = assert_ok!(resolve(&spec, None));
    let plan = assert_ok!(plan_spec(&spec, resolved, &options(), dir.path()));
    assert_eq!(plan.render.style.as_ref() as &str, "no-such-style");
    assert!(plan.content.ends_with("missing.md"));
}

// ---- letters ------------------------------------------------------------------------------------------------

fn letter_options() -> LetterPlanOptions {
    LetterPlanOptions {
        explicit: LetterExplicit::default(),
        policy: OverridePolicy::Refuse,
        drafts: false,
        json_output: false,
        machine: LetterMachineFlags::default(),
        out: None,
    }
}

fn letter_plan_of(options: &LetterPlanOptions) -> Result<resume::spec::plan::LetterSpecPlan, LetterPlanError> {
    let spec = spec_of("letter-scopes");
    let resolved = assert_ok!(resolve(&spec, None));
    plan_letter_spec(&spec, resolved, options, &fixtures())
}

#[test]
fn the_letter_of_a_spec_plans_every_field_from_the_spec() {
    let plan = assert_ok!(letter_plan_of(&letter_options()));
    assert!(plan.content.ends_with("base.md"), "{}", plan.content.display());
    assert_eq!(plan.pdf.file_name(), "acme-letter.pdf");
    assert_eq!(plan.pdf.ident(), "acme-letter");
    assert_eq!(plan.out_dir, PathBuf::from("out"));
    // The letter's own style and paper, not the resume's; one page unless a flag said more.
    assert_eq!(plan.render.style.as_ref() as &str, "pragmatic");
    assert_eq!(plan.render.paper, Paper::A4);
    assert_eq!(plan.render.max_pages.map(|p| p.into_inner()), Some(1));
    assert_eq!(plan.render.fonts, FontsMode::default());
    assert_eq!(plan.text.limit.map(|l| l.max), Some(2000));
    let wordings: Vec<String> = plan.selection.prefer.iter().map(ToString::to_string).collect();
    // `[letter.select]` merged over the chain: the profiles' `short` and `warm`, and the spec's `formal`, in the README's order.
    assert_eq!(
        wordings,
        ["formal", "short", "warm"],
        "the letter's scope, not the resume's"
    );
    let letter_tags: Vec<String> = match &plan.selection.scope {
        Scope::Tags(tags) => {
            let mut found: Vec<String> = tags.as_set().iter().map(ToString::to_string).collect();
            found.sort();
            found
        }
        other => panic!("the letter selects by tags: {other:?}"),
    };
    assert_eq!(letter_tags, ["ai", "platform"]);
    assert!(plan.style_at.is_some());
    let origins = format!("{:?}", plan.names);
    assert!(origins.contains("formal") && origins.contains("acme.toml"), "{origins}");
}

#[test]
fn a_typed_page_limit_and_output_directory_reach_the_letter_plan() {
    let plan = assert_ok!(letter_plan_of(&LetterPlanOptions {
        machine: LetterMachineFlags {
            styles_dir: Some(PathBuf::from("my-styles")),
            max_pages: Some(assert_ok!(resume::model::options::PageLimit::try_new(2))),
            fonts: Some(FontsMode::Auto),
            font_path: vec![PathBuf::from("fonts-a"), PathBuf::from("fonts-b")],
        },
        out: Some(PathBuf::from("elsewhere")),
        ..letter_options()
    }));
    assert_eq!(plan.out_dir, PathBuf::from("elsewhere"));
    // Every machine flag reaches the render settings, in the order typed.
    assert_eq!(plan.render.styles_dir, Some(PathBuf::from("my-styles")));
    assert_eq!(plan.render.max_pages.map(|p| p.into_inner()), Some(2));
    assert_eq!(plan.render.fonts, FontsMode::Auto);
    assert_eq!(
        plan.render.font_path,
        [PathBuf::from("fonts-a"), PathBuf::from("fonts-b")]
    );
}

#[test]
fn json_on_stdout_refuses_a_letter_that_prints_text() {
    let spec = fixtures().join("letter-scopes/positions/acme.toml");
    let text =
        assert_ok!(std::fs::read_to_string(&spec)).replace("output-format = \"both\"", "output-format = \"text\"");
    let dir = assert_ok!(tempfile::tempdir());
    let copy = dir.path().join("acme.toml");
    assert_ok!(std::fs::write(&copy, text));
    let resolved = assert_ok!(resolve(&copy, Some(&fixtures().join("letter-scopes/profiles"))));
    let error = match plan_letter_spec(
        &copy,
        resolved,
        &LetterPlanOptions {
            json_output: true,
            ..letter_options()
        },
        dir.path(),
    ) {
        Err(error) => error,
        Ok(_) => panic!("expected a usage error"),
    };
    assert!(
        matches!(error, LetterPlanError::Usage(LetterUsage::JsonWithText)),
        "{error:?}"
    );
}

#[test]
fn a_length_unit_without_a_limit_is_a_usage_mistake() {
    let dir = assert_ok!(tempfile::tempdir());
    let spec = dir.path().join("acme.toml");
    assert_ok!(std::fs::write(
        &spec,
        "[build]\ncontent = \"a.md\"\n[letter]\ncontent = \"l.md\"\n"
    ));
    let resolved = assert_ok!(resolve(&spec, None));
    let error = match plan_letter_spec(
        &spec,
        resolved,
        &LetterPlanOptions {
            explicit: LetterExplicit {
                length_unit: Some(resume::letter::LengthUnit::Utf16),
                ..LetterExplicit::default()
            },
            policy: OverridePolicy::Allow,
            ..letter_options()
        },
        dir.path(),
    ) {
        Err(error) => error,
        Ok(_) => panic!("expected a usage error"),
    };
    assert!(
        matches!(error, LetterPlanError::Usage(LetterUsage::UnitWithoutLimit)),
        "{error:?}"
    );
}

#[test]
fn a_spec_without_a_letter_table_has_no_letter_to_plan() {
    let spec = spec_of("two-profiles");
    let resolved = assert_ok!(resolve(&spec, None));
    let error = match plan_letter_spec(&spec, resolved, &letter_options(), &fixtures()) {
        Err(error) => error,
        Ok(_) => panic!("expected a refusal"),
    };
    assert!(matches!(error, LetterPlanError::Error(Error::Spec { .. })), "{error:?}");
}

#[test]
fn naming_is_public_and_matches_what_the_cli_writes() {
    assert_eq!(PdfStem::of_input(Path::new("dir/Resume.md")).file_name(), "Resume.pdf");
    let pdf = LetterPdf::of_file(Path::new("dir/Cover.md"));
    assert_eq!((pdf.ident(), pdf.file_name()), ("Cover", "Cover-letter.pdf"));
}

#[test]
fn the_spec_tags_are_typed_names() {
    assert_eq!(tag("ai").to_string(), "ai");
}

/// The code of `file` before its `#[cfg(test)]` module, so test helpers may use the command line.
fn production_code(file: &str) -> String {
    let text = assert_ok!(std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(file)
    ));
    text.split("#[cfg(test)]").next().unwrap_or_default().to_string()
}

#[test]
fn the_shared_plan_does_not_depend_on_the_command_line() {
    // The desktop app calls these without clap, so they must not name clap or the `cli` module.
    for file in ["src/spec/plan.rs", "src/pipeline.rs"] {
        let code = production_code(file);
        for forbidden in ["clap", "crate::cli", "BuildArgs", "LetterArgs", "RenderArgs"] {
            assert!(!code.contains(forbidden), "{file} must not mention `{forbidden}`");
        }
    }
}

#[test]
fn a_resume_plan_keeps_the_styles_directory_and_font_paths_it_was_given() {
    let plan = assert_ok!(plan_of(
        "two-profiles",
        &SpecPlanOptions {
            render: RenderSettings {
                styles_dir: Some(PathBuf::from("my-styles")),
                fonts: FontsMode::Auto,
                font_path: vec![PathBuf::from("fonts-a"), PathBuf::from("fonts-b")],
                ..render()
            },
            ..options()
        }
    ));
    assert_eq!(plan.render.styles_dir, Some(PathBuf::from("my-styles")));
    assert_eq!(plan.render.fonts, FontsMode::Auto);
    assert_eq!(
        plan.render.font_path,
        [PathBuf::from("fonts-a"), PathBuf::from("fonts-b")]
    );
}

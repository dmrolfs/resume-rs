//! PDF preparation, checks and atomic commit for letters; never writes letter text.
use super::{Env, emit::Reporter, letter_plan::LetterPlan};
use crate::{
    cli::Cli,
    error::Error,
    layout::LayoutReport,
    letter::Letter,
    output::{Overwrite, Planned, WriteMode, Writer},
    pipeline::Session,
    typeset::{RenderOptions, Typesetter},
};
use std::path::PathBuf;

pub(super) fn render(
    cli: &Cli,
    plan: &LetterPlan,
    env: &Env,
    letter: &Letter,
    out: &mut Reporter<'_, '_>,
) -> Result<PathBuf, ()> {
    attempt(cli, plan, env, letter, out).map_err(|error| {
        if let Some(error) = error {
            out.error(&error);
        }
    })
}

fn attempt(
    cli: &Cli,
    plan: &LetterPlan,
    env: &Env,
    letter: &Letter,
    out: &mut Reporter<'_, '_>,
) -> Result<PathBuf, Option<Error>> {
    // The plan's render settings are resolved values, not clap defaults: what a spec chose came before them.
    let render = &plan.render;
    let session = Session::open(render, env.resume_font_path.as_deref())
        .require_ready()
        .map_err(|error| Some(plan.style_error(error)))?;
    let (style, fonts) = session.ready().expect("session was checked ready");
    let compiled = Typesetter::new(std::sync::Arc::clone(fonts))
        .compile_letter(letter, style, RenderOptions::new(render.paper))
        .map_err(Some)?;
    let mut sources = out.sources();
    for source in compiled.sources() {
        sources.insert(&source.path, &source.text);
    }
    out.diagnostics(&sources, &compiled.warnings().iter().collect::<Vec<_>>());
    let report =
        LayoutReport::letter(&compiled, fonts, render.max_pages.expect("letter default is one page")).map_err(Some)?;
    out.fonts(&report.fonts);
    out.diagnostics(&sources, &report.diagnostics.iter().collect::<Vec<_>>());
    if report
        .diagnostics
        .iter()
        .any(|finding| finding.severity == crate::diagnostics::Severity::Error)
    {
        return Err(None);
    }
    let target = plan.out_dir.join(plan.pdf.file_name());
    let pdf = compiled.export_pdf(plan.pdf.ident()).map_err(Some)?;
    Writer::new(if cli.dry_run {
        WriteMode::DryRun
    } else {
        WriteMode::Write
    })
    .commit(Planned {
        target: &target,
        bytes: &pdf,
        overwrite: Overwrite::Replace,
    })
    .map_err(Some)?;
    out.line(&format!(
        "ok {} letter PDF: {} ({} pages)",
        if cli.dry_run { "would write" } else { "wrote" },
        target.display(),
        compiled.page_count()
    ));
    Ok(target)
}

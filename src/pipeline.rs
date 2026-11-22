//! ABOUTME: The read-only part of building a resume: parse, compile, and measure the layout.
//! ABOUTME: `build` and `doctor` both start here, so what each reports about a resume cannot drift apart.

use std::path::Path;
use std::sync::Arc;

use crate::cli::RenderArgs;
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::error::Error;
use crate::layout::LayoutReport;
use crate::markdown::{Parsed, parse};
use crate::model::Priority;
use crate::model::Resume;
use crate::select::{self, CorePages, CoreSkip, Scope, Selection, SelectionReport, SelectionSummary, UnknownNames};
use crate::style::{self, StyleSource};
use crate::typeset::fonts::{FontSet, FontSetBuilder};
use crate::typeset::{Compiled, RenderOptions, Typesetter};

/// What one run shares across the files it assesses: the style, the fonts, and the typesetter.
///
/// The style and the fonts are results, not values: `build` stops the run when either failed, because it can
/// do nothing without them, while `doctor` reports the failure beside everything else it found.
pub struct Session {
    style: Result<StyleSource, Error>,
    fonts: Result<Arc<FontSet>, Error>,
    typesetter: Option<Typesetter>,
}

impl Session {
    /// Resolves the style and builds the font set once.
    ///
    /// `resume_font_path` is the value of `RESUME_FONT_PATH`, passed in so tests do not touch the process.
    #[must_use]
    pub fn open(render: &RenderArgs, resume_font_path: Option<&std::ffi::OsStr>) -> Self {
        let style = style::resolve(&render.style, render.styles_dir.as_deref());
        let builder: FontSetBuilder = render
            .font_path
            .iter()
            .fold(FontSet::builder().mode(render.fonts), |builder, dir| {
                builder.font_path(dir)
            })
            .env_paths(resume_font_path);
        let fonts = builder.build().map(Arc::new);
        let typesetter = fonts.as_ref().ok().map(|fonts| Typesetter::new(Arc::clone(fonts)));
        Self {
            style,
            fonts,
            typesetter,
        }
    }

    /// The style, or why it could not be resolved.
    ///
    /// # Errors
    /// The style does not exist, has no `style.typ`, or its styles directory cannot be used.
    pub fn style(&self) -> Result<&StyleSource, &Error> {
        self.style.as_ref()
    }

    /// The font set, or why it could not be built.
    ///
    /// # Errors
    /// A `--font-path` or `RESUME_FONT_PATH` entry cannot be read.
    pub fn fonts(&self) -> Result<&Arc<FontSet>, &Error> {
        self.fonts.as_ref()
    }

    /// Resolves the style again, keeping the fonts, for a caller that builds many times and wants a style that
    /// appeared or changed on disk since the last build. A second [`Session`] would scan the fonts again.
    ///
    /// # Errors
    /// The style cannot be resolved now. The previous style is kept, so the next build still has one.
    pub fn resolve_style_again(&mut self, render: &RenderArgs) -> Result<(), Error> {
        self.style = Ok(style::resolve(&render.style, render.styles_dir.as_deref())?);
        Ok(())
    }

    /// The first setup failure, if the style or the fonts could not be set up.
    ///
    /// # Errors
    /// The style or the font set failed; the style is reported first.
    pub fn ready(&self) -> Result<(&StyleSource, &Arc<FontSet>), &Error> {
        Ok((self.style.as_ref()?, self.fonts.as_ref()?))
    }

    /// Hands back the session when the style and the fonts are ready, or the first failure by value, so a
    /// command that cannot go on without them can stop with that error.
    ///
    /// # Errors
    /// The style or the font set failed; the style is reported first.
    pub fn require_ready(self) -> Result<Self, Error> {
        let Self {
            style,
            fonts,
            typesetter,
        } = self;
        let style = style?;
        let fonts = fonts?;
        Ok(Self {
            style: Ok(style),
            fonts: Ok(fonts),
            typesetter,
        })
    }

    /// Parses `text` and, when it has no errors, selects from it and compiles what is selected, then measures the
    /// layout. Writes nothing.
    ///
    /// The selection tailors what is compiled; `parsed` stays the whole baseline, so doctor's content lints and
    /// `--fix` read every item whatever the selection leaves out.
    #[must_use]
    pub fn assess(&self, file: &Path, text: &str, render: &RenderArgs, selection: &Selection) -> Assessment {
        let parsed = parse(file, text);
        // Selection needs no fonts or style, so its result and its errors are known even when setup failed.
        let tailored = parsed.valid().map(|baseline| {
            let findings = unknown_names(file, &UnknownNames::of(baseline, selection));
            select::select(baseline, selection).map(|(resume, report)| Selected {
                summary: SelectionSummary::new(baseline, &resume, selection, &report),
                resume,
                report,
                findings,
            })
        });
        let (rendered, selected) = match (tailored, self.ready()) {
            (None, _) => (Rendered::Skipped(Skip::FormatErrors), None),
            (Some(Err(source)), _) => (Rendered::Failed(Error::Select { source }), None),
            (Some(Ok(selected)), Err(_)) => (Rendered::Skipped(Skip::Setup), Some(selected)),
            (Some(Ok(selected)), Ok((style, fonts))) => {
                let rendered = self.render(&selected.resume, style, fonts, render);
                (rendered, Some(selected))
            }
        };
        Assessment {
            parsed,
            selected,
            rendered,
        }
    }

    /// The pages of `baseline`'s core content alone with `selection`'s wording choices and drafts setting: one
    /// more compile with the same style, paper, and fonts, so it equals what a `--core-only` build would print.
    ///
    /// Skipped, and why, when `selection` is already core only or nothing in `baseline` is `@core`.
    ///
    /// # Errors
    /// The core-only compile's own error (a style that compiles the whole resume but not its core), or a
    /// selection error; the caller reports it and fails the run, because a promised count must not vanish.
    pub fn core_pages(
        &self,
        baseline: &Resume,
        selection: &Selection,
        render: &RenderArgs,
    ) -> Result<CorePages, Error> {
        if selection.scope == Scope::CoreOnly {
            return Ok(CorePages::Skipped {
                reason: CoreSkip::AlreadyCoreOnly,
            });
        }
        let core = baseline
            .nodes()
            .iter()
            .any(|n| n.annotations.priority == Some(Priority::Core));
        if !core {
            return Ok(CorePages::Skipped {
                reason: CoreSkip::NoCore,
            });
        }
        let core_only = Selection {
            scope: Scope::CoreOnly,
            ..selection.clone()
        };
        let (resume, _) = select::select(baseline, &core_only).map_err(|source| Error::Select { source })?;
        // A session that could not be set up has nothing to measure with; `build` never asks one.
        let (Ok((style, _)), Some(typesetter)) = (self.ready(), self.typesetter.as_ref()) else {
            return Ok(CorePages::Skipped {
                reason: CoreSkip::NoSetup,
            });
        };
        let compiled = typesetter.compile(&resume, style, RenderOptions::new(render.paper))?;
        Ok(CorePages::Measured {
            pages: compiled.page_count(),
        })
    }

    fn render(&self, resume: &Resume, style: &StyleSource, fonts: &Arc<FontSet>, render: &RenderArgs) -> Rendered {
        let typesetter = self
            .typesetter
            .as_ref()
            .expect("fonts are ready, so there is a typesetter");
        match typesetter.compile(resume, style, RenderOptions::new(render.paper)) {
            Err(error) => Rendered::Failed(error),
            Ok(compiled) => match LayoutReport::of(&compiled, fonts, render.max_pages) {
                Ok(layout) => Rendered::Laid {
                    compiled: Box::new(compiled),
                    layout,
                },
                // The document compiled, so its warnings and style sources are still worth showing.
                Err(error) => Rendered::LayoutFailed {
                    compiled: Box::new(compiled),
                    error,
                },
            },
        }
    }
}

/// What assessing one file found: the parse, the selection from it, and what came of rendering it.
pub struct Assessment {
    /// The whole baseline as parsed, whatever the selection.
    pub parsed: Parsed,
    /// The tailored resume and what the selection did; `None` when there was nothing to select from, or the
    /// selection failed (then `rendered` is `Failed`).
    pub selected: Option<Selected>,
    pub rendered: Rendered,
}

/// The resume that was compiled, and the report of how it was selected from the baseline.
#[derive(Debug)]
pub struct Selected {
    pub resume: Resume,
    pub report: SelectionReport,
    /// What the summary lines and the JSON `selection` record say; `core` is filled by the caller that wants it.
    pub summary: SelectionSummary,
    /// Warnings about the selection itself: tags and wordings asked for that the resume does not use.
    pub findings: Vec<Diagnostic>,
}

/// One warning per unknown kind of name, listing what the file does use, so a typo is caught before it quietly
/// selects nothing.
pub(crate) fn unknown_names(file: &Path, unknown: &UnknownNames) -> Vec<Diagnostic> {
    unknown_name_findings(
        file,
        unknown,
        "resume",
        Some("`resume tags <file>` lists every tag and wording"),
    )
}

pub(crate) fn unknown_letter_names(file: &Path, unknown: &UnknownNames) -> Vec<Diagnostic> {
    unknown_name_findings(file, unknown, "letter", None)
}

fn unknown_name_findings(file: &Path, unknown: &UnknownNames, document: &str, hint: Option<&str>) -> Vec<Diagnostic> {
    let finding = |code: DiagnosticCode, asked: String, what: &str, known: String| {
        let has = if known.is_empty() {
            format!("the {document} uses no {what}s")
        } else {
            format!("the {document}'s {what}s are {known}")
        };
        Diagnostic {
            severity: code.default_severity(),
            code,
            message: format!("{asked}: not used in {}", file.display()),
            span: None,
            hints: std::iter::once(has).chain(hint.map(str::to_owned)).collect(),
            fix: None,
        }
    };
    let listed = |names: Vec<String>| names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ");
    let mut out = Vec::new();
    if !unknown.tags.is_empty() {
        let asked = listed(unknown.tags.iter().map(ToString::to_string).collect());
        let known = listed(unknown.known_tags.iter().map(ToString::to_string).collect());
        out.push(finding(
            DiagnosticCode::UnknownTag,
            format!("--tags {asked}"),
            "tag",
            known,
        ));
    }
    if !unknown.wordings.is_empty() {
        let asked = listed(unknown.wordings.iter().map(ToString::to_string).collect());
        let known = listed(unknown.known_wordings.iter().map(ToString::to_string).collect());
        out.push(finding(
            DiagnosticCode::UnknownWording,
            format!("--prefer {asked}"),
            "wording",
            known,
        ));
    }
    out
}

/// What rendering a file came to.
pub enum Rendered {
    /// Nothing was rendered, and why.
    Skipped(Skip),
    /// The style or the layout measurement failed; the error carries any Typst findings.
    Failed(Error),
    /// The document compiled but its layout could not be measured (the style's metadata is malformed); what
    /// compiled is kept, so its warnings and style files can be shown.
    LayoutFailed { compiled: Box<Compiled>, error: Error },
    /// The document compiled and its layout was measured.
    Laid {
        compiled: Box<Compiled>,
        layout: LayoutReport,
    },
}

/// Why nothing was rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// The resume has an error finding, so there is no model to render.
    FormatErrors,
    /// The style or the fonts could not be set up.
    Setup,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Command};
    use crate::model::{GroupName, VariantName};
    use claims::{assert_none, assert_ok, assert_some};

    fn render_args() -> RenderArgs {
        let cli = assert_ok!(Cli::try_parse_validated([
            "resume", "build", "r.md", "--fonts", "bundled"
        ]));
        match cli.command {
            Command::Build(build) => build.render,
            _ => unreachable!("parsed a build"),
        }
    }

    const TEXT: &str = "# Ada Lovelace\n\nLondon · a@b.co\n\n## S\n\n- Ready. <!-- =g.default -->\n- Later. <!-- =g.short @draft -->\n- Draft. <!-- @draft -->\n";

    #[test]
    fn assess_compiles_the_tailored_resume_and_keeps_the_baseline_in_parsed() {
        let render = render_args();
        let session = Session::open(&render, None);
        let assessment = session.assess(Path::new("r.md"), TEXT, &render, &Selection::default());
        let selected = assert_some!(assessment.selected);
        let crate::model::Block::Bullets { items } = &selected.resume.sections[0].blocks[0] else {
            panic!()
        };
        assert_eq!(items.len(), 1, "only the ready wording");
        assert_eq!(
            selected.report.drafts_left_out, 2,
            "the draft bullet and the draft wording"
        );
        let baseline = assert_some!(assessment.parsed.valid());
        let crate::model::Block::Bullets { items } = &baseline.sections[0].blocks[0] else {
            panic!()
        };
        assert_eq!(items.len(), 3, "parsed is the whole baseline");
        assert!(matches!(assessment.rendered, Rendered::Laid { .. }));
    }

    #[test]
    fn assess_selects_and_reports_a_draft_pick_even_when_the_style_cannot_be_set_up() {
        let render = render_args();
        let missing = assert_ok!(Cli::try_parse_validated([
            "resume", "build", "r.md", "--style", "nope", "--fonts", "bundled"
        ]));
        let Command::Build(build) = missing.command else {
            unreachable!("parsed a build")
        };
        let session = Session::open(&build.render, None);
        let plain = session.assess(Path::new("r.md"), TEXT, &render, &Selection::default());
        assert!(matches!(plain.rendered, Rendered::Skipped(Skip::Setup)));
        assert_some!(plain.selected, "the selection is known without a style");
        let pick = Selection {
            picks: [(
                assert_ok!(GroupName::try_new("g")),
                assert_ok!(VariantName::try_new("short")),
            )]
            .into(),
            ..Selection::default()
        };
        let failed = session.assess(Path::new("r.md"), TEXT, &render, &pick);
        assert!(
            matches!(failed.rendered, Rendered::Failed(Error::Select { .. })),
            "the draft pick is not hidden"
        );
    }

    #[test]
    fn assess_turns_a_draft_pick_into_a_failed_rendering_and_no_selected_resume() {
        let render = render_args();
        let session = Session::open(&render, None);
        let pick = Selection {
            picks: [(
                assert_ok!(GroupName::try_new("g")),
                assert_ok!(VariantName::try_new("short")),
            )]
            .into(),
            ..Selection::default()
        };
        let assessment = session.assess(Path::new("r.md"), TEXT, &render, &pick);
        assert_none!(assessment.selected);
        let Rendered::Failed(error) = &assessment.rendered else {
            panic!("expected a failure")
        };
        assert!(matches!(error, Error::Select { .. }));
        assert!(error.to_string().contains("`g.short`"), "{error}");
        assert!(assert_some!(error.hint()).contains("--drafts"));
    }
}

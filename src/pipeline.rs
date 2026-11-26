//! ABOUTME: The read-only part of building a resume: parse, compile, and measure the layout.
//! ABOUTME: `build` and `doctor` both start here, so what each reports about a resume cannot drift apart.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::error::Error;
use crate::layout::LayoutReport;
use crate::markdown::{Parsed, parse};
use crate::model::Resume;
use crate::model::options::{FontsMode, PageLimit, Paper};
use crate::model::{Priority, StyleName, Tag, VariantName};
use crate::select::{self, CorePages, CoreSkip, Scope, Selection, SelectionReport, SelectionSummary, UnknownNames};
use crate::style::{self, StyleSource};
use crate::typeset::fonts::{FontSet, FontSetBuilder};
use crate::typeset::{Compiled, RenderOptions, Typesetter};

/// How a resume or letter is rendered: the style and where to find it, the paper, the page limit, and the fonts.
///
/// The plain values a render needs, so a caller that has no command line (the desktop app) asks for a render in
/// the same terms the command line does. The command line's render options convert into it with `From`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderSettings {
    pub style: StyleName,
    /// A directory of styles besides the built-in ones; a style there wins over a built-in of the same name.
    pub styles_dir: Option<PathBuf>,
    pub paper: Paper,
    /// Fail when the document needs more pages than this.
    pub max_pages: Option<PageLimit>,
    pub fonts: FontsMode,
    /// Directories of fonts to use in `FontsMode::Auto`.
    pub font_path: Vec<PathBuf>,
}

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
    pub fn open(render: &RenderSettings, resume_font_path: Option<&std::ffi::OsStr>) -> Self {
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
    pub fn resolve_style_again(&mut self, render: &RenderSettings) -> Result<(), Error> {
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
    pub fn assess(&self, file: &Path, text: &str, render: &RenderSettings, selection: &Selection) -> Assessment {
        self.assess_with(file, text, render, selection, &NameOrigins::default())
    }

    /// [`Self::assess`] for a selection that a spec chose: `names` say where each tag and wording was written,
    /// so a finding about one points at that line and not at a flag nobody typed.
    #[must_use]
    pub fn assess_with(
        &self,
        file: &Path,
        text: &str,
        render: &RenderSettings,
        selection: &Selection,
        names: &NameOrigins,
    ) -> Assessment {
        let parsed = parse(file, text);
        // Selection needs no fonts or style, so its result and its errors are known even when setup failed.
        let tailored = parsed.valid().map(|baseline| {
            let findings = unknown_names_from(file, &UnknownNames::of(baseline, selection), names);
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
        render: &RenderSettings,
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

    fn render(&self, resume: &Resume, style: &StyleSource, fonts: &Arc<FontSet>, render: &RenderSettings) -> Rendered {
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

/// Where a tag or a wording a selection asks for was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameOrigin {
    /// Typed on the command line, as `--tags` or `--prefer`.
    Flag,
    /// Written in the spec itself, at this place. A spec is written for one resume, so a name that resume lacks
    /// is probably a typo.
    Spec(String),
    /// Written only in a profile, at this place. A profile is meant to be shared by resumes that use different
    /// names, so a name this resume lacks is expected.
    Profile(String),
}

/// Where each name a selection asks for was written; a name not listed was typed as a flag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameOrigins {
    tags: BTreeMap<Tag, NameOrigin>,
    wordings: BTreeMap<VariantName, NameOrigin>,
    /// Apart from `tags`: one tag can be selected in one file and promoted in another.
    promoted: BTreeMap<Tag, NameOrigin>,
}

impl NameOrigins {
    pub fn tag(&mut self, tag: Tag, origin: NameOrigin) {
        self.tags.insert(tag, origin);
    }

    pub fn wording(&mut self, wording: VariantName, origin: NameOrigin) {
        self.wordings.insert(wording, origin);
    }

    pub fn promoted(&mut self, tag: Tag, origin: NameOrigin) {
        self.promoted.insert(tag, origin);
    }
}

/// One warning per unknown kind of name, listing what the file does use, so a typo is caught before it quietly
/// selects nothing.
pub(crate) fn unknown_names(file: &Path, unknown: &UnknownNames) -> Vec<Diagnostic> {
    unknown_names_from(file, unknown, &NameOrigins::default())
}

/// [`unknown_names`] for names that may come from a spec: one finding per kind of name and place it was written.
///
/// A name typed as a flag, or written in the spec's own file, is a warning. A name written only in a profile is a
/// note, because a profile is shared by resumes that do not all use the same tags and wordings.
pub(crate) fn unknown_names_from(file: &Path, unknown: &UnknownNames, origins: &NameOrigins) -> Vec<Diagnostic> {
    unknown_name_findings(
        file,
        unknown,
        origins,
        "resume",
        Some("`resume tags <file>` lists every tag and wording"),
    )
}

/// One warning per unknown kind of name in a letter, with where each was written when a spec wrote it.
pub(crate) fn unknown_letter_names_from(
    file: &Path,
    unknown: &UnknownNames,
    origins: Option<&NameOrigins>,
) -> Vec<Diagnostic> {
    let default = NameOrigins::default();
    unknown_name_findings(file, unknown, origins.unwrap_or(&default), "letter", None)
}

fn unknown_name_findings(
    file: &Path,
    unknown: &UnknownNames,
    origins: &NameOrigins,
    document: &'static str,
    tags_hint: Option<&'static str>,
) -> Vec<Diagnostic> {
    let origin_of = |name: String, origin: Option<&NameOrigin>| (name, origin.cloned().unwrap_or(NameOrigin::Flag));
    let kinds = [
        UnknownKind {
            code: DiagnosticCode::UnknownTag,
            what: "tag",
            names: "tags",
            flag: "--tags",
            document,
            tags_hint,
            asked: unknown
                .tags
                .iter()
                .map(|t| origin_of(t.to_string(), origins.tags.get(t)))
                .collect(),
            known: unknown.known_tags.iter().map(ToString::to_string).collect(),
        },
        UnknownKind {
            code: DiagnosticCode::UnknownWording,
            what: "wording",
            names: "wordings",
            flag: "--prefer",
            document,
            tags_hint,
            asked: unknown
                .wordings
                .iter()
                .map(|w| origin_of(w.to_string(), origins.wordings.get(w)))
                .collect(),
            known: unknown.known_wordings.iter().map(ToString::to_string).collect(),
        },
        UnknownKind {
            code: DiagnosticCode::UnknownTag,
            what: "tag",
            names: "promote tags",
            // No flag sets it; a selection built in code without origins names the key.
            flag: "promote",
            document,
            tags_hint,
            asked: unknown
                .promote
                .iter()
                .map(|t| origin_of(t.to_string(), origins.promoted.get(t)))
                .collect(),
            known: unknown.known_tags.iter().map(ToString::to_string).collect(),
        },
    ];
    kinds
        .iter()
        .flat_map(|kind| {
            [Class::Flag, Class::Spec, Class::Profile]
                .into_iter()
                .filter_map(move |class| kind.finding(file, class))
        })
        .collect()
}

/// The names of one kind (tags or wordings) a selection asked for that the document lacks, and where each was written.
struct UnknownKind {
    code: DiagnosticCode,
    /// One name of this kind, for the hint that lists what the document uses.
    what: &'static str,
    /// The names of this kind, for the message: `tags`, `wordings`, `promote tags`.
    names: &'static str,
    /// The flag a name of this kind is typed with.
    flag: &'static str,
    /// What the names were looked for in: `resume` or `letter`.
    document: &'static str,
    /// The line that says where to see every name, when there is a command for it.
    tags_hint: Option<&'static str>,
    asked: Vec<(String, NameOrigin)>,
    /// What the document does use.
    known: Vec<String>,
}

impl UnknownKind {
    /// The finding for the names written in places of `class`; `None` when there are none.
    fn finding(&self, file: &Path, class: Class) -> Option<Diagnostic> {
        let names: Vec<&(String, NameOrigin)> = self.asked.iter().filter(|(_, o)| Class::of(o) == class).collect();
        if names.is_empty() {
            return None;
        }
        let listed = |names: &[String]| names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ");
        let (what, document) = (self.what, self.document);
        let known = listed(&self.known);
        let mut hints = vec![if known.is_empty() {
            format!("the {document} uses no {what}s")
        } else {
            format!("the {document}'s {what}s are {known}")
        }];
        hints.extend(self.tags_hint.map(str::to_owned));
        let (severity, subject) = match class {
            Class::Flag => {
                let typed: Vec<String> = names.iter().map(|(name, _)| name.clone()).collect();
                (
                    self.code.default_severity(),
                    format!("{} {}", self.flag, listed(&typed)),
                )
            }
            Class::Spec | Class::Profile => {
                let each: Vec<String> = names
                    .iter()
                    .map(|(name, origin)| format!("`{name}` ({})", origin.place()))
                    .collect();
                if class == Class::Profile {
                    hints.push(format!(
                        "a profile can name what this {document} lacks, so this is only a note"
                    ));
                }
                let (who, severity) = if class == Class::Spec {
                    ("spec", self.code.default_severity())
                } else {
                    ("profile", Severity::Note)
                };
                (severity, format!("{who} {} {}", self.names, each.join(", ")))
            }
        };
        Some(Diagnostic {
            severity,
            code: self.code,
            message: format!("{subject}: not used in {}", file.display()),
            span: None,
            hints,
            fix: None,
        })
    }
}

/// What kind of place a name was written in, for grouping findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Flag,
    Spec,
    Profile,
}

impl Class {
    fn of(origin: &NameOrigin) -> Self {
        match origin {
            NameOrigin::Flag => Self::Flag,
            NameOrigin::Spec(_) => Self::Spec,
            NameOrigin::Profile(_) => Self::Profile,
        }
    }
}

impl NameOrigin {
    fn place(&self) -> &str {
        match self {
            Self::Flag => "the command line",
            Self::Spec(at) | Self::Profile(at) => at,
        }
    }
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

    fn render_args() -> RenderSettings {
        let cli = assert_ok!(Cli::try_parse_validated([
            "resume", "build", "r.md", "--fonts", "bundled"
        ]));
        match cli.command {
            Command::Build(build) => RenderSettings::from(&build.render),
            _ => unreachable!("parsed a build"),
        }
    }

    const TEXT: &str = "# Ada Lovelace\n\nLondon · a@b.co\n\n## S\n\n- Ready. <!-- =g.default -->\n- Later. <!-- =g.short @draft -->\n- Draft. <!-- @draft -->\n";

    fn tag(name: &str) -> Tag {
        assert_ok!(Tag::try_new(name))
    }

    fn wording(name: &str) -> VariantName {
        assert_ok!(VariantName::try_new(name))
    }

    fn unknown(tags: &[&str], wordings: &[&str]) -> UnknownNames {
        UnknownNames {
            tags: tags.iter().map(|t| tag(t)).collect(),
            known_tags: vec![tag("real")],
            wordings: wordings.iter().map(|w| wording(w)).collect(),
            known_wordings: vec![wording("default")],
            promote: Vec::new(),
        }
    }

    #[test]
    fn names_typed_as_flags_are_the_warnings_they_always_were() {
        let found = unknown_names_from(
            Path::new("r.md"),
            &unknown(&["a", "b"], &["w"]),
            &NameOrigins::default(),
        );
        let messages: Vec<&str> = found.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages,
            ["--tags `a`, `b`: not used in r.md", "--prefer `w`: not used in r.md"]
        );
        assert!(found.iter().all(|d| d.severity == Severity::Warning));
        assert_eq!(found[0].code, DiagnosticCode::UnknownTag);
        assert_eq!(found[1].code, DiagnosticCode::UnknownWording);
        assert_eq!(found, unknown_names(Path::new("r.md"), &unknown(&["a", "b"], &["w"])));
    }

    #[test]
    fn a_name_the_spec_wrote_is_a_warning_and_one_only_a_profile_wrote_is_a_note() {
        let mut origins = NameOrigins::default();
        origins.tag(tag("typo"), NameOrigin::Spec("positions/acme.toml:4:9".into()));
        origins.tag(tag("shared"), NameOrigin::Profile("profiles/p.toml:2:9".into()));
        origins.wording(wording("w"), NameOrigin::Profile("profiles/p.toml:3:11".into()));
        let found = unknown_names_from(Path::new("r.md"), &unknown(&["typo", "shared"], &["w"]), &origins);
        let by_severity: Vec<(Severity, &str)> = found.iter().map(|d| (d.severity, d.message.as_str())).collect();
        assert_eq!(
            by_severity,
            [
                (
                    Severity::Warning,
                    "spec tags `typo` (positions/acme.toml:4:9): not used in r.md"
                ),
                (
                    Severity::Note,
                    "profile tags `shared` (profiles/p.toml:2:9): not used in r.md"
                ),
                (
                    Severity::Note,
                    "profile wordings `w` (profiles/p.toml:3:11): not used in r.md"
                ),
            ]
        );
        assert!(
            found[1].hints.iter().any(|h| h.contains("only a note")),
            "{:?}",
            found[1].hints
        );
        assert!(!found[0].hints.iter().any(|h| h.contains("only a note")));
        assert!(
            found[0].hints.iter().any(|h| h.contains("`real`")),
            "lists what the resume uses"
        );
    }

    #[test]
    fn a_flag_name_a_spec_name_and_a_profile_name_of_one_kind_are_three_findings_in_that_order() {
        let mut origins = NameOrigins::default();
        origins.tag(tag("b"), NameOrigin::Profile("p:1:1".into()));
        origins.tag(tag("c"), NameOrigin::Spec("s:1:1".into()));
        let found = unknown_names_from(Path::new("r.md"), &unknown(&["a", "b", "c"], &[]), &origins);
        let starts: Vec<&str> = found
            .iter()
            .map(|d| d.message.split(' ').next().unwrap_or(""))
            .collect();
        assert_eq!(starts, ["--tags", "spec", "profile"]);
    }

    #[test]
    fn a_resume_that_uses_no_tags_says_so_and_every_finding_points_at_the_tags_command() {
        let none = UnknownNames {
            tags: vec![tag("a")],
            ..UnknownNames::default()
        };
        let found = unknown_names_from(Path::new("r.md"), &none, &NameOrigins::default());
        assert_eq!(found[0].hints[0], "the resume uses no tags");
        assert_eq!(found[0].hints[1], "`resume tags <file>` lists every tag and wording");
        let some = unknown_names_from(Path::new("r.md"), &unknown(&["a"], &[]), &NameOrigins::default());
        assert_eq!(some[0].hints[0], "the resume's tags are `real`");
    }

    #[test]
    fn an_unknown_promote_tag_is_a_tag_finding_of_its_own_warned_from_the_spec_and_noted_from_a_profile() {
        let mut origins = NameOrigins::default();
        origins.promoted(tag("lede"), NameOrigin::Spec("positions/acme.toml:5:12".into()));
        origins.promoted(tag("shared"), NameOrigin::Profile("profiles/p.toml:2:12".into()));
        let asked = UnknownNames {
            promote: vec![tag("lede"), tag("shared")],
            ..unknown(&["lede"], &[])
        };
        let found = unknown_names_from(Path::new("r.md"), &asked, &origins);
        let seen: Vec<(DiagnosticCode, Severity, &str)> =
            found.iter().map(|d| (d.code, d.severity, d.message.as_str())).collect();
        assert_eq!(
            seen,
            [
                (
                    DiagnosticCode::UnknownTag,
                    Severity::Warning,
                    "--tags `lede`: not used in r.md"
                ),
                (
                    DiagnosticCode::UnknownTag,
                    Severity::Warning,
                    "spec promote tags `lede` (positions/acme.toml:5:12): not used in r.md"
                ),
                (
                    DiagnosticCode::UnknownTag,
                    Severity::Note,
                    "profile promote tags `shared` (profiles/p.toml:2:12): not used in r.md"
                ),
            ],
            "a tag selected by a flag and promoted by the spec is two findings, each naming where it was written"
        );
        assert!(
            found[1].hints.iter().any(|h| h.contains("`real`")),
            "lists the resume's tags"
        );
    }

    #[test]
    fn unknown_names_finds_promote_tags_no_node_carries_once_each_in_promote_order() {
        let resume = assert_some!(crate::markdown::parse("r.md", TEXT).valid()).clone();
        let selection = Selection {
            promote: vec![tag("zz"), tag("aa"), tag("zz")],
            ..Selection::default()
        };
        let found = UnknownNames::of(&resume, &selection);
        assert_eq!(found.promote, vec![tag("zz"), tag("aa")]);
        assert!(!found.is_empty());
    }

    #[test]
    fn nothing_unknown_is_nothing_said() {
        assert!(unknown_names_from(Path::new("r.md"), &unknown(&[], &[]), &NameOrigins::default()).is_empty());
    }

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
        let session = Session::open(&RenderSettings::from(&build.render), None);
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

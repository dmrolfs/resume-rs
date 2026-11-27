//! ABOUTME: What a spec build tells a person: the position, the profile chain, the content, overrides, environment.
//! ABOUTME: One value feeds both the human lines and the JSON `spec` record, so the two cannot disagree.

use std::path::Path;

use serde::Serialize;

use crate::cli::{BuildArgs, LetterArgs};
use crate::model::options::FontsMode;
use crate::pipeline::RenderSettings;
use crate::spec::error::shown;
use crate::spec::{
    Applied, AppliedLetter, Located, Origin, Override, Position, Replaced, ResolvedLetter, ResolvedSpec, paper_name,
};

/// The position a spec says it applies for, as text safe to print.
#[derive(Debug, Clone, Default, Serialize)]
pub(super) struct PositionFacts {
    company: Option<String>,
    title: Option<String>,
    level: Option<String>,
    domain: Option<String>,
    audience: Option<String>,
    date: Option<String>,
    posting: Option<String>,
    emphasis: EmphasisFacts,
}

#[derive(Debug, Clone, Default, Serialize)]
struct EmphasisFacts {
    business: Option<String>,
    org: Option<String>,
    technical: Option<String>,
}

impl From<&Position> for PositionFacts {
    fn from(p: &Position) -> Self {
        let text = |value: Option<&str>| value.map(shown);
        Self {
            company: text(p.company.as_ref().map(AsRef::as_ref)),
            title: text(p.title.as_ref().map(AsRef::as_ref)),
            level: text(p.level.as_ref().map(AsRef::as_ref)),
            domain: text(p.domain.as_ref().map(AsRef::as_ref)),
            audience: text(p.audience.as_ref().map(AsRef::as_ref)),
            date: p.date.map(|d| d.to_string()),
            posting: text(p.posting.as_ref().map(url::Url::as_str)),
            emphasis: EmphasisFacts {
                business: p.emphasis.business.map(|l| l.to_string()),
                org: p.emphasis.org.map(|l| l.to_string()),
                technical: p.emphasis.technical.map(|l| l.to_string()),
            },
        }
    }
}

/// One file of the chain.
#[derive(Debug, Clone, Serialize)]
struct ChainFact {
    /// The profile's name; `null` for the spec itself.
    name: Option<String>,
    file: String,
}

/// One flag typed beside the spec.
#[derive(Debug, Clone, Serialize)]
struct OverrideFact {
    flag: String,
    /// The values typed; empty for a switch.
    set: Vec<String>,
    /// What the spec chose that the flag replaced; `null` when the spec chose nothing.
    replaced: Option<ReplacedFact>,
    cleared_groups: Option<ClearedGroupsFact>,
    /// Where the spec chose it, one place for each file.
    at: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ClearedGroupsFact {
    names: Vec<String>,
    at: String,
}

#[derive(Debug, Clone, Serialize)]
struct ReplacedFact {
    /// The kind of choice: `tags`, `core-only`, `prefer`, `style`, or `paper`.
    setting: String,
    /// Its values; empty for `core-only`.
    value: Vec<String>,
}

/// The flags about the machine, which a spec may be built under without changing what it says.
#[derive(Debug, Clone, Serialize)]
struct EnvironmentFact {
    fonts: String,
    font_path: Vec<String>,
    styles_dir: Option<String>,
    max_pages: Option<u8>,
    out: String,
}

/// Everything a spec build reports about its spec. Spec data goes only here, never into the typesetter.
#[derive(Debug, Clone, Serialize)]
pub(super) struct SpecFacts {
    #[serde(rename = "type")]
    kind: &'static str,
    file: String,
    position: Option<PositionFacts>,
    chain: Vec<ChainFact>,
    content: String,
    /// The PDF this build wrote or would write; `null` when none was.
    pdf: Option<String>,
    style: String,
    paper: String,
    drafts: bool,
    overrides: Vec<OverrideFact>,
    environment: EnvironmentFact,
    version: &'static str,
    /// Where the spec wrote `content =`, for an error about the file.
    #[serde(skip)]
    content_at: Located,
    /// The fonts mode differs from the default, which changes the bytes of a PDF, so the summary says it.
    #[serde(skip)]
    fonts_changed: bool,
}

pub(super) use crate::spec::plan::{located, shown_path};

impl OverrideFact {
    fn of(cwd: &Path, item: &Override) -> Self {
        let (replaced, at) = match &item.replaced {
            Replaced::Default => (None, Vec::new()),
            Replaced::Spec { setting, value, at } => (
                Some(ReplacedFact {
                    setting: setting.to_string(),
                    value: value.clone(),
                }),
                at.iter().map(|a| located(cwd, a)).collect(),
            ),
        };
        Self {
            flag: item.flag.to_string(),
            set: item.set.clone(),
            replaced,
            cleared_groups: item.cleared_groups.as_ref().map(|groups| ClearedGroupsFact {
                names: groups.names.clone(),
                at: located(cwd, &groups.at),
            }),
            at,
        }
    }
}

impl SpecFacts {
    /// The facts of building `resolved` from the spec file `spec_path` with `applied` and `args`' environment.
    pub(super) fn new(
        cwd: &Path,
        spec_path: &Path,
        resolved: &ResolvedSpec,
        applied: &Applied,
        args: &BuildArgs,
        render: &RenderSettings,
    ) -> Self {
        let shown = |path: &Path| shown_path(cwd, path).display().to_string();
        Self {
            kind: "spec",
            file: shown(spec_path),
            position: resolved.position.as_ref().map(PositionFacts::from),
            chain: resolved
                .chain
                .iter()
                .map(|link| ChainFact {
                    name: link.name.as_ref().map(ToString::to_string),
                    file: shown(&link.path),
                })
                .collect(),
            content: shown(&resolved.content.path),
            pdf: None,
            style: render.style.to_string(),
            paper: paper_name(render.paper),
            drafts: args.select.drafts,
            overrides: applied.overrides.iter().map(|o| OverrideFact::of(cwd, o)).collect(),
            environment: EnvironmentFact::of(&RenderSettings::from(&args.render), &args.out),
            version: env!("CARGO_PKG_VERSION"),
            content_at: resolved.content.at.clone(),
            fonts_changed: args.render.fonts != FontsMode::default(),
        }
    }

    /// Where the spec wrote `content =`.
    pub(super) fn content_at(&self) -> &Located {
        &self.content_at
    }

    /// The record for one build, which names the PDF it wrote or would write.
    pub(super) fn record(&self, pdf: Option<&Path>) -> Self {
        Self {
            pdf: pdf.map(|p| p.display().to_string()),
            ..self.clone()
        }
    }

    /// The lines a person reads under the build line, without indentation.
    pub(super) fn lines(&self) -> Vec<String> {
        let mut head = format!("spec: {}", self.file);
        if !self.overrides.is_empty() {
            head.push_str(", plus overrides");
        }
        if self.drafts {
            head.push_str(", with drafts");
        }
        let mut out = vec![head];
        if let Some(position) = &self.position
            && let Some(line) = position.line()
        {
            out.push(format!("position: {line}"));
        }
        let chain: Vec<String> = self
            .chain
            .iter()
            .map(|link| link.name.clone().unwrap_or_else(|| "the spec".to_string()))
            .collect();
        out.push(format!("profiles: {}", chain.join(" -> ")));
        out.push(format!("content: {}", self.content));
        for item in &self.overrides {
            out.push(item.line());
        }
        if let Some(line) = self.environment_line() {
            out.push(line);
        }
        out
    }

    fn environment_line(&self) -> Option<String> {
        let env = &self.environment;
        let mut parts = Vec::new();
        if self.fonts_changed {
            parts.push(format!("--fonts {}", env.fonts));
        }
        parts.extend(env.font_path.iter().map(|p| format!("--font-path {p}")));
        parts.extend(env.styles_dir.iter().map(|p| format!("--styles-dir {p}")));
        parts.extend(env.max_pages.map(|n| format!("--max-pages {n}")));
        if env.out != "out" {
            parts.push(format!("-o {}", env.out));
        }
        (!parts.is_empty()).then(|| format!("environment: {}", parts.join("; ")))
    }
}

impl PositionFacts {
    /// The facts that are set, in one line; `None` when the spec says nothing about the position.
    fn line(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        parts.extend(self.company.clone());
        parts.extend(self.title.clone());
        let labelled = [
            ("level", &self.level),
            ("domain", &self.domain),
            ("audience", &self.audience),
            ("dated", &self.date),
            ("posting", &self.posting),
        ];
        parts.extend(
            labelled
                .into_iter()
                .filter_map(|(label, value)| value.as_ref().map(|v| format!("{label} {v}"))),
        );
        let emphasis = &self.emphasis;
        let axes = [
            ("business", &emphasis.business),
            ("org", &emphasis.org),
            ("technical", &emphasis.technical),
        ];
        let axes: Vec<String> = axes
            .into_iter()
            .filter_map(|(axis, level)| level.as_ref().map(|l| format!("{axis} {l}")))
            .collect();
        if !axes.is_empty() {
            parts.push(format!("emphasis {}", axes.join(", ")));
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    }
}

impl OverrideFact {
    /// `override: --tags ai replaces tags a, b (profiles/p.toml:2:9, positions/acme.toml:5:9)`.
    fn line(&self) -> String {
        let set = if self.set.is_empty() {
            String::new()
        } else {
            format!(" {}", self.set.join(", "))
        };
        let replaced = match &self.replaced {
            None => "replaces nothing the spec chose".to_string(),
            Some(replaced) => {
                let what = std::iter::once(replaced.setting.clone())
                    .chain((!replaced.value.is_empty()).then(|| replaced.value.join(", ")))
                    .collect::<Vec<_>>()
                    .join(" ");
                if self.at.is_empty() {
                    format!("replaces {what}")
                } else {
                    format!("replaces {what} ({})", self.at.join(", "))
                }
            }
        };
        let cleared = self.cleared_groups.as_ref().map_or_else(String::new, |groups| {
            format!(
                "; cleared explicit groups [{}] from {}",
                groups.names.join(", "),
                groups.at
            )
        });
        format!("override: {}{set} {replaced}{cleared}", self.flag)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The letter of a spec
// ---------------------------------------------------------------------------------------------------------------

/// One effective letter setting and where it came from.
#[derive(Debug, Clone, Serialize)]
struct SettingFact {
    value: String,
    /// `flag`, `default`, the file and line that wrote it, or for company and title the spec's file.
    from: String,
}

impl SettingFact {
    fn new(cwd: &Path, value: impl ToString, origin: &Origin) -> Self {
        let from = match origin {
            Origin::Flag => "flag".to_string(),
            Origin::Default => "default".to_string(),
            Origin::Spec(at) => located(cwd, at),
            Origin::Build(at) => format!("build, {}", located(cwd, at)),
            Origin::Position(file) => shown_path(cwd, file).display().to_string(),
        };
        Self {
            value: value.to_string(),
            from,
        }
    }

    fn text(&self) -> String {
        format!("{} ({})", self.value, self.from)
    }
}

/// The flags about the machine, shared by what the resume and the letter of a spec report.
impl EnvironmentFact {
    fn of(render: &RenderSettings, out: &Path) -> Self {
        Self {
            fonts: render.fonts.to_string(),
            font_path: render.font_path.iter().map(|p| p.display().to_string()).collect(),
            styles_dir: render.styles_dir.as_ref().map(|p| p.display().to_string()),
            max_pages: render.max_pages.map(|p| p.into_inner()),
            out: out.display().to_string(),
        }
    }
}

/// Everything a letter built from a spec reports about its spec. Spec data goes only here, never into the PDF.
///
/// Field values (a recipient, a date) are shown through the same scrub as every other text a spec supplies.
#[derive(Debug, Clone, Serialize)]
pub(super) struct LetterFacts {
    #[serde(rename = "type")]
    kind: &'static str,
    file: String,
    position: Option<PositionFacts>,
    chain: Vec<ChainFact>,
    /// The letter file, never the resume.
    content: String,
    output: SettingFact,
    links: SettingFact,
    max_length: Option<SettingFact>,
    length_unit: Option<SettingFact>,
    /// The company, title, recipient, and date that are set, by the name a placeholder uses.
    fields: std::collections::BTreeMap<&'static str, SettingFact>,
    /// What the PDF is made with; only meaningful when a PDF is made, so `null` for a text letter.
    style: Option<SettingFact>,
    paper: Option<SettingFact>,
    drafts: bool,
    /// The PDF this run wrote or would write; `null` when none was.
    pdf: Option<String>,
    overrides: Vec<OverrideFact>,
    environment: EnvironmentFact,
    version: &'static str,
    #[serde(skip)]
    fonts_changed: bool,
    #[serde(skip)]
    content_at: Located,
}

impl LetterFacts {
    /// The facts of building the letter of `resolved`, the spec at `spec_path`, with `applied` and `args`.
    pub(super) fn new(
        cwd: &Path,
        spec_path: &Path,
        resolved: &ResolvedSpec,
        letter: &ResolvedLetter,
        applied: &AppliedLetter,
        args: &LetterArgs,
        out: &Path,
    ) -> Self {
        let shown_file = |path: &Path| shown_path(cwd, path).display().to_string();
        let choices = &applied.choices;
        let make_pdf = choices.output.value != crate::letter::LetterOutput::Text;
        let mut fields = std::collections::BTreeMap::new();
        let mut field = |name: &'static str, chosen: Option<(String, &Origin)>| {
            if let Some((value, origin)) = chosen {
                fields.insert(name, SettingFact::new(cwd, shown(&value), origin));
            }
        };
        field(
            "company",
            choices.company.as_ref().map(|c| (c.value.to_string(), &c.origin)),
        );
        field(
            "title",
            choices.title.as_ref().map(|c| (c.value.to_string(), &c.origin)),
        );
        field(
            "recipient",
            choices.recipient.as_ref().map(|c| (c.value.to_string(), &c.origin)),
        );
        field("date", choices.date.as_ref().map(|c| (c.value.to_string(), &c.origin)));
        // The machine flags as typed: a default the command fills in (one page) is not something the user said.
        let typed = RenderSettings {
            max_pages: args.render.max_pages,
            ..RenderSettings::from(&args.render_for(choices))
        };
        Self {
            kind: "letter-spec",
            file: shown_file(spec_path),
            position: resolved.position.as_ref().map(PositionFacts::from),
            chain: resolved
                .chain
                .iter()
                .map(|link| ChainFact {
                    name: link.name.as_ref().map(ToString::to_string),
                    file: shown_file(&link.path),
                })
                .collect(),
            content: shown_file(&letter.content.path),
            output: SettingFact::new(cwd, choices.output.value, &choices.output.origin),
            links: SettingFact::new(cwd, choices.links.value, &choices.links.origin),
            max_length: choices
                .max_length
                .as_ref()
                .map(|m| SettingFact::new(cwd, m.value, &m.origin)),
            length_unit: choices
                .max_length
                .as_ref()
                .map(|_| SettingFact::new(cwd, choices.length_unit.value, &choices.length_unit.origin)),
            fields,
            style: make_pdf.then(|| SettingFact::new(cwd, &choices.style.value, &choices.style.origin)),
            paper: make_pdf.then(|| SettingFact::new(cwd, paper_name(choices.paper.value), &choices.paper.origin)),
            drafts: args.select.drafts,
            pdf: None,
            overrides: applied.overrides.iter().map(|o| OverrideFact::of(cwd, o)).collect(),
            environment: EnvironmentFact::of(&typed, out),
            version: env!("CARGO_PKG_VERSION"),
            fonts_changed: typed.fonts != FontsMode::default(),
            content_at: letter.content.at.clone(),
        }
    }

    /// Where the spec wrote `[letter] content =`.
    pub(super) fn content_at(&self) -> &Located {
        &self.content_at
    }

    /// The record for one run, which names the PDF it wrote or would write.
    pub(super) fn record(&self, pdf: Option<&Path>) -> Self {
        Self {
            pdf: pdf.map(|p| p.display().to_string()),
            ..self.clone()
        }
    }

    /// The lines a person reads under the letter's line, without indentation.
    pub(super) fn lines(&self) -> Vec<String> {
        let mut head = format!("spec: {}", self.file);
        if !self.overrides.is_empty() {
            head.push_str(", plus overrides");
        }
        if self.drafts {
            head.push_str(", with drafts");
        }
        let mut out = vec![head];
        if let Some(line) = self.position.as_ref().and_then(PositionFacts::line) {
            out.push(format!("position: {line}"));
        }
        let chain: Vec<String> = self
            .chain
            .iter()
            .map(|link| link.name.clone().unwrap_or_else(|| "the spec".to_string()))
            .collect();
        out.push(format!("profiles: {}", chain.join(" -> ")));
        out.push(format!("letter: {}", self.content));
        let mut settings = vec![
            format!("output {}", self.output.text()),
            format!("links {}", self.links.text()),
        ];
        settings.extend(self.max_length.iter().map(|m| format!("max-length {}", m.text())));
        settings.extend(self.length_unit.iter().map(|u| format!("length-unit {}", u.text())));
        settings.extend(self.style.iter().map(|s| format!("style {}", s.text())));
        settings.extend(self.paper.iter().map(|p| format!("paper {}", p.text())));
        out.push(format!("settings: {}", settings.join("; ")));
        if !self.fields.is_empty() {
            let fields: Vec<String> = self.fields.iter().map(|(k, v)| format!("{k} {}", v.text())).collect();
            out.push(format!("fields: {}", fields.join("; ")));
        }
        for item in &self.overrides {
            out.push(item.line());
        }
        if let Some(line) = self.environment_line() {
            out.push(line);
        }
        out
    }

    fn environment_line(&self) -> Option<String> {
        let env = &self.environment;
        let mut parts = Vec::new();
        if self.fonts_changed {
            parts.push(format!("--fonts {}", env.fonts));
        }
        parts.extend(env.font_path.iter().map(|p| format!("--font-path {p}")));
        parts.extend(env.styles_dir.iter().map(|p| format!("--styles-dir {p}")));
        parts.extend(env.max_pages.map(|n| format!("--max-pages {n}")));
        if env.out != "out" {
            parts.push(format!("-o {}", env.out));
        }
        (!parts.is_empty()).then(|| format!("environment: {}", parts.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_position_is_shown_without_control_characters_so_it_cannot_drive_the_terminal() {
        let position = Position {
            company: Some(crate::model::Company::try_new("Acme").expect("a company")),
            ..Position::default()
        };
        let mut facts = PositionFacts::from(&position);
        facts.title = Some(shown("Principal\u{1b}[31m Architect"));
        let line = facts.line().expect("a position with a company has a line");
        assert!(!line.contains('\u{1b}'), "{line:?}");
        assert!(line.starts_with("Acme; "), "{line}");
    }

    #[test]
    fn an_override_over_nothing_says_so_and_a_switch_shows_no_value() {
        let nothing = OverrideFact {
            cleared_groups: None,
            flag: "--style".into(),
            set: vec!["mono".into()],
            replaced: None,
            at: vec![],
        };
        assert_eq!(nothing.line(), "override: --style mono replaces nothing the spec chose");
        let switch = OverrideFact {
            cleared_groups: None,
            flag: "--core-only".into(),
            set: vec![],
            replaced: Some(ReplacedFact {
                setting: "tags".into(),
                value: vec!["a".into(), "b".into()],
            }),
            at: vec!["p.toml:2:9".into(), "s.toml:5:9".into()],
        };
        assert_eq!(
            switch.line(),
            "override: --core-only replaces tags a, b (p.toml:2:9, s.toml:5:9)"
        );
    }

    #[test]
    fn a_path_under_the_current_directory_is_shown_relative_to_it() {
        let cwd = Path::new("/work/here");
        assert_eq!(
            shown_path(cwd, Path::new("/work/here/positions/../profiles/a.toml")),
            Path::new("profiles").join("a.toml")
        );
        assert_eq!(
            shown_path(cwd, Path::new("/elsewhere/a.toml")),
            Path::new("/elsewhere/a.toml")
        );
    }
}

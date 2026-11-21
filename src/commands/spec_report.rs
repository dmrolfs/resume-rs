//! ABOUTME: What a spec build tells a person: the position, the profile chain, the content, overrides, environment.
//! ABOUTME: One value feeds both the human lines and the JSON `spec` record, so the two cannot disagree.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::{BuildArgs, RenderArgs};
use crate::model::options::FontsMode;
use crate::spec::error::{shown, tidy_path};
use crate::spec::{Applied, Located, Override, Position, Replaced, ResolvedSpec, paper_name};

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
    /// Where the spec chose it, one place for each file.
    at: Vec<String>,
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

/// How a path is shown: tidied, and relative to the current directory when it is under it. Never opened.
pub(super) fn shown_path(cwd: &Path, path: &Path) -> PathBuf {
    let tidy = tidy_path(path);
    tidy.strip_prefix(cwd).map_or(tidy.clone(), Path::to_path_buf)
}

/// `file:line:column` of a place, with the file shown as [`shown_path`] shows it.
pub(super) fn located(cwd: &Path, at: &Located) -> String {
    format!("{}:{}:{}", shown_path(cwd, &at.path).display(), at.line, at.column)
}

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
        render: &RenderArgs,
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
            environment: EnvironmentFact {
                fonts: args.render.fonts.to_string(),
                font_path: args.render.font_path.iter().map(|p| p.display().to_string()).collect(),
                styles_dir: args.render.styles_dir.as_ref().map(|p| p.display().to_string()),
                max_pages: args.render.max_pages.map(|p| p.into_inner()),
                out: args.out.display().to_string(),
            },
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
        format!("override: {}{set} {replaced}", self.flag)
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
            flag: "--style".into(),
            set: vec!["mono".into()],
            replaced: None,
            at: vec![],
        };
        assert_eq!(nothing.line(), "override: --style mono replaces nothing the spec chose");
        let switch = OverrideFact {
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

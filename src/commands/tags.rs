//! ABOUTME: `resume tags`: lists the tags, keywords, and alternate groups one resume's annotations offer.
//! ABOUTME: What a position spec can name; a human listing, or one JSON object with `--format json`.

use std::fs;
use std::path::Path;

use serde::Serialize;
use snafu::ResultExt;

use super::emit::Reporter;
use super::{Env, Io, Outcome};
use crate::cli::{Cli, TagsArgs};
use crate::diagnostics::{Diagnostic, Summary};
use crate::error::{Error, ReadInputSnafu};
use crate::markdown::parse;
use crate::model::options::OutputFormat;
use crate::select::Inventory;

/// Prints the inventory of one resume to stdout. A resume with format errors prints its findings and no
/// inventory, because a partial list would read as "this name is not used".
pub(super) fn run(cli: &Cli, args: &TagsArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    let mut out = Reporter::new(cli.format, &env.cwd, io);
    match inventory(cli, &args.file, env, &mut out) {
        Ok(outcome) => outcome,
        Err(error) => {
            out.error(&error);
            Outcome::Failure
        }
    }
}

/// The one JSON object `--format json` prints.
#[derive(Serialize)]
struct Record<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    file: String,
    #[serde(flatten)]
    inventory: &'a Inventory,
}

fn inventory(cli: &Cli, file: &Path, env: &Env, out: &mut Reporter<'_, '_>) -> Result<Outcome, Error> {
    let text = fs::read_to_string(file).context(ReadInputSnafu { path: file })?;
    let parsed = parse(file, &text);
    let shown = file.strip_prefix(&env.cwd).unwrap_or(file);
    let Some(resume) = parsed.valid() else {
        let mut sources = out.sources();
        sources.insert(file, &text);
        let findings: Vec<&Diagnostic> = parsed.diagnostics().iter().collect();
        out.diagnostics(&sources, &findings);
        out.line(&format!(
            "error {}  {}  no inventory",
            shown.display(),
            Summary::of(findings.iter().copied())
        ));
        return Ok(Outcome::Failure);
    };
    let inventory = Inventory::of(resume, &parsed.node_lines(&text));
    match cli.format {
        OutputFormat::Human => out.product(&listing(&inventory)),
        OutputFormat::Json => out.record(&Record {
            kind: "inventory",
            file: shown.display().to_string(),
            inventory: &inventory,
        }),
    }
    Ok(Outcome::Success)
}

/// The human listing: tags, keywords, and groups, each aligned, with the lines where they are.
fn listing(inventory: &Inventory) -> String {
    // A person needs a few places to look; JSON has every line.
    const SHOWN: usize = 6;
    let lines = |lines: &[usize]| {
        let mut shown: Vec<String> = lines.iter().take(SHOWN).map(ToString::to_string).collect();
        if lines.len() > SHOWN {
            shown.push(format!("and {} more", lines.len() - SHOWN));
        }
        shown.join(", ")
    };
    let mut out = String::from("tags\n");
    if inventory.tags.is_empty() {
        out.push_str("  (none)\n");
    }
    let width = inventory.tags.iter().map(|t| t.name.chars().count()).max().unwrap_or(0) + 1;
    for tag in &inventory.tags {
        let inherited = match tag.inherited {
            0 => String::new(),
            n => format!(", and {n} more under it"),
        };
        out.push_str(&format!(
            "  {:<width$}  {} own (lines {}){inherited}\n",
            format!("#{}", tag.name),
            tag.own.len(),
            lines(&tag.own),
        ));
    }
    out.push_str("keywords\n");
    for keyword in &inventory.keywords {
        let at = if keyword.lines.is_empty() {
            String::new()
        } else {
            format!(" (lines {})", lines(&keyword.lines))
        };
        out.push_str(&format!(
            "  {:<8} {}{at}\n",
            format!("@{}", keyword.name),
            keyword.lines.len()
        ));
    }
    if !inventory.ids.is_empty() {
        out.push_str("ids\n");
        let width = inventory.ids.iter().map(|i| i.name.chars().count()).max().unwrap_or(0) + 1;
        for id in &inventory.ids {
            out.push_str(&format!(
                "  {:<width$}  {}  line {}\n",
                format!("&{}", id.name),
                id.kind,
                id.line
            ));
        }
    }
    out.push_str("groups\n");
    if inventory.groups.is_empty() {
        out.push_str("  (none)\n");
    }
    for group in &inventory.groups {
        out.push_str(&format!("  {} ({}s)\n", group.name, group.kind));
        let width = group.variants.iter().map(|v| v.name.chars().count()).max().unwrap_or(0);
        for variant in &group.variants {
            let marks: Vec<&str> = [(variant.default, "default"), (variant.draft, "draft")]
                .into_iter()
                .filter_map(|(on, mark)| on.then_some(mark))
                .collect();
            let marks = if marks.is_empty() {
                String::new()
            } else {
                format!(" [{}]", marks.join(", "))
            };
            out.push_str(&format!(
                "    ={}.{:<width$}  line {}{marks}  \"{}\"\n",
                group.name, variant.name, variant.line, variant.text
            ));
        }
    }
    out
}

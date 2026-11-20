//! ABOUTME: Small guides built from the code's own facts, for the negative controls to break one thing at a time.
//! ABOUTME: A control that edited docs/User-Guide.md would depend on its exact words; these depend only on the code.

use crate::checks::REQUIRED;
use crate::facts;

/// Every required heading, each with a few lines of body.
pub fn headings() -> String {
    REQUIRED
        .iter()
        .map(|(level, text)| {
            format!(
                "{} {text}\n\nA first line.\n\nA second line.\n\nA third line.\n\n",
                "#".repeat(*level)
            )
        })
        .collect()
}

/// A planned section named `title`, with `witness` as the feature whose arrival ends it, and `extra` after its stub.
pub fn planned(title: &str, witness: &str, extra: &str) -> String {
    format!(
        "## {title}\n\n<!-- guide:planned rsm-0.0 witness={witness} -->\n\n**Planned, not available yet.**\n\n{extra}\n"
    )
}

/// The Theme keys table: one row per key any built-in theme defines, with the styles that define it.
pub fn theme_keys() -> String {
    let by_style = facts::theme_keys_by_style();
    let all_keys: std::collections::BTreeSet<&String> = by_style.values().flatten().collect();
    let mut out = String::from("### Theme keys\n\n| Key | Unit | Styles |\n|---|---|---|\n");
    for key in all_keys {
        let styles: Vec<&str> = by_style
            .iter()
            .filter(|(_, keys)| keys.contains(key))
            .map(|(name, _)| name.as_str())
            .collect();
        let cell = if styles.len() == by_style.len() {
            "all".to_string()
        } else {
            styles.join(", ")
        };
        out.push_str(&format!("| `{key}` | length | {cell} |\n"));
    }
    out
}

/// The Shared components table: one row per public component.
pub fn components() -> String {
    let mut out = String::from("### Shared components\n\n| Component | What it does |\n|---|---|\n");
    for name in facts::components() {
        out.push_str(&format!("| `{name}` | does a thing |\n"));
    }
    out
}

/// The Built-in styles section: one `####` heading per style.
pub fn styles() -> String {
    let mut out = String::from("### Built-in styles\n\n");
    for name in facts::built_in_styles() {
        out.push_str(&format!("#### {name}\n\nWhen to choose it.\n\n"));
    }
    out
}

/// The row of one option, in the form the guide uses.
fn option_row(option: &facts::CliOption) -> String {
    let first = match (option.positional, option.short) {
        (true, _) => format!("`{}...`", option.key()),
        (false, Some(short)) => format!("`-{short}, {}`", option.key()),
        (false, None) => format!("`{}`", option.key()),
    };
    let mut second: Vec<String> = Vec::new();
    if !option.values.is_empty() {
        second.push(
            option
                .values
                .iter()
                .map(|v| format!("`{v}`"))
                .collect::<Vec<_>>()
                .join(" or "),
        );
    }
    if option.repeatable {
        second.push("repeatable".to_string());
    }
    if let Some(default) = &option.default {
        second.push(format!("default `{default}`"));
    }
    format!("| {first} | {} | what it does |\n", second.join("; "))
}

fn option_table(options: &[facts::CliOption]) -> String {
    let rows: String = options.iter().map(option_row).collect();
    format!("| Option | Values and default | What it does |\n|---|---|---|\n{rows}\n")
}

/// The Commands section: global options, one `### resume NAME` section per command, and the environment variables.
pub fn commands() -> String {
    let mut out = String::from("## Commands\n\n### Global options\n\n");
    out.push_str(&option_table(&facts::global_options()));
    for (name, options) in facts::commands() {
        out.push_str(&format!("### resume {name}\n\nWhat it does.\n\n"));
        out.push_str(&option_table(&options));
    }
    out
}

/// The Fonts section: the roles, the fields, the families, the three font codes linked, and the names it mentions.
pub fn fonts() -> String {
    let mut out = String::from("## Fonts\n\n");
    for role in facts::font_roles() {
        out.push_str(&format!("- `{role}`: a role.\n"));
    }
    for field in ["preferred", "fallback", "metric-compatible"] {
        out.push_str(&format!("- `{field}`: a field.\n"));
    }
    for family in facts::bundled_families() {
        out.push_str(&format!("- {family}: bundled.\n"));
    }
    for code in ["font-fallback-metric", "font-fallback-layout", "font-missing"] {
        out.push_str(&format!("- [`{code}`](#code-{code}).\n"));
    }
    out.push_str("\nRESUME_FONT_PATH, --fonts, --font-path, and resume licenses.\n");
    out
}

/// The Environment variables section, naming each of `names`.
pub fn environment(names: &[&str]) -> String {
    let list: String = names.iter().map(|n| format!("`{n}` does something.\n\n")).collect();
    format!("### Environment variables\n\n{list}")
}

//! ABOUTME: The checks that bind the guide to the code. Each returns a list of problems; empty means the guide is true.
//! ABOUTME: They read hooks (headings, table cells, fence info strings, markers) and compare sets, never wording.

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use resume::cli::Cli;
use resume::diagnostics::DiagnosticCode;
use resume::diagnostics::code::Category;
use strum::IntoEnumIterator;

use crate::extract::{Guide, Heading, plain};
use crate::facts::{self, CliOption};

/// The headings the guide must have, in order: `(level, text)`. Each exactly once.
pub const REQUIRED: [(usize, &str); 18] = [
    (2, "Quick start"),
    (2, "Writing your resume"),
    (2, "Checking your resume"),
    (2, "Fonts"),
    (2, "Styles"),
    (3, "Built-in styles"),
    (3, "Customizing a style"),
    (3, "Theme keys"),
    (3, "Shared components"),
    (3, "Writing a style"),
    (2, "Commands"),
    (3, "Global options"),
    (3, "Planned commands"),
    (2, "Annotations"),
    (2, "Profiles and position specs"),
    (2, "Troubleshooting"),
    (2, "Appendix: diagnostic codes"),
    (2, "Where to read more"),
];

/// The resume text an `in-section` example is placed after.
pub const SECTION_PREAMBLE: &str = "# Ada Lovelace\n\nLondon, UK · ada@example.com\n\n## Experience\n\n";

fn section<'a>(guide: &'a Guide, level: usize, text: &str) -> Result<&'a Heading, String> {
    guide
        .heading(level, text)
        .ok_or_else(|| format!("the guide has no `{} {text}` heading", "#".repeat(level)))
}

fn words(info: &str) -> Vec<&str> {
    info.split_whitespace().collect()
}

// ---- required headings and planned sections --------------------------------------------------------

pub fn required_headings(guide: &Guide) -> Vec<String> {
    let mut problems = Vec::new();
    let mut last_line = 0;
    for (level, text) in REQUIRED {
        let found: Vec<&Heading> = guide
            .headings
            .iter()
            .filter(|h| h.level == level && h.text == text)
            .collect();
        match found.as_slice() {
            [] => problems.push(format!(
                "docs/User-Guide.md has no `{} {text}` heading",
                "#".repeat(level)
            )),
            [one] => {
                if one.line < last_line {
                    problems.push(format!(
                        "`{text}` (line {}) is out of order: it belongs after line {last_line}",
                        one.line
                    ));
                }
                last_line = last_line.max(one.line);
                let planned = guide.markers_in(Guide::range(one)).any(|m| m.kind == "planned");
                let body = guide.body(one).lines().skip(1).filter(|l| !l.trim().is_empty()).count();
                if body < 3 && !planned {
                    problems.push(format!("`{text}` (line {}) is empty: write the section", one.line));
                }
            }
            _ => problems.push(format!("`{text}` appears more than once")),
        }
    }
    problems
}

/// What a planned section's witness looks like: `build:--tags` (an option) or `command:watch` (a command).
fn witness_exists(witness: &str) -> Option<bool> {
    let (kind, value) = witness.split_once(':')?;
    match kind {
        "command" => Some(facts::commands().contains_key(value)),
        "build" => Some(
            facts::commands()["build"]
                .iter()
                .any(|o| !o.positional && format!("--{}", o.name) == value),
        ),
        _ => None,
    }
}

pub fn planned_sections(guide: &Guide) -> Vec<String> {
    let mut problems = Vec::new();
    let option = Regex::new(r"`--[a-z]").expect("a valid regex");
    let mut seen = 0;
    for marker in guide.markers.iter().filter(|m| m.kind == "planned") {
        seen += 1;
        let Some(heading) = guide
            .headings
            .iter()
            .filter(|h| h.start < marker.start && marker.start < h.end)
            .max_by_key(|h| h.level)
        else {
            problems.push(format!("line {}: a planned marker outside any section", marker.line));
            continue;
        };
        let body = guide.body(heading);
        if guide.fences_in(Guide::range(heading)).next().is_some() {
            problems.push(format!(
                "`{}` is planned and has a fenced block: a planned section documents nothing yet",
                heading.text
            ));
        }
        if option.is_match(body) {
            problems.push(format!(
                "`{}` is planned and names an option: it cannot, the option does not exist",
                heading.text
            ));
        }
        if body.lines().count() > 14 {
            problems.push(format!(
                "`{}` is planned and longer than 14 lines: a stub is short",
                heading.text
            ));
        }
        let args = marker.args.split_whitespace().collect::<Vec<_>>();
        let witness = args.iter().find_map(|a| a.strip_prefix("witness="));
        match witness.map(witness_exists) {
            Some(Some(true)) => problems.push(format!(
                "`{}` is marked planned, but `{}` now exists: write the section from {}, delete the planned marker, and add its checks to tests/guide",
                heading.text,
                witness.unwrap_or_default(),
                args.first().unwrap_or(&"its bead")
            )),
            Some(Some(false)) => {}
            Some(None) => problems.push(format!("line {}: the witness `{}` is not `command:NAME` or `build:--OPTION`", marker.line, witness.unwrap_or_default())),
            None => problems.push(format!("line {}: a planned marker needs `witness=KIND:VALUE`", marker.line)),
        }
    }
    if seen == 0 {
        problems.push(
            "the guide has no planned section: Annotations and Profiles and position specs are still planned"
                .to_string(),
        );
    }
    problems
}

// ---- Markdown examples and shell examples ----------------------------------------------------------

/// The codes parsing and linting `text` report, as `resume check` does.
pub fn codes_of(text: &str) -> BTreeSet<String> {
    let parsed = resume::markdown::parse("example.md", text);
    let mut codes: BTreeSet<String> = parsed.diagnostics().iter().map(|d| d.code.to_string()).collect();
    codes.extend(
        resume::doctor::content::lint_parsed(&parsed, std::path::Path::new("example.md"))
            .iter()
            .map(|d| d.code.to_string()),
    );
    codes
}

pub fn markdown_examples(guide: &Guide) -> Vec<String> {
    let mut problems = Vec::new();
    let (mut checked, mut clean_resume, mut unchecked) = (0, 0, 0);
    for fence in guide
        .fences
        .iter()
        .filter(|f| words(&f.info).first() == Some(&"markdown"))
    {
        let info = words(&fence.info);
        let at = format!("line {}", fence.line);
        let mode: Vec<&&str> = info
            .iter()
            .filter(|w| matches!(**w, "resume" | "in-section" | "unchecked"))
            .collect();
        if mode.len() != 1 {
            problems.push(format!(
                "{at}: a `markdown` fence needs exactly one of `resume`, `in-section`, `unchecked` (found {mode:?})"
            ));
            continue;
        }
        if *mode[0] == "unchecked" {
            unchecked += 1;
            if guide
                .headings
                .iter()
                .filter(|h| h.start < fence.start && fence.start < h.end)
                .any(|h| guide.markers_in(Guide::range(h)).any(|m| m.kind == "planned"))
            {
                problems.push(format!("{at}: an unchecked example inside a planned section"));
            }
            continue;
        }
        let Some(expect) = info.iter().find_map(|w| w.strip_prefix("expect=")) else {
            problems.push(format!(
                "{at}: a checked example needs `expect=none` or `expect=code,code`"
            ));
            continue;
        };
        let expected: BTreeSet<String> = if expect == "none" {
            BTreeSet::new()
        } else {
            expect.split(',').map(str::to_string).collect()
        };
        let text = if *mode[0] == "in-section" {
            format!("{SECTION_PREAMBLE}{}", fence.body)
        } else {
            fence.body.clone()
        };
        let found = codes_of(&text);
        checked += 1;
        if *mode[0] == "resume" && expected.is_empty() {
            clean_resume += 1;
        }
        if found != expected {
            problems.push(format!(
                "{at}: the example is documented as producing {expected:?} but producing {found:?}; fix the text or its `expect=`"
            ));
        }
        for code in &expected {
            if !fence.links_before.iter().any(|l| l == &format!("#code-{code}")) {
                problems.push(format!(
                    "{at}: the paragraph before the example must link `{code}` as [`{code}`](#code-{code}), so the sentence the reader sees is the one that is checked"
                ));
            }
        }
    }
    if unchecked > 1 {
        problems.push(format!(
            "{unchecked} unchecked examples: at most one is allowed; use a `text` fence for text that is not a resume"
        ));
    }
    if clean_resume == 0 {
        problems.push(
            "the guide has no `markdown resume expect=none` example: it must show one whole resume that is clean"
                .to_string(),
        );
    }
    if checked < 5 {
        problems.push(format!(
            "only {checked} checked Markdown examples: the extractor may be broken, or the guide has lost its examples"
        ));
    }
    problems
}

pub fn shell_examples(guide: &Guide) -> Vec<String> {
    let mut problems = Vec::new();
    let mut lines = 0;
    for fence in guide.fences.iter().filter(|f| words(&f.info).first() == Some(&"sh")) {
        for (i, line) in fence.body.lines().enumerate() {
            let command = line.split(" #").next().unwrap_or(line).trim();
            if command.is_empty() || command.starts_with('#') {
                continue;
            }
            let at = format!("line {}", fence.line + 1 + i);
            if !command.starts_with("resume ") {
                problems.push(format!(
                    "{at}: a `sh` example line must start with `resume `: {command}"
                ));
                continue;
            }
            if command.contains(['"', '\'', '\\', '|', '>']) {
                problems.push(format!(
                    "{at}: keep example commands free of quotes, pipes, and redirects so they can be checked: {command}"
                ));
                continue;
            }
            lines += 1;
            if let Err(e) = Cli::try_parse_validated(command.split_whitespace()) {
                let first = e.to_string().lines().next().unwrap_or_default().to_string();
                problems.push(format!("{at}: `{command}` is not accepted by the program: {first}"));
            }
        }
    }
    if lines < 5 {
        problems.push(format!(
            "only {lines} checked shell lines: the extractor may be broken, or the guide has lost its examples"
        ));
    }
    problems
}

// ---- theme keys, components, styles ----------------------------------------------------------------

pub fn theme_keys(guide: &Guide) -> Vec<String> {
    let heading = match section(guide, 3, "Theme keys") {
        Ok(h) => h,
        Err(e) => return vec![e],
    };
    let Some(table) = guide.tables_in(Guide::range(heading)).next() else {
        return vec!["`### Theme keys` has no table".to_string()];
    };
    let styles_column = table.header.iter().position(|h| h == "Styles");
    let by_style = facts::theme_keys_by_style();
    let mut defined: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (style, keys) in &by_style {
        for key in keys {
            defined.entry(key.clone()).or_default().push(style.clone());
        }
    }
    let mut documented: BTreeMap<String, Option<String>> = BTreeMap::new();
    for row in &table.rows {
        let key = plain(&row[0]);
        let styles = styles_column.and_then(|i| row.get(i)).map(|c| plain(c));
        documented.insert(key, styles);
    }
    let mut problems = Vec::new();
    if documented.len() < 30 {
        problems.push(format!(
            "only {} theme keys documented: the table may have lost its rows",
            documented.len()
        ));
    }
    if styles_column.is_none() {
        problems.push("the theme table needs a `Styles` column".to_string());
    }
    for (key, styles) in &defined {
        match documented.get(key) {
            None => problems.push(format!(
                "theme key `{key}` ({}) is not documented: add a row to the table under `### Theme keys`, with the key in backticks in the first cell",
                styles.join(", ")
            )),
            Some(cell) => {
                let expected = if styles.len() == by_style.len() { "all".to_string() } else { styles.join(", ") };
                if let Some(cell) = cell {
                    let given: BTreeSet<&str> = cell.split(',').map(str::trim).collect();
                    let want: BTreeSet<&str> = expected.split(',').map(str::trim).collect();
                    if given != want {
                        problems.push(format!("theme key `{key}`: the Styles column says `{cell}` but it is defined by `{expected}`"));
                    }
                }
            }
        }
    }
    for key in documented.keys().filter(|k| !defined.contains_key(*k)) {
        problems.push(format!(
            "the guide documents theme key `{key}`, which no built-in style defines"
        ));
    }
    problems
}

/// Every `theme.a.b` the Typst sources read must be a key some theme defines, or a prefix of one.
pub fn theme_reads_are_defined() -> Vec<String> {
    let read = Regex::new(r"(?:^|[^/\w.])theme((?:\.[a-z][a-z-]*)+)").expect("a valid regex");
    let defined: BTreeSet<String> = facts::theme_keys_by_style().into_values().flatten().collect();
    let mut problems = Vec::new();
    let mut seen = 0;
    for relative in [
        "styles/lib/components.typ",
        "styles/pragmatic/style.typ",
        "styles/mono/style.typ",
    ] {
        // Comments are prose and mention `theme.typ`; only code reads keys.
        let code: String = facts::read(relative)
            .lines()
            .map(|line| line.split("//").next().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");
        for cap in read.captures_iter(&code) {
            let path = cap[1].trim_start_matches('.').to_string();
            seen += 1;
            let normalized = if path.starts_with("fonts.") {
                match path.splitn(3, '.').nth(2) {
                    Some(field) => format!("fonts.<role>.{field}"),
                    None => "fonts.<role>".to_string(),
                }
            } else {
                path.clone()
            };
            let known = defined
                .iter()
                .any(|d| d == &normalized || d.starts_with(&format!("{normalized}.")));
            if !known {
                problems.push(format!("{relative} reads `theme.{path}`, which no theme defines"));
            }
        }
    }
    if seen < 30 {
        problems.push(format!(
            "only {seen} theme reads found in the Typst sources: the pattern may be broken"
        ));
    }
    problems
}

pub fn components(guide: &Guide) -> Vec<String> {
    let heading = match section(guide, 3, "Shared components") {
        Ok(h) => h,
        Err(e) => return vec![e],
    };
    let Some(table) = guide.tables_in(Guide::range(heading)).next() else {
        return vec!["`### Shared components` has no table".to_string()];
    };
    let documented: BTreeSet<String> = table.rows.iter().map(|r| plain(&r[0])).collect();
    let defined = facts::components();
    let mut problems = Vec::new();
    for name in defined.difference(&documented) {
        problems.push(format!(
            "component `{name}` of styles/lib/components.typ is not in the table under `### Shared components`"
        ));
    }
    for name in documented.difference(&defined) {
        problems.push(format!(
            "the table documents component `{name}`, which styles/lib/components.typ does not define"
        ));
    }
    if documented.len() < 20 {
        problems.push(format!(
            "only {} components documented: the table may have lost its rows",
            documented.len()
        ));
    }
    problems
}

pub fn styles(guide: &Guide) -> Vec<String> {
    let heading = match section(guide, 3, "Built-in styles") {
        Ok(h) => h,
        Err(e) => return vec![e],
    };
    let documented: BTreeSet<String> = guide
        .headings
        .iter()
        .filter(|h| h.level == 4 && heading.start < h.start && h.start < heading.end)
        .map(|h| h.text.clone())
        .collect();
    let defined: BTreeSet<String> = facts::built_in_styles().into_iter().collect();
    let mut problems = Vec::new();
    for name in defined.difference(&documented) {
        problems.push(format!("built-in style `{name}` has no `#### {name}` section under `### Built-in styles`: describe when to choose it"));
    }
    for name in documented.difference(&defined) {
        problems.push(format!("the guide describes style `{name}`, which is not built in"));
    }
    problems
}

// ---- commands and options --------------------------------------------------------------------------

/// The first cell of an option row, as the user types it, without backticks.
fn key_of(cell: &str) -> String {
    plain(cell)
}

fn check_option_rows(label: &str, options: &[CliOption], rows: &[Vec<String>]) -> Vec<String> {
    let mut problems = Vec::new();
    for option in options {
        let key = option.key();
        let Some(row) = rows.iter().find(|r| {
            let cell = key_of(&r[0]);
            if option.positional {
                cell.contains(&key)
            } else {
                cell.split(|c: char| !(c.is_alphanumeric() || c == '-'))
                    .any(|w| w == key)
            }
        }) else {
            problems.push(format!("{label}: `{key}` is not documented: add a row to its table"));
            continue;
        };
        let cell = key_of(&row[0]);
        if let Some(short) = option.short {
            // A whole token: `--out` contains `-o` and must not count as showing it.
            let shown = cell
                .split(|c: char| c.is_whitespace() || c == ',')
                .any(|token| token == format!("-{short}"));
            if !shown {
                problems.push(format!(
                    "{label}: the row for `{key}` should show its short form `-{short}`"
                ));
            }
        }
        let text = row.join(" ");
        for value in &option.values {
            if !text.contains(&format!("`{value}`")) {
                problems.push(format!(
                    "{label}: the row for `{key}` must list the value `{value}` in backticks"
                ));
            }
        }
        if let Some(default) = option.default.as_ref().filter(|d| !text.contains(&format!("`{d}`"))) {
            problems.push(format!(
                "{label}: the row for `{key}` must name its default `{default}` in backticks"
            ));
        }
        if option.repeatable && !option.positional && !text.contains("repeat") {
            problems.push(format!(
                "{label}: `{key}` can be given more than once, and its row must say it is repeatable"
            ));
        }
    }
    // A row for an option the command does not have documents something that does not exist.
    let keys: BTreeSet<String> = options.iter().map(CliOption::key).collect();
    for row in rows {
        for word in key_of(&row[0])
            .split(|c: char| !(c.is_alphanumeric() || c == '-'))
            .filter(|w| w.starts_with("--"))
        {
            if !keys.contains(word) {
                problems.push(format!(
                    "{label}: the guide documents `{word}`, which this command does not have"
                ));
            }
        }
    }
    problems
}

pub fn commands(guide: &Guide) -> Vec<String> {
    let commands_heading = match section(guide, 2, "Commands") {
        Ok(h) => h,
        Err(e) => return vec![e],
    };
    let mut problems = Vec::new();
    let range = Guide::range(commands_heading);

    // Which commands have a `### resume X` section.
    let documented: BTreeMap<String, &Heading> = guide
        .headings
        .iter()
        .filter(|h| h.level == 3 && range.contains(&h.start))
        .filter_map(|h| h.text.strip_prefix("resume ").map(|name| (name.to_string(), h)))
        .collect();
    let real = facts::commands();
    for name in real.keys().filter(|n| !documented.contains_key(*n)) {
        problems.push(format!(
            "command `resume {name}` has no `### resume {name}` section under `## Commands`"
        ));
    }
    for name in documented.keys().filter(|n| !real.contains_key(*n)) {
        problems.push(format!("the guide documents `resume {name}`, which is not a command"));
    }
    for (name, options) in &real {
        let Some(heading) = documented.get(name) else { continue };
        let rows: Vec<Vec<String>> = guide
            .tables_in(Guide::range(heading))
            .flat_map(|t| t.rows.clone())
            .collect();
        problems.extend(check_option_rows(&format!("resume {name}"), options, &rows));
    }

    // The options every command accepts are documented once.
    match section(guide, 3, "Global options") {
        Ok(heading) => {
            let rows: Vec<Vec<String>> = guide
                .tables_in(Guide::range(heading))
                .flat_map(|t| t.rows.clone())
                .collect();
            problems.extend(check_option_rows("global options", &facts::global_options(), &rows));
        }
        Err(e) => problems.push(e),
    }

    // No `--option` anywhere in the Commands section may be one that does not exist.
    let every = facts::every_long_option();
    let long = Regex::new(r"`(--[a-z][a-z-]*)").expect("a valid regex");
    for cap in long.captures_iter(guide.body(commands_heading)) {
        let name = &cap[1];
        if !every.contains(name.trim_start_matches("--")) {
            problems.push(format!("the Commands section mentions `{name}`, which no command has"));
        }
    }

    // A planned command is allowed only with a marker that names a witness, and only while it does not exist.
    if let Ok(planned) = section(guide, 3, "Planned commands") {
        let markers: Vec<_> = guide
            .markers_in(Guide::range(planned))
            .filter(|m| m.kind == "planned")
            .collect();
        if markers.is_empty() {
            problems.push("`### Planned commands` has no `guide:planned` marker".to_string());
        }
    }
    problems
}

// ---- diagnostics: the generated appendix and the links to codes ----------------------------------------

const START: &str = "<!-- guide:generated codes -->";
const END: &str = "<!-- /guide:generated -->";

/// What the appendix between the markers must say, from the code.
pub fn render_code_tables() -> String {
    let sentence = |category: Category| match category {
        Category::Format => "How the Markdown is written. Every command that reads a resume reports these.",
        Category::Content => "What the resume says. `doctor` and `check` report these.",
        Category::Layout => "How the rendered document looks. `build`, `doctor`, and `check` report these.",
        Category::Setup => {
            "The environment: styles, fonts, and the output location. `doctor` and `check` report these."
        }
    };
    let mut out = String::new();
    for category in Category::iter() {
        let title = {
            let name = category.to_string();
            let mut chars = name.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        };
        out.push_str(&format!(
            "### {title} codes\n\n{}\n\n| Code | Severity | What it means |\n|---|---|---|\n",
            sentence(category)
        ));
        for code in DiagnosticCode::iter().filter(|c| c.category() == category) {
            out.push_str(&format!(
                "| <a id=\"code-{code}\"></a>`{code}` | {} | {} |\n",
                code.default_severity(),
                code.advice().summary
            ));
        }
        out.push('\n');
    }
    out
}

/// The text between the markers, or why there is none.
pub fn generated_block(text: &str) -> Result<(usize, usize), String> {
    let start = text
        .find(START)
        .ok_or_else(|| format!("the guide has no `{START}` marker"))?
        + START.len();
    let end = text
        .find(END)
        .ok_or_else(|| format!("the guide has no `{END}` marker"))?;
    if end < start {
        return Err("the generated block ends before it starts".to_string());
    }
    Ok((start, end))
}

pub fn code_tables(guide: &Guide) -> Vec<String> {
    let (start, end) = match generated_block(&guide.text) {
        Ok(range) => range,
        Err(e) => return vec![e],
    };
    let committed = guide.text[start..end].trim();
    let wanted = render_code_tables();
    if committed == wanted.trim() {
        return Vec::new();
    }
    vec!["the diagnostic code tables are out of date with the code (a code was added, removed, or changed). Regenerate them with: cargo test --test guide -- --ignored bless_guide_tables".to_string()]
}

pub fn links(guide: &Guide) -> Vec<String> {
    let mut problems = Vec::new();
    let codes: BTreeSet<String> = DiagnosticCode::iter().map(|c| c.to_string()).collect();
    let slugs: BTreeSet<&str> = guide.headings.iter().map(|h| h.slug.as_str()).collect();
    let anchors: BTreeSet<&str> = guide.anchors.iter().map(String::as_str).collect();
    for link in &guide.links {
        let at = format!("line {}", link.line);
        if let Some(code) = link.url.strip_prefix("#code-") {
            if !codes.contains(code) {
                problems.push(format!("{at}: the link `{}` names no diagnostic code", link.url));
            }
        } else if let Some(target) = link.url.strip_prefix('#') {
            if !slugs.contains(target) && !anchors.contains(target) {
                problems.push(format!("{at}: the link `{}` points at no heading", link.url));
            }
        } else if !link.url.contains("://") && !link.url.starts_with("mailto:") {
            let path = link.url.split('#').next().unwrap_or_default();
            if !crate::root().join("docs").join(path).exists() {
                problems.push(format!(
                    "{at}: the link `{}` points at a file that does not exist",
                    link.url
                ));
            }
        }
    }
    if guide.links.len() < 10 {
        problems.push(format!("only {} links: the extractor may be broken", guide.links.len()));
    }
    problems
}

// ---- fonts -----------------------------------------------------------------------------------------

pub fn fonts(guide: &Guide) -> Vec<String> {
    let heading = match section(guide, 2, "Fonts") {
        Ok(h) => h,
        Err(e) => return vec![e],
    };
    let body = guide.body(heading);
    let lower = body.to_lowercase();
    let mut problems = Vec::new();
    for role in facts::font_roles() {
        if !body.contains(&format!("`{role}`")) {
            problems.push(format!(
                "`## Fonts` does not mention the font role `{role}` in backticks"
            ));
        }
    }
    for field in ["preferred", "fallback", "metric-compatible"] {
        if !body.contains(&format!("`{field}`")) {
            problems.push(format!("`## Fonts` does not explain `{field}`"));
        }
    }
    for family in facts::bundled_families() {
        if !lower.contains(&family) {
            problems.push(format!("`## Fonts` does not name the bundled family `{family}`"));
        }
    }
    for code in ["font-fallback-metric", "font-fallback-layout", "font-missing"] {
        if !guide
            .links
            .iter()
            .any(|l| l.url == format!("#code-{code}") && heading.start < l.start && l.start < heading.end)
        {
            problems.push(format!("`## Fonts` must link the font code [`{code}`](#code-{code})"));
        }
    }
    for name in ["RESUME_FONT_PATH", "--fonts", "--font-path", "resume licenses"] {
        if !body.contains(name) {
            problems.push(format!("`## Fonts` does not mention `{name}`"));
        }
    }
    problems
}

// ---- no personal resume content --------------------------------------------------------------------

/// The companies a guide example may name: those of the synthetic fixtures, and a few invented ones.
pub fn allowed_companies() -> BTreeSet<String> {
    let role = Regex::new(r"(?m)^### ([^\n·]+?) ·").expect("a valid regex");
    let mut allowed: BTreeSet<String> = ["Acme", "Acme Corp", "Globex", "Initech", "Example Corp"]
        .map(str::to_string)
        .into();
    let dir = crate::root().join("tests/fixtures");
    for entry in std::fs::read_dir(dir).expect("fixtures").filter_map(Result::ok) {
        if entry.path().extension().is_some_and(|e| e == "md") {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            allowed.extend(role.captures_iter(&text).map(|c| c[1].trim().to_string()));
        }
    }
    allowed
}

/// Hosts a link in the guide may have: reserved example domains and the documentation the guide points to.
const ALLOWED_HOSTS: [&str; 6] = [
    "example.com",
    "example.org",
    "example.net",
    "typst.app",
    "github.com",
    "docs.rs",
];

pub fn no_personal_content(guide: &Guide) -> Vec<String> {
    let mut problems = Vec::new();
    let email = Regex::new(r"[A-Za-z0-9._%+-]+@([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+)").expect("a valid regex");
    for cap in email.captures_iter(&guide.text) {
        let host = cap[1].to_lowercase();
        if !["example.com", "example.org", "example.net"].iter().any(|h| host == *h) {
            problems.push(format!(
                "the guide has the email `{}`: use an address at example.com",
                &cap[0]
            ));
        }
    }
    let url = Regex::new(r"https?://([A-Za-z0-9.-]+)").expect("a valid regex");
    for cap in url.captures_iter(&guide.text) {
        let host = cap[1].to_lowercase();
        let ok = ALLOWED_HOSTS
            .iter()
            .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")));
        if !ok {
            problems.push(format!(
                "the guide links to `{}`: only reserved example domains and documentation hosts are allowed",
                &cap[0]
            ));
        }
    }
    let phone = Regex::new(r"\+?\(?\d[\d ()\-.]{7,}\d").expect("a valid regex");
    let reserved = Regex::new(r"^\+44 20 7946 0\d{3}$|555-01\d\d$").expect("a valid regex");
    for m in phone.find_iter(&guide.text) {
        let digits = m.as_str().chars().filter(char::is_ascii_digit).count();
        if digits >= 9 && !reserved.is_match(m.as_str()) {
            problems.push(format!(
                "the guide has the phone-like number `{}`: use +44 20 7946 0xxx",
                m.as_str()
            ));
        }
    }
    let companies = allowed_companies();
    let role = Regex::new(r"(?m)^### ([^\n·]+?) ·").expect("a valid regex");
    let name = Regex::new(r"(?m)^# (.+)$").expect("a valid regex");
    for fence in guide.fences.iter().filter(|f| {
        let w = words(&f.info);
        w.first() == Some(&"markdown") && (w.contains(&"resume") || w.contains(&"in-section"))
    }) {
        for cap in name.captures_iter(&fence.body) {
            if cap[1].trim() != "Ada Lovelace" {
                problems.push(format!(
                    "line {}: an example resume is named `{}`: use Ada Lovelace",
                    fence.line,
                    cap[1].trim()
                ));
            }
        }
        for cap in role.captures_iter(&fence.body) {
            let company = cap[1].trim();
            if !companies.contains(company) {
                problems.push(format!(
                    "line {}: an example names the company `{company}`, which is not in tests/fixtures or the invented list: if it is invented, add it to `allowed_companies`",
                    fence.line
                ));
            }
        }
    }
    problems
}

/// The private details of real resumes, found in `dir`, that must never appear in the guide.
pub fn private_details(dir: &std::path::Path) -> BTreeSet<String> {
    let email = Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").expect("a valid regex");
    let url = Regex::new(r"https?://([A-Za-z0-9.-]+)").expect("a valid regex");
    let role = Regex::new(r"(?m)^### ([^\n·]+?) ·").expect("a valid regex");
    let name = Regex::new(r"(?m)^# (.+)$").expect("a valid regex");
    let mut details = BTreeSet::new();
    for entry in std::fs::read_dir(dir)
        .expect("the baselines directory")
        .filter_map(Result::ok)
    {
        if entry.path().extension().is_none_or(|e| e != "md") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
        details.extend(name.captures_iter(&text).map(|c| c[1].trim().to_string()));
        details.extend(role.captures_iter(&text).map(|c| c[1].trim().to_string()));
        details.extend(email.find_iter(&text).map(|m| m.as_str().to_string()));
        details.extend(url.captures_iter(&text).map(|c| c[1].to_lowercase()));
    }
    // A public host the guide itself links to (github.com, typst.app) is not private, however often a resume uses it.
    details.retain(|d| {
        !ALLOWED_HOSTS
            .iter()
            .any(|host| d == host || d.ends_with(&format!(".{host}")))
    });
    details.retain(|d| d.chars().count() >= 4);
    details
}

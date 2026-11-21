//! ABOUTME: The checks that bind the guide to the code. Each returns a list of problems; empty means the guide is true.
//! ABOUTME: They read hooks (headings, table cells, fence info strings, markers) and compare sets, never wording.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use resume::cli::Cli;
use resume::diagnostics::DiagnosticCode;
use resume::diagnostics::code::Category;
use strum::IntoEnumIterator;

use crate::extract::{Fence, Guide, Heading, plain};
use crate::facts::{self, CliOption};

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|e| panic!("`{pattern}` is not a valid pattern: {e}"))
}

/// The headings the guide must have, in order: `(level, text)`. Each exactly once.
///
/// `Annotations` and `Profiles and position specs` stay required after their features land: the stub is then
/// replaced by the real section, and the `guide:planned` marker is deleted.
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
    (3, "Environment variables"),
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

/// The first table under `heading`, or the problem to report.
fn first_table<'a>(guide: &'a Guide, heading: &Heading) -> Result<&'a crate::extract::Table, String> {
    guide
        .tables_in(Guide::range(heading))
        .next()
        .ok_or_else(|| format!("`{}` has no table", heading.text))
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

/// Whether a planned section's witness exists yet: `command:watch` (a command) or `option:--tags` (an option of
/// any command, or a global one). `None` when the witness is not of either form.
fn witness_exists(witness: &str) -> Option<bool> {
    let (kind, value) = witness.split_once(':')?;
    match kind {
        "command" => Some(facts::commands().contains_key(value)),
        "option" => Some(
            facts::every_long_option()
                .iter()
                .any(|name| format!("--{name}") == value),
        ),
        _ => None,
    }
}

/// A planned section is a stub: no examples, no option names, short, and it names a witness that does not exist yet.
///
/// When the witness appears, the feature has arrived: the section must be written and its marker deleted.
pub fn planned_sections(guide: &Guide) -> Vec<String> {
    static OPTION: LazyLock<Regex> = LazyLock::new(|| regex(r"`--[a-z]"));
    let mut problems = Vec::new();
    for marker in guide.markers.iter().filter(|m| m.kind == "planned") {
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
        if OPTION.is_match(body) {
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
            Some(None) => problems.push(format!(
                "line {}: the witness `{}` is not `command:NAME` or `option:--NAME`",
                marker.line,
                witness.unwrap_or_default()
            )),
            None => problems.push(format!("line {}: a planned marker needs `witness=KIND:VALUE`", marker.line)),
        }
    }
    problems
}

// ---- Markdown examples and shell examples ----------------------------------------------------------

/// The codes parsing and linting `text` report, as `resume check` does.
pub fn codes_of(text: &str) -> BTreeSet<String> {
    let parsed = resume::markdown::parse("example.md", text);
    let mut codes: BTreeSet<String> = parsed.diagnostics().iter().map(|d| d.code.to_string()).collect();
    codes.extend(
        resume::doctor::content::lint_parsed(&parsed, Path::new("example.md"))
            .iter()
            .map(|d| d.code.to_string()),
    );
    codes
}

/// The fences in `text` whose language is `languages`, counted line by line without the Markdown parser, so a
/// parser that loses fences is noticed by comparing with what the extractor found.
fn fence_lines_in(text: &str, languages: &[&str]) -> Vec<(usize, Vec<String>)> {
    let mut found = Vec::new();
    let mut open: Option<(usize, Vec<String>, bool)> = None;
    for (i, line) in text.replace("\r\n", "\n").lines().enumerate() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("```") || trimmed.starts_with("~~~")) {
            if let Some((_, body, true)) = open.as_mut() {
                body.push(line.to_string());
            }
            continue;
        }
        match open.take() {
            Some((at, body, wanted)) => {
                if wanted {
                    found.push((at, body));
                }
            }
            None => {
                let language = trimmed
                    .trim_start_matches(['`', '~'])
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .next()
                    .unwrap_or_default()
                    .to_lowercase();
                open = Some((i + 1, Vec::new(), languages.contains(&language.as_str())));
            }
        }
    }
    found
}

/// A fence that holds resume Markdown. `md` and any capitalization count, so a spelling cannot skip the check.
fn is_markdown(fence: &Fence) -> bool {
    matches!(fence.language().as_str(), "markdown" | "md")
}

pub fn markdown_examples(guide: &Guide) -> Vec<String> {
    let mut problems = Vec::new();
    let (mut checked, mut clean_resume, mut unchecked) = (0, 0, 0);
    for fence in guide.fences.iter().filter(|f| is_markdown(f)) {
        let info = fence.words();
        let at = format!("line {}", fence.line);
        let mode: Vec<&String> = info
            .iter()
            .filter(|w| matches!(w.as_str(), "resume" | "in-section" | "unchecked"))
            .collect();
        if mode.len() != 1 {
            problems.push(format!(
                "{at}: a `markdown` fence needs exactly one of `resume`, `in-section`, `unchecked` (found {mode:?})"
            ));
            continue;
        }
        if mode[0] == "unchecked" {
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
        let text = if mode[0] == "in-section" {
            format!("{SECTION_PREAMBLE}{}", fence.body)
        } else {
            fence.body.clone()
        };
        let found = codes_of(&text);
        checked += 1;
        if mode[0] == "resume" && expected.is_empty() {
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
    let direct = fence_lines_in(&guide.text, &["markdown", "md"]).len();
    if checked + unchecked != direct {
        problems.push(format!(
            "the extractor found {} Markdown examples but the text has {direct}: it may be broken, or an example has no mode",
            checked + unchecked
        ));
    }
    problems
}

pub fn shell_examples(guide: &Guide) -> Vec<String> {
    let mut problems = Vec::new();
    let mut lines = 0;
    for fence in guide.fences.iter().filter(|f| f.language() == "sh") {
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
    let direct: usize = fence_lines_in(&guide.text, &["sh"])
        .iter()
        .map(|(_, body)| {
            body.iter()
                .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                .count()
        })
        .sum();
    // Lines the checks refused above are counted there; every line must be either checked or reported.
    if lines + problems.len() < direct {
        problems.push(format!(
            "the extractor found {lines} shell command lines but the text has {direct}: it may be broken"
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
    let table = match first_table(guide, heading) {
        Ok(t) => t,
        Err(e) => return vec![e],
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

/// The key paths a Typst source reads from `theme`, as the guide writes them (`fonts.<role>.preferred`).
///
/// `theme.at("x", default: ...)` calls a method and reads no key, and comments are prose.
pub fn theme_read_paths(typst: &str) -> BTreeSet<String> {
    static READ: LazyLock<Regex> = LazyLock::new(|| regex(r"(?:^|[^/\w.])theme((?:\.[a-z][a-z-]*)+)"));
    let code: String = typst
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    READ.captures_iter(&code)
        .filter(|cap| !code[cap.get(0).map_or(0, |m| m.end())..].starts_with('('))
        .map(|cap| {
            let path = cap[1].trim_start_matches('.').to_string();
            if path.starts_with("fonts.") {
                match path.splitn(3, '.').nth(2) {
                    Some(field) => format!("fonts.<role>.{field}"),
                    None => "fonts.<role>".to_string(),
                }
            } else {
                path
            }
        })
        .collect()
}

/// True when a theme that defines `defined` has the key `path`, or keys below it.
fn theme_has(defined: &BTreeSet<String>, path: &str) -> bool {
    defined.iter().any(|d| d == path || d.starts_with(&format!("{path}.")))
}

/// Every key the Typst `sources` (name and text) read must be one some theme defines, or a prefix of one.
pub fn theme_reads_problems(sources: &[(String, String)], defined: &BTreeSet<String>) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, typst) in sources {
        let reads = theme_read_paths(typst);
        if reads.is_empty() {
            problems.push(format!("{name} reads no theme key: the pattern may be broken"));
        }
        // A key the source builds itself (`gutter-reach: reach`) is derived from the theme, not defined by it.
        let derived = |path: &str| !path.contains('.') && typst.contains(&format!("{path}:"));
        for path in reads.iter().filter(|p| !theme_has(defined, p) && !derived(p)) {
            problems.push(format!("{name} reads `theme.{path}`, which no theme defines"));
        }
    }
    problems
}

/// The Typst sources that read a theme: the shared components and each built-in style's `style.typ`.
pub fn typst_sources() -> Vec<(String, String)> {
    std::iter::once("styles/lib/components.typ".to_string())
        .chain(
            facts::built_in_styles()
                .into_iter()
                .map(|s| format!("styles/{s}/style.typ")),
        )
        .map(|name| {
            let text = facts::read(&name);
            (name, text)
        })
        .collect()
}

pub fn theme_reads_are_defined() -> Vec<String> {
    let defined: BTreeSet<String> = facts::theme_keys_by_style().into_values().flatten().collect();
    theme_reads_problems(&typst_sources(), &defined)
}

/// The built-in styles whose theme has every key the shared components read: the ones the guide's example style
/// can borrow a `theme.typ` from.
pub fn styles_whose_theme_fits_the_components() -> BTreeSet<String> {
    let reads = theme_read_paths(&facts::read("styles/lib/components.typ"));
    facts::theme_keys_by_style()
        .into_iter()
        .filter(|(_, keys)| reads.iter().all(|path| theme_has(keys, path)))
        .map(|(name, _)| name)
        .collect()
}

pub fn components(guide: &Guide) -> Vec<String> {
    let heading = match section(guide, 3, "Shared components") {
        Ok(h) => h,
        Err(e) => return vec![e],
    };
    let table = match first_table(guide, heading) {
        Ok(t) => t,
        Err(e) => return vec![e],
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

/// The guide's last section must link each built-in style's README, and the repository's README must link the
/// guide, so neither a fourth style nor the guide itself is left undiscoverable.
pub fn reading_list(guide: &Guide, root_readme: &str) -> Vec<String> {
    let mut problems = Vec::new();
    match section(guide, 2, "Where to read more") {
        Ok(heading) => {
            for style in facts::built_in_styles() {
                let wanted = format!("../styles/{style}/README.md");
                if !guide
                    .links
                    .iter()
                    .any(|l| l.url == wanted && heading.start < l.start && l.start < heading.end)
                {
                    problems.push(format!("`## Where to read more` must link `{wanted}`"));
                }
            }
        }
        Err(e) => problems.push(e),
    }
    if !root_readme.contains("docs/User-Guide.md") {
        problems.push("README.md must link docs/User-Guide.md".to_string());
    }
    problems
}

// ---- environment variables -------------------------------------------------------------------------

/// The environment variables the program reads, from the string in each `var_os("NAME")` or `var("NAME")` of
/// `src/`. Files named `*_tests.rs` hold test-only variables and are skipped.
pub fn env_vars_read_by_the_program() -> BTreeSet<String> {
    static READ: LazyLock<Regex> = LazyLock::new(|| regex(r#"\bvar(?:_os)?\(\s*"([A-Z][A-Z0-9_]*)"\s*\)"#));
    fn walk(dir: &Path, found: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs")
                && !path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with("_tests.rs"))
            {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                found.extend(READ.captures_iter(&text).map(|c| c[1].to_string()));
            }
        }
    }
    let mut found = BTreeSet::new();
    walk(&crate::root().join("src"), &mut found);
    found
}

/// Every environment variable the program reads is named in backticks under `### Environment variables`.
pub fn environment_variables(guide: &Guide, read: &BTreeSet<String>) -> Vec<String> {
    let heading = match section(guide, 3, "Environment variables") {
        Ok(h) => h,
        Err(e) => return vec![e],
    };
    let body = guide.body(heading);
    let mut problems: Vec<String> = read
        .iter()
        .filter(|name| !body.contains(&format!("`{name}")))
        .map(|name| format!("`### Environment variables` does not document `{name}`, which the program reads"))
        .collect();
    if read.is_empty() {
        problems.push("no environment variable found in src/: the scan may be broken".to_string());
    }
    problems
}

// ---- commands and options --------------------------------------------------------------------------

/// The default a row states, as `default `X``: every one of them, in the order written.
fn stated_defaults(row_text: &str) -> Vec<String> {
    static DEFAULT: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)\bdefault `([^`]+)`"));
    DEFAULT.captures_iter(row_text).map(|c| c[1].to_string()).collect()
}

/// The short options a first cell shows: `-o, --out <DIR>` shows `o`.
fn shown_shorts(cell: &str) -> BTreeSet<char> {
    cell.split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(|token| {
            let mut chars = token.strip_prefix('-')?.chars();
            match (chars.next(), chars.next()) {
                (Some(letter), None) if letter.is_ascii_alphabetic() => Some(letter),
                _ => None,
            }
        })
        .collect()
}

/// The long options a first cell shows: `-o, --out <DIR>` shows `--out`.
fn shown_longs(cell: &str) -> Vec<String> {
    cell.split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .filter(|w| w.starts_with("--") && w.len() > 2)
        .map(str::to_string)
        .collect()
}

fn check_option_rows(label: &str, options: &[CliOption], rows: &[Vec<String>]) -> Vec<String> {
    let mut problems = Vec::new();
    for option in options {
        let key = option.key();
        let Some(row) = rows.iter().find(|r| {
            let cell = plain(&r[0]);
            if option.positional {
                cell.contains(&key)
            } else {
                shown_longs(&cell).contains(&key)
            }
        }) else {
            problems.push(format!("{label}: `{key}` is not documented: add a row to its table"));
            continue;
        };
        let cell = plain(&row[0]);
        if let Some(short) = option.short
            && !shown_shorts(&cell).contains(&short)
        {
            problems.push(format!(
                "{label}: the row for `{key}` should show its short form `-{short}`"
            ));
        }
        let text = row.join(" ");
        for value in &option.values {
            if !text.contains(&format!("`{value}`")) {
                problems.push(format!(
                    "{label}: the row for `{key}` must list the value `{value}` in backticks"
                ));
            }
        }
        // The default is the one the row states as ``default `X` ``, not any value that is also listed.
        let stated = stated_defaults(&text);
        match (&option.default, stated.as_slice()) {
            (Some(default), [only]) if only == default => {}
            (Some(default), _) => problems.push(format!(
                "{label}: the row for `{key}` must state its default as ``default `{default}` `` (it states {stated:?})"
            )),
            (None, []) => {}
            (None, _) => problems.push(format!(
                "{label}: the row for `{key}` states a default {stated:?}, but the option has none"
            )),
        }
        if option.repeatable && !option.positional && !text.contains("repeat") {
            problems.push(format!(
                "{label}: `{key}` can be given more than once, and its row must say it is repeatable"
            ));
        }
    }
    // A row for an option the command does not have documents something that does not exist.
    for row in rows {
        let cell = plain(&row[0]);
        let longs = shown_longs(&cell);
        for word in &longs {
            if !options.iter().any(|o| &o.key() == word) {
                problems.push(format!(
                    "{label}: the guide documents `{word}`, which this command does not have"
                ));
            }
        }
        // And a short form must be the one the long option has.
        for long in &longs {
            if let Some(option) = options.iter().find(|o| &o.key() == long) {
                let shown = shown_shorts(&cell);
                let wanted: BTreeSet<char> = option.short.into_iter().collect();
                for extra in shown.difference(&wanted) {
                    problems.push(format!(
                        "{label}: the row for `{long}` shows `-{extra}`, which it does not have"
                    ));
                }
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

    // No `--option` or `-x` anywhere in the Commands section may be one that does not exist.
    static LONG: LazyLock<Regex> = LazyLock::new(|| regex(r"`(--[a-z][a-z-]*)"));
    static SHORT: LazyLock<Regex> = LazyLock::new(|| regex(r"`-([A-Za-z])`"));
    let every = facts::every_long_option();
    let shorts = facts::every_short_option();
    let body = guide.body(commands_heading);
    for cap in LONG.captures_iter(body) {
        let name = &cap[1];
        if !every.contains(name.trim_start_matches("--")) {
            problems.push(format!("the Commands section mentions `{name}`, which no command has"));
        }
    }
    for cap in SHORT.captures_iter(body) {
        let letter = cap[1].chars().next().unwrap_or_default();
        if !shorts.contains(&letter) {
            problems.push(format!(
                "the Commands section mentions `-{letter}`, which no command has"
            ));
        }
    }
    problems
}

// ---- diagnostics: the generated appendix and the links to codes ----------------------------------------

pub const START: &str = "<!-- guide:generated codes -->";
pub const END: &str = "<!-- /guide:generated -->";

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

/// How many links the text has, counted without the Markdown parser: `](` outside fenced blocks and code spans.
pub fn links_in_text(text: &str) -> usize {
    static DOUBLE_SPAN: LazyLock<Regex> = LazyLock::new(|| regex(r"``.+?``"));
    static SPAN: LazyLock<Regex> = LazyLock::new(|| regex(r"`[^`\n]*`"));
    let mut outside = String::new();
    let mut in_fence = false;
    for line in text.replace("\r\n", "\n").lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            outside.push_str(line);
            outside.push('\n');
        }
    }
    let outside = DOUBLE_SPAN.replace_all(&outside, "");
    SPAN.replace_all(&outside, "").matches("](").count()
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
        } else if link.url.is_empty() {
            problems.push(format!("{at}: a link with no target"));
        } else if !link.url.contains("://") && !link.url.starts_with("mailto:") {
            let path = link.url.split('#').next().unwrap_or_default();
            if path.is_empty() || !crate::root().join("docs").join(path).is_file() {
                problems.push(format!(
                    "{at}: the link `{}` points at a file that does not exist",
                    link.url
                ));
            }
        }
    }
    let direct = links_in_text(&guide.text);
    if guide.links.len() != direct {
        problems.push(format!(
            "the extractor found {} links but the text has {direct}: it may be broken",
            guide.links.len()
        ));
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
    let mut allowed: BTreeSet<String> = [
        "Acme",
        "Acme Corp",
        "Globex",
        "Initech",
        "Example Corp",
        "Analytical Engines Ltd",
        "Babbage & Co",
        "University of London",
    ]
    .map(str::to_string)
    .into();
    allowed.extend(fixture_text().lines().filter_map(role_company).map(str::to_string));
    allowed
}

/// All the text of the synthetic fixtures: nothing in it is private, so a detail found there is not a leak.
fn fixture_text() -> String {
    let mut all = String::new();
    let dir = crate::root().join("tests/fixtures");
    for entry in std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok) {
        if entry.path().extension().is_some_and(|e| e == "md") {
            all.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
            all.push('\n');
        }
    }
    all
}

/// The company of a role line `### Company · Title · Period` or `### Company | Title | Period`.
fn role_company(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("### ")?;
    let end = rest.find(['·', '|'])?;
    Some(rest[..end].trim())
}

/// Hosts a link in the guide may have: reserved example domains and the documentation the guide points to.
/// `github.com` is not here: the guide may link only the synthetic `github.com/ada`.
const ALLOWED_HOSTS: [&str; 5] = ["example.com", "example.org", "example.net", "typst.app", "docs.rs"];

/// The one GitHub address the guide may show: the synthetic user's.
const ALLOWED_GITHUB: &str = "github.com/ada";

/// Public names the guide may use, such as the product whose fonts a Mac may have installed. A resume that names
/// one as an employer has not leaked anything when the guide names it too.
const PUBLIC_NAMES: [&str; 1] = ["Microsoft"];

/// Sites where the host is public but the path is a person: only `host/path` is private there.
const PROFILE_HOSTS: [&str; 6] = [
    "github.com",
    "gitlab.com",
    "linkedin.com",
    "twitter.com",
    "x.com",
    "medium.com",
];

fn host_is_allowed(host: &str) -> bool {
    ALLOWED_HOSTS
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

/// True when `address` (`github.com/ada`, `github.com/ada/repo`) is the synthetic GitHub user's.
fn is_synthetic_github(address: &str) -> bool {
    address == ALLOWED_GITHUB || address.starts_with(&format!("{ALLOWED_GITHUB}/"))
}

/// Fails when the guide contains something that looks like a real person's details.
///
/// What it checks: email addresses (only `example.*` hosts), web addresses with or without `https://` (only the
/// example domains, the documentation hosts, and `github.com/ada`), phone-like numbers (only the reserved
/// `+44 20 7946 0xxx` and `555-01xx` ranges), and, inside the example resumes, the name (only Ada Lovelace) and
/// the company of every role line, written with `·` or `|` (only the fixtures' companies and a few invented ones).
///
/// What it cannot see: a real name, school, city, street address, or project written in prose, in a bullet, in an
/// era line, or in a contact line. `private_details` and the ignored baseline test cover the owner's own resumes
/// for those; for anyone else's details, review is the check.
pub fn no_personal_content(guide: &Guide) -> Vec<String> {
    static EMAIL: LazyLock<Regex> = LazyLock::new(|| regex(r"[A-Za-z0-9._%+-]+@([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+)"));
    static ADDRESS: LazyLock<Regex> = LazyLock::new(|| {
        regex(r"(?i)\b((?:[a-z0-9-]+\.)+(?:com|org|net|io|dev|me|app|co|ai|us|uk|de|ca|edu))\b(/[A-Za-z0-9_.~-]+)?")
    });
    static PHONE: LazyLock<Regex> = LazyLock::new(|| regex(r"\+?\(?\d[\d ()\-.]{7,}\d"));
    static RESERVED: LazyLock<Regex> = LazyLock::new(|| regex(r"^\+44 20 7946 0\d{3}$|555-01\d\d$"));
    static NAME: LazyLock<Regex> = LazyLock::new(|| regex(r"(?m)^# (.+)$"));

    let mut problems = Vec::new();
    for cap in EMAIL.captures_iter(&guide.text) {
        let host = cap[1].to_lowercase();
        if !["example.com", "example.org", "example.net"].iter().any(|h| host == *h) {
            problems.push(format!(
                "the guide has the email `{}`: use an address at example.com",
                &cap[0]
            ));
        }
    }
    let without_emails = EMAIL.replace_all(&guide.text, "");
    for cap in ADDRESS.captures_iter(&without_emails) {
        let host = cap[1].to_lowercase();
        let address = format!(
            "{host}{}",
            cap.get(2).map_or(String::new(), |m| m.as_str().to_lowercase())
        );
        let ok = host_is_allowed(&host) || (host == "github.com" && is_synthetic_github(&address));
        if !ok {
            problems.push(format!(
                "the guide has the address `{}`: only reserved example domains, documentation hosts, and github.com/ada are allowed",
                &cap[0]
            ));
        }
    }
    for m in PHONE.find_iter(&guide.text) {
        let digits = m.as_str().chars().filter(char::is_ascii_digit).count();
        if digits >= 9 && !RESERVED.is_match(m.as_str()) {
            problems.push(format!(
                "the guide has the phone-like number `{}`: use +44 20 7946 0xxx",
                m.as_str()
            ));
        }
    }
    let companies = allowed_companies();
    for fence in guide.fences.iter().filter(|f| {
        let w = f.words();
        is_markdown(f) && (w.iter().any(|x| x == "resume") || w.iter().any(|x| x == "in-section"))
    }) {
        for cap in NAME.captures_iter(&fence.body) {
            if cap[1].trim() != "Ada Lovelace" {
                problems.push(format!(
                    "line {}: an example resume is named `{}`: use Ada Lovelace",
                    fence.line,
                    cap[1].trim()
                ));
            }
        }
        for company in fence.body.lines().filter_map(role_company) {
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

/// The private details of the real resumes in `dir` (`.md` files, searched in subdirectories too) that must never
/// appear in the guide: names, companies, era names, emails, web addresses with the user's path, phone numbers
/// (as digits only), and the places in contact lines. Details that also occur in the synthetic fixtures, or are
/// public documentation hosts, are not private and are left out. Returns the details; callers print only counts.
pub fn private_details(dir: &Path) -> BTreeSet<String> {
    static EMAIL: LazyLock<Regex> = LazyLock::new(|| regex(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"));
    static ADDRESS: LazyLock<Regex> =
        LazyLock::new(|| regex(r"(?i)(?:https?://)?((?:[a-z0-9-]+\.)+[a-z]{2,})(/[A-Za-z0-9_.~-]+)?"));
    static PHONE: LazyLock<Regex> = LazyLock::new(|| regex(r"\+?\(?\d[\d ()\-.]{7,}\d"));
    static NAME: LazyLock<Regex> = LazyLock::new(|| regex(r"(?m)^# (.+)$"));
    static ERA: LazyLock<Regex> = LazyLock::new(|| regex(r"(?m)^\*\*([^*·\n]+?)\s*·"));

    fn collect(dir: &Path, texts: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                collect(&path, texts);
            } else if path.extension().is_some_and(|e| e == "md") {
                texts.extend(std::fs::read_to_string(&path).ok());
            }
        }
    }
    let mut texts = Vec::new();
    collect(dir, &mut texts);

    let mut details = BTreeSet::new();
    for text in &texts {
        details.extend(NAME.captures_iter(text).map(|c| c[1].trim().to_string()));
        details.extend(text.lines().filter_map(role_company).map(str::to_string));
        details.extend(ERA.captures_iter(text).map(|c| c[1].trim().to_string()));
        details.extend(EMAIL.find_iter(text).map(|m| m.as_str().to_string()));
        details.extend(PHONE.find_iter(text).filter_map(|m| {
            let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
            (digits.len() >= 9).then_some(digits)
        }));
        let without_emails = EMAIL.replace_all(text, "");
        for cap in ADDRESS.captures_iter(&without_emails) {
            let host = cap[1].to_lowercase();
            if host_is_allowed(&host) {
                continue;
            }
            let on_profile_site = PROFILE_HOSTS
                .iter()
                .any(|p| host == *p || host.ends_with(&format!(".{p}")));
            if !on_profile_site {
                details.insert(host.clone());
            }
            if let Some(path) = cap.get(2) {
                details.insert(format!("{host}{}", path.as_str().to_lowercase()));
            }
        }
        // The contact line is the paragraph right after the name: its items without a link, a number, or an address.
        if let Some(after_name) = text.split_once('\n').map(|(_, rest)| rest.trim_start()) {
            let contact = after_name.lines().next().unwrap_or_default();
            details.extend(
                contact
                    .split(" · ")
                    .map(str::trim)
                    .filter(|item| !item.contains(['@', '[', '/', '(']) && !item.chars().any(|c| c.is_ascii_digit()))
                    .map(str::to_string),
            );
        }
    }
    let synthetic = fixture_text().to_lowercase();
    details.retain(|d| {
        d.chars().count() >= 4
            && !synthetic.contains(&d.to_lowercase())
            && !PUBLIC_NAMES.iter().any(|name| name.eq_ignore_ascii_case(d))
    });
    details
}

// ---- the worked example of specs and profiles ----------------------------------------------------------

/// The worked example under "Profiles and position specs": the files the guide shows resolve, every profile it
/// shows is used, and what it says `--print-spec` prints is what the program prints.
///
/// A `toml` fence names its file in the info string (`toml positions/acme.toml`), or is `toml resolved` for the
/// printed spec.
pub fn spec_examples(guide: &Guide) -> Vec<String> {
    use std::fs;

    let heading = match section(guide, 2, "Profiles and position specs") {
        Ok(h) => h,
        Err(e) => return vec![e],
    };
    let range = Guide::range(heading);
    let mut files: Vec<(String, String)> = Vec::new();
    let mut resolved: Option<String> = None;
    let mut problems = Vec::new();
    for fence in guide
        .fences
        .iter()
        .filter(|f| range.contains(&f.start) && f.language() == "toml")
    {
        let words = fence.words();
        let body = fence.body.replace("\r\n", "\n");
        match words.get(1).map(String::as_str) {
            Some("resolved") => resolved = Some(body),
            Some(name) if name.ends_with(".toml") => files.push((name.to_string(), body)),
            _ => problems.push(format!(
                "line {}: a `toml` example here must name its file (`toml positions/acme.toml`) or be `toml resolved`",
                fence.line
            )),
        }
    }
    let specs: Vec<&String> = files
        .iter()
        .map(|(name, _)| name)
        .filter(|name| name.starts_with("positions/"))
        .collect();
    let [spec_name] = specs.as_slice() else {
        problems.push(format!(
            "the worked example needs exactly one spec under positions/, found {}",
            specs.len()
        ));
        return problems;
    };
    let Some(printed) = resolved else {
        problems.push("the worked example has no `toml resolved` example of what --print-spec prints".to_string());
        return problems;
    };
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => return vec![format!("cannot make a temporary directory: {e}")],
    };
    for (name, body) in &files {
        let path = dir.path().join(name);
        let written = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&path, body));
        if let Err(e) = written {
            return vec![format!("cannot write {name}: {e}")];
        }
    }
    match resume::spec::resolve(&dir.path().join(spec_name.as_str()), None) {
        Err(e) => problems.push(format!("the spec in the guide does not resolve: {e}")),
        Ok(spec) => {
            let text = spec.to_toml();
            if text.trim() != printed.trim() {
                problems.push(format!(
                    "the `toml resolved` example differs from what --print-spec prints:\n--- the guide\n{}\n--- the program\n{}",
                    printed.trim(),
                    text.trim()
                ));
            }
            let used: BTreeSet<String> = spec
                .chain
                .iter()
                .filter_map(|l| l.name.as_ref().map(ToString::to_string))
                .collect();
            for (name, _) in files.iter().filter(|(n, _)| n.starts_with("profiles/")) {
                let profile = name.trim_start_matches("profiles/").trim_end_matches(".toml");
                if !used.contains(profile) {
                    problems.push(format!("the guide shows `{name}`, which the spec does not extend"));
                }
            }
            if problems.is_empty() {
                problems.extend(spec_build_sample(guide, &range, dir.path(), spec_name));
            }
        }
    }
    problems
}

/// The sample of what a spec build prints must be what the program prints for the example's own files.
///
/// The baseline is the synthetic fixture the spec tests use; the sample says which wordings it has. Everything but the
/// timing on the first line is compared, so a changed line, a renamed label, or a reordered line is caught.
fn spec_build_sample(guide: &Guide, range: &std::ops::Range<usize>, dir: &std::path::Path, spec: &str) -> Vec<String> {
    use std::fs;
    use std::process::Command;

    let Some(sample) = guide
        .fences
        .iter()
        .find(|f| range.contains(&f.start) && f.language() == "text" && f.body.starts_with("ok out/"))
    else {
        return vec!["the worked example has no `text` sample of what a spec build prints".to_string()];
    };
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/specs/baseline.md");
    let baseline = dir.join("baselines/baseline.md");
    let copied = baseline
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::copy(&fixture, &baseline).map(|_| ()));
    if let Err(e) = copied {
        return vec![format!("cannot stage the baseline: {e}")];
    }
    let output = Command::new(env!("CARGO_BIN_EXE_resume"))
        .current_dir(dir)
        .args([
            "--color", "never", "build", "--spec", spec, "--fonts", "bundled", "-o", "out",
        ])
        .output();
    let output = match output {
        Ok(output) => output,
        Err(e) => return vec![format!("cannot run resume: {e}")],
    };
    // Paths print with the platform's separator; the guide shows `/`.
    let actual = String::from_utf8_lossy(&output.stderr)
        .replace("\r\n", "\n")
        .replace('\\', "/");
    let timeless = |text: &str| -> Vec<String> {
        text.trim_end()
            .lines()
            .map(|line| line.split("  built in ").next().unwrap_or(line).trim_end().to_string())
            .collect()
    };
    let (shown, real) = (timeless(&sample.body.replace("\r\n", "\n")), timeless(&actual));
    if shown == real {
        return Vec::new();
    }
    vec![format!(
        "the sample of what a spec build prints differs from the program (the first line is compared without its time):\n--- the guide\n{}\n--- the program\n{}",
        shown.join("\n"),
        real.join("\n")
    )]
}

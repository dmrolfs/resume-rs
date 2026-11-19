//! ABOUTME: Property tests that check what the parser means, not just that it survives: Edits preserve content,
//! ABOUTME: generated documents parse to the model their generator implies, and no word is lost without a finding.

use std::collections::HashSet;

use claims::assert_ok;
use proptest::prelude::*;
use resume::diagnostics::Severity;
use resume::markdown::{Parsed, parse};
use serde_json::{Value, json};

mod common;
use common::{apply_edits, edits_of};

// ---------------------------------------------------------------------------------------------
// Edits are meaning-preserving
// ---------------------------------------------------------------------------------------------

/// Markdown-shaped fragments, including the characters that make Edits risky (colons next to bold,
/// backslashes next to spaces, dashes in periods).
const TOKENS: &[&str] = &[
    "# ",
    "## ",
    "### ",
    " · ",
    "·",
    " - ",
    "-",
    "–",
    "—",
    "--",
    "|",
    "•",
    "**",
    "*",
    "_",
    "__",
    "`",
    "[",
    "](",
    ")",
    "https://x.y",
    "<!--",
    "-->",
    "<!-->",
    "![a](b)",
    "\n",
    "\r\n",
    "\n\n",
    "- ",
    "  ",
    " ",
    ":",
    "Label",
    "**Label**:",
    "**Label**: ",
    "Aug 2017",
    "2015",
    "2017",
    "Present",
    "Current",
    "now",
    "\\",
    "  \n",
    "word",
    "Acme",
    "Engineer",
    "&ndash;",
    "###Acme",
    "\\·",
    "Spring",
];

fn token_soup() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(TOKENS), 0..160).prop_map(|tokens| tokens.concat())
}

/// Whole lines a resume might contain, correct and defective, which token soup rarely assembles
/// (a bullet that starts with a label whose colon is outside the bold, say).
const LINES: &[&str] = &[
    "# Ada Lovelace",
    "London · a@b.co · [Notes](https://x.y)",
    "## Experience",
    "### Acme · Engineer · 2015 – 2017",
    "### Acme | Engineer | 2015 – 2017",
    "### Acme - Engineer - 2015 - 2017",
    "### Acme · Engineer · 2015 - Current",
    "### Acme · 2015-2017",
    "### Acme · Eng · Present – 2019",
    "###Acme · Eng · 2019 – 2021",
    "- plain bullet",
    "- **Label:** text here",
    "- **Label**: text here",
    "- **Label**:text here",
    "- **Label**:",
    "- Use **Rust**: fast",
    "- hard break  ",
    "  continued",
    "- backslash break\\",
    "\\",
    "  \\",
    "-\\",
    "#\\",
    "---\\",
    "= \\",
    "  - nested bullet",
    "1. numbered",
    "**Era · 2016 – 2017**",
    "**Era - 2016 – 2017**",
    "**Era | 2016 - 2017**",
    "**Award - 2019**",
    "**Python · Rust** are fine",
    "Blurb text with **bold** inside.",
    "A paragraph with <!-- unclosed comment",
    "<!-- a note -->",
    "<!-->",
    "<div class=\"page-break\"></div>",
    "![img](a.png)",
    "> a quote",
    "---",
    "[x](ftp://a)",
    "[x](notes.md)",
    "2019 &ndash; 2021",
];

/// Every line-soup document starts with a name, contact line, section, and role, so a model exists to compare.
const PREAMBLE: &str = "# Ada\n\nHere\n\n## S\n\n### Acme · Eng · 2015 – 2017\n\n";

fn line_soup() -> impl Strategy<Value = String> {
    let separator = prop_oneof![Just("\n"), Just("\n\n")];
    prop::collection::vec((prop::sample::select(LINES), separator), 1..14).prop_map(|lines| {
        lines
            .into_iter()
            .fold(PREAMBLE.to_string(), |doc, (line, sep)| format!("{doc}{line}{sep}"))
    })
}

fn structured_markdown() -> impl Strategy<Value = String> {
    prop_oneof![token_soup(), line_soup()]
}

/// Alphanumeric words, lowercased, with the spellings of Present that an Edit rewrites folded together.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| match word.to_lowercase().as_str() {
            "current" | "now" => "present".to_string(),
            other => other.to_string(),
        })
        .collect()
}

/// Counts bold spans (bold inlines and bullet labels) and literal emphasis marks left in text nodes.
fn emphasis_stats(parsed: &Parsed) -> (usize, usize) {
    fn walk(value: &Value, bold: &mut usize, marks: &mut usize) {
        match value {
            Value::Object(map) => {
                *bold += usize::from(map.get("kind") == Some(&json!("strong")));
                *bold += usize::from(map.get("label").is_some_and(|l| !l.is_null()));
                if map.get("kind") == Some(&json!("text"))
                    && let Some(Value::String(text)) = map.get("text")
                {
                    *marks += text.chars().filter(|c| matches!(c, '*' | '_')).count();
                }
                map.values().for_each(|v| walk(v, bold, marks));
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, bold, marks)),
            _ => {}
        }
    }
    let value = assert_ok!(serde_json::to_value(parsed.best_effort()));
    let (mut bold, mut marks) = (0, 0);
    walk(&value, &mut bold, &mut marks);
    (bold, marks)
}

/// 512 cases by default; `PROPTEST_CASES=50000 cargo test --test parse_property_test` runs deeper locally
/// (`ProptestConfig::with_cases` would otherwise ignore the variable).
fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(512)
}

/// How many findings are errors.
fn error_count(parsed: &Parsed) -> usize {
    parsed
        .diagnostics()
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    /// An Edit rewrites separators, dashes, spellings of Present, colons, and breaks. It must never add or
    /// remove a word, never leave a bold span unclosed (which is how a colon moved next to text corrupts a
    /// bullet), and never turn markup into literal text.
    #[test]
    fn applying_edits_preserves_words_and_emphasis(text in structured_markdown()) {
        let before = parse("r.md", &text);
        let fixed = apply_edits(&text, &edits_of(before.diagnostics()));
        prop_assert_eq!(words(&text), words(&fixed), "an Edit changed the words");
        let after = parse("r.md", &fixed);
        // An Edit that fixes an error can unmask others behind it (an error rejects its whole paragraph, so
        // what follows is only checked once it is fixed). An Edit that fixes a warning is a style change,
        // and must never create an error.
        let fixes_only_warnings = before
            .diagnostics()
            .iter()
            .filter(|d| d.fix.is_some())
            .all(|d| d.severity != Severity::Error);
        if fixes_only_warnings {
            prop_assert!(
                error_count(&after) <= error_count(&before),
                "a warning's Edit added an error: {} -> {}\nbefore: {text:?}\nafter: {fixed:?}",
                error_count(&before),
                error_count(&after)
            );
        }
        let (bold_before, marks_before) = emphasis_stats(&before);
        let (bold_after, marks_after) = emphasis_stats(&after);
        if before.best_effort().is_some() && after.best_effort().is_some() {
            prop_assert!(bold_after >= bold_before, "bold spans {bold_before} -> {bold_after}\nbefore: {text:?}\nafter: {fixed:?}");
            prop_assert!(marks_after <= marks_before, "literal marks {marks_before} -> {marks_after}\nbefore: {text:?}\nafter: {fixed:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Generated documents: the model is what the generator says it should be
// ---------------------------------------------------------------------------------------------

/// Periods, newest first, so roles sorted by index never trip the chronology warning.
const PERIODS: [&str; 4] = [
    "Aug 2017 – Present",
    "2015 – 2017",
    "Spring 2012 – Fall 2014",
    "2009 – 2011",
];

#[derive(Debug, Clone)]
enum Item {
    Plain,
    Labeled,
}

#[derive(Debug, Clone)]
enum Body {
    Bullets(Vec<Item>),
    Paragraph,
    Era(Vec<Item>),
}

#[derive(Debug, Clone)]
struct RoleSpec {
    has_title: bool,
    period: usize,
    body: Vec<Body>,
}

/// Pieces that parse with a warning, and so are only used when testing that warnings account for loss.
#[derive(Debug, Clone, Copy)]
enum Warn {
    Image,
    HardBreak,
    ColonOutside,
    Nested,
    MidBold,
}

#[derive(Debug, Clone)]
enum Intro {
    Paragraph,
    Bullets(Vec<Item>),
    Comment,
    Warning(Warn),
}

#[derive(Debug, Clone)]
struct SectionSpec {
    intro: Vec<Intro>,
    roles: Vec<RoleSpec>,
}

#[derive(Debug, Clone)]
struct DocSpec {
    contact_items: usize,
    sections: Vec<SectionSpec>,
}

fn items() -> impl Strategy<Value = Vec<Item>> {
    prop::collection::vec(prop_oneof![Just(Item::Plain), Just(Item::Labeled)], 1..4)
}

fn role() -> impl Strategy<Value = RoleSpec> {
    let body = prop_oneof![
        items().prop_map(Body::Bullets),
        Just(Body::Paragraph),
        items().prop_map(Body::Era),
    ];
    (any::<bool>(), 0..PERIODS.len(), prop::collection::vec(body, 0..5)).prop_map(|(has_title, period, body)| {
        RoleSpec {
            has_title,
            period,
            body,
        }
    })
}

fn section(with_warnings: bool) -> impl Strategy<Value = SectionSpec> {
    let warning = prop_oneof![
        Just(Warn::Image),
        Just(Warn::HardBreak),
        Just(Warn::ColonOutside),
        Just(Warn::Nested),
        Just(Warn::MidBold),
    ];
    let intro = if with_warnings {
        prop_oneof![
            Just(Intro::Paragraph),
            items().prop_map(Intro::Bullets),
            Just(Intro::Comment),
            warning.prop_map(Intro::Warning),
        ]
        .boxed()
    } else {
        prop_oneof![
            Just(Intro::Paragraph),
            items().prop_map(Intro::Bullets),
            Just(Intro::Comment)
        ]
        .boxed()
    };
    (prop::collection::vec(intro, 0..4), prop::collection::vec(role(), 0..4)).prop_map(|(intro, mut roles)| {
        roles.sort_by_key(|r| r.period);
        SectionSpec { intro, roles }
    })
}

fn doc(with_warnings: bool) -> impl Strategy<Value = DocSpec> {
    (1..=4usize, prop::collection::vec(section(with_warnings), 0..4)).prop_map(|(contact_items, sections)| DocSpec {
        contact_items,
        sections,
    })
}

/// Renders a spec to Markdown, numbering every word uniquely, and builds the outline it should parse to.
struct Writer {
    words: usize,
    marker_flip: bool,
    text: String,
}

impl Writer {
    fn word(&mut self) -> String {
        self.words += 1;
        format!("wd{}q", self.words)
    }

    /// Alternating list markers keep two consecutive lists from merging into one.
    fn marker(&mut self) -> &'static str {
        self.marker_flip = !self.marker_flip;
        if self.marker_flip { "-" } else { "*" }
    }

    fn push(&mut self, block: &str) {
        self.text.push_str(block);
        self.text.push_str("\n\n");
    }

    fn list(&mut self, items: &[Item]) -> Vec<Value> {
        let marker = self.marker();
        let mut block = String::new();
        let mut outline = Vec::new();
        for item in items {
            match item {
                Item::Plain => {
                    let body = self.word();
                    block.push_str(&format!("{marker} {body}\n"));
                    outline.push(json!({"label": null, "text": body}));
                }
                Item::Labeled => {
                    let (label, body) = (self.word(), self.word());
                    block.push_str(&format!("{marker} **{label}:** {body}\n"));
                    outline.push(json!({"label": label, "text": body}));
                }
            }
        }
        self.push(block.trim_end());
        outline
    }

    fn paragraph(&mut self) -> String {
        let text = format!("{} {} {}", self.word(), self.word(), self.word());
        self.push(&text);
        text
    }

    fn warning(&mut self, warn: Warn) {
        let block = match warn {
            Warn::Image => format!("{} ![{}](p.png) {}", self.word(), self.word(), self.word()),
            Warn::HardBreak => format!("{}  \n{}", self.word(), self.word()),
            Warn::ColonOutside => format!("- **{}**: {}", self.word(), self.word()),
            Warn::Nested => format!("- {}\n  - {}", self.word(), self.word()),
            Warn::MidBold => format!("{} **{}** {}", self.word(), self.word(), self.word()),
        };
        self.push(&block);
    }

    fn role(&mut self, spec: &RoleSpec) -> Value {
        let company = self.word();
        let title = spec.has_title.then(|| self.word());
        let period = PERIODS[spec.period];
        let heading = match &title {
            Some(title) => format!("### {company} · {title} · {period}"),
            None => format!("### {company} · {period}"),
        };
        self.push(&heading);
        let mut blurb: Option<String> = None;
        let mut body: Vec<Value> = Vec::new();
        let mut era: OpenEra = None;
        let mut started = false;
        for part in &spec.body {
            match part {
                Body::Paragraph if !started => {
                    started = true;
                    blurb = Some(self.paragraph());
                }
                Body::Paragraph => {
                    close_era(&mut era, &mut body);
                    body.push(json!({"paragraph": self.paragraph()}));
                }
                Body::Bullets(list) => {
                    started = true;
                    let outline = self.list(list);
                    match era.as_mut() {
                        Some((_, items)) => items.extend(outline),
                        None => body.push(json!({"bullets": outline})),
                    }
                }
                Body::Era(list) => {
                    started = true;
                    close_era(&mut era, &mut body);
                    let name = self.word();
                    self.push(&format!("**{name} · 2016 – 2017**"));
                    era = Some((name, self.list(list)));
                }
            }
        }
        close_era(&mut era, &mut body);
        json!({"role": {"company": company, "title": title, "period": period, "blurb": blurb, "body": body}})
    }
}

/// An era being collected: its name and the items of the lists that follow it.
type OpenEra = Option<(String, Vec<Value>)>;

fn close_era(era: &mut OpenEra, body: &mut Vec<Value>) {
    if let Some((name, items)) = era.take() {
        body.push(json!({"era": {"name": name, "period": "2016 – 2017", "items": items}}));
    }
}

fn render(spec: &DocSpec) -> (String, Value) {
    let mut w = Writer {
        words: 0,
        marker_flip: false,
        text: String::new(),
    };
    let name = format!("{} {}", w.word(), w.word());
    w.push(&format!("# {name}"));
    let contact: Vec<String> = (0..spec.contact_items).map(|_| w.word()).collect();
    w.push(&contact.join(" · "));
    let mut sections = Vec::new();
    for section in &spec.sections {
        let title = w.word();
        w.push(&format!("## {title}"));
        let mut blocks = Vec::new();
        for intro in &section.intro {
            match intro {
                Intro::Paragraph => blocks.push(json!({"paragraph": w.paragraph()})),
                Intro::Bullets(list) => blocks.push(json!({"bullets": w.list(list)})),
                Intro::Comment => w.push("<!-- a note -->"),
                Intro::Warning(warn) => w.warning(*warn),
            }
        }
        blocks.extend(section.roles.iter().map(|r| w.role(r)));
        sections.push(json!({"title": title, "blocks": blocks}));
    }
    let outline = json!({"name": name, "contact": contact.join(" · "), "sections": sections});
    (w.text, outline)
}

/// Reduces the real model to the same outline shape: names and texts only.
fn outline(model: &Value) -> Value {
    let text = |inlines: &Value| -> String { inline_text(inlines) };
    let items = |items: &Value| -> Value {
        Value::Array(
            items
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|i| json!({"label": if i["label"].is_null() { Value::Null } else { json!(text(&i["label"])) }, "text": text(&i["content"])}))
                        .collect()
                })
                .unwrap_or_default(),
        )
    };
    let blocks = |blocks: &Value| -> Value {
        Value::Array(
            blocks
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|b| match b["kind"].as_str() {
                            Some("paragraph") => json!({"paragraph": text(&b["content"])}),
                            Some("bullets") => json!({"bullets": items(&b["items"])}),
                            Some("era") => json!({"era": {"name": b["name"], "period": b["period"]["text"], "items": items(&b["items"])}}),
                            _ => b.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        )
    };
    let sections: Vec<Value> = model["sections"]
        .as_array()
        .map(|sections| {
            sections
                .iter()
                .map(|section| {
                    let mapped: Vec<Value> = section["blocks"]
                        .as_array()
                        .map(|list| {
                            list.iter()
                                .map(|b| match b["kind"].as_str() {
                                    Some("role") => json!({"role": {
                                        "company": b["company"],
                                        "title": b["title"],
                                        "period": b["period"]["text"],
                                        "blurb": if b["blurb"].is_null() { Value::Null } else { json!(text(&b["blurb"])) },
                                        "body": blocks(&b["body"]),
                                    }}),
                                    _ => blocks(&json!([b]))[0].clone(),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    json!({"title": section["title"], "blocks": mapped})
                })
                .collect()
        })
        .unwrap_or_default();
    json!({"name": model["name"], "contact": text(&model["contact"]), "sections": sections})
}

fn inline_text(inlines: &Value) -> String {
    inlines
        .as_array()
        .map(|list| {
            list.iter()
                .map(|inline| match inline["text"].as_str() {
                    Some(text) => text.to_string(),
                    None => inline_text(&inline["content"]),
                })
                .collect()
        })
        .unwrap_or_default()
}

proptest! {
    /// Documents built from the conventions, in any combination and order, parse with no findings to
    /// exactly the model their generator implies: roles, blurbs, eras that collect following lists,
    /// labels, and paragraphs after bullets.
    #[test]
    fn generated_documents_parse_to_the_model_their_generator_implies(spec in doc(false)) {
        let (text, expected) = render(&spec);
        let parsed = parse("r.md", &text);
        prop_assert_eq!(parsed.diagnostics(), &[], "unexpected findings for:\n{}", text);
        let model = assert_ok!(serde_json::to_value(parsed.valid()));
        prop_assert_eq!(outline(&model), expected, "for:\n{}", text);
    }

    /// Whatever the parser drops it must say so: with warnings in play (and no errors), every word of
    /// the input is in the model or inside the span of a finding.
    #[test]
    fn no_word_is_lost_without_a_finding(spec in doc(true)) {
        let (text, _) = render(&spec);
        let parsed = parse("r.md", &text);
        prop_assert!(!parsed.has_errors(), "findings: {:?}\n{}", parsed.diagnostics(), text);
        let model = assert_ok!(serde_json::to_value(parsed.best_effort()));
        let present = model_words(&model);
        for word in words_numbered(&text) {
            if present.contains(&word) {
                continue;
            }
            let at = text.find(&word).unwrap_or(0);
            let covered = parsed.diagnostics().iter().any(|d| {
                d.span.as_ref().is_some_and(|s| s.range.start < at + word.len() && at < s.range.end)
            });
            prop_assert!(covered, "`{}` was dropped with no finding covering it\n{}", word, text);
        }
    }
}

/// Every string in the model (except structural tags), split into words.
fn model_words(model: &Value) -> HashSet<String> {
    fn collect(value: &Value, out: &mut HashSet<String>) {
        match value {
            Value::String(s) => out.extend(
                s.split(|c: char| !c.is_alphanumeric())
                    .filter(|w| !w.is_empty())
                    .map(str::to_string),
            ),
            Value::Array(items) => items.iter().for_each(|v| collect(v, out)),
            Value::Object(map) => map
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "kind" | "precision"))
                .for_each(|(_, v)| collect(v, out)),
            _ => {}
        }
    }
    let mut out = HashSet::new();
    collect(model, &mut out);
    out
}

/// The generator's unique words (`wd<N>q`) found in `text`.
fn words_numbered(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| {
            w.starts_with("wd")
                && w.ends_with('q')
                && w[2..w.len() - 1].bytes().all(|b| b.is_ascii_digit())
                && w.len() > 3
        })
        .map(str::to_string)
        .collect()
}

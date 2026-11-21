<!-- ABOUTME: Architecture for resume-rs, a Rust CLI that typesets Markdown resumes to PDF with embedded Typst. -->
<!-- ABOUTME: Covers the data model, Markdown parsing, Typst integration, styles, CLI, feedback, and decisions. -->

# resume-rs: Architecture

**Epic ID:** rsm-141
**Status:** Draft
**Date:** 2026-11-17

---

## Problem Statement

Resumes are written as Markdown in `~/Documents/vrm/my/job/drafts/` and built by `~/Documents/vrm/my/job/tools/resume-pdf`, a Python script that runs pandoc with a Lua filter and prints HTML to PDF with headless Chrome. That tool works, but it falls short on the three priorities for this project:

1. **Document quality.** Chrome lays out text like a web page. Keeping a role heading with its first bullet depends on `break-after: avoid`, which Chrome treats as a hint. The tool has one visual style, and there is no way to switch to the other layouts the book recommends.
2. **Reliability.** The build depends on two external programs (pandoc and Chrome) found at runtime. Chrome updates silently and can change layout. Fonts come from the machine, so output varies between machines.
3. **Feedback.** Markdown that doesn't match a convention silently renders as something else: a malformed role line becomes a plain heading, and a stray paragraph becomes a company description. There is no page count, no location for errors, and no layout report.

## Current State

The Python tool (`tools/resume-pdf/`) does the following:

- `resume.lua` recognizes these patterns:
  - the first paragraph after `# Name` (the contact line)
  - `### Company · Title · Dates` and `### Company · Dates` (role lines)
  - the first paragraph after a role line (the company description)
  - a paragraph that is only `**Era · Dates**` (an era line)
  - HTML comments, which are dropped
  - `<div class="page-break">`
- `resume.css` styles one design: single column, accent color `#1f3f8a`, Helvetica Neue at 9.8 pt, US Letter, and a footer with the name and "Page N of M".
- `build_resume.py` runs pandoc and then Chrome `--print-to-pdf` with an isolated profile. It exits 1 on any failure.

Current inputs: two baseline resumes (a principal-architect and an org-leader version, about 2,000 words and 4 pages each) kept in the job workspace, outside this repository. Both must build unchanged. Resume content never enters this repo; tests use synthetic fixtures, and the baselines are exercised only by `#[ignore]` tests that read `RESUME_BASELINES`.

### Reference templates

| Source | Layout | Book rating |
|---|---|---|
| Pragmatic Engineer's (docx) / Standard Resume **Parker** | Top-down. The header has location and LinkedIn on the left, the name in the center, and phone and email on the right. Section headings have a bottom rule. The role line is title, then company, with the date on a right tab. Calibri 11 pt, 0.5" margins, US Letter. | ★★★★★, the safe default |
| Mono Engineer's (docx) | The Pragmatic template's body with a different header and no color. The name is centered, with one centered contact line under it. Section titles are black bold capitals (12 pt) over a black rule. The role line has the title, the company (in a centered column), and the dates, all bold, with the team and location on a second line. Calibri 11 pt. | ★★★★★ |
| Experienced Engineer's (docx) / Standard Resume **Venables** | Two zones: section labels in a left gutter (red `#CC0000` in the docx, black serif in Venables), and roles in the right column with dates right-aligned. Built from 5 tables in the docx. | ★★★★★ for principal, staff, and distinguished candidates with standout experience |

The other Standard Resume templates (Sidney, Georgia, Pender, Seymore, Keefer, Eton, Cordova, Yew, Arbutus) are excluded. They use two columns, photos, or monospace type, which the book advises against.

## Proposed Design

### Design Principles

1. **The Markdown file is the single source of truth.** The tool never asks you to duplicate content into another format.
2. **Parse once into a typed model, and render from the model.** Styles never see Markdown, and the parser never knows about layout.
3. **Every input problem gets a location and a hint.** Silent reinterpretation is a bug.
4. **The same input and the same resolved fonts produce byte-identical PDFs.** No clock or network. `--fonts bundled` is identical on every machine and OS.
6. **Runs the same on macOS and Windows.** Built and tested on both, with no platform assumptions in paths, line endings, or file replacement.
5. **Structure and theme are separate inside each style.** Layout lives in `style.typ`, and fonts, sizes, colors, and spacing live in `theme.typ`, which plays the role CSS plays today.

### Data Flow

```
resume.md ──► markdown::parse ──► Resume model ──serde_json──► /resume.json (virtual file)
                 │                                              │
                 └─► Vec<Diagnostic> (Markdown spans)           ▼
                                           ResumeWorld (styles/, fonts/) ──► typst::compile
                                                                                │
                                         Vec<Diagnostic> (Typst spans) ◄────────┤
                                                                                ▼
                                                  PagedDocument ──► layout::report ──► LayoutReport
                                                        │
                                                        └──► typst_pdf::pdf ──► resume.pdf
```

The model reaches Typst as JSON in a virtual file, read with `json("/resume.json")`, rather than as typed `sys.inputs` values. That has two benefits. Styles can be developed outside the CLI: `resume export` writes `resume.json` with a copy of the style and `lib/`, so `typst watch --root DIR DIR/<name>/style.typ` or the tinymist editor extension gives a live preview. And the style contract is a documented JSON schema rather than Rust types.

### Crate Layout

One package, `resume`, with a library (for tests) and a binary.

```
resume-rs/
  Cargo.toml
  src/
    main.rs              # clap entry point; maps outcomes to exit codes
    lib.rs               # public API: parse, render, doctor
    cli.rs               # clap Args and Commands; --dry-run plumbing
    error.rs             # snafu Error for I/O, style lookup, Typst/PDF failures
    model/
      mod.rs             # Resume, Section, Block, Role, RoleBlock, Bullet, Inline
      text.rs            # nutype newtypes: Name, Company, Title, SectionTitle, EraName, LinkUrl, StyleName
      period.rs          # Year, PartialDate, Season, Period, PeriodEnd (time::Month)
      options.rs         # Paper, OutputFormat, BuiltinStyle, PageLimit (strum + clap ValueEnum)
    diagnostics/
      mod.rs             # Diagnostic, Severity, Span, SourceKind
      code.rs            # DiagnosticCode (strum), Category, Advice
      advice.rs          # the advice catalog: DiagnosticCode::advice()
      lines.rs           # LineIndex: one definition of a line, shared by the parser and the renderer
      render/
        mod.rs           # emit_human, emit_json, RenderError
        sources.rs       # Sources: text by path, display paths, span snapping
        excerpt.rs       # the bounded, sanitized excerpt codespan draws
        sanitize.rs      # escapes control and bidi characters; caps long text
        human.rs         # one finding -> a codespan diagnostic (header, caret, notes)
        json.rs          # the JSON-lines record
        color.rs         # ColorEnv, resolve_color
        summary.rs       # Summary: the one-line counts
    markdown/
      mod.rs             # parse(path, text) -> Parsed (valid() / best_effort()); Sink, limits, BOM and CR handling
      tree.rs            # phase 1: events -> located tree (iterative, depth-capped; text-to-source segment map)
      blocks.rs          # phase 2: name, contact, sections, role bodies, eras, bullets, HTML classification
      role.rs            # role lines and era lines: canonical split, look-alike recovery, nutype-error mapping
      inline.rs          # inline runs (text, strong, emph, link, code), labels, links, hard breaks
      period.rs          # period grammar (pure) and located findings with Edits
      lint.rs            # format lints (nesting, chronology, mid-sentence bold, images, contact count)
    doctor/
      mod.rs             # doctor(&Parsed, &Rendered, &Env) -> DoctorReport
      content.rs         # book-guided content lints (quantification, weak openers, first person, ...)
      setup.rs           # environment checks (style resolution, fonts, output dir)
      fix.rs             # mechanical fixes -> Vec<Edit>; unified diff via `similar`
    typeset/
      mod.rs             # Typesetter::compile(&Resume, &StyleSource, &RenderOptions) -> Compiled; Compiled::export_pdf
      world.rs           # ResumeWorld: impl typst::World
      fonts.rs           # FontSet: bundled set now; layers, ordering, and role resolution in rsm-141.18
      pdf.rs             # deterministic PdfOptions
      diag.rs            # Typst diagnostics -> Diagnostic with style-file spans
    style.rs             # StyleSource (built-in or directory), registry
    layout.rs            # LayoutReport from PagedDocument (pages, role placement)
    watch.rs             # which events count and when a burst is over (the loop is in commands/watch.rs)
  styles/                # built-in styles, embedded with include_dir
    lib/components.typ   # shared rendering of inlines, bullets, role lines, eras
    pragmatic/{style.typ, theme.typ, README.md}
    mono/{style.typ, theme.typ, README.md}
    experienced/{style.typ, theme.typ, README.md}
  fonts/                 # bundled OFL/GUST-licensed fonts + LICENSES.md
  tests/
    fixtures/            # synthetic resumes covering every convention and error
    parse_test.rs  diagnostics_test.rs  render_test.rs  doctor_test.rs  cli_test.rs
```

### Model (`src/model/`)

#### Validated text newtypes (`model/text.rs`, nutype 0.8)

Every string with domain meaning is a `nutype` newtype. That makes it impossible to build an empty company or an untrimmed title, and keeps function signatures self-describing (`fn role_line(company: &Company, title: Option<&Title>)`). Free-flowing prose stays `String` inside `Inline::Text`, because a newtype there adds no invariant.

```rust
use nutype::nutype;

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 80),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom),
)]
pub struct Name(String);              // the person's name, from the single `# ` heading

#[nutype(sanitize(trim), validate(not_empty, len_char_max = 120),
         derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom))]
pub struct Company(String);           // role line, first part

#[nutype(sanitize(trim), validate(not_empty, len_char_max = 120),
         derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom))]
pub struct Title(String);             // role line, middle part

#[nutype(sanitize(trim), validate(not_empty, len_char_max = 60),
         derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom))]
pub struct SectionTitle(String);      // `## ` heading

#[nutype(sanitize(trim), validate(not_empty, len_char_max = 80),
         derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom))]
pub struct EraName(String);           // `**Era · Period**` name

#[nutype(validate(predicate = |u: &url::Url| matches!(u.scheme(), "http" | "https" | "mailto")),
         derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom))]
pub struct LinkUrl(url::Url);         // parsed with `url` 2.5 (WHATWG); http(s):// or mailto: only

#[nutype(sanitize(trim, lowercase), validate(regex = "^[a-z][a-z0-9-]{0,31}$"),
         derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom, FromStr))]
pub struct StyleName(String);         // built-in or --styles-dir style directory name
```

The parser converts each nutype error into a located `Diagnostic`, so `CompanyError::NotEmptyViolated` becomes `DiagnosticCode::EmptyRolePart` with the span of the role line. Raw nutype errors never reach the user.

Links are parsed with `url::Url::parse` before the newtype check. A `url::ParseError` becomes an `invalid-link` error, with the parser's message as a hint. A valid URL with another scheme, such as `ftp:` or `file:`, becomes an `unsupported-link` error. A relative path fails to parse, so it gets `invalid-link`. `url` serializes to a string with its `serde` feature, so the JSON contract is unchanged.

#### Periods and dates (`model/period.rs`, `time` 0.3)

Resume dates have month, season, or year precision, never a day, so they're modeled as partial dates. `time::Month` gives typed months with `Display` and conversion to and from `u8`. The rest is a small hand-written parser. `time`'s format-description parser expects a full calendar date and can't express seasons or "Present". **`today()` is never consulted.** "Present" is a variant, not a resolved date, which keeps output deterministic.

```rust
#[nutype(validate(greater_or_equal = 1950, less_or_equal = 2100),
         derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Serialize, TryFrom, Into))]
pub struct Year(u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize,
         strum::Display, strum::EnumString, strum::EnumIter, strum::AsRefStr)]
#[strum(ascii_case_insensitive)]
#[serde(rename_all = "kebab-case")]
pub enum Season {
    Spring,
    Summer,
    #[strum(serialize = "Fall", serialize = "Autumn")]
    Fall,
    Winter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "precision", rename_all = "kebab-case")]
pub enum PartialDate {
    Year { year: Year },
    Month { year: Year, #[serde(serialize_with = "month_number")] month: time::Month },
    Season { year: Year, season: Season },
}

impl PartialDate {
    /// Chronological sort key; a season maps to its first month (Spring=3, Summer=6, Fall=9, Winter=12).
    pub fn sort_key(&self) -> (Year, u8);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", content = "date", rename_all = "kebab-case")]
pub enum PeriodEnd { Present, Date(PartialDate) }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Period {
    pub text: String,              // as written after normalization, e.g. "Aug 2017 – Present"; styles display this
    pub start: PartialDate,
    pub end: PeriodEnd,            // invariant: end >= start (checked by the parser)
}
```

`humantime` 2.4 isn't used for resume dates, because it parses timestamps and durations, not "Aug 2017". It is used where durations reach the user: `watch --debounce 150ms` is parsed with `humantime::Duration`, and build summaries print elapsed time with `humantime::format_duration`, for example `built in 84ms`.

#### Document structure (`model/mod.rs`)

```rust
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resume {
    pub schema: u32,                 // always SCHEMA_VERSION
    pub name: Name,
    pub contact: Vec<Inline>,        // first paragraph after the name; may be empty
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Section { pub title: SectionTitle, pub blocks: Vec<Block> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Block {
    Paragraph { content: Vec<Inline> },
    Bullets { items: Vec<Bullet> },
    Role(Role),
    PageBreak,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Role {
    pub company: Company,
    pub title: Option<Title>,        // None for `### Company · Period`
    pub period: Period,
    pub blurb: Option<Vec<Inline>>,  // first paragraph after the role line
    pub body: Vec<RoleBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RoleBlock {
    Era { name: EraName, period: Period, items: Vec<Bullet> },
    Bullets { items: Vec<Bullet> },
    Paragraph { content: Vec<Inline> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bullet {
    pub label: Option<Vec<Inline>>,  // leading `**Label:**`, colon stripped
    pub content: Vec<Inline>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Inline {
    Text { text: String },
    Strong { content: Vec<Inline> },
    Emph { content: Vec<Inline> },
    Link { url: LinkUrl, content: Vec<Inline> },
    Code { text: String },
}
```

`Bullet.label` gives styles a typed hook for skill groups (`**Languages:** …`) and grouped roles (`**Initech, VP of Engineering:** …`). The `experienced` style renders skills as a label and value table, which isn't possible without it. The model derives `Serialize` only. It is produced by the parser, never deserialized, so no nutype `Deserialize` impls are needed.

#### Option enums (`model/options.rs`, strum 0.28 + clap)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum,
         strum::Display, strum::EnumString, strum::AsRefStr, strum::EnumIter)]
#[strum(serialize_all = "kebab-case")]
pub enum BuiltinStyle { Pragmatic, Mono, Experienced }   // listed by `resume styles`, embedded via include_dir

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, strum::Display, strum::AsRefStr)]
pub enum Paper {
    #[default]
    #[strum(serialize = "us-letter")] Letter,   // AsRefStr value is passed to Typst's `page(paper: ..)`
    #[strum(serialize = "a4")] A4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum OutputFormat { #[default] Human, Json }

#[nutype(validate(greater_or_equal = 1, less_or_equal = 20),
         derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Display, FromStr, TryFrom, Into))]
pub struct PageLimit(u8);          // --max-pages
```

### Markdown Parsing (`src/markdown/`)

The parser uses `pulldown-cmark` 0.13 with `into_offset_iter()`, so every event carries a byte range for diagnostics. It runs in two phases. Phase 1 (`tree.rs`) lowers the events into a small located syntax tree, iteratively and with depth caps, and maps every piece of text back to bytes in the original source. Phase 2 (`blocks.rs`, `role.rs`, `inline.rs`, `period.rs`, `lint.rs`) interprets that tree into the model and reports findings. Lints run in phase 2, where every node still has its source range, so the model carries no span fields.

`parse(path, text)` returns `Parsed`, whose fields are private. `valid()` is `Some` only when no error-severity diagnostic exists, which is what `build`, `export`, and `check` use. `best_effort()` is `Some` whenever a valid name heading was found, which is what `doctor` uses to lint the content of a file that still has format errors. The invariants are: no error implies a model, and no model implies an error. Parsing never aborts on content errors. Invalid nodes are dropped (an unrecoverable role) or degraded without losing text (an invalid link keeps its text, an era outside a role becomes a paragraph).

Options: smart punctuation, tables, and footnotes. Tables and footnotes are on only so they can be diagnosed. Math, strikethrough, YAML metadata blocks, heading attributes, task lists, and definition lists stay off: a resume writes `$39K` and `~5`, and a `---` line is not front matter. Every span and Edit is checked at the one public exit, `parse`: it is in bounds and on UTF-8 character boundaries, so a consumer that slices the source with it can never panic. A violation panics in debug builds, which is how the test suite finds one, and is snapped to the nearest boundaries in release builds. A UTF-8 BOM is skipped, and a lone `\r` is treated as a line ending (offsets are unchanged, because the replacement is byte for byte). All spans index the original text, for LF, CRLF, and mixed line endings. A list's span starts at its first marker: the parser's own range for a list inside another one starts where the parent item's content column ends, so it includes any indentation past that column, the unspent part of a tab, and, after a blank line, the blank line's line ending; `tree.rs` moves the start past them. A block quote, fenced code block, heading, HTML block, or footnote does not have this problem. An indented code block's range starts after its four columns of indent, so it can begin with further indentation, which is code content and is left alone.

HTML comments are skipped unless they are annotations (see Annotations below), and the exact page-break marker becomes a page break. Any other raw HTML is an error. A comment that never closes, text after `-->` on the same line, and a page-break marker followed by a non-blank line each hide content, so each is an `unsupported-html` error. So is a `<!--` inside a paragraph that never closes, whose text would print. `<!-->` and `<!--->` are complete comments, as in CommonMark 0.31.

| Convention | Rule | Diagnostic when violated |
|---|---|---|
| Name | Exactly one `# ` heading, the first block other than comments | Error if missing or repeated. Content before it is `content-before-section`. |
| Contact | The first paragraph after the name | Warning if missing. `too-many-contact-items` warning above 4 ` · `-separated items (book: four contact details at most). Links must parse as URLs (`invalid-link`) with an `http(s)://` or `mailto:` scheme (`unsupported-link`). |
| Section | `## Title`. Everything until the next `##` belongs to it. | Error for content before the first `##` other than the name and contact |
| Role | `### Company · Title · Period` or `### Company · Period`. A period is recognized by containing a four-digit year. | Error with a hint naming the expected shape and the separator ` · ` (U+00B7 with spaces). A separator typed as `-`, `\|`, or `•` gets a "did you mean ` · `?" hint. |
| Company description | The first paragraph directly after a role line, if it is not an era | None. This is the documented behavior. A first block that is a list or an era means the role has no description. |
| Era | A paragraph that is only `**Name · Period**`, inside a role. Following bullets belong to it until the next era, role, section, page break, or non-list block. | A bold paragraph that has ` · ` but no year gets an error, and so does an era followed by text in the same paragraph (the old tool treated that as a normal paragraph). An era outside a role gets an error. |
| Bullet label | A list item starting with `**Label:**`, the colon inside the bold | `**Label**: text` (colon outside) is a `mid-sentence-bold` warning. The Edit that moves the colon inside is offered only for the leading bold of a bullet, and only when whitespace follows the colon, because `**Label:**text` would not close the bold. Elsewhere (a paragraph, the contact line, bold in the middle of a bullet) the warning has no Edit. |
| Page break | `<div class="page-break"></div>` on its own lines, with blank lines around it, kept for compatibility | A page break inside a role body is an error (schema v1 has no `RoleBlock::PageBreak`); between roles or at the end of a section it closes the role. |
| Period | Start: `Mon YYYY`, `Month YYYY`, `YYYY`, or a season plus `YYYY`. A single date is allowed. Separator: ` – ` (en dash with spaces). End: the same forms, or `Present`. | `period-unparseable` for unreadable dates (word separators and numeric months get a targeted hint) and for years outside 1950–2100. `period-end-before-start` for a reversed range; a bare year end is compared as the end of that year, so `Mar 2017 – 2017` is valid. A hyphen, an unspaced or em dash, or `--` instead of the en dash is a fixable `period-hyphen` warning. `Current`, `Now`, or another capitalization of Present is a fixable `period-present-spelling` warning. |
| Heading marker | `#` to `######` followed by a space | A paragraph that starts with `#` markers and no space (`###Acme · …`, `##Skills`) is text, not a heading, so the hashes would print and later content would land in the wrong role: a `heading-marker-spacing` warning with no Edit (turning text into a heading is not meaning-preserving). |
| Unsupported constructs | Everything the model cannot represent: block quotes, code blocks, tables, footnotes, thematic breaks, setext headings, headings below level 3, numbered lists, and a bullet with more than one paragraph | `unsupported-markdown` error, never dropped silently. Nested lists are a `nested-list` warning and are flattened. A hard line break is a `hard-break` warning and renders as a space; it is fixable unless removing the marker would change how the paragraph is read (see Fixes). |

Names, titles, and headings collapse runs of whitespace, so a comment dropped from the middle of a name does not leave a double space; spacing inside a paragraph is the author's, and only the spaces a dropped comment or image leaves at the edges and joins are removed. An era line with markup (italics, code, a link) or wrapped over two lines is judged by its text: a wrapped plain era is valid, and markup in an otherwise era-shaped line is an `era-shape` error rather than a silent description. A bold span followed by prose (`**Python · Rust** are my favourite languages`) is not an era attempt. Not recognized, by design: a link's `"title"` attribute is dropped because it is not visible text, and `***Label:***` (bold and italic) is not a label; the latter is reported as `mid-sentence-bold`.

Every `###` heading is a role-line attempt, because the model has no plain-heading variant. A line whose separators are look-alikes (`|`, a bullet or other dot, a spaced hyphen, a misspaced middle dot) is recovered period first: the earliest separator whose suffix parses as a period is where the dates start. The result is a `role-separator` error with a "did you mean" hint, and an Edit only when it is unambiguous. No Edit is offered when the split would yield more than a company and a title, or when the only wrong separator before the title is a hyphen (it could belong to the company name, as in `Gusto - Webtab`). A hyphen with no spaces around it, as in `Rolls-Royce`, is never a separator. A split is rejected when the text before it already contains a year, or ends in a lone month, season, or Present word, so a period with a bad start (`08/2017 – 2018`, `Present – 2019`) is reported as an unparseable period, not as a separator. A look-alike directly after a backslash is literal text (`\·` is not an escape), never a separator.

Limits, all reported as `input-limit` errors: input over 1 MiB, emphasis, strong, or link nesting deeper than 8 levels (the excess is flattened, text kept), list nesting deeper than 8, and more than 500 findings (the rest are summarized in one finding whose severity is the worst of those suppressed). Role and era lines over 512 bytes are `text-too-long`. The depth cap matters beyond this parser: Typst's `json()` rejects documents nested deeper than 128 levels, so deep emphasis would break every style.

Format lints run during parsing. They are warnings about how the Markdown is written:

- Nested lists. The book says to avoid sub-bullets.
- Roles that aren't in reverse chronological order within a section. A role is flagged only when the earliest date its end can denote (`PartialDate::sort_key`) is after the latest date the previous role's end can denote (`PartialDate::latest_key`), with Present as the most recent, so `2017` after `Aug 2017` is never flagged. Eras do not take part.
- Bold inside a sentence (a `Strong` that isn't a bullet label, an era, or the whole content of its paragraph or bullet). The book says to avoid bolding in mid-sentence.
- Images. The book says no photos. They are dropped from the model.
- More than four contact items, counted over `·`, `|`, and `•` separators, never inside link text.

Content advice (bullet length, quantification, weak openers, and so on) belongs to `doctor`. See the Doctor section.

#### Annotations (`annotation.rs`, `model/annotation.rs`; rsm-141.22.1)

A baseline holds every item and every wording; annotations in Markdown comments say how each item may be tailored, and a position spec (rsm-141.23) chooses tags and wordings for one application.
A comment is an annotation when the first word of its body is `@`, `#`, or `=` followed by an ASCII letter (`annotation::is_annotation`, the one predicate resumes and letters share). The letter keeps a commented-out `## Section`, a `<!-- ===== -->` banner, and `<!-- #1 ... -->` ordinary, while `#AI` or `@Draft` is still a located typo.
Any other comment is ordinary and dropped as before.

| Token | Meaning |
|---|---|
| `@core`, `@extra` | Priority: at most one. |
| `@draft`, `@verify` | Status: at most one. |
| `#tag` | A topic tag; zero to many, duplicates collapse. |
| `=group.variant` | One wording of a group; at most one. |

Tag, group, and variant names are lowercase ASCII letters and digits, starting with a letter, with single hyphens, at most 40 characters (`Tag`, `GroupName`, `VariantName`). There is no case folding, so a spec written months earlier still matches.

Two placements, both supported:
- Inline, as the last thing on the content line, annotating that same line: `- Led the gateway. <!-- =gateway.default @core #ai -->`. An ordinary comment after it is allowed.
- Alone on its own line, annotating the content on the line directly below it, in the same container: the document root or the same list item. Above a list it marks the first bullet; at a bullet's own indentation between bullets it marks the next bullet.

The tree cannot tell a blank line from none, so the full-line rule is checked on source lines (`annotation::leading`, a pre-pass per block sequence). A column-0 comment between bullets ends the list in CommonMark; the walker joins the halves back, so annotating a bullet never changes the model.

Positions: section headings, role lines, era lines, blurbs, bullets, and paragraphs. Alternates are allowed on blurbs, bullets, and paragraphs only. Annotations are `#[serde(skip)]` fields on `Section`, `Role` (and `blurb_annotations`), the era, `Bullet`, and both paragraph kinds, so JSON schema 1 is unchanged and `doctor --fix`'s whole-model comparison sees them.

Ambiguity is an error, never a silent drop:

| Code | When |
|---|---|
| `annotation-unknown-keyword` | An `@` word outside the four, with a did-you-mean hint. |
| `annotation-malformed` | A bad name, a token without a sigil, or prose mixed into the comment. |
| `annotation-conflict` | Two priorities, two statuses, or two alternates in one comment. |
| `annotation-duplicate` | Two annotations for one item (two lines above it, one above and one inline, or two on one line); never merged. |
| `annotation-without-target` | A full-line annotation with nothing after it in its container, including one indented under the bullet above. |
| `annotation-detached` | A blank line or an ordinary comment between a full-line annotation and its target. |
| `annotation-misplaced` | In the middle of a line, on or above the name or contact line, above a page break or unsupported block, on a line that shows nothing (an image only), a bullet that is only an annotation, a line that would otherwise continue the text above it, or a comment line between bullets that is not at their column (it would turn a nested bullet into a sibling). |
| `alternate-not-allowed` | An alternate on a section heading, role line, or era line; its other tokens are kept. |
| `alternate-missing-default`, `alternate-variant-repeated`, `alternate-priority-conflict` | A group without `default`, with a variant name twice, or whose variants do not all state the same priority (stating it on none is fine). |
| `alternate-not-adjacent` | Variants of one group apart from each other, in different containers (section body, role, era, parent bullet), across a role line, era line, or page break, or of different kinds (bullets and paragraphs mixed). |
| `alternate-group-reused` | A second group with the same name anywhere in the document; a spec picks by group name alone. |

A group is a run of consecutive items of one kind in one container (a section body, role, era, or parent bullet; a blurb and the paragraphs after it are one kind) that share a group name (`annotation::group`, over a flat list of `GroupItem<T>` with caller-numbered containers and a caller payload, so letters reuse it). A comment line between bullets rejoins the list only when it is at their column and the markers match, as CommonMark would without it. The outline keeps each valid group's variant texts for doctor's `alternate-facts-differ` warning.

### Selection (`src/select/`; rsm-141.22.2)

`select(&Resume, &Selection) -> Result<(Resume, SelectionReport), SelectError>` turns the annotated baseline into the resume for one application.
`Session::assess` runs it between parse and compile (`pipeline.rs`), so `build`, `doctor`, and `check` all compile the tailored resume; `Assessment::parsed` stays the whole baseline, so doctor's content lints and `--fix` read every item. Selection needs no fonts or style, so its result, and a selection error, are known even when setup failed.
With the default `Selection` every build leaves drafts out.

Two layers, so cover letters (rsm-vqr.2) reuse the rules without the resume walk:
- `select::rules` (no document types): `judge(Facts, Selection) -> Verdict` for one item; `GroupFacts::of` and `judge_group` for a group judged as one item (its shared priority, the union of its tags); and `resolve(group, variants, Selection)` for one group's wording.
- `select::resume`: collects groups by their document-unique name, resolves each, walks the resume with the inherited tags, keeps or drops each leaf, and drops empty containers on the way back up.

`Selection` is a `Scope` (`All`, `Tags(SelectedTags)` which is never empty, or `CoreOnly`), `drafts`, `prefer` (variant names, most preferred first), and `picks` (group to variant).

The keep rule, first match wins: a draft is dropped unless drafts are on, even when `@core`; `@core` is kept; core only drops the rest; with no tags everything else is kept; with tags, an item whose own or inherited tags meet them is kept, `@extra` is dropped, an untagged item is kept (general), an item with only inherited tags is dropped as `inherited-tags-only`, and the rest as `tags-not-selected`.

Leaves are the judged items (bullets, paragraphs, blurbs; a group counts as one).
Sections, roles, and eras are containers: their tags flow down (union); `@draft` on one drops it and everything under it; a role's priority is its blurb's when the blurb has none, which is how a `@core` role keeps its blurb; a container that held something, all of which selection left out, is dropped, `@core` or not, and reported as `empty`. One the author wrote empty (a heading-only role, an empty section) stays, as it would without selection. So every eligible `@core` leaf survives and selection never leaves a heading alone.

A group's wording: its pick; else the first preferred name it has that is ready; else `default`; else its first ready wording (`first-ready`, when `default` is a draft); else the group is dropped (`no-ready-variant`). A pick of a draft wording is `SelectError::PickedDraft`, listing every such pick in document order, unless drafts are on (it becomes `Error::Select` and the build stops). A preference skips a draft wording and the report names it. A pick that names no group or no variant is reported (`UnknownPick`), and the spec layer (rsm-141.23) turns it into an error.
A group is judged as one item by the union of its wordings' tags, and the kept wording carries that union, so selecting the result again changes nothing.
The kept wording stays where the group was; when a blurb's chosen wording is the paragraph after the blurb, it takes the blurb's place.

`SelectionReport` lists every dropped node with a `DropReason` (a dropped group once), naming each by kind, a label, and its index in `Resume::nodes()` (the parser's `Parsed::node_lines` maps that index to a 1-based source line, so the model stays span-free), each chosen wording with `ChosenBy` and the others, kept `@verify` nodes, draft wordings a preference skipped in groups that render, unknown picks, and the draft count: with drafts off, every node marked `@draft` is counted as left out except what a draft container holds; with drafts on, every one is counted as included. Nodes are named by kind and a short label; the model has no source positions.

#### Tailoring from the command line (rsm-141.22.3)

`cli::SelectArgs` (`--tags`, `--core-only`, `--prefer`, `--drafts`) is flattened into `build`, `watch`, `doctor`, `check`, and `export`, and later letters; `selection()` turns it into the `Selection` a position spec also produces. Names are parsed by the model newtypes, so a malformed one is a usage error (exit 2), and `--tags` conflicts with `--core-only`. A well-formed name the file does not use is a Setup warning, `unknown-tag` or `unknown-wording`, listing the file's own (`select::UnknownNames`), so it reaches human and JSON output and doctor alike.

`select::SelectionSummary` is the one value both outputs read. It is silent (prints nothing) when the default selection changed nothing, so an unannotated resume builds exactly as before. Otherwise `build` and `watch` print, under the fonts line: the selection, kept and dropped counts by kind, one line counting the wordings by how they were chosen, then one line each for what can surprise (a `first-ready` fallback, a skipped draft wording, a drop caused only by an inherited tag, an emptied container, a `@verify` item, with its source line), the draft count, and `core alone: N pages`. The core-only count is one more compile of the core-only selection with the same style, paper, fonts, preferences, and drafts setting (`Session::core_pages`, about 40 ms for a 5-page resume), skipped when the build is already core only or nothing is `@core`; it equals a separate `--core-only` build. With `--format json`, the same facts are one `{"type":"selection",...}` JSON-lines record on stdout (`select::SelectionRecord`, written by `diagnostics::emit_json_record` with the line-safe escaping findings use).

`resume tags <FILE>` (`commands/tags.rs` over `select::Inventory`) lists a baseline's tags (own and inherited counts), the four keywords, and its groups with every wording, the default, and drafts (own or under a draft container), each with its source line, from the whole baseline before any selection. Human output is a listing on stdout; `--format json` prints one `{"type":"inventory",...}` object. A file with format errors prints its findings and no inventory, and exits 1.

### Diagnostics (`src/diagnostics/`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize,
         strum::Display, strum::AsRefStr, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Severity { Note, Warning, Error }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display, strum::EnumIter)]
#[serde(rename_all = "kebab-case")]
pub enum Category { Format, Content, Layout, Setup }

/// Stable, documented identifier for every finding; `resume doctor --explain <code>` prints its advice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize,
         strum::Display, strum::EnumString, strum::IntoStaticStr, strum::EnumIter)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCode {
    // Format (parser)
    MissingName, DuplicateName, ContentBeforeSection, RoleLineShape, RoleSeparator, EmptyRolePart,
    EraShape, EraOutsideRole, PeriodUnparseable, PeriodEndBeforeStart, PeriodHyphen,
    UnsupportedHtml, InvalidLink, UnsupportedLink, NestedList, RoleOrder, MidSentenceBold, Image, TooManyContactItems,
    MissingContact, PeriodPresentSpelling, UnsupportedMarkdown, HardBreak, TextTooLong, EmptyHeading, InputLimit,
    HeadingMarkerSpacing, PlaceholderUnresolved, // letter field resolution, after selection
    // Content (doctor)
    LongBullet, UnquantifiedBullet, WeakOpener, FirstPerson, SloppyPhrase, LongSummary,
    EducationNotLast, OldRoleDetail,
    // Layout (render)
    PageLimitExceeded, HeadingNearPageBottom, SparseLastPage, FontFallbackMetric, FontFallbackLayout, FontMissing,
    // Setup (doctor)
    UnknownStyle, StyleMissingMain, OutputNotWritable, TypstError, TypstWarning,
}

impl DiagnosticCode {
    pub fn category(self) -> Category;
    /// `None` until the task that owns the code writes its advice (exhaustive match, no wildcard).
    pub const fn advice(self) -> Option<&'static Advice>;
}

pub struct Advice {
    pub summary: &'static str,          // one line, shown by `doctor` and `--explain` (the renderer prints the hints instead)
    pub why: &'static str,              // rationale, citing the book where applicable
    pub fix: &'static str,              // what to change
    pub example: Option<(&'static str, &'static str)>,  // (before, after)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind { Markdown, Style }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span { pub file: PathBuf, pub kind: SourceKind, pub range: Range<usize> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,                // specific to this occurrence
    pub span: Option<Span>,
    pub hints: Vec<String>,             // occurrence-specific, e.g. the corrected line
    pub fix: Option<Edit>,              // present when the fix is mechanical and meaning-preserving
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit { pub range: Range<usize>, pub replacement: String }
```

Using `strum` on `DiagnosticCode` gives:
- stable kebab-case IDs for output and `--explain`
- `EnumIter`, so a test can assert that every code has advice and the README catalog can be generated
- `EnumString`, for parsing `--explain` arguments

Advice lives in a `match` in `advice.rs`, not in strum attributes, because multi-line rationale and before/after examples read poorly inside attributes. The match is exhaustive with no wildcard, so adding a code forces a decision. Format codes are filled in; the owner of each other category writes its entries, and its test then tightens "every Format code has advice" to "every code has advice". A rationale cites *The Tech Resume Inside Out* only where the book says it, and otherwise starts "Project convention:".

Markdown and Typst diagnostics share one type and are rendered with `codespan-reporting` 0.13, the same renderer Typst's own CLI uses. Typst diagnostics are converted by resolving each span's `FileId` through `ResumeWorld` back to the style file path and byte range. `--format json` emits diagnostics as JSON lines for editors and scripts.

#### Rendering (`src/diagnostics/render/`)

```rust
pub fn emit_human<'d>(out: &mut dyn termcolor::WriteColor, sources: &Sources<'_>,
                      diagnostics: impl IntoIterator<Item = &'d Diagnostic>) -> Result<(), RenderError>;
pub fn emit_json<'d>(out: &mut dyn io::Write, sources: &Sources<'_>,
                     diagnostics: impl IntoIterator<Item = &'d Diagnostic>) -> Result<(), RenderError>;

pub struct Sources<'a> { /* text by path */ }
impl<'a> Sources<'a> {
    pub fn new(base_dir: Option<PathBuf>) -> Self;       // paths under base_dir are shown relative to it
    pub fn insert(&mut self, path: impl Into<PathBuf>, text: &'a str);
}
pub struct Summary { pub errors: usize, pub warnings: usize, pub notes: usize, pub fixable: usize }
pub fn resolve_color(mode: ColorMode, env: ColorEnv) -> termcolor::ColorChoice;   // never `Auto`
```

The renderer does no input I/O: every caller registers the text its spans index into, under the same path the spans carry. The CLI registers the file it read. Built-in styles register their embedded text under a recognizable relative path such as `builtin/pragmatic/style.typ`, so it does not read as a real absolute path. Typst sources are borrowed from `typst::Source::text()` for the duration of one render. Output is written in the order given; sorting and grouping belong to the caller (`doctor` calls `emit_human` once per category).

**Human output** is a rustc-style excerpt with an ASCII frame, one block per finding:

```
warning[period-hyphen]: dates are separated by `-`; use an en dash with a space on each side
   --> drafts/Resume-Org-Leader.md:41:51
    |
 41 | ### Globex · Engineering Director, Platform · 2015 - 2017
    |                                                  ^^^ fixable
    |
    = help: write `2015 – 2017`
```

- The header is `severity[code]: message`, then `--> file:line:column`. The caret label says `fixable` when the finding carries an Edit; the Edit text itself is not shown, because the hints already carry the corrected text.
- Each hint that is not blank is a `= help:` note. A finding with no span prints only its header and notes; a fixable one adds `= note: fixable`.
- The advice summary is not shown here. `doctor` and `--explain` show it, because most messages already say the same thing.
- A span with no visible width (a zero-width space, combining marks, a variation selector) would get no caret from codespan, which sizes carets by display width. The caret moves to the next visible character (or past the end of the line) and a `= note: the span has no visible width; the caret marks where it sits` says so; the header column stays the true one.
- A finding whose source was not registered, or whose span starts past the end of the text, is *unlocated*: it prints a note naming the file and the reason (`= note: in `style.typ`; no excerpt: source not available`) rather than a misleading location.
- The frame is always ASCII (`-->`, `|`), with no option: legacy Windows consoles, CI logs, and text pasted into issues all render it. The quoted source line is the user's own text, so it can still contain `·` and `–`.
- `Summary`'s `Display` is `N errors, N warnings[, N notes][; N fixable]` (singular for 1; notes and fixable appear only when non-zero; a clean run is `0 errors, 0 warnings`). `has_errors()` is all the renderer says about exit codes; mapping to codes belongs to the CLI.

**Bounded and inert.** Source text is untrusted, so an excerpt is a bounded copy, never the raw lines:

| Rule | Value |
|---|---|
| Source lines shown for one finding | 6, with a `= note: excerpt shortened; the span runs from 10:3 to 9000:1` |
| Characters shown from one line | 120, around the span, with `...` where the line is cut (30 characters of context before the span) |
| Message, each hint, and the file name (human output only) | 500 characters, then `...`; JSON stays complete |
| Control and bidi characters (C0, C1, DEL, U+061C, U+200E/F, U+202A-E, U+2066-9), in the excerpt, message, hints, and file name | written as ASCII `\u{1b}`; tabs are kept |
| Line and column in the header | always the true values, whatever was cut |

So 500 findings on a 400 KiB line produce about 300 KB, not 400 MB, and `--color never` or `NO_COLOR` output cannot contain an escape byte even when the source does. A span that is not on character boundaries, is reversed, starts inside a BOM, or ends on a line terminator is snapped to the text first (`LineIndex::snap`); it never panics.

**Lines and columns** (`lines.rs`, used by the parser, the human header, and JSON, so they agree by construction): a line ends at `\n`, `\r\n`, or a lone `\r`; a leading BOM belongs to no line; lines are 1-based; columns are 1-based and count Unicode scalar values from the start of the line (a tab counts as 1, and so does a wide or combining character). An editor that wants UTF-16 offsets converts from the line text.

**Color** (`ColorMode`: `auto` (default), `always`, `never`). `resolve_color` returns only `Always` or `Never`, because termcolor's own `Auto` does not check for a terminal and treats an empty `NO_COLOR` as set. The rules: `always` wins, even over `NO_COLOR` (no-color.org); `never` is never colored; `auto` colors only when the stream is a terminal, `NO_COLOR` is unset or empty, and `TERM` is not `dumb`. The CLI passes `ColorEnv::detect(stderr().is_terminal())`.

**JSON lines** (`--format json`): one object per finding, one finding per line, each terminated by `\n` on every OS. Every key is always present; `null` means unknown.

```json
{"type":"diagnostic","severity":"warning","code":"period-hyphen","category":"format","message":"dates are separated by `-`; use an en dash with a space on each side","file":"drafts/Resume-Org-Leader.md","line":41,"column":51,"end_line":41,"end_column":54,"hints":["write `2015 – 2017`"],"fixable":true}
```

| Key | Type | Meaning |
|---|---|---|
| `type` | string | `"diagnostic"` for a finding; a tailored build also writes one `"selection"` record (rsm-141.22.3), whose keys are listed in the Tailoring section |
| `severity` | string | `note`, `warning`, or `error` |
| `code` | string | The stable kebab-case code |
| `category` | string | `format`, `content`, `layout`, or `setup` |
| `message` | string | Specific to this occurrence; never truncated |
| `file` | string or null | The path as shown to people (relative to the current directory when under it); `null` when the finding has no span |
| `line`, `column` | number or null | 1-based start; `null` when the finding has no span or cannot be located |
| `end_line`, `end_column` | number or null | Just past the span (exclusive), in the same units |
| `hints` | array of strings | Possibly empty |
| `fixable` | bool | The finding carries an Edit; `doctor --fix` applies it |

The contract is additive only: new keys may be appended, existing keys never change meaning or type, and consumers must ignore keys they do not know. There is no version field, summary object, or `fix` object (the exact Edit stays with `doctor --fix`). A finding that cannot be located keeps its `file`, and `file` is the path exactly (invalid UTF-8 becomes U+FFFD); only the human output escapes and caps it. A record never contains a character that a line splitter could break on: control characters and U+0085, U+2028, and U+2029 are written as `\u` escapes, so Python's `splitlines()` and `\n` agree. The record is a private serde type, not `Serialize` on `Diagnostic`, so the public contract does not depend on the internal byte-range representation.

**Streams** (for the CLI): human diagnostics and the summary go to stderr, like cargo and rustc; JSON lines go to stdout and nothing else does. Use termcolor's `BufferedStandardStream` (codespan writes one character at a time, and stderr is unbuffered), and treat `BrokenPipe` as a quiet exit: both emitters report every write failure as `RenderError::Write`, whose `source.kind()` is the original `io::ErrorKind`.

### Typst Integration (`src/typeset/`)

```rust
pub(crate) struct ResumeWorld {
    library: LazyHash<Library>,       // Library::builder().with_inputs(inputs).build()
    fonts: Arc<FontSet>,              // shared by every render; book() and font(i) delegate to it
    main: FileId,                     // /<style>/style.typ
    resume_id: FileId,                // /resume.json
    resume_json: Bytes,               // served from memory, ahead of disk
    style: StyleSource,               // built-in (include_dir) or a directory on disk; cloned in, so a Compiled can outlive the caller's
    files: Mutex<HashMap<FileId, FileResult<Bytes>>>,  // every style file is read once per render, failures included
    sources: Mutex<HashMap<FileId, Source>>,  // decoded text (BOM stripped); also what diagnostics are shown against
}
impl typst::World for ResumeWorld { /* library, book, main, source, file, font, today */ }

pub struct Typesetter { fonts: Arc<FontSet> }
impl Typesetter { pub fn compile(&self, &Resume, &StyleSource, RenderOptions) -> Result<Compiled, Error>;
                  pub fn evict_cache(max_age: usize); }  // for watch: Typst's memoization cache never shrinks by itself
impl Compiled { pub fn page_count(&self) -> usize; pub fn warnings(&self) -> &[Diagnostic];
                pub fn sources(&self) -> &StyleSources; pub fn export_pdf(&self, ident: &str) -> Result<Vec<u8>, Error>; }
```

Compile and PDF export are separate steps, so `--dry-run`, `doctor`, and the layout report can use the laid-out document without producing bytes. Both run on a named thread with a 16 MiB stack, so a Typst panic becomes `Error::TypesetPanicked` instead of killing `watch`.

- **Pinned dependencies:** `typst`, `typst-pdf`, `typst-kit`, and `typst-layout` are all `=0.15.1`. `typst-layout` is needed because `PagedDocument` lives there and `typst` does not re-export it. `typst-kit` has no `FontSearcher` in 0.15.1; it provides `FontStore`, `FontSource`, `embedded()`, `system()`, and `scan()`. It is enabled with `embedded-fonts` and `scan-fonts` (rsm-141.18); `scan-fonts` adds `dirs`, `dirs-sys`, and the MPL-2.0 `option-ext` to the build, and `fontdb` and `memmap2` were already linked through `krilla-svg`. `typst-as-lib` is not used, because its README says its API is unstable and it adds a second layer that would lag Typst releases. `tests/dependencies_test.rs` enforces the pins and that no network crate (`ureq`, `openssl`, `native-tls`, `reqwest`, `hyper`, `rustls`) is in `Cargo.lock`.
- **Virtual root:** the style directory's parent, so a style can import `/lib/components.typ`. Typst rejects `..` above the root and backslashes before the `World` is called (`path ... would escape the project root`), and treats an absolute path as root-relative (so `/etc/passwd` is "file not found"). Our `AccessDenied` covers what remains: a symlink inside a style directory that resolves outside it. That is deliberately stricter than Typst's own CLI. Embedded styles are case-sensitive, directories are not on macOS and Windows.
- **Style root and symlinks:** the whole styles directory is readable, not only the chosen style, because styles share `/lib`. A style can therefore read any file under the directory passed as `--styles-dir`, so that flag must never default to a directory that holds private files (rsm-141.10). `StyleSource::directory` refuses a style directory that is a symlink leaving the root (`StyleOutsideRoot`) and every read re-checks containment by path components after canonicalizing, so `styles-evil` is not inside `styles`. FIFOs and devices are refused.
- **Reads are consistent per render:** each style file is read once and served from memory, so a file edited mid-compile cannot answer differently for the same id (Typst assumes a pure `World`). A UTF-8 byte order mark is stripped from sources, as the Typst CLI does, so a Windows-saved style renders the same. Invalid UTF-8 is reported with the file name.
- **Packages are refused:** a `VirtualRoot::Package` file gets `FileError::Package` with "packages are not supported; copy the file into the style directory". No downloader code is linked.
- **`today()` returns `None`,** so a style can't make output depend on the date. `datetime.today()` is then a hard error, "unable to get the current date", at the call.
- **`sys.inputs` carries render options and the installed font families:** `paper` (`"us-letter"` or `"a4"`) and `fonts`, the lowercased families of the `FontSet`. Typst warns `unknown font family` for every missing name in a `text(font: (..))` list and cannot be told not to, so a style builds its font list with the `font-stack` helper, which keeps only installed names. In Bundled mode the list holds only bundled families, so no installed font can reach the output. When neither the preferred nor the fallback font is installed, the helper returns the fallback anyway, so Typst warns and the CLI reports `font-missing`. An `accent` override is deferred until a CLI flag produces it. Content always comes from `/resume.json`; a file of that name on disk is shadowed.
- **PDF export:** `PdfOptions { ident: Smart::Custom(<file stem>), creator: Smart::Custom(Some("resume-rs <version>")), timestamp: None, tagged: true, ..Default::default() }`. Tagged output is kept on for accessible reading order.
- **Diagnostics:** Typst errors become `TypstError` and warnings `TypstWarning`, with a style-file span from the byte range. Hints and trace frames become hint strings. Files embedded in the binary are shown as `<built-in>/<style>/style.typ`. Spans in `/resume.json` or outside the style files have no span. Line numbers use Typst's own line index (`\r`, `\r\n`, and Unicode breaks). PDF export errors are located too, because `Compiled` keeps its world. Spans into data files read with `json()` or `read()` have a path but no entry in `sources`, so a renderer must show those without an excerpt.
- **Determinism, verified on one machine (`tests/determinism_test.rs`):** 20 repeated renders, a cold versus warm memoization cache, `RAYON_NUM_THREADS` 1, 2, and 8, 500 file ids interned first, and a fresh process all give the same bytes. A different ident or different content changes them. No dates are written. Cross-OS equality is checked in CI (rsm-141.15, `scripts/compare-pdfs.sh`): a job builds the fixture with `--fonts bundled` on macOS and Windows and compares the PDF bytes.
- **Depth limits (`tests/nesting_test.rs`):** nesting beyond about 35 blocks gives "maximum layout depth exceeded", and beyond the parser cap "maximum parsing depth exceeded". Both are diagnostics; the cap is reached long before any stack limit, and the largest passing depth also compiles with worker stacks at a quarter of the default.

### Styles

Each style is a directory:

```
<style>/
  style.typ   # structure: reads /resume.json, lays out sections, calls components
  theme.typ   # theme: a `theme` dictionary of fonts, sizes, colors, spacing, page margins
  README.md   # what the layout is for, its source template, and the knobs in theme.typ
```

- **Shared components** (`styles/lib/components.typ`; the user guide's `### Shared components` table lists every one, and a test fails when a component is added without a row). Role and era headings are wrapped in `block(sticky: true)`, so they always move to the next page with their first bullet. Each bullet is `block(breakable: false)`, so it never splits across pages.
- **Layout probes:** `components.typ` emits labeled `metadata`: `<resume-fonts>` (`theme.fonts`: role to `(preferred, fallback, metric-compatible)`), `<resume-role>` (`index`, `company`, `title`, `period`), and `<resume-era>` (`role`, `index`, `name`, `period`). `Compiled::font_roles()` and `Compiled::probes()` (src/typeset/probe.rs) read them from the `PagedDocument` introspector, with the page and y position of each heading; malformed metadata in a user-edited style is `Error::Probe`. The keys are a contract between `components.typ` and `probe.rs`: change both together. `layout.rs` (rsm-141.10) builds the layout report on `probes()`.
- **Built-in styles** are embedded with `include_dir!("$CARGO_MANIFEST_DIR/styles")` (`StyleSource::builtin`). `build.rs` re-runs when `styles/` changes, because include_dir does not notice a file that was added or removed after its last run; `every_file_under_styles_is_embedded_in_the_binary` catches a miss. `--styles-dir <dir>` uses styles from disk instead, which is how customized styles are built. `resume export-style <name> <dir>` copies a built-in style plus `lib/` so you can start from it.

| Style | Modeled on | Structure | Default theme |
|---|---|---|---|
| `pragmatic` | Pragmatic Engineer's / Parker | Three-part header grid (location and links / name / phone and email). Ruled section headings. Role line: **Title** · Company with right-aligned dates, then the description line. | Calibri when installed, else Carlito (metric-compatible): 11 pt body, 14 pt headings, 12 pt role line and contact text, 24 pt name (the template uses Calibri throughout; a `display` font role can be switched to Georgia/Gelasio), blue `#1155cc` underlined links and accent, 0.6" side margins, line pitch 1.32em, footer with the name and page count |
| `mono` | The Mono Engineer's template | Centered name with one centered contact line (email, phone, location, links), capitalized black section titles over a black rule, no color except blue links; the role line and the rest as in pragmatic | Calibri when installed, else Carlito (metric-compatible): 11 pt body, 24 pt name, 12 pt headings, pure black text and accent, 0.6" side margins, the pragmatic spacing and line pitch |
| `experienced` | Experienced Engineer's / Venables | Two-zone layout: the section label in a 1.35" gutter at the left, beside the first block of its section, and the content in the column to its right. Role line: **Company, Title** with the bold period on the right. A block of bullets that all have a label is a skills table (label and value rows). | Carlito 11 pt for the body, the name and the section labels in TeX Gyre Pagella (a free stand-in for Book Antiqua), accent `#cc0000` (links) |

All three put the name and "Page N of M" in the footer, which the current tool already does.

**Bundled fonts** (`fonts/`, with `LICENSES.md`): besides typst-kit's own fonts, only the free fallbacks (Carlito, Gelasio, Arimo, and TeX Gyre Pagella from rsm-141.9, where it is first used), in regular, bold, italic, and bold italic. The template fonts (Calibri, Georgia, Book Antiqua, Arial) are the preferred fonts in each theme and are used when installed. See Fonts: Preferred Originals with Bundled Fallbacks.

### Fonts: Preferred Originals with Bundled Fallbacks (`src/typeset/fonts.rs`)

Each theme names, for every typographic role, the **preferred** font (the template's original, which may be non-free) and a **bundled fallback** that is free and, where one exists, has the same character widths. Non-free fonts are never bundled. They're used only when they're already installed or found on a font path. The bundled fallbacks are always present, so a build never fails for lack of a font.

**Measured comparison** (spike rsm-141.1). Advance widths were read directly from the font files for 87 glyphs (letters, digits, punctuation, `–`, `—`, `·`, `’`), and line widths were measured for resume sample lines at 11 pt:

| Role | Preferred (template) | Bundled fallback | Per-glyph width difference | Line width difference | License of fallback |
|---|---|---|---|---|---|
| Body (all styles) | Calibri | **Carlito** | 0.00% for all 87 glyphs, regular, bold, and italic | 0.00% | OFL 1.1 |
| Name and headings (`pragmatic`) | Georgia | **Gelasio** | 0.00% | 0.00% | OFL 1.1 |
| Section labels (`experienced`) | Book Antiqua | **TeX Gyre Pagella** (chosen; P052 rejected) | Pagella: mean 2.4%, wider brackets, narrower `#`, `/`, `·`. P052: mean 0.5%, only `#` −17.5% and `·` −24.9% differ. | Pagella −1.3% to +0.7%. P052 −1.6% to −0.1%. | Pagella: GUST Font License. P052: AGPL 3.0 with font exception. |
| Occasional runs (the Mono template's bullet glyphs; the `mono` style sets bullets in the body font and does not use Arimo) | Arial | **Arimo** | 0.00% except `·` +19.9% | 0% to +0.3% | OFL 1.1 |

Cambria was dropped. Caladea doesn't match the installed Cambria 6.90 (lines are 4–8% narrower), and none of the templates use Cambria visibly. Lato (Carlito's parent) is 8–18% wider than Calibri, so it's useful only to judge feel. All four originals allow embedding: Calibri and Book Antiqua have `fsType` set to editable embedding, Georgia and Arial to installable.

**Fallback mechanism.**
1. **Theme declares stacks.** For example: `fonts: (body: (preferred: "Calibri", fallback: "Carlito", metric-compatible: true), ...)`. `style.typ` passes `(preferred, fallback)` to `text(font: ..)`. Typst uses the first family present in the font book, and falls back per glyph if one is missing.
2. **Font sources are layered** into one `FontBook`:
   - bundled fallbacks (always)
   - in the default `--fonts auto` mode, also:
     - system fonts via `typst-kit` `scan-fonts`: macOS `/System/Library/Fonts`, `/Library/Fonts`, `/Network/Library/Fonts`, `~/Library/Fonts`, the `AssetsV2` mobile-asset fonts, and Adobe's CoreSync directories; Windows `%SYSTEMROOT%\Fonts`, `%USERPROFILE%\AppData\Local\Microsoft\Windows\Fonts`, the same under `Roaming`, and Adobe's directories. Windows needs no extra layer for system or per-user fonts
     - known application font directories: on macOS, `/Applications/Microsoft {Word,Excel,PowerPoint,Outlook}.app/Contents/Resources/DFonts`, where Office keeps Calibri and Cambria. The system scan does not look there, so a Mac with Office reports Calibri as `(application)`, not `(system)`. Word, Excel, and PowerPoint ship identical directories, so a directory whose files (name and size) an earlier one already holds is skipped
     - every `--font-path <DIR>` (repeatable) and every entry in `RESUME_FONT_PATH`
   - `--fonts bundled` uses only the bundled fonts (typst-kit's and the fallbacks).
   - TrueType and OpenType collections (`.ttc`) are loaded natively.
   - **Order and identity (rsm-141.18).** Layers are bundled, `--font-path` directories in command-line order, `RESUME_FONT_PATH` entries, application directories, then system. Inside one scan, faces are sorted by canonical path and face index, because typst-kit returns directory-enumeration order (verified not path-sorted). A face is identified by canonical path and index, so a symlink or a `/var` versus `/private/var` spelling cannot load it twice (typst-kit alone returned 8 faces for 4). Typst breaks a tie toward the earliest face only among equal static faces; a later variable font or a closer variant still wins, so a bundled family must ship every variant it needs.
   - **Errors.** An explicit `--font-path` or `RESUME_FONT_PATH` entry that is not a readable directory is `Error::FontDirectory` and fails the build (a single file is rejected too). System and application directories are best effort. typst-kit panics if a font file vanishes mid-scan, so a scan is retried once (a font manager or installer running at the same moment is the usual cause) and a second panic surfaces as `Error::FontScanPanicked`, naming the layer and directory. The stored path of every face is the canonical one, so a relative `--font-path` still loads after the working directory changes. `--fonts bundled` reads neither the disk nor the environment, so a stray `RESUME_FONT_PATH` is harmless.
   - **Cost.** An Auto build scans every installed font: about 0.8 s on a Mac with 1,100 faces, once per process. `--fonts bundled` takes about 5 ms (it parses the 33 bundled faces).
3. **Resolution is reported.** `components.typ` emits `metadata(theme.fonts) <resume-fonts>`. `FontSet::resolve` answers per role from the font book alone (family presence; Typst falls back to the closest variant of a family it has), and the CLI prints it in the build summary, for example `fonts: body Calibri (system) · headings Georgia (system) · labels TeX Gyre Pagella (fallback for Book Antiqua)`.
4. **Fallback diagnostics:**
   - `font-fallback-metric` (note): a metric-compatible fallback was used. Layout is identical, only the glyph shapes differ.
   - `font-fallback-layout` (warning): a fallback with different widths was used, so line breaks may change. The advice says to install the font or pass `--font-path`.
   - `font-missing` (error): neither the preferred font nor the fallback is available. This can only happen with an edited theme.

**Determinism.**
- Output is byte-identical for the same input and the same resolved fonts.
- `--fonts bundled` is byte-identical across machines and across macOS and Windows. Tests and CI use it.
- In `--fonts auto`, a machine with Calibri installed produces Calibri glyphs, and one without produces Carlito glyphs, with identical line breaks.

### Platforms and Cross-Building

**Targets:**

| Platform | Target | Built by |
|---|---|---|
| macOS, Apple silicon | `aarch64-apple-darwin` | native, local and CI |
| macOS, Intel | `x86_64-apple-darwin` | local cross (`rustup target add`), CI. Combined with arm64 into a universal2 binary using `lipo`. |
| Windows, x64 | `x86_64-pc-windows-msvc` | CI on `windows-latest`, for release artifacts and tests. `.cargo/config.toml` sets `-C target-feature=+crt-static`, because by default the exe imports `VCRUNTIME140.dll`, which Windows does not ship. |
| Windows, x64 (local cross-check) | `x86_64-pc-windows-gnu` | Local cross-build from macOS with the installed `x86_64-w64-mingw32-gcc` (`just build-windows`). It catches compile and link problems quickly. No `.cargo/config.toml` is needed: for ordinary executables rustc already links `libgcc_eh` and winpthread statically on this target. `just check-windows-imports` proves it by failing on any `lib*.dll`, `std-<hash>.dll`, or hash-suffixed import. `cargo-zigbuild` is not used: its README says only Linux and macOS targets are supported. |

**Self-contained Windows binaries.** One rule, `scripts/check-pe-imports.sh`, rejects the mingw runtime (`lib*.dll`), a dynamic Rust std (`std-<hash>.dll`), and the Visual C++ runtime (`vcruntime*`, `msvcp*`, `msvcr*`, `vcomp*`, `concrt*`), and fails closed when it finds no imports. It reads imports with the `llvm-readobj` from rustup's `llvm-tools`, so it runs on macOS, and CI runs it on the MSVC release exe. Proven on a real build: without `crt-static` the exe imported `VCRUNTIME140.dll` (run 44251515002); with it, only `kernel32.dll`, `ntdll.dll`, and `api-ms-win-core-synch-l1-2-0.dll` (run 44251779665). The rule is unit-tested by `tests/pe_imports_test.rs` on unix, because a bare `bash` on Windows can be the WSL launcher.

A cross-built Windows binary can't run its tests on macOS, so behavior on Windows is verified in CI. The gnu build is a local compile check only: its output must never be compared with the MSVC build.

**macOS universal2.** `universal2-apple-darwin` is not a rustc target (it exists only as a cargo-zigbuild pseudo-target). `just build-macos` builds `aarch64-apple-darwin` and `x86_64-apple-darwin` with `MACOSX_DEPLOYMENT_TARGET=11.0` (the arm64 floor, used for both slices), combines them with `lipo -create`, re-signs the result with an ad-hoc signature (`codesign --force --sign -`), and verifies the architectures and signature. Re-signing is needed because Apple silicon refuses to run an unsigned or invalidly signed binary, and `lipo` replaces the signature.

**CI (GitHub Actions, `.github/workflows/ci.yml`)** runs a matrix of `macos-latest` (arm64) and `windows-latest` (x64, MSVC) on every push and pull request, with `fail-fast: false`, `contents: read` permissions, a concurrency group, and 30-minute timeouts. Each leg runs `just check` and a `--locked` release build, and uploads the raw binary. Third-party actions are pinned to commit SHAs and kept current by Dependabot. A `universal2` job runs `just build-macos` on `main` and `v*` tags only. On Windows, the workflow sets `core.autocrlf true` before checkout so `.gitattributes` is exercised. The `justfile` sets `windows-shell` to PowerShell: a bare `bash` on Windows resolves to `C:\Windows\System32\bash.exe` (the WSL launcher) before `PATH` is consulted, which the first CI run hit ("Windows Subsystem for Linux has no installed distributions"). The first design, putting Git Bash first on `PATH`, could not work for that reason. Recipes that run on Windows must stay plain commands that PowerShell also runs; bash-only recipes are marked `[macos]` or `[unix]`. A failing gate publishes the tail of its output as a public error annotation, because job logs need a signed-in GitHub session. Release packaging (tarball, zip, checksums) belongs to the README and release task.

**Cross-OS PDF determinism** (a job that builds the fixture with `--fonts bundled` on both OSes and compares the PDF bytes on `ubuntu-latest`) needs a PDF-producing build, so it was a separate task, rsm-141.15, delivered in three rungs as rsm-141.6, rsm-141.7, and rsm-141.10 landed; it is in CI now. It fails closed if either artifact is missing. It costs nothing: standard GitHub-hosted runners are free for public repositories (https://docs.github.com/en/billing/concepts/product-billing/github-actions). macOS minutes would be metered at $0.062 per minute if the repo ever went private.

**Dependency hygiene.** A task adds a dependency when it first uses it, and `just check` (so every CI leg) enforces that with checks that see different things:
- The `unused_dependencies` manifest lint, set to `deny` in `Cargo.toml` (needs cargo 1.100 or newer; an older cargo warns "unused manifest key" and ignores the table, so the check only runs on current stable). It is exact about what rustc loaded, but rustc also counts a crate as used when another dependency pulls it in, so the lint cannot see an unused direct dependency that is also a transitive one (at rsm-141.17, 7 of 17: `regex`, `serde`, `serde_json`, `time`, `typst-layout`, `unicode-width`, `url`), and it never checks `[dev-dependencies]`.
- `cargo check --lib --bins` (the first line of `just unused-deps`). Clippy's all-targets run builds the test units, so a normal dependency used only by `#[cfg(test)]` code looks used there and passed the gate until CI's release build. Checking the library and binary alone makes the lint flag it: it belongs in `[dev-dependencies]`. (A dependency used only by an integration test is flagged by clippy as well.)
- `cargo machete --with-metadata` (the second line; CI installs `cargo-machete` 0.9.2 with the same action as `just`) reads the source text, so it sees an unused transitive dependency and an unused dev-dependency. It does not change `Cargo.lock` when that is up to date (a stale lock is updated by `cargo metadata`, as it would be by any cargo command).
- A guard test, `regex_is_a_dependency_exactly_while_nutype_validates_with_it`. The cargo lint cannot see `regex` (it is transitive) and machete is told to ignore it, because only nutype's generated code uses it (`validate(regex = ...)`; the build fails without it). If that validation is removed, the test fails and says to drop `regex` and its ignore entry.

**What still passes the gate:** an unused dev-dependency, or an unused dependency that is also transitive, whose name appears in a source string literal or a block comment (machete matches text and strips only `//` comments), or whose only use is in code that is compiled out (`#[cfg(any())]`, a feature nobody enables). Closing that needs a tool that parses the source (`cargo-udeps` on nightly, or an AST-based checker); it is not worth a second tool in the gate yet. A new dependency that nothing else in the graph pulls in, and that nothing names, is always caught.

**Portability rules,** which apply to all code:
- **Paths:** `PathBuf` and `&Path` everywhere. Typst `VirtualPath`s are built from path components, never by string concatenation with `/`. Paths in diagnostics are shown relative to the current directory.
- **Line endings:** text is LF in the repo and in checkouts (`* text=auto eol=lf`, plus explicit rules for `.rs`, `.typ`, `.snap`, `.toml`, `.md`, and `justfile`). The parser accepts LF and CRLF, spans are byte offsets into the original text, and `doctor --fix` edits keep the file's existing line endings. Fixtures with CRLF or mixed endings live in `tests/fixtures/crlf/` and are marked `-text`. Embedded files (`.typ`, fonts) must be byte-identical on every OS, because `include_dir` embeds whatever git checked out. Verified by simulation: under `core.autocrlf=true`, a pure-CRLF fixture survives even without the attributes, so the discriminating guards are the mixed-ending and LF fixtures (`tests/portability_test.rs`).
- **Program name:** clap derives its display name from `argv[0]`, which is `resume.exe` on Windows, so `Cli` pins `bin_name = "resume"`.
- **Replacing output files:** `src/output.rs` is the only code that writes a file. `Writer::commit` first runs the same checks in a dry run and a real write (a directory target, a read-only target, an existing target when the caller said not to overwrite), so `--dry-run` predicts the real result and exit code. A write creates the output directory, writes a temp file in the target's own directory (`.resume-*.tmp`, created with the permissions a plain write would give, not `tempfile`'s private 0600), syncs it, and persists it over the target with `tempfile` 3, which replaces atomically on both platforms; a failed persist deletes the temp file. Nothing of the old file is kept. A read-only target is refused on every OS (Unix `rename` would silently replace it, Windows refuses). On Unix a symlink target is replaced, not followed. On Windows a PDF that is open in a viewer can stop the replacement, and the error codes 5 (access denied), 32 (sharing violation), and 33 (lock violation) all map to `OutputLocked { path }` with the hint "close the PDF viewer or use `-o`", after a second look rules out a read-only file or a directory, which also give code 5. Which code a viewer really causes is not known until it runs on Windows: the `cfg(windows)` tests assert the variant and print the raw code to the CI log.
- **Opening files:** `watch --open` uses the `opener` 0.9 crate, which maps to `open` on macOS and `ShellExecuteW` on Windows.
- **Terminal output:** codespan-reporting's `termcolor` handles Windows consoles. `--color auto|always|never` and `NO_COLOR` are respected (rules under Rendering), and the excerpt frame and summary markers are ASCII (`ok`, `error`) so legacy Windows consoles render them. Source text is sanitized, so `--color never` output never contains an escape byte.
- **File watching:** `notify` 8 uses its default backend on each platform (FSEvents on macOS, ReadDirectoryChangesW on Windows, inotify on Linux). inotify also reports reads, so `watch` ignores every access event except a close after a write.

### Layout Report (`src/layout.rs`)

```rust
pub struct LayoutReport {
    pub pages: usize,
    pub roles: Vec<RolePlacement>,
    pub diagnostics: Vec<Diagnostic>,
}
pub struct RolePlacement { pub index: usize, pub company: String, pub title: Option<String>,
                           pub first_page: NonZeroUsize, pub last_page: NonZeroUsize }
pub struct LayoutInput<'a> { pub pages: &'a [PageGeometry], pub probes: &'a [Probe],
                             pub fonts: &'a [FontResolution], pub fonts_mode: FontsMode,
                             pub max_pages: Option<PageLimit> }
impl LayoutReport { pub fn from_input(&LayoutInput) -> Self;   // pure: every check, in a fixed order
                    pub fn of(&Compiled, &FontSet, Option<PageLimit>) -> Result<Self, Error>; }
```

`RolePlacement` holds `String`s and the role's index, because that is what the probes carry; rebuilding validated newtypes from style metadata would add a way to fail for no gain. The checks are pure functions over `PageGeometry` (measured by `typeset/geometry.rs`, the only code besides `probe.rs` that reads page frames), `Probe`, and `FontResolution`, so they are tested without Typst. `of` is the thin adapter. The findings come in a fixed order: page limit, overflow, stranded headings, sparse last page, fonts. None has a span (a layout finding names a page, not a place in the Markdown); the hint names the role or heading.

Checks, run on every render:
- **`page-limit-exceeded`** (error, only with `--max-pages`): names the page count and the limit.
- **`content-overflows-page`** (error): content on a page runs past the bottom of the page body. A bullet cannot break across pages, so a bullet taller than the body overflows without any Typst warning, and the text past the bottom is cut from the PDF. One finding per page, naming the role placed most recently before it. It is found by walking the leaves (text baselines, shapes, images), not by measuring frames: Typst clamps the frame of an unbreakable block to the body, so the frame box looks fine while its text runs 500 pt past the page. An overflow smaller than the gap between the body bottom and the footer (about 1.4 lines in `pragmatic`) is not reported, and overlaps nothing.
- **`heading-near-page-bottom`** (warning): a role or era heading whose top is in the bottom 8% of the page body. Sticky blocks make this rare, so it acts as a guard if a style is edited.
- **`sparse-last-page`** (warning): the last page of a document of two or more pages is less than 15% full ("trim or tighten to save a page"). An empty last page counts as 0%, and a one-page resume is never sparse.
- **`font-fallback-metric`** (note), **`font-fallback-layout`** (warning), **`font-missing`** (error): see the Fonts section. Each one names the role, the requested font, and the font that was used.

**Both percentages are measured against the page body, not the page height.** The body runs from the top of the first body block to the top of the footer; the page height would include both margins and the footer, so the same resume in a style with different margins would be judged differently. Typst flattens the page body into the page frame, so the body is not one inner frame: the frame's top-level items are the blocks, and a block that can break (a paragraph of plain text) is inlined as bare text. `geometry.rs` is the only code besides `probe.rs` that reads page frames. It separates artifacts from body by Typst's own tags. The header, footer, and background are artifacts, and Typst marks each with an `ArtifactElem` start tag and its end tag; whether the artifact is one group or inlined items depends on its content (a footer made of a grid is a group, a footer of plain text is inlined, which a verified experiment showed when the first version looked only for groups), so both forms are read, and only an artifact of kind `Footer` bounds the body. The body top is the smallest top of any body item over the whole document (content always starts at the region top on some page), using a text baseline when a body has no block at all; the body bottom is the footer's top, or the page height when a page has no footer; the content bottom is the lowest body text baseline, shape, or image, found by walking the leaves (`move` shifts a frame's position, so it is honored).

Limits: a group's transform is ignored, so rotated, scaled, or skewed content is measured by its unrotated position; a style that sets `page(foreground: ..)` has its foreground counted as body, because Typst does not mark the foreground as an artifact (the shipped styles do not use it); the body bottom is the footer top, which is about 2% above the true body bottom in `pragmatic`, and the top of an inlined footer is taken as its first baseline.

**Role placement** comes from the role probes. A role starts on its probe's page. It ends on the page before the next role when that role opens its page (its top is within 1 pt of the body top) and on the next role's page otherwise. The last role ends on the last page. When a non-role section follows the last role, `last_page` can be one page too high; an exact value needs an end-of-role probe, which `doctor`'s page map (rsm-141.11) can add if it needs one.

### Doctor (`src/doctor/`)

`resume doctor` is the advisor. It runs everything `build` runs (parse, render, layout report) without writing a PDF. It adds content advice and setup checks, and groups every finding by category, with the reason and the fix:

```
resume doctor drafts/Resume-Org-Leader.md --style mono --max-pages 3

drafts/Resume-Org-Leader.md
Format  2 findings
warning[period-hyphen]: dates are separated by `-`; use an en dash with a space on each side
   --> drafts/Resume-Org-Leader.md:41:51
    |
 41 | ### Globex · Engineering Director, Platform · 2015 - 2017
    |                                                   ^^^ fixable
    |
    = help: write `2015 – 2017`
    = why: Project convention: one separator everywhere keeps dates consistent ...
    = fix: Replace the separator with ` – ` (an en dash; Option+hyphen on a Mac).
Content 5 findings
warning[unquantified-bullet]: bullet has no number
    ...
    = why: The Tech Resume Inside Out says to quantify impact with numbers ...
    = fix: Add a measurable result: a percentage, a team size, a dollar amount, users, or time saved.
Layout  1 finding
  = roles: Acme p1-2; Globex p2-3; Initech p3
error[page-limit-exceeded]: 4 pages, limit is 3
Setup   ok (style mono; fonts: body Calibri (system); display Gelasio (fallback for Georgia); out/Resume-Org-Leader.pdf is free)

error 1 error, 6 warnings; 3 fixable with `resume doctor --fix`
```

The text the tool prints itself is ASCII (`p1-2`, `;`), like the rest of the CLI; the findings quote the user's own text, which may not be. Each category always has a header, with `ok` when empty, so the shape is fixed. The `why` and `fix` of a code print in full on its first finding in a file and as a pointer on the later ones.

**Content lints** (`doctor/content.rs`). All are warnings with book-cited advice, and each is deterministic and computed from the document alone. "From the document alone" means never from the clock or the environment, not "from the span-free model": the lints read the parser's located outline (`markdown/outline.rs`), which the same pass that builds the model records, so a finding points at the words that caused it. `build` does not run them; `doctor` and `check` do. The rules, exactly:

| Code | Rule |
|---|---|
| `long-bullet` | A bullet of more than 320 characters (label included, link addresses not, characters not bytes), roughly three lines. The book advises two lines or fewer. Any section. |
| `unquantified-bullet` | An unlabelled bullet in a role (era bullets too), outside Education, with no quantity: no digit that starts a number and no quantity word (`two`, `half`, `doubled`, `percent`, ...). A digit counts only when no letter, digit, `/`, or `.` precedes it, so `EC2`, `Q4`, `K8s`, `v2`, and `HTTP/2` do not, while `3rd`, `40%`, `$39K`, and `~5` do. A bare year from 1950 to 2100 is not a quantity (`in 2019`) unless a currency sign precedes it or `%`, `+`, `x`, `k`, `m`, or `b` follows. The book: quantify impact. |
| `weak-opener` | A bullet that opens with "Responsible for", "Worked on", "Helped", "Participated in", "Involved in", or "Assisted", whole words, any case, after a label or emphasis. Only the opening counts. Not in Education or on a labelled bullet outside a role. The book: active verbs, your contribution. |
| `first-person` | "I" (upper case only; not `I/O` or `I-9`, and not a numeral right after a label word such as `Phase I` or `Tier I`), "we", "me", "my", "our", "us" (lower case only, so `US` is left alone), and their contractions, in a bullet or Summary text, not in Education or a role's description (written in the company's voice). One finding per bullet, with a count. The book: drop "I", avoid "we". |
| `sloppy-phrase` | "etc.", "and so on", "and so forth", "and the like", "various", "stuff", whole words, never inside code, in any bullet, summary, or role text. The book: sloppy phrases. |
| `long-summary` | A Summary section (title has the word `summary` or `profile`, or is exactly `About` or `About me`) of more than 120 words, prose and bullets together. The book: keep the summary short and specific. |
| `education-not-last` | An Education section (title has the word `education`) followed by a section that is not Education, certifications, licenses, awards, honors, publications, patents, interests, or references. The book: move Education to the end. |
| `alternate-facts-differ` | The variants of one alternate group state different numbers (compared as a sorted list, keeping `,`, `.`, `%`, `+`, so `5,000+` and `5,000` differ). Project convention: alternates say the same facts in different words. |
| `old-role-detail` | A role outside Education that ended 15 or more years before the latest year in the resume, with at least 2 bullets and more than the most recent role has. The latest year is the largest year in any role or era period, and the most recent role is the one with the latest end (`Present` last): neither reads the clock, so a run gives the same findings in any year. The book: older roles take proportionally less space. |

The thresholds (320 characters, 120 words, 15 years, 2 bullets) and the word lists beyond the book's own examples are this project's reading of the book, and the advice for those codes says so. A section's class is decided from its title by whole word, so an unrecognized title gets fewer lints, never more.

**Setup checks** (`doctor/setup.rs`), run by the same session `build` uses (`pipeline.rs`): `build` stops the run when the style or the fonts cannot be set up, because it can do nothing without them, and `doctor` reports the failure as a Setup finding beside everything else it found.
- the style resolves, and its directory contains `style.typ` (`unknown-style`, `style-missing-main`); a `--styles-dir` that cannot be read or holds a style that links out of it is `styles-dir-unusable`
- each font role resolves, showing whether the preferred font or the fallback was used and where it came from (system, app directory, `--font-path`, bundled); a `--font-path` or `RESUME_FONT_PATH` entry that cannot be read is `font-path-unusable`. The fallback findings stay in the Layout group, and the Setup line names the fonts
- the output location is usable: the Writer's own dry run, so the same checks as a real write (a file or a dangling link in the way, a directory where the PDF goes, a read-only PDF) and nothing is created to test it. It cannot see permissions or free space, so the line says "would be created", "is free", or "would be replaced", never "writable"
- every Setup code is an error: an unknown style would otherwise exit 0, which `DiagnosticCode::default_severity` (one explicit table for every code) prevents

**Fixes** (`doctor/fix.rs`). A diagnostic carries an `Edit` only when the fix is mechanical and preserves meaning:
- a role or era separator typed as `-`, `|`, or `•` between parts becomes ` · `
- a period hyphen becomes an en dash
- "Current", "Now", or another capitalization of Present becomes "Present"
- the trailing spaces or backslash of a hard line break are removed, but only when removing them cannot change how the document is read, which is proven twice (`markdown/edit_safety.rs`). First, locally: the three lines around the break are re-parsed with the parser's own options (the same BOM-free text, a lone CR as a line end) and compared with and without the marker, so a line that is only the marker (a blank line, a paragraph break) or only `-`, `---`, `=`, `#`, or `>` (a setext underline, an empty heading, a block quote) is withheld on its own. Then, for the whole document: every surviving hard-break Edit is applied together, the document is parsed again, and the Edits are all withdrawn unless the model and every other finding are unchanged. Three lines cannot see an emphasis closer or a tag end further on, a link definition elsewhere, or two Edits that only form a table together. A withheld finding keeps its warning and says to rewrite or delete the line
- `**Label**: text` at the start of a bullet becomes `**Label:** text`, when whitespace follows the colon

`doctor --fix` plans, checks, writes, and re-reports, per file and in the order given:
1. The edits come only from Format findings, sorted by position. Two edits that overlap are a parser bug: the file is not fixed, and the run exits 1. Edits that only touch are fine. No edit may contain or cover a line ending, so every ending is kept as it is (LF, CRLF, a lone CR, mixed) with no conversion; the same applier (`Edit::apply_all`) serves the hard-break proof and the fix.
2. The fixed text is parsed in memory before anything is written. It must give the same model and the same findings less the fixed ones; moving the colon of `**Label**: text` into the bold is the one fix meant to change the model (the bullet gains a label), so for it the structure (section titles, roles with their periods, bullets per role) must be identical instead. A file with no name heading has no model to check against and is not fixed. The hard-break edits were proven only as a set and are applied as a set or not at all.
3. The file is written through `output::Writer` with `Overwrite::ReplaceSource`: the permissions are kept (a private resume stays private), a symlink is followed and kept, a read-only file is refused, and the write fails with `InputChanged` when the file no longer holds what was read, so an edit saved meanwhile is not lost. Nothing is written when there is nothing to fix.
4. A unified diff (`similar`, three lines of context) goes to stderr, then the report of the fixed text, so the exit code is what a second run would give. `--dry-run` runs every step but the write, labelled "would fix".

Content advice is never auto-fixed. `doctor --explain <code>` prints the advice entry to stdout, like `rustc --explain`; a code that is not one is a usage error (exit 2) that names the close ones. `doctor --format json` writes the same JSON lines as `build` (one record per finding, with its `category`), and the diff and the Setup facts stay human-only on stderr.

`check` is a separate command with no `--fix` and no `--explain` field, so clap rejects them (exit 2) and no code path from `check` reaches the fix. It prints the same report as `doctor`.

### CLI (`src/cli.rs`)

```
resume [--dry-run] [--format human|json] [--color auto|always|never] <COMMAND>

  build  <FILE>... [-s|--style <NAME>] [-o|--out <DIR>] [--paper letter|a4]
                   [--styles-dir <DIR>] [--max-pages <N>] [--fonts auto|bundled] [--font-path <DIR>]...
  doctor <FILE>... [same options as build] [--fix] [--explain <CODE>]
  check  <FILE>... [same options as build]   # alias: read-only `doctor` (no --fix, no --explain)
  watch  <FILE>    [same options as build, for one file] [--debounce <DURATION>] [--open]
  styles           [--styles-dir <DIR>]
  export-style <NAME> <DIR> [--force]
  export <FILE> <DIR> [-s <NAME>] [--styles-dir <DIR>] [--force]   # resume.json + style, for tinymist or typst watch
  licenses                              # the license notices of the fonts built into the program
```

**`--dry-run`** is global and applies to every command that writes:

| Command | With `--dry-run` |
|---|---|
| `build` | Parses, compiles, runs the layout checks, and exports the PDF (so an export error shows and the exit code matches a real build's), then reports `would write out/X.pdf (3 pages, overwrites existing)`. Writes nothing, and creates no directory. |
| `doctor --fix` | Prints the unified diff of the fixes and leaves the file unchanged |
| `export-style` | Lists the files that would be created (or, with `--force`, replaced) |
| `export` | Lists each file as `would create`, `would replace` (`resume.json`, or a style file with `--force`), or `would keep` (a style file that exists) |
| `watch` | Rejected at argument parsing (clap `conflicts_with`), since watching without writing has no purpose |
| `styles`, `check`, `doctor` without `--fix` | No effect; they don't write |

`check` is a read-only alias of `doctor`. It runs the same assessment and prints the same report, but it doesn't accept `--fix` or `--explain`, so `check` in a script or a pre-commit hook can never modify files. `doctor` is the command for fixing (`--fix`) and learning (`--explain`). A no-write build is `build --dry-run`.

- `build` prints one summary line per file, then a fonts line, then the findings. For example: `ok out/Resume-Org-Leader.pdf  pragmatic  3 pages  0 errors, 1 warning  built in 84ms`, then `  fonts: body Carlito (fallback for Calibri); display Gelasio (fallback for Georgia)`. The counts are the renderer's own `Summary` text (`0 errors, 1 warning`), so there is one count format. Markers are `ok` and `error` (green and red when color is on), separators are two spaces and `; `, and nothing the tool prints itself is outside ASCII (a font name or a path is the user's text and passes through). The time is whole milliseconds, at least 1, from reading the input to the end of the write; the one-time font scan is not in it. A `--dry-run` line reads `ok would write out/X.pdf (3 pages, overwrites existing)  pragmatic  ...  checked in 80ms`, with `new file` or `creates out` in place of `overwrites existing`. An input that has errors reads `error drafts/Resume.md  2 errors, 0 warnings  no PDF written`. Paths are shown relative to the current directory when they are under it.
- **Findings and fonts.** A metric-compatible fallback (`font-fallback-metric`, a note) is already named on the fonts line, so human output leaves the note out and its count; `--format json` keeps every finding. A fallback that changes widths (`font-fallback-layout`, a warning) and a missing font (`font-missing`, an error) are always shown. Layout findings have no span: the hint names the role.
- **A layout error still writes the PDF.** When the document compiled but a layout check fails (`page-limit-exceeded`, `content-overflows-page`), `build` writes the PDF and exits 1, so the author can open it and see what overflowed. Only an input with format errors (no model) writes nothing. `--dry-run` runs the same checks and exports the PDF, so its exit code is a real build's.
- **Several inputs.** Files are built in the order given, each with its own summary line; an error in one never hides or skips the others, and the exit code is the worst of them. Two inputs whose output names are equal ignoring case (`a/Resume.md`, `b/resume.md`) are a usage error before anything is written. The style and the font set are resolved once, so an unknown style or an unreadable `--font-path` fails the run before any file is read.
- **Streams.** Summaries and human findings go to stderr, like cargo and rustc. With `--format json`, stdout holds only JSON lines (the summary and fonts lines still go to stderr, because they are human text), and a closed pipe (`resume build ... --format json | head`) ends the output quietly. `--color` and `NO_COLOR` follow the Rendering rules.
- **`--format json` is a diagnostics format.** It changes only where findings go (JSON lines on stdout). Operational errors (an unreadable input, an unknown style, an output that cannot be written) are human lines on stderr with exit 1 in either format, so stdout stays parseable; a consumer reads the exit code and stderr for them. `styles`, `licenses`, and `export-style` have no findings and ignore `--format`: `styles` and `licenses` print their text on stdout either way.
- **Output failures.** A failed PDF export or write keeps all findings collected so far and prints a `no PDF written` summary before the error.
  Each input still gets a summary, including unreadable inputs, and later inputs are still processed.
  Summary counts include the operational error or the diagnostics carried by a compile/export error.
  The command runner records stdout write failures across all files and flushes stdout before choosing its outcome.
  Non-pipe write or flush failures cause exit 1; `BrokenPipe` stays quiet without changing the build outcome.
- **Exit codes:** 0 on success (warnings allowed), 1 on any error finding, failed build, or operational error (an unreadable input, an output that cannot be written), 2 on a usage error (from clap, plus two checks clap cannot express: `--fonts bundled` with `--font-path`, and two inputs that would write the same PDF).
- **Defaults:** `--style pragmatic`, `--out out`, `--paper letter`, `--fonts auto`, `--color auto`, `--debounce 150ms`. `--styles-dir` has no default, so nothing is read from the current directory unless asked.
- **`licenses`** prints, to stdout, the license notices every distributed binary has to carry: the overview in `fonts/LICENSES.md`, the full text of each bundled fallback family's license (the SIL Open Font License, whose condition 2 asks that a copy of it travel with the font), and the `NOTICE` of the `typst-assets` crate, which supplies the fonts Typst itself embeds (Libertinus Serif, New Computer Modern, DejaVu Sans Mono) and is Apache-2.0, whose section 4(d) requires it. The `NOTICE` is vendored verbatim in `fonts/typst-assets/NOTICE` (with `-text` in `.gitattributes` so Windows leaves its bytes alone). Tests keep the notices from drifting: every bundled family must have its license text in the output (adding a font fails until its license is added), the vendored `NOTICE` must equal the one in the locked crate (a Typst upgrade that changes it fails until it is vendored again), and the output must contain each license text in full. `--dry-run` has no effect. The notices of the other crates a binary links (MIT and Apache attribution for about 300 crates) are not covered here; the owner decided (rsm-141.13) that no attribution list for them is needed.
- **Structure.** `src/cli.rs` holds the clap types and `Cli::try_parse_validated`; `src/commands/` holds the commands as library code over injected streams (`Io`, `Env`, `Outcome`), so almost everything is tested in process; `src/main.rs` only builds the real streams and environment and turns the `Outcome` into an exit code. A test fails if any file under `src/commands/` writes the file system directly: every write goes through `output::Writer`.
- **`styles`** prints one line per style to stdout: the built-in styles (the embedded directories other than the shared `lib`), then the directories under `--styles-dir`, sorted by name, with where each lives. A directory style with the name of a built-in is marked `(overrides the built-in)`, and one that cannot be used is listed with the reason, not hidden: `(invalid: not a valid style name)` or `(invalid: no style.typ)`. Only the names the style resolver would accept are valid, as written (`Mine` would be looked up as `mine`). An unreadable `--styles-dir` is an error, exit 1.
- **Choosing a style.** `--style NAME` looks for `<styles-dir>/NAME/` first and then among the built-in styles, so a copy made with `export-style` is used in place of the built-in of its name. An unknown name is exit 1 before anything is built, and the message lists the styles that exist (`available styles: pragmatic`).
- **`export-style <NAME> <DIR>`** copies a built-in style (`style.typ`, `theme.typ`, `README.md`) to `<DIR>/<NAME>/` and the files every style shares to `<DIR>/lib/`, byte for byte as embedded, so the copy works with `--styles-dir <DIR>` and builds the same PDF as the built-in one. Only built-in styles can be copied, and `lib` is not a style. It **refuses to overwrite**: every target is looked at first, and if any file exists, nothing is written, each existing file is reported with the hint `use --force to replace it`, and the exit code is 1; this protects a customized `theme.typ`. `--force` replaces them. The paths come from the embedded tree, so none can climb out of `<DIR>`. `--dry-run` prints `would create` (or `would replace`) for each file and creates nothing, and the real run prints `created` or `replaced` for each and a final `ok exported pragmatic to mine (4 files)`.

### Export (`src/commands/export.rs`)

`export <FILE> <DIR>` parses the resume (no compile, no fonts) and writes three kinds of file: `<DIR>/resume.json`, the style at `<DIR>/<NAME>/`, and the files the styles share at `<DIR>/lib/`. That is the layout a style expects (`json("/resume.json")`, `/lib/components.typ`, `./theme.typ`), so `typst watch --root DIR DIR/<NAME>/style.typ` previews it with click-to-source, and `DIR` is also a valid `--styles-dir`, so `resume build --styles-dir DIR` builds with the edited style. With tinymist, open `DIR` as the workspace folder.

- **One serializer.** `resume.json` is `typeset::resume_json(resume, pretty)`, the function the compiler calls, so the preview cannot be given a different model. The exported file is indented and ends with a newline; a style reads both forms alike.
- **Overwrite policy.** `resume.json` is always replaced, because exporting again after editing the resume must refresh it. A style or lib file that exists is kept (reported `kept`, with `use --force to refresh the style files` on the closing line), because the author is probably editing `theme.typ` under `typst watch`; `--force` replaces them. This uses `Overwrite::Refuse`, so there is no check-then-write race, and `resume.json` is written last so a running preview never sees a new resume with a half-updated lib.
- **Which files.** `StyleSource::files()` is the one list, for built-in and directory styles. A directory style is walked (real directories only, dot-files left out) through the same sandboxed `read` that compiling uses, so a link that leaves the styles directory fails the export. A file that cannot be read fails the whole export before anything is written, rather than being skipped. A style that imports a file outside its own directory and `lib/` is legal for `build` and is not copied; Typst then reports that file as not found, and the style author copies it by hand. A style with a symlinked subdirectory builds but cannot be exported, since a linked directory is read as a file and refused, which also keeps a link from looping.
- **Exit codes.** A resume with format errors writes nothing, prints its findings and `nothing exported`, and exits 1; warnings do not stop it. An unknown style (`lib` is not a style, since it holds what the styles share) or an unreadable style file exits 1 before anything is written, and so does a target the Writer's dry run finds cannot be written (a directory or a read-only file in the way, a parent that is a file). A failure only a real write can find (permissions on a directory, a full disk, a locked file on Windows) can leave the style files written before it; a rerun reports them as kept and completes the export. `--format json` sends findings to stdout as JSON lines; the file lines go to stderr.
- **Previewing outside the CLI.** `typst watch` does not set `sys.inputs.paper` or `sys.inputs.fonts`: paper defaults to US Letter (`--input paper=a4` for A4) and fonts are not filtered. Do not pass `--input fonts=...`: the Typst CLI gives strings, and the style asserts an array. The preview uses the fonts installed on the machine, not the ones in `resume`, so page breaks can differ from `resume build`, which is the authority on page count.

### Watch (`src/watch.rs`, `src/commands/watch.rs`)

`watch <FILE>` takes the options of `build` for one file, plus `--debounce` (default `150ms`, parsed with `humantime`) and `--open`. `--dry-run` is a usage error (exit 2).

- **What is watched.** `notify` 8 watches the canonical parent directory of the Markdown file (not recursive) and, with `--styles-dir`, that directory recursively. The file itself is not watched, because an editor that saves by renaming a new file over the old one replaces the thing a file watch holds. An event counts when its file name equals the input's (ASCII case-insensitive; on a case-sensitive system an unrelated `resume.md` beside `Resume.md` costs one extra build) or it is the same file under another spelling, or when it is under the styles directory and is not a dot-file and not under the output directory. All paths are canonical, because FSEvents reports real paths (`/private/var/...`); the output directory is resolved one part at a time, so a symlink is followed before a `..` after it (`styles/link/../out` is where the link points, as for the system) and a `..` after a part that does not exist yet (`styles/new/../out`) just cancels it.
- **Which events count.** Creates, modifies, removes, and renames do. Access events do not, except a close after a write: inotify reports the build's own reads (`OPEN`, `CLOSE_NOWRITE`), and counting them would start a build after every build. macOS and Windows never report reads, so the filter has its own tests with synthetic events.
- **Order.** The watches are armed before the first build, so a save made while the fonts are being scanned is not lost, and the first summary line means the watch is ready. The filter runs in the watcher's own thread and sends a small signal over a channel of 16; a signal that does not fit is dropped, because a rebuild is already waiting, so memory is bounded however slow a build is.
- **Debounce.** A build starts `--debounce` after the last change (trailing edge, so the editor has finished writing), but no later than the larger of 2 s and twice the debounce after the first, so a file that is rewritten steadily still builds. Changes during a build wait and cause one more build. A running build is not cancelled.
- **Each build** is the `build` code path for one file (`Job::build_one`), so its summary, findings, and exit rules are the same, and the PDF is replaced atomically by the Writer. `Typesetter::evict_cache(10)` runs after each build, because Typst's memoization cache never shrinks. With `--styles-dir`, the style is resolved again before each build (a style made, changed, or removed while watching is seen); the fonts are kept.
- **Errors never end the loop.** An unreadable input (a delete and re-create), a resume with errors, a style that does not compile, and `OutputLocked` (Acrobat on Windows holds the PDF open; the hint says to close the viewer) are printed like a failed build, and the next change builds again. Only a watcher that cannot start (`Error::Watch`), a missing input, or an unusable style or font path at startup ends `watch` (exit 1), and so does a watcher that dies later (an `error:` line and exit 1; the dead watcher is leaked, not dropped, because notify's Linux drop panics when its thread is gone and the exit code would be 101). A stderr whose reader has gone (`resume watch ... 2>&1 | head`) ends the watch with exit 0 after the build that found it, the way `styles` treats a closed stdout, since a watch that went on would rebuild unseen. Asked to stop (tests only; the process is otherwise ended by the user), it exits 0. An input that is a file system root has no directory to watch beside it and is an error (`WatchInput`), not a panic. A watcher error event prints a warning and triggers a rebuild, since a change may have been missed.
- **`--open`** opens the PDF once, after the first build that wrote it, even one with layout errors, which is when the author most wants to see it. Failing to open it is a warning. Skim on macOS and SumatraPDF on Windows reload on change.
- **Stopping.** There is no signal handler: the process ends, and because every write is a temp file renamed into place, a viewer never sees half a PDF. A kill between creating the temp file and renaming it can leave `.resume-*.tmp` in the output directory.
- **Not handled.** An input given as a symlink is watched where its target lives, so an editor that saves by renaming a new file over the link (in the link's directory) is not seen; watch the file itself or give the real path. Deleting a directory style while watching falls back to the built-in style of the same name without saying so; deleting one with no built-in twin prints `unknown style` and the build fails on the missing file until the style returns. If the watched directory itself is deleted, the operating system stops reporting and `watch` does not notice (restart it). A decomposed Unicode file name is found by canonicalizing the event path, which is reasoned and not tested against a real macOS volume.

### Releases (the `release` job in `.github/workflows/ci.yml`, `scripts/package-release.sh`)

A tag `v*` runs the `release` job of `ci.yml`, after `check`, `determinism`, and `universal2` have passed, so what ships is the binary those jobs built and tested: the macOS universal2 binary (`just build-macos`, signed ad hoc) and the Windows x64 binary (MSVC, C runtime linked in, imports checked by `scripts/check-pe-imports.sh`). First it fails unless the tag is `v` plus the `Cargo.toml` version and the tagged commit is on `main`. `scripts/package-release.sh` packs each into one archive named `resume-<tag>-<label>`: a `.tar.gz` for macOS and a `.zip` for Windows, each holding the program, `LICENSE`, `README.md`, and `FONT-LICENSES.md`, plus a `.sha256` file in the form `shasum -a 256 -c` reads. The last job re-checks the four files against their checksums and creates a draft GitHub release with them and a `SHA256SUMS` file. It never publishes: the owner reads the draft and presses Publish. It is the only job with write access, and only to the release. The script is tested (`tests/package_release_test.rs`, unix), and on every push the Windows `check` job also runs it on the release exe, unzips the result with `Expand-Archive`, and runs `resume.exe --version` from it. The `release` job itself runs only on a tag, so it is checked by `actionlint` and by the first tag. The binaries are not notarized or code-signed (no paid developer accounts), and the README says how to get past Gatekeeper and SmartScreen.

### Position specs and profiles (`src/spec/`)

A **profile** is a reusable, named set of choices; a **position spec** is one application, which extends profiles and names its content file. Both are small TOML files read with `deny_unknown_fields`. `spec::resolve(spec, profiles_dir)` loads the spec and the profiles it needs, puts them in one order, and merges them into a `ResolvedSpec`. Fixtures: `tests/fixtures/specs/` (one directory per case, outcomes in its README). Analysis: `history/rsm-141.23.1_oracle-analysis-and-plan.md` and `history/rsm-141.23.1_design-note.md`.

```toml
# positions/2026-11-acme.toml            # profiles/ai-platform.toml
extends = ["principal", "ai-platform"]   # extends = ["principal"]   (optional)

[position]                               # spec only; every key optional
company = "Acme"                         # title, level, domain, audience: text
emphasis = { business = "low", org = "medium", technical = "high" }
date = 2026-11-19                        # a bare TOML date; posting is an http(s) URL

[build]
content = "../baselines/baseline.md"     # spec only; relative to this file
style = "mono"                           # a StyleName (a built-in or a --styles-dir style)
paper = "a4"                             # letter | a4, the spelling `--paper` uses

[select]
tags = ["iceberg"]
prefer = ["short"]
pick = { gateway = "long" }              # group = variant
promote = ["leadership"]                 # reserved: parsed and merged, applied by rsm-141.23.3
core-only = false
```

- **`extends` is top-level.** It drives the resume and the letter selections alike, so it cannot live in `[build]`, which is resume-only. A profile may not have `[position]` (however TOML spells the table) or `build.content`. A `posting` address may not carry a user name or password. `drafts` is not a key: `--drafts` stays a command-line preview switch, so no file can make a sent resume contain draft text. `[letter]` is an unknown key until rsm-vqr.4.
- **Names.** Tags, groups, and variants use the annotation name grammar. A profile name uses it too, plus no Windows device name (`con`, `nul`, `com1`), so a name cannot contain a separator, a drive, a dot, or a capital: `extends = ["../x"]` is refused before any file is opened.
- **Order.** Later is more specific, so `extends` lists run from least to most specific and the spec is the most specific of all. Two kinds of fact fix the order of the whole chain: a file is applied after every profile it extends, and an entry is applied before the entry listed after it. The chain uses an order that satisfies all of them; where they leave a choice, the profile the depth-first walk found first among those ready to go next goes next, so the order is the same on every run and every machine. If none exists, a file lists two profiles the other files order the other way (`["b", "base"]` where `b` extends `base`), which is `ExtendsOrderConflict` with the chain of reasons. A shared base (a diamond) is applied once. A cycle prints the way round. Limits: 64 KiB a file, 8 levels, 32 profiles. The first design checked only the order of a depth-first walk and so refused listings that some order satisfies; a test against an exhaustive search of all orders now pins the difference.
- **Merge.** Tags are a union. `prefer` and `promote` list the most specific file's names first, then the less specific files' names not already listed. A pick and a scalar (`style`, `paper`, `core-only`) are decided by the most specific file that has one. A name repeated inside one list is an error, which is what makes the merge associative. Every merged value remembers the file, line, and column that decided it (`ResolvedSpec::origin_of`).
- **Profiles are found** in `--profiles DIR`, else in `profiles/` beside the spec's own directory (`positions/acme.toml` finds `positions/../profiles`). There is no second fallback, so a directory can never be silently ambiguous. A spec that extends nothing never touches the directory. A profile that is a link leading out of its directory is refused, and so is a link that cannot be followed (dangling or looping): the containment check never passes on error, and the file that was checked is the file that is read. A profile's file name must be its name in lowercase: `Principal.toml` answers to `principal` on macOS and Windows but not on Linux, so it is refused everywhere rather than loaded on some machines. Do not make the spec's own directory a link: the operating system follows `..` after a link on macOS and Linux and Windows does not, so `positions/../profiles` could mean two places.
- **`content`** is a relative, portable path checked as text (forward slashes, no drive, no device name, no part ending in a dot or a space). It may climb with `..`. The loader joins it onto the spec's directory part by part and neither canonicalizes nor opens it. There is no extension rule: `build FILE` accepts any extension too, and a rule would not stop a spec that names another private `.md` file; the control is that the summary prints the resolved path.
- **Errors** are one `SpecError` (`Error::Spec`, operational: stderr, exit 1, in both formats), each naming the file and, where it can, the line, column, and key. It adds no `DiagnosticCode`. Echoed values, including an unknown key's path, are escaped with `display_text`, and a position's free text and a content path may not contain control or direction-changing characters at all. A read error keeps the cause the system gave. Each error is a snafu variant built with its selector. The one place that reads `toml`'s wording of an unknown key is `read::unknown_field`, which a test pins.
- **Parsing** is `toml` 0.8 (already in the lock through Typst) with `serde_path_to_error` and `Spanned` string leaves, so a wrong type names its key path and a semantic error (a bad name, a bad path) carries the exact span.
- **Output.** `ResolvedSpec::to_toml()` prints the choices as one flat spec with no `extends`; saved beside the original it resolves to the same choices. `to_selection()` makes the `Selection` (core-only with tags is `ScopeConflict`, and drafts are always off); `check_picks` turns the report's unknown picks into an error that names the line that wrote each.

### Errors (`src/error.rs`)

`snafu` errors, following the guidelines, for operational failures: reading input, writing output, an unknown style, a style directory missing `style.typ`, an unreadable style file (`StyleFile`), PDF export failure, and watcher failure (`Watch { path, source: notify::Error }`). Writing output has one variant per cause, so each can say what to do: `CreateOutputDir` and `WriteOutput` (a failure with a cause and nothing more specific to suggest), `OutputLocked` (another program holds the file open, common on Windows), `OutputReadOnly`, `OutputIsDirectory`, and `OutputExists` (the target exists and may not be replaced, as in `export-style` without `--force`). `Error::hint()` returns the one-line advice for the four that have a specific one (`OutputLocked`: "close the PDF viewer or use -o <DIR>"). Content problems are `Diagnostic`s, not `Error`s, so one run can report all of them, and `doctor` can advise on each. `main` prints the errors and diagnostics and picks the exit code.

## Key Design Decisions

| Decision | Rationale | Trade-off |
|---|---|---|
| Embedded Typst instead of HTML/CSS + Chrome | Print-grade typesetting, sticky and unbreakable blocks, deterministic output, structured diagnostics with spans, and an in-memory layout for page reports. No runtime dependencies. | Styles are written in Typst, not CSS. Typst is pre-1.0, so versions are pinned and upgraded deliberately. |
| Our own `World` (about 200 lines) instead of `typst-as-lib` | One less unstable layer, full control over file resolution, caching for watch, and diagnostics mapping | More code to maintain across Typst upgrades |
| Model passed as a `/resume.json` virtual file | A documented, versioned contract, and styles can be developed with standard Typst tools | JSON conversion cost, negligible at resume size |
| `pulldown-cmark` instead of `comrak` | Streaming events with byte offsets map directly to diagnostics, and it's smaller and very widely used | No full AST, so the parser keeps a small amount of state for one-block lookahead |
| Preferred (possibly non-free) font plus a bundled free fallback per role, with `--fonts auto` by default | Uses the template's real font where installed (Calibri is standard on Windows), keeps identical line breaks through metric-compatible fallbacks, never fails for a missing font, and reports which font each role used | Output depends on the machine's fonts in `auto` mode. `--fonts bundled` restores machine-independence. |
| Cross-build locally for quick checks, but test on each OS in CI | A Windows binary built on macOS can't run its tests on macOS. CI on `windows-latest` catches path, CRLF, file-locking, and console differences, and compares PDF hashes across OSes. | Needs the GitHub remote and Actions minutes |
| Release Windows binaries built natively with MSVC, plus a local `windows-gnu` cross-build | MSVC is the platform's standard ABI and is what Typst's own releases use. The installed mingw toolchain gives fast local cross-checks without new installs. | Two Windows toolchains to keep working |
| Structure (`style.typ`) and theme (`theme.typ`) per style | Mirrors the HTML/CSS split, so most customization touches only the theme | Two files per style |
| Stricter parsing than the Python tool (malformed roles are errors) | The feedback priority: silent reinterpretation hid mistakes | A file the Python tool accepted with a malformed heading now fails until it's fixed. Neither baseline has one. |
| Keep `<div class="page-break"></div>` | Both tools can build the same baselines during the migration | A little HTML stays in the Markdown |
| `nutype` newtypes for domain strings and numbers | Invalid values (empty company, untrimmed title, page limit 0) can't be built, and signatures say what they take | One type per concept, plus mapping nutype errors to diagnostics |
| `time::Month` plus a custom partial-date parser | Typed months, no hand-rolled month table. Resumes need month, season, or year precision and "Present", which `time`'s parser doesn't express. | Two date concepts: `time` for months, our own `PartialDate` for precision |
| `humantime` for CLI durations and timings only | Readable `--debounce 150ms` and `built in 84ms` | None. It isn't suited to resume dates. |
| `strum` on `DiagnosticCode` and the option enums | Stable IDs, iteration for the advice catalog and its tests, and parsing for `--explain` | Advice text lives in a `match`, not in derives |
| `check` as a read-only alias of `doctor`; global `--dry-run` | `check` is the safe assessment for scripts and hooks, and `doctor` adds `--fix` and `--explain`. One consistent way to preview any write. | Two names for one report |
| No `garde`; validation is nutype plus parser checks | Construction-time invariants with sanitizing, severities (warnings), and source spans, none of which garde provides. The "validated" guarantee comes from `Resume` being constructible only by the parser. | Revisit garde if a structured config or style-options file is added |
| `doctor --fix` limited to mechanical edits | Fixes are safe to apply without review, and the diff is always shown | Content issues still need a human edit |

## Dependency Graph

```
rsm-141.2 scaffold
  └─► rsm-141.3 model, newtypes, periods, diagnostic types
        ├─► rsm-141.4 markdown parser + format lints ──────────┬──────────────┐
        ├─► rsm-141.5 diagnostics rendering ───────────────────┤              │
        └─► rsm-141.6 typst world + compile + pdf              │              │
              └─► rsm-141.7 components + pragmatic + fonts ◄── rsm-141.1 font spike (no deps)
                    ├─► rsm-141.8 mono                         │              │
                    ├─► rsm-141.9 experienced                  │              │
                    └─► rsm-141.10 CLI + --dry-run + layout ◄───┘              │
                          ├─► rsm-141.11 doctor (content, setup, fix) ◄────────┘
                          └─► rsm-141.12 watch + export
rsm-141.14 cross-platform builds + CI ◄── rsm-141.2
rsm-141.13 README + release + baseline migration ◄── rsm-141.14, rsm-141.8, rsm-141.9, rsm-141.11, rsm-141.12
```

## Effort Summary

| Task | Effort | Dependencies |
|---|---|---|
| rsm-141.2 Scaffold | 2 h | none |
| rsm-141.3 Model, newtypes, periods, diagnostic types | 0.5 d | rsm-141.2 |
| rsm-141.4 Markdown parser and format lints | 1.5 d | rsm-141.3 |
| rsm-141.5 Diagnostics rendering | 0.5 d | rsm-141.3 |
| rsm-141.6 Typst world, compile, PDF | 1 d | rsm-141.3 |
| rsm-141.1 Font selection spike | 0.5 d plus review | none |
| rsm-141.7 Shared components, `pragmatic` style, fonts | 1 d | rsm-141.6, rsm-141.1 |
| rsm-141.8 `mono` style | 0.5 d | rsm-141.7 |
| rsm-141.9 `experienced` style | 1 d | rsm-141.7 |
| rsm-141.10 CLI, `--dry-run`, and layout report | 1 d | rsm-141.4, rsm-141.5, rsm-141.7 |
| rsm-141.11 `doctor`: content advice, setup checks, `--explain`, `--fix` | 1.5 d | rsm-141.10, rsm-141.4 |
| rsm-141.12 Watch and export | 0.5 d | rsm-141.10 |
| rsm-141.14 Cross-platform builds and CI matrix | 0.5 d | rsm-141.2 |
| rsm-141.13 README, release artifacts, baseline migration | 0.5 d | rsm-141.14, rsm-141.8, rsm-141.9, rsm-141.11, rsm-141.12 |

Total: about 11.5 working days.

## Open Questions

1. ~~Fonts~~ Resolved (rsm-141.1): fallbacks are Carlito (Calibri), Gelasio (Georgia), Arimo (Arial), and TeX Gyre Pagella (Book Antiqua). Pagella was chosen over P052 because it is closer on the uppercase labels where it's used (−1.25% vs. −1.59% regular, −0.68% vs. −0.85% bold), its outlying glyphs (brackets, `/`, `#`) don't occur in labels, and its GUST license avoids bundling an AGPL font in an MIT binary.
2. ~~Remote~~ Resolved: push to GitHub as `dmrolfs/resume-rs`. Tests use synthetic fixtures only. The real baselines build in an `#[ignore]` test that reads a path from `RESUME_BASELINES`.
3. ~~Python tool~~ Resolved: `tools/resume-pdf` stays in the job workspace as a fallback. rsm-141.13 adds a pointer to resume-rs in its README.

## Updates & Evolution

- 2026-11-17: Initial draft.
- 2026-11-17: Revised per review. Added `nutype` newtypes for domain values (Name, Company, Title, SectionTitle, EraName, LinkUrl, StyleName, Year, PageLimit). Replaced `Dates` with `Period`/`PartialDate` built on `time::Month`, and added `humantime` for CLI durations. Added `strum` on DiagnosticCode, Severity, Category, and the option enums. Added a global `--dry-run`. Replaced `check` with `doctor` (content advice, setup checks, `--explain`, `--fix`) and added the doctor task (now rsm-141.11).
- 2026-11-18: `LinkUrl` wraps `url::Url`, with new code `invalid-link`. `check` restored as a read-only alias of `doctor`. Resolved: GitHub remote, and keep the Python tool as a fallback. Decided against `garde`.
- 2026-11-18: Beads tasks recreated as hierarchical children rsm-141.1 through rsm-141.13 following the epics-and-tasks skill, adding the font spike rsm-141.1.
- 2026-11-18: Fonts redesigned as a preferred original plus a bundled fallback per role (measured in rsm-141.1), with `--fonts auto|bundled` and `--font-path`. Added macOS (universal2) and Windows (x64) builds: local cross-build plus a CI matrix with a cross-OS determinism check (task rsm-141.14).
- 2026-11-18: Book Antiqua fallback decided: TeX Gyre Pagella. Spike rsm-141.1 closed.
- 2026-11-18 (rsm-141.14 follow-up): the shipped MSVC exe imported VCRUNTIME140.dll. Added the self-contained check for the release build and `crt-static` for the MSVC target; verified before and after on CI.
- 2026-11-18 (rsm-141.14, oracle analysis in history/): universal2 is built with two builds plus `lipo` and re-signed. The windows-gnu local build needs no `.cargo/config.toml`. The cross-OS PDF comparison moved to rsm-141.15 (owner decision: not critical, pursued because GitHub runners are free for public repos). The owner declined a blocking link from rsm-141.3 to rsm-141.14. `tempfile` and `opener` are not added in the platform task (cargo 1.100 warns on unused dependencies); they arrive with their first users. `cargo-zigbuild` removed as a Windows option.
- 2026-11-18 (rsm-141.6, oracle analysis in history/): corrected the Typst Integration section to the 0.15.1 API (no `FontSearcher`; `typst-layout` is a direct dependency; `FileId` is built from a `RootedPath`). The World is per render and borrows the `StyleSource`; fonts are an `Arc<FontSet>`. Typst's own path rules reject `..` and backslashes before our `AccessDenied`, which now covers symlinks. Compile and export are separate steps on a 16 MiB thread. Atomic persist and `OutputLocked` moved to rsm-141.10, and font layering and role resolution moved to the new rsm-141.18. The `accent` input is deferred. New code `typst-warning`. Typst warns "unknown font family" for every missing preferred font in a font list, which rsm-141.7 must resolve.
- 2026-11-18 (rsm-141.6 review): independent code and test reviews (history/rsm-141.6_code-review.md, history/rsm-141.6_test-review.md) led to: a UTF-8 BOM stripped from sources (a BOM style rendered two pages instead of one); each style file read once per render and cached; export errors located through an owned world; a symlinked style directory refused up front (`StyleOutsideRoot`); non-regular files refused; `RenderOptions` is `#[non_exhaustive]` with `new`; `StyleFile { path, text }` replaces the positional tuple; `Typesetter::evict_cache` for `watch`; Typst's own line index for hint lines; one separator style in display paths. Open for rsm-141.10: an `Error` to `DiagnosticCode` mapping, the default styles-dir, and whether the default panic hook's stderr output is kept.
- 2026-11-18 (rsm-141.17): the gate now fails on an unused dependency: `[lints.cargo] unused_dependencies = "deny"` plus `cargo machete --with-metadata` in `just check` (see Dependency hygiene). Found while verifying: the cargo lint does not see a dependency that is also transitive, or any dev-dependency.
- 2026-11-18 (rsm-141.18, oracle analysis in history/): `FontSet` is an ordered `FontStore` plus a parallel `origins` vector, built by `FontSetBuilder` (all I/O in `build()`, none in Bundled mode). Corrections to the earlier plan: faces are keyed by canonical path (not raw path) and sorted per scan, not per directory, because typst-kit returns neither directory boundaries nor sorted output; "the earliest face wins" holds only among equal static faces; `application_dirs()` and `system()` builder methods were dropped because the mode decides. `FontsMode { Auto, Bundled }` lives in `model/options.rs`. Roles resolve on family presence (`FontResolution` is Preferred, Fallback, or Missing, with the origin and a `Metrics` flag) and convert to the three font diagnostics, which now have advice. The typst-kit embedded fonts stay as the last bundled layer. Open for rsm-141.10: do not print the metric-compatible note in human `build` output (the summary line already says "fallback"), and decide whether a stale `RESUME_FONT_PATH` entry should fail the build (it does now). Known gap: only the regular variant is resolved, so a `--font-path` holding only a regular Calibri makes bold text use regular glyphs with Carlito Bold's widths absent; a per-role variant list would fix it if wanted.
- 2026-11-18 (rsm-141.18 review): independent code and test reviews (history/rsm-141.18_code-review.md, history/rsm-141.18_test-review.md) led to: faces stored by canonical path (a relative `--font-path` failed to load after a `chdir`); a vanished face counts as absent in `resolve` (it was reported as Preferred); a scan panic is retried once and names its layer and directory (an Office-directory panic was labelled `font-path`); `FontResolution::diagnostic(mode)` gives a Bundled-mode hint that does not suggest what that mode ignores; `FontRole` is `#[non_exhaustive]` with `new()`, so later requirements (such as the variants a role needs) can be added without breaking rsm-141.7; `FontResolution::role()` and `origin()`; `FontOrigin` moved to `model::options` (re-exported from `typeset::fonts`) so `error.rs` does not depend on a pipeline stage; the builder's bundled-layer replacement is test-only, because replacing it would drop the fonts Typst needs for math and code; an Office directory with anything but plain files is never skipped. Layer tests that need exact contents moved next to the injected sources, so no test reads the machine's fonts (three integration tests failed on any machine with Libertinus, DejaVu, or New Computer Modern installed). Still open: only the Regular variant is resolved; a stale `RESUME_FONT_PATH` entry fails the build (the reviewer would warn and skip it instead); an explicit directory that yields no faces is silent; the system layer is sorted alphabetically by canonical path, which is deterministic but platform-asymmetric.
- 2026-11-18 (rsm-141.19): the `hard-break` Edit is withheld when deleting the marker would reshape the document (found by the rsm-141.4 property test: a lone `\` line, or `-\`, dropped the text after it). The check re-parses the three lines around the break and compares block and inline structure, so it covers every shape at once; the property test now also asserts that an Edit attached to a warning never adds an error, and its generator reaches lone-marker lines.
- 2026-11-18 (rsm-141.17 review): the unused-dependency gate gained `cargo check --lib --bins` (a dependency used only by `#[cfg(test)]` code passed clippy) and a guard test for the `regex` ignore; the docs now state what still passes (a name in a string literal or block comment, for an unused dev-dependency or a transitive one).
- 2026-11-18 (rsm-141.19 review): code and adversarial review found the three-line proof was not a proof (an emphasis opener or tag ending beyond the window, a reference definition elsewhere, a BOM, lone CR, and two Edits that form a table together all produced offered Edits that changed the model). The hard-break Edit is now also verified against the whole document and withdrawn together if anything differs; the property test asserts model equality, not just words and bold.
- 2026-11-19 (rsm-141.7.2, oracle analysis in history/): the pragmatic style and shared components. Corrections to the Styles section: the template uses Calibri for the name (no Georgia), so the pragmatic name is Carlito unless the `display` role is switched; line pitch is set from explicit text edges (`top-edge` 0.952em, `bottom-edge` -0.269em, `leading: 0pt`), because with Typst's default edges Calibri and Carlito differ vertically (168 lines drifted on a baseline) even though their widths match; `lang`, `region`, `hyphenate`, and `ligatures` are fixed so output does not depend on the machine. The header emits the name first so PDF text starts with it, and stacks the name above the sides when the name, the wider side, and a gap on each side of the name do not fit the line. Contact items are classified (email, phone, links, location) by heuristic because the model's `contact` is an untyped inline list. Pagination is `block(sticky: true)` and `block(breakable: false)`, proven by a break-position sweep with negative controls (`pragmatic_tests.rs`). Known gap: a section heading directly followed by a page-break block (an empty section) is left on the page; the parser already reports empty sections.
- 2026-11-19 (rsm-141.7.2 review): independent code and test reviews (history/rsm-141.7.2_code-review.md, history/rsm-141.7.2_test-review.md) led to: contact items trimmed only at their edges (the space in `LinkedIn: <link>` was lost); email and phone classification tightened (a link such as `[@ada](...)` is never an email); the name-overlap guard measures the wider side; unknown inline, block, and role-body kinds and resume schemas other than 1 stop the render; `default-headings` (section, role, era) replaces the per-hook parameters, with stickiness outside the hook; each bullet group is one list of unbreakable items; probes carry `period`, reject a title of the wrong type, and fail on an unplaced heading; the sweep generator varies role shape, finds the footer by content, requires every bullet token exactly once, and has one negative control per guarantee. Documented, not changed: a bullet taller than a page overflows; the name is not a tagged title.
- 2026-11-19 (rsm-141.7.3): visual review against the template PDF; the pragmatic theme now has 11 pt body, 14 pt headings, 12 pt role line and contact text, blue underlined links, 0.15 in bullet indent, 0.6 in side margins, and line pitch 1.32em (`leading: 0.1em`).
- 2026-11-19 (rsm-141.8): the mono style, built on the shared components with its own centered header and capitalized section title (`headings` and header in `style.typ`). Correction: the first version (4bfc77d, 9798d89, never merged) was written without the template file from this document's earlier row, which was wrong (left-aligned name, a colored accent, a regular-weight company). The Mono Engineer's template is the Pragmatic template's body with a centered name and one centered contact line (email, phone, location, links), black bold capitalized section titles over a black rule, and no color. The style now follows it and keeps the choices approved for pragmatic (company inline after the title, a `Summary` heading, blue links, the footer, bullet indent, spacing). The Alexander layout (Standard Resume) is not this template and is not modeled.
- 2026-11-19 (rsm-141.8 review): independent code and test reviews of the first version (history/rsm-141.8_code-review.md, history/rsm-141.8_test-review.md) and of the version that follows the template (history/rsm-141.8_code-review-2.md, history/rsm-141.8_test-review-2.md) led to: `role-grid` and `render-sections` shared in components.typ (mono had copied their bodies); the section title reusing `section-heading-content`; contact items unbreakable, so a long line wraps between items and never inside an address or link; the stale Mono row in the reference-template table corrected; and tests that pin each decision measured against the template: the size, weight, and face of the name, contact line, section titles, role line, body, and code; the black 1 pt rule under each section title and its extent; the 17 pt gap under the name (also under a name that wraps) and centering on Letter and A4; the order of the contact items and that none is dropped; a role without a title; capitals; black everywhere except blue underlined links; a negative control for the heading guarantees on mono; and a check that mono's copy of the values approved for pragmatic (margins, spacing, line pitch, rule, bullets, footer color) stays equal to pragmatic's. Every mutation the second test review found surviving is now killed (twenty-one tried). Not used by mono: the Arimo fallback. Documented, not changed: uppercase is real text in the PDF, `ß` capitalizes to `SS`, contact items are split only at ` · `, a wrapped contact line ends with its separator, and the inline-code size is 0.8 x 0.92 em because Typst applies 0.8em to raw text itself.
- 2026-11-18 (rsm-141.20, rsm-141.21): a nested list's span could start before its marker: with a tab after a blank line it began with the line ending and the tab (`nested-list quotes "\n\t1. "`), and with three spaces of indent it began with the extra space. Found by the `spans_are_in_bounds` property, which made `just check` fail at random. Root cause: the parser's range for a list inside another one starts where the parent item's content column ends. Of the constructs probed (block quotes, HTML, fenced code, tables, images, rules, and headings) only lists have it, and a top-level indented list had it too. The fix is one function in `tree.rs` that moves a list's start to its marker, so every finding that uses the list's range is repaired: `nested-list`, the numbered-list `unsupported-markdown`, `content-before-section` for a list, and the depth-cap `input-limit`.
- 2026-11-19 (rsm-141.9): the `experienced` style and TeX Gyre Pagella (2.501, GUST Font License) bundled, as the fourth fallback family (8 families, 33 faces; fonts/LICENSES.md states the three LPPL conditions). The gutter is part of the left page margin (`margin.left = 0.5in + gutter + gap`), so every block starts in the text column with no per-block indent, and the name, the contact lines and the footer reach back over it (`gutter-reach`, derived in the style from `gutter` and `page-margin` and read by `resume-document` with a default of 0pt, so the other styles are unchanged). The section label shares one grid row with the first unit of its section (a block, or for a role its heading and blurb), which keeps it at that line's height and off the foot of a page; the row is sticky when a role's heading has something after it, because a grid cell is one block and the heading's own stickiness does not reach out of it. `role-head` and `role-body` were split out of `render-role` for that. A resume that only parses to a labeled-bullet block gets the skills table; one unlabeled bullet in the block makes it a list. A Pagella fallback for Book Antiqua is `font-fallback-layout` (a warning), by the theme's `metric-compatible: false`.
- 2026-11-20 (rsm-141.22.1, oracle design in history/rsm-141.22.1_oracle-q1-design.md): tailoring annotations. The usage model is spec-driven: one baseline with annotations, one position spec per application, no clone-and-edit. Both placements (inline marks its own line; a comment alone on a line marks the line below), ambiguity as located errors (13 Format codes, one Content warning), document-unique group names, names shared with spec TOML, `#[serde(skip)]` fields on the model, and one shared `markdown::annotation` module that the letter parser reuses (rsm-5q1.2, rsm-vqr.1).
- 2026-11-21 (rsm-vqr.1, history/rsm-vqr.1_oracle-q1-model-shape.md and _q2-parser-integration-review.md): cover letters read annotations with the resume grammar and group checker. `LetterSource` holds `SourceItem`s (`Paragraph(Item)` or `Alternates(AlternateGroup)`), each `Item` with its `Annotations` and the comment's range; the code `letter-annotation-unsupported` is removed; `From<LetterSource> for SelectedLetter` became a checked `TryFrom` that refuses any annotation until selection (rsm-vqr.2) exists. Details in ref/Cover-Letters-Parser.md. The shared `annotation::group` now finds a repeated group name and variant name through hash sets, because the list search it used took about 1.5 s on a 1 MiB input (30,000 groups, or one group of 30,000 variants), which a letter could now also reach; regression tests are `many_alternate_groups_and_variants_parse_in_linear_time` and `many_groups_in_one_letter_parse_in_linear_time`.
- 2026-11-20 (rsm-141.24): the user guide, `docs/User-Guide.md`, and the tests that keep it true (`tests/guide/`, one test target with modules `extract`, `theme_keys`, `facts`, and `checks`). Each fact the code also knows is bound to the code by a check that compares sets and structure, never wording, and every check has a negative control that mutates the guide and asserts the problem is noticed. Markdown examples are fenced as `markdown resume expect=CODES` or `markdown in-section expect=CODES` and are parsed and linted like `resume check` does; the paragraph before an example that expects a code must link it. Theme keys are read from every built-in `theme.typ` by a small Rust reader of Typst dictionaries (checked against the `theme.` reads in the Typst sources), so a new key or a new style fails until the guide documents it. The commands and options are read from clap in process, with short flags, values, defaults, and repeatability checked per row. The diagnostic code tables are generated and committed between `guide:generated` markers; after a code changes, run `cargo test --test guide -- --ignored bless_guide_tables`. A section of the guide for an unbuilt feature carries `<!-- guide:planned BEAD witness=build:--OPTION -->`; the test fails when the witness exists, which tells whoever builds the feature to write the section. A positive check keeps the guide to synthetic names, example domains, and reserved phone numbers, and an ignored test reads the real baselines from `RESUME_BASELINES` to prove none of their private details appear. The guide owns the theme key reference and the component list; the style READMEs link to it. Found on the way: 11 of the 34 `--explain` examples did not reproduce their own code (fixed, with `tests/advice_examples_test.rs`), the architecture doc's component list named 7 of 27, and the pragmatic README's key list omitted `size.role` and `size.contact`.
- 2026-11-20 (rsm-141.24 review): independent reviews of the guide and its tests (history/rsm-141.24_code-review.md, history/rsm-141.24_test-review.md) found three false statements in the guide (what changes the page count, what the stranded-heading check catches, the font licenses), an option default that the checks could not see change, and controls that depended on the guide's exact words. All were fixed. The checks now state an option's default as ``default `X` `` and compare it with clap's; compare each short option both ways; read the Typst sources of every built-in style; scan `src/` for environment variables; compare link, example and shell-line counts with an independent count; and read a `md` or `Markdown` fence as Markdown. The negative controls run on small guides built from the code's facts (`tests/guide/synthetic.rs`), so rewording the guide cannot break them. Planned sections name `command:NAME` or `option:--NAME` witnesses, and a section needs no test edit when its feature lands. The unused `color.accent` key was removed from `experienced`.
- 2026-11-20 (rsm-141.24, rsm-141.12): `watch` and `export` landed while the guide was in review, and the guide's planned-section check failed as designed. The guide now has `### resume watch` and `### resume export` sections with option tables checked against the CLI, and the planned markers are gone. The check for a command's own section did not need a test change.
- 2026-11-21 (rsm-141.22.2, oracle design in history/rsm-141.22.2_oracle-analysis-and-plan.md): selection. Leaves are judged, containers are kept only when something under them is; a draft container drops its subtree; a draft `default` falls back to the first ready wording; a pick of a draft is an error; the kept wording carries its group's tags so selection is idempotent; `Session::assess` takes a `Selection` and compiles the tailored resume, so a plain build now leaves `@draft` items out.
- 2026-11-21 (rsm-141.22.3, design in history/rsm-141.22.3_oracle-design-*.md): tailoring from the command line. Shared `SelectArgs` on build, watch, doctor, check, export; `unknown-tag` and `unknown-wording` warnings; a silent-by-default selection summary that counts routine events and lists the surprises with their source lines (measured on a merged baseline of the two real drafts: 9 and 14 lines), the same facts as a JSON `selection` record; the core-only page count; `resume tags`. Report nodes carry their index in `Resume::nodes()`, mapped to a source line by `Parsed::node_lines`.
- 2026-11-21 (rsm-141.23.1): position specs and profiles (`src/spec/`). Decisions from the owner after analysis (history/rsm-141.23.1_design-note.md): `extends` is top-level, not in `[build]`; profiles live in `--profiles DIR` or `profiles/` beside the spec's directory, with no second fallback; `content` has no `.md` rule (the summary prints the resolved path instead); a spec whose `extends` order contradicts another file's is an error. `paper` is spelled `letter` or `a4`, as `--paper` spells it. `drafts` is never a file key. The ordering rule is a topological sort over two kinds of fact, not a check of a depth-first walk, after a test against an exhaustive search showed the check refused orders that exist. The cover-letter documents' examples were edited to match (`extends` moved, `paper` respelled).

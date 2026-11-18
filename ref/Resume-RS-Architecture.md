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
| Mono Engineer's (docx) / Standard Resume **Alexander** | Top-down. The name is on the left, with the contact details on one line under it. Section headings are uppercase. The title or the company gets the emphasis. Calibri 11 pt. | ★★★★★ |
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

The model reaches Typst as JSON in a virtual file, read with `json("/resume.json")`, rather than as typed `sys.inputs` values. That has two benefits. Styles can be developed outside the CLI: `resume export` writes `resume.json` next to a style, and the tinymist editor extension or `typst watch` gives a live preview. And the style contract is a documented JSON schema rather than Rust types.

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
      code.rs            # DiagnosticCode (strum), Category, Advice catalog
      render.rs          # codespan-reporting human output; JSON lines
    markdown/
      mod.rs             # parse(path, text) -> Parsed { resume, diagnostics }
      blocks.rs          # block-level conventions (name, contact, roles, eras, blurbs)
      inline.rs          # inline runs (text, strong, emph, link, code)
      period.rs          # Period parsing and validation
      lint.rs            # format lints (nesting, chronology, mid-sentence bold, images)
    doctor/
      mod.rs             # doctor(&Parsed, &Rendered, &Env) -> DoctorReport
      content.rs         # book-guided content lints (quantification, weak openers, first person, ...)
      setup.rs           # environment checks (style resolution, fonts, output dir)
      fix.rs             # mechanical fixes -> Vec<Edit>; unified diff via `similar`
    typeset/
      mod.rs             # render(&Resume, &Style, &RenderOptions) -> Rendered
      world.rs           # ResumeWorld: impl typst::World
      fonts.rs           # layered font book: bundled fallbacks + system/app/--font-path; role resolution
      pdf.rs             # deterministic PdfOptions
    style.rs             # StyleSource (built-in or directory), registry
    layout.rs            # LayoutReport from PagedDocument (pages, role placement)
    watch.rs             # notify-based rebuild loop
  styles/                # built-in styles, embedded with include_dir
    lib/components.typ   # shared rendering of inlines, bullets, role lines, eras
    pragmatic/{style.typ, theme.typ, README.md}
    mono/{style.typ, theme.typ, README.md}
    experienced/{style.typ, theme.typ, README.md}
  fonts/                 # bundled OFL/GUST-licensed fonts + LICENSES.md
  tests/
    fixtures/            # synthetic resumes covering every convention and error
    parse_test.rs  render_test.rs  doctor_test.rs  cli_test.rs
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

The parser uses `pulldown-cmark` 0.13 with `into_offset_iter()`, so every event carries a byte range for diagnostics. Smart punctuation is enabled to match pandoc's `smart` extension. HTML comments (`Event::Html` starting with `<!--`) are skipped. Any other raw HTML is an error, except the exact page-break marker.

| Convention | Rule | Diagnostic when violated |
|---|---|---|
| Name | Exactly one `# ` heading, first in the document | Error if missing or repeated |
| Contact | The first paragraph after the name | Warning if missing. `too-many-contact-items` warning above 4 ` · `-separated items (book: four contact details at most). Links must parse as URLs (`invalid-link`) with an `http(s)://` or `mailto:` scheme (`unsupported-link`). |
| Section | `## Title`. Everything until the next `##` belongs to it. | Error for content before the first `##` other than the name and contact |
| Role | `### Company · Title · Period` or `### Company · Period`. A period is recognized by containing a four-digit year. | Error with a hint naming the expected shape and the separator ` · ` (U+00B7 with spaces). A separator typed as `-`, `\|`, or `•` gets a "did you mean ` · `?" hint. |
| Company description | The first paragraph directly after a role line | None. This is the documented behavior. |
| Era | A paragraph that is only `**Name · Period**`, inside a role | A bold paragraph that has ` · ` but no year gets an error. An era outside a role gets an error. |
| Bullet label | A list item starting with `**Label:**` | None |
| Page break | `<div class="page-break"></div>` on its own lines, kept for compatibility | None |
| Period | Start: `Mon YYYY`, `Month YYYY`, `YYYY`, or a season plus `YYYY`. Separator: ` – ` (en dash) or ` - `. End: the same forms, or `Present` (also accepts `Current` and `Now`, with a fixable warning). | Error on an unparseable year, a year outside 1950–2100, or an end before the start. Fixable warning for a hyphen instead of an en dash. |

Format lints run during parsing. They are warnings about how the Markdown is written:

- Nested lists. The book says to avoid sub-bullets.
- Roles that aren't in reverse chronological order within a section, compared with `PartialDate::sort_key` on the end date, with Present treated as the most recent.
- Bold inside a sentence (a `Strong` that isn't a bullet label or an era). The book says to avoid bolding in mid-sentence.
- Images. The book says no photos.

Content advice (bullet length, quantification, weak openers, and so on) belongs to `doctor`. See the Doctor section.

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
    // Content (doctor)
    LongBullet, UnquantifiedBullet, WeakOpener, FirstPerson, SloppyPhrase, LongSummary,
    EducationNotLast, OldRoleDetail,
    // Layout (render)
    PageLimitExceeded, HeadingNearPageBottom, SparseLastPage, FontFallbackMetric, FontFallbackLayout, FontMissing,
    // Setup (doctor)
    UnknownStyle, StyleMissingMain, OutputNotWritable, TypstError,
}

impl DiagnosticCode {
    pub fn category(self) -> Category;
    pub fn advice(self) -> &'static Advice;   // static catalog, one entry per code
}

pub struct Advice {
    pub summary: &'static str,          // one line, shown with every finding
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

Advice lives in a `match` in `code.rs`, not in strum attributes, because multi-line rationale and before/after examples read poorly inside attributes.

Markdown and Typst diagnostics share one type and are rendered with `codespan-reporting` 0.13, the same renderer Typst's own CLI uses. Typst diagnostics are converted by resolving each span's `FileId` through `ResumeWorld` back to the style file path and byte range. `--format json` emits diagnostics as JSON lines for editors and scripts.

### Typst Integration (`src/typeset/`)

```rust
pub struct ResumeWorld {
    library: LazyHash<Library>,       // Library::builder().with_inputs(inputs).build()
    book: LazyHash<FontBook>,
    fonts: Vec<FontSlot>,             // bundled fallbacks, plus system/app/--font-path fonts in --fonts auto
    main: FileId,                     // /<style>/style.typ
    resume_json: Bytes,               // served at /resume.json
    style_files: StyleFiles,          // built-in (include_dir) or a directory on disk
    sources: Mutex<HashMap<FileId, Source>>,  // cleared per compile; cached for watch
}
impl typst::World for ResumeWorld { /* library, book, main, source, file, font, today */ }
```

- **Pinned dependencies:** `typst = "=0.15.1"`, `typst-pdf = "=0.15.1"`, and `typst-kit = "=0.15.1"` with the `embedded-fonts` feature, as a fallback set. `typst-as-lib` is not used, because its README says its API is unstable and it adds a second layer that would lag Typst releases.
- **Virtual root:** the style directory's parent, so a style can import `/lib/components.typ`. Paths outside the root resolve to `FileError::AccessDenied`.
- **`today()` returns `None`,** so a style can't make output depend on the date.
- **`sys.inputs` carries render options only:** `paper` (`"us-letter"` or `"a4"`) and an optional `accent` color override. Content always comes from `/resume.json`.
- **PDF export:** `PdfOptions { ident: Smart::Custom(<file stem>), creator: Smart::Custom(Some("resume-rs <version>")), timestamp: None, tagged: true, ..Default::default() }`. Tagged output is kept on for accessible reading order.

### Styles

Each style is a directory:

```
<style>/
  style.typ   # structure: reads /resume.json, lays out sections, calls components
  theme.typ   # theme: a `theme` dictionary of fonts, sizes, colors, spacing, page margins
  README.md   # what the layout is for, its source template, and the knobs in theme.typ
```

- **Shared components** (`styles/lib/components.typ`): `render-inlines`, `bullets`, `role-line`, `era-line`, `blurb`, `section-heading`, and `footer`. Role and era headings are wrapped in `block(sticky: true)`, so they always move to the next page with their first bullet. Each bullet is `block(breakable: false)`, so it never splits across pages.
- **Layout probes:** `components.typ` emits labeled `metadata` with the role index for every role line and era line. `layout.rs` queries those labels on the `PagedDocument` introspector to find the page and position of each one.
- **Built-in styles** are embedded with `include_dir`. `--styles-dir <dir>` uses styles from disk instead, which is how customized styles are built. `resume export-style <name> <dir>` copies a built-in style plus `lib/` so you can start from it.

| Style | Modeled on | Structure | Default theme |
|---|---|---|---|
| `pragmatic` | Pragmatic Engineer's / Parker | Three-part header grid (location and links / name / phone and email). Ruled section headings. Role line: **Title** · Company with right-aligned dates, then the description line. | Carlito (metric-compatible with Calibri) 10.5 pt, name in Gelasio (metric-compatible with Georgia), accent `#1155cc`, 0.5" margins |
| `mono` | Mono Engineer's / Alexander | Left-aligned name with one contact line, uppercase section headings, role line with the title emphasized | Carlito 10.5 pt, near-black text with one accent, 0.5" margins |
| `experienced` | Experienced Engineer's / Venables | Two-zone grid: section labels in a 1.35" left gutter, content on the right. Role line: **Company**, Title with right-aligned dates. Skills render as a label and value table. | Carlito 10.5 pt for the body, section labels in TeX Gyre Pagella (a free stand-in for Book Antiqua), accent `#cc0000` |

All three put the name and "Page N of M" in the footer, which the current tool already does.

**Bundled fonts** (`fonts/`, with `LICENSES.md`): only the free fallbacks (Carlito, Gelasio, TeX Gyre Pagella, Arimo), in regular, bold, italic, and bold italic. The template fonts (Calibri, Georgia, Book Antiqua, Arial) are the preferred fonts in each theme and are used when installed. See Fonts: Preferred Originals with Bundled Fallbacks.

### Fonts: Preferred Originals with Bundled Fallbacks (`src/typeset/fonts.rs`)

Each theme names, for every typographic role, the **preferred** font (the template's original, which may be non-free) and a **bundled fallback** that is free and, where one exists, has the same character widths. Non-free fonts are never bundled. They're used only when they're already installed or found on a font path. The bundled fallbacks are always present, so a build never fails for lack of a font.

**Measured comparison** (spike rsm-141.1). Advance widths were read directly from the font files for 87 glyphs (letters, digits, punctuation, `–`, `—`, `·`, `’`), and line widths were measured for resume sample lines at 11 pt:

| Role | Preferred (template) | Bundled fallback | Per-glyph width difference | Line width difference | License of fallback |
|---|---|---|---|---|---|
| Body (all styles) | Calibri | **Carlito** | 0.00% for all 87 glyphs, regular, bold, and italic | 0.00% | OFL 1.1 |
| Name and headings (`pragmatic`) | Georgia | **Gelasio** | 0.00% | 0.00% | OFL 1.1 |
| Section labels (`experienced`) | Book Antiqua | **TeX Gyre Pagella** (chosen; P052 rejected) | Pagella: mean 2.4%, wider brackets, narrower `#`, `/`, `·`. P052: mean 0.5%, only `#` −17.5% and `·` −24.9% differ. | Pagella −1.3% to +0.7%. P052 −1.6% to −0.1%. | Pagella: GUST Font License. P052: AGPL 3.0 with font exception. |
| Occasional runs (`mono`) | Arial | **Arimo** | 0.00% except `·` +19.9% | 0% to +0.3% | OFL 1.1 |

Cambria was dropped. Caladea doesn't match the installed Cambria 6.90 (lines are 4–8% narrower), and none of the templates use Cambria visibly. Lato (Carlito's parent) is 8–18% wider than Calibri, so it's useful only to judge feel. All four originals allow embedding: Calibri and Book Antiqua have `fsType` set to editable embedding, Georgia and Arial to installable.

**Fallback mechanism.**
1. **Theme declares stacks.** For example: `fonts: (body: (preferred: "Calibri", fallback: "Carlito", metric-compatible: true), ...)`. `style.typ` passes `(preferred, fallback)` to `text(font: ..)`. Typst uses the first family present in the font book, and falls back per glyph if one is missing.
2. **Font sources are layered** into one `FontBook`:
   - bundled fallbacks (always)
   - in the default `--fonts auto` mode, also:
     - system fonts via `typst-kit` `scan-fonts`: macOS `/System/Library/Fonts`, `/Library/Fonts`, and `~/Library/Fonts`; Windows `C:\Windows\Fonts` and `%LOCALAPPDATA%\Microsoft\Windows\Fonts`
     - known application font directories: on macOS, `/Applications/Microsoft {Word,Excel,PowerPoint,Outlook}.app/Contents/Resources/DFonts`, where Office keeps Calibri and Cambria
     - every `--font-path <DIR>` (repeatable) and every entry in `RESUME_FONT_PATH`
   - `--fonts bundled` uses only the bundled fallbacks.
   - TrueType and OpenType collections (`.ttc`) are loaded natively.
3. **Resolution is reported.** `components.typ` emits `metadata(theme.fonts) <resume-fonts>`. Before compiling, the CLI resolves each role against the font book and prints it in the build summary, for example `fonts: body Calibri (system) · headings Georgia (system) · labels TeX Gyre Pagella (fallback for Book Antiqua)`.
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
| Windows, x64 | `x86_64-pc-windows-msvc` | CI on `windows-latest`, for release artifacts and tests |
| Windows, x64 (local cross-check) | `x86_64-pc-windows-gnu` | Local cross-build from macOS with the installed `x86_64-w64-mingw32-gcc` (or `cargo zigbuild`). It catches compile and link problems quickly. Statically linked `libgcc` and `winpthread` make `resume.exe` self-contained. |

A cross-built Windows binary can't run its tests on macOS, so behavior on Windows is verified in CI.

**CI (GitHub Actions)** runs a matrix of `macos-latest` and `windows-latest`. Each job runs `just check`, builds release artifacts (a universal2 tarball for macOS, a zip with `resume.exe` for Windows), and builds the full fixture with `--fonts bundled`. A final job compares the PDF hashes from both jobs, so cross-OS determinism is tested, not assumed.

**Portability rules,** which apply to all code:
- **Paths:** `PathBuf` and `&Path` everywhere. Typst `VirtualPath`s are built from path components, never by string concatenation with `/`. Paths in diagnostics are shown relative to the current directory.
- **Line endings:** the parser accepts LF and CRLF, spans are byte offsets into the original text, and `doctor --fix` edits keep the file's existing line endings. CRLF fixtures are marked `-text` in `.gitattributes`.
- **Replacing output files:** write to a temp file in the same directory and persist it with `tempfile` 3, which replaces atomically on both platforms. On Windows a PDF that is open in a viewer can lock the file. That case becomes `OutputLocked { path }`, with the hint "close the PDF viewer or use `-o`".
- **Opening files:** `watch --open` uses the `opener` 0.9 crate, which maps to `open` on macOS and `ShellExecuteW` on Windows.
- **Terminal output:** codespan-reporting's `termcolor` handles Windows consoles. `--color auto|always|never` and `NO_COLOR` are respected, and summary markers are ASCII (`ok`, `error`) so legacy Windows consoles render them.
- **File watching:** `notify` 8 uses its default backend on each platform (FSEvents on macOS, ReadDirectoryChangesW on Windows).

### Layout Report (`src/layout.rs`)

```rust
pub struct LayoutReport {
    pub pages: usize,
    pub roles: Vec<RolePlacement>,
    pub diagnostics: Vec<Diagnostic>,
}
pub struct RolePlacement { pub company: Company, pub title: Option<Title>, pub first_page: usize, pub last_page: usize }
```

Checks, run on every render:
- **`page-limit-exceeded`** (error, only with `--max-pages`): names the page count and the limit.
- **`heading-near-page-bottom`** (warning): a role or era heading in the bottom 8% of a page. Sticky blocks make this rare, so it acts as a guard if a style is edited.
- **`sparse-last-page`** (warning): the last page is less than 15% full ("trim or tighten to save a page").
- **`font-fallback-metric`** (note), **`font-fallback-layout`** (warning), **`font-missing`** (error): see the Fonts section. Each one names the role, the requested font, and the font that was used.

### Doctor (`src/doctor/`)

`resume doctor` is the advisor. It runs everything `build` runs (parse, render, layout report) without writing a PDF. It adds content advice and setup checks, and groups every finding by category, with the reason and the fix:

```
resume doctor drafts/Resume-Org-Leader.md --style mono --max-pages 3

Format   2 findings
  warning[period-hyphen]: period uses a hyphen instead of an en dash
    ┌─ drafts/Resume-Org-Leader.md:41:56
    │  ### Globex · Engineering Director, Platform · 2015 - 2017
    │                                                ^^^^^^^^^^^ fixable
    = fix: write `2015 – 2017` (en dash: Option+hyphen)
Content  5 findings
  warning[unquantified-bullet]: bullet has no number
    ...
    = why: The book: quantify impact; "most resumes do not contain numbers"
    = fix: add a measurable result (team size, %, $, users, time saved)
Layout   1 finding
  error[page-limit-exceeded]: 4 pages, limit is 3
    = roles: Acme p1–2 · Globex p2–3 · Initech p3 · ... (page map)
Setup    ok (style mono, 3 fonts embedded, out/ writable)

1 error, 6 warnings; 3 fixable with `resume doctor --fix`
```

**Content lints** (`doctor/content.rs`). All are warnings with book-cited advice, and each is deterministic and computed from the document alone:

| Code | Rule |
|---|---|
| `long-bullet` | A bullet over 320 characters, roughly three or more lines. The book advises two lines or fewer. |
| `unquantified-bullet` | An experience bullet with no digit. The book: use numbers to quantify impact. |
| `weak-opener` | A bullet that starts with "Responsible for", "Worked on", "Helped", "Participated in", "Involved in", or "Assisted". The book: active verbs, your contribution. |
| `first-person` | "I", "we", "our team", or "my team" in a bullet. The book: drop "I", avoid "we". |
| `sloppy-phrase` | "etc.", "and so on", "various", "stuff". The book: sloppy phrases. |
| `long-summary` | A summary section over 120 words. The book: keep the summary short and specific. |
| `education-not-last` | An Education section that isn't last. The book: move Education to the end. |
| `old-role-detail` | A role that ended 15 or more years before the latest role's end, with more bullets than the most recent role. The book: older roles take proportionally less space. "Latest" comes from the document, not today's date. |

**Setup checks** (`doctor/setup.rs`):
- the style resolves, and its directory contains `style.typ`
- each font role resolves, showing whether the preferred font or the fallback was used and where it came from (system, app directory, `--font-path`, bundled), plus any metric difference
- the output directory exists or can be created, and is writable

**Fixes** (`doctor/fix.rs`). A diagnostic carries an `Edit` only when the fix is mechanical and preserves meaning:
- a role or era separator typed as `-`, `|`, or `•` between parts becomes ` · `
- a period hyphen becomes an en dash
- "Current" or "Now" as a period end becomes "Present"
- trailing whitespace on role and era lines is removed

`doctor --fix` applies all edits in one pass (sorted, non-overlapping, applied from the end), writes the file atomically, prints a unified diff (`similar` 3), and re-runs the checks. Content advice is never auto-fixed. `doctor --explain <code>` prints the full advice entry, like `rustc --explain`.

### CLI (`src/cli.rs`)

```
resume [--dry-run] [--format human|json] [--color auto|always|never] <COMMAND>

  build  <FILE>... [-s|--style <NAME>] [-o|--out <DIR>] [--paper letter|a4]
                   [--styles-dir <DIR>] [--max-pages <N>] [--fonts auto|bundled] [--font-path <DIR>]...
  doctor <FILE>... [same options as build] [--fix] [--explain <CODE>]
  check  <FILE>... [same options as build]   # alias: read-only `doctor` (no --fix, no --explain)
  watch  <FILE>    [-s <NAME>] [-o <DIR>] [--debounce <DURATION>] [--open]
  styles           [--styles-dir <DIR>]
  export-style <NAME> <DIR>
  export <FILE> <DIR> [-s <NAME>]      # resume.json + style, for tinymist or typst watch
```

**`--dry-run`** is global and applies to every command that writes:

| Command | With `--dry-run` |
|---|---|
| `build` | Parses, compiles, and runs the layout checks, then reports `would write out/X.pdf (3 pages, overwrites existing)`. Writes nothing. |
| `doctor --fix` | Prints the unified diff of the fixes and leaves the file unchanged |
| `export-style`, `export` | Lists the files that would be created or overwritten |
| `watch` | Rejected at argument parsing (clap `conflicts_with`), since watching without writing has no purpose |
| `styles`, `check`, `doctor` without `--fix` | No effect; they don't write |

`check` is a read-only alias of `doctor`. It runs the same assessment and prints the same report, but it doesn't accept `--fix` or `--explain`, so `check` in a script or a pre-commit hook can never modify files. `doctor` is the command for fixing (`--fix`) and learning (`--explain`). A no-write build is `build --dry-run`.

- `build` prints one summary line per file, for example `✓ out/Resume-Org-Leader.pdf  experienced  3 pages  1 warning  built in 84ms`, followed by the diagnostics.
- **Exit codes:** 0 on success (warnings allowed), 1 on any error diagnostic or failed build, 2 on a usage error (from clap).
- **Defaults:** `--style pragmatic`, `--out out`, `--paper letter`, `--fonts auto`, `--color auto`, `--debounce 150ms`.

### Watch (`src/watch.rs`)

`notify` 8 watches the Markdown file and, if given, `--styles-dir`. Events are debounced for `--debounce`, parsed with `humantime`. Each change re-parses and recompiles, reusing the `ResumeWorld` font book. Typst's incremental compilation (`comemo`) makes rebuilds after the first fast. Every build prints its summary and diagnostics, and the PDF is replaced atomically by writing a temp file and renaming it, so a viewer never reads a half-written file. `--open` opens the PDF once with the `opener` crate (macOS and Windows). Skim on macOS and SumatraPDF on Windows reload automatically; Acrobat locks the file on Windows, which `watch` reports as `OutputLocked`. Live preview inside an editor is covered by `resume export` plus tinymist.

### Errors (`src/error.rs`)

`snafu` errors, following the guidelines, for operational failures: reading input, writing output, an output file locked by another program (`OutputLocked`, common on Windows), an unknown style, a style directory missing `style.typ`, PDF export failure, and watcher failure. Content problems are `Diagnostic`s, not `Error`s, so one run can report all of them, and `doctor` can advise on each. `main` prints the errors and diagnostics and picks the exit code.

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

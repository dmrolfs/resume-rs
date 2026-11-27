<!-- ABOUTME: The user guide for resume-rs: writing a resume, checking it, fonts, styles, and every command. -->
<!-- ABOUTME: Tests in tests/guide keep it true: they compare its tables, examples, and headings with the code. -->

# resume-rs User Guide

`resume` turns a resume written in Markdown into a print-quality PDF. You write the content once. A **style** decides how it looks. A built-in checker, `doctor`, tells you what is wrong with the resume and why it matters, and fixes what is safe to fix.

This guide is for the person using the tool. It says what the tool does and how to ask for it. Why it is built that way, and how, is in the [architecture notes](../ref/Resume-RS-Architecture.md), which are for people changing the tool.

The advice behind the checks comes from the book *The Tech Resume Inside Out*, which this guide calls "the book" after this.

How to read it: start with the [quick start](#quick-start), then [writing your resume](#writing-your-resume). Come back to [the commands](#commands) and [the styles](#styles) when you need them. A line like `period-hyphen` in this guide is the name of a finding; it links to [the list of every finding](#appendix-diagnostic-codes), and `resume doctor --explain period-hyphen` explains it in your terminal.

Two conventions help when you read. A section marked **Planned, not available yet** describes work that has not been done; nothing in it works today. And every example in a `markdown` block on this page is checked by the project's tests: it produces exactly the findings the text says it does.

## Quick start

This guide assumes the `resume` program is on your path; the [README](../README.md) says how to get it. Save this as `Resume.md`. It is a whole resume, small enough to read at a glance, and it builds as written.

```markdown resume expect=none
# Ada Lovelace

London, UK · ada@example.com

## Summary

Engineer who built the first published program and led a team of 4 through 3 releases.

## Experience

### Analytical Engines Ltd · Principal Engineer · 2017 – Present

Builds mechanical computers for scientific customers.

- Cut the cost of a weekly run from $39K to $29K by caching 80% of the tables.
- Led a team of 12 to deliver the engine in 18 months.
```

Then, in the directory that holds `Resume.md`:

```sh
resume check Resume.md
resume doctor Resume.md --fix --dry-run
resume doctor Resume.md --fix
resume build Resume.md
resume build Resume.md --style mono --paper a4
resume build Resume.md --fonts bundled --max-pages 2
resume export-style pragmatic my-styles
resume build Resume.md --styles-dir my-styles --style pragmatic
resume doctor --explain period-hyphen
```

What each line does, in order:

1. `check` reads the resume and reports what is wrong. It never changes a file, so it is safe in a script or a hook.
2. `doctor --fix --dry-run` shows the changes `doctor` would make, as a diff, and changes nothing.
3. `doctor --fix` makes them.
4. `build` writes `out/Resume.pdf`.
5. `--style mono --paper a4` builds the same resume in another built-in style, on A4 paper.
6. `--fonts bundled --max-pages 2` uses only the fonts inside the program, so the PDF does not depend on the machine, and fails if the resume needs more than two pages.
7. `export-style` copies a built-in style into `my-styles` so you can edit it.
8. `--styles-dir my-styles` names a directory of styles, not one style; `--style pragmatic` then picks your copy from it.
9. `doctor --explain` explains one finding, with an example.

Something went wrong? See [troubleshooting](#troubleshooting).

## Writing your resume

A resume is a Markdown file with a small number of conventions. The program reads these conventions, not general Markdown: anything it cannot represent is reported, never dropped silently.

Here is a whole resume that uses every convention. Its page break forces a second page, so a build of it warns that the last page is nearly empty; that is the example's doing, not a problem with the convention. The comment lines explain each part. Comments on their own lines are ignored when the PDF is built, so they are safe to leave in.

```markdown resume expect=none
<!-- The name: exactly one `#` heading. It is the title of the PDF and the text of the footer. -->
# Ada Lovelace

<!-- The contact line: the paragraph right after the name. Separate the items with ` · `. Keep it short. -->
London, UK · +44 20 7946 0000 · ada@example.com · [GitHub](https://github.com/ada)

<!-- A section: `##`. A resume is a list of sections. -->
## Summary

<!-- A paragraph is a paragraph. A bullet list is a list. -->
Engineer who built the first published program and led teams of up to 12.

- Led a team of 12 to deliver the analytical engine in 18 months.
- Won adoption of *event sourcing* and `CQRS` across 5 organizations.

## Skills

<!-- A bullet that starts with **Label:** (the colon inside the bold) is a labeled bullet. -->
- **Languages:** Rust, Java, Scala
- **Cloud and infrastructure:** AWS (EKS, EC2), Kubernetes

## Experience

<!-- A role: `###` then `Company · Title · Period`. The first paragraph after it is its description. -->
### Analytical Engines Ltd · Principal Engineer · Aug 2017 – Present

Builds mechanical computers for scientific customers.

<!-- An era: a paragraph that is only bold text, `**Name · Period**`. It groups the bullets after it. -->
**Programming languages · 2026 – Present**

- Designed the first loop construct in 3 weeks.
- Wrote the first published program, 25 lines long.

**Early machines · Spring 2020 – Winter 2021**

- Prototyped the difference engine with 8 columns of gears.

<!-- A role without a title: `Company · Period`. -->
### Babbage & Co · 2012 – 2017

- Calculated 40 tables of logarithms, checked by 2 reviewers.

<!-- A page break: this exact line, with a blank line before and after. -->
<div class="page-break"></div>

## Education

### University of London · 2004 – 2008

- Mathematics, first class honours.
```

### The parts

**The name** is the one first-level heading, `# Your Name`. A second `#` heading, or none, is an error: see [`missing-name`](#code-missing-name) and [`duplicate-name`](#code-duplicate-name).

**The contact line** is the paragraph right after the name. Separate the items with ` · ` (a middle dot with a space on each side; Option+Shift+9 on a Mac, Alt+0183 on Windows, or copy it from an example here). The book recommends a short line, and `doctor` warns when it is too long with [`too-many-contact-items`](#code-too-many-contact-items). Links need an `http`, `https`, or `mailto` address.

**Sections** are `## Title`. Everything up to the next `##` belongs to the section. Only the name and the contact line may come before the first section.

**Roles** are `### Company · Title · Period`, or `### Company · Period` when there is no title. Each part needs text, and the separator is exactly ` · `. A period is recognized by a four-digit year in it.

**The description** is the first paragraph after a role line, if it is not an era. It is the one line that says what the company does. (The theme keys call it the blurb.)

**Eras** show progress inside one employer. An era is a paragraph that is only bold text: `**Name · Period**`. Bullets after it belong to it until the next era, role, or section.

**Bullets** are Markdown list items. One level only: nested bullets are flattened, with a warning. A bullet that starts with `**Label:**`, the colon inside the bold, is a labeled bullet, and the label is set in bold.

**Emphasis, code, and links** work inside text: `*italic*`, `**bold**`, `` `code` ``, and `[text](https://example.com)`.

**Page breaks** are the line `<div class="page-break"></div>`, on its own, with a blank line before and after. A page break may sit between roles or at the end of a section, never inside a role.

**Comments** are `<!-- like this -->`, on their own lines. They are dropped. A comment that never closes, or text after `-->` on the same line, is an error, because it would hide your content.

### Periods

A period is a start, ` – ` (an en dash with a space on each side), and an end. A start is `2017`, `Aug 2017`, `August 2017`, or a season such as `Spring 2020`. An end is the same, or `Present`. A single date is fine. Use the en dash: a hyphen gets a warning that `doctor --fix` repairs.

### Right and wrong

The same resume, written wrongly, shows what the checker says. Each example below is placed after a name, a contact line, and a `## Experience` heading, so you can see just the part that matters.

A role whose parts are separated by `|` instead of ` · ` is [`role-separator`](#code-role-separator). The program sees what you meant and says so:

```markdown in-section expect=role-separator
### Acme | Engineer | 2015 – 2017

- Cut build time from 40 minutes to 9.
```

Dates joined by a hyphen (or an em dash) are [`period-hyphen`](#code-period-hyphen), a warning with a mechanical fix:

```markdown in-section expect=period-hyphen
### Acme · Engineer · 2015 - 2017

- Cut build time from 40 minutes to 9.
```

A bullet with no number in it is [`unquantified-bullet`](#code-unquantified-bullet). The book says to measure what you did:

```markdown in-section expect=unquantified-bullet
### Acme · Engineer · 2015 – 2017

- Improved the build system.
```

A bold line that looks like an era but has prose after it is [`era-shape`](#code-era-shape), and because the bold now sits inside a sentence it is also [`mid-sentence-bold`](#code-mid-sentence-bold). Make the whole paragraph the era and put the rest in its own paragraph:

```markdown in-section expect=era-shape,mid-sentence-bold
### Acme · Engineer · 2015 – 2017

**AI platforms · 2016 – 2017** Led the platform team.

- Cut build time from 40 minutes to 9.
```

A label with its colon outside the bold is [`mid-sentence-bold`](#code-mid-sentence-bold), a warning `doctor --fix` repairs. This example sits directly under the section heading, not in a role; inside a role, the bullet would also be reported as unquantified:

```markdown in-section expect=mid-sentence-bold
- **Languages**: Rust, Java, Scala
```

### What is not supported

The program tells you rather than guessing. These are errors, not silent omissions: block quotes, code blocks, tables, headings below `###`, numbered lists, and a bullet with more than one paragraph. Images are dropped with a warning, because the book advises against photos. An input larger than the size limit is rejected with [`input-limit`](#code-input-limit).

## Checking your resume

`resume check` and `resume doctor` run the same assessment and print the same report. It groups every finding by what kind it is:

- **Format**: how the Markdown is written. The parser reports these, and a Format error means there is no resume to build.
- **Content**: what the resume says. Bullet length, numbers, weak openers, and so on.
- **Layout**: how the rendered document comes out: page count, headings stranded at a page bottom, fonts that stand in for the template's.
- **Setup**: whether the run can work: the style, the fonts, the output directory.

Each finding has a **severity**. An `error` stops a build and makes the command exit 1. A `warning` is advice: fix it or accept it. A `note` is information.

```sh
resume check Resume.md
resume doctor Resume.md --style mono --max-pages 3
resume check Resume.md --format json
```

**`doctor --fix`** applies the changes that are mechanical and keep the meaning, such as replacing a hyphen with an en dash or moving a colon inside a bold label. It never changes what the resume says: before writing, it checks that the fixed file reads as the same resume. Add `--dry-run` to see the diff first. A finding that can be fixed this way is marked `fixable` in the report.

**`doctor --explain CODE`** prints what a finding means, the reason for it, how to fix it, and usually a before-and-after example. The code is written exactly as the report shows it, in lowercase. Every example it prints is one that really produces its code; the one for `font-fallback-metric` needs `--format json`, because that note is not shown in a human report.

**Where output goes.** A human report is written to standard error, so `resume check Resume.md > report.txt` leaves the file empty. With `--format json`, each finding is one JSON object per line on standard output, for scripts. A JSON finding has the fields `type` (`diagnostic`; a tailored build also writes one `selection` record, see [Annotations](#annotations), and a build from a spec one `spec` record, see [Profiles and position specs](#profiles-and-position-specs)), `severity`, `code`, `category`, `message`, `file`, `line`, `column`, `end_line`, `end_column`, `hints`, and `fixable`. Operational problems (a file that cannot be read, an unknown style) are still plain lines on standard error with exit code 1, so standard output stays machine-readable.

**Exit codes.** 0 means success, and warnings are allowed. 1 means an error finding, a failed build, or an operational error such as an unreadable file. 2 means the command line itself was wrong.

## Fonts

A style names fonts by **font role** (not to be confused with a role in your resume, which is a job), not by file. A style's `theme.typ` lists the font roles it uses. Every built-in style has a `body` role and a `mono` role, and a third role for its display type:

- `body`: the text of the resume.
- `mono`: inline code.
- `display`: the name and the headings, in `pragmatic` and `mono`.
- `label`: the name and the section labels, in `experienced`.

Each font role has three fields in the style's theme (the `theme.typ` file, described under [Customizing a style](#customizing-a-style) and in [the theme keys](#theme-keys)):

- `preferred`: the font the template itself uses, such as Calibri. It is used when it is installed.
- `fallback`: a free font bundled inside the program, used when the preferred one is not installed.
- `metric-compatible`: `true` when the fallback has the same character widths as the preferred font, so lines break in the same places either way.

The bundled fallbacks are Carlito (for Calibri), Gelasio (for Georgia), Arimo (for Arial), and TeX Gyre Pagella (for Book Antiqua). DejaVu Sans Mono is bundled for the `mono` role. A style that prefers Calibri and falls back to Carlito produces the same line breaks and the same page count on a machine that has Calibri and on one that does not: the difference is only in the shapes of the letters. TeX Gyre Pagella is the exception. Its character widths differ a little from Book Antiqua's, so the `experienced` style, which sets its name and labels in it, can break those lines differently when Book Antiqua is not installed. The style says so with `metric-compatible: false`, and a build reports [`font-fallback-layout`](#code-font-fallback-layout).

**Which fonts may be used** is up to you:

- `--fonts auto` (the default) uses the bundled fonts, then any in `--font-path` and in the `RESUME_FONT_PATH` environment variable, then the fonts installed on the machine (on a Mac, this includes the fonts of Microsoft Office).
- `--fonts bundled` uses only the bundled fonts. The PDF no longer depends on the machine: the same input gives the same bytes on macOS and Windows, which the project's tests compare on both. `--font-path` and `RESUME_FONT_PATH` are ignored then, and `--font-path` is refused. Use it when you want to be sure.
- `--font-path <DIR>` adds a directory of font files. Give it more than once for more directories.

After a build, the `fonts:` line says which font each role uses, and where it came from:

- `Calibri (system)`: installed on the machine.
- `Calibri (application)`: installed with an application, such as Microsoft Office on a Mac.
- `Calibri (font-path)`: found in a directory you gave with `--font-path` or `RESUME_FONT_PATH`.
- `Carlito (fallback for Calibri)`: the bundled stand-in for a preferred font that is not installed.
- `DejaVu Sans Mono (bundled)`: a bundled font that the style asks for by name.
- `MISSING (neither A nor B)`: neither the preferred font nor its fallback was found.

Three findings describe the situation:

- [`font-fallback-metric`](#code-font-fallback-metric) is a note: a bundled font stands in and the layout is unchanged. Nothing to do. A human report does not show it, because the `fonts:` line already says so; `--format json` includes it.
- [`font-fallback-layout`](#code-font-fallback-layout) is a warning: the stand-in has different widths, so lines may break differently. Install the preferred font or check the page count.
- [`font-missing`](#code-font-missing) is an error: neither the preferred font nor its fallback is available, which means the style's font list was edited wrongly.

**Licenses.** The bundled fonts are free to embed. Most are under the SIL Open Font License; TeX Gyre Pagella is under the GUST Font License. The program carries a notice for each, and `resume licenses` prints them. They are also in [`fonts/LICENSES.md`](../fonts/LICENSES.md).

## Styles

A style decides how a resume looks. The content never changes between styles. The program has built-in styles, listed below, and you can make your own.

### Built-in styles

`resume styles` lists the styles you can choose. Pick one with `--style NAME`.

#### pragmatic

The default. It follows The Pragmatic Engineer's resume template: the name in the center of the header with the location and links on one side and the phone and email on the other, blue section titles over a rule, and each role as one line with the dates on the right. A good first choice. [Its README](../styles/pragmatic/README.md) describes where it differs from the template.

#### mono

It follows The Mono Engineer's resume template: a centered name over one centered contact line, section titles in black capitals over a black rule, and no color but the links. Choose it when the page should be quiet and the roles should carry the weight. [Its README](../styles/mono/README.md) describes where it differs from the template.

#### experienced

It follows The Experienced Engineer's resume template (the layout of Standard Resume's Venables): two zones, with each section's label in a gutter at the left and its content in the column beside it. The name and the labels are set in Book Antiqua, or in the bundled TeX Gyre Pagella when it is not installed. A block of bullets that all start with a label, such as `**Languages:** Rust`, becomes a skills table: the labels in one column and the values in the next. Links are red. Choose it when you want sections to be found by their labels at a glance. [Its README](../styles/experienced/README.md) describes where it differs from the template, and its limits.

### Customizing a style

To change a style, copy it, edit the copy, and tell `build` to use it:

```sh
resume export-style pragmatic my-styles
resume build Resume.md --styles-dir my-styles
```

`export-style NAME DIR` copies the style to `DIR/NAME/` and the files every style shares to `DIR/lib/`. It refuses to overwrite: if any file exists, nothing is written, unless you add `--force`. The copy builds exactly the same PDF as the built-in one until you change it.

A style in `--styles-dir` wins over a built-in style of the same name, and `resume styles --styles-dir DIR` marks it. A directory in `--styles-dir` that cannot be used is listed with the reason, not hidden.

A style has two files you edit:

- `theme.typ` holds **values**: fonts, sizes, colors, spacing, margins. Most changes belong here.
- `style.typ` holds **structure**: how the header is laid out and which parts appear. Change this to move things around.

To see a change as you make it, run [`resume watch`](#resume-watch) with `--styles-dir`: it rebuilds the PDF each time you save a style file. To design a style with Typst's own live preview instead, use [`resume export`](#resume-export).

### Theme keys

Every value a built-in theme sets is listed here. A theme is a Typst dictionary, and `spacing.section` means the `section` entry of `spacing`. Lengths are in Typst units, such as `pt` (points), `in` (inches), and `em` (a multiple of the font size). The **Styles** column says which built-in styles have the key. The styles do not all have the same keys: `experienced` keeps its page margins in `page-margin` and has no `margin`, for example.

| Key | Unit | What it controls | Styles |
|---|---|---|---|
| `fonts.<role>.preferred` | font name | The template's own font for the role, used when installed. The roles are `body`, `mono`, and `display` or `label`. | all |
| `fonts.<role>.fallback` | font name | The bundled font used when the preferred one is not installed. | all |
| `fonts.<role>.metric-compatible` | `true` or `false` | Whether the fallback has the preferred font's character widths. When `false`, `doctor` warns that lines may break differently. | all |
| `size.body` | length | The size of the text. | all |
| `size.name` | length | The size of the name. | all |
| `size.heading` | length | The size of the section titles. | mono, pragmatic |
| `size.role` | length | The size of the role line's title and company. In `experienced`, the dates are set with `size.period`. | all |
| `size.contact` | length | The size of the contact items. | mono, pragmatic |
| `size.footer` | length | The size of the footer text. | all |
| `color.text` | color | The color of the text. | all |
| `color.accent` | color | The color of section titles and the rule under them. | mono, pragmatic |
| `color.link` | color | The color of links. They are also underlined. | all |
| `color.muted` | color | The color of the footer. | all |
| `margin.x` | length | The left and right page margins. | mono, pragmatic |
| `margin.top` | length | The top page margin. | mono, pragmatic |
| `margin.bottom` | length | The bottom page margin. Leave room for the footer. | mono, pragmatic |
| `footer-descent` | percentage | How far the footer is lowered into the bottom margin, as a share of that margin's height. A larger value puts the footer closer to the page edge. | all |
| `line.top` | length, positive | The space above the baseline of a line. Together with `line.bottom` it sets the line's height from fixed lengths, so a fallback font with a slightly different shape gives identical line positions. Keep it positive. | all |
| `line.bottom` | length, negative | The space below the baseline of a line. Keep it negative: a positive value is silently clamped. | all |
| `line.leading` | length | Extra space between lines. | all |
| `spacing.section` | length | The space above a section title. | all |
| `spacing.section-below` | length | The space between a section title's rule and what follows. | mono, pragmatic |
| `spacing.role` | length | The space above a role. | all |
| `spacing.role-line-below` | length | The space between the role line and what follows. | all |
| `spacing.era` | length | The space above an era. | all |
| `spacing.era-line-below` | length | The space between the era line and what follows. | all |
| `spacing.blurb` | length | The space around a role's description. | all |
| `spacing.bullet` | length | The space between bullets. | all |
| `spacing.paragraph` | length | The space around paragraphs. | all |
| `rule.weight` | length | The thickness of the rule under a section title. | mono, pragmatic |
| `rule.gap` | length | The space between a section title and its rule. | mono, pragmatic |
| `bullet.marker` | content | The bullet character. | all |
| `bullet.indent` | length | How far the bullet is indented from the margin. | all |
| `bullet.gap` | length | The space between the bullet and its text. | all |
| `header.name-gap` | length | The least clear space between the name and either side of the header. When the name and the wider side do not fit with that space, the name moves to a line of its own. | pragmatic |
| `header.stacked-gap` | length | The space under the name when it has moved to its own line. | pragmatic |
| `header.gap` | length | The space between the name and the contact line, beyond what the two lines' own heights give. With a 24 pt name and an 11 pt contact line, 0 pt puts their baselines 17 pt apart. | mono |
| `color.label` | color | The color of the section labels in the gutter. | experienced |
| `size.label` | length | The size of the section labels. | experienced |
| `size.label-min` | length | The smallest a label is set when one word of it is wider than the gutter. A word that is still too wide at this size is broken between letters. | experienced |
| `size.period` | length | The size of a role's dates. | experienced |
| `gutter.width` | length | The width of the gutter at the left of the page, which holds the section labels. | experienced |
| `gutter.inset` | length | How far a label is inset from the page margin. | experienced |
| `gutter.gap` | length | The space between the gutter and the text column. | experienced |
| `page-margin.left` | length | The left page margin, outside the gutter. The style adds the gutter's width and gap to it, so the text column starts after them. | experienced |
| `page-margin.right` | length | The right page margin. | experienced |
| `page-margin.top` | length | The top page margin. | experienced |
| `page-margin.bottom` | length | The bottom page margin. Leave room for the footer. | experienced |
| `skills.label-max` | length | The widest the label column of a skills table may be. A longer label wraps. | experienced |
| `skills.column-gap` | length | The space between a skills label and its value. | experienced |
| `spacing.name-below` | length | The space under the name. | experienced |
| `spacing.skill-row` | length | The space between the rows of a skills table. | experienced |

Any size, margin, or spacing can change the page count and where lines break, so pass `--max-pages` after editing them. Spacing between blocks does not add up: when two blocks meet, the larger space wins, so a small change to a smaller space can do nothing. `line.top` and `line.bottom` are fixed lengths rather than font measurements, so a fallback font gives the same line positions as the preferred one.

For a serif look in `pragmatic` or `mono`, set the `display` role's `preferred` font to Georgia and its `fallback` to Gelasio.

### Shared components

Every style is built from the same components, in `styles/lib/components.typ`. They are why every style keeps the same promises, and why a style you write gets them for free:

- **A heading never ends a page.** A section title, a role line, an era line, and a role's description each move to the next page together with what follows them.
- **A bullet never splits across pages.** A bullet that does not fit moves whole. (A bullet taller than a whole page cannot, and overflows; `build` reports it.)
- **The output does not depend on the date, the language, or hyphenation.** These are fixed, so the same input gives the same PDF.
- **The layout is observable.** The components leave invisible markers that `build` and `doctor` read to report page counts, stranded headings, and which fonts were used. A style that removes them cannot be checked.

| Component | What it does |
|---|---|
| `installed-families` | The font families the program says are installed, so a style names only fonts that exist. |
| `font-stack` | Turns a font role (`preferred` and `fallback`) into the list of fonts to use. |
| `role-probe` | The invisible marker a role leaves for the layout checks. |
| `era-probe` | The invisible marker an era leaves for the layout checks. |
| `schema-version` | The version of the resume data these components understand. A resume of another version stops the build. |
| `index-roles` | Numbers each role and era, so the markers can name them. |
| `render-inlines` | Sets text, bold, italic, links, and code. An unknown kind stops the build rather than losing text. |
| `inlines-text` | The plain text of some inline content, used to recognize contact items. |
| `contact-items` | Splits the contact line at its separators into items. |
| `contact-parts` | Sorts the items into location, links, phones, and emails, so a style can arrange them. |
| `join-items` | Joins items with a separator. |
| `bullets` | A group of bullets as one list, each bullet kept whole on a page. |
| `role-grid` | The layout of a role line: title and company left, the period right. |
| `role-heading-content` | What a role line says: title, company, and period, in bold. |
| `era-heading-content` | What an era line says: the name and the period. |
| `section-heading-content` | What a section title says, in the accent color. |
| `default-headings` | The three heading looks (section, role, era) a style may replace one by one. |
| `section-heading` | A section title with its rule, kept with what follows. |
| `paragraph` | A paragraph of text. |
| `blurb` | A role's one-line description. |
| `role-head` | The start of a role: its heading and description. A style that sets something beside them, such as `experienced`'s label, renders this and then `role-body`. |
| `role-body` | The rest of a role: its eras, bullets, and paragraphs. |
| `role-line` | A role heading, kept with the next block. |
| `era-line` | An era heading, kept with the next block. |
| `render-role` | A role with its description, eras, bullets, and paragraphs. |
| `render-block` | One block of a section. |
| `render-sections` | Every section of the resume, with its blocks. Most styles end with this. |
| `footer` | The footer: the name on the left, `Page N of M` on the right. |
| `letter-document` | Letter schema/kind checks, metadata, page and text settings, and font-role reporting. |
| `resume-document` | The page setup, text settings, and rules every style shares. |

### Writing a style

A style is a directory with a `style.typ` that Typst compiles. The program passes the resume as data: `json("/resume.json")` holds the name, the contact items, and the sections with their blocks. You can write a whole style from scratch, but it is easier to start from a built-in one with `export-style`.

This is the smallest useful style. It centers the name and then lets the shared components set everything else:

```typst style
#import "/lib/components.typ": *
#import "./theme.typ": theme

#let resume = index-roles(json("/resume.json"))

#show: resume-document.with(theme, resume)

#align(center, text(size: 22pt, resume.name))

#render-sections(theme, resume)
```

It needs a `theme.typ` beside it, and the shared `lib/components.typ`. `export-style` gives you both. Copy the theme from `pragmatic` or `mono`: `render-sections` and `resume-document` read the keys those two share, which is what the **Styles** column of the theme keys marks, and Typst names the first missing key if a theme lacks one. The `experienced` theme has a different set of keys, so it does not work with this example.

Typst has its own reference, which is the place to learn how to arrange things on a page: [text](https://typst.app/docs/reference/text/text/), [block](https://typst.app/docs/reference/layout/block/), [grid](https://typst.app/docs/reference/layout/grid/), [lists](https://typst.app/docs/reference/model/list/), [set rules](https://typst.app/docs/reference/styling/#set-rules), and [show rules](https://typst.app/docs/reference/styling/#show-rules).

## Commands

```text
resume [--dry-run] [--format FORMAT] [--color COLOR] COMMAND
```

### Global options

These work with every command, and so do `-h` and `--help` (a command's usage) and `-V` and `--version` (the version).

| Option | Values and default | What it does |
|---|---|---|
| `--dry-run` | a switch | Shows what would be written and writes nothing. For `build` it still compiles and runs every check, so the exit code is a real build's. For `doctor --fix` it prints the diff. For `export-style` and `export` it lists the files. `watch` refuses it. `styles`, `check`, `licenses`, and `doctor` without `--fix` write nothing and ignore it. |
| `--format <FORMAT>` | `human` or `json`; default `human` | How findings are written: for people, or as JSON lines on standard output. `styles`, `licenses`, and `export-style` have no findings and ignore it. |
| `--color <COLOR>` | `auto`, `always`, or `never`; default `auto` | When to color the output. `auto` colors only on an interactive terminal, and turns color off when `NO_COLOR` is set to something or `TERM` is `dumb`. `always` and `never` override both. |

### resume letter

Prints a validated cover letter as website-ready plain text on stdout.
Findings and counts go to stderr; a parse error, missing application field or exceeded limit prints no letter.
The letter source uses `# Name`, optional contact, and `## Greeting`, `## Body`, `## Closing`; the name supplies the signature.
Use `{{company}}`, `{{title}}` and `{{recipient}}` in letter paragraphs; HTML character references can spell the braces too.
Values are inserted literally, never reparsed as Markdown.
The parser accepts and validates letter annotations using the shared annotation and alternate-group rules.
Letter builds share the resume `--tags`, `--core-only`, `--prefer` and `--drafts` flags.
With no flags, all ready paragraphs remain and each group uses its default ready wording, falling back to its first ready wording if the default is a draft.
Tag selection keeps eligible core and untagged standard paragraphs; `--core-only` keeps only eligible core body paragraphs.
Greeting and closing remain under every scope, with alternate wording selected normally.
Body order stays authored, and selecting an empty body is an error.
Only selected wording requires application field values; malformed placeholders anywhere still fail parsing.
Unknown tags and preferred wording names warn rather than fail, as they do for resumes.
Human selection reports list kept/dropped locations, chosen variants, draft counts and verification reminders on stderr, without copying letter prose.
With `--output-format pdf --format json`, nontrivial selections produce a `"type": "letter-selection"` record on stdout instead, containing `file`, `scope`, `tags`, `prefer`, `drafts`, `kept`, `dropped`, `chosen`, `verify`, `skipped_drafts`, `drafts_left_out` and `drafts_included`.
Locations have a `part` and 1-based `line`; dropped entries add `reason`, and chosen entries have `group`, `variant`, `by` and `not_chosen`.
An unchanged default selection is silent; text/both output still refuses JSON diagnostics.
Read every verification reminder and review paragraph coherence yourself.

From a source checkout, these examples use the [synthetic letter](../tests/fixtures/letters/valid.md).
For your own application, copy that file, edit its paragraphs, and substitute its path and your fields.
Read the complete text and PDF before submission; checks cannot establish truthful claims or paragraph coherence.

```sh
resume letter tests/fixtures/letters/valid.md --company Acme --title Architect --recipient Team
resume letter tests/fixtures/letters/valid.md --company Acme --title Architect --recipient Team --max-length 2000 --length-unit utf16
resume letter tests/fixtures/letters/tailored.md --tags platform --prefer technical
```

| Option | Values and default | What it does |
|---|---|---|
| `<FILE>` | one Markdown letter; not needed with `--spec` | The letter to validate and print. |
| `--spec <FILE>` | a position spec | Build the letter the spec names, with the choices it and its profiles make. See [Profiles and position specs](#profiles-and-position-specs). |
| `--profiles <DIR>` | none; needs `--spec` | Where the profiles are found; without it, `profiles` beside the spec's directory. |
| `--allow-overrides` | a switch; needs `--spec` | Let the flags a spec can also say replace its choices; the summary lists each. |
| `--print-spec` | a switch; needs `--spec` | Print the spec as resolved, as one TOML file, and build nothing. |
| `--tags <TAG>` | comma-separated or repeated | Keep matching, core and untagged standard body paragraphs; conflicts with `--core-only`. |
| `--core-only` | a switch | Keep eligible core body paragraphs and mandatory frames. |
| `--prefer <NAME>` | comma-separated or repeated | Preferred alternate wordings, most preferred first. |
| `--drafts` | a switch | Include authored draft wording for preview. |
| `--output-format <OUTPUT_FORMAT>` | `text`, `pdf` or `both`; none at argument parsing | Product format; text goes to stdout, PDF to a file. |
| `-o, --out <DIR>` | none at argument parsing | PDF directory, ignored for text; resolves to `out`. |
| `--company <COMPANY>` | none | Literal company field. |
| `--title <TITLE>` | none | Literal position title field. |
| `--recipient <RECIPIENT>` | none | Literal recipient field. |
| `--date <DATE>` | none | An explicit ISO date; retained but not printed in website text. |
| `--links <LINKS>` | `text-and-url` or `text-only`; none at argument parsing | Keep labels plus link destinations, or labels only. |
| `--max-length <MAX_LENGTH>` | none | Inclusive limit; zero is valid. Never truncates. |
| `--length-unit <LENGTH_UNIT>` | `utf16`, `scalars`, `bytes`, `words`; none at argument parsing | Needs a maximum length, from `--max-length` or the spec. Counts the exact final text, including newlines. |
| `-s, --style <STYLE>` | none | Parsed but ignored for text. |
| `--styles-dir <STYLES_DIR>` | none | Parsed but never opened for text. |
| `--paper <PAPER>` | `letter` or `a4`; none | Parsed but ignored for text. |
| `--max-pages <MAX_PAGES>` | none | Parsed but ignored for text. |
| `--fonts <FONTS>` | `auto` or `bundled`; none | Parsed but ignored for text. |
| `--font-path <FONT_PATH>` | none; repeatable | Parsed but never opened for text; still conflicts with `--fonts bundled`. |

No style or font setup runs in text mode, including paths from `RESUME_FONT_PATH`.
Every explicit option is retained separately from defaults for future spec support.
When omitted, product format resolves to `text`, links to `text-and-url`, and the length unit to `utf16`.
`--format json` conflicts with letter text, including in dry-run mode, because both would own stdout.
`--dry-run` performs the same checks but emits no letter bytes or files.
A failed stdout write or flush, including a closed pipe, returns exit 1 for this command; other listing commands retain their quiet closed-pipe behavior.
Partial delivery is possible on an output failure, so check the exit status before sending a letter.
No `.txt` file is created automatically; shell redirection can create or truncate a destination before the command starts.
Website counters may differ from these exact byte, Unicode scalar, UTF-16 and whitespace-word counts.

PDF modes use the `pragmatic` style, `letter` paper, `auto` fonts, and a one-page limit unless explicitly overridden.
The file is `<stem>-letter.pdf` under `--out`, avoiding the usual resume filename. For that reason `build` and `watch` refuse a resume whose name ends in `-letter` (`acme-letter.md`), which would be written over the letter of `acme.md`; the error suggests a name that works, such as `acme-resume.md`. `doctor` and `check` still read such a file, because they write no PDF.
A configured text limit also checks PDF-only builds.
All content and page checks finish before replacing a PDF; unlike `build`, a letter that exceeds its page limit is not saved.
In `both` mode the PDF is committed first, then text is printed; a later stdout failure leaves that PDF in place and reports it.
JSON diagnostics are allowed with PDF-only output.

The pragmatic PDF uses full-block letter formatting: body-sized sender details aligned left, compact recipient/company lines, and blank-line spacing between the sender, date, recipient, greeting and body paragraphs.
It includes the optional date (for example `February 29, 2028`) and authored closing/signature.
The greeting is printed exactly as authored; `Hiring Team:` is valid, and the renderer never inserts `Dear`.
No date appears when omitted; the clock is never consulted.
Metadata uses title `<Name> - Cover letter` and author `<Name>`, not company or position fields.
Only pragmatic supplies a letter entry initially; `resume styles` marks letter capability, and `export-style` includes `letter.typ`.
Custom styles keep required `style.typ` for resumes and optionally add `letter.typ`, reading schema 1 with kind `letter` from `/letter.json`.
Each compile reserves both payload paths and refuses the other document kind's payload, even if a file exists on disk.
`resume export` still previews resumes only.

```sh
resume letter tests/fixtures/letters/valid.md --company Acme --title Architect --recipient Team --date 2028-02-29 --output-format both --fonts bundled
```

### resume build

Typesets one or more resumes into PDFs, or the resume a [position spec](#profiles-and-position-specs) names. The PDF is named after the input file, or after the spec, and written to the output directory, which is created if it does not exist. If the resume has format errors, nothing is written. If it compiles but a layout check fails, such as the page limit, the PDF is still written so you can see what overflowed, and the command exits 1. With several files, each is built and each gets its own summary line; one failing never skips the others. Two inputs with the same file name, such as `Resume.md` and `sub/Resume.md`, would write the same PDF, so `build` refuses them with exit code 2: rename one or build them separately.

```sh
resume build Resume.md
resume build Resume.md Other.md --out pdfs
```

| Option | Values and default | What it does |
|---|---|---|
| `<FILE>...` | one or more Markdown files; not needed with `--spec` | The resumes to build. |
| `--spec <FILE>` | a position spec | Build from this spec instead of a FILE. See [Profiles and position specs](#profiles-and-position-specs). |
| `--profiles <DIR>` | none; needs `--spec` | Where the profiles are found; without it, `profiles` beside the spec's directory. |
| `--allow-overrides` | a switch; needs `--spec` | Let `--tags`, `--core-only`, `--prefer`, `--style`, and `--paper` replace the spec's choices; the summary lists each. |
| `--print-spec` | a switch; needs `--spec` | Print the spec as resolved, as one TOML file, and build nothing. |
| `-o, --out <DIR>` | default `out` | The directory the PDFs are written to. |
| `-s, --style <NAME>` | default `pragmatic` | The style to render with. |
| `--styles-dir <DIR>` | none | A directory of styles to use besides the built-in ones. A style here wins over a built-in of the same name. |
| `--paper <PAPER>` | `letter` or `a4`; default `letter` | The paper size. |
| `--max-pages <N>` | a number of pages | Fail when the resume needs more pages than this. |
| `--fonts <FONTS>` | `auto` or `bundled`; default `auto` | Which fonts may be used. See [Fonts](#fonts). |
| `--font-path <DIR>` | none; repeatable | A directory of fonts to use when `--fonts` is `auto`. Give it more than once. It cannot be combined with `--fonts bundled`. |
| `--tags <TAG>` | tag names; comma-separated or repeatable | Keep only items with one of these tags, besides `@core` and untagged items. See [Annotations](#annotations). It cannot be combined with `--core-only`. |
| `--core-only` | a switch | Keep only `@core` items. |
| `--prefer <NAME>` | wording names; comma-separated or repeatable | The wordings to use where a group has them, most preferred first. |
| `--drafts` | a switch | Keep what is marked `@draft`, to preview the resume once the drafts are ready. |

### resume watch

Builds one resume, then builds it again each time the resume or a style file changes. Leave it running in a terminal beside your editor and open the PDF in a viewer that reloads on change (Skim on a Mac, SumatraPDF on Windows). Stop it with Ctrl+C.

```sh
resume watch Resume.md --open
resume watch Resume.md --styles-dir my-styles --style pragmatic
```

Each build is a `build`: the same summary line, the same findings, and the same rules for what is written. A build that fails is printed, and the watch goes on to the next change. The watch ends only when it cannot start (a missing resume, an unknown style, a file watcher that cannot be created), when the file watcher dies, or when nobody reads its output any more. A PDF that a viewer holds open cannot be replaced on Windows; close the viewer, and the next change builds again.

A build starts shortly after the last change, so an editor that is still writing the file is not built half-way. Changes during a build cause one more build. A resume saved by an editor that replaces the file is seen. The resume's folder is watched, and with `--styles-dir`, so is that folder, except the output directory.

`--dry-run` is refused with `watch`, since watching without writing has no purpose: use `build --dry-run`.

| Option | Values and default | What it does |
|---|---|---|
| `<FILE>` | one Markdown file | The resume to watch and build. |
| `-o, --out <DIR>` | default `out` | The directory the PDF is written to. |
| `-s, --style <NAME>` | default `pragmatic` | The style to render with. The style is looked up again before each build, so a style you create or change while watching is used. |
| `--styles-dir <DIR>` | none | A directory of styles to use besides the built-in ones. It is watched too. |
| `--paper <PAPER>` | `letter` or `a4`; default `letter` | The paper size. |
| `--max-pages <N>` | a number of pages | Fail a build when the resume needs more pages than this. |
| `--fonts <FONTS>` | `auto` or `bundled`; default `auto` | Which fonts may be used. See [Fonts](#fonts). |
| `--font-path <DIR>` | none; repeatable | A directory of fonts to use when `--fonts` is `auto`. It cannot be combined with `--fonts bundled`. |
| `--debounce <DURATION>` | a time such as `1s`; default `150ms` | How long to wait after the last change before building. |
| `--open` | a switch | Opens the PDF once, after the first build that writes it, even a build with a layout error. A failure to open it is a warning. |
| `--tags <TAG>` | tag names; comma-separated or repeatable | Keep only items with one of these tags, besides `@core` and untagged items. See [Annotations](#annotations). It cannot be combined with `--core-only`. |
| `--core-only` | a switch | Keep only `@core` items. |
| `--prefer <NAME>` | wording names; comma-separated or repeatable | The wordings to use where a group has them, most preferred first. |
| `--drafts` | a switch | Keep what is marked `@draft`, to preview the resume once the drafts are ready. |

### resume doctor

Checks resumes, explains what is wrong and why it matters, and can fix what is safe to fix. It runs everything `build` runs, without writing a PDF, and adds the content advice and the setup checks. Findings are grouped by kind. See [Checking your resume](#checking-your-resume).

```sh
resume doctor Resume.md --fix
resume doctor --explain role-separator
```

| Option | Values and default | What it does |
|---|---|---|
| `<FILE>...` | one or more Markdown files | The resumes to check. Not needed with `--explain`. |
| `-o, --out <DIR>` | default `out` | The directory `build` would write to. It is checked, never written. |
| `-s, --style <NAME>` | default `pragmatic` | The style to render with. |
| `--styles-dir <DIR>` | none | A directory of styles to use besides the built-in ones. |
| `--paper <PAPER>` | `letter` or `a4`; default `letter` | The paper size. |
| `--max-pages <N>` | a number of pages | Report a layout error when the resume needs more pages than this. |
| `--fonts <FONTS>` | `auto` or `bundled`; default `auto` | Which fonts may be used. |
| `--font-path <DIR>` | none; repeatable | A directory of fonts to use when `--fonts` is `auto`. |
| `--fix` | a switch | Apply the changes that are mechanical and keep the meaning, to the files themselves. Add `--dry-run` to see the diff and change nothing. |
| `--explain <CODE>` | a finding's code | Explain a finding by its code, with an example, and exit. It cannot be combined with files or `--fix`. |
| `--tags <TAG>` | tag names; comma-separated or repeatable | Keep only items with one of these tags, besides `@core` and untagged items. See [Annotations](#annotations). It cannot be combined with `--core-only`. |
| `--core-only` | a switch | Keep only `@core` items. |
| `--prefer <NAME>` | wording names; comma-separated or repeatable | The wordings to use where a group has them, most preferred first. |
| `--drafts` | a switch | Keep what is marked `@draft`, to preview the resume once the drafts are ready. |

### resume check

The same report as `doctor`, which can never change a file. Its options are those of `doctor` without `--fix` and `--explain`. Use it in a script or a pre-commit hook.

```sh
resume check Resume.md --format json
```

| Option | Values and default | What it does |
|---|---|---|
| `<FILE>...` | one or more Markdown files | The resumes to check. |
| `-o, --out <DIR>` | default `out` | The directory `build` would write to. It is checked, never written. |
| `-s, --style <NAME>` | default `pragmatic` | The style to render with. |
| `--styles-dir <DIR>` | none | A directory of styles to use besides the built-in ones. |
| `--paper <PAPER>` | `letter` or `a4`; default `letter` | The paper size. |
| `--max-pages <N>` | a number of pages | Report a layout error when the resume needs more pages than this. |
| `--fonts <FONTS>` | `auto` or `bundled`; default `auto` | Which fonts may be used. |
| `--font-path <DIR>` | none; repeatable | A directory of fonts to use when `--fonts` is `auto`. |
| `--tags <TAG>` | tag names; comma-separated or repeatable | Keep only items with one of these tags, besides `@core` and untagged items. See [Annotations](#annotations). It cannot be combined with `--core-only`. |
| `--core-only` | a switch | Keep only `@core` items. |
| `--prefer <NAME>` | wording names; comma-separated or repeatable | The wordings to use where a group has them, most preferred first. |
| `--drafts` | a switch | Keep what is marked `@draft`, to preview the resume once the drafts are ready. |

### resume styles

Lists the styles that can be chosen: the built-in ones, then those in `--styles-dir`, with where each lives. A style that overrides a built-in one is marked, and one that cannot be used is listed with the reason.

```sh
resume styles
resume styles --styles-dir my-styles
```

| Option | Values and default | What it does |
|---|---|---|
| `--styles-dir <DIR>` | none | A directory of styles to list besides the built-in ones. |

### resume export-style

Copies a built-in style to a directory, to edit it. See [Customizing a style](#customizing-a-style).

```sh
resume export-style mono my-styles
```

| Option | Values and default | What it does |
|---|---|---|
| `<NAME>` | a built-in style | The built-in style to copy. |
| `<DIR>` | a directory | Where to copy it. The style goes to `DIR/NAME/` and what styles share to `DIR/lib/`. |
| `--force` | a switch | Replace files that already exist, including any edits you made to an earlier copy. Without it, nothing is written when any file exists. |

### resume export

Writes a resume as data, together with a style, so you can design the style with a live preview in Typst's own tools. It reads and checks the resume but does not compile it, so it needs no fonts.

```sh
resume export Resume.md preview
resume export Resume.md preview --style mono --force
```

It writes three things under the directory you give: `resume.json`, the resume as the style reads it; the style, in `DIR/NAME/`; and the files every style shares, in `DIR/lib/`. `DIR` is also a valid `--styles-dir`, so `resume build Resume.md --styles-dir preview` builds with what you edit there.

To preview, open the directory in an editor with the tinymist extension, or run Typst's own watcher on the style file:

```text
typst watch --root preview preview/pragmatic/style.typ
```

Exporting again refreshes `resume.json` and keeps style files that already exist, because you are probably editing `theme.typ`; `--force` replaces them too. A resume with format errors writes nothing and exits with 1.

The preview is a guide to the look and not to the page count: Typst's own tools use the fonts installed on your machine and US Letter paper, so page breaks can differ from `resume build`, which is the authority on the number of pages. To preview A4, give Typst the `paper` input, as in `typst watch --root preview --input paper=a4 preview/pragmatic/style.typ`. Do not set a `fonts` input: the style expects a list, and Typst's command line gives text.

| Option | Values and default | What it does |
|---|---|---|
| `<FILE>` | one Markdown file | The resume to export. |
| `<DIR>` | a directory | Where to write `resume.json`, the style, and what styles share. |
| `-s, --style <NAME>` | default `pragmatic` | The style to export. |
| `--styles-dir <DIR>` | none | A directory of styles to use besides the built-in ones. |
| `--force` | a switch | Replace style files that already exist. `resume.json` is always replaced. |
| `--tags <TAG>` | tag names; comma-separated or repeatable | Keep only items with one of these tags, besides `@core` and untagged items. See [Annotations](#annotations). It cannot be combined with `--core-only`. |
| `--core-only` | a switch | Keep only `@core` items. |
| `--prefer <NAME>` | wording names; comma-separated or repeatable | The wordings to use where a group has them, most preferred first. |
| `--drafts` | a switch | Keep what is marked `@draft`, to preview the resume once the drafts are ready. |

### resume tags

Lists what one resume's annotations offer: its tags, how often each keyword is used, the `&id` names a spec can change, and its groups of alternate wordings (a group is changed by its group name), with the line of each and which wordings are drafts. A position spec, or you at the command line, can only name what this lists. See [Annotations](#annotations).

```sh
resume tags Resume.md
resume tags Resume.md --format json
```

A tag shows how many items carry it themselves, and how many more carry it because a section, role, or era above them does. An `ids` section appears only when the resume has names, and lists each with its kind and line. With `--format json` it prints one JSON object, with `type` `inventory`, instead. A resume with format errors prints its findings and no list, and exits 1, because a partial list would suggest a name is not used.

| Option | Values and default | What it does |
|---|---|---|
| `<FILE>` | one Markdown file | The resume to list. |

### resume licenses

Prints the license notices of the fonts built into the program, to standard output. It takes no options of its own.

```sh
resume licenses
```

### Environment variables

- `RESUME_FONT_PATH` adds font directories, like `--font-path`. It is a list separated by `:` on macOS and Linux and by `;` on Windows, and it is ignored with `--fonts bundled`.
- `NO_COLOR`, when set and not empty, turns off color with `--color auto`. `--color always` overrides it.
- `TERM`, when it is `dumb`, also turns off color with `--color auto`.

Nothing else is read from the environment. Nothing is read from the current directory unless you ask: styles come from `--styles-dir`, and there is no configuration file.


## Annotations

One baseline resume can hold everything you might say, for every kind of job. Annotations in Markdown comments say how each item may be tailored: how important it is, whether it is ready, which jobs it fits, and which other wordings it has. A build then keeps what you select. Comments never print, so the baseline still reads as one resume, and a resume without annotations builds exactly as it did before them.

### Syntax

An annotation is an HTML comment whose first word starts with `@`, `#`, `=`, or `&` followed by a letter. Any other comment is an ordinary comment and is ignored.

| Write | Meaning |
|---|---|
| `@core` | Important for every job: kept whatever the tags. At most one of `@core` and `@extra`. |
| `@extra` | Kept only when one of its tags is selected. |
| `@draft` | Not ready: left out of every build unless you pass `--drafts`. At most one of `@draft` and `@verify`. |
| `@verify` | Kept, and listed after each build until you remove the mark. |
| `#tag` | A topic, such as `#ai` or `#platform`. Any number on one line; the same tag twice counts once. |
| `=group.variant` | One wording of an item. Every group has a `default`. At most one per line. |
| `&id` | A name for one item, so a position spec can change its tags or priority for one application. At most one per item. |

Tag, group, wording, and id names are lowercase letters and digits, starting with a letter, with single hyphens between, and at most 40 characters. A misspelled keyword, such as `@cor`, is the error [`annotation-unknown-keyword`](#code-annotation-unknown-keyword):

```markdown in-section expect=annotation-unknown-keyword
### Acme · Engineer · 2015 – 2017

- Cut build time from 40 minutes to 9. <!-- @cor -->
```

### Where an annotation goes

An annotation marks an item in one of two places: at the end of the item's own line, or alone on the line directly above it.

```markdown in-section expect=none
### Acme · Engineer · 2015 – 2017 <!-- #platform -->

- Cut build time from 40 minutes to 9. <!-- @core #ai -->
<!-- @extra #data -->
- Moved 3 data pipelines to a new scheduler.
```

It can mark a section heading, a role line, an era line, a role's first paragraph (its description), any other paragraph, or a bullet. Anything less clear is an error with a fix, never ignored: a blank line between the comment and its item is [`annotation-detached`](#code-annotation-detached), a second annotation for one item is [`annotation-duplicate`](#code-annotation-duplicate), and an annotation in the middle of a line is [`annotation-misplaced`](#code-annotation-misplaced):

```markdown in-section expect=annotation-misplaced
### Acme · Engineer · 2015 – 2017

- Cut build time <!-- #ai --> from 40 minutes to 9.
```

### Alternate wordings

Several wordings of one item sit next to each other, each marked with the same group name and its own wording name. A build renders exactly one: the one you prefer, or `default`. A group's wordings are all bullets or all paragraphs, state the same priority (or none), and its name is used once in the file. Section headings, role lines, and era lines have one wording, so a wording there is [`alternate-not-allowed`](#code-alternate-not-allowed).

```markdown in-section expect=none
### Acme · Engineer · 2015 – 2017

- Led delivery of the LLM gateway for 20 teams. <!-- =gateway.default @core -->
- Built the LLM gateway 20 teams use. <!-- =gateway.short @core -->
- Gave 20 teams control of their AI spend. <!-- =gateway.business @core -->
```

A group without a `default` is [`alternate-missing-default`](#code-alternate-missing-default). The numbers in the wordings of one group should agree; [`alternate-facts-differ`](#code-alternate-facts-differ) warns when they do not.

### Naming an item

`&id` gives one item a name. A position spec uses the name to change that item's tags or priority for one application, so the baseline stays as it is for every other job. The name does not appear in the PDF and does not change what a build keeps.

```markdown in-section expect=none
### Acme · Engineer · 2015 – 2017

- Led delivery of the LLM gateway for 20 teams. <!-- &gateway @extra #ai -->
- Mentored 6 engineers through promotion in 2 years. <!-- #leadership -->
```

An id is unique in a document, and shares its names with the groups of alternate wordings, so an id cannot also be a group name. Two items with one id are [`item-id-reused`](#code-item-id-reused):

```markdown in-section expect=item-id-reused
### Acme · Engineer · 2015 – 2017

- Led delivery of the LLM gateway for 20 teams. <!-- &gateway -->
- Cut the gateway's cache cost by 30% in 2 quarters. <!-- &gateway -->
```

A wording in a group has no id of its own; the group name is its name. An `&id` on one is [`item-id-on-alternate`](#code-item-id-on-alternate):

```markdown in-section expect=item-id-on-alternate
### Acme · Engineer · 2015 – 2017

- Led delivery of the LLM gateway for 20 teams. <!-- =gateway.default &gateway -->
- Built the LLM gateway 20 teams use. <!-- =gateway.short -->
```

### What a build keeps

| You build with | It keeps |
|---|---|
| no option | everything except drafts |
| `--tags a,b` | `@core` items, items tagged `a` or `b`, and untagged items; an `@extra` item needs one of the tags |
| `--core-only` | `@core` items only |

A tag on a section, role, or era applies to everything under it, so an untagged bullet under a `#platform` role counts as tagged `#platform`, and `--tags ai` leaves it out. The build lists every item dropped this way. Priority does not pass down: `@core` on a role keeps its line and its description, not its bullets. A role's description without a priority of its own takes the role's. `@draft` on a section, role, or era leaves out everything under it. A section, role, or era the selection empties is dropped, so a heading never prints alone; one you wrote empty stays.

For each group, the build uses the first wording of `--prefer` that the group has, else `default`. When `default` is a draft, it uses the group's first ready wording.

### Drafts and verification

Every build leaves `@draft` items out. `--drafts` puts them back, to preview the resume once they are ready; the summary says so. A preferred wording that is a draft is skipped, and the summary names it. `@verify` items are kept and listed after the build, so a claim you still need to check is not sent by accident.

### Tailoring from the command line

`build`, `watch`, `doctor`, `check`, and `export` take the same four options:

```sh
resume build Resume.md --tags ai,platform --prefer short
resume build Resume.md --core-only
resume build Resume.md --tags platform --drafts
```

`--tags` and `--prefer` take a comma-separated list, or the option given more than once. `--tags` cannot be combined with `--core-only`. A tag or wording that the file does not use is a warning that lists the ones it does, [`unknown-tag`](#code-unknown-tag) or [`unknown-wording`](#code-unknown-wording), because a misspelled tag would quietly select nothing.

After a tailored build, a few lines under the summary say what the selection did:

```text
ok out/Resume.pdf  pragmatic  2 pages  0 errors, 0 warnings  built in 90ms
  fonts: body Calibri (installed); display Calibri (installed); mono DejaVu Sans Mono (bundled)
  selection: tags lead; prefer leader
  kept: 4 sections, 6 roles, 3 paragraphs, 41 bullets; dropped: 2 sections, 22 bullets (of which 14 other wordings)
  wordings: 28 chosen (28 preference)
  dropped by an inherited tag: bullet at line 19 "Rust, Java, Scala, SQL"
  dropped as emptied: section at line 17 "Languages and Technologies"
  to verify: bullet at line 44 "Cut costs 40 percent across 3 regions."
  drafts left out: 2
  core alone: 1 page
```

Routine choices are counted; what can surprise you is listed with its line. `core alone` is the page count of the `@core` content by itself, with the same style, paper, fonts, and wordings: it is what a `--core-only` build would print. A build of a file without annotations, with no options, prints none of these lines.

With `--format json`, the same facts are one JSON object on standard output, with `type` `selection`, before the build's findings.
Each item it names has its source `file`, `line`, and `column`, including dropped, verify, and reordered items.
The count field `baseline` means the prepared input to selection, before anything is dropped or reordered.

To see what a file's annotations offer, use [`resume tags`](#resume-tags).

## Profiles and position specs

One annotated baseline can serve every application. A **position spec** is one small file that records the choices for one application: which baseline to use, which tags and wordings to keep, the style, and the paper. A **profile** is a named set of choices that several specs share, such as "principal-level positions" or "AI platform positions". `resume build --spec positions/acme.toml` builds the PDF from the spec and its profiles, so the PDF you sent can be rebuilt from the spec and the same version of the program.

### A worked example

Keep the files side by side. The spec sits in `positions/`, the profiles it extends in `profiles/` beside it, and the baseline wherever the spec says.

```text
baselines/baseline.md
letters/cover.md
profiles/principal.toml
profiles/ai-platform.toml
positions/acme.toml
```

Two profiles, one for the level and one for the topic:

```toml profiles/principal.toml
[build]
style = "pragmatic"

[select]
tags = ["leadership"]
prefer = ["technical", "short"]
pick = { gateway = "long" }
```

```toml profiles/ai-platform.toml
[build]
paper = "a4"

[select]
tags = ["ai", "platform"]
prefer = ["business"]
pick = { gateway = "short", summary = "ai" }

[letter.select]
tags = ["platform"]
prefer = ["short"]
```

And the spec for one application, which extends both and adds its own choices:

```toml positions/acme.toml
extends = ["principal", "ai-platform"]

[position]
company = "Acme"
title = "Principal Architect, AI Platform"
level = "principal"
date = 2026-11-19
posting = "https://example.com/jobs/123"

[build]
content = "../baselines/baseline.md"
style = "mono"

[select]
tags = ["iceberg"]
prefer = ["short"]
pick = { summary = "acme" }
promote = ["leadership"]

[letter]
content = "../letters/cover.md"
recipient = "Hiring Team"
date = 2026-11-20
max-length = 2000

[letter.select]
pick = { opening = "warm" }
```

The `[letter]` table is for the cover letter of the same application; [The letter of a spec](#the-letter-of-a-spec) says what it does. A spec without one builds exactly as before.

Build it, and print what it resolves to:

```sh
resume build --spec positions/acme.toml
resume build --spec positions/acme.toml --print-spec
```

The second command prints the choices of all three files as one flat spec and builds nothing. It does not read the baseline, so it works before the baseline exists:

```toml resolved
# Resolved from: principal, ai-platform, the spec

[position]
company = "Acme"
title = "Principal Architect, AI Platform"
level = "principal"
date = 2026-11-19
posting = "https://example.com/jobs/123"

[build]
content = "../baselines/baseline.md"
style = "mono"
paper = "a4"

[select]
tags = ["ai", "iceberg", "leadership", "platform"]
prefer = ["short", "business", "technical"]
promote = ["leadership"]

[select.pick]
gateway = "short"
summary = "acme"

[letter]
content = "../letters/cover.md"
recipient = "Hiring Team"
date = 2026-11-20
max-length = 2000

[letter.select]
tags = ["platform"]
prefer = ["short"]

[letter.select.pick]
opening = "warm"
```

The `style` is the spec's `mono`, because the spec is more specific than `principal`'s `pragmatic`. The `paper` comes from `ai-platform`, the only file that chose one. The `gateway` pick is `short`, from `ai-platform`, because it is listed after `principal`. Saved beside the original (the path in `content` is relative to the spec's own directory), the printed file resolves to the same choices and needs no profiles.

### The files

Both kinds of file are TOML and strict: a key the program does not know is an error that names the file and line and suggests the key you probably meant. A name (a tag, a group, a wording, a profile) follows the rule in [Annotations](#annotations): lowercase letters and digits, starting with a letter, with single hyphens, at most 40 characters.

| Key | In | What it says |
|---|---|---|
| `extends` | spec or profile | The profiles to start from, least specific first. A name is a file name without `.toml`, in lowercase. |
| `[position]` | spec only | What the application is for: `company`, `title`, `level`, `domain`, `audience`, `date` (`2026-11-19`), `posting` (a web address), and `emphasis` with `business`, `org`, and `technical`, each `low`, `medium`, or `high`. Every key is optional. The summary prints them; they never change a build, and they never reach the PDF. |
| `content` | spec only, under `[build]` | The baseline to build: a relative path, written with forward slashes, from the spec's own directory. It may climb with `..`. |
| `style`, `paper` | under `[build]` | The style, and the paper (`letter` or `a4`). |
| `tags` | under `[select]` | The tags to keep, as `--tags` does. |
| `prefer` | under `[select]` | The wordings to prefer, most preferred first, as `--prefer` does. |
| `pick` | under `[select]` | The wording a named group must use: `pick = { gateway = "short" }`. A group or wording the baseline does not have is an error that names the line that wrote it, and no PDF is made. |
| `core-only` | under `[select]` | `true` keeps only `@core` items. It cannot be combined with `tags` in the files that are merged. |
| `promote` | under `[select]` | The tags whose items come first, most promoted first. See [Ordering with promote](#ordering-with-promote). |

`drafts` is never a key. Whether to preview drafts is a choice you make at the command line, so a file can never put unfinished text into a PDF you send.

### How the choices merge

The files are applied from the least specific to the most specific: each profile after the profiles it extends, the profiles in the order `extends` lists them, and the spec last. Later is more specific. A profile that two files extend, such as a shared base, is applied once.

- `tags` are a union of every file's tags.
- `prefer` and `promote` list the most specific file's names first, then the names of less specific files that are not listed yet.
- `pick`, `style`, `paper`, and `core-only` are decided by the most specific file that has one. A pick is decided for each group on its own.
- An item override ([Changing one item for one application](#changing-one-item-for-one-application)) is decided for each item and each field on its own: the most specific file that writes an item's `tags` decides its tags, and the most specific file that writes its `priority` decides its priority. Tags are replaced, never added together.

If the files cannot be put in one order, because a spec lists `["b", "base"]` while `b` extends `base`, the build stops and names both. A cycle is an error that prints the way round. The most you can nest is 8 levels and 32 profiles.

Profiles are looked for in the directory given with `--profiles`, and otherwise in `profiles` beside the spec's own directory: `positions/acme.toml` finds `positions/../profiles`. There is no other place, so the result cannot depend on where you run the command. A spec that extends nothing needs no profiles directory. Give a profile's file the lowercase name it is called by: `principal.toml`. Do not make the spec's directory a link, because `..` after a link does not mean the same thing on every system.

### What a spec build prints

With the worked example above and a baseline whose wordings are `acme`, `ai`, `default`, `long`, and `short`, `resume build --spec positions/acme.toml --fonts bundled` prints:

```text
ok out/acme.pdf  mono  1 page  0 errors, 0 warnings, 1 note  built in 65ms
  spec: positions/acme.toml
  position: Acme; Principal Architect, AI Platform; level principal; dated 2026-11-19; posting https://example.com/jobs/123
  profiles: principal -> ai-platform -> the spec
  content: baselines/baseline.md
  environment: --fonts bundled
  fonts: body Carlito (fallback for Calibri); display Carlito (fallback for Calibri); mono DejaVu Sans Mono (bundled)
  selection: tags ai, iceberg, leadership, platform; prefer short, business, technical; promote leadership; picks gateway.short, summary.acme
  kept: 2 sections, 1 role, 1 blurb, 1 paragraph, 3 bullets; dropped: 2 paragraphs, 3 bullets (of which 4 other wordings)
  wordings: 2 chosen (2 pick)
  moved up in role at line 18 "Analytical Engines Ltd · Principal Engineer": 1 of 3 bullets (line 27)
  core alone: not measured (no-core)
note[unknown-wording]: profile wordings `business` (profiles/ai-platform.toml:6:11), `technical` (profiles/principal.toml:6:11): not used in baselines/baseline.md
 = help: the resume's wordings are `acme`, `ai`, `default`, `long`, `short`
 = help: `resume tags <file>` lists every tag and wording
 = help: a profile can name what this resume lacks, so this is only a note
```

The spec lines come before anything else is said, so you see which spec and which file were used first. `profiles` is the order the files were applied in. `environment` echoes the options that are about your machine and not about the resume (`--fonts`, `--font-path`, `--styles-dir`, `--max-pages`, `-o`) when you change them. Then come the lines of an ordinary [tailored build](#tailoring-from-the-command-line).

The PDF is named after the spec, not the baseline: `positions/acme.toml` builds `acme.pdf`, and that name is also the identity written into the PDF. Two specs over one baseline therefore write two files. Two specs with the same name in different directories would write the same file, and so would two names that differ only in capitals on macOS and Windows, so give each application its own file name.

With `--format json`, a build writes one object with `type` `spec` on standard output, before the `selection` object and the findings. Every key is always there: `file`, `position` (the fields of `[position]`, each `null` when unset; `null` itself when the spec has no `[position]`), `chain` (the `name` and `file` of each link; the spec's `name` is `null`), `content`, `pdf` (the file written or, with `--dry-run`, the file that would be written; `null` when there is none), `style`, `paper`, `drafts`, `overrides`, `environment`, and `version`. Each override has the `flag`, the values it `set` (none for a switch), what it `replaced` (`null` when the spec chose nothing, else the `setting` and its `value` list), and `at`, where the spec chose it, one place for each file. The `selection` object also has `picks`, which is empty for a build of a file.

### Changing one item for one application

Sometimes one application needs one item to count differently, such as a bullet that is `@extra` in the baseline but central to this job. Do not edit the baseline, which every other application also reads. Give the item a name in the baseline with [`&id`](#naming-an-item), then say what changes in the spec or in a profile:

```text
[select.override.gateway]
tags = ["ai", "platform"]
priority = "core"

[select.override.cache]
tags = []
priority = "none"
```

The name after `override.` is the item's `&id`, or the group name of a group of alternate wordings. An override can change only two things:

| Key | What it does |
|---|---|
| `tags` | Replaces the item's own tags as a whole. `tags = []` removes them all. It does not change the tags `--tags` selects, and it does not change the tags the item inherits from its section, role, or era. |
| `priority` | Sets the item's own priority to `core` or `extra`. `none` removes the item's own `@core` or `@extra`, so the item is judged by its tags alone. A role's blurb that has no priority of its own still takes its role's. |

A table must write at least one of them; an empty one is an error. Nothing else about an item can change: not its text, its draft status, its order, or its name. Use `pick` and `prefer` to choose a wording, and `promote` to move items up. A group override applies to every wording of the group, so the wordings still agree.

An item in a container follows the container's rules as before: a tag on a section, role, or era still applies to everything under it, and a priority on a container does not pass down. An item that is a draft stays out of the build whatever its priority.

The cover letter has its own table, `[letter.select.override.<id>]`, for the body paragraphs of the letter. A name used in the resume and in the letter means two different items, one in each document. The greeting and closing are not items a spec can address.

Naming an item that the baseline does not have is an error that names the line that wrote the override, and no PDF is made. The check runs before anything is selected, so a misspelled name is found even when the item it meant would have been left out. `--print-spec` prints the overrides, including `tags = []` and `priority = "none"`, and still does not read the baseline. `resume tags` lists the names a spec can use.

### Ordering with promote

Selection keeps and drops items; `promote` also moves them.
The first items of a role are the ones a reader takes in, so a leadership application can put its leadership bullets first while a technical one keeps the baseline's order, from one baseline.

`promote = ["people", "leadership"]` ranks each item by the first of those tags it carries, counting the tags it inherits from its section, role, or era: `people` items first, then `leadership` items, then the rest.
Items of equal rank keep the baseline's order, so promoting changes only what it must.
The worked example promotes `leadership`, so in the Analytical Engines role "Mentored 6 engineers through promotion" rises above "Cut a weekly run", and the summary says so with the bullet's line:

```text
  moved up in role at line 18 "Analytical Engines Ltd · Principal Engineer": 1 of 3 bullets (line 27)
```

What moves, and what never does:

- Items move only within their own list.
  A list is the bullets between two other blocks, or a run of paragraphs next to each other.
  A bullet never moves past a paragraph, a page break, an era line, or into another role, and paragraphs and bullets never trade places.
- Sections, roles, eras, page breaks, and a role's blurb stay where they are.
- A group of wordings ranks by the tags of all its wordings, and the wording chosen renders in the group's place.
- A nested bullet is already flattened by the parser ([`nested-list`](#code-nested-list)), so it orders as a bullet of its own and can move away from its parent.
- `@core`, `@extra`, `@draft`, and `@verify` do not change rank.
  Promoting decides order only after selection has decided what is kept.

A tag you add only to rank an item also counts for selection.
A general bullet (untagged, so every build keeps it) that you tag `#people` is dropped by a build with `tags = ["arch"]`.
Tag it for every audience that keeps it: `#people #arch`.

A bullet that only one audience keeps can sit anywhere in the baseline, because the other builds never show it.
Put it where that audience should read it, and it needs no promote tag.

The summary has one `moved up` line for each list in which something rose, or `moved up: nothing` when nothing did.
The JSON `selection` object has `promote` and `reordered` (the `container`, the list's `kind` and `kept` count, and each item that rose with its `from` and `to` place).
A promote tag the baseline does not use is reported like any other tag: a warning when the spec wrote it, a note when only a profile did.
Letters keep the order you wrote.

### Flags beside a spec

A spec is the whole recipe, so the options that choose what goes into the resume are refused beside it, and the error names every one:

```text
error: --tags and --paper cannot be used beside --spec: the spec is the whole recipe, so a flag would silently change what it rebuilds; put the choice in the spec or a profile, or add --allow-overrides to experiment
```

Those options are `--tags`, `--core-only`, `--prefer`, `--style`, and `--paper`. To experiment with one, add `--allow-overrides`:

```sh
resume build --spec positions/acme.toml --allow-overrides --tags ai
resume build --spec positions/acme.toml --allow-overrides --core-only --style pragmatic
```

Each option you type then **replaces** the spec's choice. `--tags` or `--core-only` replaces the whole scope, so `--tags ai` narrows a spec that keeps four tags to one, and `--core-only` is allowed over a spec that has tags. `--prefer` replaces the whole list. A pick can only come from a file. The summary says `spec: positions/acme.toml, plus overrides` and lists each override with what it replaced and where that was written, and the JSON `overrides` lists the same. The PDF you send should be built without them.

The options about your machine (`--fonts`, `--font-path`, `--styles-dir`, `--max-pages`, `-o`) and `--dry-run` are always allowed. So is `--drafts`, to preview the spec with its unfinished items; the summary says `with drafts`, and the PDF is written as `acme.drafts.pdf`, so a preview never replaces the `acme.pdf` you built to send. A spec file whose name ends in `.drafts` is refused for that reason.

### The letter of a spec

The same spec can say how to build the cover letter for the application, so the letter you send and the resume you send are rebuilt from one file. Add a `[letter]` table that names the letter, and a `[letter.select]` table for what to keep of it. `resume letter --spec` builds it:

```sh
resume letter --spec positions/acme.toml
resume letter --spec positions/acme.toml --output-format pdf --allow-overrides
```

With the worked example above, `resume letter --spec positions/acme.toml` prints the letter on standard output and, on standard error:

```text
selection: tags platform; prefer short; picks opening.warm
letter: 5 kept, 0 dropped
wording opening.warm: pick
wording evidence.short: preference
wording summary.default: default
kept: greeting at line 8
kept: body at line 16
kept: body at line 22
kept: body at line 24
kept: closing at line 32
ok printed letter: 132 utf16, 132 scalars, 132 bytes, 23 words, 10 line breaks
  spec: positions/acme.toml
  position: Acme; Principal Architect, AI Platform; level principal; dated 2026-11-19; posting https://example.com/jobs/123
  profiles: principal -> ai-platform -> the spec
  letter: letters/cover.md
  settings: output text (default); links text-and-url (default); max-length 2000 (positions/acme.toml:24:14); length-unit utf16 (default)
  fields: company Acme (positions/acme.toml); date 2026-11-20 (positions/acme.toml:23:8); recipient Hiring Team (positions/acme.toml:22:13); title Principal Architect, AI Platform (positions/acme.toml)
```

The letter and the resume are chosen apart. `[select]` is the resume's and `[letter.select]` is the letter's, and each is merged over the same profiles by the same rules. A profile can hold `[letter.select]` and nothing else of the letter, because a letter's content, recipient, and date describe one application. A pick of the same group name in each table picks that wording in each document, and neither is checked against the other's content: a letter pick that names nothing in the letter is an error that names the line that wrote it, and a resume pick the letter lacks is not. The letter has no `promote`, because it keeps its written order, and `drafts` is not a key here either.

What `[letter]` can say, each also a flag of [`resume letter`](#resume-letter):

| Key | The flag it matches | What it says |
|---|---|---|
| `content` | the `FILE` | The letter, a relative path from the spec's own directory. Required. |
| `recipient`, `date` | `--recipient`, `--date` | The recipient, and the date (`2026-11-20`). The date is never taken from the clock. |
| `style`, `paper` | `--style`, `--paper` | What a PDF is made with. Used only when a PDF is made. Without them, the `style` and `paper` under `[build]` are used, so the letter and resume of one application look alike, and the summary says which line chose them. |
| `output-format`, `links` | `--output-format`, `--links` | `text`, `pdf`, or `both`, and how links print. |
| `max-length`, `length-unit` | `--max-length`, `--length-unit` | A limit on the text, and what it counts. A unit needs a limit. |

The company and the title come from `[position]`, and nothing else. Neither document reads the other's content: `letter --spec` works when the baseline does not exist yet, and `build --spec` works when the letter does not. A mistake in the shape of the other document's table is still an error for both commands, so a typo is never hidden.

The PDF is named after the spec, with `-letter`: `positions/acme.toml` builds `out/acme-letter.pdf`, beside the resume's `out/acme.pdf`. That name is also the identity written into the PDF, so a viewer sees two documents. With `--drafts` the letter is written as `acme-letter.drafts.pdf`. A spec file whose name ends in `-letter` is refused by both commands, and so is a resume file with that ending, because its PDF would be written over the letter of another file.

Flags beside a spec follow the rule of [Flags beside a spec](#flags-beside-a-spec), and the list is every flag the spec can also say: `--tags`, `--core-only`, `--prefer`, `--style`, `--paper`, `--output-format`, `--max-length`, `--length-unit`, `--links`, `--company`, `--title`, `--recipient`, and `--date`. They are refused unless you add `--allow-overrides`, and the summary lists each with what it replaced and where that was written. The options about your machine, `--drafts`, and `--dry-run` are always allowed. With `--format json` and a PDF, one object with `type` `letter-spec` is written on standard output; it has the keys `file`, `position`, `chain`, `content`, `output`, `links`, `max_length`, `length_unit`, `fields`, `style`, `paper`, `drafts`, `pdf`, `overrides`, `environment`, and `version`. Each setting there is a `value` and where it came from (`from`: `flag`, `default`, or the line that wrote it).

`resume letter --spec positions/acme.toml --print-spec` prints the same resolved spec as `resume build --spec positions/acme.toml --print-spec`, letter included, and builds nothing.

Two runs of one spec give the same text and the same PDF bytes, with `--fonts bundled` for the PDF. The `company`, `title`, and `recipient` you send are in the summary and the JSON record, as for a resume.

### Rebuilding what you sent

Two builds of one spec are byte for byte the same PDF, and a build from a spec is the same as the build from the options it resolves to, when the file is named like the spec. That holds for the same version of the program, the same fonts, and the same style: the version is written into the PDF, and `--fonts auto` can use fonts installed on your machine. Build the copy you send with `--fonts bundled`, so it does not depend on the machine. Keep the spec, its profiles, the baseline, and the version of `resume` together, for example in one repository.

Nothing from `[position]`, the profiles' names, or the tags reaches the PDF: the PDF holds the resume's own text, the name for its title and author, the program's version, and an identity made from the spec's file name. The spec lines in the summary and the JSON record do hold the company, the title, and the posting address. They are printed to your terminal and to whatever captures its output, such as a log or a continuous-integration job.

A spec can name any file you can read as its `content`, and the summary prints the resolved path before anything is read from it. Build only specs you wrote or have read.

### Adding content for one application

A final position spec can add content without changing either baseline file.
Use `[[add]]` for a resume bullet, paragraph, or role blurb; use `[[letter.add]]` for a letter body paragraph.
Profiles cannot contain either array, even an empty one.

```toml additions
[[add]]
id = "application-result"
kind = "bullet"
place = { after = "gateway" }
content = '**Delivery:** Shipped the migration to 12 teams.'
tags = ["platform"]
priority = "core"

[[letter.add]]
id = "application-fit"
place = { at = "end" }
content = 'My platform experience fits the {{title}} role at {{company}}.'
```

Every addition needs an `id` that is unique within its document, including baseline item and wording-group names.
References always name the unchanged baseline, never another addition.
An override cannot target an addition; edit its declaration directly.
Before/after a wording group means before/after the whole group, even if selection later drops it.
Additions at the same original gap keep their declaration order.

Resume placement forms:

- A bullet may go before/after a bullet or bullet group, or at the start/end of a section, role body, or era.
- A paragraph may go before/after a paragraph or paragraph group, after a role blurb/group, or at the start/end of a section or role body.
- A blurb uses `place = { parent = "role-id", at = "blurb" }` and requires an empty blurb slot.
- Container boundaries use `place = { parent = "container-id", at = "start" }` or `at = "end"`.

Letter placement accepts `before` or `after` a body item/group, or `at = "start"` / `at = "end"` of the body.
Greeting and closing cannot be insertion targets.

Content must be one nonempty Markdown leaf, without a list marker for bullets.
The usual leading bold bullet label is supported.
Headings, lists, images, raw HTML, reference definitions, and annotation comments are rejected.
Put `tags`, `priority` (`core` or `extra`), and `status` (`draft` or `verify`) in the declaration instead.
Omitted metadata follows normal selection rules; a draft addition stays excluded even when marked core.
All fragments are checked when loading the spec, including drafts and the document not being built.

The baseline plus decoded additions may total at most 1 MiB per document, with drafts included.
The spec itself retains its existing 64 KiB limit.
Diagnostics for additions point at their content literal in the spec; decoded Markdown positions appear as context, not automatic edits.
`--print-spec` preserves authored content and array order without reading either baseline.

### Names a profile uses that the baseline does not

A tag or wording written in the spec itself that the baseline does not use is a warning that names the line, as for the options: it is probably a misspelling. A profile is shared by baselines that use different tags and wordings, and a `prefer` list means "where a group has it", so a name only a profile writes is a note, not a warning. The note names the profile and the line. The two codes are [`unknown-tag`](#code-unknown-tag) and [`unknown-wording`](#code-unknown-wording).

## Troubleshooting

**`build` says the style is unknown.** The name is in neither `--styles-dir` nor the built-in styles. Run `resume styles` to see the names, and check the spelling. `resume check` reports the same problem as [`unknown-style`](#code-unknown-style).

**The resume needs more pages than you want.** [`page-limit-exceeded`](#code-page-limit-exceeded) appears when you pass `--max-pages`. Cut content, or choose a style with tighter spacing. The PDF is still written so you can see where it overflows. If the last page is nearly empty, [`sparse-last-page`](#code-sparse-last-page) says so: trimming a little may bring the resume back to one fewer page.

**A role or era heading sits at the foot of a page.** [`heading-near-page-bottom`](#code-heading-near-page-bottom) means a heading starts near the foot of a page. The built-in styles never leave a heading alone there, because it moves to the next page with its first bullet, but it can still sit close to the foot with only a line or two under it. Add a page break before it, or tighten the text above so it moves up. A style of your own that keeps the shared components' markers but drops their keep-with-next behavior can strand a heading, and this finding catches that. A style with no markers is not checked at all.

**One block is taller than a page.** [`content-overflows-page`](#code-content-overflows-page): a single bullet, longer than a whole page, cannot be kept whole. Shorten it.

**The fonts are not the ones you expected.** Look at the `fonts:` line after a build, and see [Fonts](#fonts). The note [`font-fallback-metric`](#code-font-fallback-metric) is normal and harmless, and only `--format json` shows it; a warning, [`font-fallback-layout`](#code-font-fallback-layout), means the page count may change.

**`build` cannot write the PDF.** The `--out` path is a file, or is read-only; `resume check` reports it as [`output-not-writable`](#code-output-not-writable). Choose another directory with `-o`. On Windows, a PDF that is open in a viewer cannot be replaced; close it and build again.

**A style you edited fails.** [`typst-error`](#code-typst-error) carries Typst's own message with the line in your style. A style directory with no `style.typ` is an error from `build`, and `resume check` reports it as [`style-missing-main`](#code-style-missing-main).

**You are not sure what a finding means.** `resume doctor --explain CODE` explains any of them.

## Appendix: diagnostic codes

Every finding has a stable code. The tables below are generated from the program, and a test fails when they drift from it. Each says how serious a finding is by default and what it means; `resume doctor --explain CODE` gives the reasoning and an example.

<!-- guide:generated codes -->

### Format codes

How the Markdown is written. Every command that reads a resume reports these.

| Code | Severity | What it means |
|---|---|---|
| <a id="code-missing-name"></a>`missing-name` | error | The resume has no `# Name` heading. |
| <a id="code-duplicate-name"></a>`duplicate-name` | error | A resume has exactly one `# ` heading. |
| <a id="code-content-before-section"></a>`content-before-section` | error | Content appears before the first `## ` section. |
| <a id="code-role-line-shape"></a>`role-line-shape` | error | This `###` heading is not a role line. |
| <a id="code-role-separator"></a>`role-separator` | error | Role parts are separated by something other than ` · `. |
| <a id="code-empty-role-part"></a>`empty-role-part` | error | A part of the role line is empty. |
| <a id="code-era-shape"></a>`era-shape` | error | The bold line looks like an era but is not `**Name · Period**`. |
| <a id="code-era-outside-role"></a>`era-outside-role` | error | An era line is outside a role. |
| <a id="code-period-unparseable"></a>`period-unparseable` | error | The dates can't be read. |
| <a id="code-period-end-before-start"></a>`period-end-before-start` | error | The period ends before it starts. |
| <a id="code-period-hyphen"></a>`period-hyphen` | warning | Use an en dash with a space on each side between dates. |
| <a id="code-unsupported-html"></a>`unsupported-html` | error | Raw HTML can't be typeset. |
| <a id="code-invalid-link"></a>`invalid-link` | error | The link target is not a valid address. |
| <a id="code-unsupported-link"></a>`unsupported-link` | error | Only http, https, and mailto links are supported. |
| <a id="code-nested-list"></a>`nested-list` | warning | Nested bullets are flattened to one level. |
| <a id="code-role-order"></a>`role-order` | warning | Roles are not listed newest first. |
| <a id="code-mid-sentence-bold"></a>`mid-sentence-bold` | warning | Bold appears inside a sentence. |
| <a id="code-image"></a>`image` | warning | Images are not rendered. |
| <a id="code-too-many-contact-items"></a>`too-many-contact-items` | warning | The contact line has more than four items. |
| <a id="code-missing-contact"></a>`missing-contact` | warning | The name has no contact line. |
| <a id="code-period-present-spelling"></a>`period-present-spelling` | warning | Write `Present` for an ongoing role. |
| <a id="code-unsupported-markdown"></a>`unsupported-markdown` | error | This Markdown construct can't be represented. |
| <a id="code-spec-fragment-shape"></a>`spec-fragment-shape` | error | Spec content must be one Markdown leaf. |
| <a id="code-hard-break"></a>`hard-break` | warning | A hard line break is rendered as a space. |
| <a id="code-text-too-long"></a>`text-too-long` | error | A heading or role part is too long. |
| <a id="code-empty-heading"></a>`empty-heading` | error | A heading has no text. |
| <a id="code-input-limit"></a>`input-limit` | error | The input exceeds a size or nesting limit. |
| <a id="code-heading-marker-spacing"></a>`heading-marker-spacing` | warning | A line starts with `#` but has no space after it, so it is text, not a heading. |
| <a id="code-placeholder-unresolved"></a>`placeholder-unresolved` | error | A selected letter placeholder has no application field value. |
| <a id="code-annotation-unknown-keyword"></a>`annotation-unknown-keyword` | error | An `@` word in an annotation is not one the tool knows. |
| <a id="code-annotation-malformed"></a>`annotation-malformed` | error | An annotation token is not `@word`, `#tag`, `=group.variant`, or `&id` with valid names. |
| <a id="code-annotation-conflict"></a>`annotation-conflict` | error | One annotation gives two priorities, two statuses, or two alternates. |
| <a id="code-annotation-duplicate"></a>`annotation-duplicate` | error | An item has two annotations. |
| <a id="code-annotation-without-target"></a>`annotation-without-target` | error | An annotation on its own line has nothing after it to annotate. |
| <a id="code-annotation-detached"></a>`annotation-detached` | error | A blank line or another comment separates an annotation from the item below it. |
| <a id="code-annotation-misplaced"></a>`annotation-misplaced` | error | An annotation is where no annotation is allowed. |
| <a id="code-alternate-not-allowed"></a>`alternate-not-allowed` | error | An alternate wording is on a section heading, role line, or era line. |
| <a id="code-alternate-missing-default"></a>`alternate-missing-default` | error | A group of alternate wordings has no `default` variant. |
| <a id="code-alternate-not-adjacent"></a>`alternate-not-adjacent` | error | The variants of one group are apart, in different places, or of different kinds. |
| <a id="code-alternate-group-reused"></a>`alternate-group-reused` | error | Two separate groups of alternates use the same group name. |
| <a id="code-alternate-variant-repeated"></a>`alternate-variant-repeated` | error | A group has two variants with the same name. |
| <a id="code-alternate-priority-conflict"></a>`alternate-priority-conflict` | error | The variants of one group give different priorities. |
| <a id="code-item-id-reused"></a>`item-id-reused` | error | Two items share one `&id`, or an `&id` is also a group name. |
| <a id="code-item-id-on-alternate"></a>`item-id-on-alternate` | error | A variant of a group has an `&id`. |
| <a id="code-letter-missing-section"></a>`letter-missing-section` | error | The letter needs Greeting, Body and Closing sections in that order. |
| <a id="code-letter-unknown-section"></a>`letter-unknown-section` | error | The letter needs Greeting, Body and Closing sections in that order. |
| <a id="code-letter-duplicate-section"></a>`letter-duplicate-section` | error | The letter needs Greeting, Body and Closing sections in that order. |
| <a id="code-letter-section-order"></a>`letter-section-order` | error | The letter needs Greeting, Body and Closing sections in that order. |
| <a id="code-letter-section-shape"></a>`letter-section-shape` | error | The letter section has the wrong number of paragraphs. |
| <a id="code-letter-unsupported-block"></a>`letter-unsupported-block` | error | This construct is not supported in a cover letter. |
| <a id="code-placeholder-unknown"></a>`placeholder-unknown` | error | The letter placeholder names an unknown application field. |
| <a id="code-placeholder-malformed"></a>`placeholder-malformed` | error | The letter placeholder is malformed or explicitly escaped. |
| <a id="code-placeholder-position"></a>`placeholder-position` | error | A placeholder appears outside permitted letter text. |
| <a id="code-text-limit-exceeded"></a>`text-limit-exceeded` | error | The letter text exceeds its configured length limit. |
| <a id="code-letter-selection-required"></a>`letter-selection-required` | error | The annotated letter needs paragraph selection before it can be built. |
| <a id="code-letter-body-empty"></a>`letter-body-empty` | error | Selection leaves the letter body empty. |
| <a id="code-letter-frame-not-ready"></a>`letter-frame-not-ready` | error | The letter greeting or closing has no ready wording. |

### Content codes

What the resume says. `doctor` and `check` report these.

| Code | Severity | What it means |
|---|---|---|
| <a id="code-long-bullet"></a>`long-bullet` | warning | A bullet runs past about three lines. |
| <a id="code-unquantified-bullet"></a>`unquantified-bullet` | warning | A bullet in a role has no number. |
| <a id="code-weak-opener"></a>`weak-opener` | warning | A bullet opens with a phrase that hides what you did. |
| <a id="code-first-person"></a>`first-person` | warning | A bullet or summary speaks in the first person. |
| <a id="code-sloppy-phrase"></a>`sloppy-phrase` | warning | A phrase like "etc." or "various" says nothing. |
| <a id="code-long-summary"></a>`long-summary` | warning | The summary is longer than about 120 words. |
| <a id="code-education-not-last"></a>`education-not-last` | warning | Education comes before experience. |
| <a id="code-old-role-detail"></a>`old-role-detail` | warning | An old role has more bullets than your latest one. |
| <a id="code-alternate-facts-differ"></a>`alternate-facts-differ` | warning | The numbers in the variants of one group differ. |

### Layout codes

How the rendered document looks. `build`, `doctor`, and `check` report these.

| Code | Severity | What it means |
|---|---|---|
| <a id="code-page-limit-exceeded"></a>`page-limit-exceeded` | error | The resume runs past the page limit given with `--max-pages`. |
| <a id="code-heading-near-page-bottom"></a>`heading-near-page-bottom` | warning | A role or era heading sits at the foot of a page with almost nothing under it. |
| <a id="code-sparse-last-page"></a>`sparse-last-page` | warning | The last page is less than 15% full. |
| <a id="code-content-overflows-page"></a>`content-overflows-page` | error | A block of content is taller than a page and runs off it. |
| <a id="code-font-fallback-metric"></a>`font-fallback-metric` | note | A bundled font stands in for the template's font; layout is unchanged. |
| <a id="code-font-fallback-layout"></a>`font-fallback-layout` | warning | A bundled font with different character widths stands in; line breaks may change. |
| <a id="code-font-missing"></a>`font-missing` | error | Neither the preferred font nor its bundled fallback is available. |

### Setup codes

The environment: styles, fonts, and the output location. `doctor` and `check` report these.

| Code | Severity | What it means |
|---|---|---|
| <a id="code-unknown-style"></a>`unknown-style` | error | No style of this name exists in --styles-dir or among the built-in styles. |
| <a id="code-style-missing-main"></a>`style-missing-main` | error | The style directory has no style.typ, so it cannot be rendered. |
| <a id="code-styles-dir-unusable"></a>`styles-dir-unusable` | error | The --styles-dir cannot be read. |
| <a id="code-font-path-unusable"></a>`font-path-unusable` | error | A --font-path or RESUME_FONT_PATH entry cannot be read. |
| <a id="code-output-not-writable"></a>`output-not-writable` | error | The output directory cannot be created or written, so build would fail. |
| <a id="code-typst-error"></a>`typst-error` | error | The style's Typst code failed to compile. |
| <a id="code-typst-warning"></a>`typst-warning` | warning | Typst reported a warning while rendering the style. |
| <a id="code-unknown-tag"></a>`unknown-tag` | warning | A tag given with --tags, or written in a spec, is not used anywhere in the resume. |
| <a id="code-unknown-wording"></a>`unknown-wording` | warning | A wording given with --prefer, or written in a spec, is not a variant of any group in the resume. |

<!-- /guide:generated -->

## Where to read more

- [The architecture notes](../ref/Resume-RS-Architecture.md): why the program is built the way it is. For people changing it.
- [The pragmatic style's README](../styles/pragmatic/README.md), [the mono style's README](../styles/mono/README.md), and [the experienced style's README](../styles/experienced/README.md): how each differs from the template it follows, and why.
- [Font licenses](../fonts/LICENSES.md): the notices for the bundled fonts.
- [The Typst reference](https://typst.app/docs/reference/): everything about writing a style.

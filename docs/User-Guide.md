<!-- ABOUTME: The user guide for resume-rs: writing a resume, checking it, fonts, styles, and every command. -->
<!-- ABOUTME: Tests in tests/guide keep it true: they compare its tables, examples, and headings with the code. -->

# resume-rs User Guide

`resume` turns a resume written in Markdown into a print-quality PDF. You write the content once. A **style** decides how it looks. A built-in checker, `doctor`, tells you what is wrong with the resume and why it matters, and fixes what is safe to fix.

This guide is for the person using the tool. It says what the tool does and how to ask for it. Why it is built that way, and how, is in the [architecture notes](../ref/Resume-RS-Architecture.md), which are for people changing the tool.

How to read it: start with the [quick start](#quick-start), then [writing your resume](#writing-your-resume). Come back to [the commands](#commands) and [the styles](#styles) when you need them. A line like `period-hyphen` in this guide is the name of a finding; it links to [the list of every finding](#appendix-diagnostic-codes), and `resume doctor --explain period-hyphen` explains it in your terminal.

Two conventions help when you read. A section marked **Planned, not available yet** describes work that has not been done; nothing in it works today. And every example in a `markdown` block on this page is checked by the project's tests: it produces exactly the findings the text says it does.

## Quick start

Save this as `Resume.md`. It is a whole resume, small enough to read at a glance, and it builds as written.

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
6. `--fonts bundled --max-pages 2` uses only the fonts inside the program, so the PDF is the same on any machine, and fails if the resume needs more than two pages.
7. `export-style` copies a built-in style into `my-styles` so you can edit it.
8. `--styles-dir my-styles` builds with your copy.
9. `doctor --explain` explains one finding, with an example.

Something went wrong? See [troubleshooting](#troubleshooting).

## Writing your resume

A resume is a Markdown file with a small number of conventions. The program reads these conventions, not general Markdown: anything it cannot represent is reported, never dropped silently.

Here is a whole resume that uses every convention. The comment lines explain each part. Comments on their own lines are ignored when the PDF is built, so they are safe to leave in.

```markdown resume expect=none
<!-- The name: exactly one `#` heading. It is the title of the PDF and the text of the footer. -->
# Ada Lovelace

<!-- The contact line: the paragraph right after the name. Separate the items with ` · `. Keep it to four. -->
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

**The contact line** is the paragraph right after the name. Separate the items with ` · ` (a middle dot with a space on each side; Option+Shift+9 on a Mac). The book recommends at most four, and `doctor` warns above that with [`too-many-contact-items`](#code-too-many-contact-items). Links need an `http`, `https`, or `mailto` address.

**Sections** are `## Title`. Everything up to the next `##` belongs to the section. Only the name and the contact line may come before the first section.

**Roles** are `### Company · Title · Period`, or `### Company · Period` when there is no title. Each part needs text, and the separator is exactly ` · `. A period is recognized by a four-digit year in it.

**The description** is the first paragraph after a role line, if it is not an era. It is the one line that says what the company does.

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

Dates joined by a hyphen are [`period-hyphen`](#code-period-hyphen), a warning with a mechanical fix:

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

A label with its colon outside the bold is [`mid-sentence-bold`](#code-mid-sentence-bold), a warning `doctor --fix` repairs:

```markdown in-section expect=mid-sentence-bold
- **Languages**: Rust, Java, Scala
```

### What is not supported

The program tells you rather than guessing. These are errors, not silent omissions: block quotes, code blocks, tables, headings below `###`, numbered lists, and a bullet with more than one paragraph. Images are dropped with a warning, because the book advises against photos. An input larger than 1 MiB is rejected with [`input-limit`](#code-input-limit).

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

**`doctor --fix`** applies the changes that are mechanical and keep the meaning, such as replacing a hyphen with an en dash or moving a colon inside a bold label. It never rewrites your words. Add `--dry-run` to see the diff first. A finding that can be fixed this way is marked `fixable` in the report.

**`doctor --explain CODE`** prints what a finding means, why it matters (with the book's reasoning), how to fix it, and a before-and-after example. Every example it prints is one that really produces its code.

**`--format json`** writes each finding as one JSON object per line on standard output, for scripts. Operational problems (a file that cannot be read, an unknown style) are still plain lines on standard error with exit code 1, so standard output stays machine-readable.

**Exit codes.** 0 means success, and warnings are allowed. 1 means an error finding, a failed build, or an operational error such as an unreadable file. 2 means the command line itself was wrong.

## Fonts

A style names fonts by **role**, not by file. Every built-in style has a `body` role and a `mono` role, and a third role for its display type:

- `body`: the text of the resume.
- `mono`: inline code.
- `display`: the name and the headings, in `pragmatic` and `mono`.
- `label`: the name and the section labels, in `experienced`.

Each role has three fields in the style's theme, described in [the theme keys](#theme-keys):

- `preferred`: the font the template itself uses, such as Calibri. It is used when it is installed.
- `fallback`: a free font bundled inside the program, used when the preferred one is not installed.
- `metric-compatible`: `true` when the fallback has the same character widths as the preferred font, so lines break in the same places either way.

The bundled fallbacks are Carlito (for Calibri), Gelasio (for Georgia), Arimo (for Arial), and TeX Gyre Pagella (for Book Antiqua). A style that prefers Calibri and falls back to Carlito produces the same line breaks and the same page count on a machine that has Calibri and on one that does not: the difference is only in the shapes of the letters. TeX Gyre Pagella is the exception. Its character widths differ from Book Antiqua's by about 2% on average, so the `experienced` style, which sets its name and labels in it, can break those lines differently when Book Antiqua is not installed. The style says so with `metric-compatible: false`, and a build reports [`font-fallback-layout`](#code-font-fallback-layout).

**Which fonts may be used** is up to you:

- `--fonts auto` (the default) uses the bundled fonts, then any in `--font-path` and in the `RESUME_FONT_PATH` environment variable, then the fonts installed on the machine.
- `--fonts bundled` uses only the bundled fonts. The PDF no longer depends on the machine: the same input gives the same bytes on macOS and Windows. Use it when you want to be sure.
- `--font-path <DIR>` adds a directory of font files. Give it more than once for more directories.

After a build, the `fonts:` line says what was used, for example `body Carlito (fallback for Calibri)`. Three findings describe the situation:

- [`font-fallback-metric`](#code-font-fallback-metric) is a note: a bundled font stands in and the layout is unchanged. Nothing to do.
- [`font-fallback-layout`](#code-font-fallback-layout) is a warning: the stand-in has different widths, so lines may break differently. Install the preferred font or check the page count.
- [`font-missing`](#code-font-missing) is an error: neither the preferred font nor its fallback is available, which means the style's font list was edited wrongly.

**Licenses.** The bundled fonts are free fonts under the SIL Open Font License, and the program carries a notice for each. `resume licenses` prints them. They are also in [`fonts/LICENSES.md`](../fonts/LICENSES.md).

## Styles

A style decides how a resume looks. The content never changes between styles. The program has three built-in styles, and you can make your own.

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

### Theme keys

Every value a built-in theme sets is listed here. A theme is a Typst dictionary, and `spacing.section` means the `section` entry of `spacing`. Lengths are in Typst units: `pt` (points), `in` (inches), `em` (a multiple of the font size). The **Styles** column says which built-in styles have the key. The styles do not all have the same keys: `experienced` keeps its page margins in `page-margin` and has no `margin`, for example.

| Key | Unit | What it controls | Styles |
|---|---|---|---|
| `fonts.<role>.preferred` | font name | The template's own font for the role, used when installed. The roles are `body`, `mono`, and `display` or `label`. In `pragmatic` and `mono`, set `display` to Georgia, with Gelasio as its fallback, for a serif look. | all |
| `fonts.<role>.fallback` | font name | The bundled font used when the preferred one is not installed. | all |
| `fonts.<role>.metric-compatible` | `true` or `false` | Whether the fallback has the preferred font's character widths. When `false`, `doctor` warns that lines may break differently. | all |
| `size.body` | length | The size of the text. | all |
| `size.name` | length | The size of the name. | all |
| `size.heading` | length | The size of the section titles. | mono, pragmatic |
| `size.role` | length | The size of the role line: title, company, and period. | all |
| `size.contact` | length | The size of the contact items. | mono, pragmatic |
| `size.footer` | length | The size of the footer text. | all |
| `color.text` | color | The color of the text. | all |
| `color.accent` | color | The color of section titles and the rule under them. | all |
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
| `header.gap` | length | The space between the name and the contact line, beyond the 17 pt that the two lines' own heights give. | mono |
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

Changing `line.top` and `line.bottom` changes the page count and where lines break. Everything else changes only the look.

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

It needs a `theme.typ` beside it, which you can copy from a built-in style, and the shared `lib/components.typ`. `export-style` gives you both.

Typst has its own reference, which is the place to learn how to arrange things on a page: [text](https://typst.app/docs/reference/text/text/), [block](https://typst.app/docs/reference/layout/block/), [grid](https://typst.app/docs/reference/layout/grid/), [lists](https://typst.app/docs/reference/model/list/), [set rules](https://typst.app/docs/reference/styling/#set-rules), and [show rules](https://typst.app/docs/reference/styling/#show-rules).

## Commands

```text
resume [--dry-run] [--format FORMAT] [--color COLOR] COMMAND
```

### Global options

These work with every command.

| Option | Values and default | What it does |
|---|---|---|
| `--dry-run` | a switch | Shows what would be written and writes nothing. For `build` it still compiles and runs every check, so the exit code is a real build's. For `doctor --fix` it prints the diff. For `export-style` it lists the files. `styles`, `check`, and `licenses` write nothing and ignore it. |
| `--format <FORMAT>` | `human` or `json`; default `human` | How findings are written: for people, or as JSON lines on standard output. `styles`, `licenses`, and `export-style` have no findings and ignore it. |
| `--color <COLOR>` | `auto`, `always`, or `never`; default `auto` | When to color the output. `auto` colors only on an interactive terminal, and `NO_COLOR` is respected. |

### resume build

Typesets one or more resumes into PDFs. The PDF is named after the input file and written to the output directory, which is created if it does not exist. If the resume has format errors, nothing is written. If it compiles but a layout check fails, such as the page limit, the PDF is still written so you can see what overflowed, and the command exits 1. With several files, each is built and each gets its own summary line; one failing never skips the others.

```sh
resume build Resume.md
resume build Resume.md Other.md --out pdfs
```

| Option | Values and default | What it does |
|---|---|---|
| `<FILE>...` | one or more Markdown files | The resumes to build. |
| `-o, --out <DIR>` | default `out` | The directory the PDFs are written to. |
| `-s, --style <NAME>` | default `pragmatic` | The style to render with. |
| `--styles-dir <DIR>` | none | A directory of styles to use besides the built-in ones. A style here wins over a built-in of the same name. |
| `--paper <PAPER>` | `letter` or `a4`; default `letter` | The paper size. |
| `--max-pages <N>` | 1 to 20 | Fail when the resume needs more pages than this. |
| `--fonts <FONTS>` | `auto` or `bundled`; default `auto` | Which fonts may be used. See [Fonts](#fonts). |
| `--font-path <DIR>` | none; repeatable | A directory of fonts to use when `--fonts` is `auto`. Give it more than once. It cannot be combined with `--fonts bundled`. |

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
| `--max-pages <N>` | 1 to 20 | Report a layout error when the resume needs more pages than this. |
| `--fonts <FONTS>` | `auto` or `bundled`; default `auto` | Which fonts may be used. |
| `--font-path <DIR>` | none; repeatable | A directory of fonts to use when `--fonts` is `auto`. |
| `--fix` | a switch | Apply the changes that are mechanical and keep the meaning, to the files themselves. Add `--dry-run` to see the diff and change nothing. |
| `--explain <CODE>` | a finding's code | Explain a finding by its code, with an example, and exit. |

### resume check

The same report as `doctor`, which can never change a file. It has the options of `doctor` except `--fix` and `--explain`. Use it in a script or a pre-commit hook.

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
| `--max-pages <N>` | 1 to 20 | Report a layout error when the resume needs more pages than this. |
| `--fonts <FONTS>` | `auto` or `bundled`; default `auto` | Which fonts may be used. |
| `--font-path <DIR>` | none; repeatable | A directory of fonts to use when `--fonts` is `auto`. |

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
| `--force` | a switch | Replace files that already exist. Without it, nothing is written when any file exists. |

### resume licenses

Prints the license notices of the fonts built into the program, to standard output. It takes no options of its own.

```sh
resume licenses
```

### Planned commands

<!-- guide:planned rsm-141.12 witness=command:watch -->
<!-- guide:planned rsm-141.12 witness=command:export -->

**Planned, not available yet** (rsm-141.12). A `watch` command that rebuilds when the file changes, and an `export` command that writes the resume data and the style for use with Typst's own tools. Neither exists today; the commands above are the whole program.

### Environment variables

`RESUME_FONT_PATH` adds font directories, like `--font-path`. `NO_COLOR` turns off color, like `--color never`. Nothing is read from the current directory unless you ask: styles come from `--styles-dir`, and there is no configuration file.

## Annotations

<!-- guide:planned rsm-141.22 witness=build:--tags -->

**Planned, not available yet** (epic rsm-141.22). Annotations will let one baseline resume produce several tailored resumes, by marking which items are essential and which suit which kind of job. This section will describe the syntax and the options of `build` that use it. Until then, a comment in your resume is ignored.

## Profiles and position specs

<!-- guide:planned rsm-141.23 witness=build:--spec -->

**Planned, not available yet** (epic rsm-141.23). A profile will be a reusable set of choices for a kind of position, and a position spec the choices for one application, so that a tailored resume can be rebuilt exactly. This section will describe both file formats and how they combine. Until then, nothing reads such files.

## Troubleshooting

**`build` says the style is unknown.** [`unknown-style`](#code-unknown-style): the name is in neither `--styles-dir` nor the built-in styles. Run `resume styles` to see the names, and check the spelling.

**The resume needs more pages than you want.** [`page-limit-exceeded`](#code-page-limit-exceeded) appears when you pass `--max-pages`. Cut content, or choose a style with tighter spacing. The PDF is still written so you can see where it overflows. If the last page is less than 15% full, [`sparse-last-page`](#code-sparse-last-page) says so: trimming a little may bring the resume back to one fewer page.

**A role or era heading sits at the foot of a page.** [`heading-near-page-bottom`](#code-heading-near-page-bottom) means a heading starts in the bottom 8% of a page. The built-in styles never leave a heading alone there, because it moves to the next page with its first bullet, but it can still sit close to the foot with only a line or two under it. Add a page break before it, or tighten the text above so it moves up. A style of your own that does not use the shared components can strand a heading completely, which this finding also catches.

**One block is taller than a page.** [`content-overflows-page`](#code-content-overflows-page): a single bullet, longer than a whole page, cannot be kept whole. Shorten it.

**The fonts are not the ones you expected.** Look at the `fonts:` line after a build, and see [Fonts](#fonts). A note, [`font-fallback-metric`](#code-font-fallback-metric), is normal and harmless; a warning, [`font-fallback-layout`](#code-font-fallback-layout), means the page count may change.

**`build` cannot write the PDF.** [`output-not-writable`](#code-output-not-writable): the `--out` path is a file, or is read-only. Choose another directory with `-o`. On Windows, a PDF that is open in a viewer cannot be replaced; close it and build again.

**A style you edited fails.** [`typst-error`](#code-typst-error) carries Typst's own message with the line in your style. [`style-missing-main`](#code-style-missing-main) means the style directory has no `style.typ`.

**You are not sure what a finding means.** `resume doctor --explain CODE` explains any of them.

## Appendix: diagnostic codes

Every finding has a stable code. The tables below are generated from the program, so they are always current. Each says how serious a finding is by default and what it means; `resume doctor --explain CODE` gives the reasoning and an example.

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
| <a id="code-hard-break"></a>`hard-break` | warning | A hard line break is rendered as a space. |
| <a id="code-text-too-long"></a>`text-too-long` | error | A heading or role part is too long. |
| <a id="code-empty-heading"></a>`empty-heading` | error | A heading has no text. |
| <a id="code-input-limit"></a>`input-limit` | error | The input exceeds a size or nesting limit. |
| <a id="code-heading-marker-spacing"></a>`heading-marker-spacing` | warning | A line starts with `#` but has no space after it, so it is text, not a heading. |
| <a id="code-placeholder-unresolved"></a>`placeholder-unresolved` | error | A selected letter placeholder has no application field value. |

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

<!-- /guide:generated -->

## Where to read more

- [The architecture notes](../ref/Resume-RS-Architecture.md): why the program is built the way it is. For people changing it.
- [The pragmatic style's README](../styles/pragmatic/README.md) and [the mono style's README](../styles/mono/README.md): how each differs from the template it follows, and why.
- [Font licenses](../fonts/LICENSES.md): the notices for the bundled fonts.
- [The Typst reference](https://typst.app/docs/reference/): everything about writing a style.

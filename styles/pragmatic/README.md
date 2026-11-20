# pragmatic

The book's default layout: a three-part header, ruled section headings, and one line per role with the
period on the right. It is modeled on The Pragmatic Engineer's resume template (also the layout of
Standard Resume's Parker), set in Calibri when installed and the bundled, metric-compatible Carlito
otherwise.

```
City, Country                 Ada Lovelace                 +44 20 7946 0000
linkedin.com/in/ada                                         ada@example.com

SUMMARY ─────────────────────────────────────────────────────────────────
• One bullet per achievement, never split across pages

EXPERIENCE ──────────────────────────────────────────────────────────────
Principal Engineer · Analytical Engines Ltd                  Aug 2017 – Present
Builds mechanical computers for scientific customers.
```

## Files

| File | What it holds |
|---|---|
| `style.typ` | Structure: the header and the loop over sections and blocks. |
| `theme.typ` | Values: font roles, sizes, colors, spacing, margins, line pitch. |
| `../lib/components.typ` | Shared by every style: inlines, bullets, role and era lines, footer, probes. |

To customize, run `resume export-style pragmatic <dir>` and edit `theme.typ` in the copy, then build with
`--styles-dir <dir>`.

## The theme and the guarantees

Every key of `theme.typ` is described in the guide's [theme keys](https://github.com/dmrolfs/resume-rs/blob/main/docs/User-Guide.md#theme-keys), and what the shared
components guarantee (a heading never ends a page, a bullet never splits, the output does not depend on the
date or the language) is in its [shared components](https://github.com/dmrolfs/resume-rs/blob/main/docs/User-Guide.md#shared-components). Two things are particular to
this style:

- **`color.accent`** is the section title color and the rule under it, `#1155cc` as in the template.
- **`header`**: `name-gap` is the least clear space between the name and either side. When the name, the
  wider side, and two gaps do not fit the line, the name moves onto a line of its own above the sides, and
  `stacked-gap` is the space under it.

## Header contact items

The contact line is split at ` · `. A `mailto:` link, or plain text that looks like an address, is an email;
text that looks like a phone number (digits with `+ ( ) . -` and an optional `ext`) is a phone. Both go on
the right, phone first. Everything else is a location, or a link when it contains one, and goes on the left,
locations first. An item that matches nothing is treated as a location, so nothing is dropped.

## Limits

- A bullet taller than a page cannot be kept whole. It stays unbreakable and overflows the page, and the
  `resume build` layout check (rsm-141.10) reports it.
- A page break directly after a section heading leaves the heading alone on its page; the parser reports
  an empty section, so this only happens with a section that has no content.
- `lang` is fixed at `en-US` so output does not depend on the machine. Resumes in another language need a
  different style.
- Inline kinds, block kinds, and resume schema versions the style does not know stop the render with an
  error instead of dropping content.

## Where it differs from the template, and why

Chosen in the visual review against the template's PDF (rsm-141.7.3):

| Setting | Template | Here | Why |
|---|---|---|---|
| Text sizes | body 11pt, role line 12pt, headings 14pt, contact 12pt | the same | |
| Line pitch | 1.22em | 1.32em (`line.leading: 0.1em`) | the template's density is tiring to read |
| Spacing between blocks and margins | roomy | similar (`spacing`, `margin`) | |
| Company on the role line | centered column | after the title (`Title · Company`) | long titles crowd a centered column |
| Summary | no heading | a `Summary` heading | screening software finds sections by heading |
| Bullet indent | 0.25in marker, 0.5in text | 0.15in marker, 0.15in gap | keeps line width |
| Links | blue in the body, black in the header | blue (`#1155cc`) and underlined everywhere | black links read as a font style |
| Skill lists | tab-aligned labels | bold labels in the line | a tab column breaks on long labels |
| Footer | none | name and `Page N of M` | helps on a multi-page resume |
| Font | Calibri | Calibri when installed, else Carlito | same line breaks either way |

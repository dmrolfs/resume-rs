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

## The settings that matter in `theme.typ`

- **`fonts`**: one entry per role (`body`, `display`, `mono`). `preferred` is the template's own font, used
  when installed. `fallback` is a free bundled font. `metric-compatible: true` promises the fallback has the
  same character widths, so line breaks do not change when it stands in. Carlito matches Calibri. The `mono`
  role names the same family twice: DejaVu Sans Mono is bundled by Typst, so nothing stands in for it.
  `display` is the font of the name and the section headings; set it to Georgia and Gelasio for a serif look.
- **`line`**: the line pitch is set from explicit lengths, not from font metrics. Calibri and Carlito have
  different cap heights, and with Typst's default text edges their lines would sit at different heights.
  Keep `top` positive and `bottom` negative (`bottom: 0.269em` is silently clamped).
- **`margin`**: the bottom margin leaves room for the footer.
- **`color.accent`**: the section heading color and rule (`#1155cc`, from the template).
- **`header`**: `name-gap` is the least clear space between the name and either side. When the name, the
  wider side, and two gaps do not fit the line, the name moves onto a line of its own above the sides, and
  `stacked-gap` is the space under it.

## Guarantees (from the shared components)

- A role heading never ends a page: it moves with the next block (`block(sticky: true)`).
- A bullet never splits across pages (`block(breakable: false)`).
- Output does not depend on the date, the language, or hyphenation: `lang`, `region`, `hyphenate`, and
  `ligatures` are fixed.

## The whole theme

`size` (`body`, `name`, `heading`, `footer`), `color` (`text`, `accent`, `link`, `muted`), `margin`
(`x`, `top`, `bottom`), `footer-descent`, `spacing` (`section`, `section-below`, `role`, `role-line-below`,
`era`, `era-line-below`, `blurb`, `bullet`, `paragraph`), `rule` (`weight`, `gap`), and `bullet`
(`marker`, `indent`, `gap`). Every key is read; a missing one is an error from Typst naming it.

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

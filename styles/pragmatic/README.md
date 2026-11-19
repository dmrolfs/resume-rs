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
  same character widths, so line breaks do not change when it stands in. Carlito matches Calibri.
- **`line`**: the line pitch is set from explicit lengths, not from font metrics. Calibri and Carlito have
  different cap heights, and with Typst's default text edges their lines would sit at different heights.
  Keep `top` positive and `bottom` negative (`bottom: 0.269em` is silently clamped).
- **`margin`**: the bottom margin leaves room for the footer.
- **`color.accent`**: the section heading color and rule (`#1155cc`, from the template).
- **`header.name-share`**: how wide the name may be, as a fraction of the line, before it moves above the
  two sides instead of between them.

## Guarantees (from the shared components)

- A role heading never ends a page: it moves with the next block (`block(sticky: true)`).
- A bullet never splits across pages (`block(breakable: false)`).
- Output does not depend on the date, the language, or hyphenation: `lang`, `region`, `hyphenate`, and
  `ligatures` are fixed.

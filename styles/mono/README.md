# mono

The layout for people whose job titles carry the weight: a left-aligned name with one contact line under it,
capitalized section headings, and a role line that leads with the title. It is modeled on The Mono Engineer's
resume template (a slight variation of the Pragmatic Engineer's, also the layout of Standard Resume's
Alexander), set in Calibri when installed and the bundled, metric-compatible Carlito otherwise.

```
Ada Lovelace
London, UK · +44 20 7946 0000 · ada@example.com · linkedin.com/in/ada

SUMMARY ─────────────────────────────────────────────────────────────────
• One bullet per achievement, never split across pages

EXPERIENCE ──────────────────────────────────────────────────────────────
Principal Engineer · Analytical Engines Ltd                  Aug 2017 – Present
Builds mechanical computers for scientific customers.
```

## Files

| File | What it holds |
|---|---|
| `style.typ` | Structure: the header, the capitalized section title, the title-first role line, and the loop over sections and blocks. |
| `theme.typ` | Values: font roles, sizes, colors, spacing, margins, line pitch. |
| `../lib/components.typ` | Shared by every style: inlines, bullets, role and era lines, footer, probes. |

To customize, run `resume export-style mono <dir>` and edit `theme.typ` in the copy, then build with
`--styles-dir <dir>`.

## How it differs from pragmatic

- The name is on the left and the contact items are one line under it (location, phone, email, then links),
  not split around the name.
- Section titles are set in capitals, in the accent color, with a little extra letter spacing (`tracking.heading`).
- The role line is **Title** · Company: the title is bold, and the company and the period are regular weight.
  A role without a title shows its company in bold.
- Text is near black (`#1a1a1a`), not pure black.

Everything else comes from the shared components, so the same guarantees hold: a role heading never ends a
page, a bullet never splits across pages, and line positions are the same with Calibri and Carlito. The theme
keys are described in the pragmatic README; `header.gap` and `tracking.heading` are mono's own.

## Limits

The same limits as pragmatic: a bullet taller than a page overflows, and `lang` is fixed at `en-US`.

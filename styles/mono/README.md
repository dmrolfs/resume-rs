# mono

The Mono Engineer's resume template from The Tech Resume Inside Out: black on white, a centered name with one
contact line under it, and capitalized section titles over a black rule. It is the Pragmatic template's body with
a different header and no color, and it suits people whose companies or titles should carry the weight. Set in
Calibri when installed and the bundled, metric-compatible Carlito otherwise.

```
                              Ada Lovelace
        ada@example.com · +44 20 7946 0000 · London, UK · linkedin.com/in/ada

SUMMARY ─────────────────────────────────────────────────────────────────
• One bullet per achievement, never split across pages

EXPERIENCE ──────────────────────────────────────────────────────────────
Principal Engineer · Analytical Engines Ltd                  Aug 2017 – Present
Builds mechanical computers for scientific customers.
```

## Files

| File | What it holds |
|---|---|
| `style.typ` | Structure: the centered header, the capitalized section title, and the loop over sections and blocks. |
| `theme.typ` | Values: font roles, sizes, colors, spacing, margins, line pitch. |
| `../lib/components.typ` | Shared by every style: inlines, bullets, role and era lines, footer, probes. |

To customize, run `resume export-style mono <dir>` and edit `theme.typ` in the copy, then build with
`--styles-dir <dir>`.

## How it differs from pragmatic

- The name and the contact line are centered. The contact items are one line in the book's order: email, phone,
  location, then links. Pragmatic splits them around the name.
- Section titles are set in capitals, in black bold over a black rule. `color.accent` is black, so nothing on the
  page is colored except the links, which stay blue as in pragmatic.
- The name is 24 pt, the section titles 12 pt (pragmatic: 14 pt), and the contact line 11 pt (pragmatic: 12 pt), as in the
  template. The contact line sits 17 pt under the name (`header.gap: 0pt`), as in the template.
- The capitals are real text in the PDF (`SUMMARY`, not `Summary`), and `ß` becomes `SS`.

Everything else is shared with pragmatic and was approved for it: Calibri/Carlito at 11 pt, the role line with
the title, company, and period all bold, company after the title, the `Summary` heading, bullets at 0.15 in, the
spacing, line pitch, and footer. The guarantees are shared too: a role heading never ends a page, a bullet never
splits across pages, and line positions are the same with Calibri and Carlito. The theme keys are described in
the pragmatic README; `header.gap` is mono's own.

## Where it differs from the template, and why

| Setting | Template | Here | Why |
|---|---|---|---|
| Company on the role line | centered column, with the team and location on a second line | after the title (`Title · Company`) | long titles crowd a centered column; the same choice as pragmatic |
| Summary | no heading | a `Summary` heading | screening software finds sections by heading |
| Links | black, underlined | blue, underlined | black links read as a font style |
| Contact order | as written | email, phone, location, links | the book's order, kept whatever the order in the file |
| Bullets, spacing, line pitch | roomy, large bullets | as approved for pragmatic | |
| Skill lists | tab-aligned labels | bold labels in the line | a tab column breaks on long labels |
| Footer | none | name and `Page N of M` | helps on a multi-page resume |

## Limits

- The contact line is split at ` · ` only. A line that is too wide wraps between items; an item is never
  broken, so one wider than the line overflows it.
- The same limits as pragmatic: a bullet taller than a page overflows, and `lang` is fixed at `en-US`.

# experienced

Two zones: the section label in a gutter at the left, and the content in the column beside it. It is modeled
on The Experienced Engineer's resume template (the layout of Standard Resume's Venables): the name, the
company before the title, and a skills table of labels and values. It is set in Calibri when installed and the
bundled, metric-compatible Carlito otherwise; the name and the labels are set in Book Antiqua when installed and
the bundled TeX Gyre Pagella otherwise.

```
                            Ada Lovelace
              ada@example.com • +44 20 7946 0000 • London, UK
                       LinkedIn • GitHub

SUMMARY         Principal engineer who designs distributed systems.
                • One bullet per achievement, never split across pages

SKILLS          Languages:                 Rust, Java, Scala
                Cloud and infrastructure:  AWS (EKS, EC2), Kubernetes

EXPERIENCE      Analytical Engines Ltd, Principal Engineer      Aug 2017 – Present
                Builds mechanical computers for scientific customers.
                • One bullet per achievement
```

## Files

| File | What it holds |
|---|---|
| `style.typ` | Structure: the header, the gutter label, the role line, the skills table, and the section rows. |
| `theme.typ` | Values: font roles, sizes, colors, spacing, the gutter, page margins, line pitch. |
| `../lib/components.typ` | Shared by every style: inlines, bullets, era lines, footer, probes. |

To customize, run `resume export-style experienced <dir>` and edit `theme.typ` in the copy, then build with
`--styles-dir <dir>`.

## The settings that matter in `theme.typ`

- **`fonts`**: `body`, `label` (the name and the section labels), and `mono`. `preferred` is the template's own
  font, used when installed, and `fallback` a free bundled font. Carlito has Calibri's character widths. TeX Gyre
  Pagella does not have Book Antiqua's: its widths differ by about 2.4% on average, and the role says so with
  `metric-compatible: false`. A build that falls back to Pagella reports `font-fallback-layout` (a warning),
  because line breaks in the name and the labels can differ. Install Book Antiqua, or pass its files with
  `--font-path`, for the template's own breaks.
- **`gutter`** (`width: 113.15pt`, `inset: 5.9pt`, `gap: 5.95pt`) and **`margin`**: the gutter is part of the left
  page margin (`margin.left` is the page margin plus the gutter plus the gap), so every block starts in the text
  column. The labels, the name, the contact lines and the footer reach back over it; a label is inset by `inset`
  from the page margin. The three numbers are the template's: measured in its PDF, labels start 41.9pt from the
  page edge and the content 155.1pt. `footer-bleed` is how far; keep it equal
  to `gutter.width + gutter.gap`.
- **`skills`**: the label column of the skills table is as wide as its longest label, up to `label-max`; a
  longer label wraps. `column-gap` is the space between the label and the value.
- **`line`**: the line pitch is set from explicit lengths, not from font metrics, so Calibri and Carlito give
  the same line positions. Keep `top` positive and `bottom` negative.
- **`color.accent`**: `#cc0000`, from the template; used for links (`color.link`). The labels are black.

## How the layout works

- A section's label and the first thing in it (a block, or for a role its heading and blurb) share one grid row.
  The label is level with that first line, and a label can never end a page alone. The rest of the section
  follows in the text column.
- A block of bullets in which every bullet has a label (`- **Languages:** Rust`) is a skills table: one row per
  bullet. If one bullet has no label the block is a list.
- The role line is `Company, Title` in bold with the bold period on the right; a role without a title shows the
  company alone.
- The header is three centered lines: the name, then email, phone and location, then the links. The lines sit one
  line pitch apart, with no extra space under the name.
- A page break that opens a section comes before its label, so the label is on the same page as its content.

## Guarantees (from the shared components)

- A role heading never ends a page: it moves with the next block. In the row beside a label, the row is sticky
  for the same reason.
- A bullet never splits across pages (`block(breakable: false)`), and neither does a skills row.
- Output does not depend on the date, the language, or hyphenation.

## Limits

- A section with no blocks shows its label and nothing beside it; the row is sticky, so the label moves with the
  section after it.
- One unlabeled bullet in a block of labeled ones makes the whole block a list, not a table; nothing warns.
- A label wider than the gutter wraps over several lines (`Languages and Technologies` takes three); the row
  grows to hold it. A single word wider than the label cell (`ACCOMPLISHMENTS` is 130pt against 101pt) cannot
  wrap, so the label is set smaller until it fits, down to `size.label-min` (8pt); below that the word would run
  into the content, so shorten the title or widen the gutter.
- The Experienced template is optimized for two pages. Nothing here limits the length; `resume build` reports a
  longer one. The principal architect baseline is five pages in this style, which comes from its content: no
  theme setting tried (line pitch, bullet gap, indents) brings it under five.
- `lang` is fixed at `en-US`, as in the other styles.

## Where it differs from the template, and why

Reviewed against the template's PDF and Word file (rsm-141.9); Standard Resume's Venables was not available:

| Setting | Template | Here | Why |
|---|---|---|---|
| Layout | five Word tables | a grid row per section | the grid paginates, the tables are fixed |
| Section label | Book Antiqua bold 12pt, black, in capitals | the same | |
| Name | Book Antiqua 24pt, centered | the same | |
| Links | red `#cc0000` | the same, underlined | a link should look like one |
| Role line | company and title in bold 12pt, period bold 11pt at the right | the same | |
| Skills labels | regular weight | the same (`Languages:` is not bold) | owner feedback in the visual review |
| Bullets | marker 18pt in, text 36pt in | the same | |
| Line pitch | 13.4pt | 14.5pt (`line.leading: 0.1em`) | owner: both read about the same, so the pragmatic pitch is kept |
| Summary | unlabeled paragraph across the page | a `Summary` label in the gutter | screening software finds sections by heading |
| Skills value column | label column fixed at 85pt | as wide as the longest label | a fixed width would wrap `Cloud and infrastructure:` |
| Footer | none | name and `Page N of M`, across the page | helps on a multi-page resume |

// ABOUTME: The experienced style's theme: font roles, sizes, colors, spacing, the label gutter and page margins.
// ABOUTME: Edit this file (after `resume export-style`) to restyle; style.typ holds the structure.


// Each font role names the template's own font (`preferred`, used when installed) and a free, bundled
// `fallback`. `metric-compatible: true` promises the fallback has the same character widths. TeX Gyre
// Pagella is not a match for Book Antiqua (its widths differ by about 2.4% on average), so a build that
// falls back to it reports `font-fallback-layout`; install Book Antiqua, or pass its files with
// `--font-path`, for the template's own line breaks.
#let theme = (
  fonts: (
    body: (preferred: "Calibri", fallback: "Carlito", metric-compatible: true),
    // The name and the section labels.
    label: (preferred: "Book Antiqua", fallback: "TeX Gyre Pagella", metric-compatible: false),
    mono: (preferred: "DejaVu Sans Mono", fallback: "DejaVu Sans Mono", metric-compatible: true),
  ),
  // `name` is the name above the contact lines, `label` the section labels (set smaller, down to `label-min`, when
  // one word is wider than the gutter), `role` the company and title of a role line, and `period` its dates.
  size: (body: 11pt, name: 24pt, label: 12pt, label-min: 8pt, role: 12pt, period: 11pt, footer: 8.5pt),
  color: (
    text: rgb("#000000"),
    label: rgb("#000000"),
    accent: rgb("#cc0000"),
    link: rgb("#cc0000"),
    muted: rgb("#595959"),
  ),
  // The section labels sit in a gutter at the left of the page, inside `page-margin.left`. The style makes the
  // page's left margin `page-margin.left + gutter.width + gutter.gap`, so every block starts in the text column,
  // and sets the labels, the name, the contact lines and the footer back over the gutter. Edit these two entries
  // only; nothing else needs to change with them. The numbers are the template's: its first table column is 2263
  // twips (113.15pt) wide, and measured in its PDF a label starts 5.9pt in from the page margin (at 41.9pt) and
  // the content 5.95pt after the column (at 155.1pt). The bottom margin leaves room for the footer.
  gutter: (width: 113.15pt, inset: 5.9pt, gap: 5.95pt),
  page-margin: (left: 0.5in, right: 0.5in, top: 0.5in, bottom: 0.75in),
  footer-descent: 35%,
  // Line pitch is set from explicit lengths, not font metrics, so a fallback font with a slightly
  // different cap height still gives identical line positions. 0.952em above and 0.269em below the
  // baseline is Calibri's single line, 1.22em; `leading` adds 0.1em between lines.
  line: (top: 0.952em, bottom: -0.269em, leading: 0.1em),
  spacing: (
    section: 14pt,
    name-below: 0pt,
    role: 10pt,
    role-line-below: 1pt,
    era: 6pt,
    era-line-below: 1pt,
    blurb: 1pt,
    bullet: 2pt,
    paragraph: 4pt,
    skill-row: 2pt,
  ),
  // The skills table: the label column is as wide as its longest label, but no wider than `label-max`.
  skills: (label-max: 2.2in, column-gap: 0.15in),
  // The template puts the marker 18pt in and the text 36pt in.
  bullet: (marker: [•], indent: 0.25in, gap: 12.5pt),
)

// ABOUTME: The pragmatic style's theme: font roles, sizes, colors, spacing and page margins.
// ABOUTME: Edit this file (after `resume export-style`) to restyle; style.typ holds the structure.

// Each font role names the template's own font (`preferred`, used when installed) and a free,
// bundled `fallback`. `metric-compatible: true` promises the fallback has the same character widths,
// so line breaks do not change when the fallback stands in.
#let theme = (
  fonts: (
    body: (preferred: "Calibri", fallback: "Carlito", metric-compatible: true),
    display: (preferred: "Calibri", fallback: "Carlito", metric-compatible: true),
    mono: (preferred: "DejaVu Sans Mono", fallback: "DejaVu Sans Mono", metric-compatible: true),
  ),
  size: (body: 10.5pt, name: 24pt, heading: 13pt, footer: 8.5pt),
  color: (
    text: rgb("#000000"),
    accent: rgb("#1155cc"),
    link: rgb("#000000"),
    muted: rgb("#595959"),
  ),
  // The page margins; the bottom one leaves room for the footer.
  margin: (x: 0.5in, top: 0.5in, bottom: 0.7in),
  footer-descent: 35%,
  // Line pitch is set from explicit lengths, not font metrics, so a fallback font with a slightly
  // different cap height still gives identical line positions. 0.952em above and 0.269em below the baseline is Calibri's single line, 1.22em.
  line: (top: 0.952em, bottom: -0.269em, leading: 0pt),
  spacing: (
    section: 12pt,
    section-below: 5pt,
    role: 9pt,
    role-line-below: 2pt,
    era: 5pt,
    era-line-below: 1pt,
    blurb: 1pt,
    bullet: 1.5pt,
    paragraph: 3pt,
  ),
  // `name-share`: the widest the name may be, as a fraction of the line, to stay between the two sides.
  header: (name-share: 0.55, stacked-gap: 4pt),
  rule: (weight: 1pt, gap: 2pt),
  bullet: (marker: [•], indent: 0.1in, gap: 0.1in),
)

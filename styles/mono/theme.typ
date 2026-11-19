// ABOUTME: The mono style's theme: font roles, sizes, colors, spacing and page margins.
// ABOUTME: Edit this file (after `resume export-style`) to restyle; style.typ holds the structure.

// Each font role names the template's own font (`preferred`, used when installed) and a free, bundled
// `fallback`. `metric-compatible: true` promises the fallback has the same character widths, so line
// breaks do not change when the fallback stands in.
#let theme = (
  fonts: (
    body: (preferred: "Calibri", fallback: "Carlito", metric-compatible: true),
    display: (preferred: "Calibri", fallback: "Carlito", metric-compatible: true),
    mono: (preferred: "DejaVu Sans Mono", fallback: "DejaVu Sans Mono", metric-compatible: true),
  ),
  // `role` is the role line (title, company, period); `contact` is the contact line under the name.
  size: (body: 11pt, name: 24pt, heading: 12pt, role: 12pt, contact: 12pt, footer: 8.5pt),
  // The template is black on white. `accent` colors the section titles and their rules, so it is black here;
  // links are blue, because a black link reads as a font style.
  color: (
    text: rgb("#000000"),
    accent: rgb("#000000"),
    link: rgb("#1155cc"),
    muted: rgb("#595959"),
  ),
  // The page margins; the bottom one leaves room for the footer.
  margin: (x: 0.6in, top: 0.7in, bottom: 0.75in),
  footer-descent: 35%,
  // Line pitch is set from explicit lengths, not font metrics, so a fallback font with a slightly
  // different cap height still gives identical line positions. 0.952em above and 0.269em below the
  // baseline is Calibri's single line, 1.22em; `leading` adds 0.1em between lines.
  line: (top: 0.952em, bottom: -0.269em, leading: 0.1em),
  spacing: (
    section: 18pt,
    section-below: 7pt,
    role: 14pt,
    role-line-below: 3pt,
    era: 8pt,
    era-line-below: 2pt,
    blurb: 2pt,
    bullet: 2.5pt,
    paragraph: 5pt,
  ),
  // `gap`: the space between the name and the contact line.
  header: (gap: 4pt),
  rule: (weight: 1pt, gap: 2pt),
  bullet: (marker: [•], indent: 0.15in, gap: 0.15in),
)

// ABOUTME: The mono style: left-aligned name, one contact line, capitalized section headings, title-first role line.
// ABOUTME: Modeled on The Mono Engineer's resume template; structure lives here, values in theme.typ.

#import "/lib/components.typ": *
#import "./theme.typ": theme

#let resume = index-roles(json("/resume.json"))

#show: resume-document.with(theme, resume)

// The title carries the emphasis; the company and period are regular weight. A role without a title
// has only the company to show, so that is bold.
#let role-title-first(theme, role) = {
  let lead = if role.title != none { strong(role.title) + [ · #role.company] } else { strong(role.company) }
  text(size: theme.size.role, grid(columns: (1fr, auto), column-gutter: 1em, lead, align(right, role.period.text)))
}

// Section titles in capitals, in the accent color.
#let section-capitals(theme, title) = text(
  font: font-stack(theme.fonts.display),
  size: theme.size.heading,
  weight: "bold",
  fill: theme.color.accent,
  tracking: theme.tracking.heading,
  upper(title),
)

#let headings = (..default-headings, role: role-title-first, section: section-capitals)

// Header: the name on the left, then one line with every contact item. The name comes first, so the
// text of the PDF starts with it.
#let header(theme, resume) = {
  let parts = contact-parts(resume.contact)
  let items = parts.where + parts.phones + parts.emails + parts.links
  text(font: font-stack(theme.fonts.display), size: theme.size.name, resume.name)
  if items.len() > 0 {
    v(theme.header.gap)
    text(size: theme.size.contact, join-items(items))
  }
}

#header(theme, resume)

#for section in resume.sections {
  section-heading(theme, section.title, headings: headings)
  for blk in section.blocks {
    render-block(theme, blk, headings: headings)
  }
}

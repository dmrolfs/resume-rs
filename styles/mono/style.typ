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
  role-grid(theme, lead, role.period.text)
}

// Section titles in capitals, with a little extra letter spacing; the look is the shared one.
#let section-capitals(theme, title) = text(
  tracking: theme.tracking.heading,
  section-heading-content(theme, upper(title)),
)

#let headings = (..default-headings, role: role-title-first, section: section-capitals)

// Header: the name on the left, then one line with every contact item. The name comes first, so the
// text of the PDF starts with it. Each item is unbreakable, so a line that is too wide wraps between items
// and never inside an address or a link.
#let header(theme, resume) = {
  let parts = contact-parts(resume.contact)
  let items = parts.where + parts.phones + parts.emails + parts.links
  text(font: font-stack(theme.fonts.display), size: theme.size.name, resume.name)
  if items.len() > 0 {
    v(theme.header.gap)
    text(size: theme.size.contact, items.map(item => box(render-inlines(item))).join([ · ]))
  }
}

#header(theme, resume)

#render-sections(theme, resume, headings: headings)

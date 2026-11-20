// ABOUTME: The mono style: centered name and one contact line, black capitalized section titles, no color.
// ABOUTME: Modeled on The Mono Engineer's resume template; structure lives here, values in theme.typ.

#import "/lib/components.typ": *
#import "./theme.typ": theme

#let resume = index-roles(json("/resume.json"))

#show: resume-document.with(theme, resume)

// Section titles in capitals. The look is the shared one: with this theme's black accent it is black bold text
// over a black rule.
#let section-capitals(theme, title) = section-heading-content(theme, upper(title))

#let headings = (..default-headings, section: section-capitals)

// Header: the name, then one line with every contact item, both centered. The name comes first, so the text of
// the PDF starts with it. Items follow the book's order: email, phone, location, links. Each item is
// unbreakable, so a line that is too wide wraps between items and never inside an address or a link.
#let header(theme, resume) = {
  let parts = contact-parts(resume.contact)
  let items = parts.emails + parts.phones + parts.where + parts.links
  align(center, {
    // The two lines are paragraphs; `gap` is the only space between them.
    set par(spacing: theme.header.gap)
    text(font: font-stack(theme.fonts.display), size: theme.size.name, resume.name)
    if items.len() > 0 {
      parbreak()
      text(size: theme.size.contact, items.map(item => box(render-inlines(item))).join([ · ]))
    }
  })
}

#header(theme, resume)

#render-sections(theme, resume, headings: headings)

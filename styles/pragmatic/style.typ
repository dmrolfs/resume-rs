// ABOUTME: The pragmatic style: three-part header, ruled section headings, role line with right-aligned period.
// ABOUTME: Modeled on The Pragmatic Engineer's resume template; structure lives here, values in theme.typ.

#import "/lib/components.typ": *
#import "./theme.typ": theme

#let resume = index-roles(json("/resume.json"))

#show: resume-document.with(theme, resume)

// Header: location and links on the left, the name in the center, phone and email on the right.
// The name is emitted first so the text of the PDF starts with it, not with the location. A name too
// wide to share the line with both sides moves onto a line of its own above them.
#let header(theme, resume) = {
  let parts = contact-parts(resume.contact)
  let side(..groups) = groups.pos().filter(g => g.len() > 0).map(g => join-items(g)).join(linebreak())
  let name = text(font: font-stack(theme.fonts.display), size: theme.size.name, resume.name)
  let (where, reach) = (side(parts.where, parts.links), side(parts.phones, parts.emails))
  layout(size => if measure(name).width > size.width * theme.header.name-share {
    align(center, name)
    v(theme.header.stacked-gap)
    grid(columns: (1fr, 1fr), align: (left, right), where, reach)
  } else {
    block(width: 100%, {
      place(center + horizon, name)
      // The middle cell only reserves the height of the name, which `place` takes out of the flow.
      grid(
        columns: (1fr, 0pt, 1fr),
        align: (left + horizon, center, right + horizon),
        where,
        box(height: measure(name).height),
        reach,
      )
    })
  })
}

#header(theme, resume)

#for section in resume.sections {
  section-heading(theme, section.title)
  for block in section.blocks {
    render-block(theme, block)
  }
}

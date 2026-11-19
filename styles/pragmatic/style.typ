// ABOUTME: The pragmatic style: three-part header, ruled section headings, role line with right-aligned period.
// ABOUTME: Modeled on The Pragmatic Engineer's resume template; structure lives here, values in theme.typ.

#import "/lib/components.typ": *
#import "./theme.typ": theme

#let resume = index-roles(json("/resume.json"))

#show: resume-document.with(theme, resume)

// Header: location and links on the left, the name in the center, phone and email on the right.
// The name is emitted first so the text of the PDF starts with it, not with the location. When the name and
// the wider side, with a gap on each side of the name, do not fit the line, the name moves onto a line of
// its own above the two sides.
#let header(theme, resume) = {
  let parts = contact-parts(resume.contact)
  let side(..groups) = text(
    size: theme.size.contact,
    groups.pos().filter(g => g.len() > 0).map(g => join-items(g)).join(linebreak(), default: []),
  )
  let name = text(font: font-stack(theme.fonts.display), size: theme.size.name, resume.name)
  let (where, reach) = (side(parts.where, parts.links), side(parts.phones, parts.emails))
  layout(size => {
    let widest = calc.max(measure(where).width, measure(reach).width)
    if measure(name).width + 2 * (widest + theme.header.name-gap) > size.width {
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
    }
  })
}

#header(theme, resume)

#render-sections(theme, resume)

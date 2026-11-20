// ABOUTME: The experienced style: section labels in a left gutter, roles as Company, Title with the period on the right.
// ABOUTME: Modeled on the Experienced Engineer's resume template; structure lives here, values in theme.typ.

#import "/lib/components.typ": *
#import "./theme.typ": theme

#let resume = index-roles(json("/resume.json"))

#show: resume-document.with(theme, resume)

// The name centered over the whole page, then email, phone and location, then the links, each centered.
// The header reaches back over the gutter in the left margin, so it is centered on the page.
#let header(theme, resume) = {
  let parts = contact-parts(resume.contact)
  let line(..groups) = {
    let items = groups.pos().fold((), (all, group) => all + group)
    if items.len() > 0 { align(center, join-items(items, sep: [ • ])) }
  }
  let bleed = theme.footer-bleed
  pad(left: -bleed, {
    // The lines of the header sit one line pitch apart, with no paragraph spacing between them.
    set par(spacing: 0pt)
    align(center, text(font: font-stack(theme.fonts.label), size: theme.size.name, resume.name))
    v(theme.spacing.name-below)
    line(parts.emails, parts.phones, parts.where)
    line(parts.links)
  })
}

// The label in the gutter: the section title in capitals, in the label font, black, inset from the page margin.
// The text edges are lengths taken from the size of the first line beside the label (`line-size`), not from the
// label's own size, so the label and that line share a baseline.
#let section-label(theme, title, line-size) = pad(left: theme.gutter.inset, right: theme.gutter.inset, heading(
  level: 1,
  text(
    font: font-stack(theme.fonts.label),
    size: theme.size.label,
    weight: "bold",
    fill: theme.color.label,
    top-edge: theme.line.top * (line-size / theme.size.label),
    bottom-edge: theme.line.bottom * (line-size / theme.size.label),
    upper(title),
  ),
))

// The role line: Company, Title in bold, with the period on the right in bold at the body size.
#let role-heading(theme, role) = {
  let lead = if role.title != none { role.company + ", " + role.title } else { role.company }
  grid(
    columns: (1fr, auto),
    column-gutter: 1em,
    align: (left + bottom, right + bottom),
    text(size: theme.size.role, strong(lead)),
    text(size: theme.size.period, strong(role.period.text)),
  )
}

#let headings = (..default-headings, role: role-heading)

// A group of skills as rows of a label and its value. The label column is as wide as the longest label,
// up to `skills.label-max`; longer labels wrap. Each row is unbreakable. Labels are regular weight, as in the
// template; an inline in a label (a link) keeps its own style.
#let skills-table(theme, items) = context {
  let labels = items.map(item => render-inlines(item.label) + [:])
  let widest = calc.max(..labels.map(l => measure(l).width))
  grid(
    columns: (calc.min(widest, theme.skills.label-max), 1fr),
    column-gutter: theme.skills.column-gap,
    row-gutter: theme.spacing.skill-row,
    ..items
      .zip(labels)
      .map(((item, label)) => (
        grid.cell(breakable: false, label),
        grid.cell(breakable: false, render-inlines(item.content)),
      ))
      .flatten(),
  )
}

// A block of bullets in which every bullet has a label is a skills table; any other bullets are a list.
#let is-skills(blk) = blk.kind == "bullets" and blk.items.len() > 0 and blk.items.all(item => item.label != none)

#let render-section-block(theme, blk) = {
  if is-skills(blk) {
    block(above: theme.spacing.paragraph, below: theme.spacing.paragraph, skills-table(theme, blk.items))
  } else { render-block(theme, blk, headings: headings) }
}

// The first unit of a section, which sits beside the label: a whole block, or for a role its heading and
// blurb. The rest of a role follows below the row.
#let render-first(theme, blk) = {
  if blk.kind == "role" { role-head(theme, blk, headings: headings) } else { render-section-block(theme, blk) }
}

// A section: its label in the gutter beside its first unit, then the rest in the text column. The label
// and the first unit share a grid row, so a label taller than the unit still gets room and the label never
// ends a page alone. Page breaks that open a section come before the label.
#let render-section(theme, section) = {
  let start = section.blocks.position(blk => blk.kind != "page-break")
  let leading = if start == none { section.blocks } else { section.blocks.slice(0, start) }
  for blk in leading { render-block(theme, blk) }
  let first-line-size = if start != none and section.blocks.at(start).kind == "role" { theme.size.role } else { theme.size.body }
  let label = section-label(theme, section.title, first-line-size)
  let side = theme.footer-bleed
  let row(content, sticky: false) = block(above: theme.spacing.section, below: 0pt, sticky: sticky, pad(left: -side, grid(
    columns: (theme.gutter.width, 1fr),
    column-gutter: theme.gutter.gap,
    label,
    content,
  )))
  if start == none { row([]) } else {
    let first = section.blocks.at(start)
    // A role heading in the row has lost its own stickiness (a grid cell is one block), so the row takes
    // it over: it moves to the next page with the first block after it. Other first blocks break normally.
    let follows = first.kind == "role" and (first.blurb != none or first.body.len() > 0)
    row(render-first(theme, first), sticky: follows)
    if first.kind == "role" { role-body(theme, first, headings: headings) }
    for blk in section.blocks.slice(start + 1) { render-section-block(theme, blk) }
  }
}

#header(theme, resume)

#for section in resume.sections { render-section(theme, section) }

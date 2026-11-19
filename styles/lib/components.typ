// ABOUTME: Shared resume components: inlines, bullets, role and era lines, sections, footer, probes.
// ABOUTME: Every style imports this; it fixes the pagination guarantees and the layout probes the CLI reads.

// ---- fonts ----------------------------------------------------------------------------------------

// font-stack:begin (tests keep this block identical to tests/fixtures/styles/lib/font-stack.typ)
// The families the CLI says are installed, lowercased, or `none` outside the CLI (then nothing is filtered).
#let installed-families() = {
  let given = sys.inputs.at("fonts", default: none)
  if given == none { none } else { given.map(lower) }
}

// The families of `(preferred, fallback)` that are installed. Typst warns `unknown font family` for every
// missing name in a stack and cannot be told not to, so a missing preferred font must never reach
// `text(font: ..)`. When neither is installed the fallback is returned anyway: Typst's warning is then the
// right signal, and the CLI reports the role as missing.
#let font-stack(role, installed: installed-families()) = {
  assert(installed == none or type(installed) == array, message: "installed must be an array of family names")
  let names = (role.preferred, role.fallback)
  if installed == none { return names }
  let present = names.filter(name => lower(name) in installed)
  if present.len() == 0 { (role.fallback,) } else { present }
}
// font-stack:end

// ---- probes ---------------------------------------------------------------------------------------

// Invisible markers the CLI queries for the page and position of each role and era heading.
// Keys are the contract with src/typeset/probe.rs; change both together.
#let role-probe(role) = [#metadata((
    index: role.index,
    company: role.company,
    title: role.title,
    period: role.period.text,
  )) <resume-role>]

#let era-probe(role-index, era) = [#metadata((
    role: role-index,
    index: era.index,
    name: era.name,
    period: era.period.text,
  )) <resume-era>]

// ---- model helpers --------------------------------------------------------------------------------

// Numbers every role (document order, from 0) and every era (within its role, from 0) so probes can
// name them. Pure: returns a new resume; the JSON contract itself carries no indexes.
#let index-roles(resume) = {
  let next = 0
  let sections = ()
  for section in resume.sections {
    let blocks = ()
    for block in section.blocks {
      if block.kind == "role" {
        let eras = 0
        let body = ()
        for part in block.body {
          if part.kind == "era" {
            body.push((..part, index: eras))
            eras += 1
          } else { body.push(part) }
        }
        blocks.push((..block, index: next, body: body))
        next += 1
      } else { blocks.push(block) }
    }
    sections.push((..section, blocks: blocks))
  }
  (..resume, sections: sections)
}

// ---- inlines --------------------------------------------------------------------------------------

// Text, strong, emph, link and code runs. Text is inserted as a string, never re-parsed as markup.
#let render-inlines(items) = {
  for item in items {
    if item.kind == "text" { item.text } else if item.kind == "strong" {
      strong(render-inlines(item.content))
    } else if item.kind == "emph" { emph(render-inlines(item.content)) } else if item.kind == "link" {
      link(item.url, render-inlines(item.content))
    } else if item.kind == "code" { raw(item.text) }
  }
}

// The text of inlines without formatting, for classifying contact items.
#let inlines-text(items) = {
  let parts = items.map(item => if item.kind == "text" or item.kind == "code" { item.text } else {
    inlines-text(item.content)
  })
  parts.join()
}

// ---- contact line ---------------------------------------------------------------------------------

// Splits the contact line at its ` · ` separators into items, each an array of inlines.
#let contact-items(contact) = {
  let items = ((),)
  for inline in contact {
    if inline.kind == "text" and inline.text.contains("·") {
      let parts = inline.text.split("·")
      for (i, part) in parts.enumerate() {
        if i > 0 { items.push(()) }
        if part.trim() != "" { items.last().push((kind: "text", text: part.trim())) }
      }
    } else { items.last().push(inline) }
  }
  items.filter(item => item.len() > 0)
}

#let _has-link(item) = item.any(i => i.kind == "link")
#let _is-email(item) = {
  let flat = inlines-text(item)
  flat.contains("@") or item.any(i => i.kind == "link" and i.url.starts-with("mailto:"))
}
#let _is-phone(item) = inlines-text(item).trim().match(regex("^\+?[0-9][0-9 ().\-]{6,}$")) != none

// Sorts the contact items into the pragmatic header's two sides: `where` (location, then links) and
// `reach` (phone, then email). Anything unrecognized counts as location, so nothing is dropped.
#let contact-parts(contact) = {
  let items = contact-items(contact)
  let emails = items.filter(_is-email)
  let phones = items.filter(item => not _is-email(item) and _is-phone(item))
  let rest = items.filter(item => not _is-email(item) and not _is-phone(item))
  (
    where: rest.filter(item => not _has-link(item)),
    links: rest.filter(_has-link),
    phones: phones,
    emails: emails,
  )
}

// Items joined by a separator, as content.
#let join-items(items, sep: [ · ]) = items.map(render-inlines).join(sep)

// ---- bullets --------------------------------------------------------------------------------------

// One bullet: an optional strong label, then the text. Unbreakable, so it never splits across pages.
#let bullet(theme, item) = block(
  breakable: false,
  above: theme.spacing.bullet,
  below: theme.spacing.bullet,
  list(
    marker: theme.bullet.marker,
    indent: theme.bullet.indent,
    body-indent: theme.bullet.gap,
    {
      if item.label != none { strong[#render-inlines(item.label):] + [ ] }
      render-inlines(item.content)
    },
  ),
)

#let bullets(theme, items) = for item in items { bullet(theme, item) }

// ---- headings -------------------------------------------------------------------------------------

// A section heading with a rule. Sticky, so it moves to the next page with the block after it.
#let section-heading(theme, title) = heading(level: 1, title)

// A paragraph of inlines.
#let paragraph(theme, content) = block(above: theme.spacing.paragraph, below: theme.spacing.paragraph, render-inlines(content))

// The one-line description under a role heading. Sticky when something follows it in the role.
#let blurb(theme, content, sticky: false) = block(
  breakable: false,
  sticky: sticky,
  above: theme.spacing.blurb,
  below: theme.spacing.blurb,
  render-inlines(content),
)

// Title, company and period of a role, as the default (pragmatic) heading content.
#let role-heading-content(theme, role) = {
  let lead = if role.title != none { strong(role.title) + [ · ] + strong(role.company) } else {
    strong(role.company)
  }
  grid(columns: (1fr, auto), column-gutter: 1em, lead, align(right, strong(role.period.text)))
}

// A role heading. Sticky, so it never ends a page: it moves with the next block. `heading-content` renders the
// heading, so a style can lay the same facts out differently and keep the guarantees.
#let role-line(theme, role, heading-content: role-heading-content) = {
  let follows = role.blurb != none or role.body.len() > 0
  block(
    sticky: follows,
    breakable: false,
    above: theme.spacing.role,
    below: theme.spacing.role-line-below,
    {
      role-probe(role)
      heading(level: 2, outlined: false, heading-content(theme, role))
    },
  )
}

// An era heading inside a role: name and period. Sticky for the same reason.
#let era-heading-content(theme, era) = grid(
  columns: (1fr, auto),
  column-gutter: 1em,
  emph(era.name),
  align(right, era.period.text),
)

#let era-line(theme, role-index, era, heading-content: era-heading-content) = block(
  sticky: true,
  breakable: false,
  above: theme.spacing.era,
  below: theme.spacing.era-line-below,
  {
    era-probe(role-index, era)
    heading-content(theme, era)
  },
)

// ---- blocks ---------------------------------------------------------------------------------------

// A role with its blurb and body (eras, bullets, paragraphs).
#let role(theme, r, heading-content: role-heading-content) = {
  role-line(theme, r, heading-content: heading-content)
  if r.blurb != none { blurb(theme, r.blurb, sticky: r.body.len() > 0) }
  for part in r.body {
    if part.kind == "era" {
      era-line(theme, r.index, part)
      bullets(theme, part.items)
    } else if part.kind == "bullets" { bullets(theme, part.items) } else if part.kind == "paragraph" {
      paragraph(theme, part.content)
    }
  }
}

// One block of a section.
#let render-block(theme, block, role-heading-content: role-heading-content) = {
  if block.kind == "paragraph" { paragraph(theme, block.content) } else if block.kind == "bullets" {
    bullets(theme, block.items)
  } else if block.kind == "role" { role(theme, block, heading-content: role-heading-content) } else if (
    block.kind == "page-break"
  ) { pagebreak(weak: true) }
}

// ---- page furniture -------------------------------------------------------------------------------

// The footer: the name on the left, "Page N of M" on the right.
#let footer(theme, name) = context {
  set text(size: theme.size.footer, fill: theme.color.muted)
  grid(
    columns: (1fr, auto),
    name,
    [Page #counter(page).display() of #counter(page).final().first()],
  )
}

// Document-wide rules: metadata, page, text, paragraph, links, code, and the font-role probe. Use as
// `#show: resume-document.with(theme, resume)`.
#let resume-document(theme, resume, body) = {
  set document(title: resume.name, author: resume.name)
  set page(
    paper: sys.inputs.at("paper", default: "us-letter"),
    margin: theme.margin,
    footer: footer(theme, resume.name),
    footer-descent: theme.footer-descent,
  )
  set text(
    font: font-stack(theme.fonts.body),
    size: theme.size.body,
    fill: theme.color.text,
    lang: "en",
    region: "US",
    hyphenate: false,
    ligatures: false,
    top-edge: theme.line.top,
    bottom-edge: theme.line.bottom,
  )
  set par(leading: theme.line.leading, spacing: theme.spacing.paragraph, justify: false)
  set list(spacing: 0pt)
  show link: it => text(fill: theme.color.link, underline(it))
  show raw: set text(font: font-stack(theme.fonts.mono), size: 0.92em)
  show heading.where(level: 1): it => block(
    width: 100%,
    sticky: true,
    above: theme.spacing.section,
    below: theme.spacing.section-below,
    stroke: (bottom: theme.rule.weight + theme.color.accent),
    inset: (bottom: theme.rule.gap),
    text(
      font: font-stack(theme.fonts.display),
      size: theme.size.heading,
      weight: "bold",
      fill: theme.color.accent,
      it.body,
    ),
  )
  // Typst sizes and bolds headings by level; role lines keep the body size and set their own weights.
  show heading.where(level: 2): it => text(size: theme.size.body, weight: "regular", it.body)
  [#metadata(theme.fonts) <resume-fonts>]
  body
}

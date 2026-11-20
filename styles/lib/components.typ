// ABOUTME: Shared resume components: inlines, bullets, role and era lines, sections, footer, probes.
// ABOUTME: Every style imports this; it fixes the pagination guarantees and the layout probes the CLI reads.

// ---- fonts ----------------------------------------------------------------------------------------

// font-stack:begin (tests keep this block identical to tests/fixtures/styles/lib/font-stack.typ)
// The families the CLI says are installed, lowercased, or `none` outside the CLI (then nothing is filtered).
#let installed-families() = {
  let given = sys.inputs.at("fonts", default: none)
  if given == none { none } else {
    assert(type(given) == array, message: "sys.inputs.fonts must be an array of family names")
    given.map(lower)
  }
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

// The JSON contract version these components understand.
#let schema-version = 1

// Numbers every role (document order, from 0) and every era (within its role, from 0) so probes can
// name them. Pure: returns a new resume; the JSON contract itself carries no indexes.
#let index-roles(resume) = {
  let next = 0
  let sections = ()
  for section in resume.sections {
    let blocks = ()
    for blk in section.blocks {
      if blk.kind == "role" {
        let eras = 0
        let body = ()
        for part in blk.body {
          if part.kind == "era" {
            body.push((..part, index: eras))
            eras += 1
          } else { body.push(part) }
        }
        blocks.push((..blk, index: next, body: body))
        next += 1
      } else { blocks.push(blk) }
    }
    sections.push((..section, blocks: blocks))
  }
  (..resume, sections: sections)
}

// ---- inlines --------------------------------------------------------------------------------------

// Text, strong, emph, link and code runs. Text is inserted as a string, never re-parsed as markup.
// An unknown kind stops the render: dropping it would silently lose part of the resume.
#let render-inlines(items) = {
  for item in items {
    if item.kind == "text" { item.text } else if item.kind == "strong" {
      strong(render-inlines(item.content))
    } else if item.kind == "emph" { emph(render-inlines(item.content)) } else if item.kind == "link" {
      link(item.url, render-inlines(item.content))
    } else if item.kind == "code" { raw(item.text) } else {
      panic("unknown inline kind `" + item.kind + "`: the style and the resume schema have drifted apart")
    }
  }
}

// The text of inlines without formatting, for classifying contact items.
#let inlines-text(items) = {
  let parts = items.map(item => if item.kind == "text" or item.kind == "code" { item.text } else {
    inlines-text(item.content)
  })
  parts.join(default: "")
}

// ---- contact line ---------------------------------------------------------------------------------

// Drops the whitespace at the two ends of an item, so a separator's padding is gone and the space inside
// `LinkedIn: <link>` stays.
#let _trim-edges(item) = {
  let kept(items) = items.filter(i => i.kind != "text" or i.text != "")
  let trimmed = item
  if trimmed.len() > 0 and trimmed.first().kind == "text" {
    trimmed.first() = (..trimmed.first(), text: trimmed.first().text.trim(at: start))
  }
  trimmed = kept(trimmed)
  if trimmed.len() > 0 and trimmed.last().kind == "text" {
    trimmed.last() = (..trimmed.last(), text: trimmed.last().text.trim(at: end))
  }
  kept(trimmed)
}

// Splits the contact line at its ` · ` separators into items, each an array of inlines.
#let contact-items(contact) = {
  let items = ((),)
  for inline in contact {
    if inline.kind == "text" and inline.text.contains("·") {
      for (i, part) in inline.text.split("·").enumerate() {
        if i > 0 { items.push(()) }
        if part != "" { items.last().push((kind: "text", text: part)) }
      }
    } else { items.last().push(inline) }
  }
  items.map(_trim-edges).filter(item => item.len() > 0)
}

#let _is-link(inline) = inline.kind == "link"
#let _has-link(item) = item.any(_is-link)

// An email address, optionally after a label (`Email: a@b.co`). Only text without a link is tested this way:
// a link such as `[@ada](https://mastodon.social/@ada)` is a link, not an address.
#let _email-text = regex("^([^\s:@]+:\s*)?[^\s@]+@[^\s@]+\.[^\s@]+$")
#let _is-email(item) = if _has-link(item) {
  item.any(inline => _is-link(inline) and inline.url.starts-with("mailto:"))
} else { inlines-text(item).trim().match(_email-text) != none }

// A phone number: digits with the usual separators, an optional `+` or `(`, an optional extension.
#let _phone-text = regex("^\+?[0-9(][0-9 ().\-]{6,}(\s*(ext|x)\.?\s*\d+)?$")
#let _is-phone(item) = inlines-text(item).trim().match(_phone-text) != none

// Sorts the contact items into four groups, each in the order written: `where` (location and anything
// unrecognized, so nothing is dropped), `links`, `phones`, and `emails`. A style arranges them: pragmatic puts
// `where` and `links` left and `phones` and `emails` right; mono puts them on one line.
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

// The bullets of one group as a single list, so the PDF tags them as one list. Each item is unbreakable,
// so it never splits across pages: its row cannot break, and moves whole. A bullet taller than a page cannot
// be kept whole; it overflows the page instead (see the README).
#let bullets(theme, items) = list(
  marker: theme.bullet.marker,
  indent: theme.bullet.indent,
  body-indent: theme.bullet.gap,
  tight: false,
  spacing: theme.spacing.bullet,
  ..items.map(item => block(
    breakable: false,
    above: 0pt,
    below: 0pt,
    {
      if item.label != none { strong[#render-inlines(item.label):] + [ ] }
      render-inlines(item.content)
    },
  )),
)

// ---- heading content ------------------------------------------------------------------------------

// What the three kinds of heading show. A style may replace any of them (see `default-headings`); the
// stickiness and spacing stay with the components that call them, so a replacement keeps the guarantees.

// The layout of a role line: `lead` (title and company) on the left, `period` on the right, at the role size.
#let role-grid(theme, lead, period) = text(
  size: theme.size.role,
  grid(columns: (1fr, auto), column-gutter: 1em, lead, align(right, period)),
)

// Title, company and period of a role, all bold.
#let role-heading-content(theme, role) = {
  let lead = if role.title != none { strong(role.title) + [ · ] + strong(role.company) } else {
    strong(role.company)
  }
  role-grid(theme, lead, strong(role.period.text))
}

// Name and period of an era.
#let era-heading-content(theme, era) = grid(
  columns: (1fr, auto),
  column-gutter: 1em,
  emph(era.name),
  align(right, era.period.text),
)

// The section title, in the accent color.
#let section-heading-content(theme, title) = text(
  font: font-stack(theme.fonts.display),
  size: theme.size.heading,
  weight: "bold",
  fill: theme.color.accent,
  title,
)

// Pass a copy with one entry replaced as `headings:` to render, say, role lines differently.
#let default-headings = (
  section: section-heading-content,
  role: role-heading-content,
  era: era-heading-content,
)

// ---- headings and blurbs --------------------------------------------------------------------------

// A section heading with a rule under it. Sticky, so it moves to the next page with the block after it.
#let section-heading(theme, title, headings: default-headings) = block(
  width: 100%,
  sticky: true,
  above: theme.spacing.section,
  below: theme.spacing.section-below,
  stroke: (bottom: theme.rule.weight + theme.color.accent),
  inset: (bottom: theme.rule.gap),
  heading(level: 1, (headings.section)(theme, title)),
)

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

// A role heading. Sticky, so it never ends a page: it moves with the next block.
#let role-line(theme, role, headings: default-headings) = {
  let follows = role.blurb != none or role.body.len() > 0
  block(
    sticky: follows,
    breakable: false,
    above: theme.spacing.role,
    below: theme.spacing.role-line-below,
    {
      role-probe(role)
      heading(level: 2, outlined: false, (headings.role)(theme, role))
    },
  )
}

// An era heading inside a role. Sticky for the same reason.
#let era-line(theme, role-index, era, headings: default-headings) = block(
  sticky: true,
  breakable: false,
  above: theme.spacing.era,
  below: theme.spacing.era-line-below,
  {
    era-probe(role-index, era)
    (headings.era)(theme, era)
  },
)

// ---- blocks ---------------------------------------------------------------------------------------

// A role with its blurb and body (eras, bullets, paragraphs).
#let render-role(theme, role, headings: default-headings) = {
  role-line(theme, role, headings: headings)
  if role.blurb != none { blurb(theme, role.blurb, sticky: role.body.len() > 0) }
  for part in role.body {
    if part.kind == "era" {
      era-line(theme, role.index, part, headings: headings)
      bullets(theme, part.items)
    } else if part.kind == "bullets" { bullets(theme, part.items) } else if part.kind == "paragraph" {
      paragraph(theme, part.content)
    } else { panic("unknown role body kind `" + part.kind + "`: the style and the resume schema have drifted apart") }
  }
}

// One block of a section.
#let render-block(theme, blk, headings: default-headings) = {
  if blk.kind == "paragraph" { paragraph(theme, blk.content) } else if blk.kind == "bullets" {
    bullets(theme, blk.items)
  } else if blk.kind == "role" { render-role(theme, blk, headings: headings) } else if (
    blk.kind == "page-break"
  ) { pagebreak(weak: true) } else {
    panic("unknown block kind `" + blk.kind + "`: the style and the resume schema have drifted apart")
  }
}

// Every section of the resume: its heading, then its blocks. `headings` replaces the content of the headings (see
// `default-headings`); it reaches both the section headings and the role and era lines.
#let render-sections(theme, resume, headings: default-headings) = for section in resume.sections {
  section-heading(theme, section.title, headings: headings)
  for blk in section.blocks { render-block(theme, blk, headings: headings) }
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
// `#show: resume-document.with(theme, resume)`. Stops when the resume is of a schema this file does not know.
#let resume-document(theme, resume, body) = {
  assert(
    resume.schema == schema-version,
    message: "this style reads resume schema " + str(schema-version) + ", but the resume is schema " + repr(resume.schema),
  )
  set document(title: resume.name, author: resume.name)
  set page(
    paper: sys.inputs.at("paper", default: "us-letter"),
    margin: theme.margin,
    footer: footer(theme, resume.name),
    footer-descent: theme.footer-descent,
  )
  // The page language is fixed (not read from the resume) so output does not depend on the machine.
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
  show link: it => text(fill: theme.color.link, underline(it))
  show raw: set text(font: font-stack(theme.fonts.mono), size: 0.92em)
  // Typst sizes and bolds headings by level. Every heading is neutral: its content, from `default-headings`,
  // sets its own size and weight.
  show heading: it => text(size: theme.size.body, weight: "regular", it.body)
  [#metadata(theme.fonts) <resume-fonts>]
  body
}

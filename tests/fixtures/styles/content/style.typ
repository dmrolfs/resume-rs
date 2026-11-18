// Renders every kind of node in /resume.json with plain Typst, to exercise layout, links,
// several font variants, and page breaks. Not a real style.
#let resume = json("/resume.json")
#set document(title: resume.name)
#set page(
  paper: sys.inputs.paper,
  margin: 0.75in,
  footer: context [#resume.name #h(1fr) #counter(page).display() / #counter(page).final().first()],
)
#set text(font: "Libertinus Serif", size: 10.5pt)

#let inlines(items) = for i in items {
  if i.kind == "text" { [#i.text] }
  else if i.kind == "strong" { [*#inlines(i.content)*] }
  else if i.kind == "emph" { [_#inlines(i.content)_] }
  else if i.kind == "link" { link(i.url)[#inlines(i.content)] }
  else if i.kind == "code" { raw(i.text) }
}

#let bullets(items) = list(..items.map(b => [#if b.label != none [*#inlines(b.label):* ]#inlines(b.content)]))

#let role(r) = {
  block(sticky: true)[*#r.company*#if r.title != none [ · #r.title] #h(1fr) #r.period.text]
  if r.blurb != none { inlines(r.blurb); parbreak() }
  for b in r.body {
    if b.kind == "era" {
      block(sticky: true)[_#b.name _ #h(1fr) #b.period.text]
      bullets(b.items)
    } else if b.kind == "bullets" {
      bullets(b.items)
    } else {
      inlines(b.content)
      parbreak()
    }
  }
}

#align(center, text(size: 20pt, weight: "bold")[#resume.name])
#align(center, inlines(resume.contact))

#for s in resume.sections {
  heading(level: 1, s.title)
  for b in s.blocks {
    if b.kind == "paragraph" { inlines(b.content); parbreak() }
    else if b.kind == "bullets" { bullets(b.items) }
    else if b.kind == "role" { role(b) }
    else if b.kind == "page-break" { pagebreak() }
  }
}

// Several more pages, so layout has independent page runs to spread over worker threads.
#for i in range(4) {
  pagebreak()
  heading(level: 1, [Filler #i])
  lorem(120)
}

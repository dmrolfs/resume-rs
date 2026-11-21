// ABOUTME: Pragmatic cover letter: matching fonts and margins, explicit date and authored prose.
#import "/lib/components.typ": *
#import "./theme.typ": theme

#let letter = json("/letter.json")
#show: letter-document.with(theme, letter)

#align(center, text(font: font-stack(theme.fonts.display), size: theme.size.name, letter.name))
#if letter.contact.len() > 0 {
  align(center, text(size: theme.size.contact, render-inlines(letter.contact)))
}
#v(1em)
#if letter.date != none {
  let parts = letter.date.split("-").map(int)
  datetime(year: parts.at(0), month: parts.at(1), day: parts.at(2)).display("[month repr:long] [day padding:none], [year]")
  parbreak()
}
#if letter.recipient != none { [#letter.recipient #linebreak()] }
#if letter.company != none { letter.company; parbreak() }

#render-inlines(letter.greeting)
#parbreak()
#for paragraph in letter.body {
  render-inlines(paragraph)
  parbreak()
}
#block(breakable: false)[
  #render-inlines(letter.closing)
  #linebreak()
  #letter.name
]

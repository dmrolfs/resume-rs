// ABOUTME: Full-block professional letter with restrained sender details and separated letter blocks.
#import "/lib/components.typ": *
#import "./theme.typ": theme

#let letter = json("/letter.json")
#show: letter-document.with(theme, letter)

#block(breakable: false)[
  #letter.name
  #if letter.contact.len() > 0 { linebreak(); render-inlines(letter.contact) }
]
#parbreak()
#if letter.date != none {
  let parts = letter.date.split("-").map(int)
  datetime(year: parts.at(0), month: parts.at(1), day: parts.at(2)).display("[month repr:long] [day padding:none], [year]")
  parbreak()
}
#if letter.recipient != none or letter.company != none {
  block(breakable: false, {
    if letter.recipient != none { letter.recipient }
    if letter.recipient != none and letter.company != none { linebreak() }
    if letter.company != none { letter.company }
  })
  parbreak()
}
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

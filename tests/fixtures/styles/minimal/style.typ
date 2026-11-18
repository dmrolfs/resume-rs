#let resume = json("/resume.json")
#set document(title: resume.name)
#set page(paper: sys.inputs.paper, margin: 1cm)
#set text(font: "Libertinus Serif", size: 12pt)
= #resume.name
Schema #resume.schema

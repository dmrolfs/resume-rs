// Page 1 starts lower than page 2: the body top is the smallest over the document, not the first page's.
#set page(paper: sys.inputs.paper, margin: 1cm, footer: [the footer])
#v(200pt, weak: false)
Lower on the first page.
#pagebreak()
Higher on the second page.

// A clipped block holds text taller than itself: the text is hidden, but is measured at its full size.
#set page(paper: sys.inputs.paper, margin: 1cm)
#block(height: 50pt, clip: true)[#v(900pt) hidden text]

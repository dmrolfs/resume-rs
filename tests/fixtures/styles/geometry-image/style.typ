// A style whose only content is an image taller than any page.
#set page(paper: sys.inputs.paper, margin: 1cm)
#image(bytes("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\"/></svg>"), format: "svg", width: 100pt, height: 900pt)

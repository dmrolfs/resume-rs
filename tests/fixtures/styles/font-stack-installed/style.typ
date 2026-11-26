#import "/lib/font-stack.typ": font-stack

#set document(title: "Installed font fixture")
#set page(paper: sys.inputs.paper)
#set text(font: font-stack((preferred: "Libertinus Serif", fallback: "Carlito")))
Hello from the font stack helper.

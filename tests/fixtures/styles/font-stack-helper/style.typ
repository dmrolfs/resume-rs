#import "/lib/font-stack.typ": font-stack

#set document(title: "Font stack helper fixture")
#set page(paper: sys.inputs.paper)
#set text(font: font-stack((preferred: "Calibri", fallback: "Carlito")))
Hello from the font stack helper.

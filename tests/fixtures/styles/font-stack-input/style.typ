// Asserts what a style sees in sys.inputs.fonts; a wrong value is a compile error the test shows.
#import "/lib/font-stack.typ": font-stack, installed-families

#assert(type(sys.inputs.fonts) == array, message: "sys.inputs.fonts must be an array")
#assert("carlito" in sys.inputs.fonts, message: "carlito must be listed, lowercased")
#assert(sys.inputs.fonts == sys.inputs.fonts.map(lower), message: "every name must already be lowercase")
#assert(installed-families() == sys.inputs.fonts)
#assert(font-stack((preferred: "Calibri", fallback: "Carlito")) == ("Carlito",))
#assert(font-stack((preferred: "Carlito", fallback: "Gelasio")) == ("Carlito", "Gelasio"))
#assert(font-stack((preferred: "Nada", fallback: "Nope")) == ("Nope",))
#assert(font-stack((preferred: "Calibri", fallback: "Carlito"), installed: ()) == ("Carlito",))
#assert(font-stack((preferred: "Calibri", fallback: "Carlito"), installed: none) == ("Calibri", "Carlito"))
#set page(paper: sys.inputs.paper)
ok

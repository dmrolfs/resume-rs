// The font-stack mechanism: keep only the installed names of (preferred, fallback), so Typst never
// sees a missing preferred family and never warns. rsm-141.7.2 moves this helper into components.typ.
#let installed-families() = {
  let given = sys.inputs.at("fonts", default: none)
  if type(given) == str { given.split(",").map(s => lower(s.trim())) } else if given == none { none } else {
    given.map(lower)
  }
}

#let font-stack(role, installed: installed-families()) = {
  let names = (role.preferred, role.fallback)
  if installed == none { return names }
  let present = names.filter(name => lower(name) in installed)
  if present.len() == 0 { (role.fallback,) } else { present }
}

#set page(paper: sys.inputs.paper)
#set text(font: font-stack((preferred: "Calibri", fallback: "Carlito")))
Hello from the font stack helper.

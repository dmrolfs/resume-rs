// The font-stack mechanism: keep only the installed names of (preferred, fallback), so Typst never
// sees a missing preferred family and never warns. rsm-141.7.2 moves this helper into components.typ;
// a test there keeps the two copies identical.

// The families the CLI says are installed, lowercased, or `none` outside the CLI (then nothing is filtered).
#let installed-families() = {
  let given = sys.inputs.at("fonts", default: none)
  if given == none { none } else {
    assert(type(given) == array, message: "sys.inputs.fonts must be an array of family names")
    given.map(lower)
  }
}

// The families of `(preferred, fallback)` that are installed. When neither is, the fallback is returned
// anyway: Typst's warning is then the right signal, and the CLI reports the role as missing.
#let font-stack(role, installed: installed-families()) = {
  assert(installed == none or type(installed) == array, message: "installed must be an array of family names")
  let names = (role.preferred, role.fallback)
  if installed == none { return names }
  let present = names.filter(name => lower(name) in installed)
  if present.len() == 0 { (role.fallback,) } else { present }
}

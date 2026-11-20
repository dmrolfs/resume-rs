<!-- ABOUTME: Sources, versions, checksums, and licenses of the fonts embedded in the resume binary. -->
<!-- ABOUTME: Each font is embedded unmodified; each family's license text sits beside its files. -->

# Bundled fonts

These fonts are compiled into the binary (`src/typeset/fonts.rs`, `FALLBACK_FAMILIES`) so a resume looks
the same on every machine. Each stands in for a proprietary font that the style templates prefer and
that is **never** bundled: Carlito for Calibri, Gelasio for Georgia, Arimo for Arial, TeX Gyre Pagella for Book Antiqua. A style uses the
original when it is installed (or given with `--font-path`), and the stand-in otherwise. The stand-ins for Calibri, Georgia, and Arial
have the same character widths on the characters a resume uses, so line breaks match. TeX Gyre Pagella does not: its widths differ from Book Antiqua's by about 2.4% on average, so a style that falls back to it reports `font-fallback-layout` (a warning). Line positions also depend on the style setting explicit text edges, which the pragmatic theme does.

Every file is the unmodified upstream file: nothing is subset, instanced, converted, or renamed.
Carlito's license names a Reserved Font Name, and embedding the whole file keeps clear of it.

Typst itself also ships fonts through the `typst-assets` crate, linked in through `typst-kit`, so they are in the
binary too. Per that crate's `NOTICE`: Libertinus Serif is OFL-1.1 (Reserved Font Names "Linux Libertine",
"Biolinum", "STIX Fonts"); New Computer Modern is under the GUST Font License v1.0, except `NewCM10-Regular`,
which is GPL-3.0-or-later with the Font Exception and the Distribution Exception; DejaVu Sans Mono is under
the Bitstream Vera license (DejaVu's own changes are public domain). That crate is Apache-2.0, whose section 4(d)
requires a redistribution to carry a readable copy of its `NOTICE`, so the file is vendored verbatim as
[`typst-assets/NOTICE`](typst-assets/NOTICE) and `resume licenses` prints it after the texts above (a test fails
when it differs from the copy in the locked crate, which is how a Typst upgrade shows up). The NOTICE also covers
the other data the crate ships (Foxit fonts, ICC profiles, and W3C data). A search of the built binary finds no
Foxit font (none of `FoxitSans`, `FoxitFixed`, or `FoxitSerif`; rsm-141.10), only the fonts listed here; the notice
is printed whole anyway, so a later change that links them does not leave a license out. Whether the two GPL exceptions cover
shipping `NewCM10-Regular` inside a program is for the owner to confirm; as of rsm-141.13 the README says the question is open, and it is not settled by that task.

This is a summary of the obligations of each license, not legal advice.

## Carlito 1.104 (stands in for Calibri)

- Source: <https://github.com/google/fonts/tree/605c9f29dd5770762bd7ee8ea8ac4ae7664b2b7b/ofl/carlito>
- License: SIL Open Font License 1.1 (`OFL-1.1`), text in [`carlito/OFL.txt`](carlito/OFL.txt)
- Copyright: Copyright 2013 The Carlito Project Authors (https://github.com/googlefonts/carlito), with Reserved Font Name "Carlito"
- Files, SHA-256:
  - `carlito/Carlito-Regular.ttf` `f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595`
  - `carlito/Carlito-Bold.ttf` `bb5d20f79b82599ec72983597437373a80f2d2085fa91fc144fd74e876a594db`
  - `carlito/Carlito-Italic.ttf` `0b019225e58d702bfedcbd35c21696769f8ee115cb6343f84c2f240312450d1c`
  - `carlito/Carlito-BoldItalic.ttf` `b32928186c119599e03ca6a1ffc680fdcb7fac95772f4b95d989cf6cd3861517`

## Gelasio 1.008 (stands in for Georgia)

- Source: <https://github.com/SorkinType/Gelasio/tree/7ab20e7e5c42791e603b9ee3201a0b49849cfdb2/fonts/ttf> (the Google Fonts copy is variable-only; these are the static files from the project)
- License: SIL Open Font License 1.1 (`OFL-1.1`), text in [`gelasio/OFL.txt`](gelasio/OFL.txt)
- Copyright: Copyright 2022 The Gelasio Project Authors (https://github.com/SorkinType/Gelasio)
- Files, SHA-256:
  - `gelasio/Gelasio-Regular.ttf` `48c797fbe0e07c48a18cb962e7bdfa23f19618327dddf54093265328dc9eb39d`
  - `gelasio/Gelasio-Bold.ttf` `e0ef3addf1acf35f5c6aef2be00d0a2c01363bf70a5950e16650976051a0c462`
  - `gelasio/Gelasio-Italic.ttf` `1d8bda4ff258bb8251eae4f7d7500c8d97d084136192bb264bc2305168765d1b`
  - `gelasio/Gelasio-BoldItalic.ttf` `aa87e21b46aef3dcb50a68dc6e1946f0c0a9819d067e4be6a98a6daa8f5b3c16`

## Arimo 1.33 (stands in for Arial)

- Source: <https://github.com/googlefonts/arimo/tree/22e482e2ce0ebb2e6f88ba49d029d60f558d5d67/fonts/ttf> (the Google Fonts copy is variable-only)
- License: SIL Open Font License 1.1 (`OFL-1.1`), text in [`arimo/OFL.txt`](arimo/OFL.txt)
- Copyright: Copyright 2020 The Arimo Project Authors (https://github.com/googlefonts/arimo) (the font files' own notice; the repository's `OFL.txt` header says 2026)
- Files, SHA-256:
  - `arimo/Arimo-Regular.ttf` `41b22bc8f0b51f932825d37bc55b5eb6ba67dfe599a626e4aff2b43b624f9f8c`
  - `arimo/Arimo-Bold.ttf` `d7a8b187cf8444d4cfee102e8eae9e3043682fd5106d5d33ed677fe268a0e2ba`
  - `arimo/Arimo-Italic.ttf` `ffdf690e4357a392e4edcc3df0b3adb0302920f75e54f2e6eec355afbe7edcba`
  - `arimo/Arimo-BoldItalic.ttf` `77c967d49d4ef4a63c39fbcd1e27892102e503e35a20983cb058430c4f268b34`

## TeX Gyre Pagella 2.501 (stands in for Book Antiqua)

- Source: <https://www.gust.org.pl/projects/e-foundry/tex-gyre/pagella/qpl2_501otf.zip>. The CTAN files (<https://mirrors.ctan.org/fonts/tex-gyre/opentype/>) are byte-identical.
- License: GUST Font License v1.0 (22 June 2009), text in [`pagella/GUST-FONT-LICENSE.txt`](pagella/GUST-FONT-LICENSE.txt). It is an instance of the LaTeX Project Public License 1.3c (LPPL). The TeX Gyre README and MANIFEST are beside the fonts: [`pagella/README-TeX-Gyre-Pagella.txt`](pagella/README-TeX-Gyre-Pagella.txt) and [`pagella/MANIFEST-TeX-Gyre-Pagella.txt`](pagella/MANIFEST-TeX-Gyre-Pagella.txt) (from CTAN, version 2.501, matching the fonts; the GUST site now serves 2.609).
- Copyright: Copyright 2006-2018 for TeX Gyre extensions by B. Jackowski, J.M. Nowacki, et al. (on behalf of TeX USERS GROUPS). Vietnamese characters were added by Han The Thanh.
- Files, SHA-256:
  - `pagella/texgyrepagella-regular.otf` `44e64260716d8f2bbe412baa1ee99b7c995190ac4573177c24def0b9200438c7`
  - `pagella/texgyrepagella-bold.otf` `dd4f9b69f0b24f1fb9be8634abf1fce2009e5c68a30842a3e9f5d6e535900abd`
  - `pagella/texgyrepagella-italic.otf` `069bb27f48eb98a741715227fc6e4b7baf3639c29fc13925f70d1ed8884ac29a`
  - `pagella/texgyrepagella-bolditalic.otf` `556fd64655a92bd9f5485ae2c03f7b9cdaf745a27a91d7edb3c3abde997206d2`
- Notice files kept beside the fonts, SHA-256 (so a truncated copy is noticed):
  - `pagella/GUST-FONT-LICENSE.txt` `9372accba7de09e11b7e1d0a6a27d6703c5e329a60218032805e0accca1b8eeb`
  - `pagella/README-TeX-Gyre-Pagella.txt` `6fc4c72a8754f7e04ffbbec99167a568c0a3d21e231318ed624b58ddfeb1f896`
  - `pagella/MANIFEST-TeX-Gyre-Pagella.txt` `6ac4bc1448a1d71a3a7ad5fd13566f00855b37ca4db5b20451c88806ac3f5242`

## What the GUST Font License and the LPPL ask of a distribution that embeds Pagella

Shipping four of the Work's files counts as distributing part of the Work, and the LPPL (version 1.3c, clause 2) treats that as modifying the Work, even though the four files themselves are unchanged.
So the conditions of clause 6 for distributing a Derived Work apply, and this program meets them as follows:

- No change was made. The four files are the unmodified upstream files (the checksums above tie them to the table), so there is no changed component to identify (6a) or to describe in a change notice (6b), and the request in the GUST license to rename modified fonts does not arise. The request is a request, not a legal requirement.
- No endorsement by the authors is implied, and no support from them is implied (6c). The authors of TeX Gyre Pagella have not reviewed or endorsed this program and do not offer support for it.
- The information needed to obtain the complete unmodified Work is given (6d): the GUST page <https://www.gust.org.pl/projects/e-foundry/tex-gyre/pagella>, and CTAN, <https://ctan.org/pkg/tex-gyre> (files under <https://mirrors.ctan.org/fonts/tex-gyre/>). The GUST license, the README, and the MANIFEST travel with the fonts.

Embedding the fonts in a program that has other, unrelated parts is aggregation (LPPL clause 11).

## What the OFL asks of a distribution that embeds Carlito, Gelasio, and Arimo

- Keep each family's copyright notice and license text with the fonts. They are in this directory.
- Do not sell the fonts by themselves. They are part of a program.
- Do not use a Reserved Font Name (Carlito) for a modified version. These files are not modified.
- Do not use the names of the copyright holders or authors to promote the product without their permission.
- Typst subsets fonts when it embeds them in a PDF. That is about the generated documents; the files in this
  repository and in the binary are whole and unmodified. The OFL does not apply to documents made with the
  fonts, which is the common reading of its preamble; this is not legal advice.

## How the stand-ins were checked

`cargo test --lib fallback_widths_match_the_originals -- --ignored`, with `RESUME_ORIGINAL_FONTS_DIR`
pointing at a directory of `Calibri-`, `Georgia-`, and `Arial-` files in Regular, Bold, Italic, and
BoldItalic, compares the advance width of every printable ASCII character and the punctuation a resume
uses (en and em dash, apostrophe, quotation marks, bullet) and fails above 0.2%. The originals are
proprietary and are never committed, so the test is ignored by default and reported as ignored.
Calibri against Carlito matches exactly on those characters. Georgia against Gelasio differs only in rare
Latin-1 and Latin Extended-A characters outside that set (for example the fraction signs). Arimo matches
Arial as shipped with macOS; an older Arial's middle dot is wider.

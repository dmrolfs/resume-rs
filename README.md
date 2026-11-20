<!-- ABOUTME: Overview of resume-rs, a CLI that typesets Markdown resumes into print-quality PDFs. -->
<!-- ABOUTME: Install, a quick start, fonts and licenses, and troubleshooting; the full guide is docs/User-Guide.md. -->

# resume-rs

`resume` typesets resumes written in Markdown into print-quality PDFs using embedded
[Typst](https://typst.app). It ships styles modeled on the templates that
*The Tech Resume Inside Out* and Standard Resume recommend for experienced engineers and leaders, and a checker,
`resume doctor`, that says what is wrong with a resume, why it matters, and fixes what is safe to fix.

This page is for installing it and getting a first PDF. The whole reference (how to write the Markdown, every
command and option, the styles and how to change them, and every finding the checker can report) is the
[User Guide](docs/User-Guide.md).

## Install

Download the archive for your system from the [releases page](https://github.com/dmrolfs/resume-rs/releases),
together with its `.sha256` file (or `SHA256SUMS`). Each archive holds one directory with the program, `LICENSE`,
`README.md`, and `FONT-LICENSES.md`.

Check the download before you unpack it. On macOS, in the directory with both files:

```sh
shasum -a 256 -c resume-v0.1.0-macos-universal2.tar.gz.sha256
```

On Windows, in PowerShell, compare the output of this with the line in the `.sha256` file:

```powershell
(Get-FileHash resume-v0.1.0-windows-x64.zip -Algorithm SHA256).Hash
```

### macOS

The archive is a universal2 binary: it runs natively on Apple silicon and on Intel Macs, from macOS 11.

```sh
tar -xzf resume-v0.1.0-macos-universal2.tar.gz
mkdir -p ~/.local/bin
cp resume-v0.1.0-macos-universal2/resume ~/.local/bin/
resume --version
```

`~/.local/bin` is only an example: use any directory on your `PATH`. If `resume` is not found, add this line to
`~/.zshrc` and open a new terminal: `export PATH="$HOME/.local/bin:$PATH"`.

The program is signed ad hoc and is not notarized, because that needs a paid Apple developer account. A file
downloaded in a browser is quarantined, and Gatekeeper then refuses to open it (the message says the developer
cannot be verified, or that Apple could not check it for malware, depending on the macOS version). Either remove the
quarantine mark once:

```sh
xattr -d com.apple.quarantine ~/.local/bin/resume
```

(`No such xattr` means the file was not quarantined, which is fine), or open System Settings, Privacy & Security, and
choose Open Anyway after the first refusal. A file fetched with `curl` is not quarantined and needs neither.

### Windows

The archive is a zip for 64-bit Windows (x64). Unzip it, then put `resume.exe` in a directory on your `PATH`,
or run it from where it is.

```powershell
Expand-Archive resume-v0.1.0-windows-x64.zip .
.\resume-v0.1.0-windows-x64\resume.exe --version
```

`resume.exe` needs no Visual C++ runtime or other install: the build links its runtime in, and CI checks that it imports
only system DLLs. It is not code-signed, so SmartScreen may say "Windows protected your PC". Choose More info, then
Run anyway. Right-clicking the zip, choosing Properties, and ticking Unblock before you unzip it avoids the message.

### From source

```sh
cargo install --locked --path .
```

needs Rust 1.92 or later (see [Development](#development)).

## Quick start

Save this as `Resume.md`. It is a whole resume, small enough to read at a glance.

```markdown
# Ada Lovelace

London, UK · ada@example.com

## Summary

Engineer who built the first published program and led a team of 4 through 3 releases.

## Experience

### Analytical Engines Ltd · Principal Engineer · 2017 – Present

Builds mechanical computers for scientific customers.

- Cut the cost of a weekly run from $39K to $29K by caching 80% of the tables.
- Led a team of 12 to deliver the engine in 18 months.
```

Then, in the directory that holds `Resume.md`:

```sh
resume check Resume.md
resume build Resume.md
```

`check` reads the resume and reports what is wrong with it; it never changes a file. `build` writes `out/Resume.pdf`.
The commands you will use next:

| To | Run |
|---|---|
| Fix the mechanical problems (dates, separators, trailing spaces), after looking at the diff | `resume doctor Resume.md --fix --dry-run`, then `resume doctor Resume.md --fix` |
| Learn what a finding means | `resume doctor --explain period-hyphen` |
| Build in another style or on A4 paper | `resume build Resume.md --style mono --paper a4` |
| Make the PDF the same on any machine, and fail past two pages | `resume build Resume.md --fonts bundled --max-pages 2` |
| Rebuild whenever you save | `resume watch Resume.md --open` |
| See the styles | `resume styles` |
| Copy a style to edit it | `resume export-style pragmatic my-styles`, then `resume build Resume.md --styles-dir my-styles` |
| Preview a style in tinymist or `typst watch` | `resume export Resume.md preview` |

`--dry-run` comes before or after any command and shows what would be written without writing it (`watch` refuses it,
since watching without writing has no purpose). `--format json` prints findings as JSON lines for a script, and `--color never` or `NO_COLOR` turns
off color. The [User Guide](docs/User-Guide.md) has every option.

The built-in styles are `pragmatic` (the default, modeled on the Pragmatic Engineer's template), `mono` (the same
body, no color, a centered header), and `experienced` (section labels in a left gutter).

## Fonts

The program contains fonts, so a resume looks the same on every machine. Each stands in for a proprietary font the
templates prefer and that is never bundled: Carlito for Calibri, Gelasio for Georgia, Arimo for Arial, and TeX Gyre
Pagella for Book Antiqua. A style uses the original when it is installed, and the stand-in otherwise. Carlito, Gelasio,
and Arimo have the same character widths as the fonts they replace, so line breaks and page counts match. TeX Gyre
Pagella does not (about 2% off on average), so a build that falls back to it says `font-fallback-layout`.

- `--fonts auto` (the default) uses the bundled fonts, then the ones you point it at, then the fonts installed on
  the machine. On macOS it also looks in Microsoft Office's own font folders, so Calibri is found if Office is
  installed even though macOS does not list it as a system font.
- `--fonts bundled` uses only the fonts in the program. The PDF is then byte for byte the same on macOS and Windows,
  which is what to use when you compare builds.
- `--font-path DIR` (repeatable) or the `RESUME_FONT_PATH` environment variable adds a directory of font files. On
  Windows separate several directories in `RESUME_FONT_PATH` with `;`, elsewhere with `:`.

The summary line of a build names the font each role resolved to: `fonts: body Calibri (application); ...`.

## Troubleshooting

| You see | It means | Do this |
|---|---|---|
| `error: ... is in use by another program` | A PDF viewer holds `out/Resume.pdf` open. Windows will not replace an open file. Acrobat does this; Skim and SumatraPDF do not. | Close the viewer, or use `-o OTHER_DIR`. `resume watch` reports it and keeps going. |
| `error: unknown style` | The name is not a built-in style or a directory in `--styles-dir`. The message lists the styles that exist. | Run `resume styles`. A style name is lower case. |
| `error[role-separator]`, `error[era-shape]`, ... | The Markdown does not follow the format. No PDF is written until the errors are fixed. | `resume doctor FILE`. Many errors have a fix: `resume doctor FILE --fix`. |
| `warning[font-fallback-layout]` | A stand-in font has different widths, so lines may break differently from the original. | Install the original font, or check the page count. |
| `error[font-missing]` | Neither the preferred font nor its fallback was found. | Give `--font-path` a directory that has one of them, or install it. |
| `error[page-limit-exceeded]` | The resume needs more pages than `--max-pages`. The PDF is still written, so you can see what overflowed. | Shorten it, or raise the limit. |
| A PDF with a different page count than a colleague's | Different fonts. | Both use `--fonts bundled`. |
| `error: ... resolves outside the styles directory` | A file in a style is a link that leaves the styles directory. | Copy the file in. |
| `resume doctor --explain CODE` says the code is not one | A typo. The message suggests the close ones. | Use a code from the report, which is printed with each finding. |

Exit codes: `0` success (warnings allowed), `1` an error finding or a failed build, `2` a usage error. A script can
rely on them, and on `resume check` never writing a file.

## Reporting a problem

Open an issue at <https://github.com/dmrolfs/resume-rs/issues> with the output of `resume --version`, the command you
ran, and the report. Remove personal details from any text you paste: a resume holds a phone number and an address.

## Using it

Build and install the program from a checkout with `cargo install --path .` (it needs Rust 1.92 or later). The [User Guide](docs/User-Guide.md) says how to write a resume, check it, choose a style and a font, and what every command does.

## Development

```bash
just           # list every recipe, grouped
just check     # the quality gate: cargo fmt --check, cargo clippy -D warnings, cargo machete, cargo test
```

Common recipes (run `just` for the full list):

| Recipe | What it does |
|---|---|
| `just check` | The gate every task runs before closing |
| `just ci` | One CI leg: the gate, then a locked release build |
| `just run build resume.md` | Run the CLI with arguments |
| `just test-one NAME` | Run tests whose name contains `NAME` |
| `just watch` | Re-run the gate on every change (needs `cargo-watch`) |
| `just unused-deps` | Fail on a dependency nothing uses (part of `just check`; needs `cargo-machete`) |
| `just doc-check` | Build the API docs and fail on any rustdoc warning |
| `just coverage` | Line coverage summary (needs `cargo-llvm-cov`) |
| `just audit` | Check dependencies for known vulnerabilities (needs `cargo-audit`) |
| `just verify` | Everything to run before pushing, including both cross-builds (macOS only) |

`just check` needs `cargo-machete` (`cargo install cargo-machete --locked --version 0.9.2`, the version CI pins; without it the gate fails with cargo's "no such command: `machete`"); dependencies are added when first used, and the gate enforces it (see Dependency hygiene in `ref/Resume-RS-Architecture.md`). Recipes that need an optional tool print the `cargo install` command if it is missing. The recipes without a
platform attribute also run on Windows, where `just` uses PowerShell, so they must stay plain commands:
`tests/justfile_test.rs` enforces this, because a recipe that breaks loading on Windows would break all of them.

Requires Rust 1.92 or later.

### Building for macOS and Windows

```bash
just build-macos     # universal2 (arm64 + x86_64) binary at target/universal2/resume, re-signed and verified (macOS only)
just build-windows   # quick windows-gnu cross-build of resume.exe plus a self-contained import check (needs mingw-w64)
```

`build-windows` is a fast compile and link check. Release binaries are built natively with MSVC in CI,
with the C runtime linked statically (`.cargo/config.toml`) so `resume.exe` runs on a clean Windows machine.
To inspect any Windows exe from your Mac, run `bash scripts/check-pe-imports.sh exe path/to/resume.exe`
(needs `rustup component add llvm-tools`).
A binary cross-built on a Mac cannot be tested there, so behavior on Windows is verified by the CI
matrix, which runs `just check` on macOS and Windows for every push.

### Portability rules

These keep output identical on macOS and Windows:

- **Paths.** Use `Path`/`PathBuf` and build them from components. Never concatenate with `/` or `\`.
- **Line endings.** Text is LF in the repo and in checkouts (`.gitattributes`). Code that reads user files
  must accept LF and CRLF and keep spans as byte offsets into the original text. Fixtures that need CRLF
  live in `tests/fixtures/crlf/` and are marked `-text`.
- **Fonts, PDFs, and images** are binary in `.gitattributes`; never let git convert them.
- **Replacing output files.** Write a temp file in the same directory and persist it. A PDF open in a
  viewer is locked on Windows, so report that case instead of failing opaquely.
- **No `-C target-cpu=native`** and no reliance on the machine's clock, locale, or installed fonts for
  anything that affects output bytes.
- **Don't hash a windows-gnu build against the MSVC build.** The gnu build is only a local compile check.

## Licenses

The program's own code is MIT (`LICENSE`). The binary also embeds fonts, which keep their own licenses, and the
archives carry them in `FONT-LICENSES.md`. The `license` field in `Cargo.toml` (`MIT AND OFL-1.1`) names the first two
and does not list every font license below.

- Carlito, Gelasio, and Arimo are under the SIL Open Font License 1.1, and TeX Gyre Pagella is under the GUST Font
  License; each family's license text sits beside its files in `fonts/`, with the sources, versions, and checksums in
  [`fonts/LICENSES.md`](fonts/LICENSES.md).
- Typst's own fonts (Libertinus Serif, New Computer Modern, DejaVu Sans Mono) come with the `typst-assets` crate under
  their own licenses, listed in `fonts/LICENSES.md`. New Computer Modern's `NewCM10-Regular` is GPL-3.0-or-later with
  the Font Exception and the Distribution Exception; whether those cover embedding the font in this program is for
  the owner to confirm, and is not settled by this README.
- `resume licenses` prints every license notice the binary has to carry: the font texts and the `typst-assets`
  `NOTICE` (Apache-2.0, section 4(d)). Run it, and keep its output with any copy of the program you give someone.
- The Rust crates linked into the program (about 400, nearly all MIT or Apache-2.0) are not printed by `resume licenses`.
  `cargo tree` lists them and their licenses are in their own packages; an attribution list for them is not
  generated yet.

This is a summary of the obligations of each license, not legal advice.

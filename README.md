<!-- ABOUTME: Overview of resume-rs, a CLI that typesets Markdown resumes into print-quality PDFs. -->
<!-- ABOUTME: Stub for now; the full guide (writing, styling, commands) lands in task rsm-141.13. -->

# resume-rs

`resume` typesets resumes written in Markdown into print-quality PDFs using embedded
[Typst](https://typst.app). It ships styles modeled on the templates that
*The Tech Resume Inside Out* and Standard Resume recommend for experienced engineers and leaders.

Status: under development. See `ref/Resume-RS-Architecture.md` for the design.

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

The program is MIT licensed (`LICENSE`). The binary also embeds fonts, which keep their own licenses:

- `fonts/` holds Carlito, Gelasio, and Arimo under the SIL Open Font License 1.1, with each family's license
  text beside its files and the sources, versions, and checksums in [`fonts/LICENSES.md`](fonts/LICENSES.md).
  They stand in for Calibri, Georgia, and Arial, which are never bundled.
- Typst's own fonts (Libertinus Serif, New Computer Modern, DejaVu Sans Mono) come with the `typst-assets`
  crate under their own licenses, listed in `fonts/LICENSES.md`.
- `bundled_font_notices()` returns the notices for the bundled fallback fonts as text, so the CLI can print them.

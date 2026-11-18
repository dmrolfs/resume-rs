<!-- ABOUTME: Tiered implementation plan for the resume-rs epic (rsm-141), one entry per beads child task. -->
<!-- ABOUTME: Lists purpose, scope, complexity, and verification per task; detailed steps come at task start. -->

# resume-rs: Implementation Plan

**Epic ID:** rsm-141
**Date:** 2026-11-17

---

## Task Breakdown

### Tier 1: No dependencies
- **rsm-141.2**, Scaffold: crate, lints, rustfmt, justfile, pinned dependencies.

### Tier 2: Depends on Tier 1
- **rsm-141.3**, Model, validated newtypes, periods, and diagnostic types: the contract that the parser and the renderer share.

### Tier 2b: Depends on rsm-141.2
- **rsm-141.14**, Cross-platform builds and CI matrix: macOS universal2 and Windows x64 via a local cross-build, plus CI on both OSes with a PDF hash comparison.

### Tier 3: Depends on rsm-141.3 (these three can run in parallel)
- **rsm-141.4**, Markdown parser and format lints.
- **rsm-141.5**, Diagnostic rendering, human and JSON.
- **rsm-141.6**, Typst World, compile, and deterministic PDF export.

### Tier 1b: No code dependencies
- **rsm-141.1**, Font selection spike: a comparison sheet of template fonts vs. free substitutes. Damon decides.

### Tier 4: Depends on rsm-141.6 and rsm-141.1
- **rsm-141.7**, Shared components, bundled fonts, and the `pragmatic` style.

### Tier 5: Depends on rsm-141.7 (and rsm-141.4 and rsm-141.5 for the CLI)
- **rsm-141.8**, The `mono` style.
- **rsm-141.9**, The `experienced` style.
- **rsm-141.10**, CLI (`build`, `styles`, `export-style`), global `--dry-run`, and the layout report. Also depends on rsm-141.4 and rsm-141.5.

### Tier 6 (depends on rsm-141.10; the two can run in parallel)
- **rsm-141.11**, The `doctor` command: content advice, setup checks, `--explain`, `--fix`. Also depends on rsm-141.4.
- **rsm-141.12**, The `watch` and `export` commands.

### Tier 7
- **rsm-141.13**, README, release artifacts, and migration of the baselines. Depends on rsm-141.14, rsm-141.8, rsm-141.9, rsm-141.11, and rsm-141.12.

## Per-Task Summary

### rsm-141.2: Scaffold resume-rs crate, lints, tooling

**Purpose:** Every later task should start from passing quality gates and the same conventions as the other dmrolfs crates.

**Scope:**
- Modify: `Cargo.toml`, `src/main.rs`, `.gitignore`
- Create: `src/lib.rs`, `rustfmt.toml`, `justfile`, `README.md`, `LICENSE`, `tests/cli_test.rs`
- Dependencies: only `clap` (derive), plus dev dependencies `assert_cmd` and `predicates`. Cargo 1.100 warns on declared-but-unused dependencies, and since rsm-141.17 `just check` fails on one (the `unused_dependencies` lint plus `cargo machete`; see Dependency hygiene in the architecture doc), so each dependency is added by the task that first uses it:
  - rsm-141.3: nutype (serde, regex), regex, serde, serde_json, time (serde), strum (derive), url (serde); dev: insta (json), claims, pretty_assertions
  - rsm-141.4: pulldown-cmark; dev: proptest
  - rsm-141.5: codespan-reporting
  - rsm-141.6: typst, typst-pdf, typst-kit (`=0.15.1` exactly; typst-kit with embedded-fonts and scan-fonts), include_dir, snafu, tempfile; dev: lopdf
  - rsm-141.10: humantime
  - rsm-141.11: similar
  - rsm-141.12: notify, opener

**Complexity:** Mechanical setup.

**Verification:**
```bash
just check
```

### rsm-141.3: Resume model, validated newtypes, periods, and diagnostic types

**Purpose:** Fix the JSON contract early, and make invalid domain values impossible to construct, so the styles and the parser can be built against it independently.

**Scope:**
- Create: `src/model/{mod,text,period,options}.rs`, `src/diagnostics/{mod,code}.rs` (types only; advice entries come in rsm-141.4 and rsm-141.11)
- Modify: `src/lib.rs`

**Complexity:** Greenfield types: nutype, strum, and serde derives.

**Verification:**
```bash
cargo test model && cargo test diagnostics && cargo clippy --all-targets -- -D warnings
```

### rsm-141.4: Markdown parser with located diagnostics and format lints

**Purpose:** Close the biggest feedback gap. Every convention must be parsed, and every violation reported with a location and a hint. Mechanical problems also carry an `Edit`.

**Scope:**
- Create: `src/markdown/{mod,blocks,inline,period,lint}.rs`, `tests/parse_test.rs`, `tests/fixtures/{full,errors-*,warnings-*}.md`

**Complexity:** Greenfield module. This is the largest task. It builds a small state machine over `pulldown-cmark` offset events.

**Verification:**
```bash
cargo test --test parse_test && cargo test markdown
RESUME_BASELINES=~/Documents/vrm/my/job/drafts cargo test --test parse_test -- --ignored
```

### rsm-141.5: Diagnostic rendering (human and JSON)

**Purpose:** Diagnostics should read like `rustc` and `typst` output, and tools should be able to consume them.

**Scope:**
- Create: `src/diagnostics/lines.rs` (shared line index), `src/diagnostics/render/{mod,sources,excerpt,human,json,color,summary}.rs` (codespan rendering, JSON lines, summary); `tests/diagnostics_test.rs`, `tests/diagnostics_property_test.rs`
- Modify: `src/model/options.rs` (`ColorMode`), `src/markdown/mod.rs` (delegates `line_of` and span snapping to `lines.rs`)

**Complexity:** Small integration.

**Verification:**
```bash
cargo test diagnostics
```

### rsm-141.6: Typst World, compile, and deterministic PDF export

**Purpose:** Prove embedded compile, PDF export, determinism, and diagnostics mapping before any real style exists.

**Scope:**
- Create: `src/typeset/{mod,world,fonts,pdf}.rs`, `src/style.rs`, `src/error.rs`, `tests/render_test.rs`, `tests/fixtures/styles/minimal/style.typ`

**Complexity:** Integration with an unstable, pre-1.0 API. Confirm the exact 0.15.1 signatures (`LibraryExt::builder`, `typst::compile::<PagedDocument>`, `typst_pdf::pdf`) against docs.rs before writing code.

**Verification:**
```bash
cargo test --test render_test
```

### rsm-141.7: Shared components, bundled fonts, and pragmatic style

**Purpose:** Provide the book's default style and the components every style reuses, including the layout probes that `check` depends on.

**Scope:**
- Create: `styles/lib/components.typ`, `styles/pragmatic/{style,theme}.typ`, `styles/pragmatic/README.md`, `fonts/*` plus `fonts/LICENSES.md`
- Modify: `src/typeset/fonts.rs` (embed the bundled fonts), `tests/render_test.rs`

**Complexity:** Greenfield Typst code, plus visual review against the Pragmatic docx and Parker.

**Verification:**
```bash
cargo test --test render_test pragmatic
```

### rsm-141.8: mono style

**Purpose:** Provide a layout that emphasizes the title, for the org leader resumes.

**Scope:**
- Create: `styles/mono/{style,theme}.typ`, `styles/mono/README.md`

**Complexity:** Greenfield Typst that reuses the components.

**Verification:**
```bash
cargo test --test render_test mono
```

### rsm-141.9: experienced style

**Purpose:** Provide the book's top pick for principal-level candidates: section labels in a left gutter and skills as a label and value table.

**Scope:**
- Create: `styles/experienced/{style,theme}.typ`, `styles/experienced/README.md`

**Complexity:** Greenfield Typst. The grid layout is the most involved of the three styles.

**Verification:**
```bash
cargo test --test render_test experienced
```

### rsm-141.10: CLI: build, styles, export-style, global --dry-run, and layout report

**Purpose:** Deliver feedback to the user: summaries with page counts and timings, layout checks, one consistent `--dry-run`, and dependable exit codes.

**Scope:**
- Create: `src/cli.rs`, `src/layout.rs`
- Modify: `src/main.rs`, `src/lib.rs`, `tests/cli_test.rs`

**Complexity:** Integration.

**Verification:**
```bash
cargo test --test cli_test && cargo test layout
```

### rsm-141.11: doctor command: content advice, setup checks, --explain, and --fix

**Purpose:** Turn findings into fixes. Explain why each one matters, citing the book, say how to fix it, and apply mechanical fixes safely.

**Scope:**
- Create: `src/doctor/{mod,content,setup,fix}.rs`, `tests/doctor_test.rs`
- Modify: `src/diagnostics/code.rs` (advice catalog for every code), `src/cli.rs` (`doctor` plus the read-only `check` alias)

**Complexity:** Greenfield module. Most of the work is the advice catalog and the content heuristics, which need tests for both the positive and the negative case.

**Verification:**
```bash
cargo test --test doctor_test && cargo test doctor
```

### rsm-141.12: watch and export commands

**Purpose:** Fast feedback while editing, and live preview of styles in an editor through tinymist. `--debounce` is parsed with `humantime`, and `watch` rejects `--dry-run`.

**Scope:**
- Create: `src/watch.rs`
- Modify: `src/cli.rs`, `tests/cli_test.rs`

**Complexity:** Small integration with timing-sensitive tests. Keep them timeout-bounded.

**Verification:**
```bash
cargo test --test cli_test export watch
```

### rsm-141.13: README and migration of job-workspace baselines

**Purpose:** resume-rs replaces `tools/resume-pdf` only once the real baselines build in every style and the guide is complete.

**Scope:**
- Create: `tests/baselines_test.rs`
- Modify: `README.md`, and a pointer in `~/Documents/vrm/my/job/tools/resume-pdf/README.md`

**Complexity:** Documentation plus one ignored integration test.

**Verification:**
```bash
RESUME_BASELINES=~/Documents/vrm/my/job/drafts cargo test --test baselines_test -- --ignored
just check
```

## Sequencing Notes

- rsm-141.4, rsm-141.5, and rsm-141.6 are independent once rsm-141.3 lands. rsm-141.6 is the riskiest because of the Typst API, so start it first.
- rsm-141.8 and rsm-141.9 can start as soon as rsm-141.7 lands, in parallel with rsm-141.10.
- rsm-141.11 and rsm-141.12 can run in parallel once rsm-141.10 lands.
- Visual review gate: after rsm-141.7, rsm-141.8, and rsm-141.9, render both baselines and review the page images with the user before closing the task.
- Real resume content stays out of the repo. Tests use synthetic fixtures. Baseline tests are `#[ignore]` and read `RESUME_BASELINES`.

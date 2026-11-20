<!-- ABOUTME: Plans two cover-letter epics and a bounded advisor evaluation with real task dependencies. -->
<!-- ABOUTME: Implementation is not started; Beads is the authoritative task status source. -->
# Cover letters: implementation plan

Date: 2026-11-20 UTC.
Status: planned only; Beads IDs and dependency verification are recorded below after filing.

## Purpose and scope

Extend `resume-rs` with authored cover letters, first-class website text and optional PDF, then the existing tags/core/status/alternates and position/profile machinery.
Keep model assistance in the existing separate Resume Advisor effort.
Do not silently broaden rsm-141's original PDF-only scope: E1 and E2 are separate top-level epics related to that work.
A1 is a true child task of rsm-eam; it does not replace or widen rsm-eam.1.

[Architecture](Cover-Letters-Architecture.md) defines the user-facing contract, models, data flow and corrections to Oracle's preliminary recommendations.
[Analysis](Cover-Letters-Analysis.md) preserves all three Oracle reports verbatim.
[Test plan](Cover-Letters-Test-Plan.md) maps invariants to each task's tests.
Every filed task also includes its relevant contract directly so the Beads record remains understandable independently.

## Epic E1: deterministic letter core

Ship a useful text/PDF letter command without waiting for the open tailoring/spec tasks.
Before annotations are supported, annotation-shaped comments are errors, never silently discarded.
No general template language, model client, arbitrary document plugin layer or generated letter prose is included.

| Alias | Deliverable | Blockers | Estimate |
|---|---|---|---|
| D1 | Located domain/source/selected/resolved models and field resolution | None | 0.75 to 1 day |
| D2 | Strict Markdown grammar, placeholder spans and annotation reservation | D1 | 1 to 1.5 days |
| D3 | Exact website text, link handling, counts and limits | D1 | 0.5 to 1 day |
| D4 | Text-first CLI, effective settings and diagnostic streams | D2, D3 | 0.75 to 1 day |
| D5 | Optional PDF style, sandbox/cache integration and output modes | D4 | 1 to 1.5 days |
| D6 | Guide, synthetic examples, compatibility and release evidence | D5 | 0.5 to 1 day |

Total estimate: 4.5 to 7 working days, excluding owner review latency and integration changes.
D2 and D3 can proceed in parallel after D1.
D5 depends on D4 because it edits the letter command; it is not a parallel task pretending that command already exists.
D6 owns final integration evidence, not tests that earlier tasks should have written.

## Epic E2: tailoring and application specs

Reuse, rather than replace, rsm-141.22 annotations/selection/flags and rsm-141.23 spec loading/CLI.
Each task waits for the actual upstream API and carries only the letter adaptation.

| Alias | Deliverable | Blockers | Estimate |
|---|---|---|---|
| T1 | Letter annotation/group recognition | D2, rsm-141.22.1 | 0.5 to 1 day |
| T2 | Shared-rule letter selection | T1, rsm-141.22.2 | 0.5 to 1 day |
| T3 | Shared tailoring CLI flags and reports | T2, D4, rsm-141.22.3 | 0.5 day |
| T4 | Strict letter tables and independent profile selection | T2, D4, rsm-141.23.1 | 0.75 to 1.25 days |
| T5 | letter --spec, end-to-end compatibility and release | T3, T4, D6, rsm-141.23.2 | 0.75 to 1 day |

Total estimate: 3 to 4.75 working days, excluding upstream work.
T4 explicitly depends on D4 for output/settings types and transitively D3 for text-limit/link types.
T5 waits for complete E1 release evidence through D6, not just a partially wired command.
The same profile chain resolves two independent scopes; no cross-document content dependency is introduced.

## Advisor A1: bounded evaluation

One true task under rsm-eam evaluates local-model letter selections and evidence-linked review for two real postings.
Depends on rsm-eam.1 for tool-home/go-no-go evidence and T5 for working letter --spec output.
Estimate: 1 to 1.5 days.
It does not generate prose, modify the baseline, ship a production advisor or expand the original resume spike.
A no-go result is valid evidence; it is not permission to invent a replacement product.

## Proposed file map and ownership

These are target paths, not claims that proposed modules already exist.
Actual spec/selection helper paths follow the upstream tasks when they land.

| Task | Main implementation paths | Verification |
|---|---|---|
| D1 | `src/letter/{mod,source,model,resolve}.rs`, `src/lib.rs`, relevant diagnostics | Direct-value unit/resolve tests, `cargo test --lib letter`, `just check` |
| D2 | `src/markdown/letter.rs`, shared Markdown helpers, `src/diagnostics/{code,advice}.rs` | `tests/letter_parse_test.rs`, span/property tests, `just check` |
| D3 | `src/letter/plain.rs`, optional `limit.rs` | Direct-model golden/count/limit tests and targeted mutants, `just check` |
| D4 | `src/cli.rs`, `src/commands/{letter,mod,report,emit}.rs` as needed | `tests/letter_cli_test.rs`, stream/absence-of-Typst tests, `just check` |
| D5 | `src/style.rs`, `src/typeset/{world,mod}.rs`, `styles/pragmatic/letter.typ`, shared components, letter/styles commands | `tests/letter_pdf_test.rs`, mixed-world cache tests, resume regressions, `just check` |
| D6 | Guide/README/reference docs, synthetic fixtures, existing CI coverage | Native macOS/Windows tests, `just verify` on provisioned macOS, owner text/PDF review |
| T1 | Letter parser/source and upstream annotation helper boundary | Shared grammar/grouping diagnostic tests, `just check` |
| T2 | `src/letter/select.rs`, shared selection helper boundary | Selection parity/properties/mutants and selected-field tests, `just check` |
| T3 | CLI shared selection args and letter/report command | Flag parity and stream regressions, `just check` |
| T4 | Upstream spec module (proposed `src/spec/`), letter settings conversion | Strict independent-scope/path/precedence tests and mutants, `just check` |
| T5 | Spec CLI helpers, letter command, guide and reference examples | `tests/letter_spec_test.rs`, reproducibility/native CI, `just check`, provisioned `just verify` |
| A1 | Private harness at tool home chosen by rsm-eam.1 | Two postings, owner corrections, local/remote privacy and invalid-output tests |

D1 owns field validation and resolution; D2 owns placeholder recognition and syntax/source diagnostics.
D1/D3 tests construct values directly and cannot require a parser that has not been implemented.
D5 retains Option-valued explicit settings from the start, instead of inheriting resume clap defaults that would later clobber spec values.

## Upstream coordination

Add non-destructive notes to the OPEN tasks:
- rsm-141.22.1: one annotation-shape predicate and item-level grouping usable by letters; coordinate ownership if the letter reservation helper lands first.
- rsm-141.22.2: callable item keep/alternate resolver and container-independent report; explicitly decide/test selected-draft-variant behavior.
- rsm-141.22.3: reusable clap Args for the same selection flags.
- rsm-141.23.1/.23.2: letter work adds tables later, preserve upstream scope and override-policy ownership; no new dependencies that block resume-only delivery.
- rsm-eam: new letter evaluation child, original .1 scope retained.

These are coordination notes, not replacements for existing acceptance criteria.
Letter tasks own any necessary adaptation/refactoring not provided upstream.

## Integration and completion rules

Use an approved integration base containing main dee46ac or later equivalent APIs.
The current worktree has unrelated modifications; do not reset, rebase or switch it automatically.
Leave `.gitignore` and `ref/Resume-RS-Implementation-Plan.md` untouched without further owner approval.
History is ignored, so this planning session preserves the synthesis and full raw appendices under new `ref/` paths rather than waiting for implementation to make the design available.

Before closing each implementation task, record separate Rust code-review and test-review outcomes, requirement-to-test evidence and actual gate results.
Run targeted property/mutation tests where assigned; explain surviving mutants rather than counting them as success.
E1/E2 completion requires native platform runtime tests, unchanged resume behavior and owner readability evidence.
Do not close epics merely because their plans are written.
This session files all implementation tasks open and performs no implementation, commit, push or remote Beads sync.

## Filed Beads mapping and verified graph

Filing results are appended here once IDs have been assigned and verified.

| Alias | Bead | Type | Parent | Blocking prerequisites |
|---|---|---|---|---|
| E1 | rsm-5q1 | epic | top-level | Child prerequisites below, not epic/child cycles |
| E2 | rsm-vqr | epic | top-level | Child prerequisites below, not epic/child cycles |
| D1 | rsm-5q1.1 | task | rsm-5q1 | None |
| D2 | rsm-5q1.2 | task | rsm-5q1 | rsm-5q1.1 |
| D3 | rsm-5q1.3 | task | rsm-5q1 | rsm-5q1.1 |
| D4 | rsm-5q1.4 | task | rsm-5q1 | rsm-5q1.2, rsm-5q1.3 |
| D5 | rsm-5q1.5 | task | rsm-5q1 | rsm-5q1.4 |
| D6 | rsm-5q1.6 | task | rsm-5q1 | rsm-5q1.5 |
| T1 | rsm-vqr.1 | task | rsm-vqr | rsm-5q1.2, rsm-141.22.1 |
| T2 | rsm-vqr.2 | task | rsm-vqr | rsm-vqr.1, rsm-141.22.2 |
| T3 | rsm-vqr.3 | task | rsm-vqr | rsm-vqr.2, rsm-5q1.4, rsm-141.22.3 |
| T4 | rsm-vqr.4 | task | rsm-vqr | rsm-vqr.2, rsm-5q1.4, rsm-141.23.1 |
| T5 | rsm-vqr.5 | task | rsm-vqr | rsm-vqr.3, rsm-vqr.4, rsm-5q1.6, rsm-141.23.2 |
| A1 | rsm-eam.2 | task | rsm-eam | rsm-eam.1, rsm-vqr.5 |

All issues were filed OPEN with explicit acceptance criteria, tests, estimates, references and parent relationships.
Blocking dependencies were supplied at creation, not left as prose-only sequencing.
Nonblocking related links connect both epics to rsm-141 and connect tailoring/spec/advisor planning.
The first implementation task is `bd show rsm-5q1.1`; claim it only when implementation is authorized.

### Planning validation, 2026-11-20

- `bd lint` checked all 14 new issues: no template warnings.
- `bd dep cycles --json` returned `[]`.
- Verified each of 12 tasks has exactly its intended parent-child relationship and blocking prerequisites, including T4 -> D4 and A1 -> T5.
- Verified both epics are top-level, all 14 issues are open, all acceptance/estimate fields are populated, and every task has the required descriptive sections.
- Compared existing tailoring/spec/advisor description, acceptance, status and type before/after: unchanged; only coordination notes and new relationships were added.
- Verified all three raw Oracle files appear verbatim in the analysis appendix; local Markdown links resolve and code fences are paired.
- `git diff --check` passed for tracked changes; new document structure was checked separately.
- No Rust code, tests, CI, model evaluation or live form submission was run for this planning-only work.

Raw source SHA-256 values, for appendix audit:

```text
6e1fac242ccf6ac1338cebf9211a79f6430f419ee1c38f1a907533ded50dbaa6  cover-letter-oracle-architecture.md
d5ccff5ecd42e63a834235e02c20de3bf02afc3e986251fa8d880d09fd4b258a  cover-letter-oracle-plan.md
99ba7b7b5dcd409b3a066fe1e717bbdd1e481858bb95e0c176eebe8653245498  cover-letter-oracle-final-review.md
```

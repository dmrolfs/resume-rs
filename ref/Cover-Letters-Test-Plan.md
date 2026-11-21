<!-- ABOUTME: Maps cover-letter requirements to implementation-owned tests and release evidence. -->
<!-- ABOUTME: Defines planned quality gates; no test execution is claimed by this planning record. -->
# Cover letters: test plan

Status: planned, 2026-11-20 UTC.
Task aliases D1-D6, T1-T5 and A1 map to Beads IDs in [Implementation plan](Cover-Letters-Implementation-Plan.md).
Architecture contracts are in [Architecture](Cover-Letters-Architecture.md); raw reasoning is in [Analysis](Cover-Letters-Analysis.md).

## Strategy and placement

Test the pure domain, resolution, rendering and selection helpers in their source modules with `#[cfg(test)]`.
Black-box parser/CLI/PDF/spec contracts belong in `tests/letter_*_test.rs`.
Use claims macros and pretty_assertions consistent with existing tests.
Use synthetic identities and `.invalid` addresses in public fixtures; never commit real applicant data or job materials.
Tests are part of each implementation task, not deferred to the documentation task.
Property and mutation tests target content loss, draft leakage, substitution and limit boundaries.
Do not use invented coverage percentages or a count of tests as the acceptance criterion.

## Acceptance matrix

| Owner | Requirement | Named test intent and location |
|---|---|---|
| D1 | Field validation and explicit dates | Unit tests: empty/whitespace, 120/121 scalars, Cc/newline rejection, leap dates and invalid dates; no implicit clock default |
| D1 | Literal placeholder resolution | Unit tests: nested emphasis/link labels; repeated missing fields all reported; inserted Markdown/braces stay literal; no link creation; resolved JSON contains neither annotation nor placeholder nodes |
| D2 | Strict source shape | `tests/letter_parse_test.rs`: exact sections/order; duplicate/missing/unknown headings; optional contact; one greeting/closing; nonempty body; all unsupported blocks diagnosed |
| D2 | Original-source spans | Multibyte Unicode, CRLF/lone CR, escaped/entity text and smart punctuation preceding placeholders; exact byte ranges and UTF-8 boundaries; arbitrary input never panics |
| D2 | Reserved annotations cannot leak | `@draft`, `#tag`, `=g.v` in every position, standalone and header comments fail before ordinary comment stripping; ordinary comments remain ignored |
| D2 | Placeholder syntax and placement | Known keys, unknown/malformed braces, split formatting, code spans, raw/normalized URL destinations and header/contact positions; explicit errors instead of reliance on URL parser rejection |
| D3 | Website text is predictable | Golden exact bytes: greeting/body/closing/signature only, paragraph blank lines, no hard wrapping, one final LF, source line breaks folded, no Markdown markers |
| D3 | Link labels retain meaning | URL exact equality collapse, nonmatching scheme/text, mailto address equality, non-address mailto label preserved, explicit text-only mode |
| D3 | Counts and limits | Emoji, decomposed/composed letters, CJK, ZWJ and NBSP; byte/scalar/UTF-16/word/LF counts with independent fixture oracles; limit below/at/above count, zero limit, all units, no truncation |
| D4 | CLI stream contract | `tests/letter_cli_test.rs`: default text stdout, summary stderr, JSON/text usage error 2, unresolved/over-limit error 1 without body bytes; dry-run no body or file output |
| D4 | Text does not need a renderer | Valid text with nonexistent styles-dir, missing letter entry and unusable font path succeeds; render settings not applied, while syntactically invalid options still fail |
| D4/D5 | Explicit settings survive defaults | Explicit style/paper/page/output/limit values distinguishable from absence; no clap defaults overwrite later spec settings |
| D5 | PDF correctness | `tests/letter_pdf_test.rs`: one-page synthetic letter, every selected paragraph exactly once, frame and signature order, explicit date, contact header only in PDF |
| D5 | World and cache identity | Alternate resume/letter/resume compilation on the same Typesetter with distinct payloads and entrypoints; no wrong cached document or exposed payload path |
| D5 | Sandbox and style compatibility | Resume-only style still renders resumes, letter request diagnoses missing letter.typ, entry symlink/package/network restrictions unchanged; exported pragmatic style includes letter capability |
| D5 | Output failure ordering | Compile/page-limit/destination-lock/precommit failures preserve prior file and leave stdout empty in both mode; stdout failure after PDF commit reported with PDF remaining valid |
| D5/D6 | Determinism and metadata | Same inputs/resolved bundled fonts yield byte-identical PDF on repeated/native CI builds; compare metadata using neutral stems while changing field values; no spec/selection metadata added |
| D6 | Existing resume behavior | Existing CLI, parser, style, diagnostics, portability and determinism tests remain unchanged in meaning; native macOS/Windows release evidence recorded |
| T1 | Annotation grouping | Shared classifier diagnostics, unique document group IDs, unique variants, required default, no cross-section groups, frame alternate-only rule and ordinary comments |
| T2 | Shared selection semantics | Eligible-core preservation with draft excluded, tag/extra/core-only combinations, verify reports, unknown picks, same selected-draft fallback as upstream named tests |
| T2 | Selection before field requirements | Missing recipient in unchosen greeting is allowed; chosen variant requires it; malformed placeholder syntax in any source variant still errors; empty selected body errors |
| T2 | Selection properties | Authored subsequence, one variant per surviving group, no additions/rewrites, accurate kept/dropped counts; upstream idempotence contract adapted to distinct source/selected types |
| T3 | Flag parity | Same tags/prefer/core-only/drafts args as resume command, errors and help semantics, selected variants reported without polluting text stdout |
| T4 | Separate spec scopes | Conflicting group names in resume/letter, typo in each scope, no cross-document file load, independent ordered inheritance and precedence, profile cycles and origin reporting |
| T4 | Strict but compatible loading | Unknown keys rejected; profile letter table may hold select only; application letter requires content; valid missing other-document path ignored by opposite command; letter-specific field validation not imposed on resumes |
| T4/T5 | Defaults and paths | Spec and declaring-profile relative paths on macOS/Windows; CLI override/refusal matches actual upstream policy; no default accidentally overrides explicit spec values |
| T5 | End-to-end application | letter --spec matches equivalent flags; repeat text/PDF equality; build --spec unchanged with well-formed letter tables; letter --spec works with nonexistent resume content and vice versa |
| A1 | Advisor usefulness and privacy | Two real postings in private storage; exact rendered-text provenance, edits/timings and evidence links, local-only default, remote opt-in, invalid model output rejected, no baseline modifications |

## Mutation and property work

D1: mutate missing-field handling and literal-versus-reparsed insertion; tests must reject unsafe output.
D3: mutate limit comparison and count units; boundary fixtures must kill off-by-one and byte/scalar confusion.
T2: mutate draft filtering, core eligibility, alternate precedence and order preservation; explain equivalent or unsupported mutants in task notes.
T4: mutate selection scope lookup and merge precedence; tests must detect leakage between documents.
Run targeted `cargo mutants --file <implemented module>` or the project's `just mutants` recipe after its normal unmutated baseline passes.
Do not mistake a failed baseline for killed mutants.

## Quality gates

At implementation claim, inspect the then-current integration base and exact recipes.
At inspected main dee46ac:
- `just check`: formatting, clippy, unused dependency checks and all tests.
- `just ci`: the quality gate plus locked release build.
- `just verify`: macOS pre-push verification including docs, workflow/justfile lint and both cross-builds.

Each task records separate Rust code-review and test-review outcomes, tests run, and any unresolved finding.
D6 and T5 require native macOS and Windows CI and bundled-font determinism evidence.
Cross-compilation alone does not verify Windows runtime behavior such as locked destinations or paths.
Owner review reads a real selected letter end to end in text and PDF; no lint claims to establish prose coherence.
Two target forms validate practical counter ergonomics without changing the precise output-count contract.

## D6 release evidence protocol

The release candidate extends the existing native macOS/Windows CI matrix, not a separate workflow.
Each release binary builds the synthetic `tests/fixtures/letters/valid.md` as LF and CRLF input with identical stems and explicit fields/date.
The existing fail-closed artifact comparator checks all four letter PDFs and, separately, all four stdout text products.
Missing or empty artifacts fail; PDF comparison also forbids Calibri in bundled mode.
`tests/guide/letters.rs` executes the guide's letter commands unchanged in an isolated checkout-shaped directory and checks exact text plus PDF content.

Record candidate SHA, CI run URL, native job results and artifact hashes in `rsm-5q1.6` notes.
Local `just verify` covers the quality gate, release build, docs/workflow lint and macOS/Windows cross-builds, not Windows runtime behavior.
The owner's private real-letter text/PDF read-through is a separate required gate.
Until those findings are supplied, leave the bead open even when every automated check passes.
Keep private input, fields, paths and products out of public Git and CI artifacts.

## Planning-only validation

This session validates task types, parent relationships, blocker direction, absence of dependency cycles, document references, and raw appendix preservation.
It does not run future tests, build PDFs, exercise target websites, run model evaluation or claim release readiness.

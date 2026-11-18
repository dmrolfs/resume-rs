<!-- ABOUTME: Test strategy for the resume-rs epic (rsm-141): coverage goals, categories, per-task requirements. -->
<!-- ABOUTME: Tests use synthetic fixtures; real baselines are exercised only by ignored tests via RESUME_BASELINES. -->

# resume-rs: Test Plan

**Epic ID:** rsm-141
**Date:** 2026-11-17

---

## Test Strategy

### Coverage Goals
- **Line coverage:** at least 85% across `src/markdown`, `src/model`, `src/diagnostics`, `src/doctor`, and `src/layout.rs`, measured with `cargo tarpaulin`.
- **Typst integration:** every built-in style compiles the full fixture with zero warnings.
- **Mutation testing:** `cargo mutants` on `src/markdown/` and `src/doctor/` with no surviving mutants in convention detection or the content lints.

### Test Categories

#### A. Unit tests (about 60% of the tests)
- `model::text` and `model::period`: nutype validation (trimming, empty, length, URL scheme, year range, page limit), `PartialDate::sort_key`, and Season parsing.
- `markdown::period`: every accepted form, en dash vs. hyphen, Present/Current/Now, seasons, and rejected forms (no year, out of range, end before start).
- `markdown::blocks`: each convention in isolation, and each error and warning code with its byte range and hint.
- `markdown::inline`: text, strong, emph, link, code, nesting, and smart punctuation.
- `markdown::lint`: nested lists, chronology, bold in mid-sentence, images, contact count.
- `doctor::content`: each content lint, with a positive and a negative case.
- `doctor::fix`: edit application (sorted, non-overlapping), the diff, idempotence.
- `model`: JSON shape snapshots (insta) for every `Block`, `RoleBlock`, and `Inline` variant.
- `diagnostics`: human rendering snapshots and JSON lines.
- `layout`: report logic over hand-built placements (page limit, heading near the page bottom, sparse last page).

#### B. Integration tests (about 30%)
- `tests/parse_test.rs`: whole fixture files parsed to a model (snapshot), plus error fixtures.
- `tests/render_test.rs`:
  - the full fixture compiled in each style
  - determinism, by building twice and comparing bytes
  - text extracted from the PDF in reading order, using `lopdf`
  - page count
  - Typst error mapping, and the file access sandbox
- `tests/cli_test.rs` (with `assert_cmd`):
  - each command
  - exit codes 0, 1, and 2
  - summary output
  - `--format json`
  - `--max-pages`
  - `export-style` and `export`
  - `watch` with a timeout
- `tests/baselines_test.rs` (`#[ignore]`): both real baselines in all three styles, with zero errors.

#### C. Property tests (about 10%)
- The parser never panics on arbitrary UTF-8 Markdown (proptest).
- Every diagnostic span lies within the input's bounds and on UTF-8 character boundaries.
- `Period::text` round-trips: parsing it again yields the same `start` and `end`.
- Applying `doctor --fix` edits is idempotent: a second run finds nothing fixable.

## Per-Task Test Requirements

### rsm-141.2: Scaffold
- [ ] `cli_help_exits_zero`: verifies the binary builds and clap is wired up.

### rsm-141.14: Cross-platform
- [ ] CI: `just check` passes on macos-latest and windows-latest, on every push
- [ ] `crlf_fixture_keeps_every_byte`, `mixed_fixture_keeps_every_byte`, `lf_fixture_has_no_carriage_returns` (`tests/portability_test.rs`). Under `core.autocrlf=true` the pure-CRLF fixture alone survives without `.gitattributes`; the mixed and LF fixtures are the ones that detect conversion.
- [ ] `help_names_the_binary_resume_with_no_extension` (both OSes) and `help_ignores_an_exe_suffix_in_argv0` (unix): clap's display name must not include `.exe`
- [ ] Local: `just build-macos` produces a universal2 Mach-O (`lipo -info`, `lipo -verify_arch`, `codesign --verify`, both slices run)
- [ ] Local: `just build-windows` produces a PE32+ x86-64 `resume.exe`, and `just check-windows-imports` fails for a dynamic-std exe and passes for the real one
- [ ] `tests/pe_imports_test.rs` (unix): system DLLs accepted; `VCRUNTIME140.dll` and its variants rejected case-insensitively; mingw runtime and dynamic std rejected; CRLF tool output handled; empty import list fails closed; unknown subcommand exits 2
- [ ] CI: the Windows release exe passes `scripts/check-pe-imports.sh` (imports published as an annotation)

Cross-OS PDF byte comparison and the Windows-only behavior tests are owned by the tasks that introduce the code:

| Test | Owner |
|---|---|
| Cross-OS PDF hash job on ubuntu-latest, failing closed (rung 1: test-written PDF with embedded fonts; rung 2: bundled fonts; rung 3: `resume build --fonts bundled`) | rsm-141.15 (P3) |
| `crlf_spans_point_into_original`, LF and CRLF inputs parse to equal models | rsm-141.4 |
| `--color never` and `NO_COLOR` emit no `\x1b`; summary markers are ASCII | rsm-141.5, rsm-141.10 |
| `virtual_paths_from_windows_style_components` (all OSes; `\`-separated `PathBuf` under `cfg(windows)`) | rsm-141.6 |
| `output_locked_reports_hint` (`cfg(windows)`: open the target with `share_mode(0)`, expect `OutputLocked`) | rsm-141.6 if it writes files, else rsm-141.10 |
| Build twice with `RAYON_NUM_THREADS=1` and `=8`, same bytes; deep-nesting fixture on a spawned thread with an explicit stack size | rsm-141.6 |
| CRLF input produces the same PDF as LF input | rsm-141.10 |
| `fix_preserves_crlf_line_endings` | rsm-141.11 |
| `watch_continues_after_output_locked` (`cfg(windows)`) | rsm-141.12 |

### rsm-141.3: Model, newtypes, periods, diagnostic types
- [ ] `role_with_era_json_snapshot`: verifies the documented JSON contract.
- [ ] `newtypes_trim_and_reject_empty`: Name, Company, Title, SectionTitle, EraName.
- [ ] `link_url_rejects_unsupported_scheme`, `link_url_serializes_as_string`, `year_rejects_out_of_range`, `page_limit_rejects_zero`
- [ ] `partial_date_sort_key_orders_season_and_month`, `season_parses_fall_and_autumn`
- [ ] `period_end_present_serializes_tagged`: verifies `{"kind":"present"}`.
- [ ] `paper_as_ref_str_matches_typst_names`: `us-letter` and `a4`.
- [ ] `diagnostic_code_ids_are_kebab_case_and_unique` (strum `EnumIter`)
- [ ] `schema_version_is_emitted`

### rsm-141.4: Markdown parser
Unit tests sit beside the code in `src/markdown/{tree,blocks,role,inline,period,lint,mod}.rs`; whole-document tests are in `tests/parse_test.rs` with fixtures `tests/fixtures/{full,errors-*,warnings-*}.md`.
- [x] One test per convention: `parses_name_and_contact`, `parses_three_part_role_line`, `parses_two_part_role_line`, `first_paragraph_after_role_is_blurb`, `bold_only_paragraph_with_dates_is_era`, `bullet_label_is_extracted_without_colon`, `page_break_marker_becomes_block`, `comments_are_dropped`
- [x] One test per Format code, asserting the byte range, a hint, and the Edit when fixable (for example `role_with_wrong_separator_suggests_middle_dot`, `hyphen_range_warns_with_edit`, `current_end_warns_with_edit`, `empty_company_maps_nutype_error_to_located_diagnostic`), and `every_format_code_is_demonstrated_by_a_fixture`
- [x] Role-line safety: `hyphenated_company_is_not_rewritten`, `ambiguous_hyphen_separators_have_no_edit`, `role_separator_and_period_hyphen_edits_do_not_overlap`, `a_period_with_a_bad_start_is_not_mistaken_for_a_role_separator`
- [x] Links: `malformed_link_is_invalid_link_with_parser_hint`, `relative_link_is_invalid_link`, `ftp_link_is_unsupported_link`, `email_autolink_gets_mailto`
- [x] Nothing silently lost: `unsupported_blocks_are_errors`, `unterminated_comment_is_error`, `page_break_without_blank_line_after_is_error`, `nested_items_are_flattened_in_order`
- [x] Fixtures: `full_fixture_has_no_diagnostics`, `full_fixture_snapshot`, `finding_fixtures_snapshots` (diagnostics with the quoted source), `warnings_fixable_edits_apply_cleanly`
- [x] Windows portability (owned by this task): `crlf_spans_point_into_original`, `lf_and_crlf_inputs_parse_to_equal_models`, `mixed_line_endings_parse_like_lf`, `lone_cr_line_endings_parse_like_lf`, `final_line_without_newline_parses`, `bom_is_ignored_for_the_model_and_spans_shift`
- [x] Property tests (proptest): `parse_never_panics_on_arbitrary_text`, `parse_never_panics_on_structured_markdown`, `spans_are_in_bounds` (bounds, char boundaries, hints, Edits non-overlapping), `resume_is_present_unless_errors`, `applying_edits_clears_fixable_findings`, `period_text_reparses_identically`
- [x] Review fixes (each with a regression test): `many_html_blocks_parse_in_linear_time` (a run of comment blocks was quadratic), `colon_edit_needs_whitespace_after_the_colon`, `colon_edit_is_only_offered_at_label_position`, `second_dash_is_reported_at_the_real_character`, `snap_moves_a_range_onto_character_boundaries`, `era_with_markup_is_error`, `empty_and_dash_comments_are_complete_comments`, `unclosed_inline_comment_is_unsupported_html`, `heading_marker_without_a_space_warns`, `a_period_cannot_start_with_a_month_word_or_present`, `format_code_severities_are_exactly_the_documented_sets`, `every_length_limit_is_exact`
- [x] Property tests that check meaning, in `tests/parse_property_test.rs`: `applying_edits_preserves_words_and_emphasis` (an Edit never changes a word, loses a bold span, or turns markup into literal text), `generated_documents_parse_to_the_model_their_generator_implies` (a model-based oracle over roles, blurbs, eras, labels, and lists in any combination), and `no_word_is_lost_without_a_finding` (every word is in the model or inside a finding's span). Each was checked for sensitivity by re-introducing the bug it guards against and confirming it fails. Shared helpers live in `tests/common/mod.rs` (`subject_matches` checks that a finding's span quotes what its code names) and `src/markdown/testutil.rs`.
- [x] Mutation testing: `cargo mutants --file 'src/markdown/*.rs' -j 4 -- --lib` (unit tests only; integration tests kill more). Surviving mutants that were real gaps got tests; redundant code was deleted; the rest are equivalent mutants (for example `|` to `^` on disjoint flag bits).
- [x] Pathological input: `pathological_inputs_do_not_overflow_a_small_stack` (256 KiB stack), `nesting_beyond_the_cap_is_reported_not_crashed_on`, `oversized_input_is_one_error_and_no_model`
- [x] `baselines_parse_without_errors` (ignored; set `RESUME_BASELINES`, it fails loudly when unset). Each baseline emits exactly one `too-many-contact-items` warning, because each has five contact items and the book says at most four.

### rsm-141.5: Diagnostic rendering
Unit tests sit beside the code in `src/diagnostics/lines.rs` and `src/diagnostics/render/`; integration tests are in `tests/diagnostics_test.rs` and `tests/diagnostics_property_test.rs` (`tests/render_test.rs` is reserved for Typst).
- [x] `human_error_shows_excerpt_and_hint` (snapshot)
- [x] `json_lines_one_object_per_diagnostic`: every line deserializes into a `deny_unknown_fields` record, every key is present, LF only
- [x] `summary_counts_errors_warnings_and_fixable`
- [x] `human@<fixture>` snapshots: every fixture renders in both formats, with its summary line
- [x] `human_header_and_json_agree_on_line_and_column` (every finding in every fixture), and an independent count of line and column
- [x] `lf_crlf_and_cr_render_identical_excerpts`, `bom_does_not_shift_columns`, and a span inside the BOM
- [x] `never_and_no_color_emit_no_escape_bytes` (the source itself contains `\x1b[31m`), `always_emits_ansi_but_still_neutralizes_the_source`, `color_never_and_no_color_emit_no_ansi_end_to_end`, `the_summary_is_ascii`
- [x] `tall_and_long_spans_output_is_bounded`: 500 findings over a 400 KiB line and over a 1 MiB block of one-byte lines
- [x] Property tests: `render_never_panics_on_arbitrary_text_and_ranges` (hostile text; reversed, past-the-end, and mid-character ranges), header and JSON agree, located exactly when the span starts inside the text, output per finding bounded
- [x] `line_index_matches_the_naive_reference` (proptest) and `line_of_counts_a_crlf_as_one_ending_wherever_the_offset_falls` (the Markdown `line_of` bug found by the design review)

### rsm-141.6: Typst World and PDF
- [ ] `compiles_minimal_style_to_pdf`: the output starts with `%PDF` and has 1 page.
- [ ] `same_input_twice_is_byte_identical`
- [ ] `style_syntax_error_maps_to_style_file_line`
- [ ] `file_outside_root_is_denied`
- [ ] `resume_json_is_readable_from_style`
- [ ] `today_is_unavailable`: a style calling `datetime.today()` produces a diagnostic, not an output that depends on the date.

### rsm-141.7: Components and pragmatic
- [ ] `full_fixture_renders_in_pragmatic_without_warnings`
- [ ] `pragmatic_text_order_title_company_dates`: from `lopdf` text extraction.
- [ ] `role_heading_is_kept_with_first_bullet`: a fixture with spacing chosen to force a page break at a role.
- [ ] `bullet_does_not_split_across_pages`
- [ ] `footer_shows_name_and_page_of_total`
- [ ] `pdf_is_tagged`: checks for the `StructTreeRoot` entry.

### rsm-141.8: mono
- [ ] `full_fixture_renders_in_mono_without_warnings`
- [ ] `mono_text_order_title_then_company`

### rsm-141.9: experienced
- [ ] `full_fixture_renders_in_experienced_without_warnings`
- [ ] `experienced_section_labels_precede_content`
- [ ] `experienced_skills_render_label_value_rows`

### rsm-141.10: CLI, --dry-run, and layout report
- [ ] `build_writes_pdf_and_prints_summary`: includes the page count and build time.
- [ ] `build_dry_run_writes_nothing_and_reports_target`
- [ ] `max_pages_exceeded_exits_1`
- [ ] `malformed_role_exits_1_with_located_error`
- [ ] `unknown_style_exits_1_listing_styles`
- [ ] `bad_flag_exits_2`
- [ ] `styles_lists_three_builtins`
- [ ] `export_style_copies_style_and_lib`, `export_style_dry_run_lists_files`, `styles_dir_overrides_builtin`
- [ ] `sparse_last_page_warns`, `font_fallback_warns`

### rsm-141.11: doctor
- [ ] `every_diagnostic_code_has_advice` (strum `EnumIter`)
- [ ] For each content lint, a positive and a negative test: `long_bullet`, `unquantified_bullet`, `weak_opener`, `first_person`, `sloppy_phrase`, `long_summary`, `education_not_last`, `old_role_detail`
- [ ] `setup_reports_unknown_style`, `setup_reports_unwritable_output`
- [ ] `doctor_groups_findings_by_category` (snapshot), including the role-to-page map
- [ ] `explain_prints_advice_for_code`, `explain_unknown_code_exits_2_with_suggestions`
- [ ] `fix_applies_edits_and_reports_diff`, `fix_dry_run_leaves_file_unchanged`, `fix_is_idempotent`
- [ ] `doctor_exit_1_only_on_errors`
- [ ] `check_matches_doctor_report`, `check_rejects_fix_and_explain` (exit 2), `check_never_writes`

### rsm-141.12: watch and export
- [ ] `export_writes_resume_json_matching_model`, `export_dry_run_lists_files`
- [ ] `watch_rejects_dry_run`, `debounce_parses_humantime_values`
- [ ] `exported_dir_compiles_with_world`
- [ ] `watch_rebuilds_after_file_change` (bounded to 10 s)
- [ ] `watch_replaces_pdf_atomically`: a reader never sees a partially written file.

### rsm-141.13: README and migration
- [ ] `baselines_build_in_all_styles` (ignored, needs `RESUME_BASELINES`)
- [ ] Manual: review the README commands by running each one exactly as documented.

## Test File Organization

```
src/markdown/*.rs        # inline unit tests (#[cfg(test)] mod tests)
src/model/*.rs           # inline unit tests (nutype validation, periods)
src/diagnostics/*.rs     # inline unit tests + insta snapshots
src/doctor/*.rs          # inline unit tests (content lints, fixes)
src/layout.rs            # inline unit tests
tests/parse_test.rs      # fixture parsing, error fixtures, proptests
tests/render_test.rs     # Typst compile per style, determinism, PDF text/tagging
tests/doctor_test.rs     # doctor report, --explain, --fix end-to-end
tests/cli_test.rs        # assert_cmd end-to-end
tests/baselines_test.rs  # #[ignore], real baselines via RESUME_BASELINES
tests/fixtures/          # synthetic resumes and minimal test styles
```

## Verification Commands

```bash
# Per task
cargo test --test parse_test
cargo test --test render_test
cargo test --test doctor_test
cargo test --test cli_test

# Full epic
just check            # fmt --check, clippy --all-targets -D warnings, test
RESUME_BASELINES=~/Documents/vrm/my/job/drafts cargo test -- --ignored
cargo tarpaulin --out Html
cargo mutants -f src/markdown
```

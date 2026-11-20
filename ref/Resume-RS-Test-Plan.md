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
| Cross-OS PDF hash job on ubuntu-latest, failing closed (rung 1: test-written PDF with the typst-kit fonts, done and proven; rung 2: the full fixture in the pragmatic style with the bundled fonts, refusing any other embedded font, done; rung 3: the release binary's `resume build --fonts bundled` on LF and CRLF copies of the fixture, four PDFs byte-identical across both systems with no Calibri embedded, done; the comparison is `scripts/compare-pdfs.sh`, tested by `tests/compare_pdfs_test.rs`). Proven red on scratch branches: a missing macOS artifact fails the download step (run 44426707817), and a Windows PDF rendered under another ident fails the comparison at byte 49096 with both hashes (run 44427040025); a passing run prints both hashes (run 44423516807). | rsm-141.15 (P3) |
| `crlf_spans_point_into_original`, LF and CRLF inputs parse to equal models | rsm-141.4 |
| `--color never` and `NO_COLOR` emit no `\x1b`; summary markers are ASCII | rsm-141.5, rsm-141.10 |
| `virtualize_builds_from_path_components_not_strings` (all OSes) and `virtualize_and_realize_handle_drive_letter_roots` (`cfg(windows)`) | rsm-141.6 |
| `output_locked_reports_hint` (`cfg(windows)`: open the target with `share_mode(0)`, expect `OutputLocked`; map both `ERROR_ACCESS_DENIED` and `ERROR_SHARING_VIOLATION`) | rsm-141.10 (owns the atomic write; rsm-141.6 has no write path) |
| Build with `RAYON_NUM_THREADS` 1, 2, and 8, same bytes; deep-nesting fixtures in child processes | rsm-141.6 (done: `determinism_test.rs`, `nesting_test.rs`) |
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
- [x] Hard-break Edit safety (rsm-141.19, regression for a bug the property test found, then hardened after code and adversarial review): document-level tests in `tests/parse_test.rs` (`hard_break_edit_is_withheld_when_removing_the_marker_would_change_the_structure`, `hard_break_edit_is_withheld_for_the_inputs_the_property_found`, `hard_break_edit_is_kept_and_changes_nothing_when_text_precedes_the_marker`, `hard_break_edits_never_change_the_document_in_the_review_cases` (eleven inputs the review found: an emphasis opener or tag ending beyond the local window, a reference definition elsewhere, a setext line before it, a BOM, adjacent Edits that form a table), `hard_break_edits_are_offered_identically_for_lf_crlf_and_lone_cr`); unit tests in `src/markdown/edit_safety.rs` for the local proof (CRLF, lone CR, BOM, bad ranges), the whole-document proof (a stubbed re-parse pins each comparison: findings, messages, order; one unsafe break withdraws every Edit), and `lines_around`. Properties in `tests/parse_property_test.rs`: `hard_break_edits_never_change_the_document` (apply only the hard-break Edits: the model and every other finding are unchanged; a purpose-built line generator with BOM and CR variants, because token soup rarely assembles the shapes) and `applying_edits_preserves_words_and_emphasis` (a warning's Edit never adds an error; lone-marker lines in its generator). `PROPTEST_CASES=300000 cargo test --test parse_property_test` runs them deep: all four passed (208 s); with the whole-document proof disabled the new property fails in 4 of 5 default 512-case runs, and with the original local-only proof the old property needed luck.
- [x] Unused-dependency gate (rsm-141.17, hardened after review): `tests/justfile_test.rs` pins the `check` chain and that `unused-deps` is platform-less and runs `cargo check --lib --bins` then `cargo machete --with-metadata`; `tests/manifest_test.rs` fails when nothing in `src/` uses `validate(regex` any more (the one dependency both checks are blind to). Through the real gate: an unused dependency fails `just check`; one used only by `#[cfg(test)]` code passed clippy but now fails `just unused-deps`.
- [x] List spans (rsm-141.20, rsm-141.21; regressions for two inputs the `spans_are_in_bounds` property found, then hardened after an independent review that found the property blind to most of this class): in `src/markdown/tree.rs`, `a_nested_list_starts_at_its_first_marker_whatever_precedes_it` (thirteen shapes; two of them, an aligned bullet and an aligned numbered item, are controls that already held), `a_top_level_list_starts_at_its_first_marker`, `a_list_starts_at_its_first_marker_when_the_text_is_shifted_by_a_base` (a BOM), and `the_depth_cap_finding_points_at_the_list_marker` (LF, CRLF, lone CR); in `tests/parse_test.rs`, the two bead inputs with their exact spans, `every_finding_about_a_list_points_at_its_marker` (nested, numbered, before the first section, before the name heading, after a BOM), and the zero-width `missing-name` position; in `tests/parse_property_test.rs`, `list_findings_point_at_a_marker` over list-only documents (any indent, tabs, blank lines, LF, CRLF, lone CR, BOM) checks that every finding about a list points at a marker, and `subject_matches` now rejects any span that starts with a line ending, for every code. Sensitivity: the property fails for each of five wrong fixes (none, nested lists only, spaces only, line endings only, no line endings); before it was added, a fix that skipped top-level lists passed every property. Both seeds are in `tests/parse_test.proptest-regressions`; each fails alone with the fix disabled and passes with it. Not covered by design: an indented code block's span starts after its indent, which is code content.

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
- [x] Review follow-ups (code-review-rust, then test-review-rust): a write failure keeps its `ErrorKind` in both formats (`RenderError::Write`); a span of only zero-width characters gets a caret and a note (`every_located_finding_draws_a_caret` property); a JSON record holds no Unicode line separator (property over `\u{2028}`, `\u{2029}`, `\u{85}`); the file name is exact in JSON and escaped and capped in human output; whitespace-only hints are skipped

### rsm-141.6: Typst World and PDF
Style, world, and PDF settings (unit, in `src`):
- [x] `style.rs`: `directory_style_requires_style_typ`, `unknown_style_reports_searched_directory`, `missing_styles_root_is_a_style_root_error`, `main_file_is_style_typ_under_the_style_name`, `directory_style_reads_shared_files_through_the_root`, `embedded_style_reads_by_relative_path`, `embedded_style_reports_unknown_and_missing_main`, `missing_file_is_not_found_in_both_sources`, `reading_a_directory_is_is_directory_in_both_sources`, `symlink_escape_is_access_denied` (unix), `display_paths_hide_the_canonical_root_and_mark_built_ins`
- [x] `world.rs`: `style_files_resolve_through_virtual_path_components`, `today_is_none`, `resume_json_is_served_from_memory_and_shadows_disk`, `package_roots_are_refused_with_a_clear_message`, `served_sources_record_only_style_files_in_display_order`, `backslash_style_path_is_rejected_by_virtual_path`, `virtualize_builds_from_path_components_not_strings`, `virtualize_and_realize_handle_drive_letter_roots` (`cfg(windows)`)
- [x] `pdf.rs`: `pdf_options_are_deterministic`. `fonts.rs`: the bundled set loads, is sorted and unique. `diag.rs`: error, warning, detached span, in-memory resume span, hints and trace, order. `typeset/mod.rs`: a panic becomes `TypesetPanicked`, the work runs on the named thread.

`tests/render_test.rs` (fixtures in `tests/fixtures/styles/`). Added after review: BOM and CRLF render the same bytes, a non-UTF-8 style names its file, an error in a shared file is located there, errors precede warnings in a failed compile, the requested font is the embedded one, the PDF is tagged with the resume-rs creator and no dates, an embedded style renders the same bytes as the same files on disk and reports `<built-in>` paths, and export works after the cache is evicted.

- [x] `compiles_minimal_style_to_pdf`: `%PDF-` ... `%%EOF`, 1 page, an embedded subset font, and the title equals the resume name (proves `/resume.json` was read).
- [x] `paper_input_selects_page_size`: MediaBox 612x792 and about 595x842.
- [x] `style_syntax_error_maps_to_style_file_line`, `compile_errors_carry_the_style_text_for_excerpts`
- [x] `today_is_unavailable`, `relative_path_escaping_the_root_is_denied`, `absolute_path_is_root_relative_and_never_reads_the_host`, `package_import_is_refused_without_network`, `symlink_pointing_out_of_the_styles_directory_is_denied_end_to_end` (unix)
- [x] `root_relative_import_through_the_style_parent`, `content_style_renders_every_node_kind_without_warnings`
- [x] `unknown_font_family_in_a_stack_warns`: pins the Typst warning that rsm-141.7 resolves.

`tests/determinism_test.rs` (cross-process cases re-run the test binary through an `#[ignore]` child entry point):
- [x] `repeated_in_process_renders_are_identical` (20; the content fixture has at least five pages, and the child proves `RAYON_NUM_THREADS` reached rayon's pool), `cold_and_warm_comemo_cache_agree`, `different_content_gives_different_bytes` (negative control), `a_different_ident_changes_the_bytes_and_the_same_ident_does_not`, `pdf_has_no_dates`
- [x] `rayon_thread_count_does_not_change_bytes` (1, 2, 8), `file_id_numbering_does_not_change_bytes` (500 ids interned first), `a_fresh_process_matches_this_warm_one`

`tests/nesting_test.rs` (child processes, because a stack overflow aborts the harness):
- [x] `nesting_just_under_the_layout_cap_compiles`, `nesting_just_under_the_cap_survives_quarter_size_worker_stacks`, `nesting_beyond_the_layout_cap_is_a_diagnostic_not_a_crash`, `nesting_beyond_the_parse_cap_is_a_diagnostic_not_a_crash`

`tests/dependencies_test.rs`: `typst_crates_are_pinned_exactly`, `no_network_stack_in_dependency_tree`.

Not covered here: cross-architecture byte equality (rsm-141.15), the text of the PDF in reading order (rsm-141.7; lopdf text extraction on Typst's CID fonts is unverified), font layering and role resolution (rsm-141.18).

### rsm-141.18: Layered font set and role resolution
Pure and injected-source tests in `src/typeset/fonts.rs` (synthetic `FontInfo`; no test reads this machine's fonts), real-file tests in `src/typeset/fonts_disk_tests.rs` (faces copied from the typst-kit embedded fonts into temp directories, the system and Office scans switched off), and the public API in `tests/fonts_test.rs`:
- [x] Resolution: `role_resolution_*` (case-insensitive, the book's spelling, metric note vs layout warning vs missing), `a_family_present_only_in_another_variant_...`, `a_face_that_cannot_be_read_does_not_count`, `a_font_file_removed_after_the_build_no_longer_loads_and_the_role_falls_back`, exact messages and hints in Auto and Bundled mode, `font_roles_are_built_with_new`, `a_role_resolves_through_the_public_api`
- [x] Order: layer order, `--font-path` and `RESUME_FONT_PATH` command-line order, files sorted by path on a real disk, sort by canonical not spelled path (symlinked subdirectory), `ordering_within_a_group_is_independent_of_enumeration_order`, duplicate path and index, symlink and case-spelling dedup, a relative path stored as the canonical one
- [x] Typst's tie-break traps rsm-141.7 must design around: `a_later_variable_face_beats_an_earlier_static_one`, `a_later_closer_variant_beats_an_earlier_one`
- [x] Builder: bundled mode never scans or validates (unit and public API), auto asks the system source, bad or file `--font-path`, bad env entry, env splitting, a scan that panics once is retried and twice is an error naming layer and directory, the four Office directories, subset skip and never skipping a directory with non-files
- [x] Real files: `font_path_adds_fonts`, `ttc_collections_load_with_one_face_per_index` (a two-face collection built in the test; indexes 0 and 1), `a_directory_with_no_fonts_adds_nothing`, `bundled_mode_ignores_font_paths_environment_and_bad_directories`, `resolved_font_appears_in_the_pdf`
- [x] `fonts_mode_names_match_clap_and_strum`, `font_origin_names_are_kebab_case_and_round_trip`, `the_font_codes_have_advice_that_says_how_to_get_the_font`
- Not covered: Windows `canonicalize` beyond the case-spelling test, Windows Office directories, Linux fontconfig, a font file vanishing during a real scan (the guard is tested with injected panics).

### rsm-141.7.1: Bundled fallback fonts and the installed-families input
Unit tests in `src/typeset/fonts.rs`, `fonts_disk_tests.rs`, and `world.rs`; integration in `tests/render_test.rs` and `tests/fonts_license_test.rs`:
- [x] `bundled_set_lists_typst_kit_fonts_plus_the_fallback_families` (7 families, 29 faces), `typst_kit_faces_come_first_in_their_own_order_so_their_indexes_never_move`, `every_fallback_family_has_all_four_variants_and_is_static` (no VARIABLE flag; protects the tie-break design)
- [x] `installed_families_are_the_lowercased_families_of_the_font_set` (`sys.inputs.fonts`)
- [x] `font_stack_keeps_only_installed_families_and_renders_without_warnings`, `font_stack_uses_the_preferred_family_when_it_is_installed`, `font_stack_with_neither_family_installed_still_warns_so_the_problem_is_visible`
- [x] `bundled_mode_embeds_only_bundled_families_even_on_a_machine_that_has_calibri`, `bundled_and_hermetic_auto_pdfs_are_byte_identical`
- [x] `every_embedded_font_file_is_listed_in_licenses_md`, `every_font_family_directory_keeps_its_license_text_beside_the_fonts`
- [x] Ignored unless `RESUME_ORIGINAL_FONTS_DIR` is set (Calibri, Georgia, Arial in four variants, named `<Family>-<Variant>.ttf`): `fallback_widths_match_the_originals`, which fails above 0.2% per glyph advance and was checked to fail for a wrong pairing

### rsm-141.7.2: Components, probes, and the pragmatic style
In-crate (`src/typeset/pragmatic_tests.rs`, reads frames) and through the public API (`tests/pragmatic_test.rs`):
- [x] `full_fixture_renders_in_pragmatic_without_warnings` (Letter and A4, 2 pages), `pragmatic_text_order_name_then_title_company_period` (lopdf `extract_text`, whitespace-normalized), `footer_shows_name_and_page_of_total`, `pdf_is_tagged`
- [x] `role_heading_is_kept_with_first_bullet`, `bullet_does_not_split_across_pages`: a sweep of spacer lines puts the page break at every offset of 7 generated roles; `the_check_fails_for_a_style_without_sticky_headings` and `..._with_breakable_bullets` prove the check can fail; `consecutive_and_trailing_page_breaks_never_leave_a_blank_page`
- [x] `theme_fonts_metadata_is_queryable`, `every_role_and_era_leaves_a_probe_in_document_order`, `malformed_font_metadata_in_a_user_style_is_a_typed_error_not_a_panic`, `a_style_without_probes_has_no_font_roles_and_no_probes`
- [x] `the_built_in_pragmatic_style_and_the_same_files_on_disk_render_the_same_bytes`, `an_edited_copy_of_the_theme_renders_differently`, `every_file_under_styles_is_embedded_in_the_binary`, `styles_that_are_named_but_not_written_yet_are_unknown_not_a_panic`, `the_font_stack_helper_in_the_fixtures_is_the_one_components_ships`
- [x] Ignored unless `RESUME_ORIGINAL_FONTS_DIR` holds Calibri and Georgia in four variants: `bundled_and_original_body_fonts_break_lines_identically` (text, x, and baseline y of every line, full fixture and generated resumes) and its negative control `the_identity_check_catches_line_pitch_taken_from_font_metrics`
- Not covered: Windows rendering of the embedded `./theme.typ` import (CI proves it), tag-tree order versus content-stream order, a section heading followed directly by a page break.

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
- [ ] `section_headings_are_set_in_capitals`, `the_contact_details_follow_the_books_order_under_the_name`, `footer_shows_name_and_page_of_total`, `roles_and_eras_leave_probes_in_document_order`, `font_roles_prefer_calibri_and_fall_back_to_carlito`, `bundled_mode_embeds_only_bundled_families`, and `the_built_in_mono_style_and_the_same_files_on_disk_render_the_same_bytes` (`tests/mono_test.rs`)
- [ ] `mono_keeps_the_same_pagination_guarantees` (sweep, `src/typeset/pragmatic_tests.rs`) and the ignored `bundled_and_original_body_fonts_break_lines_identically_in_mono` (needs `RESUME_ORIGINAL_FONTS_DIR`)
- [ ] In `src/typeset/pragmatic_tests.rs`: `the_mono_role_line_is_bold_throughout_like_the_template` (also a role without a title), `the_mono_contact_items_are_one_centered_line_under_the_centered_name_in_the_books_order`, `a_mono_contact_line_that_is_too_wide_wraps_between_items_never_inside_one`, `a_mono_resume_without_a_contact_line_renders_the_centered_name_then_the_first_section`, `mono_capitalizes_whatever_the_section_is_called`, `mono_has_no_color_but_its_links`, `mono_underlines_its_links_in_the_link_color`, `mono_sets_each_kind_of_text_in_the_templates_size_weight_and_face`, `each_mono_section_title_sits_over_one_black_one_point_rule_the_width_of_the_text`, `a_mono_name_that_wraps_keeps_the_contact_line_17pt_under_its_last_line`, `mono_is_centered_on_a4_too`, `mono_keeps_the_values_approved_for_pragmatic_in_step_with_pragmatic`, `the_mono_heading_checks_fail_once_their_stickiness_is_removed`; and `an_edited_copy_of_the_mono_theme_renders_differently` (`tests/mono_test.rs`)
- Not covered: the mono ignored identity test does not compare x positions or a wrapped contact line

### rsm-141.9: experienced
- [ ] `full_fixture_renders_in_experienced_without_warnings`
- [ ] `experienced_section_labels_precede_content`
- [ ] `experienced_skills_render_label_value_rows`

### rsm-141.10: CLI, --dry-run, and layout report
- [x] Output writer (slice 2; `src/output.rs`, `src/error.rs`): a new file, replacing a file, creating the output directory, no leftover temp file, the permissions a plain write would give (unix), a directory target and a read-only target refused on every OS, `Refuse` never overwrites but writes a new file, a dry run touches nothing and says whether it replaces and which directory it would create, a dry run reports the refusals a write would hit, a failed persist leaves no temp file, `classify` maps an existing target to `OutputExists` and anything else to `WriteOutput` with its cause, and the hints of the output errors. `cfg(windows)`: `output_locked_reports_hint` (a handle with `share_mode(0)`) and `output_locked_by_a_reader_that_shares_reads` (`FILE_SHARE_READ`) assert `OutputLocked`, the hint, the old bytes intact, and no temp file, and print the raw OS error so CI records which code occurs. Compile-checked locally against `x86_64-pc-windows-gnu`; run on `windows-latest`.
- [x] CLI and build (slice 3): `src/cli.rs` (12 unit tests: defaults, the `--paper` default trap, global `--dry-run`/`--format`/`--color`, repeatable `--font-path`, `--max-pages` 1 to 20, a bad style name, a missing file, `--fonts bundled` with `--font-path` is a usage error while explicit `--fonts auto` is not, duplicate output names ignoring case), `src/commands/report.rs` (10 unit tests of the summary and fonts lines), `src/style.rs` (`resolve`: directory, fallback to built-in, unknown, unopenable directory), and `tests/build_test.rs` (17 in-process tests over real documents, below), and `tests/cli_test.rs` (the real binary: exit 0, 1, and 2, the dry run, JSON on stdout, color).
- [x] `build_writes_pdf_and_prints_summary`: includes the page count and build time.
- [x] `build_dry_run_writes_nothing_and_reports_target` (also `a_dry_run_says_when_it_would_overwrite` and `a_dry_run_ends_the_way_a_real_build_would`)
- [x] `max_pages_exceeded_exits_1_and_still_writes_the_pdf` (the owner's decision: the PDF is written so it can be looked at)
- [x] `malformed_role_exits_1_with_located_error` (also `an_error_in_one_file_keeps_the_output_of_the_others`, `an_input_that_cannot_be_read_exits_1_and_says_why`)
- [x] `unknown_style_exits_1_listing_styles` (in `tests/styles_test.rs`; the error lists the available styles, and `tests/cli_test.rs` checks it through the binary)
- [x] `bad_flag_exits_2` is `cli_unknown_flag_exits_two` in `tests/cli_test.rs`, with `no_command_exits_2`, `build_without_a_file_exits_2`, `fonts_bundled_with_font_path_exits_2`, and the duplicate-output check
- [x] `styles_lists_every_embedded_builtin` (renamed from `styles_lists_three_builtins`, owner decision: only `pragmatic` is embedded while `mono` and `experienced` are open; rsm-141.9 tightens it to three), `styles_marks_invalid_directory_styles`, `a_styles_dir_that_cannot_be_read_exits_1`, and the closed or failing stdout
- [x] `export_style_copies_style_and_lib` (exactly the style and the lib, byte for byte), `export_style_dry_run_lists_files`, `styles_dir_overrides_builtin` (the listing says so, and the directory's `pragmatic` is the one built with), `export_style_refuses_to_overwrite_without_force` (one existing file stops every write and no edit is lost), `export_style_force_replaces_existing_files`, `a_dry_run_with_force_says_it_would_replace`, `export_style_of_an_unknown_style_exits_1_listing_styles` (including `lib`, which is not a style), and `exported_style_builds_the_same_pdf_as_the_builtin`
- [x] Layout report (slice 1; `src/layout.rs`, `src/typeset/geometry.rs`, `src/typeset/geometry_tests.rs`): thirteen pure unit tests over synthetic page geometry (page limit names the count and the limit, no finding at the limit, sparse last page below 15% of the body, never on a one-page resume, an empty last page, a heading in the bottom 8% of the body and not above it, an era by its name, one overflow error per page naming the role with a half-point tolerance, role placement when the next role opens a page and when it shares one, the last role on the last page, eras are not placements, font findings joined with their severities, a fixed order); and real-document tests on the built-in style: the footer is found as an artifact and is not body content, a bullet taller than a page is one error naming the role while a long bullet that fits is not, and `tests/fixtures/full.md` has a sparse last page and every role placed. Four tiny fixture styles under `tests/fixtures/styles/geometry-*` draw one kind of thing each, so the measuring is tested on shapes and images (a 900 pt rectangle and image are measured to 928 pt and reported as an overflow), on a header and footer of plain text (an inlined artifact with no group around it, which the first version of the classifier took for body content), and on content moved below the page by a transform. Mutation testing of `layout.rs` and `geometry.rs` found the page index, the plural in a hint, and three thresholds untested; each now has a test.
- [x] Output delivery regressions (`tests/output_delivery_test.rs`, `tests/build_test.rs`, `tests/cli_test.rs`): immediate and buffered stdout failures return failure; a failed write remains a failure after successful writes and flush; zero-byte writes fail; interrupted writes retry; closed pipes stay quiet; duplicate flush errors are suppressed.
  A real Unix binary test uses an unconnected datagram socket, and a Linux test uses `/dev/full`.
  Refused directory and read-only PDF targets preserve findings and bytes in real and dry-run modes; JSON keeps its findings, later inputs still build, and summaries count operational and compiler errors.
- [x] Closing pass (rsm-141.10): `src/output.rs` dry run and write refuse the same parents (a file or a dangling link in the chain; the topmost missing directory is named); symlink targets are replaced and never written through; a dry run refuses a read-only target. `src/layout.rs` and `src/typeset/geometry_tests.rs`: exact boundaries (heading line, 15% sparse line), zero-height body, font-mode forwarding, two overflowing pages with an era between, the no-role hint, exact hint texts, and fixtures for a document-wide body top, no footer, a background, a footer holding a nested artifact, and clipped content (the last two pin documented limits). Advice is checked against the parser's page-break marker and the sparse threshold. `build`: a failing input before a good one, `--format json` operational errors (stdout empty), exact `creates out` text; `styles` lists exactly the embedded styles; a dry-run `export-style` refuses like a real one.
- [x] Licenses (slice 5; `tests/fonts_license_test.rs`, `tests/licenses_test.rs`, `tests/cli_test.rs`): the notices carry the vendored typst-assets `NOTICE` and every bundled OFL text in full, every bundled font family has its license text in the output (adding a font fails until its license is listed), the vendored `NOTICE` equals the one in the locked crate (`cargo metadata --locked --offline`), `.gitattributes` stops line-ending conversion of it, and `resume licenses` prints all of it on stdout, exits 0, is unaffected by `--dry-run`, and stops quietly on a closed stdout, also through the real binary.
- [x] `fallback_font_reports_note_or_warning_by_metric_compatibility` (the bead's name; the test plan said `font_fallback_warns`)

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

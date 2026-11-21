<!-- ABOUTME: The fixtures for the position spec and profile loader (rsm-141.23.1): one directory per case, with the outcome each must give. -->
<!-- ABOUTME: Data only and synthetic (Ada Lovelace, example.com); tests/spec_test.rs reads every case. -->

# Spec and profile fixtures

Each directory is one case, laid out as a user lays out specs: `positions/` holds the spec, `profiles/` holds the profiles it extends, and a spec names its content file relative to itself.
`baseline.md` is one small annotated resume (groups `gateway` and `summary`, tags `ai`, `llm`, `platform`, `iceberg`, `leadership`).
It builds with no findings (`resume check tests/fixtures/specs/baseline.md`), so a later task can build every valid case with `--spec`.
`path-relative/baselines/baseline.md` is a copy, because that case exists to exercise `../baselines/baseline.md`.

| Case | Spec file | Expected outcome |
|---|---|---|
| `two-profiles` | `positions/acme.toml` | Loads. Chain `principal`, `ai-platform`, then the spec. Tags `{ai, iceberg, leadership, platform}`. Prefer `[short, business, technical]`. Pick `gateway = short` (from `ai-platform`, which is listed after `principal`), `summary = acme` (the spec). Style `mono` (the spec beats `principal`'s `pragmatic`). Paper `a4` (`ai-platform`). |
| `diamond` | `positions/acme.toml` | Loads, no error. `left` and `right` both extend `base`: the order is `base`, `left`, `right`, spec, and `base` appears once. Prefer `[business, short, technical]`. Pick `gateway = long` (`right`, listed last). |
| `path-relative` | `positions/2026-11-acme.toml` | Loads from any working directory. The content path is the spec's directory, then `..`, `baselines`, `baseline.md`. |
| `unknown-key` | `positions/acme.toml` | Error `UnknownKey`: `tag` in `select`, naming the file and line, listing the allowed keys, suggesting `tags`. |
| `position-in-profile` | `positions/acme.toml` | Error `ForbiddenInProfile`: `position` in `profiles/principal.toml`. The error names the profile file. |
| `cycle` | `positions/acme.toml` | Error `Cycle`: `a -> b -> c -> a`, located at the entry in `profiles/c.toml` that closes the circle. |
| `missing-profile` | `positions/acme.toml` | Error `MissingProfile`: `ai-platfrom`, the directory searched, and the available profile `principal`. |
| `extends-order-conflict` | `positions/acme.toml` | Error `ExtendsOrderConflict`: the spec lists `b` before `base`, but `b` extends `base`. `extends = ["base", "b"]` would load. The error also names the chain of reasons: `base` then `b`. |

The error names are the variants of `resume::spec::SpecError`. `tests/spec_test.rs` resolves every case here and asserts the outcome in this table.

Cases that need no files (a file that is not UTF-8, a BOM, CRLF line endings, an oversized file, a directory named `x.toml`, invalid names, a path that is not portable) are built in memory by the tests.

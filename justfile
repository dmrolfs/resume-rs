# ABOUTME: Task runner recipes for resume-rs: the dev loop, the quality gate, CI, and verification.
# ABOUTME: `just` lists them. `just check` is the gate beads tasks run, `just ci` is one CI leg, `just verify` is the full pre-push check on macOS.

set shell := ["bash", "-ueo", "pipefail", "-c"]

# On Windows a bare `bash` resolves to C:\Windows\System32\bash.exe, the WSL launcher, before PATH is
# consulted, so `just check` fails with "Windows Subsystem for Linux has no installed distributions".
# PowerShell always exists and is enough: the recipes that run on Windows are plain commands.

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# PLATFORM RULE: CI loads this file on Windows too. A recipe with no platform attribute must be a plain
# command (no bash syntax) and may depend only on other recipes with no platform attribute. Depending on
# a [macos] or [unix] recipe makes `just` fail to load on Windows, which breaks every recipe there.

# List the recipes
[private]
default:
    @just --list --unsorted

# ---------------------------------------------------------------------------------------------------
# Quality gate: what every task runs before closing, and what CI runs on each OS
# ---------------------------------------------------------------------------------------------------

[doc('Check formatting without changing files')]
[group('gate')]
fmt:
    cargo fmt --all --check

[doc('Apply formatting')]
[group('gate')]
fmt-fix:
    cargo fmt --all

[doc('Lint all targets of every workspace member; any warning fails')]
[group('gate')]
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

[doc('Run all tests of every workspace member, continuing past failures')]
[group('gate')]
test:
    cargo test --workspace --no-fail-fast

# Needs cargo-machete (cargo install cargo-machete --locked --version 0.9.2, the version CI pins). Two checks.
# First, cargo's `unused_dependencies` lint on the library and binary alone: clippy's all-targets run compiles the
# test units, so a normal dependency that only `#[cfg(test)]` code uses looks used there. Then cargo-machete, which
# reads the source text and so sees what the lint cannot: an unused dependency another one also pulls in, and an
# unused [dev-dependencies] entry (`--with-metadata` adds those; it does not change Cargo.lock when that is current).

[doc('Fail on a dependency nothing uses: the lint on the library and binary, then cargo-machete')]
[group('gate')]
unused-deps:
    cargo check --workspace --lib --bins
    cargo machete --with-metadata

[doc('The quality gate: formatting, lints, unused dependencies, and tests')]
[group('gate')]
check: fmt clippy unused-deps test

# ---------------------------------------------------------------------------------------------------
# Development loop
# ---------------------------------------------------------------------------------------------------

[doc('Debug build for the host')]
[group('dev')]
build:
    cargo build

[doc('Run the CLI, for example: just run build resume.md')]
[group('dev')]
run *args:
    cargo run -- {{ args }}

[doc('Run the tests whose name contains NAME, for example: just test-one crlf')]
[group('dev')]
test-one name:
    cargo test {{ name }}

[doc('Build the API docs and open them')]
[group('dev')]
doc:
    cargo doc --no-deps --open

[doc('Re-run the quality gate on every file change (needs cargo-watch)')]
[group('dev')]
[unix]
watch: (_need 'cargo-watch' 'cargo install cargo-watch --locked')
    cargo watch --clear --shell "just check"

[doc('Delete build output')]
[group('dev')]
clean:
    cargo clean

# ---------------------------------------------------------------------------------------------------
# CI: what a CI leg runs, so a failure can be reproduced locally
# ---------------------------------------------------------------------------------------------------

[doc('Locked release build, the same flags the shipped binaries use')]
[group('ci')]
release:
    cargo build --release --locked

[doc('One CI leg: the quality gate, then a locked release build')]
[group('ci')]
ci: check release

# ---------------------------------------------------------------------------------------------------
# Verification beyond the gate
# ---------------------------------------------------------------------------------------------------

[doc('Build the API docs and fail on any rustdoc warning')]
[group('verify')]
doc-check:
    cargo rustdoc --lib -- -D warnings

[doc('Fail if this justfile is not formatted the way `just --fmt` writes it')]
[group('verify')]
lint-justfile:
    just --unstable --fmt --check

# Fails with an install hint when a tool a recipe needs is missing, instead of cargo's generic "no such command".
[private]
[unix]
_need command install:
    @command -v {{ command }} >/dev/null 2>&1 || { echo "error: {{ command }} not found; install it with: {{ install }}" >&2; exit 1; }

[doc('Lint the GitHub Actions workflows (needs actionlint)')]
[group('verify')]
[unix]
lint-workflows: (_need 'actionlint' 'brew install actionlint')
    actionlint

[doc('Check dependencies against the RustSec advisory database, for example: just audit --no-fetch (needs cargo-audit)')]
[group('verify')]
[unix]
audit *args: (_need 'cargo-audit' 'cargo install cargo-audit --locked')
    cargo audit {{ args }}

[doc('Show compatible dependency updates without changing Cargo.lock (cargo update --dry-run)')]
[group('verify')]
[unix]
outdated:
    cargo update --dry-run

[doc('Line coverage summary, for example: just coverage --html (needs cargo-llvm-cov)')]
[group('verify')]
[unix]
coverage *args: (_need 'cargo-llvm-cov' 'cargo install cargo-llvm-cov --locked')
    cargo llvm-cov --summary-only {{ args }}

[doc('Mutation testing, for example: just mutants --file src/markdown (needs cargo-mutants)')]
[group('verify')]
[unix]
mutants *args: (_need 'cargo-mutants' 'cargo install cargo-mutants --locked')
    cargo mutants {{ args }}

[doc('Check each package with its own minimum Rust version: resume (Cargo.toml) and the app (crates/workbench/Cargo.toml)')]
[group('verify')]
[unix]
msrv:
    #!/usr/bin/env bash
    set -euo pipefail
    check_floor() {
        local manifest="$1" package="$2" msrv
        msrv=$(sed -n 's/^rust-version = "\(.*\)"/\1/p' "$manifest")
        [ -n "$msrv" ] || { echo "error: no rust-version in $manifest" >&2; exit 1; }
        rustup toolchain list | grep -q "^${msrv}" || {
            echo "error: Rust ${msrv} is not installed; run: rustup toolchain install ${msrv} --profile minimal" >&2
            exit 1
        }
        cargo +"${msrv}" check --locked --all-targets --package "$package"
    }
    check_floor Cargo.toml resume
    check_floor crates/workbench/Cargo.toml workbench

[doc('Everything to run before pushing: CI leg, docs, lint, and both cross-builds (macOS)')]
[group('verify')]
[macos]
verify: ci doc-check lint-justfile lint-workflows build-macos build-windows

# ---------------------------------------------------------------------------------------------------
# Cross-builds
# ---------------------------------------------------------------------------------------------------

[doc('Build a signed, verified universal2 (arm64 + x86_64) macOS binary at target/universal2/resume')]
[group('build')]
[macos]
build-macos:
    #!/usr/bin/env bash
    set -euo pipefail
    # 11.0 is the arm64 floor; using it for both slices gives uniform behavior (the cc crate reads it too).
    export MACOSX_DEPLOYMENT_TARGET=11.0
    for target in aarch64-apple-darwin x86_64-apple-darwin; do
        rustup target list --installed | grep -qx "$target" || rustup target add "$target"
        cargo build --release --locked --target "$target"
    done
    mkdir -p target/universal2
    lipo -create -output target/universal2/resume \
        target/aarch64-apple-darwin/release/resume target/x86_64-apple-darwin/release/resume
    # lipo output must be re-signed: Apple silicon refuses to run an unsigned or invalidly signed binary.
    codesign --force --sign - target/universal2/resume
    lipo -info target/universal2/resume
    lipo target/universal2/resume -verify_arch arm64 x86_64
    codesign --verify --verbose=2 target/universal2/resume
    file target/universal2/resume

[doc('Cross-build a Windows x64 resume.exe from macOS or Linux (windows-gnu) and prove it is self-contained')]
[group('build')]
[unix]
build-windows:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v x86_64-w64-mingw32-gcc >/dev/null || { echo "error: mingw-w64 not found (brew install mingw-w64)" >&2; exit 1; }
    rustup target list --installed | grep -qx x86_64-pc-windows-gnu || rustup target add x86_64-pc-windows-gnu
    cargo build --release --locked --target x86_64-pc-windows-gnu
    just check-windows-imports target/x86_64-pc-windows-gnu/release/resume.exe

[doc('Fail if a Windows exe imports a runtime DLL a clean machine may lack (mingw, dynamic std, VC++)')]
[group('build')]
[unix]
check-windows-imports exe:
    bash scripts/check-pe-imports.sh exe "{{ exe }}"

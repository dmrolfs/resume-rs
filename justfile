# ABOUTME: Task runner recipes for resume-rs (format, lint, test, and the combined `check` gate).
# ABOUTME: `just check` is the acceptance gate every beads task runs before closing.

set shell := ["bash", "-ueo", "pipefail", "-c"]

# Check formatting without changing files
fmt:
    cargo fmt --all --check

# Apply formatting
fmt-fix:
    cargo fmt --all

# Lint all targets; any warning fails
clippy:
    cargo clippy --all-targets -- -D warnings

# Run all tests
test:
    cargo test --no-fail-fast

# Build a release binary for the host
build:
    cargo build --release

# The quality gate: formatting, lints, and tests
check: fmt clippy test

# Build a universal2 (arm64 + x86_64) macOS binary at target/universal2/resume, ad-hoc signed and verified.
# Two builds plus lipo is the supported path; `universal2-apple-darwin` is not a rustc target.
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

# Cross-build a Windows x64 resume.exe from macOS or Linux (windows-gnu) and prove it is self-contained.
# This is a fast compile and link check only. Release binaries are built with MSVC in CI.
[unix]
build-windows:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v x86_64-w64-mingw32-gcc >/dev/null || { echo "error: mingw-w64 not found (brew install mingw-w64)" >&2; exit 1; }
    rustup target list --installed | grep -qx x86_64-pc-windows-gnu || rustup target add x86_64-pc-windows-gnu
    cargo build --release --locked --target x86_64-pc-windows-gnu
    just check-windows-imports target/x86_64-pc-windows-gnu/release/resume.exe

# Fail if a Windows exe imports a non-system runtime DLL, meaning it is not self-contained.
# rustc links libgcc_eh and winpthread statically for ordinary windows-gnu executables, so any
# `lib*.dll` (libgcc_s_seh-1, libwinpthread-1, libstdc++), `std-<hash>.dll` (dynamic Rust std), or
# hash-suffixed DLL means the link went wrong. Windows system DLLs (kernel32, api-ms-win-*) are fine.
[unix]
check-windows-imports exe:
    #!/usr/bin/env bash
    set -euo pipefail
    file "{{exe}}"
    imports=$(x86_64-w64-mingw32-objdump -p "{{exe}}" | awk '/DLL Name:/ {print $3}' | sort -fu)
    echo "$imports"
    if bad=$(echo "$imports" | grep -iE '^lib|^std-|-[0-9a-f]{8,}\.dll$'); then
        echo "error: {{exe}} is not self-contained; it imports:" >&2
        echo "$bad" >&2
        exit 1
    fi
    echo "ok: {{exe}} imports only system DLLs"

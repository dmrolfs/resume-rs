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

# ABOUTME: Deliberately broken justfile used to prove tests/justfile_test.rs detects platform-rule violations.
# ABOUTME: Not a real justfile: never run these recipes.

# Valid: platform-gated and plain.
[unix]
gated-helper:
    echo helper

# VIOLATION 1: an ungated recipe depends on a [unix] recipe, so `just` could not load this file on Windows.
broken-dependency: gated-helper
    echo broken

# VIOLATION 2: an ungated recipe is a bash script, which Windows (PowerShell) cannot run.
broken-shebang:
    #!/usr/bin/env bash
    echo broken

# Fine: ungated, plain command, no gated dependency.
plain:
    echo ok

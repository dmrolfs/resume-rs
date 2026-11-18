//! ABOUTME: Tests the rule that decides whether a Windows exe's imported DLL names are self-contained.
//! ABOUTME: Runs scripts/check-pe-imports.sh on name lists (unix only; bash on Windows can be the WSL stub).

#![cfg(unix)]

use assert_cmd::Command;
use predicates::str::contains;

/// Runs `check-pe-imports.sh names` with `input` on stdin, from the crate root.
fn check_names(input: &str) -> assert_cmd::assert::Assert {
    Command::new("bash")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["scripts/check-pe-imports.sh", "names"])
        .write_stdin(input)
        .assert()
}

#[test]
fn windows_system_dlls_are_accepted() {
    check_names("KERNEL32.dll\nntdll.dll\nbcryptprimitives.dll\napi-ms-win-crt-heap-l1-1-0.dll\n")
        .success()
        .stdout(contains("ok: imports only system DLLs"));
}

#[test]
fn the_vc_runtime_is_rejected_by_name() {
    check_names("KERNEL32.dll\nVCRUNTIME140.dll\n")
        .failure()
        .stderr(contains("VCRUNTIME140.dll"));
}

#[test]
fn vc_runtime_variants_are_rejected_regardless_of_case() {
    for dll in [
        "vcruntime140_1.dll",
        "MSVCP140.dll",
        "msvcr120.dll",
        "VCOMP140.DLL",
        "ConcRT140.dll",
    ] {
        check_names(&format!("KERNEL32.dll\n{dll}\n"))
            .failure()
            .stderr(contains(dll));
    }
}

#[test]
fn mingw_runtime_and_dynamic_std_are_rejected() {
    check_names("KERNEL32.dll\nlibgcc_s_seh-1.dll\nlibwinpthread-1.dll\nstd-68a98b1e3aff2557.dll\n")
        .failure()
        .stderr(contains("libgcc_s_seh-1.dll"))
        .stderr(contains("libwinpthread-1.dll"))
        .stderr(contains("std-68a98b1e3aff2557.dll"));
}

#[test]
fn windows_line_endings_in_the_tool_output_are_handled() {
    check_names("KERNEL32.dll\r\nntdll.dll\r\n").success();
    check_names("KERNEL32.dll\r\nVCRUNTIME140.dll\r\n")
        .failure()
        .stderr(contains("VCRUNTIME140.dll"));
}

#[test]
fn an_empty_import_list_fails_closed() {
    // If the reading tool ever changes its output format, no names would be found. That must not pass.
    check_names("").failure().stderr(contains("no imports found"));
    check_names("\n\n").failure().stderr(contains("no imports found"));
}

#[test]
fn an_unknown_subcommand_is_a_usage_error() {
    Command::new("bash")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["scripts/check-pe-imports.sh", "frobnicate"])
        .assert()
        .code(2)
        .stderr(contains("usage:"));
}

#!/usr/bin/env bash
# ABOUTME: Fails if a Windows exe imports a runtime DLL that a clean machine may not have (VC++ or mingw runtime).
# ABOUTME: `exe PATH` reads imports with rustup's llvm-readobj on macOS or Windows; `names` reads DLL names on stdin.
#
# Usage:
#   bash scripts/check-pe-imports.sh exe PATH_TO_EXE
#   bash scripts/check-pe-imports.sh names < list-of-dll-names
#
# Windows system DLLs (kernel32, ntdll, bcryptprimitives, api-ms-win-*) are allowed. These are not:
#   lib*.dll               mingw runtime: libgcc_s_seh-1.dll, libwinpthread-1.dll
#   std-<hash>.dll         a dynamically linked Rust standard library
#   *-<8+ hex digits>.dll  any other hash-suffixed Rust DLL
#   vcruntime*, msvcp*, msvcr*, vcomp*, concrt*, ucrtbased*
#                          the Visual C++ runtime (and debug UCRT), which Windows does not ship by default.
#                          Rust's MSVC targets link it dynamically unless `-C target-feature=+crt-static`.
#
# Run this with bash explicitly: on Windows, a bare `bash` can resolve to the WSL launcher.

set -euo pipefail

deny='^lib|^std-|-[0-9a-f]{8,}\.dll$|^(vcruntime|msvcp|msvcr|vcomp|concrt|ucrtbased)'

usage() {
  echo "usage: check-pe-imports.sh exe PATH_TO_EXE | names < list-of-dll-names" >&2
  exit 2
}

# Reads DLL names on stdin; prints them and exits 1 if any is denied or if there are none.
check_names() {
  local names
  names=$(sed 's/\r$//' | awk 'NF' | sort -fu)
  if [ -z "$names" ]; then
    echo "error: no imports found; the reading tool's output format may have changed" >&2
    exit 1
  fi
  printf '%s\n' "$names"
  local bad
  if bad=$(printf '%s\n' "$names" | grep -iE "$deny"); then
    echo "error: not self-contained; it imports a runtime DLL that a clean machine may lack:" >&2
    printf '%s\n' "$bad" >&2
    exit 1
  fi
  echo "ok: imports only system DLLs"
}

# Finds llvm-readobj in the active Rust toolchain (component `llvm-tools`).
find_readobj() {
  local sysroot host dir tool
  sysroot=$(rustc --print sysroot)
  # Under Git Bash, rustc prints a Windows path; convert it so the shell can use it.
  if command -v cygpath >/dev/null 2>&1; then sysroot=$(cygpath -u "$sysroot"); fi
  host=$(rustc -vV | sed -n 's/^host: //p')
  dir="$sysroot/lib/rustlib/$host/bin"
  for tool in llvm-readobj llvm-readobj.exe; do
    if [ -x "$dir/$tool" ]; then
      printf '%s\n' "$dir/$tool"
      return 0
    fi
  done
  echo "error: llvm-readobj not found in $dir; run: rustup component add llvm-tools" >&2
  return 1
}

check_exe() {
  local exe=$1 readobj
  [ -f "$exe" ] || { echo "error: $exe not found" >&2; exit 1; }
  readobj=$(find_readobj)
  # --coff-imports prints one `Name: X.dll` line per imported DLL (and per delay-loaded DLL).
  "$readobj" --coff-imports "$exe" | sed -n 's/^ *Name: \(.*\)$/\1/p' | check_names
}

case "${1:-}" in
  names) check_names ;;
  exe)
    [ $# -eq 2 ] || usage
    check_exe "$2"
    ;;
  *) usage ;;
esac

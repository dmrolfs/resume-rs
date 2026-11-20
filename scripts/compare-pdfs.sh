#!/usr/bin/env bash
# ABOUTME: Fails unless every PDF given is present, non-empty, free of forbidden font names, and byte-identical.
# ABOUTME: CI runs it on PDFs built on different operating systems; it prints GitHub annotations and each SHA-256.
#
# Usage:
#   bash scripts/compare-pdfs.sh LABEL=PATH LABEL=PATH [LABEL=PATH ...] [--forbid NAME ...]
#
# Each LABEL=PATH names a file for the messages (for example `macOS-CRLF=pdf/macos/crlf/Resume.pdf`). At least two
# are needed. `--forbid NAME` fails the run when any file contains NAME, such as `Calibri`, which a bundled-fonts
# build must never embed. It fails closed: a missing or empty file is an error, never a pass.
#
# Run this with bash explicitly: on Windows, a bare `bash` can resolve to the WSL launcher.

set -euo pipefail

usage() {
  echo "usage: compare-pdfs.sh LABEL=PATH LABEL=PATH [LABEL=PATH ...] [--forbid NAME ...]" >&2
  exit 2
}

# sha256sum on Linux, shasum on macOS.
sha256() {
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

labels=()
paths=()
forbidden=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --forbid)
      [ "$#" -ge 2 ] || usage
      forbidden+=("$2")
      shift 2
      ;;
    *=*)
      labels+=("${1%%=*}")
      paths+=("${1#*=}")
      shift
      ;;
    *) usage ;;
  esac
done
[ "${#paths[@]}" -ge 2 ] || usage

fail() { # title, message
  echo "::error title=$1::$2"
  exit 1
}

for i in "${!paths[@]}"; do
  if [ ! -s "${paths[$i]}" ]; then
    fail "Reference PDF missing or empty" "${labels[$i]}: ${paths[$i]} is missing or empty"
  fi
done

if [ "${#forbidden[@]}" -gt 0 ]; then
  for i in "${!paths[@]}"; do
    for name in "${forbidden[@]}"; do
      if grep -aq -- "$name" "${paths[$i]}"; then
        fail "A forbidden font name is in a PDF" "${labels[$i]}: ${paths[$i]} contains the forbidden name ${name}"
      fi
    done
  done
fi

for i in "${!paths[@]}"; do
  printf '%s  %s bytes  %s  %s\n' "$(sha256 "${paths[$i]}")" "$(wc -c < "${paths[$i]}" | tr -d ' ')" "${labels[$i]}" "${paths[$i]}"
done

for i in "${!paths[@]}"; do
  [ "$i" -eq 0 ] && continue
  if ! cmp -s "${paths[0]}" "${paths[$i]}"; then
    # `cmp -l` lists each differing byte as "OFFSET OCTAL OCTAL", with the offset counted from 1 on every platform
    # (the plain message says "byte" on Linux and "char" on macOS).
    offset="$(cmp -l "${paths[0]}" "${paths[$i]}" 2>/dev/null | head -1 | awk '{print $1}' || true)"
    if [ -n "$offset" ]; then
      first="first difference at byte ${offset}"
    else
      first="one file ends where the other continues"
    fi
    sizes="$(wc -c < "${paths[0]}" | tr -d ' ') bytes (${labels[0]}), $(wc -c < "${paths[$i]}" | tr -d ' ') bytes (${labels[$i]})"
    hashes="${labels[0]} $(sha256 "${paths[0]}"), ${labels[$i]} $(sha256 "${paths[$i]}")"
    fail "PDF bytes differ across builds" "PDF bytes differ: ${labels[0]} and ${labels[$i]}: ${first}; ${sizes}; sha256 ${hashes}"
  fi
done

echo "::notice title=PDFs are byte-identical::$(sha256 "${paths[0]}") (${#paths[@]} files)"

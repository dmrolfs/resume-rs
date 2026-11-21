#!/usr/bin/env bash
# ABOUTME: Packs a built `resume` binary with its licenses into a release archive and writes its SHA-256 line.
# ABOUTME: One script for both systems: a .tar.gz for macOS and a .zip for Windows; the archive holds one directory.
#
# Usage: package-release.sh BINARY LABEL VERSION OUTDIR
#   BINARY   the built program (target/universal2/resume, target/release/resume.exe)
#   LABEL    which system it is for, used in the name: macos-universal2, windows-x64
#   VERSION  the release, such as v0.1.0 (the tag)
#   OUTDIR   where the archive and its `.sha256` file go (created if missing)
#
# Writes OUTDIR/resume-VERSION-LABEL.tar.gz (or .zip when BINARY ends in .exe) and the same name plus `.sha256`,
# whose one line is `HASH  NAME`, the form `shasum -a 256 -c` and `sha256sum -c` read.
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 BINARY LABEL VERSION OUTDIR" >&2
  exit 2
fi
binary=$1 label=$2 version=$3 outdir=$4

[ -f "$binary" ] || { echo "error: no such file: $binary" >&2; exit 1; }
case "$label$version" in
  *[!A-Za-z0-9._-]*) echo "error: LABEL and VERSION may hold only letters, digits, '.', '_' and '-'" >&2; exit 2 ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)
name="resume-$version-$label"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name" "$outdir"

cp "$binary" "$stage/$name/"
# A downloaded CI artifact has lost its permission bits; a program in a tar archive must still run when unpacked.
chmod 755 "$stage/$name/$(basename "$binary")"
# The binary carries fonts and crates with their own licenses; the files that say so travel with it.
cp "$root/LICENSE" "$root/README.md" "$stage/$name/"
cp "$root/fonts/LICENSES.md" "$stage/$name/FONT-LICENSES.md"

outdir=$(cd "$outdir" && pwd)
case "$binary" in
  *.exe)
    archive="$name.zip"
    # `python3` on a Windows runner can be the Microsoft Store stub, which does nothing; `python` is the real one there.
    # A candidate counts only if it can run a one-line program, so a stub is passed over.
    python=
    for candidate in python python3 py; do
      if command -v "$candidate" >/dev/null && "$candidate" -c 'import zipfile' >/dev/null 2>&1; then python=$candidate; break; fi
    done
    [ -n "$python" ] || { echo "error: a working python is needed to make a zip" >&2; exit 1; }
    rm -f "$outdir/$archive"
    (cd "$stage" && "$python" -m zipfile -c "$outdir/$archive" "$name") ;;
  *)
    archive="$name.tar.gz"
    # COPYFILE_DISABLE keeps macOS tar from adding `._name` entries for extended attributes.
    COPYFILE_DISABLE=1 tar -C "$stage" -czf "$outdir/$archive" "$name" ;;
esac

# `--text` keeps the line `HASH  NAME` (two spaces); Git Bash's sha256sum otherwise writes `HASH *NAME` (binary mode).
if command -v sha256sum >/dev/null; then
  (cd "$outdir" && sha256sum --text "$archive" > "$archive.sha256")
else
  (cd "$outdir" && shasum -a 256 "$archive" > "$archive.sha256")
fi
cat "$outdir/$archive.sha256"

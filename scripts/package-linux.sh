#!/usr/bin/env bash
# Zips the release build (target/release/morpheus-desktop) the way the updater
# expects: morpheus-<version>-linux-x86_64.zip holding one folder with the
# program. Everything else (fonts, icons, sounds) is inside the program.
# Prints the zip's path. Used by CI (linux.yml) and by hand.
set -euo pipefail
# Info-ZIP reads options from $ZIP and $ZIPOPT.
unset ZIP ZIPOPT
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name="morpheus-$version-linux-x86_64"
out=${DIST:-dist}
exe=target/release/morpheus-desktop
[ -x "$exe" ] || { echo "build it first: cargo build --release -p morpheus-desktop --locked" >&2; exit 1; }
# Every library it links is one a distribution has.
if ldd "$exe" | grep "not found" >&2; then exit 1; fi
rm -rf "$out/$name" "$out/$name.zip"
mkdir -p "$out/$name"
cp "$exe" "$out/$name/morpheus-desktop"
(cd "$out" && zip -qr "$name.zip" "$name")
rm -rf "$out/$name"
echo "$out/$name.zip"

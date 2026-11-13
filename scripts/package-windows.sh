#!/usr/bin/env bash
# Zips the Windows release build (target/x86_64-pc-windows-msvc/release/
# morpheus-desktop.exe) as the updater expects: morpheus-<version>-windows-
# x86_64.zip holding one folder with Morpheus.exe (fonts, icons and sounds are
# inside it). Prints the zip's path. Used by release-windows.sh (cross-built
# on Linux, into target/dist) and by CI (windows.yml, into dist; Git Bash).
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name="morpheus-$version-windows-x86_64"
out=${DIST:-dist}
exe=target/x86_64-pc-windows-msvc/release/morpheus-desktop.exe
[ -f "$exe" ] || { echo "build it first: cargo build --release -p morpheus-desktop --locked --target x86_64-pc-windows-msvc" >&2; exit 1; }
rm -rf "$out/$name" "$out/$name.zip"
mkdir -p "$out/$name"
cp "$exe" "$out/$name/Morpheus.exe"
if command -v zip >/dev/null; then
    (cd "$out" && zip -qr "$name.zip" "$name")
elif command -v 7z >/dev/null; then
    (cd "$out" && 7z a -tzip -bso0 "$name.zip" "$name")
else
    (cd "$out" && powershell -NoProfile -Command "Compress-Archive -Path '$name' -DestinationPath '$name.zip'")
fi
rm -rf "$out/$name"
echo "$out/$name.zip"

#!/usr/bin/env bash
# Zips the Windows release build as the updater expects: morpheus-<version>-
# windows-x86_64.zip holding one folder, Morpheus (no version in it, so an
# unpacked download lands in the same place each time), with
#   Morpheus.exe              the launcher (target/.../morpheus-launcher.exe):
#                             what pins and shortcuts point at; updates never
#                             touch it
#   app/morpheus-desktop.exe  the app (fonts, icons and sounds are inside it);
#                             an update replaces what is in app/
# Prints the zip's path. Used by release-windows.sh (cross-built on Linux, into
# target/dist) and by CI (windows.yml, into dist; Git Bash).
set -euo pipefail
# Info-ZIP reads options from $ZIP and $ZIPOPT.
unset ZIP ZIPOPT
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name="morpheus-$version-windows-x86_64"
folder=Morpheus
out=${DIST:-dist}
built=target/x86_64-pc-windows-msvc/release
for exe in "$built/morpheus-desktop.exe" "$built/morpheus-launcher.exe"; do
    [ -f "$exe" ] || { echo "build it first: cargo build --release -p morpheus-desktop -p morpheus-launcher --locked --target x86_64-pc-windows-msvc" >&2; exit 1; }
done
rm -rf "$out/$folder" "$out/$name.zip"
mkdir -p "$out/$folder/app"
cp "$built/morpheus-launcher.exe" "$out/$folder/Morpheus.exe"
cp "$built/morpheus-desktop.exe" "$out/$folder/app/morpheus-desktop.exe"
if command -v zip >/dev/null; then
    (cd "$out" && zip -qr "$name.zip" "$folder")
elif command -v 7z >/dev/null; then
    (cd "$out" && 7z a -tzip -bso0 "$name.zip" "$folder")
else
    (cd "$out" && powershell -NoProfile -Command "Compress-Archive -Path '$folder' -DestinationPath '$name.zip'")
fi
rm -rf "$out/$folder"
echo "$out/$name.zip"

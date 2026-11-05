#!/usr/bin/env bash
# The desktop app for Windows (x86-64), cross-built here:
# target/dist/morpheus-<version>-windows-x86_64.zip (Morpheus.exe and its
# assets). Run inside `nix develop .#windows`; cargo-xwin downloads
# Microsoft's headers and libraries on first use.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

# cargo-xwin's environment (clang-cl, lld-link, the Windows SDK), with its
# flags through RUSTFLAGS: dx sets that variable itself, which hides the
# per-target ones, and without +crt-static the C libraries take the dynamic
# runtime while LiveKit's WebRTC has the static one (the link fails). With
# --target, RUSTFLAGS reaches only the Windows build; the lld-link flavour
# is left out, as Cargo also probes the host with these flags.
eval "$(cargo xwin env --target x86_64-pc-windows-msvc)"
export RUSTFLAGS="${CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS/-C linker-flavor=lld-link/}"
dx build --release --package morpheus-desktop --platform windows --target x86_64-pc-windows-msvc

app=target/dx/morpheus-desktop/release/windows/app
name="morpheus-$version-windows-x86_64"
dist="target/dist/$name"
rm -rf "$dist" "$dist.zip"
mkdir -p "$dist"
cp -r "$app/assets" "$dist/"
cp "$app/morpheus-desktop.exe" "$dist/Morpheus.exe"
(cd target/dist && zip -qr "$name.zip" "$name")
rm -rf "$dist"
ls -la "target/dist/$name.zip"

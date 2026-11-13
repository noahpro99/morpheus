#!/usr/bin/env bash
# The desktop app for Windows (x86-64), cross-built here:
# target/dist/morpheus-<version>-windows-x86_64.zip (a folder with Morpheus.exe;
# fonts and images are inside it). Run inside `nix develop .#windows`;
# cargo-xwin downloads Microsoft's headers and libraries on first use. (CI's
# windows.yml builds the same zip natively; GPUI's shaders are compiled
# when the program starts here, as there is no fxc.exe to do it while building.)
set -euo pipefail
cd "$(dirname "$0")/.."

# cargo-xwin's environment (clang-cl, lld-link, the Windows SDK), with its
# flags through RUSTFLAGS: without +crt-static the C libraries take the
# dynamic runtime while LiveKit's WebRTC has the static one (the link
# fails). With --target, RUSTFLAGS reaches only the Windows build; the
# lld-link flavour is left out, as Cargo also probes the host with these
# flags.
eval "$(cargo xwin env --target x86_64-pc-windows-msvc)"
export RUSTFLAGS="${CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS/-C linker-flavor=lld-link/}"
cargo build --release -p morpheus-desktop --locked --target x86_64-pc-windows-msvc

DIST=target/dist scripts/package-windows.sh
ls -la target/dist/morpheus-*-windows-x86_64.zip | tail -1

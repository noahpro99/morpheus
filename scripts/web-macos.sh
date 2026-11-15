#!/usr/bin/env bash
# `just web` for macOS without Nix: the same `dx serve` of the web app at
# http://localhost:8080, with the tools `nix develop .#web` would provide
# installed through Homebrew and cargo on first run.
#
#   scripts/web-macos.sh            # dev server, rebuilds on save
#   scripts/web-macos.sh release    # static build in target/dx/morpheus-web/release/web/public
set -euo pipefail
cd "$(dirname "$0")/.."

command -v brew >/dev/null || { echo "Homebrew is needed: https://brew.sh" >&2; exit 1; }

# rustup (not brew's plain `rust`): the pinned toolchain and the wasm target come from it.
if ! command -v rustup >/dev/null && [ ! -x "$(brew --prefix)/opt/rustup/bin/rustup" ]; then
    brew install rustup
fi
export PATH="$(brew --prefix)/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
rustup show active-toolchain >/dev/null 2>&1 || rustup toolchain install
rustup target add wasm32-unknown-unknown

# Apple's clang cannot target wasm; ring and zstd compile C for it with LLVM's.
llvm=$(brew --prefix llvm 2>/dev/null || true)
if [ ! -x "$llvm/bin/clang" ]; then
    brew install llvm
    llvm=$(brew --prefix llvm)
fi
export CC_wasm32_unknown_unknown="$llvm/bin/clang"
export AR_wasm32_unknown_unknown="$llvm/bin/llvm-ar"

# dx must match the dioxus crate in Cargo.lock (0.7); it fetches wasm-bindgen and wasm-opt itself.
if ! dx --version 2>/dev/null | grep -q ' 0\.7\.'; then
    cargo install dioxus-cli --version '^0.7' --locked
fi

export MORPHEUS_HOMESERVER=${MORPHEUS_HOMESERVER:-http://localhost:8008}

if [ "${1:-}" = release ]; then
    exec dx build --package morpheus-web --platform web --release
fi
exec dx serve --package morpheus-web --platform web

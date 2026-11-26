#!/usr/bin/env bash
# The Rust half of the iOS app (macOS only): crates/ffi as static libraries for the iOS targets, the Swift bindings
# generated from the library's own metadata, and MorpheusFFI.xcframework. Usage: scripts/ios-ffi.sh [target...]
# (default: aarch64-apple-ios-sim aarch64-apple-ios). Output: apps/ios/build and apps/ios/Generated.
set -euo pipefail
cd "$(dirname "$0")/.."
targets=("$@")
[ ${#targets[@]} -gt 0 ] || targets=(aarch64-apple-ios-sim aarch64-apple-ios)
out=apps/ios

rustup target add "${targets[@]}"
args=()
for t in "${targets[@]}"; do
    # A static library from the crate's rlib (the manifest's cdylib is for Android).
    cargo rustc -p morpheus-ffi --lib --release --target "$t" --crate-type staticlib -- --print native-static-libs
    lib="target/$t/release/libmorpheus_ffi.a"
    args+=(-library "$lib" -headers "$out/build/headers")
    first=${first:-$lib}
done

CARGO_TARGET_DIR=target/uniffi-bindgen cargo build -q --manifest-path tools/uniffi-bindgen/Cargo.toml
rm -rf "$out/build" "$out/Generated"
mkdir -p "$out/build/headers" "$out/Generated" "$out/build/gen"
target/uniffi-bindgen/debug/uniffi-bindgen generate --library "$first" --language swift --no-format --out-dir "$out/build/gen"
cp "$out/build/gen/morpheus_ffi.swift" "$out/Generated/"
cp "$out/build/gen/morpheus_ffiFFI.h" "$out/build/headers/"
cp "$out/build/gen/morpheus_ffiFFI.modulemap" "$out/build/headers/module.modulemap"
xcodebuild -create-xcframework "${args[@]}" -output "$out/build/MorpheusFFI.xcframework"
echo "built $out/build/MorpheusFFI.xcframework"

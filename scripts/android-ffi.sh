#!/usr/bin/env bash
# The Rust half of the native Android app (apps/android): crates/ffi built for
# Android, and its Kotlin bindings. Gradle runs it (the `buildRust` task);
# run it by hand to check the Rust side alone.
#
#   scripts/android-ffi.sh <out-dir> [abis] [profile]
#
# abis: comma separated, `arm64-v8a` (phones) and/or `x86_64` (the emulator).
# profile: a cargo profile; `android-dev` (optimised dependencies, no debug
# info) or `release`.
# Needs the linkers `nix develop .#android` sets up.
set -euo pipefail
cd "$(dirname "$0")/.."

out=${1:?out dir}
abis=${2:-arm64-v8a,x86_64}
profile=${3:-android-dev}

case "$profile" in
    dev) profile_dir=debug ;;
    *) profile_dir=$profile ;;
esac

rm -rf "$out/jniLibs" "$out/kotlin"
mkdir -p "$out/jniLibs" "$out/kotlin"
first=""
for abi in ${abis//,/ }; do
    case "$abi" in
        arm64-v8a) triple=aarch64-linux-android ;;
        x86_64) triple=x86_64-linux-android ;;
        *) echo "unknown ABI $abi" >&2; exit 1 ;;
    esac
    cargo build -p morpheus-ffi --target "$triple" --profile "$profile"
    mkdir -p "$out/jniLibs/$abi"
    cp "target/$triple/$profile_dir/libmorpheus_ffi.so" "$out/jniLibs/$abi/"
    first=${first:-$out/jniLibs/$abi/libmorpheus_ffi.so}
done

# The bindings are read from the library's own metadata (any ABI will do).
cargo run -q -p morpheus-ffi --features bindgen --bin uniffi-bindgen -- \
    generate --library "$first" --language kotlin --no-format --out-dir "$out/kotlin"

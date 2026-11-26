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
strip=${ANDROID_NDK_HOME:+$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip}
for abi in ${abis//,/ }; do
    case "$abi" in
        arm64-v8a) triple=aarch64-linux-android ;;
        x86_64) triple=x86_64-linux-android ;;
        *) echo "unknown ABI $abi" >&2; exit 1 ;;
    esac
    cargo build -p morpheus-ffi --target "$triple" --profile "$profile"
    mkdir -p "$out/jniLibs/$abi"
    lib="target/$triple/$profile_dir/libmorpheus_ffi.so"
    if [ "$profile" = android-dev ] && [ -x "$strip" ]; then
        # The symbol table (56 MB of 186) is only for backtraces; the library in jniLibs does without. The unstripped one
        # in target/ stays: UniFFI's metadata (the bindings below) is read from its symbols.
        "$strip" --strip-all -o "$out/jniLibs/$abi/libmorpheus_ffi.so" "$lib"
    else
        cp "$lib" "$out/jniLibs/$abi/"
    fi
    first=${first:-$lib}
done

# The bindings are read from the library's own metadata (any ABI will do). The
# generator is a small crate of its own (tools/uniffi-bindgen), so no host build
# of crates/ffi (matrix-sdk, aws-lc, ...) happens here and the host target trees
# of the other shells are left alone. Its binary stays in target/uniffi-bindgen
# (cargo has nothing to do when it is there).
want=$(awk '/^name = "uniffi"$/ {getline; gsub(/[^0-9.]/, "", $0); print $0; exit}' Cargo.lock)
have=$(sed -nE 's/^uniffi = .*"=([0-9.]+)".*/\1/p' tools/uniffi-bindgen/Cargo.toml)
[ "$want" = "$have" ] || { echo "tools/uniffi-bindgen pins uniffi $have but Cargo.lock has $want: update its Cargo.toml and lock" >&2; exit 1; }
bindgen_dir=target/uniffi-bindgen
CARGO_TARGET_DIR=$bindgen_dir cargo build -q --manifest-path tools/uniffi-bindgen/Cargo.toml
"$bindgen_dir/debug/uniffi-bindgen" generate --library "$first" --language kotlin --no-format --out-dir "$out/kotlin"

# WebRTC's Java classes (calls) come with the build of its native half; the
# app compiles against them and ships them. The same jar serves every ABI.
mkdir -p "$out/libs"
case "$first" in *x86_64-linux-android*) flavour=android-x64 ;; *) flavour=android-arm64 ;; esac
# It sits in the download folder of the build script of this profile (a search of all of target/ took 20 s).
jar=$(ls -t "target/$profile_dir"/build/*/out/livekit_webrtc/livekit/"$flavour"-release-webrtc*/*/libwebrtc.jar 2>/dev/null | head -n 1 || true)
[ -n "$jar" ] || jar=$(find "target/$profile_dir" -name libwebrtc.jar -path "*/$flavour-release-webrtc*" -printf "%T@ %p\n" 2>/dev/null | sort -rn | head -n 1 | cut -d" " -f2- || true)
[ -n "$jar" ] || { echo "libwebrtc.jar was not built" >&2; exit 1; }
cp "$jar" "$out/libs/libwebrtc.jar"

#!/usr/bin/env bash
# What the Android checks that never run the app need (lint, detekt, the JVM tests): the Kotlin
# bindings of crates/ffi and the one WebRTC class the app names. Nothing is cross-built: the
# bindings are read from the HOST build of the library (they are the same on every target), and
# libwebrtc.jar is a one-class stub (the real one comes with the Android build of crates/call,
# which these checks never call).
#
#   scripts/android-check-prep.sh [out-dir]    # default target/android-checks
#
# Then: gradle -PrustPrebuilt=<out-dir> ... (`just android-lint` does both).
set -euo pipefail
cd "$(dirname "$0")/.."

out=$(realpath -m "${1:-target/android-checks}")
mkdir -p "$out/kotlin" "$out/libs"

profile=${ANDROID_CHECK_PROFILE:-debug}   # CI builds release: the Linux cache is a release one
flags=(); [ "$profile" = release ] && flags=(--release)
cargo build -p morpheus-ffi "${flags[@]}"
cargo run -q "${flags[@]}" -p morpheus-ffi --features bindgen --bin uniffi-bindgen -- \
    generate --library "target/$profile/libmorpheus_ffi.so" --language kotlin --no-format --out-dir "$out/kotlin"

# The tests load the library itself (the colour helpers) through JNA.
mkdir -p "$out/lib" && cp "target/$profile/libmorpheus_ffi.so" "$out/lib/"

stub=$out/stub
rm -rf "$stub" && mkdir -p "$stub/src/livekit/org/webrtc" "$stub/classes"
cat > "$stub/src/livekit/org/webrtc/ContextUtils.java" <<'J'
package livekit.org.webrtc;
public class ContextUtils {
    public static void initialize(Object applicationContext) {}
}
J
javac -d "$stub/classes" "$stub/src/livekit/org/webrtc/ContextUtils.java"
(cd "$stub/classes" && jar cf "$out/libs/libwebrtc.jar" .)
rm -rf "$stub"
echo "android checks prepared in $out"

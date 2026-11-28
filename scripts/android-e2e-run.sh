#!/usr/bin/env bash
# The run half of `just e2e-android-native-run`, inside `nix develop .#android-emulator`:
# the call peer (bob in the call tests), the APK on the virtual phone, then the tests.
#
#   scripts/android-e2e-run.sh <test file> [test name...]
#
# All the host-side Rust of these runs (the call peer, the harness) is built in this one shell and into its own
# target tree (target/android-host): the desktop shell's tree has other environment variables, and builds
# from both in one tree rebuilt the host crates (aws-lc and 30 more, 80 to 130 s) every time the shell changed.
set -euo pipefail
cd "$(dirname "$0")/.."
file=${1:?test file}
shift
export ANDROID_SERIAL=${ANDROID_SERIAL:-emulator-5554}
pkg=dev.morpheus.mobile.debug # the tests' app; dev.morpheus.mobile is a person's own release app on a real phone and is never touched
case "$ANDROID_SERIAL" in
    emulator-*) ;;
    *) # a real phone: the guard adb first on PATH, and only a debug-suffixed build goes on it
        export PATH=$PWD/scripts/phone-guard:$PATH
        grep -q '"applicationId": *"dev.morpheus.mobile.debug"' "$(dirname "${MORPHEUS_APK:-apps/android/app/build/outputs/apk/debug/app-debug.apk}")/output-metadata.json" || { echo "refusing: the debug APK is not dev.morpheus.mobile.debug" >&2; exit 1; } ;;
esac
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target/android-host}

cargo build -q -p morpheus-ffi --example call_peer

# Install the APK unless this very one is on the phone already (a test-only change): `adb install` of the
# 190 MB library takes 5 to 15 s. The phone's copy is the file itself, so its size says whether it is the same.
apk=${MORPHEUS_APK:-apps/android/app/build/outputs/apk/debug/app-debug.apk} # a phone needs arm64: `just android-native-build-phone` copies its APK to target/phone-apk
stamp=target/.android-installed-$ANDROID_SERIAL
# The APK must carry the phone's ABI: x86_64 emulator builds and arm64 phone builds share apps/android/app/build/outputs, and the wrong one installs fine but never starts.
abi=$(adb shell getprop ro.product.cpu.abi | tr -d '\r')
libs=$(unzip -l "$apk")
if ! grep -q "lib/$abi/" <<<"$libs"; then
    echo "refusing: $apk has no lib/$abi (the phone $ANDROID_SERIAL runs $abi); build for it: just android-native-build (x86_64 emulator) or just android-native-build-phone (arm64 phone)" >&2
    exit 1
fi
sum=$(sha256sum "$apk" | cut -d' ' -f1)
installed=$(adb shell pm path $pkg 2>/dev/null | head -n 1 | cut -d: -f2 | tr -d "\r" || true)
remote_size=""
[ -z "$installed" ] || remote_size=$(adb shell stat -c %s "$installed" 2>/dev/null | tr -d '\r' || true)
if [ -n "$installed" ] && [ "$remote_size" = "$(stat -c %s "$apk")" ] && [ "$(cat "$stamp" 2>/dev/null)" = "$sum" ]; then
    echo "app: this APK is installed already"
else
    adb install -r "$apk" || { adb uninstall $pkg; adb install "$apk"; }
    echo "$sum" > "$stamp"
fi

# A stale APK must never pass as new: the app on the phone says what it was built from (its version name ends with
# `+<id>`, scripts/build-id.sh), and that must be what this tree builds.
want=$(scripts/build-id.sh)
have=$(adb shell dumpsys package $pkg | tr -d "\r" | sed -n "s/^ *versionName=//p" | head -n 1)
case "$have" in
    *"+$want") echo "app: built from $want" ;;
    *) echo "refusing: the app on $ANDROID_SERIAL was built from '$have', this tree is +$want: build it again (just android-native-build, or android-native-build-phone for a phone)" >&2; exit 1 ;;
esac

MORPHEUS_BUILD_ID=$want MORPHEUS_E2E_ANDROID_NATIVE=1 exec scripts/e2e-cargo.sh --test "$file" -- --nocapture --test-threads=1 "$@"

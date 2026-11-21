#!/usr/bin/env bash
# The native Android app as a release file: target/dist/morpheus-<version>-android-arm64.apk
#
# Built from apps/android (Kotlin + the Rust core over UniFFI): arm64 only, cargo release profile,
# R8 on. Gradle signs it with ~/.config/morpheus/release.keystore (the key of
# every release since 0.2.2; Android installs an update only over an app signed
# with the same key, so never make a new one). versionCode is computed from the
# workspace version in app/build.gradle.kts (0.2.7 -> 207).
# Run inside `nix develop .#android`. A CI job cannot sign, so this runs on the
# release machine and the file is uploaded by hand (docs/releasing.md).
set -euo pipefail
cd "$(dirname "$0")/.."

keys="$HOME/.config/morpheus"
[ -f "$keys/release.keystore" ] && [ -f "$keys/release.keystore.password" ] || {
    echo "no release key in $keys: restore it, do not make a new one" >&2
    exit 1
}

version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "morpheus-ffi") | .version')
workspace=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ "$version" = "$workspace" ] || { echo "morpheus-ffi $version is not the workspace version $workspace" >&2; exit 1; }

(cd apps/android && gradle assembleRelease -q -PrustAbis=arm64-v8a -PrustProfile=release)

apk=apps/android/app/build/outputs/apk/release/app-release.apk
[ -f "$apk" ] || { echo "$apk is missing (the build did not sign it)" >&2; exit 1; }
mkdir -p target/dist
out="target/dist/morpheus-$version-android-arm64.apk"
cp "$apk" "$out"

tools=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)
"$tools/apksigner" verify "$out"
"$tools/aapt2" dump badging "$out" | sed -n 1p
echo "certificate: $("$tools/apksigner" verify --print-certs "$out" | sed -n 's/^Signer #1 certificate SHA-256 digest: //p')"
echo "$out"

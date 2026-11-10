#!/usr/bin/env bash
# A signed release APK for phones (arm64): target/dist/morpheus-<version>-android-arm64.apk
#
# Signed with the release key in ~/.config/morpheus/ (made on first use).
# Keep that key: Android installs an update only over an app signed with
# the same key. Run inside `nix develop .#android`.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "morpheus-mobile") | .version')
app=target/dx/morpheus-mobile/release/android/app
keys="$HOME/.config/morpheus"
keystore="$keys/release.keystore"
password_file="$keys/release.keystore.password"

if [ ! -f "$keystore" ]; then
    mkdir -p "$keys"
    chmod 700 "$keys"
    head -c 24 /dev/urandom | base64 | tr -d '/+=' > "$password_file"
    chmod 600 "$password_file"
    keytool -genkeypair -keystore "$keystore" -alias morpheus -keyalg RSA -keysize 4096 \
        -validity 10000 -storepass:file "$password_file" -keypass:file "$password_file" \
        -dname "CN=Morpheus"
    chmod 600 "$keystore"
    echo "Made a release key: $keystore (back it up, with $password_file)"
fi

# dx reuses one gradle project per profile: keep only the phone's library.
rm -rf "$app/app/src/main/jniLibs/x86_64"
dx build --package morpheus-mobile --platform android --target aarch64-linux-android --release

# What the debug builds get from `just _android-extras`: our resources,
# Kotlin, manifest entries and libwebrtc's Java half.
cp -r apps/mobile/android/res/. "$app/app/src/main/res/"
mkdir -p "$app/app/src/main/kotlin" "$app/app/libs"
cp -r apps/mobile/android/kotlin/. "$app/app/src/main/kotlin/"
manifest="$app/app/src/main/AndroidManifest.xml"
sed -i '/<!-- morpheus:begin -->/,/<!-- morpheus:end -->/d' "$manifest"
sed -i '/<\/application>/e cat apps/mobile/android/manifest-application.xml' "$manifest"
cp "$(find target -name libwebrtc.jar -path '*android*' -print -quit)" "$app/app/libs/libwebrtc.jar"
# Keep the Java and Kotlin whole (see the file).
cp apps/mobile/android/morpheus.pro "$app/app/morpheus.pro"
grep -q 'libs/libwebrtc.jar' "$app/app/build.gradle.kts" ||
    sed -i 's|^dependencies {|dependencies {\n    implementation(files("libs/libwebrtc.jar"))|' "$app/app/build.gradle.kts"
# dx writes versionCode = 1 into every build. Android, F-Droid and
# IzzyOnDroid tell an update by it, so it comes from the version:
# 0.1.4 → 104 (0.1.3 and earlier went out as 1).
IFS=. read -r major minor patch <<<"$version"
code=$((major * 10000 + minor * 100 + patch))
sed -i "s/versionCode = [0-9]*/versionCode = $code/" "$app/app/build.gradle.kts"
(cd "$app" && ./gradlew assembleRelease -q)

unsigned=$(find "$app/app/build/outputs/apk/release" -name '*.apk' -print -quit)
mkdir -p target/dist
out="target/dist/morpheus-$version-android-arm64.apk"
tools=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)
"$tools/zipalign" -f -p 4 "$unsigned" "$out.aligned"
"$tools/apksigner" sign --ks "$keystore" --ks-pass "file:$password_file" --out "$out" "$out.aligned"
rm -f "$out.aligned" "$out.idsig"
"$tools/apksigner" verify "$out"
echo "$out"

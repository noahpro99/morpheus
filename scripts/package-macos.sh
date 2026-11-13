#!/usr/bin/env bash
# Builds dist/Morpheus.app from a release build and zips it as the updater
# expects: morpheus-<version>-macos-<arch>.zip holding Morpheus.app. Ad-hoc
# signed (not notarised: Apple silicon runs it, Gatekeeper asks once).
#
#   scripts/package-macos.sh aarch64-apple-darwin arm64
#   scripts/package-macos.sh x86_64-apple-darwin x86_64
# Prints the zip's path. Used by CI (apple.yml); needs macOS (sips, iconutil, codesign).
set -euo pipefail
cd "$(dirname "$0")/.."
triple=$1
arch=$2
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
exe=target/$triple/release/morpheus-desktop
[ -x "$exe" ] || { echo "build it first: cargo build --release -p morpheus-desktop --locked --target $triple" >&2; exit 1; }

app=dist/Morpheus.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$exe" "$app/Contents/MacOS/morpheus-desktop"

# The icon (apps/desktop/icons/icon.png, 1024 px) at every size macOS asks for.
set=$(mktemp -d)/Morpheus.iconset
mkdir -p "$set"
for size in 16 32 128 256 512; do
    sips -z $size $size apps/desktop/icons/icon.png --out "$set/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) apps/desktop/icons/icon.png --out "$set/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set" -o "$app/Contents/Resources/Morpheus.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Morpheus</string>
    <key>CFBundleDisplayName</key><string>Morpheus</string>
    <key>CFBundleExecutable</key><string>morpheus-desktop</string>
    <key>CFBundleIdentifier</key><string>dev.morpheus.desktop</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleIconFile</key><string>Morpheus</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.social-networking</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
    <!-- macOS ends an app that opens the microphone or camera without saying why. -->
    <key>NSMicrophoneUsageDescription</key><string>Morpheus uses the microphone in calls.</string>
    <key>NSCameraUsageDescription</key><string>Morpheus uses the camera in video calls.</string>
    <!-- matrix: links open in Morpheus (the app takes them from the system: main.rs on_open_urls). -->
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key><string>Matrix link</string>
            <key>CFBundleURLSchemes</key>
            <array><string>matrix</string></array>
        </dict>
    </array>
</dict>
</plist>
PLIST
plutil -lint "$app/Contents/Info.plist"

# Not notarised: ad-hoc signed so Apple silicon runs it at all.
codesign --force --deep --sign - "$app"
codesign --verify --deep "$app"
zip="dist/morpheus-$version-macos-$arch.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
echo "$zip"

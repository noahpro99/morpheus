#!/usr/bin/env bash
# Builds dist/Morpheus.app from a release build and zips it as the updater
# expects: morpheus-<version>-macos-<arch>.zip holding Morpheus.app. Ad-hoc
# signed (not notarised: Apple silicon runs it, Gatekeeper asks once).
#
#   scripts/package-macos.sh aarch64-apple-darwin arm64
#   scripts/package-macos.sh x86_64-apple-darwin x86_64
# Also writes the same name as .dmg (for people and brew cask; the updater uses the zip).
# Prints the zip's path. Used by CI (apple.yml); needs macOS (sips, iconutil, codesign).
set -euo pipefail
# Info-ZIP reads options from $ZIP and $ZIPOPT.
unset ZIP ZIPOPT
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

# A disk image for people: Morpheus.app on the left, an Applications shortcut on the
# right, an arrow between them (apps/desktop/assets/dmg-background.png, 600x400).
dmg="${zip%.zip}.dmg"
stage=$(mktemp -d)
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
mkdir "$stage/.background"
cp apps/desktop/assets/dmg-background.png "$stage/.background/background.png"
rm -f "$dmg" "$stage.rw.dmg"
hdiutil create -quiet -volname Morpheus -srcfolder "$stage" -fs HFS+ -format UDRW \
    -size "$(( $(du -sm "$stage" | cut -f1) + 30 ))m" "$stage.rw.dmg"
detach() { perl -e 'alarm shift; exec @ARGV' 30 hdiutil detach "$@"; }
detach /Volumes/Morpheus -quiet 2>/dev/null || true
hdiutil attach -quiet -readwrite -noverify -noautoopen "$stage.rw.dmg"
# Finder keeps the layout in the volume's .DS_Store. Best effort: without a Finder
# (a headless session) the image still works, just without the window layout.
# Finder can hang on a headless runner (an automation prompt nobody answers): 60 s at most.
perl -e 'alarm shift; exec @ARGV' 60 osascript <<'APPLESCRIPT' || echo "DMG window layout skipped (no Finder, or it took over 60 s)" >&2
tell application "Finder"
    tell disk "Morpheus"
        open
        set current view of container window to icon view
        set toolbar visible of container window to false
        set statusbar visible of container window to false
        set the bounds of container window to {200, 120, 800, 548}
        set opts to the icon view options of container window
        set arrangement of opts to not arranged
        set icon size of opts to 100
        set background picture of opts to file ".background:background.png"
        set position of item "Morpheus.app" of container window to {120, 190}
        set position of item "Applications" of container window to {480, 190}
        close
        open
        update without registering applications
        delay 2
        close
    end tell
end tell
APPLESCRIPT
# The icon beside the window's title: the app's own, flagged as the volume's custom icon.
# After the layout step, which removes both.
cp "$app/Contents/Resources/Morpheus.icns" /Volumes/Morpheus/.VolumeIcon.icns
SetFile -a C /Volumes/Morpheus || echo "DMG volume icon skipped (no SetFile)" >&2
sync
detach /Volumes/Morpheus -quiet || { sleep 3; detach /Volumes/Morpheus -force -quiet; }
hdiutil convert -quiet "$stage.rw.dmg" -format UDZO -o "$dmg"
rm -rf "$stage" "$stage.rw.dmg"

echo "$zip"

set shell := ["bash", "-euo", "pipefail", "-c"]

hs := env("MORPHEUS_HOMESERVER", "http://localhost:8008")

# List recipes
default:
    @just --list --unsorted

# ---------------------------------------------------------------- build & run

# Type-check everything (fastest feedback loop)
check:
    cargo check --workspace --all-targets

# Format all crates
fmt:
    cargo fmt --all

# Keep target/ to the builds in use (see scripts/prune-target.sh). The build
# recipes run this first; the dev shell runs it once a day.
sweep:
    ./scripts/prune-target.sh

# Clippy with warnings as errors
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Unit tests (live tests self-skip without a homeserver)
test: sweep
    cargo test --workspace

# End-to-end tests against the local homeserver (run `just hs-up hs-seed` first)
test-live:
    MORPHEUS_TEST_HOMESERVER={{hs}} cargo test -p morpheus-core --test live -- --nocapture

# Build the desktop app with the in-app test bridge
e2e-build: sweep
    dx build --package morpheus-desktop --features e2e

# Playwright-style tests through the real window (needs `just hs-up hs-seed`)
e2e: e2e-build hs-seed
    MORPHEUS_TEST_HOMESERVER={{hs}} cargo test -p morpheus-e2e -- --nocapture --test-threads=1

# One e2e test file with per-step timings, e.g. `just e2e-one perf` (no rebuild; run `just e2e-build` after UI changes).
# Reseeds first: the moderation tests kick and ban users and the seed puts them back.
e2e-one file: hs-seed
    MORPHEUS_TEST_HOMESERVER={{hs}} MORPHEUS_E2E_TIMINGS=1 cargo test -p morpheus-e2e --test {{file}} -- --nocapture --test-threads=1

# The web app (app.morpheuschat.org) at http://localhost:8080, rebuilt on save
web:
    nix develop .#web -c dx serve --package morpheus-web --platform web

# The web app for hosting: static files in target/dx/morpheus-web/release/web/public
web-release: sweep
    nix develop .#web -c dx build --package morpheus-web --platform web --release

# The web app with the page's test bridge
e2e-web-build: sweep
    nix develop .#web -c dx build --package morpheus-web --platform web --features e2e

# The e2e tests against the web app in a headless Chromium, e.g.
# `just e2e-web web` (one file) or `just e2e-web` (all of them)
e2e-web file="": e2e-web-build hs-seed
    nix develop .#web -c bash -c 'MORPHEUS_E2E_WEB=1 MORPHEUS_TEST_HOMESERVER={{hs}} cargo test -p morpheus-e2e {{ if file == "" { "" } else { "--test " + file } }} -- --nocapture --test-threads=1'

# Everything CI would run
ci: fmt lint test

# Run the desktop app with hot reload
desktop: sweep
    dx serve --package morpheus-desktop

# Run the desktop app as a named profile (separate data dir), e.g. `just desktop-as bob`
desktop-as profile: sweep
    MORPHEUS_DATA_DIR="$PWD/.dev-data/{{profile}}" dx serve --package morpheus-desktop

# Release build of the desktop app
desktop-release: sweep
    dx build --package morpheus-desktop --release

# Run on a plugged-in Android phone (USB debugging on; check with `adb devices`)
android: sweep
    rm -rf target/dx/morpheus-mobile/debug/android/app/app/src/main/jniLibs/x86_64
    nix develop .#android -c dx serve --package morpheus-mobile --platform android

# dx 0.7 writes its own placeholder launcher icon on every build and has no
# setting for extra sources: put the Morpheus icon, name and Kotlin (the
# UnifiedPush service, apps/mobile/android) in place and let gradle repackage.
_android-extras:
    cp -r apps/mobile/android/res/. target/dx/morpheus-mobile/debug/android/app/app/src/main/res/
    mkdir -p target/dx/morpheus-mobile/debug/android/app/app/src/main/kotlin
    cp -r apps/mobile/android/kotlin/. target/dx/morpheus-mobile/debug/android/app/app/src/main/kotlin/
    # Our manifest entries, replaced on every build (dx does not always rewrite the file).
    sed -i '/<!-- morpheus:begin -->/,/<!-- morpheus:end -->/d' target/dx/morpheus-mobile/debug/android/app/app/src/main/AndroidManifest.xml
    sed -i '/<\/application>/e cat apps/mobile/android/manifest-application.xml' target/dx/morpheus-mobile/debug/android/app/app/src/main/AndroidManifest.xml
    # Calls: libwebrtc's Java half (the native half is in libmain.so).
    mkdir -p target/dx/morpheus-mobile/debug/android/app/app/libs
    cp "$(find target -name libwebrtc.jar -path '*android*' -print -quit)" target/dx/morpheus-mobile/debug/android/app/app/libs/libwebrtc.jar
    grep -q 'libs/libwebrtc.jar' target/dx/morpheus-mobile/debug/android/app/app/build.gradle.kts || sed -i 's|^dependencies {|dependencies {\n    implementation(files("libs/libwebrtc.jar"))|' target/dx/morpheus-mobile/debug/android/app/app/build.gradle.kts
    # The native library stored, not deflated: packing hundreds of MB into
    # the APK was most of a phone build. Android maps it from the APK as is.
    sed -i 's|android:extractNativeLibs="true"|android:extractNativeLibs="false"|' target/dx/morpheus-mobile/debug/android/app/app/src/main/AndroidManifest.xml
    grep -q 'useLegacyPackaging' target/dx/morpheus-mobile/debug/android/app/app/build.gradle.kts || sed -i 's|^\( *\)packaging {|\1packaging {\n\1    jniLibs.useLegacyPackaging = false|' target/dx/morpheus-mobile/debug/android/app/app/build.gradle.kts
    cd target/dx/morpheus-mobile/debug/android/app && ./gradlew assembleDebug -q

# Update the app on a plugged-in phone, keeping its sign-in and data.
# With the emulator also running, name the phone: `ANDROID_SERIAL=<id> just android-update`
android-update: sweep
    rm -rf target/dx/morpheus-mobile/debug/android/app/app/src/main/jniLibs/x86_64
    nix develop .#android -c dx build --package morpheus-mobile --platform android --target aarch64-linux-android
    nix develop .#android -c just _android-extras
    nix develop .#android -c adb install -r target/dx/morpheus-mobile/debug/android/app/app/build/outputs/apk/debug/app-debug.apk
    nix develop .#android -c adb shell am start -n dev.morpheus.mobile/dev.dioxus.main.MainActivity

# Fresh install on a plugged-in phone: uninstall (wipes its sign-in and data), build for ARM, install, launch.
# With the emulator also running, name the phone: `ANDROID_SERIAL=<id from adb devices> just android-reinstall`
android-reinstall: sweep
    rm -rf target/dx/morpheus-mobile/debug/android/app/app/src/main/jniLibs/x86_64
    nix develop .#android -c dx build --package morpheus-mobile --platform android --target aarch64-linux-android
    nix develop .#android -c just _android-extras
    -nix develop .#android -c adb uninstall dev.morpheus.mobile
    nix develop .#android -c adb install target/dx/morpheus-mobile/debug/android/app/app/build/outputs/apk/debug/app-debug.apk
    nix develop .#android -c adb shell am start -n dev.morpheus.mobile/dev.dioxus.main.MainActivity

# Create the virtual phone (once; downloads the emulator and a system image, a few GB): a Pixel-sized Android 14 device in ~/.android/avd
android-avd:
    nix develop .#android-emulator -c bash -c 'echo no | avdmanager create avd --force -n morpheus -d pixel_7 -k "system-images;android-34;google_apis;x86_64"'
    # Software rendering and 2 GB made it crawl: give it RAM, cores and the GPU.
    sed -i 's/^hw.ramSize=.*/hw.ramSize=4G/; s/^hw.cpu.ncore=.*/hw.cpu.ncore=8/; s/^vm.heapSize=.*/vm.heapSize=512M/; s/^hw.gpu.enabled=.*/hw.gpu.enabled=yes/; s/^hw.gpu.mode=.*/hw.gpu.mode=host/; s/^hw.keyboard=.*/hw.keyboard=yes/' ~/.android/avd/morpheus.avd/config.ini

# Boot the virtual phone in its own window (KVM-accelerated); the app reaches the dev homeserver at http://10.0.2.2:8008
android-emulator:
    nix develop .#android-emulator -c env LD_LIBRARY_PATH=/run/opengl-driver/lib emulator -avd morpheus -gpu host -netdelay none -netspeed full

# Run on the virtual phone (start it with `just android-emulator` first)
android-virtual: sweep
    rm -rf target/dx/morpheus-mobile/debug/android/app/app/src/main/jniLibs/arm64-v8a
    nix develop .#android-emulator -c dx serve --package morpheus-mobile --platform android

# Build the mobile app with the test bridge for the virtual phone and install it (the emulator must be running)
e2e-android-build: sweep
    # dx reuses one gradle project per app: drop the phone's ARM library so it is not bundled too.
    rm -rf target/dx/morpheus-mobile/debug/android/app/app/src/main/jniLibs/arm64-v8a
    nix develop .#android-emulator -c dx build --package morpheus-mobile --platform android --target x86_64-linux-android --features e2e
    nix develop .#android-emulator -c just _android-extras
    # Installed over the old one (the tests clear its data themselves);
    # uninstalled first only when that fails (another build's signature).
    nix develop .#android-emulator -c bash -c 'apk=target/dx/morpheus-mobile/debug/android/app/app/build/outputs/apk/debug/app-debug.apk; adb -s "${ANDROID_SERIAL:-emulator-5554}" install -r "$apk" || { adb -s "${ANDROID_SERIAL:-emulator-5554}" uninstall dev.morpheus.mobile; adb -s "${ANDROID_SERIAL:-emulator-5554}" install "$apk"; }'

# Drive the app on the virtual phone (`just android-emulator` and `just e2e-android-build` first)
e2e-android: hs-seed
    nix develop .#android-emulator -c env MORPHEUS_E2E_ANDROID=1 ANDROID_SERIAL="${ANDROID_SERIAL:-emulator-5554}" cargo test -p morpheus-e2e --test android -- --nocapture --test-threads=1

# Push notifications end to end on the virtual phone: needs Sunup installed and opened once on it, and internet
e2e-android-push: hs-seed
    nix develop .#android-emulator -c env MORPHEUS_E2E_ANDROID=1 MORPHEUS_E2E_PUSH=1 ANDROID_SERIAL="${ANDROID_SERIAL:-emulator-5554}" MORPHEUS_TEST_HOMESERVER={{hs}} cargo test -p morpheus-e2e --test android push -- --nocapture

# The whole e2e suite with the phone standing in for the first app of each test (other users run on desktop)
e2e-android-all: hs-seed
    nix develop .#android-emulator -c env MORPHEUS_E2E_ANDROID=1 ANDROID_SERIAL="${ANDROID_SERIAL:-emulator-5554}" MORPHEUS_TEST_HOMESERVER={{hs}} cargo test -p morpheus-e2e --no-fail-fast -- --test-threads=1

# Type-check the mobile crate for Android without an emulator (needs `nix develop .#android`, or runs it)
android-check:
    nix develop .#android -c cargo check -p morpheus-mobile --target aarch64-linux-android

# Wipe local app data (sessions, caches) for all dev profiles
clean-data:
    rm -rf .dev-data
    rm -rf "${XDG_DATA_HOME:-$HOME/.local/share}/morpheus"

# ---------------------------------------------------------------- homeserver

# Start the local Synapse (first run generates config)
hs-up:
    dev/homeserver/up.sh

# Stop the local Synapse
hs-down:
    cd dev/homeserver && docker compose down

# Tail Synapse logs
hs-logs:
    cd dev/homeserver && docker compose logs -f

# Destroy the local Synapse and all its data
hs-reset: hs-down
    rm -rf dev/homeserver/data dev/homeserver/.seeded

# Create users alice/bob/carol and seeded rooms
hs-seed:
    dev/homeserver/seed.sh

# Register one user: `just hs-user dave secret [--admin]`
hs-user name password *flags:
    dev/homeserver/register.sh {{name}} {{password}} {{flags}}

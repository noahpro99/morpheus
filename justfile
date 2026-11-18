set shell := ["bash", "-euo", "pipefail", "-c"]

hs := env("MORPHEUS_HOMESERVER", "http://localhost:8008")

# Without full Xcode macOS has no `metal` compiler, so GPUI compiles its shaders at startup.
shaders := if os() == "macos" { "--features runtime-shaders" } else { "" }

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

# The UniFFI layer (crates/ffi) against the local homeserver
test-ffi:
    MORPHEUS_TEST_HOMESERVER={{hs}} cargo test -p morpheus-ffi --test live -- --nocapture

# The desktop app with the in-app test bridge, its own binary (morpheus-desktop-e2e, an
# [[bin]] of apps/desktop that needs the e2e feature): cargo writes
# target/debug/morpheus-desktop-e2e directly and never touches
# target/debug/morpheus-desktop, which you may be running (`cargo r`); that one has no
# bridge and opens on your screen.
e2e-build:
    @test -d "${MORPHEUS_E2E_DIR:-../morpheus-e2e}/bridge" || { echo "The test bridge is in the private repo noahpro99/morpheus-e2e; check it out at ../morpheus-e2e (or set MORPHEUS_E2E_DIR)." >&2; exit 1; }
    cargo build -p morpheus-desktop --bin morpheus-desktop-e2e --features e2e

# Needs `just hs-up hs-seed`. Checks that need a display are skipped and counted;
# `just e2e-display` runs them on the hidden Hyprland test monitor.
# Desktop e2e tests, headless: `just e2e` (all), `just e2e timeline` or `just e2e timeline pictures`
e2e file="" name="": (e2e-headless file name)

# Pixel checks need the session unlocked.
# Desktop e2e tests in real windows on the hidden Hyprland test monitor (pixel checks too)
e2e-display file="" name="": e2e-build hs-seed
    MORPHEUS_E2E_BIN=$PWD/target/debug/morpheus-desktop-e2e MORPHEUS_TEST_HOMESERVER={{hs}} scripts/e2e-cargo.sh {{ if file == "" { "" } else { "--test " + file } }} {{name}} -- --nocapture --test-threads=1 ${E2E_TEST_ARGS:-}

# The app runs on gpui's headless platform (offscreen windows: real layout and
# input, no pixels), so it works with the screen locked, in CI, and touches no
# window of the session. Checks that need a display (pixels, a window manager)
# are skipped loudly and counted at the end.
# Desktop e2e tests with no display at all: `just e2e-headless timeline [name]`
e2e-headless file="" name="": e2e-build hs-seed
    #!/usr/bin/env bash
    set -uo pipefail
    skips=target/e2e/display-skips.log
    mkdir -p target/e2e && rm -f "$skips"
    env -u WAYLAND_DISPLAY -u DISPLAY -u HYPRLAND_INSTANCE_SIGNATURE MORPHEUS_E2E_HEADLESS=1 \
        MORPHEUS_E2E_BIN=$PWD/target/debug/morpheus-desktop-e2e MORPHEUS_TEST_HOMESERVER={{hs}} \
        scripts/e2e-cargo.sh {{ if file == "" { "" } else { "--test " + file } }} {{name}} -- --nocapture --test-threads=1 ${E2E_TEST_ARGS:-}
    status=$?
    if [ -s "$skips" ]; then
        echo "headless: $(cut -f1 "$skips" | sort -u | wc -l) test(s) skipped a check that needs a display (run them with \`just e2e-display\`):"
        sort -u "$skips" | sed 's/^/  /; s/\t/: /'
    else
        echo "headless: no test skipped a check for want of a display"
    fi
    exit $status

# The old name of `just e2e`
gpui-e2e file="" name="": (e2e file name)

# One e2e test file with per-step timings, e.g. `just e2e-one perf` (rebuilds the test copy first).
# Reseeds first: the moderation tests kick and ban users and the seed puts them back.
e2e-one file: e2e-build hs-seed
    MORPHEUS_E2E_BIN=$PWD/target/debug/morpheus-desktop-e2e MORPHEUS_TEST_HOMESERVER={{hs}} MORPHEUS_E2E_TIMINGS=1 scripts/e2e-cargo.sh --test {{file}} -- --nocapture --test-threads=1

# Everything CI would run
ci: fmt lint test

# Run the desktop app
desktop: sweep
    cargo run -p morpheus-desktop {{shaders}}

# Run the desktop app as a named profile (separate data dir), e.g. `just desktop-as bob`
desktop-as profile: sweep
    MORPHEUS_DATA_DIR="$PWD/.dev-data/{{profile}}" cargo run -p morpheus-desktop {{shaders}}

# Release build of the desktop app
desktop-release: sweep
    cargo build --release -p morpheus-desktop

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

# The native Android app (apps/android): debug APK for the emulator (x86_64) in apps/android/app/build/outputs/apk/debug
android-native-build abi="x86_64": sweep
    nix develop .#android-emulator -c bash -c "cd apps/android && gradle assembleDebug -PrustAbis={{abi}}"

# Create the virtual phone (once; downloads the emulator and a system image, a few GB): a Pixel-sized Android 14 device in ~/.android/avd
android-avd:
    nix develop .#android-emulator -c bash -c 'echo no | avdmanager create avd --force -n morpheus -d pixel_7 -k "system-images;android-34;google_apis;x86_64"'
    # Software rendering and 2 GB made it crawl: give it RAM, cores and the GPU.
    sed -i 's/^hw.ramSize=.*/hw.ramSize=4G/; s/^hw.cpu.ncore=.*/hw.cpu.ncore=8/; s/^vm.heapSize=.*/vm.heapSize=512M/; s/^hw.gpu.enabled=.*/hw.gpu.enabled=yes/; s/^hw.gpu.mode=.*/hw.gpu.mode=host/; s/^hw.keyboard=.*/hw.keyboard=yes/' ~/.android/avd/morpheus.avd/config.ini

# Boot the virtual phone in its own window (KVM-accelerated); the app reaches the dev homeserver at http://10.0.2.2:8008
android-emulator:
    nix develop .#android-emulator -c env LD_LIBRARY_PATH=/run/opengl-driver/lib emulator -avd morpheus -gpu host -netdelay none -netspeed full

# The virtual phone without a window or sound (what the e2e runs use)
android-emulator-headless:
    nix develop .#android-emulator -c env LD_LIBRARY_PATH=/run/opengl-driver/lib emulator -avd morpheus -no-window -no-audio -no-boot-anim -no-snapshot-save -gpu swiftshader_indirect -netdelay none -netspeed full

# Drive the native app on the virtual phone (`just android-emulator-headless` first): builds, installs, runs
e2e-android-native file="android_native": hs-seed (android-native-build "x86_64")
    nix develop .#android-emulator -c bash -c 'adb -s "${ANDROID_SERIAL:-emulator-5554}" install -r apps/android/app/build/outputs/apk/debug/app-debug.apk'
    nix develop .#android-emulator -c env MORPHEUS_E2E_ANDROID_NATIVE=1 ANDROID_SERIAL="${ANDROID_SERIAL:-emulator-5554}" MORPHEUS_TEST_HOMESERVER={{hs}} scripts/e2e-cargo.sh --test {{file}} -- --nocapture --test-threads=1

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
    nix develop .#android-emulator -c env MORPHEUS_E2E_ANDROID=1 ANDROID_SERIAL="${ANDROID_SERIAL:-emulator-5554}" scripts/e2e-cargo.sh --test android -- --nocapture --test-threads=1

# Push notifications end to end on the virtual phone: needs Sunup installed and opened once on it, and internet
e2e-android-push: hs-seed
    nix develop .#android-emulator -c env MORPHEUS_E2E_ANDROID=1 MORPHEUS_E2E_PUSH=1 ANDROID_SERIAL="${ANDROID_SERIAL:-emulator-5554}" MORPHEUS_TEST_HOMESERVER={{hs}} scripts/e2e-cargo.sh --test android push -- --nocapture

# The whole e2e suite with the phone standing in for the first app of each test (other users run on desktop)
e2e-android-all: hs-seed
    nix develop .#android-emulator -c env MORPHEUS_E2E_ANDROID=1 ANDROID_SERIAL="${ANDROID_SERIAL:-emulator-5554}" MORPHEUS_TEST_HOMESERVER={{hs}} scripts/e2e-cargo.sh --no-fail-fast -- --test-threads=1

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

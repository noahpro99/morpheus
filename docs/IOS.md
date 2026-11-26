# Native iOS app

SwiftUI over the shared Rust core through UniFFI, the iPhone counterpart of `apps/android` (issues #520, #521). It will
replace the Dioxus phone build (`apps/mobile`, `ios.yml`).

There is no Mac on the dev machines, so **everything Apple-side runs in GitHub Actions** (`.github/workflows/ios-native.yml`).
This machine only writes the Swift and the tests, runs the Rust-only checks, and generates the bindings.

## Layout

```
crates/ffi              the UniFFI surface (shared with Android); iOS needs no extra Rust
scripts/ios-ffi.sh      cargo rustc --crate-type staticlib per iOS target, bindings, MorpheusFFI.xcframework
apps/ios/project.yml    XcodeGen spec (the .xcodeproj is generated, never checked in)
apps/ios/Morpheus       the app: AppModel (session, sidebar), ListsView, RoomView, SignInView, Theme
apps/ios/MorpheusUITests  XCUITests, each also runs performAccessibilityAudit()
apps/ios/rules          our own HIG source rules (rustc --test, like android_rules.rs)
apps/ios/ci/homeserver.py  Synapse (pip) for the job, seeded: alice, bob, a DM, "Test Server" / "general" with 60 messages
```

Swift stays thin: the lists, rows, grouping, palette and swipe rules are Rust (`view-model`). Listeners
(`SidebarListener`, `TimelineListener`) push whole snapshots; `AppModel` and `RoomModel` hold them.

## The loop

1. Write the XCUITest in `apps/ios/MorpheusUITests`, push, watch it fail: `gh run watch` / `gh run view --log-failed`.
2. Write the Swift, push again.
3. Locally, before pushing: `rustc --edition 2021 --test apps/ios/rules/ios_rules.rs -o /tmp/ios_rules && /tmp/ios_rules`
   (run from the repo root), and regenerate bindings with `target/uniffi-bindgen/debug/uniffi-bindgen generate --library
   <any libmorpheus_ffi> --language swift --out-dir DIR` to read the real API names.

CI jobs: `lint` (HIG rules, SwiftLint --strict), `test` (simulator boots and Synapse installs while Rust builds; XCFramework;
xcodegen; `xcodebuild test`), `device` (aarch64-apple-ios build and an unsigned `.ipa` artifact). Cargo is cached by
Swatinem/rust-cache, pip by actions/cache.

## Running on a phone

The `Morpheus-unsigned-ipa` artifact of a run is unsigned: Apple installs only signed apps. Sideload with AltStore or SideStore
(they sign with your own Apple ID), or open `apps/ios` on a Mac, run `scripts/ios-ffi.sh`, `xcodegen generate`, pick your team.

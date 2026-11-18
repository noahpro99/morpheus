# Native Android app

Kotlin + Jetpack Compose over the shared Rust core, through UniFFI. It
replaces the Dioxus phone app (`apps/mobile`, still in the tree until this one
has caught up). Issues: milestone "android-native" (#160-#166).

## Layout

```
crates/ffi            UniFFI surface (Rust): the one object the app makes
crates/view-model     the logic the screens share with the desktop app
apps/android          Gradle project (Kotlin, Compose)
  app/src/main        the app
  app/src/debug       the e2e test bridge (debug builds only)
  app/src/release     no-op stand-in for the debug hooks
scripts/android-ffi.sh  builds crates/ffi per ABI + generates the Kotlin bindings
```

Application ID `dev.morpheus.mobile` and the release key
`~/.config/morpheus/release.keystore` are the Dioxus app's, so installs update
in place. Version code is computed from the workspace version as in
`scripts/release-android.sh` (0.2.6 -> 206).

## Build

`nix develop .#android` (or `.#android-emulator`) has the SDK, NDK, JDK 17,
the Rust Android targets with their linkers, and `gradle` from nixpkgs. No
Gradle wrapper is checked in. Gradle's plugins and libraries come from Maven
(Google, Central).

`just android-native-build [abi]` runs `gradle assembleDebug`. The app's
`buildRust` task runs `scripts/android-ffi.sh`, which

1. `cargo build -p morpheus-ffi --target <triple> --profile android-dev` per ABI
   (`-PrustAbis=x86_64` for the emulator, `arm64-v8a` for a phone; both by
   default) and copies `libmorpheus_ffi.so` into `build/rust/jniLibs/<abi>`;
2. generates the Kotlin bindings from the library's own metadata
   (`uniffi-bindgen generate --library`, binary in `crates/ffi`, feature
   `bindgen`) into `build/rust/kotlin`.

Both are Gradle source directories, so the bindings always match the library.
Plain cargo with the shell's NDK linkers is used, not cargo-ndk: the shell
already exports what the C dependencies (aws-lc, sqlite) need.

Compose and AndroidX are pinned to versions that build against compileSdk 34,
the platform the nix SDK has (the emulator's system image follows the platform
list, so adding one means another multi-GB image).

## The FFI surface (crates/ffi)

Small and shaped around the screens, not around the core's API:

- `Morpheus(dataDir)`: `restoreSession()`, `login(homeserver, user, password)`,
  `logout()`, `account()`, `suspend()`, `watchSync(listener)`,
  `watchSidebar(listener)`, `openRoom(roomId, listener)`.
- `RoomTimeline`: `sendText`, `sendReply`, `toggleReaction`, `loadOlder`,
  `markRead`.
- `MediaHandle`: `key()`, `thumbnail(w, h)` (bytes; the core caches on disk).
- Records: `Sidebar`/`ScopeItem`/`SectionItem`/`RoomItem`, `TimelineRow`
  (`Message`, `Date`, `Start`, `New`), `MessageItem`, `MessageBody`.

The logic is in `morpheus-view-model` (`scopes`: which rooms a server, the
direct messages, the favourites show and under which headings; `rows`:
grouping a sender's messages under one header, day dividers; `diffs`:
applying the core's diffs), the same code the desktop app runs.

## Async streams -> Flows

Rust pushes: a screen passes a listener (UniFFI foreign trait) and gets a
`TaskHandle` back. Each update is a whole snapshot of what the screen lists
(the channel list, the timeline's rows), not a diff: the diffs from the core
are applied in Rust (`view_model::diffs`), so Kotlin never reconciles. A
snapshot is cheap at chat sizes, and Compose's `LazyColumn` keys by the
stable `key` on each row.

On the Kotlin side `Flows.kt` wraps a listener in a `callbackFlow` that starts
the Rust task when collected and calls `handle.cancel()` in `awaitClose`.
`suspend` calls (`login`, `sendText`, `thumbnail`) are plain UniFFI async
functions; the core runs its own Tokio runtime, so any executor can poll
them. Nothing polls from the UI; the one poll that exists (sync state) is in
Rust.

## Test bridge

Debug builds start `TestBridge` (`app/src/debug`): JSON lines over loopback
TCP on port 47100 (the Dioxus app's port too), reached with `adb forward`,
the same as the other phone tests. Compose has no DOM and no JS, so it reads
the semantics tree through the Compose root view (`RootForTest`, an internal
API: debug only) and injects input:

- `tree`: each node's text, test tag, screen bounds, clickable / editable /
  scrollable / focused / enabled;
- `tap`, `swipe`: real `MotionEvent`s sent to the window;
- `click` (by tag or text): the node's own click action;
- `type`: the field's SetText action;
- `key`, `back`, `state` (the app's `AppState`).

morpheus-e2e's `native_android` module is the client (`NativeApp`): it
installs nothing itself (`just android-native-build` does), clears the app's
data, starts it, forwards the port, and offers `wait_for_text`, `tap_tag`,
`type_into`, `screenshot` (`adb exec-out screencap`).

Screens tag what tests need with `Modifier.testTag`, and tags are also exposed
as resource ids (`testTagsAsResourceId`), so uiautomator sees them too.

## Running the tests

```sh
just android-emulator-headless
just android-native-build    # build + (install is done by the e2e launcher)
just e2e-android-native      # the tests in morpheus-e2e/tests/android_native.rs
just test-ffi                # the Rust side alone, against the local homeserver
```

## Not done yet

Push notifications (UnifiedPush), calls, threads, settings, media upload,
verification, a release APK path (signing is wired, `assembleRelease` is
untested), iOS.

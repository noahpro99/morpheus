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

## Running the tests (the emulator, headless)

```sh
just android-emulator-headless
just e2e-android-native                 # build (Gradle skips what did not change), install, run
just e2e-android-native-run android_native a_room_shows   # no build: reinstall and run one test
just test-ffi                # the Rust side alone, against the local homeserver
```

## Release build and the phone

`just android-native-phone` builds `assembleRelease` for `arm64-v8a` (R8 on,
signed with `~/.config/morpheus/release.keystore`) and installs it with `adb
install -r` on the plugged-in phone (never the emulator; `ANDROID_SERIAL` picks
one when several are attached). Release builds have no test bridge and no
cleartext traffic (the dev homeserver on `http://10.0.2.2` is debug only).

R8 was checked on the emulator (x86_64 release APK): the app starts, the Rust
library loads through JNA, and a sign-in attempt reaches matrix.org over TLS and
comes back as the core's own error sentence, so the keep rules in
`app/proguard-rules.pro` (JNA, the generated `uniffi` package) are enough.

## Done since

Verification and recovery (#241), message menu with edit and delete (#242), files (#243), composer suggestions, emoji panel and custom emoji (#244), UnifiedPush notifications (#245), threads (#246), the New screen (message someone, join by address, create a room, browse the directory, friends) and invites on the list (#247), Typing, members, profile cards and room details (#251), Settings (#249: Appearance with themes, a custom colour and shareable theme codes, language, default notifications, account; the look and language are kept in the app folder and the whole app draws with the chosen palette, see `Palette` in `ui/Theme.kt`). The e2e bridge has `attach` (stands in for the file picker), `push` and `notifications` (hand the push service a payload, read the system notifications back). Kotlin unit tests: `gradle testDebugUnitTest`.

## Not done yet

Open issues with the `android` label: showing who has read a message (the core does not expose receipts yet), link previews, settings and themes, encrypted account data, typing and members, calls, search. A release APK path (signing is wired, `assembleRelease` is untested), iOS.

## Notes from building it

- A test leaves the app on screen when it ends and starts from a clean slate
  (`pm clear`) at the next launch, so someone watching the emulator sees the
  last state.
- The tests make their own rooms (and Bob, a second core client, joins them):
  the seeded rooms keep growing and move between servers while other suites
  use the same homeserver.
- `tap` waits for the layout to stop moving (keyboard, list scrolling), and a
  swipe holds still before it lifts so the list does not fling.
- Messages are not `clickable` (that merges a message into one semantics
  node); they take taps with `pointerInput` and expose an `onClick` action.
- Listing older messages: the screen asks again whenever a request ends and it
  still needs more (a request made while one was out used to be dropped and the
  start of the room never showed).

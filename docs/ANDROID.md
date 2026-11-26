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
`scripts/release-android.sh` (0.2.6 -> 206; that script now builds this app for releases, see docs/releasing.md).

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
   (`uniffi-bindgen generate --library`; the generator is the small crate
   `tools/uniffi-bindgen`, built once into `target/uniffi-bindgen`, so the host
   is not built for it) into `build/rust/kotlin`.

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

- `tree`: each node's text, test tag, screen bounds, (the visible part: a row cut off by its list is reported and tapped where it shows, which was the cause of the old "rooms list / room not found" flake: the centre of a half-visible row lay under the verify banner and opened the Security screen), clickable / editable /
  scrollable / focused / enabled;
- `tap`, `swipe`: real `MotionEvent`s sent to the window;
- `click` (by tag or text): the node's own click action;
- `type`: the field's SetText action;
- `key`, `back`, `state` (the app's `AppState`).

morpheus-e2e's `native_android` module is the client (`NativeApp`): it
installs nothing itself (`just android-native-build` does; `just e2e-android-native-run`
installs an APK the phone does not have yet), starts the app, forwards the port, and offers `wait_for_text`, `tap_tag`,
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

What keeps the loop short:

- The host-side Rust of these runs (the harness, the call peer) is built in the
  `android-emulator` shell into `target/android-host`, apart from the desktop
  shell's `target/`; the two shells' environments differ, and sharing one tree
  rebuilt aws-lc and 30 more crates each time the shell changed.
- `dev/homeserver/ensure-seeded.sh` runs the seed only when the seed script
  changed or the server lacks alice and the seeded rooms; `MORPHEUS_FORCE_SEED=1`
  (or `just hs-seed`) seeds anyway.
- `NativeApp::launch_signed_in("alice", "alice-password")` signs in through the
  screen once per test process; later calls restart the app, which restores the
  session, and drop what a test can leave of the app's own state (look,
  language, remembered tab and chat). A test about sign-in, a clean install,
  accounts or the store uses `NativeApp::launch()` (clears all data), after
  which the next `launch_signed_in` signs in again.
- Waits poll the bridge every 50 ms; `tap` waits for the node to stand still and
  `scroll_list_by` for the screen to stop moving, instead of fixed sleeps.

## Release build and the phone

`just android-native-phone` builds `assembleRelease` for `arm64-v8a` (R8 on,
signed with `~/.config/morpheus/release.keystore`) and installs it with `adb
install -r` on the plugged-in phone (never the emulator; `ANDROID_SERIAL` picks
one when several are attached). Release builds have no test bridge and no
cleartext traffic (the dev homeserver on `http://10.0.2.2` is debug only).

R8 is checked by `just e2e-android-native-release` (#264): it builds the x86_64 release APK signed with the debug key (`-PreleaseSigning=debug`, so the release keystore is never used), installs it on the emulator and runs `morpheus-e2e/tests/android_release.rs` through adb and uiautomator (the release build has no bridge, but its test tags are resource ids): not debuggable, the sign-in screen shows, and a sign-in to a server that is not there is answered by the Rust core, so the library loaded past R8. The keep rules in `app/proguard-rules.pro` (JNA, the generated `uniffi` package, WebRTC and its `jni_zero` helper, native methods) are what that needs; without the jni_zero rule the app died in `JNI_OnLoad`.

## Done since

Verification and recovery (#241), message menu with edit and delete (#242), files (#243), composer suggestions, emoji panel and custom emoji (#244), UnifiedPush notifications (#245), threads (#246), the New screen (message someone, join by address, create a room, browse the directory, friends) and invites on the list (#247), Synced layout (#250: the DM list follows the account's pinned and closed marks, pin and close from a long press, the servers' order is the account's; the sealed account data is read through the core as on the desktop), links that open in the browser and link cards (#248; the card test needs `dev/homeserver/spare.sh` and `just hs=http://localhost:8010 e2e-android-native-run android_native a_link_gets_a_card`, it skips on the dev homeserver), search in a room with a jump to the result, "Mark read" on a list, the NEW divider and a Jump to unread button (#253), typing, members, profile cards and room details (#251), Calls (#252: crates/call and the core's MatrixRTC signalling run in crates/ffi `calls`, which joins a call as one Rust task and pushes `CallSnapshot`s to a `CallListener`: who is in it, who speaks, mute and deafen, cards from `view_model::calls::tiles` (people, cameras, shared screens with a Watch card until watched, #191's protocol so a sharer's encoder stays paused with no watchers), the timer from the memberships; pictures are JPEGs fetched with `CallHandle.frame`; the voice channel's row lists who is in it; `CallModel.kt` keeps the call, the speaker route and `CallService`, a microphone foreground service with an ongoing CallStyle notification whose Hang up ends the call; WebRTC's Java half `libwebrtc.jar` is copied out by `scripts/android-ffi.sh`, `MorpheusApp` loads the library with `System.loadLibrary` so `JNI_OnLoad` gives WebRTC the VM, `crates/ffi/build.rs` exports its JNI symbols; debug builds send a tone, not the microphone; the test peer is `crates/ffi/examples/call_peer.rs`, built by `just call-peer-build` and run by the e2e test as a child process; the dev homeserver's call services are reached with `adb reverse` for 8080, 7880 and 7881). Mutes (#259: a long press on any room offers Mute with the desktop's lengths or Unmute; `mute_room`/`unmute_room` set the notification mode and the muted-until mark, a mute that runs out is ended by the sidebar watcher as on the desktop, and a layout change made here refreshes the list at once). Ignoring (#260: a person's card offers Ignore or Unignore, kept as the desktop keeps it; their messages fold into "Ignored message · Show" in every room). Read receipts (#254: the core's `Message.read_by` carries the SDK's receipts, `view_model::rows::read_receipts` moves a receipt that sits on the reader's own event back to the last message of someone else, and a message shows "Seen by bob"). The phone's camera in a call (#255: `CameraFeed.kt`, Camera2, is the core's `CameraDriver` (`call_set_camera_driver`) and hands each YUV picture to `call_camera_frame`; a registered platform camera wins over the test pattern, so debug builds send the emulator's virtual scene; the front camera first, a flip button when the phone has both sides, the permission asked at the first use, the camera stops when the activity stops unless in picture-in-picture (the app has none yet) and comes back with the app; `call_peer` prints `frame <user> WxH seq=N` for video it receives, and the e2e test checks the frames, `dumpsys media.camera` and the background stop; the emulator's AVD has no front camera, so the flip itself is only unit-tested in `CameraChoiceTest`). Incoming calls (#256, strings from names #263: the typing line (`view_model::typing`), Replying to, file and push texts take their names through `tf`/`t` placeholders; #256: `watch_rings` in crates/ffi `rings.rs` turns the core's `ring_updates` into `RingListener` calls and ends a ring when its time is up, it is declined, the caller leaves or this account joins; `RingModel` keeps them, a card on screen (`RingCard`: Answer, Decline) and `Ringer` when the app is away: a CallStyle notification with a full-screen intent that shows over the lock screen, the ringtone, Answer opening the app into the call and Decline sent by a receiver; a push for a call (app closed) rings the same way, with Decline restoring the saved session first. Calls on other homeservers' SFUs: the call loop listens on each one people use (`foci_to_subscribe`, `Call::subscribe`) as the desktop does; there is no federation in the dev setup, so this is checked by compiling and by the call test still passing. H.264 from desktops: webrtc-sys's decoder factory already adds Android's MediaCodec factory (hardware only) before the software ones, so a phone with a hardware H.264 decoder uses it and one without simply does not advertise H.264; the emulator has none, which is why the e2e peer sends VP8 (`MORPHEUS_HW_VIDEO=0`); not checked on a real phone. The e2e peer takes `MORPHEUS_PEER_USER` and `callroom <id>`.) Server folders (#262: `view_model::rail_folders` reads and changes the account's `rail` JSON in the desktop's format (`{"folder", "name", "spaces"}`; unit tests), ffi `rail_rows`, `move_server_to_folder`, `move_server_out_of_folder`, `rename_folder`, `set_rail_order` now takes servers and `folder:<id>` keys and keeps folders; the Servers screen shows folders with Rename, Up, Down, and each server a folder button (New folder or an existing one) or Take out. Its menu and rename field are drawn in the tree, not as popup windows, because the e2e bridge reads only the tree; the phone's rail stays flat). Settings (#249: Appearance with themes, a custom colour and shareable theme codes, language, default notifications, account; the look and language are kept in the app folder and the whole app draws with the chosen palette, see `Palette` in `ui/Theme.kt`). The e2e bridge has `attach` (stands in for the file picker), `push` and `notifications` (hand the push service a payload, read the system notifications back). Bridges (#265, the desktop's #107: Settings > Bridges lists the bridge bots the server has (usual names, ones typed in and saved in `bridge_bots.json`, ones bridged rooms name), Connect shows the bot's QR image, its questions are answered on the screen, Log out and Check status; the conversation with each bot is one Rust task, ffi `bridges.rs` `BridgeHandle`, pushing `BridgeSnapshot`s, with the login steps in `view_model::bridges`; e2e `android_bridges` runs the fake bot from the harness on the host). Kotlin unit tests: `gradle testDebugUnitTest`.

## Not done yet

Open issues with the `android` label: calls left over (ringing for a voice channel: only direct messages ring, as on the desktop). Call volumes (#261): the call's settings button shows output and microphone volume and where the sound goes (loudspeaker, earpiece, a headset), a person's speaker button their volume and "Mute for me"; kept in the desktop's files (`audio_devices.json`, `person-volumes.json`) by crates/ffi `volumes.rs`. A phone has no input list: a headset carries the microphone too. Not done: iOS.

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

## Look and navigation (#274)

`ui/Theme.kt` holds the tokens (`Space`, `Radius`, `Size`, `Type`, and `Palette`, whose colours come from the core's themes through ffi, with `onColour()` = `theme::on_colour` for text on any fill); `ui/Components.kt` the shared pieces (`UnreadBadge`, `MentionPill`, `IconAction`, `TopBar`, `SectionHeader`, `EmptyState`, `SkeletonRows`). New screens use these, not literal sizes or colours.

The signed-in home has a bottom bar with three tabs (`AppModel.tab`): Servers (the rail and the open server's channels; tags `rail`, `rooms-list`), Direct messages (`tab-dms`, the same list for the DMs scope) and You (`tab-profile`: account, status, Settings, Security, Sign out). A room has no bar: it is a pager (`room-pager`), the chat in the middle, the rail and channels (`pane-channels`) one swipe to the right, the members (`members-pane`) one swipe to the left; the back gesture returns to the chat first, then leaves the room; from Direct messages or You it goes to Servers. Tests: `android_native` `a_bottom_bar_...` and `in_a_chat_a_swipe_...`.

## Launch (#309, #313, #317)

A launch with a saved session draws the last state before the session is back: the channel list (ffi `sidebar_cache`), the tab, the server, and the open room with its last 40 rows (ffi `timeline_cache`, one room, no pictures); the live rows replace them. `Root` shows both the restoring and the signed-in state through one branch, so a room and what is typed in its composer stay as they are when the session arrives.

Start-up, measured on the emulator as the debug bridge reports it (`state`: `first_list_ms`, `first_room_ms`, `session_ms`, and `marks`, named points in ms from the process start; the e2e `a_relaunch_shows_where_you_left_off...` prints them): the Rust core (the first UniFFI call loads JNA and checks every binding) and the camera driver start on a thread of their own from `Application.onCreate`, `MainActivity` makes the model when the first frame composes instead of in `onCreate`, the list of the last run is the channel list's first value (no skeleton frame), and the push registration waits for the first frame. First list: 802 ms, then about 580 ms. About 220 ms of that is the system starting the process before `Application.onCreate`; the rest is the activity and the first composition (about 100 ms).

# Morpheus — plan

Goal: a fast, beautiful, Discord-style Matrix client. Native Rust everywhere,
one shared UI, separate desktop and mobile apps.

## Stack decisions

| Layer          | Choice                             | Why                                                                 |
| -------------- | ---------------------------------- | ------------------------------------------------------------------- |
| Matrix         | matrix-rust-sdk + matrix-sdk-ui    | Production-grade (Element X, Fractal). Timeline + sliding sync free. |
| UI             | Dioxus 0.7, webview renderer       | One component model for desktop and mobile; CSS for polish.         |
| Future UI      | Dioxus Native (Blitz)              | Same components, GPU rendering. Flip when text input/IME are solid.  |
| Async          | Tokio runtime owned by the core    | Core works under any executor; UI never touches the SDK directly.    |
| Dev homeserver | Synapse in Docker                  | Reference implementation, native simplified sliding sync.            |
| Env            | Nix flake + direnv + just          | Reproducible on NixOS; `just` for the muscle-memory commands.        |

## Architecture

```
apps/desktop ─┐                 ┌─ Session ── room_list() ── VectorDiff<RoomSummary>
apps/mobile  ─┼─ morpheus-ui ───┤
              │   (Dioxus)      └─ Session ── timeline(id) ── VectorDiff<TimelineEntry>
              └─ AppConfig                                      + TimelineHandle (send, paginate)
                                 morpheus-core (tokio runtime, matrix-sdk)
```

Rules that keep it clean:

1. **Core has zero UI deps.** It exposes owned, `Clone + PartialEq` value types
   (`RoomSummary`, `TimelineEntry`) and diff streams. Nothing from
   `matrix_sdk` leaks past `morpheus_core`'s public API except id types.
2. **One update shape.** Everything live is `Vec<VectorDiff<T>>` applied to an
   `imbl::Vector<T>` inside a Dioxus signal. Room list and timeline share the
   consumer code path.
3. **Core futures are executor-agnostic.** Work runs on the core's Tokio
   runtime; callers only await join handles.
4. **Apps are thin.** A platform crate chooses the renderer, finds the data
   directory, and passes `AppConfig`. No components live there.
5. **Separate crates, shared components.** Mobile-specific layouts go in
   `crates/ui` behind a layout switch, not in a fork.

## Phases

### 0. Scaffold — done
Workspace, flake, dev Synapse, seeding, live test, login → room list →
timeline → send.

### 1. Read and write well
- [x] Avatars (room, user) via the SDK media API (SDK disk cache + in-memory data-URL cache)
- [x] Image messages rendered inline as thumbnails
- [x] Video/file messages, downloads, full-size image viewer
- [x] Scroll-triggered back-pagination (button remains as a fallback)
- [x] Local time zones: `format.rs` uses `jiff` with the system zone (/etc/localtime on Linux, the device setting on Android); times were shown in UTC before.
- [ ] Virtualised timeline once rooms with >2k loaded items appear
- [x] Auto-fill short histories: when the first sync hands over fewer rows than fit on screen there is nothing to scroll, so older messages need the "Load older messages" button. Paginating automatically races the room subscription's first response (which can reset the timeline); needs the SDK to settle first.
- [x] Error toasts for failed actions (bottom-right, click to dismiss); dialogs keep inline validation

### 2. The Discord shell
- [x] Spaces rail filters the room list; DMs and Rooms get their own rail buttons
- [x] DMs section; unread (bold) and mention badges
- [x] Members panel grouped by power level (presence pending); header toggle hides it
- [x] Typing indicators (debounced notices, names resolved)
- [x] "New messages" marker
- [x] Read receipts sent when a room is viewed
- [x] Room header actions: a room-options button opens the room menu (edit, notifications, invite, leave…); pinned; search: the header magnifier opens a panel over the SDK's local index (decrypted messages included, only what this device has loaded) and a result jumps to and highlights the message, paging back if needed.)
- [x] Desktop notifications via the SDK latest-events stream (push actions included); mentions flagged; inbox lists recent mentions
- [x] Ctrl+K room switcher (follows the room into its space scope)
- [x] Profile modal (sender, member, own footer chip); Escape closes every modal
- [x] Styled right-click menus for messages, rooms, members; native menu suppressed
- [x] Message menu: emoji picker, view reactions, remove my reactions, forward, pin/unpin (+ pinned panel in the header), mark unread, report, copy text/link/ID, delete
- [x] Member menu: profile, mention, message (DM), invite to room, block/unblock, roles, kick, ban, copy ID (gated by power level)
- [x] Room menu: mark as read, invite people, notification mode (all / mentions / mute), edit name and topic, create room (in the scoped space), copy link/ID, leave
- [x] Account menu on the footer chip: view/edit profile (name, avatar), presence status, switch account (signs out), sign out
- [x] Composer focuses on navigation; member list windowed to 100 rows with show-more
- [x] Custom title bar (no native decorations): back/forward through visited rooms, scope name, help, inbox of unread rooms, window controls and drag
- [x] Keyboard shortcuts after Discord (Ctrl+/ lists them); Escape closes any modal from anywhere
- [x] Lucide icons in every menu and header button; consistent corner-radius scale; bottom band (user pill + message box)
- [x] Server menu (name click, badge right-click): mark read, invite, mute/notifications, hide muted, settings (name, topic, icon), privacy (join rule, history), per-server profile, create channel/category, move to folder, leave, copy info
- [x] Rail: drag to reorder, folders (drop one badge on another), rename/ungroup; layout synced via account data
- [x] Profile card popover (mutual rooms, note, role, message box, mod view, more menu) with the full modal behind "View full profile"
- [x] Friends: a private list in account data (`dev.morpheus.friends`); "Add friend"/"Remove friend" in a person's menu; a Friends page at the top of the DMs lists them (avatar, name or nickname, status) with Message and Remove. No friend requests (Matrix has no such thing).
- [ ] Mod view page (Discord's): a per-user moderation sheet across rooms
- [x] Multi-account: each account in its own folder (the first in the data dir, more under `accounts/<n>`); "Switch account" lists them and swaps without signing in, "Add an account" keeps the current one (the login screen offers a way back); signing out moves to the next. Core live test + e2e.
- [x] Invites: sidebar section, chat-column card, inbox block, rail badge; accept joins, decline leaves
- [x] Per-user nicknames: "Set nickname…" in a person's menu; kept in private account data (`dev.morpheus.nicknames`), shown instead of display names on messages and in the member list. Also DM room names (list and header) and the profile card (with their own name beneath). Not yet in mentions.
- [x] Timeline pinned to the newest message on open, "Jump to present" button
- [x] Skeleton placeholders while loading and for messages still waiting on keys
- [x] SSO login (browser hand-off) alongside password and email sign-in
- [x] Timing: slow core ops logged, e2e driver timings (`MORPHEUS_E2E_TIMINGS=1`)
- [x] Alt+↑/↓ room navigation (and the rest of Discord's set, Ctrl+/)

### 3. Rich content
- [x] Markdown composer, sanitised HTML rendering (ruma-html)
- [x] Reactions (view and toggle)
- [x] Replies (with quoted previews), edits, deletes
- [x] Threads
- [x] Emoji panel, `:code` emoji and `@name` mention autocomplete in the composer
- [x] Attachment upload by button, drag-and-drop (hint overlay) and paste

### 4. Encryption UX
- [x] Device verification between own devices (emoji SAS) from the settings modal
- [x] Restore from key backup with a recovery key (settings modal)
- [ ] QR verification. (Done: recovery setup for accounts without a backup, Settings → Security & Privacy → Set up recovery, which shows the new key once; core live test + e2e.)
- [x] Tokens and the sqlite store passphrase live in the OS keyring (Secret Service / Keychain / Credential Manager), falling back to an encrypted file under the data dir when no keyring answers (`crates/core/src/session_store.rs`); an old plaintext `session.json` is migrated in and deleted. `MORPHEUS_SESSION_STORE=keyring|file` forces one; tests use the file.
- [x] Shields on messages: in encrypted rooms a message whose sender couldn't be confirmed (unsigned device, identity changed, …) shows a red or grey shield with the reason as its tooltip (lenient mode, as Element). Device list shows verified/unverified.

### 5. Mobile
- [x] Android app-private data dir via `ndk-context` + JNI (`Context.getFilesDir()`), iOS Documents; type-checks for `aarch64-linux-android` (`just android-check`), not yet exercised on a device
- [x] Android emulator e2e: `just android-emulator` (headless works too), `just e2e-android-build`, `just e2e-android`; the driver reaches the app's bridge (fixed port 47100 on Android) through `adb forward` and screenshots with `screencap`. Also runs on a plugged-in phone (`ANDROID_SERIAL`).
- [x] Installed and launched on a Pixel 8 Pro (`just android`)
- [x] Mobile file picker: on phones the attach button opens a file input from the tap (Android shows its own picker), and picked files go the way pasted ones do. Phone e2e test.
- [x] Single-column navigation on phone-width windows (rooms → chat → members/thread) with back buttons and Escape/back; the shared shell does it, so the mobile app gets every desktop feature as is. Panes stack and slide with the finger (see below).
- [x] Larger touch targets and safe-area insets in the narrow layout
- [ ] Text selection is broken (reported on the phone, 2026-10-15); revisit later. Touch screens currently turn off selection outside typing fields so press-and-hold can open menus; messages need a "Select text" path instead.
- [ ] Swipe down to close a bottom sheet: a first version is in (sheet follows the finger, closes past a third or on a flick; phone e2e test), but the Pixel build predates it; check it feels right on the device.
- [x] Pane swipes like Discord: the chat slides in over the room list; a rightward drag moves the top pane with the finger, revealing the one underneath (members → chat → list); a leftward drag on the list brings the last chat back, and on the chat pulls in the parked member list. Phone e2e tests hold a real finger mid-drag. Opening a thread by swipe is not done (which thread would it be?).
- [x] Keyboard avoidance on Android: the page shrinks for the keyboard and the message box and newest message stay above it (phone e2e test). iOS still to verify.
- [x] UnifiedPush notifications (Sunup, ntfy…; tap opens the room). They show sender and text: a running app hands over what it decrypted; with the app closed, the push service decrypts the message itself through the core (SDK notification client, via JNI). Phone e2e test covers both.
- [ ] iOS build on a Mac runner

### 5b. Calls (MatrixRTC, Element Call compatible signalling)
- [x] Signalling in the core: the homeserver names a LiveKit token service (MSC4143 `rtc/transports`, else `org.matrix.msc4143.rtc_foci` in the well-known); an OpenID token buys a LiveKit token (`/get_token`, falling back to `/sfu/get`); each device posts `org.matrix.msc3401.call.member` (Element Call's state key and focus shape). Live tests.
- [x] A device that vanishes drops out: an MSC4140 delayed leave, restarted every 4 s while the app runs, fires 10 s after it stops. Live test.
- [x] Members may join calls: rooms Morpheus creates allow the membership at power level 0; an admin joining turns it on in older rooms; others are told why not.
- [x] Media is native, not the webview (nixpkgs WebKitGTK has no WebRTC): `crates/call` wraps the LiveKit Rust SDK with WebRTC's own audio devices (echo cancellation, noise suppression). Tests send a tone (`MORPHEUS_CALL_AUDIO=tone`) and measure what arrives. Needs clang 21+ for libwebrtc (dev shell sets it).
- [x] UI: phone button in the header joins (or leaves); people in a call are listed under the room in the sidebar with a ring while speaking, and clicking the list joins; voice panel above the user pill (state, room, hang up); mute and deafen in the user pill. Join/leave events stay out of the timeline. E2E test.
- [x] Dev services: LiveKit (dev mode) and Element's lk-jwt-service beside Synapse (`just hs-up`); Synapse gets a TLS federation listener (self-signed) for the token service's OpenID check, delayed events and the well-known focus.
- [x] Android: WebRTC set up in `JNI_OnLoad` (its Java classes are only found from a Java thread), `libwebrtc.jar` bundled by `_android-extras`, its JNI functions exported by the app's build script (webrtc-sys's own link arguments never reach the app), RECORD_AUDIO asked for at the first call. Phone runs reverse-forward the token service and LiveKit ports. Both call e2e tests pass with the phone as alice and desktop as bob (encrypted call included).
- [x] Media encryption in encrypted rooms, Element Call's way: each device gets a fresh random key, shares it as an (encrypted) `io.element.call.encryption_keys` room event, again whenever someone joins; LiveKit frame encryption (AES-GCM, per-participant keys, LiveKit's default derivation). Keys that arrive unencrypted are ignored. Tests: media (right key hears, wrong key hears nothing), core (key arrives decrypted), e2e (two apps in #encrypted hear each other). Interop with Element Call itself still to try (it may have moved to to-device keys).
- [x] Hanging up sends the scheduled leave at once (one request), so an app killed mid-hang-up leaves no ghost; the seed sweeps leftovers.
- [x] Microphone and speaker pickers in Settings → Voice & Video (kept per device, a call going switches over; phones note that the system picks).
- [ ] Android: speakerphone vs earpiece toggle (AudioManager), since WebRTC leaves routing to the system.
- [x] Chimes, Discord-style: you or someone else joins or leaves the call, mute, deafen (tones made in code, played by the webview; GStreamer plugins in the dev shell for webkitgtk).
- [ ] Android: check the chimes play (WebView may want a user gesture: `setMediaPlaybackRequiresUserGesture(false)`).
- [ ] Video and screen share; ringing for DMs (MSC4075 `m.rtc.notification`).

### 6. Native renderer
- [ ] Evaluate Dioxus Native (Blitz) on desktop; gate behind a cargo feature
- [ ] Measure cold start, scroll FPS, memory against the webview build

### 7. Always-on desktop presence
- [x] Tray icon (logo, Open / Quit menu, left click shows the window); closing the window hides it; syncing and OS notifications continue on a desktop-side thread (the webview's runtime stalls while hidden) and the UI catches up on reopen.
- [x] Tray follow-ups: Settings → Startup (desktop) opens Morpheus at sign-in through an XDG autostart entry, optionally hidden in the tray (`--hidden`); one Morpheus per data dir on Unix (a second launch shows the running window and exits). Autostart on macOS/Windows not done.

## Funding model (idea, not scheduled)
- Feature bounties instead of (or alongside) plain donations: anyone can put
  money behind a feature request. Once a maintainer approves the request the
  backer pays part of it (half, say) up front and the rest when the feature
  ships. Open questions: escrow/payment provider, refunds if a feature is
  declined or abandoned, how backers vote on priority, whether to still accept
  untied donations.

## Testing strategy

- **Unit tests** in `crates/core` and `crates/ui` for pure logic (grouping, formatting, mapping SDK items).
- **Live tests** (`crates/core/tests/live.rs`) against the seeded local Synapse: login, sliding sync, timeline back-fill, send and echo. These are the regression suite for the core.
- **End-to-end UI tests** (`crates/e2e`, `just e2e`): a Playwright-style driver launches the real desktop binary built with the `e2e` feature, which exposes a loopback socket that evaluates JavaScript in the webview. Tests log in, switch rooms, send messages, verify session restore, and can screenshot the window.
- **Manual multi-user** with `just desktop-as alice` and `just desktop-as bob` side by side.
- **Flakes**: the scroll-to-newest and Esc-jump tests used to depend on the test window being short enough for the first page to overflow (a tiled window's size follows the developer's layout); they now load history until the timeline scrolls. Still watched: the NEW-divider test when the suite runs back to back, and the two-device verification (app B has died on "They match" a few times; never with logs kept). Follow-up: isolate runs with fresh users per run (admin API) instead of the shared alice/bob/carol, and rerun failed files once before reporting.
- Later: pixel-diff the e2e screenshots against golden images, and a `cargo test` harness that drives a `Core` without a homeserver via `matrix-sdk-test`.

## Open questions
- Dioxus webview on NixOS/Wayland needs `WEBKIT_DISABLE_DMABUF_RENDERER=1` (set in the flake). Revisit when webkitgtk fixes land.
- Sliding sync requires a homeserver with MSC4186. Synapse and Conduit-family servers have it; document the fallback story for others.
- dioxus-desktop frees an eval's result slot when the JS channel object is garbage-collected, which can race the Rust-side read and surface as `EvalError::Finished`. The e2e bridge pins recent channel objects to work around it; worth an upstream issue.

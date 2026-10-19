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
- [ ] Proper local time zone handling (replace `format.rs` shims with `jiff`)
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
- [ ] Room header actions: search, pinned, settings
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
- [ ] Friends list: Matrix has no friend relationship; could map to a "favourite people" list in account data
- [ ] Mod view page (Discord's): a per-user moderation sheet across rooms
- [x] Multi-account: each account in its own folder (the first in the data dir, more under `accounts/<n>`); "Switch account" lists them and swaps without signing in, "Add an account" keeps the current one (the login screen offers a way back); signing out moves to the next. Core live test + e2e.
- [x] Invites: sidebar section, chat-column card, inbox block, rail badge; accept joins, decline leaves
- [ ] Per-user nicknames: Discord's friend nicknames have no Matrix equivalent; a local-only alias store could stand in
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
- [ ] QR verification, recovery key setup for new accounts
- [x] Tokens and the sqlite store passphrase live in the OS keyring (Secret Service / Keychain / Credential Manager), falling back to an encrypted file under the data dir when no keyring answers (`crates/core/src/session_store.rs`); an old plaintext `session.json` is migrated in and deleted. `MORPHEUS_SESSION_STORE=keyring|file` forces one; tests use the file.
- [ ] Shields on messages, device list

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
- [ ] Keyboard avoidance on Android (adjustResize) and iOS, verified on devices
- [x] UnifiedPush notifications (Sunup, ntfy…; tap opens the room). Message text in notifications needs background decryption: not yet.
- [ ] iOS build on a Mac runner

### 6. Native renderer
- [ ] Evaluate Dioxus Native (Blitz) on desktop; gate behind a cargo feature
- [ ] Measure cold start, scroll FPS, memory against the webview build

### 7. Always-on desktop presence
- [x] Tray icon (logo, Open / Quit menu, left click shows the window); closing the window hides it; syncing and OS notifications continue on a desktop-side thread (the webview's runtime stalls while hidden) and the UI catches up on reopen.
- [ ] Tray follow-ups: autostart at login, a "start hidden" flag, a single-instance guard so a second launch focuses the running one.

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

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
- [ ] Auto-fill short histories: when the first sync hands over fewer rows than fit on screen there is nothing to scroll, so older messages need the "Load older messages" button. Paginating automatically races the room subscription's first response (which can reset the timeline); needs the SDK to settle first.
- [ ] Error toasts instead of inline `<p class="error">`

### 2. The Discord shell
- [x] Spaces rail filters the room list; DMs and Rooms get their own rail buttons
- [x] DMs section; unread (bold) and mention badges
- [x] Members panel grouped by power level (presence pending); header toggle hides it
- [x] Typing indicators (debounced notices, names resolved)
- [x] "New messages" marker
- [x] Read receipts sent when a room is viewed
- [ ] Room header actions: search, pinned, settings
- [ ] Desktop notifications
- [x] Ctrl+K room switcher (follows the room into its space scope)
- [x] Profile modal (sender, member, own footer chip); Escape closes every modal
- [x] Styled right-click menus for messages, rooms, members; native menu suppressed
- [x] Timeline pinned to the newest message on open, "Jump to present" button
- [x] Skeleton placeholders while loading and for messages still waiting on keys
- [x] SSO login (browser hand-off) alongside password and email sign-in
- [x] Timing: slow core ops logged, e2e driver timings (`MORPHEUS_E2E_TIMINGS=1`)
- [ ] Alt+↑/↓ room navigation

### 3. Rich content
- [x] Markdown composer, sanitised HTML rendering (ruma-html)
- [x] Reactions (view and toggle)
- [x] Replies (with quoted previews), edits, deletes
- [x] Threads
- [ ] Emoji picker, mentions autocomplete
- [ ] Attachment upload with drag-and-drop and paste (button upload works)

### 4. Encryption UX
- [x] Device verification between own devices (emoji SAS) from the settings modal
- [x] Restore from key backup with a recovery key (settings modal)
- [ ] QR verification, recovery key setup for new accounts
- [ ] Store passphrase and tokens in the OS keyring (desktop) instead of `session.json`
- [ ] Shields on messages, device list

### 5. Mobile
- [ ] Android app-private data dir via `ndk-context` + JNI; iOS equivalent
- [ ] Single-column navigation (rooms → chat → members) with back gestures
- [ ] Touch targets, safe areas, keyboard avoidance
- [ ] UnifiedPush notifications
- [ ] iOS build on a Mac runner

### 6. Native renderer
- [ ] Evaluate Dioxus Native (Blitz) on desktop; gate behind a cargo feature
- [ ] Measure cold start, scroll FPS, memory against the webview build

### 7. Always-on desktop presence
- [ ] Tray icon with a background process that keeps syncing and raises desktop
      notifications while the window is closed; click to reopen, menu for quick
      status and quit. Needs a single-instance guard so a second launch focuses
      the running one.

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
- Later: pixel-diff the e2e screenshots against golden images, and a `cargo test` harness that drives a `Core` without a homeserver via `matrix-sdk-test`.

## Open questions
- Dioxus webview on NixOS/Wayland needs `WEBKIT_DISABLE_DMABUF_RENDERER=1` (set in the flake). Revisit when webkitgtk fixes land.
- Sliding sync requires a homeserver with MSC4186. Synapse and Conduit-family servers have it; document the fallback story for others.
- dioxus-desktop frees an eval's result slot when the JS channel object is garbage-collected, which can race the Rust-side read and surface as `EvalError::Finished`. The e2e bridge pins recent channel objects to work around it; worth an upstream issue.

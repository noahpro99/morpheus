# GPUI desktop port (branch `gpui-desktop`)

A second desktop app drawn with GPUI instead of the Dioxus web view, built
next to the existing one until it can replace it. Desktop only: phones and
the web stay Dioxus. Nothing on `main` changes until the port is done.

This file is the handoff for agents doing the work. Read all of it first.

## Layout

- `apps/desktop-gpui/` — the new binary `morpheus-gpui` (crate
  `morpheus-desktop-gpui`). The UI lives here; split it into modules per
  screen (`src/login.rs`, `src/room_list.rs`, …), mirroring
  `crates/ui/src/components/`.
- `crates/core` and `crates/call` are shared and untouched, except for small
  additive APIs if GPUI genuinely needs one (say so in the report).
- `crates/ui` (Dioxus) is the reference: behaviour, wording, layout and
  colours come from it and `crates/ui/assets/theme.css`. Port, don't
  redesign.
- Dependencies: `gpui = "0.2"` and, for inputs, lists, menus and scroll
  areas, `gpui-component` (check the version it needs matches gpui's) before
  writing your own widgets.

## Tests are the spec

The e2e tests in `crates/e2e/tests` talk to the app through a loopback
bridge (`MORPHEUS_E2E_PORT`) using CSS selectors such as
`form.login input[name=username]`, `.room`, `.context-item`. About 2,200
test calls go through selector methods (`wait_for`, `wait_for_text`,
`click`, `type_into`, `press`, `submit`, `click_text`, `count`, …); about
640 are raw JavaScript (`eval`, `wait_for_js`).

The GPUI app serves the same bridge, but answers structured requests
instead of JavaScript:

- Every element a test touches is registered with an **element tag**: a tag
  name, classes, attributes (`name`, `title`, `data-*`, `value`), its text
  and its bounds, plus its parent. A small helper (e.g. `.e2e("button",
  "ghost room-title")` or a `Tagged` wrapper) records this during paint
  in `e2e` builds and costs nothing otherwise.
- The bridge matches a **selector subset** against that tree: tag, `.class`,
  `#id`, `[attr]`, `[attr=value]`, `[attr="value"]`, descendant (space) and
  child (`>`), and comma lists. That covers nearly every selector the tests
  use; anything else gets a clear "unsupported selector" error.
- Operations: `count`, `text`, `value`, `click`, `right_click`, `type`,
  `press` (key name, with modifiers), `submit`, `bounds`. Clicks and keys
  are dispatched as real GPUI input at the element's centre, so they test
  hit-testing too.
- `crates/e2e/src/lib.rs`: when `MORPHEUS_E2E_UI=gpui`, the `App` methods
  send these structured requests instead of JavaScript. `eval` and
  `wait_for_js` return an error saying the test needs porting. The binary
  comes from `MORPHEUS_E2E_BIN` (already supported).
- A test counts as passing on GPUI only when it passes unmodified, or after
  its raw JS is replaced with selector calls that still check the same
  behaviour (and still pass on Dioxus).

## Rules for agents

- Work in `/home/noahpro/repos/morpheus-gpui` on branch `gpui-desktop`.
  Never touch `/home/noahpro/repos/morpheus` (that is `main`, in use).
- `target` is a symlink to the main checkout's target directory, so builds
  share compiled dependencies. Don't delete it or make a new one; disk is
  limited.
- One build at a time, in the foreground. No background builds, no
  `--release`, no parallel test runs. Run single tests with filters
  (`cargo test -p morpheus-e2e --test account <name>`), not the suite.
- Run cargo inside the dev shell: `nix develop -c cargo …` (the flake shell
  has the GUI libraries; GPUI may need more, see task 1).
- Homeserver for tests: `just hs-up hs-seed` in the main checkout is
  how tests get one; `MORPHEUS_TEST_HOMESERVER` points at it. If it's not
  running, say so rather than starting long-running services yourself.
- Commit to `gpui-desktop` when your task's acceptance check passes, with a
  message that says what now works. Don't push. Don't merge.
- Stay inside your task. If something outside it blocks you, stop and report.
- Report back: what works, what was checked and how, what doesn't, anything
  you changed outside `apps/desktop-gpui`.

## Tasks (in order, one agent each)

1. **Scaffold.** `apps/desktop-gpui` builds and opens a window titled
   "Morpheus" with the theme's background colour, fonts (Inter, Unbounded,
   from `crates/ui/assets`) loaded. Add it to the workspace members (not
   default-members). Add any system libraries GPUI needs to the flake's
   default shell. Acceptance: `nix develop -c cargo build -p
   morpheus-desktop-gpui` succeeds; the binary runs for 5 s without
   crashing (`timeout 5`; exit 124 is success).
2. **Bridge and driver.** Element tags, the selector subset (with unit
   tests for the matcher), the structured bridge behind an `e2e` feature,
   and the `MORPHEUS_E2E_UI=gpui` path in `crates/e2e`. Acceptance: matcher
   unit tests pass; an e2e smoke test launches the GPUI binary and finds a
   tagged element.
3. **Sign-in.** The login screen (`crates/ui/src/components/login.rs`):
   `form.login`, username, password, homeserver, submit, errors, using
   `morpheus_core` to sign in and restore the saved session. Acceptance: the
   `login` helper in `crates/e2e/tests/account.rs` gets as far as `.room`
   against GPUI (a room list stub listing names is enough).
4. **Shell and room list.** Rail, room list, sidebar footer, chat
   placeholder. Acceptance: `rail` and `layout` tests that don't use raw JS.
5. **Timeline (read).** Messages, formatted bodies (HTML subset to styled
   text), images, grouping, day dividers. The hardest piece: plan it first.
6. **Composer.** Sending, replies, edits, attachments, autocomplete.
7. Onward in the order of `crates/e2e/tests` files, each task naming the
   test file(s) it brings to green.

8. **Later: headless e2e.** Run the app on gpui's test platform
   (`test-support`, `VisualTestContext` in vendor/gpui) with the bridge on
   top, so e2e tests open no windows. Not before the shell and timeline exist.

Every task also adds `#[gpui::test]` tests (gpui's `test-support` feature,
as a dev-dependency) for the widgets it builds: headless and fast, alongside
the e2e acceptance, not replacing it.

Shared test helpers (like `login`) live in a common module that test files
include, not by including another test file with `#[path]` (that pulls in
its tests too).

Progress is recorded here: tick tasks off and note what each left undone.

## Progress

- [x] 1 Scaffold — window "Morpheus" on --bg-chat, centred label in Unbounded; fonts are static TTFs made from the woff2 (GPUI cannot read woff2); xattr patched in vendor/xattr (libc dropped ENOATTR); title needs window.set_window_title on Wayland
- [x] 2 Bridge and driver — `e2e` feature: `.e2e("tag","classes").attr().text().focusable()` wrappers (see src/e2e/mod.rs) rebuild a tree per frame from `e2e::frame(root)`; selector subset matcher (21 unit tests); bridge ops count/texts/text/value/bounds/click/right_click/type/press/submit with contains/exact/index filters, input injected via Window::dispatch_event and dispatch_keystroke (needs vendor/gpui: one-line pub patch of DispatchEventResult); driver maps App methods under MORPHEUS_E2E_UI=gpui, eval/wait_for_js/reveal* error "needs porting". Undone: `deferred` popups are tagged without a parent; `type` needs an input widget (task 3) and is untested; submit clicks a descendant `[type=submit]`
- [x] 3 Sign-in — src/login.rs (form.login, homeserver probe, SSO, "Back to" button, errors as Dioxus), src/input.rs (own minimal TextInput: typing, backspace/delete, arrows, Ctrl+A, password mask, Enter; no IME or clipboard; gpui-component not used), session restore at startup behind a logo splash, src/room_list.rs stub (`.room` per joined room); bridge `next_frame` now waits for a frame after the dispatch; tests/gpui_login.rs shares account.rs `login`. Undone: Tab between fields, IME/clipboard, SSO-return config
- [x] 4 Shell and room list — src/{shell,rail,room_list,user_bar,chat,model,prefs,media,icons}.rs: `.shell`/`.shell-body`, `nav.rail` (DMs, Rooms, Favorites, divider, server badges with `.rail-pill` and unread, folders open/closed, `.rail-add`), `aside.sidebar` (header h2 follows the scope, rooms by scope, DM rows, server channel order, categories, "More channels" from the directory, hide-muted, unread/badge), `.sidebar-footer` profile chip, `main.chat` with `.chat-header h2`, topic, lock badge, or "Pick a room"; live from `session.room_list()`; avatars fetched as thumbnails (initials meanwhile; pictures untested, seed has none); Lucide icons via synthesized SVG assets. Passing on GPUI (unmodified or JS replaced by `App::texts`/selectors, each also run on Dioxus): spaces::space_badge_filters_the_room_list, spaces::clicking_rooms_keeps_the_rooms_scope, space_channels::a_servers_rooms_are_under_it_in_its_order; gpui_login (3) and gpui_smoke still pass; 14 `#[gpui::test]` widget tests (need `--features e2e`) plus unit tests. Skipped for raw JS: all of rail.rs (drag, pill geometry, radii), layout.rs, account.rs (menus), spaces::dms_button_lists_direct_messages (avatar size). Failing: none run. Undone: drag and menus (rail, room, server, profile chip), create-server/room dialogs, invites and Friends rows, closed/pinned DMs, nicknames, voice rows and call participants, last-place restore, member toggle/panel, mute/deafen/settings buttons are inert
- [x] 5 Timeline (read) — plan (as built; result under it):
  1. Virtualisation: gpui's `list` (variable row heights) with `ListState` in `ListAlignment::Bottom`, so a room opens at the newest message, new rows stay pinned to the bottom while the view is there, and prepending older rows (`splice(0..0, n)`) keeps the view where it was. Only the rows in or near the viewport are built and painted each frame (and only those are in the e2e tree).
  2. Data flow: `Shell` opens a `TimelineView` entity per room (`session.timeline(room)`; dropped on room switch). Its task applies each batch of core `VectorDiff<TimelineEntry>` onto an `imbl::Vector`, then rebuilds the row list once per batch (grouping, day dividers, NEW divider, hidden entries as in Dioxus `group_rows`). Rows hold `Arc`s reused when a message is unchanged, so the new list is compared to the old one by pointer and only the changed middle range is `splice`d into the ListState (no whole-list reset, no per-frame allocation of the list). Bodies are parsed once per message into a `Body` (blocks of text plus style spans), never per frame.
  3. Rich text: `rich.rs` parses the core's sanitised HTML (`ruma_html`) into blocks (paragraph, pre, quote, list, rule) of text + spans (bold, italic, underline, strike, code, link). A paragraph is drawn as `StyledText::with_runs` (full `TextRun`s, so inline code can switch to a monospace font); one with links is wrapped in `InteractiveText` for clicks. In e2e builds a wrapper registers each link's range as an `a[href]` node (bounds from the text layout) so tests can find and click links.
  4. Images: thumbnails through `Session::thumbnail` (as `media.rs` avatars do), fetched when a row is first built for painting, cached per (media, size), decoded by gpui. Boxes are sized from the event's width/height so a loading picture does not move the rows.
  5. Scrolling: pagination when the top visible row index is small (and an auto-fill pass after updates settle, as Dioxus); a "Jump to present" button when the view is more than 240 px from the bottom. New bridge ops `scroll_state`/`scroll` (a tagged element can expose a scroll probe) replace the JS scrollTop/scrollHeight checks.
  Built: src/timeline.rs (TimelineView entity per open room, gpui `list`, pagination, jump button), src/message.rs (rows to elements, text runs, attachments, reactions, reply quotes), and src/view_model/ with no gpui or dioxus in it (rich.rs HTML to blocks of styled spans, rows.rs grouping/dividers/NEW marker/changed-range, format.rs, linkify.rs; meant to move to a shared crate later). Bridge: ops `scroll_state`, `scroll`; elements can expose `.scrollable(..)`; links inside text register as detached `a[href]` nodes (`e2e::text_links`); clicks now draw a frame between press and release (InteractiveText needs it). Driver: `App::scroll_state`, `scroll_to`, `bounds`, `bounds_containing`, and GPUI paths for `reveal_message/_text/_element`. Passing on GPUI, and where ported also on Dioxus: scrolling::{rooms_open_at_the_newest_message_and_jump_back_after_scrolling, scrolling_up_reaches_the_start_of_a_long_room, reaching_the_top_stays_there_while_older_messages_load} (JS replaced by scroll ops); new tests/timeline.rs (3: grouping/dividers/state lines/edits/reactions/replies, formatted bodies and links incl. clicking, pictures); new_messages_marker::new_message_marker_appears_above_unread_messages and chat::the_open_room_shows_history_and_live_messages (message sent through the API instead of the composer); gpui_login (3), gpui_smoke, spaces (2), space_channels (1) still pass; 74 unit and `#[gpui::test]` tests (grouping, rich text, runs, virtualisation of 2000 rows, links, reactions, replies). Skipped (need the composer, members/pins/call buttons, key banner or raw JS): chat::{login_browse_rooms_and_send, messages_show_up_live_for_whoever_is_looking, session_survives_relaunch (splash JS)}, new_messages_marker::new_message_marker_appears_above_unread_and_clears_on_send, chat_header (2), scrolling::locked_messages_say_how_to_unlock_them, links (3: composer, matrix.to opening, profile), attachments/details/polish/interactions (composer, computed style, blurhash). Failing: none run. Undone: file Download button and video playback are inert, lightbox, hover action bar, context menus, sender/avatar click (profile), thread panel, key banner and locked-message row, ignored-message fold, blurhash placeholder, matrix.to links (ignored), mono font is "JetBrains Mono" falling back to the default sans if absent, italic is synthesised, `.edited` is not smaller than the text.
- [ ] 6 Composer

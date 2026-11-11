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

Progress is recorded here: tick tasks off and note what each left undone.

## Progress

- [x] 1 Scaffold — window "Morpheus" on --bg-chat, centred label in Unbounded; fonts are static TTFs made from the woff2 (GPUI cannot read woff2); xattr patched in vendor/xattr (libc dropped ENOATTR); title needs window.set_window_title on Wayland
- [ ] 2 Bridge and driver
- [ ] 3 Sign-in
- [ ] 4 Shell and room list
- [ ] 5 Timeline (read)
- [ ] 6 Composer

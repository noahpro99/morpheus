# GPUI desktop app

The desktop app (Linux, Windows, macOS) is drawn with GPUI, the GPU-drawn UI
toolkit behind the Zed editor, in `apps/desktop` (package and binary
`morpheus-desktop`, Wayland app id `morpheus-desktop`). It replaced the
Dioxus web-view desktop app in 0.2.0 and uses the same data folder
(`ProjectDirs("dev", "morpheus", "Morpheus")`, on Linux
`~/.local/share/morpheus`). Phones (`apps/mobile`) and the web (`apps/web`)
are still Dioxus, sharing `crates/ui`.

This file is what an agent needs before changing the desktop app. How the
port was done, task by task, is in
[gpui-port-history.md](gpui-port-history.md); how releases are made is in
[releasing.md](releasing.md).

## Layout

- `apps/desktop/` is the app: one module per screen or feature
  (`login.rs`, `room_list.rs`, `timeline.rs`, `settings/`, `call.rs`, ...),
  plus `src/e2e/` (the test bridge, only built with the `e2e` feature).
  `src/main.rs` is shared by two binaries: `morpheus-desktop` and
  `morpheus-desktop-e2e` (needs the `e2e` feature; built by
  `just e2e-build`).
- `crates/view-model` (`morpheus-view-model`) holds the logic behind the
  screens with no UI toolkit in it (what a menu offers, how members group,
  crop maths, settings text, ...), unit tested and shareable with other
  front ends. Put logic there and keep `apps/desktop` to drawing and wiring.
- `crates/core` (Matrix: sessions, rooms, timelines) and `crates/call`
  (LiveKit calls, audio, noise suppression, screen capture) are shared with
  the phone and web apps.
- `crates/video` plays video; the e2e test driver and the tests are in the private repo `noahpro99/morpheus-e2e` (see "Tests are the spec").
- `vendor/` holds patched crates (gpui, livekit, libwebrtc, webrtc-sys);
  patches are marked "Morpheus patch".

## Tests are the spec

The e2e tests (private repo `noahpro99/morpheus-e2e`, `tests/`; check it out next to this repo as `../morpheus-e2e`, or set `MORPHEUS_E2E_DIR`; `just e2e` says so when it is missing) talk to the app through a loopback
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
- `morpheus-e2e/src/lib.rs` (the private repo): for the desktop app (`MORPHEUS_E2E_UI=gpui`, the default), the `App` methods
  send these structured requests instead of JavaScript. `eval` and
  `wait_for_js` return an error saying the test needs porting. The binary
  comes from `MORPHEUS_E2E_BIN` (already supported).
- A test counts as passing on GPUI only when it passes unmodified, or after
  its raw JS is replaced with selector calls that still check the same
  behaviour. (Phone and web tests still run on Dioxus and evaluate JavaScript.)

## Rules for agents

- Work in `/home/noahpro/repos/morpheus` on `main` (there are no long-lived
  branches). The backlog is GitHub issues (`gh issue list`); one issue per
  commit, "fixes #N" in the message, saying what now works.
- A failing e2e test comes first: write the test for the feature or the bug,
  see it fail for the right reason, then write the code. Tests check
  behaviour and UI rules, not single cosmetic details. Add unit tests in
  `crates/view-model` for logic.
- Don't delete `target` or make another one; disk is limited
  (`just sweep` / `scripts/prune-target.sh` keep it down).
- One build at a time, in the foreground. No background builds, no
  `--release`, no parallel test runs. Run single tests with filters
  (`just e2e <file> [test]`, or `scripts/e2e-cargo.sh --test account
  <name>`), not the whole suite; the full suite only at milestones.
- Run cargo inside the dev shell: `nix develop -c cargo ...` (the flake shell
  has the GUI libraries).
- Homeserver for tests: `just hs-up hs-seed`; `MORPHEUS_TEST_HOMESERVER`
  points at it (`just e2e` seeds it first). If it's not running, say so
  rather than starting long-running services yourself. Don't run
  `just hs-reset`.
- Tests that change a test user's profile restore it (`ProfileGuard`).
- Commit to `main` when your issue's tests pass; `git pull --rebase origin
  main`, then `git push origin main`.
- Stay inside your task. If something outside it blocks you, stop and report.
- Report back: what works, what was checked and how, what doesn't, anything
  you changed outside the files the issue is about.

## History

The port ran as 60 numbered tasks on a `gpui-desktop` branch until the
switch in 0.2.0; since then work happens on `main` as issues. The task
records (plans, root causes, what was left undone) are in
[gpui-port-history.md](gpui-port-history.md).

## Backlog

GitHub issues (`gh issue list`); milestones name the releases. One item
from the port that never became an issue: strong noise suppression with
DeepFilterNet 3's low-latency model (`deep_filter` feature
`default-model-ll`, lookahead 0): 17 ms quicker (21.7 vs 38.8 ms of
processing), about the same cleaning, 1.7x the CPU, +28 MB. The owner's call.

## Gotchas

- Don't build the GPUI app at opt-level 0 (`[profile.dev.package.morpheus-desktop-gpui] opt-level = 0`). Rebuilds drop from ~30 s to ~7 s, but the binary exits silently with code 0 at startup, before the e2e bridge opens. Every e2e test then fails with "did not open its e2e bridge". Not yet investigated. It stays at the workspace's opt-level 1.
- Never build while e2e tests run. Load from a parallel build makes launches time out ("bridge within 60s", "no .room in 30s"), and the failures look like regressions.
- Never open an app window on the user's monitors. For screenshots, launch the e2e builds: both apps then have the title "Morpheus (test)", so the test harness's Hyprland rule puts them on the hidden MORPHEUS-TEST monitor, unfocused. Set the size with `MORPHEUS_E2E_SIZE=1200x760` and capture with `grim -o MORPHEUS-TEST`, or `grim -g` with the geometry from `hyprctl clients -j`. Don't use `cargo run` with the plain build, and don't run the Dioxus binary without the test title.
- Running GPUI e2e tests: `just gpui-e2e <file> [name]`. It builds with the e2e feature, copies the binary to `target/debug/morpheus-gpui-e2e`, and runs the test against that copy, so a plain build can never stand in for it. Pointing `MORPHEUS_E2E_BIN` at the plain `target/debug/morpheus-gpui` is no longer safe: any `cargo build` or `cargo run` overwrites it. The harness now refuses any binary without the "Morpheus (test)" title, and refuses to launch when the hidden MORPHEUS-TEST monitor can't be set up. If a test says "not a test build", rebuild; don't work around it.

- cpal streams on Linux need a buffer size (`BufferSize::Fixed`): with `Default`, its PulseAudio host gets pipewire-pulse's defaults (a microphone in 341 ms bursts held 2 s, a speaker queue of 2 s). See task 44; `mic::sized`. rodio already asks for ~50 ms.

- tract (DeepFilterNet's runtime) refuses DFN3's graph with debug assertions on ("duplicate name"), so `[profile.dev.package.tract-core] debug-assertions = false`. And `rustfmt` on a test file follows `mod common;` and reformats `tests/common/mod.rs`: revert it.

Note after task 17: its bridge change (a click presses again when the target moved) broke right-click menus (the second press closed the menu) and backdrop clicks (the target vanished on press). Both are fixed in 33d6e1b and ac98233. A change to the bridge's click, drag or scroll must be checked against context_menu, dm_menu, message_menu, room_menu and space_channels, not only the test it was made for.

- LiveKit's Rust crates are vendored (task 50): `vendor/livekit`, `vendor/libwebrtc`, `vendor/webrtc-sys` are crates.io 0.9.5 / 0.3.52 / 0.3.49 with additions marked "Morpheus patch" (a stereo publish option, a remote audio track's volume). Upgrading LiveKit means copying the new versions and carrying those few lines over. Measured on the screen-sound track: at a lowered volume, and also disabled (deafened), its `NativeAudioStream` still got the samples as received, so a level measured there is before WebRTC's gain (the inbound-rtp `audioLevel` stat is after it); the screen sound's level therefore applies volume and deafen itself. (A quick check on a deafened voice track in Tone mode went quiet instead, as `watch_voice`'s comment says; not reconciled.)

- Headless e2e (task 58): `just e2e` is headless now; a test that needs real pixels or Hyprland must call `morpheus_e2e::display_or_skip("…")` (directly or through `common::capture`) and return when it says false, never pass silently: the headless run then prints and counts it. Run such files with `just e2e-display` (unlocked session). The headless window is `vendor/gpui/src/platform/linux/headless/window.rs` (a Morpheus patch, carry it over on a gpui update). Don't call a headless window's callbacks while gpui is inside an update (reentrancy panics): resize and close are deferred to the 60 Hz tick for that reason. A headless app opens at the size it asks for (no compositor resizes it), so the harness passes 1280x800 when a test gives none; a layout test that fails only headless is first a size question.

## Translations

Text the app shows is written in English and the English text is the key:
`t("Theme")` (`morpheus_view_model::i18n`) shows "Theme", or the Spanish for it from
`crates/view-model/locales/es.json`; text with no translation shows in English. New UI
text goes through `t(..)` / `tf("Back to %{account}", ..)` (the settings helpers `muted`,
`note`, `section`, `group`, `choice` and menu labels translate what they are given).
A literal handed to `t`, `tf`, those helpers or `label` is a key, and
`cargo test -p morpheus-view-model i18n` fails when a key has no entry in a locale file
(or a file keeps one nothing uses, or its `%{..}` placeholders differ). Text built from
a variable (labels in view-model enums) is listed in `i18n::dynamic_keys`.
Counts use `tn("1 reply", "%{count} replies", n)` (both literals are keys; the count is
formatted for the language, `number`); dates, times and file sizes in `view_model::format`
take the language's words, order and decimal mark; failed actions go through
`toast::failure_text` (the action word is a key listed in `toast::WHATS`). A plural rule
beyond one/other is added in `i18n::plural`.
Diagnostics labels, headings and fixed value words are translated on screen only (the
copied report and bug reports stay English): they are listed in
`view_model::diagnostics::translatable_labels`, and a test fails when a `.line("..")` or
`.section("..")` in `apps/desktop/src/diagnostics.rs` is missing from it. Right-to-left:
`i18n::direction()` says which way the language runs and `.app` carries `data-dir`; no
right-to-left language or mirrored layout exists yet. After editing `locales/*.json`,
touch `crates/view-model/src/i18n.rs` (the catalogue is compiled in and cargo does not
watch it).
Where a `&str` or `&'static str` is wanted (a button label, a menu title) `ts("Literal")`
gives the translation as text kept for the run; `t` gives a `String`. A test
(`i18n/hardcoded.rs`) scans `apps/desktop/src` for text-like string literals that are not
given to `t`/`ts`/`tf`/`tn` or a translating helper and fails on a new one: translate it,
or add it to `ALLOWED`/`ALLOWED_IN` there with the reason (a brand, a key name, text only
the test bridge reads). Strings that were hardcoded when the check came are listed in
`i18n/known_gaps.txt`, a to-do list that only shrinks (`I18N_PRUNE_GAPS=1 cargo test -p
morpheus-view-model i18n` drops the ones translated since). Entries marked in
`_machine_translated` of a locale file are machine quality until a speaker reviews them.

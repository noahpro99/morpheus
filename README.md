# Morpheus

A Discord-shaped Matrix client, native in Rust. Rooms in spaces and folders,
direct messages, threads, voice and video calls, end-to-end encryption, and
Discord's keyboard shortcuts, on any Matrix homeserver.

| Where | App | Built with |
| --- | --- | --- |
| Linux, Windows, macOS | `apps/desktop` | [GPUI](https://www.gpui.rs) (GPU-drawn, no web view) |
| Android, iOS | `apps/mobile` | Dioxus 0.7 |

## Features

- Rooms, spaces (servers) with channels and categories, folders on the rail, DMs, friends, invites.
- Messages with Markdown, replies, edits, reactions, threads, pins, attachments, pictures and video, search, mentions and `:emoji:`.
- Encrypted rooms: key backup, device verification (emoji and QR), shields.
- Calls (MatrixRTC with LiveKit): voice channels, DM rings, camera, screen share, noise suppression (DeepFilterNet), mic test. Linux, Windows, macOS and Android.
- Moderation tools, nicknames, per-server profiles, profile pictures with a crop view, password-locked chats, streamer mode, Vim mode, themes.
- Desktop: a tray that keeps syncing and notifying with the window closed, autostart, in-place updates, `matrix:` links.
- Phones: one pane at a time, swipes, UnifiedPush notifications, share target (Android).

## Install

Download the file for your system from the
[releases page](https://github.com/noahpro99/morpheus/releases/latest):

| System | File |
| --- | --- |
| Linux (x86-64) | `morpheus-<version>-linux-x86_64.zip`: unzip, run `morpheus-desktop` |
| Windows | `morpheus-<version>-windows-x86_64.zip`: unzip, run `Morpheus.exe` |
| macOS | `morpheus-<version>-macos-arm64.dmg` (Apple silicon) or `-macos-x86_64.dmg` (Intel): open it, drag `Morpheus.app` to Applications. The `.zip` of the same name holds the same app |

Phone apps (Android, iPhone) are paused: they will be released again once they are rebuilt natively. The last ones are attached to v0.2.2 and older releases.

The desktop app updates itself in place from the releases. The web app needs
no install. Releases are made as described in [docs/releasing.md](docs/releasing.md).

## Build from source

The repository is a Cargo workspace; on NixOS or with Nix, `direnv allow` (or
`nix develop`) gives the shell with every library and tool. Commands are in the `justfile`
(`just` lists them):

```sh
just desktop            # cargo run -p morpheus-desktop
just desktop-as bob     # a second profile (own data dir under .dev-data), to talk to yourself
just desktop-release    # release build
just android            # run on a plugged-in phone (nix develop .#android)
just android-virtual    # run on the emulator (just android-avd, just android-emulator first)
```

Elsewhere you need a recent stable Rust (`rust-toolchain.toml`), and the
GUI, audio and WebRTC libraries listed in `flake.nix` and `.github/workflows/linux.yml`.
iOS needs a Mac (`apple.yml` builds it in CI).

## The dev homeserver

Tests and manual runs use a local Synapse (with LiveKit and its token service)
in Docker, in `dev/homeserver`:

```sh
just hs-up      # start it on http://localhost:8008 (first run generates its config)
just hs-seed    # users alice, bob, carol (name-password, alice is admin) and rooms with history
just hs-down    # stop it; just hs-logs tails it
```

Log in to the desktop app with `alice` / `alice-password` and the homeserver
`http://localhost:8008`. `just hs-user dave secret` adds a user and
`just hs-reset` destroys the server and all its data.

## Testing

| Command | What it does |
| --- | --- |
| `just check` | type-check the workspace |
| `just test` | unit tests (`crates/view-model`, core, ...); live tests skip without a homeserver |
| `just test-live` | core against the local Synapse: log in, sync, read and send |
| `just e2e` | the desktop e2e tests, headless; `just e2e <file> [test]` for one file or test |
| `just e2e-display` | the same in real windows on the hidden Hyprland test monitor (pixel checks too) |
| `just e2e-one <file>` | one file with per-step timings |
| `just e2e-web`, `just e2e-android` | the same tests against the web app (headless Chromium) or the emulator |
| `just lint`, `just fmt`, `just ci` | clippy with warnings as errors, rustfmt, all of them |

### End-to-end tests

The end-to-end tests are in a private repo, `noahpro99/morpheus-e2e`
(`tests/<feature>.rs`, plus a small Playwright-style driver and the desktop
bridge, `bridge/`). Check it out next to this repo (`../morpheus-e2e`, or set
`MORPHEUS_E2E_DIR`; the bridge is read from `../morpheus-e2e/bridge`) to run
them; without it the app builds and unit-tests as usual, only the `e2e` feature
and `just e2e*` need it. `just e2e` builds the desktop app with the `e2e`
feature as its own binary, `morpheus-desktop-e2e`, which opens a loopback
control socket answering selector requests (the phone and web apps evaluate
JavaScript in their page instead):

```rust
let mut app = App::launch(&homeserver).await?;
app.wait_for("form.login", DEFAULT_WAIT).await?;
app.type_into("form.login input[type=text]", "alice").await?;
app.submit("form.login").await?;
app.click_text(".room", "dev").await?;
app.wait_for_text(".message .body", "sliding sync is working", DEFAULT_WAIT).await?;
```

- **Headless** (`just e2e`, the default): the app runs on gpui's headless
  platform (offscreen windows with real layout and input, no pixels), so it
  works with the screen locked, over SSH and in CI
  (the private repo's e2e-headless workflow). A check that needs real pixels or a
  window manager prints `SKIPPED (needs a display)` and is counted at the end.
- **With a display** (`just e2e-display`): real windows, titled "Morpheus (test)", on the hidden Hyprland
  monitor, unfocused (the driver adds `nofocus` rules for that title; undo with a config reload).
  `app.screenshot("name")` writes `target/e2e/name.png` there.
- Each test gets a fresh data directory and its own app process; `MORPHEUS_E2E_KEEP=1` keeps the
  data dir and `app.log` after a failure. `MORPHEUS_E2E_TIMINGS=1` prints how long every wait took.
- `just e2e` seeds the homeserver first (`just hs-seed` is idempotent). Tests that change a test
  user's profile restore it (`ProfileGuard`).
- Write the failing test first, run single tests, and run the whole suite only at milestones.

## Documentation

- [docs/GPUI_PORT.md](docs/GPUI_PORT.md): the desktop app's layout, how tests are the spec, rules for agents and contributors, gotchas.
- [docs/gpui-port-history.md](docs/gpui-port-history.md): how the desktop port went, task by task.
- [docs/PLAN.md](docs/PLAN.md): the original plan and architecture, with what is done marked.
- [docs/releasing.md](docs/releasing.md): the release flow. [docs/releases/](docs/releases/): release notes.
- [SECURITY.md](SECURITY.md): reporting a security problem.

The crates: `crates/core` (matrix-rust-sdk: sessions, room list, timelines, no UI), `crates/view-model`
(the logic behind the screens, no UI toolkit), `crates/call` (calls, audio, noise suppression, screen
capture), `crates/ui` (shared Dioxus components for phone and web), `crates/video`. The end-to-end tests are in a separate private repo (below).

## Storage

Everything the Matrix SDK caches (room state, crypto keys, event and media
caches) lives in SQLite databases under the data directory (`MORPHEUS_DATA_DIR`,
else the XDG data dir), encrypted with a per-install passphrase. That
passphrase and the login tokens go to the OS keyring (Secret Service on Linux,
Keychain on macOS, Credential Manager on Windows). With no keyring daemon
answering, an encrypted file under the data dir stands in and a warning is
logged. `MORPHEUS_SESSION_STORE=keyring` or `=file` forces one; test runs use
the file so they never touch your keyring.

## Tray

Closing the window hides it; Morpheus keeps syncing and raising notifications
from the tray (the logo, with Open and Quit in its menu; a left click reopens).
Quit from the tray menu to stop it. The tray needs `libayatana-appindicator`
on Linux, which the dev shell provides; without it the app runs with no tray
and says so in the log.

## License

Morpheus is licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution you submit for inclusion in Morpheus, as defined in the Apache-2.0 license, is dual licensed as above, without any additional terms or conditions.

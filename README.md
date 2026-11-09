# Morpheus

A Discord-shaped Matrix client, native in Rust.

- **Core** (`crates/core`): matrix-rust-sdk, sessions, room list, timelines. No UI.
- **UI** (`crates/ui`): shared Dioxus components and theme.
- **Apps** (`apps/desktop`, `apps/mobile`): thin binaries that pick a renderer.

See [PLAN.md](PLAN.md) for the roadmap and architecture notes.

## Quick start (NixOS)

```sh
direnv allow            # or: nix develop
just hs-up              # local Synapse on http://localhost:8008
just hs-seed            # users alice/bob/carol + rooms with history
just desktop            # dx serve with hot reload
```

Log in with `alice` / `alice-password`. Open a second window as another user
with `just desktop-as bob` to watch messages and typing flow between them.

Shortcuts: `Ctrl+K` jumps to a room, `Enter` sends, `Esc` cancels a reply or edit.
Hover a message for reactions, reply, edit and delete.

## Testing

| Command          | What it does                                                     |
| ---------------- | ---------------------------------------------------------------- |
| `just check`     | type-check all crates                                            |
| `just test`      | unit tests                                                       |
| `just test-live` | logs in to the local Synapse, syncs rooms, reads and sends       |
| `just e2e`       | Playwright-style tests that drive the real desktop window         |
| `just lint`      | clippy, warnings are errors                                      |
| `just ci`        | fmt + lint + test                                                |

The live test is the fastest way to verify core changes without a GUI.

### End-to-end UI tests

`just e2e` builds the desktop app with the `e2e` feature, which opens a
loopback control socket inside the app that evaluates JavaScript in the
webview. The `morpheus-e2e` crate wraps it in a small driver:

```rust
let mut app = App::launch(&homeserver).await?;
app.wait_for("form.login", DEFAULT_WAIT).await?;
app.type_into("form.login input[type=text]", "alice").await?;
app.submit("form.login").await?;
app.click_text(".room", "dev").await?;
app.wait_for_text(".message .body", "sliding sync is working", DEFAULT_WAIT).await?;
app.screenshot("chat").await?; // target/e2e/chat.png on Hyprland
```

Each test gets a fresh data directory and its own app process. Set
`MORPHEUS_E2E_KEEP=1` to keep the data dir and `app.log` after a failure.
Test windows are titled "Morpheus (test)" and open unfocused; on Hyprland the driver also adds
`nofocus` rules for that title so keystrokes never land in them mid-run (undo with a config reload).

Run one file at a time with `just e2e-one <file>` (for example `just e2e-one perf`).
`just e2e` and `just e2e-one` reseed the homeserver first (`just hs-seed` is idempotent and
puts kicked or banned test users back into `#moderation`).

E2E files map to features: `chat`, `spaces`, `features`, `interactions`, `attachments`, `threads`,
`new_messages_marker`, `settings`, `perf`, `layout`, `scrolling`, `context_menu`, `account`,
`moderation`, `message_menu`, `room_menu`, `hotkeys`, `space_menu`, `profile_card`, `rail`,
`titlebar`, `invites`, `notifications`, `autocomplete`, `toasts`, `focus`, `tray`, `mobile_layout`
(the phone layout, driven on the desktop build inside a phone-sized box).

Keyboard shortcuts follow Discord's defaults; press Ctrl+/ in the app for the list.
With `MORPHEUS_E2E_TIMINGS=1` the driver prints how long every wait took, and
the core logs any operation slower than 750 ms with its call site, so slow
spots show up in both the test output and the app log.

## Mobile

```sh
nix develop .#android
just android
```

Builds and runs on a connected device or a running emulator. iOS requires a Mac.

`just android-check` type-checks the mobile crate for `aarch64-linux-android` without a
device. The mobile app is the same shell as the desktop one: on a phone-width window it
shows one pane at a time (rooms, chat, members or thread) with back buttons, which
`just e2e-one mobile_layout` drives on the desktop build inside a phone-sized box.

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

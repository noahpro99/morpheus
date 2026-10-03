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
with `just desktop-as bob` to watch messages flow between them.

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

## Mobile

```sh
nix develop .#android
just android
```

Builds and runs on a connected device or a running emulator. iOS requires a Mac.

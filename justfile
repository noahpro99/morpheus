set shell := ["bash", "-euo", "pipefail", "-c"]

hs := env("MORPHEUS_HOMESERVER", "http://localhost:8008")

# List recipes
default:
    @just --list --unsorted

# ---------------------------------------------------------------- build & run

# Type-check everything (fastest feedback loop)
check:
    cargo check --workspace --all-targets

# Format all crates
fmt:
    cargo fmt --all

# Clippy with warnings as errors
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Unit tests (live tests self-skip without a homeserver)
test:
    cargo test --workspace

# End-to-end tests against the local homeserver (run `just hs-up hs-seed` first)
test-live:
    MORPHEUS_TEST_HOMESERVER={{hs}} cargo test -p morpheus-core --test live -- --nocapture

# Build the desktop app with the in-app test bridge
e2e-build:
    dx build --package morpheus-desktop --features e2e

# Playwright-style tests through the real window (needs `just hs-up hs-seed`)
e2e: e2e-build hs-seed
    MORPHEUS_TEST_HOMESERVER={{hs}} cargo test -p morpheus-e2e -- --nocapture --test-threads=1

# One e2e test file with per-step timings, e.g. `just e2e-one perf` (no rebuild; run `just e2e-build` after UI changes).
# Reseeds first: the moderation tests kick and ban users and the seed puts them back.
e2e-one file: hs-seed
    MORPHEUS_TEST_HOMESERVER={{hs}} MORPHEUS_E2E_TIMINGS=1 cargo test -p morpheus-e2e --test {{file}} -- --nocapture --test-threads=1

# Everything CI would run
ci: fmt lint test

# Run the desktop app with hot reload
desktop:
    dx serve --package morpheus-desktop

# Run the desktop app as a named profile (separate data dir), e.g. `just desktop-as bob`
desktop-as profile:
    MORPHEUS_DATA_DIR="$PWD/.dev-data/{{profile}}" dx serve --package morpheus-desktop

# Release build of the desktop app
desktop-release:
    dx build --package morpheus-desktop --release

# Run on Android (emulator or device). Needs `nix develop .#android`
android:
    dx serve --package morpheus-mobile --platform android

# Type-check the mobile crate for Android without an emulator (needs `nix develop .#android`, or runs it)
android-check:
    nix develop .#android -c cargo check -p morpheus-mobile --target aarch64-linux-android

# Wipe local app data (sessions, caches) for all dev profiles
clean-data:
    rm -rf .dev-data
    rm -rf "${XDG_DATA_HOME:-$HOME/.local/share}/morpheus"

# ---------------------------------------------------------------- homeserver

# Start the local Synapse (first run generates config)
hs-up:
    dev/homeserver/up.sh

# Stop the local Synapse
hs-down:
    cd dev/homeserver && docker compose down

# Tail Synapse logs
hs-logs:
    cd dev/homeserver && docker compose logs -f

# Destroy the local Synapse and all its data
hs-reset: hs-down
    rm -rf dev/homeserver/data dev/homeserver/.seeded

# Create users alice/bob/carol and seeded rooms
hs-seed:
    dev/homeserver/seed.sh

# Register one user: `just hs-user dave secret [--admin]`
hs-user name password *flags:
    dev/homeserver/register.sh {{name}} {{password}} {{flags}}

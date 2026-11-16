#!/usr/bin/env bash
# Runs `cargo test` for the end-to-end tests, which live in the private repo
# noahpro99/morpheus-e2e (checked out next to this one, or at
# $MORPHEUS_E2E_DIR). Builds into this repo's target/ and tells the tests
# where this repo is. Usage: scripts/e2e-cargo.sh [cargo test arguments]
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
dir=${MORPHEUS_E2E_DIR:-$repo/../morpheus-e2e}
if [ ! -f "$dir/Cargo.toml" ]; then
    echo "The end-to-end tests are in a private repo that is not checked out at $dir." >&2
    echo "Clone it next to this one (git clone git@github.com:noahpro99/morpheus-e2e.git ../morpheus-e2e)" >&2
    echo "or set MORPHEUS_E2E_DIR to where it is. Only people with access to it can run them." >&2
    exit 1
fi
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$repo/target}
export MORPHEUS_REPO=$repo
exec cargo test --manifest-path "$dir/Cargo.toml" "$@"

#!/usr/bin/env bash
# Points this checkout and its private e2e checkout at each other, so they can be worktrees with other names:
#   <repo>/.e2e   -> the e2e checkout   (apps/desktop/src/e2e/mod.rs includes the test bridge through it)
#   <e2e>/.repo   -> this checkout      (the e2e crate's path dependencies go through it)
# The e2e checkout is $MORPHEUS_E2E_DIR, else the sibling named like this one: morpheus -> ../morpheus-e2e,
# morpheus-android -> ../morpheus-e2e-android, morpheus-desktop -> ../morpheus-e2e-desktop.
# Idempotent, silent; run by scripts/e2e-cargo.sh, `just e2e-build` and the local-run/envy-run wrappers.
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
name=$(basename "$repo")
dir=${MORPHEUS_E2E_DIR:-$repo/../morpheus-e2e${name#morpheus}}
[ -d "$dir" ] || exit 0 # no private e2e checkout: nothing to link
dir=$(cd "$dir" && pwd)
[ "$(readlink "$repo/.e2e" 2>/dev/null)" = "$dir" ] || ln -sfn "$dir" "$repo/.e2e"
[ "$(readlink "$dir/.repo" 2>/dev/null)" = "$repo" ] || ln -sfn "$repo" "$dir/.repo"

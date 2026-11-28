#!/usr/bin/env bash
# What the app was built from: the commit, and when files the Android build reads differ from it, a short hash of
# those differences (so an uncommitted change is a different id). The debug app carries it in its version name
# (`0.2.8+<id>`), the test bridge reports it, and scripts/android-e2e-run.sh refuses a run when the APK on the phone
# is not the one this tree would build: a stale APK can never pass as new.
set -euo pipefail
cd "$(dirname "$0")/.."
paths="apps/android crates Cargo.toml Cargo.lock scripts/android-ffi.sh"
sha=$(git rev-parse --short=10 HEAD 2>/dev/null || echo unknown)
changes=$({
    git diff HEAD -- $paths
    git ls-files --others --exclude-standard -- $paths | while read -r f; do
        echo "$f"
        sha256sum "$f"
    done
} | sha256sum | cut -c1-8)
if [ "$changes" = e3b0c442 ]; then echo "$sha"; else echo "$sha-$changes"; fi

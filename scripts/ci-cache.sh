#!/usr/bin/env bash
# Helpers for the build caches on the Actions runners (#358); see
# .github/workflows/warm-cache.yml.
#   restore-mtimes   Cargo decides a path crate (vendor/*, crates/*, apps/*) is
#                    up to date by its files' modification times, and a fresh
#                    checkout makes every file newer than the cached build, so
#                    all of them recompiled (libwebrtc alone is 6 minutes on
#                    Windows). The cache carries .warm/sha, the commit it was
#                    built at: files unchanged since that commit get an old
#                    time, so only what changed rebuilds. Without a cache or
#                    the commit, nothing is touched (a full rebuild).
#   record           writes the current commit to .warm/sha, for the cache save.
#   unique-key       makes the rust-cache key new for this run (it would
#                    otherwise be the same until Cargo.lock changes, and an
#                    exact hit is never saved again) by adding a made-up
#                    package to Cargo.lock; `undo-key` puts the real one back
#                    (before the build, which is --locked).
set -euo pipefail
case "${1:-}" in
  restore-mtimes)
    [ -f .warm/sha ] || { echo "no .warm/sha: path crates rebuild"; exit 0; }
    sha=$(cat .warm/sha)
    git fetch --no-tags --depth=1 origin "$sha" 2>/dev/null || { echo "cannot fetch $sha: path crates rebuild"; exit 0; }
    export LC_ALL=C
    changed="${RUNNER_TEMP:-/tmp}/changed.txt"
    git -c core.quotepath=off diff --name-only "$sha" HEAD | sort > "$changed"
    git -c core.quotepath=off ls-files | sort | comm -23 - "$changed" | tr "\n" "\0" | xargs -0 touch -t 200001010000
    echo "files unchanged since $sha now carry an old time; changed: $(wc -l < "$changed")"
    ;;
  record)
    mkdir -p .warm
    git rev-parse HEAD > .warm/sha
    ;;
  unique-key)
    cp Cargo.lock "${RUNNER_TEMP:-/tmp}/Cargo.lock.orig"
    printf '\n[[package]]\nname = "warm-cache-run"\nversion = "0.0.%s"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "%s"\n' \
      "${GITHUB_RUN_ID:-0}" "$(printf '%064d' "${GITHUB_RUN_ID:-0}")" >> Cargo.lock
    ;;
  undo-key)
    cp "${RUNNER_TEMP:-/tmp}/Cargo.lock.orig" Cargo.lock
    ;;
  *) echo "usage: $0 restore-mtimes|record|unique-key|undo-key" >&2; exit 2 ;;
esac

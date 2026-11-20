#!/usr/bin/env bash
# Keeps the newest Actions cache entry of a Swatinem/rust-cache shared-key and
# deletes the older ones (#289): GitHub holds 10 GB a repository, an entry is
# about 2 GB, and a new Cargo.lock hash makes a new entry each time, so the
# old ones filled it and the in-use ones were evicted. Usage:
# scripts/trim-caches.sh <shared-key>   (needs GH_TOKEN with actions: write)
set -euo pipefail
prefix="v0-rust-$1-"
gh cache list --limit 100 --sort created_at --order desc --json id,key \
  --jq ".[] | select(.key | startswith(\"$prefix\")) | .id" |
  tail -n +2 |
  while read -r id; do
    echo "deleting the older cache $id"
    gh cache delete "$id" || true
  done

#!/usr/bin/env bash
# Keeps the Actions cache (10 GB a repository, an entry about 2 GB) holding the
# newest Swatinem/rust-cache entry of every platform (#289, #358). Entries are
# named v0-rust-<shared-key>-<os>-<arch>-<hash>-<hash>; a platform is the name
# without the two hashes, so a new Cargo.lock hash (a new entry) retires the
# older ones of its platform.
#   scripts/trim-caches.sh             keep each platform's newest entry, delete
#                                      the rest; warn when what is left is over
#                                      the limit
# Needs GH_TOKEN with actions: write.
set -euo pipefail
limit_gib=${LIMIT_GIB:-9.5}

list() { gh cache list --limit 100 --sort created_at --order desc \
  --json id,key,sizeInBytes,lastAccessedAt --jq '.[] | select(.key | startswith("v0-rust-")) | [.id, .key, .sizeInBytes, .lastAccessedAt] | @tsv'; }
group() { sed -E 's/-[0-9a-f]{8,}-[0-9a-f]{8,}$//' <<<"$1"; }

declare -A seen
keep=()
while IFS=$'\t' read -r id key size accessed; do
  g=$(group "$key")
  if [ -n "${seen[$g]:-}" ]; then
    echo "deleting the older cache $key"
    gh cache delete "$id" || true
  else
    seen[$g]=1
    keep+=("$size $key")
  fi
done < <(list)

total=0
for e in "${keep[@]}"; do total=$((total + ${e%% *})); done
gib=$(awk -v t="$total" 'BEGIN { printf "%.2f", t / 1073741824 }')
echo "kept ${#keep[@]} entries, $gib GiB (limit $limit_gib GiB)"
if awk -v t="$gib" -v l="$limit_gib" 'BEGIN { exit !(t > l) }'; then
  echo "::warning::the newest cache entries of all platforms are $gib GiB, over $limit_gib GiB: GitHub will evict some"
fi

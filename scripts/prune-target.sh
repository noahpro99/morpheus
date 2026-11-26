#!/usr/bin/env bash
# Keep target/ to the builds in use. Cargo never deletes anything itself:
# every change of code, features or flags (tests, clippy, e2e, dx's own
# profiles) leaves another copy behind, and incremental caches alone grew
# past 200 GB. Run by `just sweep` (before the build recipes) and, once a
# day, by the dev shell.
#
#   - incremental caches not written in a day: gone (rebuilt on demand);
#   - other artifacts no build has used in 2 days: gone (cargo-sweep);
#   - still over MORPHEUS_TARGET_CAP_GB (default 60): every incremental
#     cache and every artifact older than a day.
set -euo pipefail
cd "$(dirname "$0")/.."
target=target
[ -d "$target" ] || exit 0
cap_gb=${MORPHEUS_TARGET_CAP_GB:-60}

# Whole GB (rounded down); `du -sk` is the form GNU and macOS du share.
gb() { echo $(( $(du -sk "$target" 2>/dev/null | cut -f1) / 1048576 )); }

before=$(gb)

# Incremental caches: one folder per crate build, rewritten by every build
# that uses it.
find "$target" -maxdepth 4 -type d -name incremental -print0 2>/dev/null |
  while IFS= read -r -d '' dir; do
    find "$dir" -mindepth 1 -maxdepth 1 -type d -mmin +1440 -exec rm -rf {} +
  done

if command -v cargo-sweep >/dev/null 2>&1; then
  cargo-sweep sweep --recursive --time 2 "$target" >/dev/null 2>&1 || true
else
  nix run nixpkgs#cargo-sweep -- sweep --recursive --time 2 "$target" >/dev/null 2>&1 || true
fi

size=$(gb)
if [ "$size" -gt "$cap_gb" ]; then
  find "$target" -maxdepth 4 -type d -name incremental -prune -exec rm -rf {} +
  find "$target" -path '*/deps/*' -type f -mmin +1440 -delete 2>/dev/null || true
  size=$(gb)
fi
echo "target/: ${before} GB -> ${size} GB"

# The private e2e repo's target/ (it was 24 GB): the same cargo-sweep rule.
e2e_target=${MORPHEUS_E2E_DIR:-../morpheus-e2e}/target
if [ -d "$e2e_target" ]; then
  if command -v cargo-sweep >/dev/null 2>&1; then
    cargo-sweep sweep --recursive --time 2 "$e2e_target" >/dev/null 2>&1 || true
  fi
  echo "morpheus-e2e/target: $(( $(du -sk "$e2e_target" 2>/dev/null | cut -f1) / 1048576 )) GB"
fi

# Every `nix develop` leaves a /tmp/nix-shell.* dir of its own; thousands (23 GB) piled up.
# Remove the user's that are over 6 hours old (a running job's is younger, or in use by a long one: -mmin is
# on the dir itself, so a job over 6 hours old could lose it; none runs that long).
find /tmp -maxdepth 1 -type d -name 'nix-shell.*' -user "$(id -un)" -mmin +360 -exec rm -rf {} + 2>/dev/null || true
# And the per-job TMPDIRs of local-run.sh / envy-run.sh that a killed job left behind.
find /tmp -maxdepth 1 -type d -name 'morpheus-job.*' -user "$(id -un)" -mmin +360 -exec rm -rf {} + 2>/dev/null || true

#!/usr/bin/env bash
# `seed.sh` only when the homeserver needs it: a full pass takes 15 to 40 s even when nothing is missing (it
# registers, logs in and looks up every room). Skipped when this seed script and its helpers are the ones that
# last ran to the end (dev/homeserver/.seeded-stamp holds their hash) and the server still has alice and the seeded
# rooms (it does not after `just hs-reset`, or on a new server). Force a pass with MORPHEUS_FORCE_SEED=1
# (or `just hs-seed`, which always seeds).
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

stamp="$HS_STATE/.seeded-stamp"
want=$(cat "$HS_DIR/seed.sh" "$HS_DIR/lib.sh" | sha256sum | cut -d' ' -f1)

status() { curl -s -o /dev/null -w '%{http_code}' "$@" || true; }

if [[ "${MORPHEUS_FORCE_SEED:-}" != 1 && -f "$stamp" && "$(cat "$stamp")" == "$want" ]] \
  && [[ "$(status "$HS_URL/_matrix/client/v3/register/available?username=alice")" == 400 ]] \
  && [[ "$(status "$HS_URL/_matrix/client/v3/directory/room/%23lounge:localhost")" == 200 ]] \
  && [[ "$(status "$HS_URL/_matrix/client/v3/directory/room/%23encrypted:localhost")" == 200 ]]; then
  echo "homeserver: already seeded (MORPHEUS_FORCE_SEED=1 or just hs-seed to seed again)"
  exit 0
fi
"$HS_DIR/seed.sh"
echo "$want" > "$stamp"

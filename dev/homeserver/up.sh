#!/usr/bin/env bash
# Generate a Synapse config on first run, then start it and wait for health.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

export MORPHEUS_UID MORPHEUS_GID
MORPHEUS_UID=$(id -u)
MORPHEUS_GID=$(id -g)
mkdir -p "$HS_DIR/data"

if [[ ! -f "$HS_DIR/data/homeserver.yaml" ]]; then
  echo "generating synapse config…"
  compose run --rm synapse generate >/dev/null
  cat >>"$HS_DIR/data/homeserver.yaml" <<'YAML'

# ---- morpheus dev overrides ----
enable_registration: true
enable_registration_without_verification: true
# No rate limits while seeding / testing.
rc_message: { per_second: 1000, burst_count: 1000 }
rc_registration: { per_second: 1000, burst_count: 1000 }
rc_room_creation: { per_second: 1000, burst_count: 1000 }
rc_admin_redaction: { per_second: 1000, burst_count: 1000 }
rc_reports: { per_second: 1000, burst_count: 1000 }
rc_presence: { per_user: { per_second: 1000, burst_count: 1000 } }
rc_login:
  address: { per_second: 1000, burst_count: 1000 }
  account: { per_second: 1000, burst_count: 1000 }
  failed_attempts: { per_second: 1000, burst_count: 1000 }
rc_joins:
  local: { per_second: 1000, burst_count: 1000 }
  remote: { per_second: 1000, burst_count: 1000 }
rc_invites:
  per_room: { per_second: 1000, burst_count: 1000 }
  per_user: { per_second: 1000, burst_count: 1000 }
YAML
fi

compose up -d
echo -n "waiting for synapse"
for _ in $(seq 1 60); do
  if curl -fsS "$HS_URL/health" >/dev/null 2>&1; then
    echo " ready at $HS_URL"
    exit 0
  fi
  echo -n "."
  sleep 1
done
echo " timed out; see 'just hs-logs'" >&2
exit 1

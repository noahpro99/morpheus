#!/usr/bin/env bash
# Generate a Synapse config on first run, then start it and wait for health.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

export MORPHEUS_UID MORPHEUS_GID
MORPHEUS_UID=$(id -u)
MORPHEUS_GID=$(id -g)
mkdir -p "$HS_DATA"

# The desktop-lane server starts from a copy of the main one's config (same server name, listeners, room
# directory rules), signing key and certificate, with an empty database: the main server must have been
# started once.
if [[ "${MORPHEUS_HS_INSTANCE:-}" == desk && ! -f "$HS_DATA/homeserver.yaml" ]]; then
  [[ -f "$HS_HOME/data/homeserver.yaml" ]] || { echo "start the main server first (just hs-up)" >&2; exit 1; }
  for f in homeserver.yaml localhost.log.config localhost.signing.key tls.crt tls.key; do
    cp "$HS_HOME/data/$f" "$HS_DATA/$f"
  done
fi

if [[ ! -f "$HS_DATA/homeserver.yaml" ]]; then
  echo "generating synapse config…"
  compose run --rm synapse generate >/dev/null
  cat >>"$HS_DATA/homeserver.yaml" <<'YAML'

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

if [[ "${MORPHEUS_HS_INSTANCE:-}" != desk ]]; then
  "$HS_DIR/calls-setup.sh"
  "$HS_DIR/directory-setup.sh"
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

#!/usr/bin/env bash
# A throwaway Synapse beside the dev one, for e2e tests that need server
# settings the dev one lacks (room directory publishing, URL previews of
# local pages): host networking on port 8010 (so it can fetch pages served
# on localhost), its own data in /tmp/morpheus-spare, users alice, bob and
# carol (password <name>-password) and no rooms. Run the tests against it
# with `just hs=http://localhost:8010 e2e <file>`. `spare.sh down` removes it.
set -euo pipefail
DIR=/tmp/morpheus-spare
NAME=morpheus-synapse-spare
if [[ "${1:-}" == down ]]; then
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  rm -rf "$DIR"
  exit 0
fi
if docker ps --format '{{.Names}}' | grep -qx "$NAME"; then
  echo "already running on http://localhost:8010"
  exit 0
fi
docker rm -f "$NAME" >/dev/null 2>&1 || true
rm -rf "$DIR" && mkdir -p "$DIR"
run=(docker run --rm --user "$(id -u):$(id -g)" -v "$DIR:/data" -e SYNAPSE_SERVER_NAME=localhost -e SYNAPSE_REPORT_STATS=no)
"${run[@]}" matrixdotorg/synapse:latest generate >/dev/null
# The dev overrides (no rate limits, open registration) from up.sh, then ours.
sed -n '/^# ---- morpheus dev overrides/,/^YAML/p' "$(dirname "$0")/up.sh" | grep -v '^YAML' >>"$DIR/homeserver.yaml"
sed -i 's/^  - port: 8008/  - port: 8010/' "$DIR/homeserver.yaml"
cat >>"$DIR/homeserver.yaml" <<'YAML'
allow_guest_access: true
room_list_publication_rules:
  - action: allow
url_preview_enabled: true
url_preview_ip_range_blacklist: ["169.254.0.0/16"]
url_preview_ip_range_whitelist: ["127.0.0.0/8"]
YAML
docker run -d --name "$NAME" --network host --user "$(id -u):$(id -g)" -v "$DIR:/data" \
  -e SYNAPSE_SERVER_NAME=localhost -e SYNAPSE_REPORT_STATS=no matrixdotorg/synapse:latest >/dev/null
for _ in $(seq 1 60); do
  curl -fsS http://localhost:8010/health >/dev/null 2>&1 && break
  sleep 2
done
for u in alice bob carol; do
  docker exec "$NAME" register_new_matrix_user -c /data/homeserver.yaml -u "$u" -p "$u-password" --no-admin http://localhost:8010 >/dev/null
done
echo "spare Synapse ready on http://localhost:8010"

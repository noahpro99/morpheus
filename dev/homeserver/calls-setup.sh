#!/usr/bin/env bash
# Calls need more of Synapse than chat does; add it to an existing config once
# (idempotent): a TLS federation listener with a self-signed certificate (the
# token service checks OpenID tokens over federation), the LiveKit focus in
# the client well-known, MSC4143 transports and MSC4140 delayed events.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
CONFIG="$HS_DIR/data/homeserver.yaml"
MARK="# ---- morpheus calls ----"
grep -qF "$MARK" "$CONFIG" && exit 0

if [[ ! -f "$HS_DIR/data/tls.crt" ]]; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=localhost" \
    -keyout "$HS_DIR/data/tls.key" -out "$HS_DIR/data/tls.crt" 2>/dev/null
fi

# A second listener, inserted into the generated `listeners:` list.
awk '{ print } /^listeners:$/ {
  print "  - port: 8448"
  print "    tls: true"
  print "    type: http"
  print "    resources:"
  print "    - compress: false"
  print "      names:"
  print "      - federation"
  print "      - openid"
}' "$CONFIG" >"$CONFIG.new" && mv "$CONFIG.new" "$CONFIG"

cat >>"$CONFIG" <<YAML

$MARK
tls_certificate_path: /data/tls.crt
tls_private_key_path: /data/tls.key
public_baseurl: http://localhost:8008/
serve_client_wellknown: true
extra_well_known_client_content:
  org.matrix.msc4143.rtc_foci:
    - type: livekit
      livekit_service_url: http://localhost:8080
matrix_rtc:
  transports:
    - type: livekit
      livekit_service_url: http://localhost:8080
max_event_delay_duration: 24h
experimental_features:
  msc4140_enabled: true
  msc4143_enabled: true
  msc3266_enabled: true
YAML
echo "calls: synapse configured (restart to apply)"

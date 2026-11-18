#!/usr/bin/env bash
# The room directory needs publishing allowed (Synapse denies it unless a rule
# says otherwise): the e2e tests of Discover publish rooms. Added to an
# existing config once (idempotent); restart Synapse to apply.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
CONFIG="${1:-$HS_DIR/data/homeserver.yaml}"
grep -q "^room_list_publication_rules:" "$CONFIG" && exit 0
cat >>"$CONFIG" <<'YAML'

# ---- morpheus room directory ----
room_list_publication_rules:
  - action: allow
YAML
echo "directory: synapse configured (restart to apply)"

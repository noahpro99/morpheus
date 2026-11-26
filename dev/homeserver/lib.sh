# Shared helpers for the dev homeserver scripts. Source, don't execute.

HS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# The servers and their state live in the main checkout (there is one Synapse, whichever worktree runs the
# script): HS_HOME is its dev/homeserver, which is HS_DIR itself in the main checkout.
HS_HOME="$HS_DIR"
if _common=$(git -C "$HS_DIR" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) && [[ -d "${_common%/.git}/dev/homeserver" ]]; then
  HS_HOME="${_common%/.git}/dev/homeserver"
fi
# MORPHEUS_HS_INSTANCE=desk selects the second, desktop-lane homeserver (its own Synapse on :8009 with its
# own data dir and seed markers, so a desktop e2e run and an Android one never share users or rooms).
# Unset: the main one on :8008 (the only one with calls, and the one the emulator reaches).
if [[ "${MORPHEUS_HS_INSTANCE:-}" == desk ]]; then
  HS_URL="${MORPHEUS_HS_DESK_URL:-http://localhost:8009}"  # not MORPHEUS_HOMESERVER: the dev shell exports it as :8008
  COMPOSE_FILE="$HS_HOME/docker-compose.desk.yml"
  HS_DATA="$HS_HOME/data-desk"
  HS_STATE="$HS_HOME/state-desk"
  mkdir -p "$HS_STATE"
else
  HS_URL="${MORPHEUS_HOMESERVER:-http://localhost:8008}"
  COMPOSE_FILE="$HS_HOME/docker-compose.yml"
  HS_DATA="$HS_HOME/data"
  HS_STATE="$HS_HOME"
fi

compose() {
  if docker compose version >/dev/null 2>&1; then
    docker compose -f "$COMPOSE_FILE" "$@"
  else
    docker-compose -f "$COMPOSE_FILE" "$@"
  fi
}

# register_user <name> <password> [--admin]
register_user() {
  local name="$1" password="$2" flag="${3:---no-admin}"
  local out
  if out=$(compose exec -T synapse register_new_matrix_user \
      -c /data/homeserver.yaml -u "$name" -p "$password" "$flag" http://localhost:8008 2>&1); then
    echo "  + @$name:localhost"
  elif grep -q "already taken" <<<"$out"; then
    echo "  = @$name:localhost (exists)"
  else
    echo "$out" >&2
    return 1
  fi
}

# login <name> <password>  -> prints access token
login() {
  curl -fsS -X POST "$HS_URL/_matrix/client/v3/login" \
    -H 'Content-Type: application/json' \
    -d "$(jq -nc --arg u "$1" --arg p "$2" \
      '{type:"m.login.password", identifier:{type:"m.id.user", user:$u}, password:$p, initial_device_display_name:"seed"}')" \
    | jq -r .access_token
}

# api <token> <method> <path> [json-body]
api() {
  local token="$1" method="$2" path="$3" body="${4:-}"
  if [[ -n "$body" ]]; then
    curl -fsS -X "$method" "$HS_URL$path" -H "Authorization: Bearer $token" \
      -H 'Content-Type: application/json' -d "$body"
  else
    curl -fsS -X "$method" "$HS_URL$path" -H "Authorization: Bearer $token"
  fi
}

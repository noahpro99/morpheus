# Shared helpers for the dev homeserver scripts. Source, don't execute.

HS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HS_URL="${MORPHEUS_HOMESERVER:-http://localhost:8008}"
COMPOSE_FILE="$HS_DIR/docker-compose.yml"

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

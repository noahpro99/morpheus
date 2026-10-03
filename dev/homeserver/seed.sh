#!/usr/bin/env bash
# Create test users and rooms with some history. Safe to re-run.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

echo "users:"
register_user alice alice-password --admin
register_user bob bob-password
register_user carol carol-password

ALICE=$(login alice alice-password)
BOB=$(login bob bob-password)
CAROL=$(login carol carol-password)

# ensure_room <alias-localpart> <name> <topic> -> prints room id
ensure_room() {
  local alias="$1" name="$2" topic="$3" room_id
  if room_id=$(curl -fsS "$HS_URL/_matrix/client/v3/directory/room/%23$alias:localhost" 2>/dev/null | jq -r .room_id); then
    echo "  = #$alias ($room_id)" >&2
  else
    room_id=$(api "$ALICE" POST /_matrix/client/v3/createRoom "$(jq -nc \
      --arg a "$alias" --arg n "$name" --arg t "$topic" \
      '{room_alias_name:$a, name:$n, topic:$t, preset:"public_chat", invite:["@bob:localhost","@carol:localhost"]}')" | jq -r .room_id)
    api "$BOB" POST "/_matrix/client/v3/join/$room_id" '{}' >/dev/null
    api "$CAROL" POST "/_matrix/client/v3/join/$room_id" '{}' >/dev/null
    echo "  + #$alias ($room_id)" >&2
  fi
  echo "$room_id"
}

# say <token> <room> <text>
say() {
  api "$1" PUT "/_matrix/client/v3/rooms/$2/send/m.room.message/seed-$(date +%s%N)" \
    "$(jq -nc --arg b "$3" '{msgtype:"m.text", body:$b}')" >/dev/null
}

echo "rooms:"
GENERAL=$(ensure_room general "general" "Where everything starts")
RANDOM_ROOM=$(ensure_room random "random" "Off-topic")
DEV=$(ensure_room dev "dev" "Building Morpheus")

if [[ ! -f "$HS_DIR/.seeded" ]]; then
  echo "messages:"
  say "$ALICE" "$GENERAL" "Welcome to Morpheus"
  say "$BOB"   "$GENERAL" "hey! is this the new client?"
  say "$ALICE" "$GENERAL" "yep, Rust + Dioxus on top of matrix-rust-sdk"
  say "$ALICE" "$GENERAL" "this message should group under the one above"
  say "$CAROL" "$GENERAL" "looks fast 👀"
  for i in $(seq 1 30); do say "$BOB" "$RANDOM_ROOM" "random filler message $i"; done
  say "$CAROL" "$DEV" "sliding sync is working if you can read this"
  say "$ALICE" "$DEV" "next: avatars and media"
  touch "$HS_DIR/.seeded"
  echo "  + seeded"
else
  echo "messages: already seeded (delete dev/homeserver/.seeded to re-seed)"
fi

cat <<INFO

ready. log in with:
  homeserver  $HS_URL
  alice / alice-password   (admin)
  bob   / bob-password
  carol / carol-password
INFO

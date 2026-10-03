#!/usr/bin/env bash
# Create test users and rooms with some history. Safe to re-run.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

# Tiny PNGs so avatars and image messages have something to show.
PNG_ALICE="iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAMAAACdt4HsAAAAIGNIUk0AAHomAACAhAAA+gAAAIDoAAB1MAAA6mAAADqYAAAXcJy6UTwAAAADUExURXyM/0CllCQAAAAHdElNRQfqCgMUIDscruMhAAAAJXRFWHRkYXRlOmNyZWF0ZQAyMDI2LTEwLTAzVDIwOjMyOjU4KzAwOjAwNbSBVAAAACV0RVh0ZGF0ZTptb2RpZnkAMjAyNi0xMC0wM1QyMDozMjo1OCswMDowMETpOegAAAAodEVYdGRhdGU6dGltZXN0YW1wADIwMjYtMTAtMDNUMjA6MzI6NTgrMDA6MDAT/Bg3AAAAG0lEQVRYw+3BgQAAAADDoPlT3+AEVQEAAAB8AxBAAAEZszF2AAAAAElFTkSuQmCC"
PNG_BOB="iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAMAAACdt4HsAAAAIGNIUk0AAHomAACAhAAA+gAAAIDoAAB1MAAA6mAAADqYAAAXcJy6UTwAAAADUExURU/RpeIjVB4AAAAHdElNRQfqCgMUIDscruMhAAAAJXRFWHRkYXRlOmNyZWF0ZQAyMDI2LTEwLTAzVDIwOjMyOjU5KzAwOjAwk8OK4AAAACV0RVh0ZGF0ZTptb2RpZnkAMjAyNi0xMC0wM1QyMDozMjo1OSswMDowMOKeMlwAAAAodEVYdGRhdGU6dGltZXN0YW1wADIwMjYtMTAtMDNUMjA6MzI6NTkrMDA6MDC1ixODAAAAG0lEQVRYw+3BgQAAAADDoPlT3+AEVQEAAAB8AxBAAAEZszF2AAAAAElFTkSuQmCC"
PNG_CAROL="iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAMAAACdt4HsAAAAIGNIUk0AAHomAACAhAAA+gAAAIDoAAB1MAAA6mAAADqYAAAXcJy6UTwAAAADUExURf+fWmSFJwgAAAAHdElNRQfqCgMUIDscruMhAAAAJXRFWHRkYXRlOmNyZWF0ZQAyMDI2LTEwLTAzVDIwOjMyOjU5KzAwOjAwk8OK4AAAACV0RVh0ZGF0ZTptb2RpZnkAMjAyNi0xMC0wM1QyMDozMjo1OSswMDowMOKeMlwAAAAodEVYdGRhdGU6dGltZXN0YW1wADIwMjYtMTAtMDNUMjA6MzI6NTkrMDA6MDC1ixODAAAAG0lEQVRYw+3BgQAAAADDoPlT3+AEVQEAAAB8AxBAAAEZszF2AAAAAElFTkSuQmCC"
PNG_GENERAL="iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAMAAACdt4HsAAAAIGNIUk0AAHomAACAhAAA+gAAAIDoAAB1MAAA6mAAADqYAAAXcJy6UTwAAAADUExURV9y//CRoK4AAAAHdElNRQfqCgMUIDscruMhAAAAJXRFWHRkYXRlOmNyZWF0ZQAyMDI2LTEwLTAzVDIwOjMyOjU5KzAwOjAwk8OK4AAAACV0RVh0ZGF0ZTptb2RpZnkAMjAyNi0xMC0wM1QyMDozMjo1OSswMDowMOKeMlwAAAAodEVYdGRhdGU6dGltZXN0YW1wADIwMjYtMTAtMDNUMjA6MzI6NTkrMDA6MDC1ixODAAAAG0lEQVRYw+3BgQAAAADDoPlT3+AEVQEAAAB8AxBAAAEZszF2AAAAAElFTkSuQmCC"
PNG_PHOTO="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"

echo "users:"
register_user alice alice-password --admin
register_user bob bob-password
register_user carol carol-password

ALICE=$(login alice alice-password)
BOB=$(login bob bob-password)
CAROL=$(login carol carol-password)

# upload <token> <base64-png> -> prints mxc uri
upload() {
  base64 -d <<<"$2" | curl -fsS -X POST "$HS_URL/_matrix/media/v3/upload?filename=image.png" \
    -H "Authorization: Bearer $1" -H 'Content-Type: image/png' --data-binary @- | jq -r .content_uri
}

# set_avatar <token> <user-id> <base64-png>
set_avatar() {
  local mxc; mxc=$(upload "$1" "$3")
  api "$1" PUT "/_matrix/client/v3/profile/$2/avatar_url" "$(jq -nc --arg u "$mxc" '{avatar_url:$u}')" >/dev/null
}

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

# say_html <token> <room> <plain> <html>
say_html() {
  api "$1" PUT "/_matrix/client/v3/rooms/$2/send/m.room.message/seed-$(date +%s%N)" \
    "$(jq -nc --arg b "$3" --arg h "$4" '{msgtype:"m.text", body:$b, format:"org.matrix.custom.html", formatted_body:$h}')" >/dev/null
}

# send_image <token> <room> <base64-png> <caption>
send_image() {
  local mxc; mxc=$(upload "$1" "$3")
  api "$1" PUT "/_matrix/client/v3/rooms/$2/send/m.room.message/seed-$(date +%s%N)" \
    "$(jq -nc --arg u "$mxc" --arg b "$4" '{msgtype:"m.image", body:$b, url:$u, info:{mimetype:"image/png", w:320, h:200}}')" >/dev/null
}

# react <token> <room> <event-id> <key>
react() {
  api "$1" PUT "/_matrix/client/v3/rooms/$2/send/m.reaction/seed-$(date +%s%N)" \
    "$(jq -nc --arg e "$3" --arg k "$4" '{"m.relates_to":{rel_type:"m.annotation", event_id:$e, key:$k}}')" >/dev/null
}

echo "rooms:"
GENERAL=$(ensure_room general "general" "Where everything starts")
RANDOM_ROOM=$(ensure_room random "random" "Off-topic")
DEV=$(ensure_room dev "dev" "Building Morpheus")

if [[ ! -f "$HS_DIR/.seeded" ]]; then
  echo "profiles:"
  set_avatar "$ALICE" "@alice:localhost" "$PNG_ALICE"
  set_avatar "$BOB" "@bob:localhost" "$PNG_BOB"
  set_avatar "$CAROL" "@carol:localhost" "$PNG_CAROL"
  GENERAL_AVATAR=$(upload "$ALICE" "$PNG_GENERAL")
  api "$ALICE" PUT "/_matrix/client/v3/rooms/$GENERAL/state/m.room.avatar" "$(jq -nc --arg u "$GENERAL_AVATAR" '{url:$u}')" >/dev/null
  echo "  + avatars"

  echo "messages:"
  say "$ALICE" "$GENERAL" "Welcome to Morpheus"
  say "$BOB"   "$GENERAL" "hey! is this the new client?"
  say "$ALICE" "$GENERAL" "yep, Rust + Dioxus on top of matrix-rust-sdk"
  say "$ALICE" "$GENERAL" "this message should group under the one above"
  say_html "$CAROL" "$GENERAL" "looks **fast** 👀 — check https://matrix.org" \
    'looks <strong>fast</strong> 👀 — check <a href="https://matrix.org">matrix.org</a>'
  send_image "$BOB" "$GENERAL" "$PNG_PHOTO" "morpheus.png"
  say_html "$ALICE" "$GENERAL" "code:\n\`\`\`rust\nfn main() {}\n\`\`\`" \
    'code:<pre><code class="language-rust">fn main() {}</code></pre>'
  LAST=$(api "$CAROL" PUT "/_matrix/client/v3/rooms/$GENERAL/send/m.room.message/seed-$(date +%s%N)" \
    "$(jq -nc '{msgtype:"m.text", body:"react to this one"}')" | jq -r .event_id)
  react "$ALICE" "$GENERAL" "$LAST" "👍"
  react "$BOB" "$GENERAL" "$LAST" "👍"
  react "$BOB" "$GENERAL" "$LAST" "🎉"
  for i in $(seq 1 80); do say "$BOB" "$RANDOM_ROOM" "random filler message $i"; done
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

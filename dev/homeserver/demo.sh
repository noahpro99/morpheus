#!/usr/bin/env bash
# A tidy world for screenshots (the website, store pages): people with
# names and pictures, a space with a few rooms and a believable evening of
# chat, a thread, replies, reactions and a picture. Separate from the test
# seed: its own users (log in as `luna` / `luna-password`), its own rooms.
# Run `nix shell nixpkgs#imagemagick -c dev/homeserver/demo.sh`; running again replaces the demo world.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

# Each run starts over: the demo people leave (and forget) every room first.
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "people:"
PEOPLE=(luna milo ines kai rosa)
declare -A NAME=([luna]="Luna" [milo]="Milo" [ines]="Inès" [kai]="Kai" [rosa]="Rosa")
# Two colours each, for a soft diagonal gradient picture.
declare -A COLOURS=([luna]="#7c8cff #b8c0ff" [milo]="#2bb3a3 #a6f0d8" [ines]="#ff8a7a #ffd9a0" [kai]="#5f72ff #1c2a8a" [rosa]="#c06bff #ffb3e6")
declare -A TOKEN
for p in "${PEOPLE[@]}"; do
  register_user "$p" "$p-password"
  TOKEN[$p]=$(login "$p" "$p-password")
  # Joined rooms, and invites still waiting (from an earlier run).
  for r in $(api "${TOKEN[$p]}" GET /_matrix/client/v3/joined_rooms | jq -r '.joined_rooms[]') \
      $(api "${TOKEN[$p]}" GET "/_matrix/client/v3/sync?timeout=0" | jq -r '.rooms.invite // {} | keys[]'); do
    api "${TOKEN[$p]}" POST "/_matrix/client/v3/rooms/$r/leave" '{}' >/dev/null
    api "${TOKEN[$p]}" POST "/_matrix/client/v3/rooms/$r/forget" '{}' >/dev/null
  done
done

# upload_file <token> <path> <mimetype> -> prints mxc uri
upload_file() {
  curl -fsS -X POST "$HS_URL/_matrix/media/v3/upload?filename=$(basename "$2")" \
    -H "Authorization: Bearer $1" -H "Content-Type: $3" --data-binary "@$2" | jq -r .content_uri
}

echo "pictures:"
for p in "${PEOPLE[@]}"; do
  read -r a b <<<"${COLOURS[$p]}"
  magick -size 256x256 "gradient:$a-$b" -rotate 45 -gravity center -crop 256x256+0+0 +repage \
    \( -size 256x256 xc:none -fill 'rgba(255,255,255,0.18)' -draw 'circle 170,80 170,140' \) -composite \
    "$WORK/$p.png"
  mxc=$(upload_file "${TOKEN[$p]}" "$WORK/$p.png" image/png)
  api "${TOKEN[$p]}" PUT "/_matrix/client/v3/profile/@$p:localhost/displayname" \
    "$(jq -nc --arg n "${NAME[$p]}" '{displayname:$n}')" >/dev/null
  api "${TOKEN[$p]}" PUT "/_matrix/client/v3/profile/@$p:localhost/avatar_url" \
    "$(jq -nc --arg u "$mxc" '{avatar_url:$u}')" >/dev/null
  echo "  + ${NAME[$p]}"
done
# The space's picture: a crescent moon on night blue.
magick -size 256x256 'gradient:#141720-#2a3170' \
  -fill '#ffd9a0' -draw 'circle 128,128 128,58' -fill '#1f2550' -draw 'circle 158,108 158,48' \
  "$WORK/space.png"
# A photo-like picture to share: a night sky over hills.
magick -size 960x600 'gradient:#0f1118-#3b3f8f' \
  \( -size 960x600 xc:none -fill '#ffd9a0' -draw 'circle 700,170 700,120' \) -composite \
  \( -size 960x600 xc:none -fill '#141720' -draw 'ellipse 220,640 520,220 0,360' -fill '#1a1e29' -draw 'ellipse 760,660 480,200 0,360' \) -composite \
  \( -size 960x600 xc:none -fill 'rgba(255,255,255,0.8)' \
     -draw 'point 80,60' -draw 'point 160,140' -draw 'point 300,80' -draw 'point 420,190' -draw 'point 520,50' \
     -draw 'point 860,90' -draw 'point 620,260' -draw 'point 900,240' -draw 'point 40,230' -draw 'point 360,30' \) -composite \
  "$WORK/night.png"

L=${TOKEN[luna]} M=${TOKEN[milo]} I=${TOKEN[ines]} K=${TOKEN[kai]} R=${TOKEN[rosa]}

send() { # send <token> <room> <json> -> event id
  api "$1" PUT "/_matrix/client/v3/rooms/$2/send/m.room.message/demo-$(date +%s%N)" "$3" | jq -r .event_id
}
say() { send "$1" "$2" "$(jq -nc --arg b "$3" '{msgtype:"m.text", body:$b}')"; }
reply() { # reply <token> <room> <to-event> <text>
  send "$1" "$2" "$(jq -nc --arg b "$4" --arg e "$3" '{msgtype:"m.text", body:$b, "m.relates_to":{"m.in_reply_to":{event_id:$e}}}')"
}
in_thread() { # in_thread <token> <room> <root> <text>
  send "$1" "$2" "$(jq -nc --arg b "$4" --arg r "$3" '{msgtype:"m.text", body:$b, "m.relates_to":{rel_type:"m.thread", event_id:$r, is_falling_back:true, "m.in_reply_to":{event_id:$r}}}')"
}
react() {
  api "$1" PUT "/_matrix/client/v3/rooms/$2/send/m.reaction/demo-$(date +%s%N)" \
    "$(jq -nc --arg e "$3" --arg k "$4" '{"m.relates_to":{rel_type:"m.annotation", event_id:$e, key:$k}}')" >/dev/null
}
others() { # others <token> -> JSON list of everyone but that token's owner
  local me
  me=$(api "$1" GET /_matrix/client/v3/account/whoami | jq -r .user_id)
  printf '%s\n' @luna:localhost @milo:localhost @ines:localhost @kai:localhost @rosa:localhost | grep -vx "$me" | jq -R . | jq -sc .
}
join() { # join <token> <room>, retrying while the invite settles
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    api "$1" POST "/_matrix/client/v3/join/$2" '{}' >/dev/null 2>&1 && return
    sleep 0.5
  done
  echo "could not join $2" >&2
  return 1
}
room() { # room <creator-token> <name> <topic> [space] -> room id; everyone joins
  local id
  id=$(api "$1" POST /_matrix/client/v3/createRoom "$(jq -nc --arg n "$2" --arg t "$3" \
    '{name:$n, topic:$t, preset:"public_chat",
      power_level_content_override:{events:{"org.matrix.msc3401.call.member":0}},
      invite:$people}' --argjson people "$(others "$1")")" | jq -r .room_id)
  for t in "$L" "$M" "$I" "$K" "$R"; do join "$t" "$id"; done
  if [[ -n "${4:-}" ]]; then
    api "$L" PUT "/_matrix/client/v3/rooms/$4/state/m.space.child/$id" '{"via":["localhost"]}' >/dev/null
    api "$1" PUT "/_matrix/client/v3/rooms/$id/state/m.space.parent/$4" '{"via":["localhost"],"canonical":true}' >/dev/null
  fi
  echo "$id"
}

echo "space and rooms:"
SPACE=$(api "$L" POST /_matrix/client/v3/createRoom "$(jq -nc \
  '{name:"Night Owls", topic:"Friends who are up too late", preset:"public_chat", creation_content:{type:"m.space"},
    invite:["@milo:localhost","@ines:localhost","@kai:localhost","@rosa:localhost"]}')" | jq -r .room_id)
for t in "$M" "$I" "$K" "$R"; do api "$t" POST "/_matrix/client/v3/join/$SPACE" '{}' >/dev/null; done
mxc=$(upload_file "$L" "$WORK/space.png" image/png)
api "$L" PUT "/_matrix/client/v3/rooms/$SPACE/state/m.room.avatar" "$(jq -nc --arg u "$mxc" '{url:$u}')" >/dev/null

LOBBY=$(room "$L" "lobby" "Say hi, plan things" "$SPACE")
DESIGN=$(room "$I" "design" "Sketches, mockups, feedback" "$SPACE")
MUSIC=$(room "$K" "music" "What's on repeat" "$SPACE")
MOVIES=$(room "$R" "movie-night" "Fridays, 9pm, someone's couch" "$SPACE")
echo "  + Night Owls: lobby, design, music, movie-night"

echo "conversations:"
# #music first and #lobby last, so the lobby is the newest room.
a=$(say "$K" "$MUSIC" "new Nils Frahm album is perfect for 2am coding")
react "$M" "$MUSIC" "$a" "🎹"
react "$L" "$MUSIC" "$a" "🎹"
say "$R" "$MUSIC" "adding it to the shared playlist" >/dev/null
say "$M" "$MUSIC" "the shared playlist is 40% Kai's lo-fi at this point" >/dev/null
b=$(say "$K" "$MUSIC" "as it should be")
react "$I" "$MUSIC" "$b" "😂"

say "$R" "$MOVIES" "friday: we're voting between Paprika and Spirited Away" >/dev/null
c=$(say "$I" "$MOVIES" "Paprika. it's literally a movie about dreams")
react "$L" "$MOVIES" "$c" "🌙"
react "$K" "$MOVIES" "$c" "👍"
say "$M" "$MOVIES" "I'll bring snacks" >/dev/null

d=$(say "$I" "$DESIGN" "first pass at the new onboarding, thoughts?")
send "$I" "$DESIGN" "$(jq -nc --arg u "$(upload_file "$I" "$WORK/night.png" image/png)" \
  '{msgtype:"m.image", body:"onboarding-night.png", url:$u, info:{mimetype:"image/png", w:960, h:600}}')" >/dev/null
e=$(say "$L" "$DESIGN" "the moon is so good. maybe a touch less contrast on the hills?")
react "$I" "$DESIGN" "$e" "👀"
in_thread "$I" "$DESIGN" "$d" "here's what I changed since yesterday" >/dev/null
in_thread "$M" "$DESIGN" "$d" "love the softer type. could the button sit a bit lower?" >/dev/null
in_thread "$I" "$DESIGN" "$d" "yep, moving it now" >/dev/null

# Earlier in the evening.
say "$K" "$LOBBY" "made it home, the train was packed" >/dev/null
say "$I" "$LOBBY" "welcome back! how was the gig?" >/dev/null
x=$(say "$K" "$LOBBY" "loud. great. my ears are still ringing")
react "$R" "$LOBBY" "$x" "🎸"
say "$M" "$LOBBY" "did they play the new one?" >/dev/null
say "$K" "$LOBBY" "twice, as the encore too" >/dev/null
say "$R" "$LOBBY" "jealous. next time I'm coming" >/dev/null
y=$(say "$I" "$LOBBY" "reminder: movie night is friday, vote in #movie-night")
react "$L" "$LOBBY" "$y" "🍿"
react "$M" "$LOBBY" "$y" "🍿"
say "$L" "$LOBBY" "brb, making dinner" >/dev/null
say "$L" "$LOBBY" "ok back" >/dev/null
f=$(say "$L" "$LOBBY" "who's still up? 🌙")
react "$M" "$LOBBY" "$f" "🙋"
react "$K" "$LOBBY" "$f" "🙋"
react "$R" "$LOBBY" "$f" "🙋"
say "$M" "$LOBBY" "always" >/dev/null
g=$(say "$R" "$LOBBY" "finally finished the zine, printing tomorrow")
react "$L" "$LOBBY" "$g" "🎉"
react "$I" "$LOBBY" "$g" "🎉"
react "$K" "$LOBBY" "$g" "❤️"
reply "$I" "$LOBBY" "$g" "can I get a copy? I'll trade you a print" >/dev/null
say "$R" "$LOBBY" "deal" >/dev/null
say "$K" "$LOBBY" "anyone want to hop on a call and work on stuff together?" >/dev/null
h=$(say "$M" "$LOBBY" "yes, give me 5 min")
react "$L" "$LOBBY" "$h" "👍"
say "$L" "$LOBBY" "same, I'll bring tea" >/dev/null

echo "direct messages:"
DM=$(api "$M" POST /_matrix/client/v3/createRoom "$(jq -nc \
  '{preset:"trusted_private_chat", is_direct:true, invite:["@luna:localhost"]}')" | jq -r .room_id)
api "$L" POST "/_matrix/client/v3/join/$DM" '{}' >/dev/null
api "$M" PUT "/_matrix/client/v3/user/@milo:localhost/account_data/m.direct" \
  "$(jq -nc --arg r "$DM" '{"@luna:localhost":[$r]}')" >/dev/null
api "$L" PUT "/_matrix/client/v3/user/@luna:localhost/account_data/m.direct" \
  "$(jq -nc --arg r "$DM" '{"@milo:localhost":[$r]}')" >/dev/null
say "$M" "$DM" "did you see Rosa's zine?" >/dev/null
say "$L" "$DM" "yes!! the cover is beautiful" >/dev/null
say "$M" "$DM" "we should get her to do the poster for movie night" >/dev/null

echo "done: log in as luna / luna-password"

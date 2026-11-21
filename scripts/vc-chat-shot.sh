#!/usr/bin/env bash
# Real-window check of the voice channel chat's message box (#201): runs the e2e test
# that types into the box and takes a screenshot of a real window after each text,
# in a HEADLESS sway of its own (no screen of the session is touched), and compares
# where the emoji button is drawn. Needs the homeserver (`just hs-up hs-seed`) and
# ../morpheus-e2e. Screenshots land in target/e2e/vc-chat-*.png.
# Usage: nix develop -c scripts/vc-chat-shot.sh [test name]
set -euo pipefail
test=${1:-the_emoji_button_is_drawn_in_the_same_place_whatever_is_typed}
export XDG_RUNTIME_DIR=/run/user/$(id -u)
hs=${MORPHEUS_HOMESERVER:-http://localhost:8008}
data=$(mktemp -d)
just e2e-build >&2
bin=$PWD/target/debug/morpheus-desktop-e2e
cat > "$data/sway.conf" <<CONF
output HEADLESS-1 mode 760x700
default_border none
CONF
nix shell nixpkgs#sway nixpkgs#grim -c bash -s -- "$data" "$bin" "$hs" "$test" <<'INNER'
set -u
data=$1 bin=$2 hs=$3 test=$4
before=$(ls "$XDG_RUNTIME_DIR" | grep '^wayland-[0-9]*$' | sort)
env -u WAYLAND_DISPLAY -u DISPLAY -u SWAYSOCK -u HYPRLAND_INSTANCE_SIGNATURE WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway -c "$data/sway.conf" >"$data/sway.log" 2>&1 & comp=$!
trap 'kill $comp 2>/dev/null' EXIT
for _ in $(seq 30); do
  sock=$(comm -13 <(echo "$before") <(ls "$XDG_RUNTIME_DIR" | grep '^wayland-[0-9]*$' | sort) | head -1)
  [ -n "$sock" ] && break; sleep 1
done
[ -n "${sock:-}" ] || { echo "no compositor socket"; tail -20 "$data/sway.log"; exit 1; }
export WAYLAND_DISPLAY=$sock
export SWAYSOCK=$(ls -t "$XDG_RUNTIME_DIR"/sway-ipc.* | head -1)
unset DISPLAY HYPRLAND_INSTANCE_SIGNATURE MORPHEUS_E2E_HEADLESS
MORPHEUS_E2E_BIN=$bin MORPHEUS_TEST_HOMESERVER=$hs scripts/e2e-cargo.sh --test voice_chat "$test" -- --nocapture --test-threads=1
INNER

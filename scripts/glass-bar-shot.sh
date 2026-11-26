#!/usr/bin/env bash
# Real-window check of the user bar's background under Transparent (#293): runs the e2e
# test that turns Transparent on and compares the pixels round the user bar with the
# ones above it, in a HEADLESS sway of its own (no screen of the session is touched)
# over a bright magenta wallpaper (swaybg). Needs the homeserver (`just hs-up hs-seed`)
# and ../morpheus-e2e. The screenshot lands in target/e2e/glass-user-bar.png.
# Usage: nix develop -c scripts/glass-bar-shot.sh [test name]
#        nix develop -c scripts/glass-bar-shot.sh <test file> <test name>
set -euo pipefail
file=glass
if [ $# -ge 2 ]; then file=$1; shift; fi
test=${1:-the_user_bar_band_matches_the_rail_and_channel_list_above_it}
export XDG_RUNTIME_DIR=/run/user/$(id -u)
hs=${MORPHEUS_HOMESERVER:-http://localhost:8008}
data=$(mktemp -d)
just e2e-build >&2
bin=$PWD/target/debug/morpheus-desktop-e2e
cat > "$data/sway.conf" <<CONF
output HEADLESS-1 mode 1280x800
default_border none
CONF
nix shell nixpkgs#sway nixpkgs#swaybg nixpkgs#grim -c bash -s -- "$data" "$bin" "$hs" "$test" "$file" <<'INNER'
set -u
data=$1 bin=$2 hs=$3 test=$4 file=$5
before=$(ls "$XDG_RUNTIME_DIR" | grep '^wayland-[0-9]*$' | sort)
env -u WAYLAND_DISPLAY -u DISPLAY -u SWAYSOCK -u HYPRLAND_INSTANCE_SIGNATURE WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway -c "$data/sway.conf" >"$data/sway.log" 2>&1 & comp=$!
bg=0
trap 'kill $bg $comp 2>/dev/null' EXIT
for _ in $(seq 30); do
  sock=$(comm -13 <(echo "$before") <(ls "$XDG_RUNTIME_DIR" | grep '^wayland-[0-9]*$' | sort) | head -1)
  [ -n "$sock" ] && break; sleep 1
done
[ -n "${sock:-}" ] || { echo "no compositor socket"; tail -20 "$data/sway.log"; exit 1; }
export WAYLAND_DISPLAY=$sock
export SWAYSOCK=$(ls -t "$XDG_RUNTIME_DIR"/sway-ipc.* | head -1)
swaybg -c ff00ff >/dev/null 2>&1 & bg=$!
unset DISPLAY HYPRLAND_INSTANCE_SIGNATURE MORPHEUS_E2E_HEADLESS
MORPHEUS_E2E_BIN=$bin MORPHEUS_TEST_HOMESERVER=$hs scripts/e2e-cargo.sh --test "$file" "$test" -- --nocapture --test-threads=1
INNER

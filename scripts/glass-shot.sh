#!/usr/bin/env bash
# Real-compositor check of transparency (#270) for a machine with a GPU, whose own
# session may be locked or in use: starts a HEADLESS sway of its own (no screen
# involved), a bright wallpaper (swaybg), the desktop app with glass on, and grabs
# the output with grim; prints pixels over the window's backgrounds. The session in
# use is never touched. Usage: nix develop -c scripts/glass-shot.sh [opacity] [out.png]
set -euo pipefail
opacity=${1:-30}
out=${2:-target/glass-shot.png}
export XDG_RUNTIME_DIR=/run/user/$(id -u)
data=$(mktemp -d)
printf '{"theme":"dark","glass":{"on":true,"opacity":%s}}' "$opacity" > "$data/theme.json"
cargo build -p morpheus-desktop --bin morpheus-desktop
bin=$PWD/target/debug/morpheus-desktop
cat > "$data/sway.conf" <<CONF
output HEADLESS-1 mode 1280x800
default_border none
CONF
nix shell nixpkgs#sway nixpkgs#swaybg nixpkgs#grim nixpkgs#imagemagick nixpkgs#jq -c bash -s -- "$data" "$out" "$bin" <<'INNER'
set -u
data=$1 out=$2 bin=$3
before=$(ls "$XDG_RUNTIME_DIR" | grep '^wayland-[0-9]*$' | sort)
env -u WAYLAND_DISPLAY -u DISPLAY -u SWAYSOCK WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway -c "$data/sway.conf" >"$data/sway.log" 2>&1 & comp=$!
app=0 bg=0
trap 'kill $app $bg $comp 2>/dev/null' EXIT
for _ in $(seq 30); do
  sock=$(comm -13 <(echo "$before") <(ls "$XDG_RUNTIME_DIR" | grep '^wayland-[0-9]*$' | sort) | head -1)
  [ -n "$sock" ] && break; sleep 1
done
[ -n "${sock:-}" ] || { echo "no compositor socket"; tail -20 "$data/sway.log"; exit 1; }
export WAYLAND_DISPLAY=$sock
export SWAYSOCK=$(ls -t "$XDG_RUNTIME_DIR"/sway-ipc.* | head -1)
echo "sway on $WAYLAND_DISPLAY ($SWAYSOCK)"
swaybg -c ff00ff >/dev/null 2>&1 & bg=$!
MORPHEUS_DATA_DIR=$data "$bin" >"$data/app.log" 2>&1 & app=$!
for _ in $(seq 90); do
  geo=$(swaymsg -t get_tree | jq -r --argjson p $app '.. | objects | select(.pid? == $p) | "\(.rect.x) \(.rect.y) \(.rect.width) \(.rect.height)"' | head -1)
  [ -n "$geo" ] && break; sleep 1
done
echo "window: $geo"; sleep 5
grim "$out"
read -r x y w h <<<"$geo"
echo "output $(magick identify -format '%wx%h' "$out"), window at $x,$y size ${w}x${h}"
for p in "5,5" "$((x + 30)),$((y + h / 2))" "$((x + w / 2)),$((y + h / 2))" "$((x + w - 30)),$((y + h - 30))"; do
  echo "pixel $p: $(magick "$out" -format "%[pixel:p{$p}]" info:)"
done
tail -3 "$data/app.log"
INNER

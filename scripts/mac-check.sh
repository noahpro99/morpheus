#!/usr/bin/env bash
# What a Mac user expects of the app, checked on GitHub's Macs against the
# built Morpheus.app: an icon of its own; a menu bar with an Edit menu that
# pastes (GPUI's menu, mac_menu.rs; the Dioxus app needed it for ⌘V in its
# web view); ⌘W closing the window; and clicking the app again (as the Dock
# does) bringing it back; and one title bar, the traffic lights on the app's
# bar, that drags the window (scripts/mac-mouse.swift moves the mouse).
#
#   scripts/mac-check.sh path/to/Morpheus.app
set -uo pipefail
app="$1"
failed=0
fail() { echo "FAIL: $*"; failed=1; }

# The icon: Info.plist names an .icns the bundle has.
icon=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIconFile" "$app/Contents/Info.plist" 2>/dev/null || true)
icon_file="$app/Contents/Resources/${icon%.icns}.icns"
if [ -z "$icon" ] || [ ! -s "$icon_file" ]; then
    fail "no app icon (CFBundleIconFile '${icon}')"
else
    echo "ok: icon $icon_file"
fi

data=$(mktemp -d)
MORPHEUS_LOG_WINDOW_MOVE=1 MORPHEUS_DATA_DIR="$data" "$app/Contents/MacOS/morpheus-desktop" > "$data/log.txt" 2>&1 &
pid=$!
sleep 15
if ! kill -0 "$pid" 2>/dev/null; then
    fail "the app quit on start"; cat "$data/log.txt"; exit 1
fi

# osascript waits for ever on a permission prompt no one can answer here.
osa() { perl -e "alarm 20; exec @ARGV" osascript "$@" 2>&1 || echo "(osascript failed or timed out)"; }
ui() { osa -e "tell application \"System Events\" to tell (first process whose unix id is $pid) to $1"; }
windows() { ui "count windows"; }

menus=$(ui "get name of menu bar items of menu bar 1")
echo "menu bar: $menus"
case "$menus" in
    *"assistive access"*|*"not allowed"*|*"timed out"*) echo "System Events may not script apps here: $menus"; kill "$pid"; exit 1 ;;
esac
edit=$(ui "get name of menu items of menu \"Edit\" of menu bar item \"Edit\" of menu bar 1")
case "$edit" in
    *Paste*) echo "ok: Edit has Paste ($edit)" ;;
    *) fail "no Edit menu with Paste ($edit)" ;;
esac

osa -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true" >/dev/null 2>&1
sleep 2

# One title bar (#29): the system's is transparent with the traffic lights
# on the app's 32 px bar, at its left; the app draws no window buttons of its
# own and starts its content after the lights; dragging the bar moves the
# window and a double click zooms it.
chrome=$(grep -m1 "window chrome:" "$data/log.txt")
echo "$chrome"
case "$chrome" in
    *"buttons native, inset 78,"*) echo "ok: no app-drawn window buttons, content inset 78 px" ;;
    *) fail "the app did not report native buttons and a 78 px inset ($chrome)" ;;
esac
frame=$(echo "$chrome" | sed -n 's/.*frame \([0-9.x]*\),.*/\1/p')
content=$(echo "$chrome" | sed -n 's/.*content \([0-9.x]*\)$/\1/p')
if [ -n "$frame" ] && [ "$frame" = "$content" ]; then
    echo "ok: the content reaches the window's top (frame $frame = content $content)"
else
    fail "the content sits under a system title bar (frame $frame, content $content)"
fi
xy() { tr -d ',' | awk '{ printf "%d %d", $1, $2 }'; }
read -r wx wy <<< "$(ui "get position of window 1" | xy)"
read -r bx by <<< "$(ui "get position of (first button of window 1 whose subrole is \"AXCloseButton\")" | xy)"
read -r bw bh <<< "$(ui "get size of (first button of window 1 whose subrole is \"AXCloseButton\")" | xy)"
echo "window at $wx,$wy; close button at $bx,$by size ${bw}x${bh}"
if [ -n "$bh" ] && [ $((by - wy + bh / 2)) -ge 13 ] && [ $((by - wy + bh / 2)) -le 19 ] \
    && [ $((bx - wx)) -ge 4 ] && [ $((bx - wx)) -le 24 ]; then
    echo "ok: traffic lights at the bar's left, centred in it (centre $((by - wy + bh / 2)) px down)"
else
    fail "traffic lights not at the left of the 32 px bar (close button $((bx - wx)),$((by - wy)) from the window's corner)"
fi
mouse() { perl -e "alarm 120; exec @ARGV" swift "$(dirname "$0")/mac-mouse.swift" "$@" 2>&1; }
# In the system bar's own height, below it (still the app's bar), and in it again.
for down in 14 30 14; do
    read -r x0 y0 <<< "$(ui "get position of window 1" | xy)"
    moves0=$(grep -c "start_window_move" "$data/log.txt")
    mouse drag $((x0 + 220)) $((y0 + down)) 100 60
    sleep 1
    read -r x1 y1 <<< "$(ui "get position of window 1" | xy)"
    echo "the app's own drag started $(( $(grep -c "start_window_move" "$data/log.txt") - moves0 )) time(s): $(grep "start_window_move" "$data/log.txt" | tail -1)"
    if [ $((x1 - x0)) -ge 90 ] && [ $((x1 - x0)) -le 110 ] && [ $((y1 - y0)) -ge 50 ] && [ $((y1 - y0)) -le 70 ]; then
        echo "ok: dragging the bar ${down} px down moved the window ($x0,$y0 → $x1,$y1)"
    else
        fail "dragging the bar ${down} px down did not move the window by 100,60 ($x0,$y0 → $x1,$y1)"
    fi
done
read -r x0 y0 <<< "$(ui "get position of window 1" | xy)"
s0=$(ui "get size of window 1")
mouse doubleclick $((x0 + 220)) $((y0 + 16))
sleep 2
s1=$(ui "get size of window 1")
if [ -n "$s0" ] && [ "$s0" != "$s1" ]; then
    echo "ok: double-clicking the bar zoomed the window ($s0 → $s1)"
else
    fail "double-clicking the bar did not zoom the window ($s0 → $s1)"
fi

# ⌘W, as a person presses it, closes the window (the app keeps running).
osa -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true" >/dev/null 2>&1
sleep 2
before=$(windows)
echo "frontmost: $(osa -e 'tell application "System Events" to get name of first process whose frontmost is true')"
# Tried a few times: on a busy runner the first keystroke can land before the
# window is key.
for _ in 1 2 3; do
    osa -e 'tell application "System Events" to keystroke "w" using command down'
    sleep 2
    after=$(windows)
    [ "$after" = 0 ] && break
done
if [ "$before" -ge 1 ] 2>/dev/null && [ "$after" = 0 ] && kill -0 "$pid" 2>/dev/null; then
    echo "ok: ⌘W closed the window ($before → $after), still running"
else
    fail "⌘W did not close the window (windows $before → $after)"
    echo "--- app log ---"; tail -40 "$data/log.txt"
fi

# Clicking the app again (the Dock's reopen) shows the window.
open "$app"
sleep 3
again=$(windows)
if [ "$again" -ge 1 ] 2>/dev/null; then
    echo "ok: reopening showed the window ($again)"
else
    fail "reopening did not show the window (windows $again)"
fi

kill "$pid" 2>/dev/null
exit $failed

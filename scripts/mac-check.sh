#!/usr/bin/env bash
# What a Mac user expects of the app, checked on GitHub's Macs against the
# built Morpheus.app: an icon of its own; a menu bar with an Edit menu that
# pastes (GPUI's menu, mac_menu.rs; the Dioxus app needed it for ⌘V in its
# web view); ⌘W closing the window; and clicking the app again (as the Dock
# does) bringing it back. All of it applies to the GPUI app as it did before.
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
MORPHEUS_DATA_DIR="$data" "$app/Contents/MacOS/morpheus-desktop" > "$data/log.txt" 2>&1 &
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

# ⌘W, as a person presses it, closes the window (the app keeps running).
osa -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true" >/dev/null 2>&1
sleep 2
before=$(windows)
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

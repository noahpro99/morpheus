#!/usr/bin/env bash
# Does an update work on this platform? Takes a built release zip (what the
# release job made: morpheus-<version>-<platform>.zip), unpacks it as the
# installed app, makes a "new release" of it (the same program plus one file
# the old one lacks) behind a local feed, and starts the installed app in its
# self-test mode (apps/desktop/src/update_self_test.rs: it pretends to be
# 0.0.1, installs the update through the real swap code, and the program it
# starts writes a marker). Then checks: the marker, the new files in place,
# nothing of the update (.old, the unpacked download) left behind.
#
#   scripts/update-self-test.sh path/to/morpheus-0.2.0-linux-x86_64.zip
# Runs on Linux, macOS and Windows (Git Bash); needs python for the feed.
set -euo pipefail

zip=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d)
server=""
cleanup() { [ -n "$server" ] && kill "$server" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT
mkdir "$work/old" "$work/new" "$work/serve" "$work/data"

unpack() { # zip dir
    if command -v ditto >/dev/null; then ditto -x -k "$1" "$2"
    elif command -v unzip >/dev/null; then unzip -q "$1" -d "$2"
    else tar -xf "$1" -C "$2"; fi
}
pack() { # dir (its contents become the zip's top level) zip
    if command -v ditto >/dev/null; then (cd "$1" && ditto -c -k . "$2")
    elif command -v zip >/dev/null; then (cd "$1" && zip -qr "$2" .)
    else tar -a -c -f "$2" -C "$1" .; fi
}
native() { cygpath -w "$1" 2>/dev/null || echo "$1"; }

unpack "$zip" "$work/old"
unpack "$zip" "$work/new"

# The installed program, and where the new release gets its extra file.
exe=$(find "$work/old" \( -path '*/Morpheus.app/Contents/MacOS/morpheus-desktop' -o -name Morpheus.exe -o -name morpheus-desktop \) -type f | head -1)
[ -n "$exe" ] || { echo "no program in $zip"; find "$work/old"; exit 1; }
rel=${exe#"$work/old/"}
case "$rel" in
    *Morpheus.app/*) extra_dir="$work/new/Morpheus.app/Contents/Resources" ;;
    *) extra_dir="$work/new/$(dirname "$rel")" ;;
esac
echo "new release in this update" > "$extra_dir/update-self-test.txt"
if [ -d "$work/new/Morpheus.app" ]; then codesign --force --deep --sign - "$work/new/Morpheus.app"; fi
asset=$(basename "$zip")
pack "$work/new" "$work/serve/$asset"

port=$((20000 + RANDOM % 20000))
cat > "$work/serve/feed.json" <<JSON
{"version": "99.0.0", "url": "http://127.0.0.1:$port/",
 "assets": [{"name": "$asset", "browser_download_url": "http://127.0.0.1:$port/$asset"}]}
JSON
py=$(command -v python3 || command -v python)
(cd "$work/serve" && exec "$py" -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1) &
server=$!
for _ in $(seq 50); do curl -sf "http://127.0.0.1:$port/feed.json" >/dev/null && break; sleep 0.2; done

marker="$work/marker.txt"
echo "starting the installed app: $exe"
set +e
MORPHEUS_UPDATE_SELF_TEST=1 \
MORPHEUS_UPDATE_FEED="http://127.0.0.1:$port/feed.json" \
MORPHEUS_DATA_DIR="$(native "$work/data")" \
MORPHEUS_UPDATE_SELF_TEST_MARKER="$(native "$marker")" \
    "$exe"
code=$?
set -e
[ "$code" = 0 ] || { echo "FAIL: the old version exited with $code"; exit 1; }

# The new version starts on its own; it writes the marker once it has cleaned up.
for _ in $(seq 120); do [ -f "$marker" ] && break; sleep 0.5; done
failed=0
fail() { echo "FAIL: $*"; failed=1; }
[ -f "$marker" ] && echo "ok: the new version ran: $(cat "$marker")" || fail "the new version never wrote its marker"
[ -f "$exe" ] && echo "ok: the program is in place" || fail "the program is gone"
if [ -d "$work/old/Morpheus.app" ]; then
    [ -f "$work/old/Morpheus.app/Contents/Resources/update-self-test.txt" ] && echo "ok: the new bundle is in place" || fail "the old bundle was not replaced"
else
    [ -f "$(dirname "$exe")/update-self-test.txt" ] && echo "ok: the new files are beside the program" || fail "the new release's file is missing"
fi
leftover=$(find "$work/old" \( -name '*.old' -o -name '.morpheus-update*' \) | head)
[ -z "$leftover" ] && echo "ok: nothing of the update left behind" || fail "left behind: $leftover"
exit $failed

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
# A release with a launcher (Windows: Morpheus.exe beside app/) is tried twice
# more. The new release's launcher differs from the installed one by a byte,
# so the first run shows that an update leaves the launcher alone (the
# program pins and shortcuts point at) and replaces only app/, and that the
# launcher then starts the new version. The second starts from the old
# single-program layout and shows it migrates: the launcher in the old
# program's place, app/ beside it.
#
#   scripts/update-self-test.sh path/to/morpheus-0.2.0-linux-x86_64.zip
# Runs on Linux, macOS and Windows (Git Bash); needs python for the feed.
set -euo pipefail
# Info-ZIP reads options from $ZIP and $ZIPOPT.
unset ZIP ZIPOPT

zip=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d)
server=""
cleanup() {
    # Stop the file server and wait for it to exit first: on Windows it holds
    # its folder open, so removing the folder under it fails. Then retry the rm,
    # and never let cleanup alone fail the test.
    if [ -n "$server" ]; then
        kill "$server" 2>/dev/null || true
        wait "$server" 2>/dev/null || true
    fi
    cd / 2>/dev/null || true
    for _ in 1 2 3 4 5 6; do
        rm -rf "$work" 2>/dev/null
        [ -e "$work" ] || break
        sleep 1
    done
    [ -e "$work" ] && echo "note: could not remove $work" >&2
    return 0
}
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
    elif command -v 7z >/dev/null; then (cd "$1" && 7z a -tzip -bso0 "$(native "$2")" .)
    else echo "no zip, ditto or 7z to make the release zip" >&2; exit 1; fi
}
native() { cygpath -w "$1" 2>/dev/null || echo "$1"; }
sum() { cksum < "$1"; }

unpack "$zip" "$work/old"
unpack "$zip" "$work/new"

# A release with a launcher: Morpheus.exe beside app/morpheus-desktop.exe.
app_exe=$(find "$work/old" -path '*/app/morpheus-desktop.exe' -type f | head -1)
if [ -n "$app_exe" ]; then
    launcher_dir=$(dirname "$(dirname "$app_exe")")
    exe="$launcher_dir/Morpheus.exe"
    [ -f "$exe" ] || { echo "no launcher beside $app_exe"; find "$work/old"; exit 1; }
    rel=${launcher_dir#"$work/old/"}
    new_launcher="$work/new/$rel/Morpheus.exe"
    # A different launcher in the new release: an update that replaced it
    # would show.
    printf 'x' >> "$new_launcher"
    extra_dir="$work/new/$rel/app"
else
    # The installed program, and where the new release gets its extra file.
    exe=$(find "$work/old" \( -path '*/Morpheus.app/Contents/MacOS/morpheus-desktop' -o -name Morpheus.exe -o -name morpheus-desktop \) -type f | head -1)
    [ -n "$exe" ] || { echo "no program in $zip"; find "$work/old"; exit 1; }
    rel=${exe#"$work/old/"}
    case "$rel" in
        *Morpheus.app/*) extra_dir="$work/new/Morpheus.app/Contents/Resources" ;;
        *) extra_dir="$work/new/$(dirname "$rel")" ;;
    esac
fi
echo "new release in this update" > "$extra_dir/update-self-test.txt"
if [ -d "$work/new/Morpheus.app" ]; then codesign --force --deep --sign - "$work/new/Morpheus.app"; fi
asset=$(basename "$zip")
pack "$work/new" "$work/serve/$asset"

port=$((20000 + RANDOM % 20000))
cat > "$work/serve/feed.json" <<JSON
{"tag_name": "v99.0.0", "url": "https://api.github.com/repos/example/releases/1", "html_url": "http://127.0.0.1:$port/",
 "assets": [{"name": "$asset", "browser_download_url": "http://127.0.0.1:$port/$asset"}]}
JSON
py=$(command -v python3 || command -v python)
(cd "$work/serve" && exec "$py" -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1) &
server=$!
for _ in $(seq 50); do curl -sf "http://127.0.0.1:$port/feed.json" >/dev/null && break; sleep 0.2; done

failed=0
fail() { echo "FAIL: $*"; failed=1; }

# Start the installed program in self-test mode (a launcher waits for the app
# and passes its exit code on) and wait for the new version's marker.
run_update() { # program marker
    echo "starting the installed app: $1"
    set +e
    MORPHEUS_UPDATE_SELF_TEST=1 \
    MORPHEUS_LAUNCHER_WAIT=1 \
    MORPHEUS_UPDATE_FEED="http://127.0.0.1:$port/feed.json" \
    MORPHEUS_DATA_DIR="$(native "$work/data")" \
    MORPHEUS_UPDATE_SELF_TEST_MARKER="$(native "$2")" \
        "$1"
    code=$?
    set -e
    [ "$code" = 0 ] || { echo "FAIL: the old version exited with $code"; exit 1; }
    # The new version starts on its own; it writes the marker once it has cleaned up.
    for _ in $(seq 120); do [ -f "$2" ] && break; sleep 0.5; done
    [ -f "$2" ] && echo "ok: the new version ran: $(cat "$2")" || fail "the new version never wrote its marker"
}
no_leftovers() { # folder
    leftover=$(find "$1" \( -name '*.old' -o -name '.morpheus-update*' \) | head)
    [ -z "$leftover" ] && echo "ok: nothing of the update left behind" || fail "left behind: $leftover"
}

marker="$work/marker.txt"
launcher_sum=""
[ -n "$app_exe" ] && launcher_sum=$(sum "$exe")
run_update "$exe" "$marker"
[ -f "$exe" ] && echo "ok: the program is in place" || fail "the program is gone"
if [ -d "$work/old/Morpheus.app" ]; then
    [ -f "$work/old/Morpheus.app/Contents/Resources/update-self-test.txt" ] && echo "ok: the new bundle is in place" || fail "the old bundle was not replaced"
elif [ -n "$app_exe" ]; then
    [ -f "$launcher_dir/app/update-self-test.txt" ] && echo "ok: the new files are in app/" || fail "the new release's file is missing from app/"
    [ "$(sum "$exe")" = "$launcher_sum" ] && [ "$(sum "$exe")" != "$(sum "$new_launcher")" ] \
        && echo "ok: the launcher is the one that was installed, untouched by the update" \
        || fail "the update replaced the launcher"
    grep -qi 'app.morpheus-desktop' "$marker" && echo "ok: the launcher started the app in app/" || fail "the new version did not run from app/: $(cat "$marker")"
else
    [ -f "$(dirname "$exe")/update-self-test.txt" ] && echo "ok: the new files are beside the program" || fail "the new release's file is missing"
fi
no_leftovers "$work/old"

if [ -n "$app_exe" ]; then
    # The old layout: one program, Morpheus.exe, and no app/.
    echo "--- migrating from the single-program layout"
    mkdir "$work/single"
    cp "$app_exe" "$work/single/Morpheus.exe"
    cp "$app_exe" "$work/old-app.exe"
    marker2="$work/marker-migrated.txt"
    run_update "$work/single/Morpheus.exe" "$marker2"
    [ "$(sum "$work/single/Morpheus.exe")" = "$(sum "$new_launcher")" ] && echo "ok: the launcher took the old program's place" || fail "Morpheus.exe is not the launcher"
    [ -f "$work/single/app/morpheus-desktop.exe" ] && echo "ok: the app is in app/" || fail "no app/morpheus-desktop.exe after migrating"
    [ -f "$work/single/app/update-self-test.txt" ] && echo "ok: the new files are in app/" || fail "the new release's file is missing from app/"
    grep -qi 'app.morpheus-desktop' "$marker2" && echo "ok: the launcher started the app in app/" || fail "the migrated install did not run from app/: $(cat "$marker2")"
    no_leftovers "$work/single"
fi
exit $failed

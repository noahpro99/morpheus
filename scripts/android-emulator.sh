#!/usr/bin/env bash
# The virtual phone the Android e2e runs use: headless by default (no window, no sound), from a quick-boot
# snapshot so a restart after a crash takes seconds, not a cold boot. Does nothing when it is already up.
#
#   scripts/android-emulator.sh               start headless if none is running, wait until booted
#   scripts/android-emulator.sh --window      with a window (to watch it), no snapshot saved
#   scripts/android-emulator.sh --cold        cold boot (ignore the snapshot) and save a new one
#   scripts/android-emulator.sh --stop        stop it
#   scripts/android-emulator.sh --save        save the quick-boot snapshot from the running (idle) emulator now
#
# Environment: MORPHEUS_EMULATOR_PORT (default 5554, the serial is emulator-<port>),
# MORPHEUS_EMULATOR_GPU (default swiftshader_indirect: software, needs no display and no GPU driver; `host`
# works headless here too but is not faster for these tests, see docs/ANDROID.md), MORPHEUS_AVD (default morpheus).
# Only ever talks to emulator-<port> (adb -s), never to a phone.
#
# The snapshot ("headless") is saved once after the first clean boot and loaded read-only afterwards
# (-no-snapshot-save: every start begins from that clean state, and a crash cannot corrupt it). It is
# dropped and rebuilt by --cold, or when the AVD or emulator changed (the emulator then cold boots by itself).
set -euo pipefail
cd "$(dirname "$0")/.."
case "${ANDROID_SERIAL:-}" in emulator-*) port=${MORPHEUS_EMULATOR_PORT:-${ANDROID_SERIAL#emulator-}} ;; *) port=${MORPHEUS_EMULATOR_PORT:-5554} ;; esac # a plugged-in phone in ANDROID_SERIAL is never touched
serial=emulator-$port
avd=${MORPHEUS_AVD:-morpheus}
gpu=${MORPHEUS_EMULATOR_GPU:-swiftshader_indirect}
snap=headless
window=0 cold=0 stop=0 save=0
for a in "$@"; do
    case $a in
        --window) window=1 ;;
        --cold) cold=1 ;;
        --stop) stop=1 ;;
        --save) save=1 ;;
        *) echo "unknown option $a" >&2; exit 2 ;;
    esac
done

# Inside the Android shell already? If not, enter it.
if ! command -v emulator >/dev/null 2>&1 || ! command -v adb >/dev/null 2>&1; then
    exec nix develop .#android-emulator -c "$0" "$@"
fi
export LD_LIBRARY_PATH=${LD_LIBRARY_PATH:-}:/run/opengl-driver/lib
adbs() { adb -s "$serial" "$@"; }
listed() { adb devices | awk -v s="$serial" '$1 == s { found = 1 } END { exit !found }'; }
booted() { [ "$(adbs shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; }
snapdir=$HOME/.android/avd/$avd.avd/snapshots/$snap

if [ "$stop" = 1 ]; then
    listed && timeout 30 adb -s "$serial" emu kill >/dev/null 2>&1 || true
    for _ in $(seq 1 60); do listed || break; sleep 0.5; done
    echo "emulator: stopped"
    exit 0
fi

started=$(date +%s.%N)
ours=0
if ! listed; then
    ours=1
    mkdir -p target
    args=(${MORPHEUS_EMULATOR_ARGS:-} -avd "$avd" -port "$port" -no-audio -no-boot-anim -netdelay none -netspeed full -gpu "$gpu" -no-snapshot-save)
    [ "$window" = 1 ] || args+=(-no-window)
    if [ "$cold" = 1 ]; then
        rm -rf "$snapdir"
        args+=(-no-snapshot-load)
    elif [ -d "$snapdir" ] && [ "$window" = 0 ]; then
        args+=(-snapshot "$snap")
    else
        args+=(-no-snapshot-load)
    fi
    echo "emulator: starting ${args[*]}"
    # The emulator outlives this job: it must not inherit the job's lane locks (fds 8, 9), or the lane stays locked while it runs.
    TMPDIR=/tmp setsid nohup emulator "${args[@]}" >target/emulator-"$port".log 2>&1 </dev/null 8>&- 9>&- &
    adb start-server >/dev/null 2>&1 || true
fi

for _ in $(seq 1 360); do # up to 3 minutes
    if listed && booted; then break; fi
    sleep 0.5
done
booted || { echo "emulator: not booted after 3 minutes (see target/emulator-$port.log)" >&2; exit 1; }
adbs shell settings put global window_animation_scale 0 >/dev/null 2>&1 || true
adbs shell settings put global transition_animation_scale 0 >/dev/null 2>&1 || true
adbs shell settings put global animator_duration_scale 0 >/dev/null 2>&1 || true
echo "emulator: booted in $(awk -v a="$started" -v b="$(date +%s.%N)" 'BEGIN { printf "%.1f", b - a }') s ($serial)"

# First clean boot (one this script started, never somebody else's running phone): save the quick-boot snapshot (not for a windowed run).
if [ "$window" = 0 ] && { [ "$save" = 1 ] || { [ "$ours" = 1 ] && [ ! -d "$snapdir" ]; }; }; then
    sleep 15 # let the first-boot work settle before it is frozen into the snapshot
    adbs emu avd snapshot save "$snap" >target/emulator-snapshot.log 2>&1 && echo "emulator: quick-boot snapshot '$snap' saved" \
        || echo "emulator: could not save the snapshot (continuing without)" >&2
fi

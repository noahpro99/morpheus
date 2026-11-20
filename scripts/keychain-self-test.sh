#!/usr/bin/env bash
# macOS: how often one launch of the packaged app asks the keychain (#159).
# Runs the app's headless keychain self-test against a throwaway keychain (made
# and unlocked here, so nothing prompts and the runner's own is untouched):
# the first run saves three accounts, the second acts out a launch with them
# and must read the keychain once and write nothing. The debug log must show
# the same count of "keychain access" lines.
# Usage: scripts/keychain-self-test.sh path/to/Morpheus.app
set -euo pipefail
app=${1:?usage: keychain-self-test.sh Morpheus.app}
exe="$app/Contents/MacOS/morpheus-desktop"
work=$(mktemp -d)
kc="$work/ci.keychain-db"
old_default=$(security default-keychain | tr -d ' "')
cleanup() {
  security default-keychain -s "$old_default" || true
  security delete-keychain "$kc" || true
}
trap cleanup EXIT
security create-keychain -p ci "$kc"
security set-keychain-settings -t 3600 "$kc"
security unlock-keychain -p ci "$kc"
security list-keychains -d user -s "$kc"
security default-keychain -s "$kc"

run() { # mode, log
  MORPHEUS_KEYCHAIN_SELF_TEST="$1" MORPHEUS_KEYCHAIN_SELF_TEST_OUT="$work/$1.txt" \
    MORPHEUS_SESSION_STORE=keyring MORPHEUS_DATA_DIR="$work/data" RUST_LOG=morpheus=debug \
    perl -e 'alarm 120; exec @ARGV' "$exe" > "$work/$1.log" 2>&1
  cat "$work/$1.txt"
}
run seed
run launch
grep -q "keychain self-test: ok: seeded=false reads=1 writes=0 deletes=0 accounts=3" "$work/launch.txt"
accesses=$(grep -c "keychain access" "$work/launch.log" || true)
echo "keychain access lines in the debug log for one launch: $accesses"
test "$accesses" -eq 1
echo "one keychain read for a launch with 3 accounts, no writes"

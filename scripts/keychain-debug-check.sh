#!/usr/bin/env bash
# macOS (#159): a DEBUG build of the app, started headless against a fresh,
# locked temporary keychain, makes no Security.framework keychain item call
# (SecItemCopyMatching/Add/Update/Delete), which would ask for the login
# password. Every such call is logged with its call stack, and the script
# fails and prints them if there is one. Usage: scripts/keychain-debug-check.sh [binary]
set -euo pipefail
exe=${1:-target/debug/morpheus-desktop}
work=$(mktemp -d)
here=$(cd "$(dirname "$0")" && pwd)
clang -dynamiclib -o "$work/keychain-trace.dylib" "$here/keychain-trace.c" -framework Security -framework CoreFoundation
# A keychain nobody can read from: locked, and the only one searched.
kc="$work/test.keychain-db"
security create-keychain -p x "$kc"
security list-keychains -d user -s "$kc"
security default-keychain -s "$kc"
security lock-keychain "$kc"
out="$work/trace.txt"; : > "$out"
MORPHEUS_KEYCHAIN_TRACE_OUT="$out" DYLD_INSERT_LIBRARIES="$work/keychain-trace.dylib" \
  MORPHEUS_DATA_DIR="$work/data" RUST_LOG=morpheus=debug \
  "$exe" > "$work/app.log" 2>&1 &
pid=$!
sleep "${KEYCHAIN_CHECK_SECONDS:-25}"
if ! kill -0 "$pid" 2>/dev/null; then echo "the app quit early:"; tail -30 "$work/app.log"; exit 1; fi
kill "$pid" || true
if [ -s "$out" ]; then
  echo "The debug build called the keychain:"; cat "$out"
  exit 1
fi
echo "keychain debug check: ok: no keychain call in a debug build"

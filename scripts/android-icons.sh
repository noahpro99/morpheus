#!/usr/bin/env bash
# Android icons: Material Symbols Rounded (Apache 2.0, google/material-design-icons), weight 400, grade 0,
# optical size 24, as vector drawables res/drawable/ic_ms_<symbol>.xml (outlined) and ic_ms_<symbol>_fill.xml
# (filled: selected / active states). Usage: scripts/android-icons.sh <symbol>... (fetches those; the
# Lucide name -> symbol table is scripts/android-icons.map, the desktop keeps Lucide).
set -euo pipefail
cd "$(dirname "$0")/.."
out=apps/android/app/src/main/res/drawable
base=https://raw.githubusercontent.com/google/material-design-icons/master/symbols/web
for s in "$@"; do
    for v in "" _fill; do
        svg=$(curl -sf "$base/$s/materialsymbolsrounded/${s}${v/_fill/_fill1}_24px.svg")
        d=$(sed -n 's/.*<path d="\([^"]*\)".*/\1/p' <<<"$svg")
        cat > "$out/ic_ms_${s}${v}.xml" <<X
<?xml version="1.0" encoding="utf-8"?>
<!-- Material Symbols Rounded "$s"${v:+ (filled)}, Apache 2.0. Tinted by the Icon. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="24dp" android:height="24dp" android:viewportWidth="960" android:viewportHeight="960">
    <path android:fillColor="#FFFFFFFF" android:pathData="$d" android:translateY="960" />
</vector>
X
    done
done

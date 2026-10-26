#!/usr/bin/env bash
# Usage: scripts/make-dmg.sh PicoBar.app PicoBar-x.y.z-macos-universal.dmg
# Builds a compressed disk image with the app and an Applications shortcut.
set -euo pipefail
APP="$1"
OUT="$2"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$OUT"
hdiutil create -quiet -volname "PicoBar" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$OUT"
echo "created $OUT"

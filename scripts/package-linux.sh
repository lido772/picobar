#!/usr/bin/env bash
# Usage: scripts/package-linux.sh <version> <binary> <outdir>
# Builds PicoBar-<version>-linux-x86_64.tar.gz (portable; the built-in
# updater replaces `picobar` in place) and picobar_<version>_amd64.deb.
set -euo pipefail
VERSION="$1"
BIN="$2"
OUT="$3"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$OUT"

# Portable archive: files at the top level.
mkdir -p "$STAGE/tar"
install -m 755 "$BIN" "$STAGE/tar/picobar"
install -m 644 "$ROOT/packaging/linux/picobar.desktop" "$STAGE/tar/picobar.desktop"
install -m 644 "$ROOT/LICENSE" "$STAGE/tar/LICENSE"
tar -C "$STAGE/tar" -czf "$OUT/PicoBar-$VERSION-linux-x86_64.tar.gz" picobar picobar.desktop LICENSE

# Debian package.
DEB="$STAGE/deb"
install -D -m 755 "$BIN" "$DEB/usr/bin/picobar"
install -D -m 644 "$ROOT/packaging/linux/picobar.desktop" "$DEB/usr/share/applications/picobar.desktop"
install -D -m 644 "$ROOT/LICENSE" "$DEB/usr/share/doc/picobar/copyright"
SIZE_KB=$(du -sk "$DEB" | cut -f1)
mkdir -p "$DEB/DEBIAN"
cat > "$DEB/DEBIAN/control" <<EOF
Package: picobar
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Installed-Size: $SIZE_KB
Depends: libgtk-3-0 (>= 3.22), libayatana-appindicator3-1 | libappindicator3-1
Maintainer: Walid Ben Miled <lido772@gmail.com>
Homepage: https://github.com/lido772/picobar
Description: Ultra-light tray monitor for Claude and OpenCode usage
 Shows your Claude session and weekly limits, reset countdowns, token usage,
 trends and which coding agents are waiting for you, in the system tray.
EOF
dpkg-deb --build --root-owner-group "$DEB" "$OUT/picobar_${VERSION}_amd64.deb" >/dev/null
ls -l "$OUT"

#!/bin/sh
# Installs the latest PicoBar.app on macOS from the GitHub release DMG, checked against SHA256SUMS.txt.
# Usage: curl -fsSL https://raw.githubusercontent.com/lido772/picobar/main/install.sh | sh
set -eu

if [ "$(uname -s)" != Darwin ]; then
  echo "PicoBar's install script is for macOS. See https://github.com/lido772/picobar#install" >&2
  exit 1
fi

tag=$(curl -fsSL https://api.github.com/repos/lido772/picobar/releases/latest | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)
if [ -z "$tag" ]; then
  echo "Could not find the latest PicoBar release. Try again later or see https://github.com/lido772/picobar/releases" >&2
  exit 1
fi
version=${tag#v}

tmp=$(mktemp -d)
trap 'hdiutil detach -quiet "$tmp/mnt" 2>/dev/null || true; rm -rf "$tmp"' EXIT

base="https://github.com/lido772/picobar/releases/download/$tag"
dmg="PicoBar-$version-macos-universal.dmg"
curl -fsSL -o "$tmp/$dmg" "$base/$dmg"
curl -fsSL -o "$tmp/SHA256SUMS.txt" "$base/SHA256SUMS.txt"

# SHA256SUMS lines are "<hash>  <file>" (two spaces).
expected=$(grep " PicoBar-$version-macos-universal.dmg\$" "$tmp/SHA256SUMS.txt" | cut -d' ' -f1)
actual=$(shasum -a 256 "$tmp/$dmg" | cut -d' ' -f1)
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  echo "Checksum mismatch, aborting." >&2
  exit 1
fi

mkdir "$tmp/mnt"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$tmp/mnt" "$tmp/$dmg"

dest=/Applications
if [ ! -w /Applications ]; then
  dest="$HOME/Applications"
  mkdir -p "$dest"
fi

pkill -x picobar 2>/dev/null || true
sleep 1

rm -rf "$dest/PicoBar.app"
ditto "$tmp/mnt/PicoBar.app" "$dest/PicoBar.app"
xattr -dr com.apple.quarantine "$dest/PicoBar.app" 2>/dev/null || true

open "$dest/PicoBar.app"
echo "PicoBar $version installed in $dest. Use Settings > Launch at login in its menu to start it at login."

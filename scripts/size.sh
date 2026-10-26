#!/usr/bin/env bash
# Release build + binary size check against the PicoBar budget.
set -euo pipefail
BUDGET_BYTES=$((3 * 1024 * 1024))

cargo build --release --quiet
BIN=target/release/picobar
[[ -f "$BIN.exe" ]] && BIN="$BIN.exe"

SIZE=$(wc -c < "$BIN" | tr -d ' ')
KB=$((SIZE / 1024))
echo "binary: $BIN  ${KB} KB  (budget $((BUDGET_BYTES / 1024)) KB)"

if (( SIZE > BUDGET_BYTES )); then
  echo "FAIL: binary exceeds budget" >&2
  exit 1
fi
echo "PASS"

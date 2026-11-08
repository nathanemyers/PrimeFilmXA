#!/usr/bin/env bash
# Probe the PrimeFilm XA with only the SANE pieusb backend loaded, so other
# backends can't claim the USB interface first ("Device busy").
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
export SANE_CONFIG_DIR="$here/sane.d"

if ! lsusb -d 05e3:0144 >/dev/null; then
  echo "Scanner (05e3:0144) not on USB. Is it powered on?" >&2
  exit 1
fi

SANE_DEBUG_PIEUSB="${SANE_DEBUG_PIEUSB:-0}" scanimage -L
dev="$(scanimage -f '%d%n' | head -1)"
[ -n "$dev" ] && scanimage -d "$dev" -A

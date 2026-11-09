#!/bin/sh
# Make a private SANE config with pieusb only, and the PrimeFilm XA line's flags set to FLAGS,
# for the film-transport experiments in specs/002-film-scan/research.md (E1–E8).
#
#   eval "$(./scripts/spike-sane-config.sh 0x01)"
#   scanimage -L
#
# Read-only towards the scanner: this only writes files in a new temp directory.
set -eu

if [ $# -ne 1 ]; then
    echo "usage: $0 FLAGS   (e.g. 0x01)" >&2
    exit 2
fi
flags=$1
repo=$(cd "$(dirname "$0")/.." && pwd)
dir=$(mktemp -d "${TMPDIR:-/tmp}/primefilm-spike.XXXXXX")

cp "$repo/sane.d/dll.conf" "$dir/dll.conf"
sed "s/^\(usb 0x05e3 0x0144 0x0048\) .*/\1 $flags/" "$repo/sane.d/pieusb.conf" >"$dir/pieusb.conf"

echo "# $(grep '^usb 0x05e3 0x0144' "$dir/pieusb.conf")" >&2
echo "export SANE_CONFIG_DIR=$dir"

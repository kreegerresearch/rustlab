#!/usr/bin/env bash
# Real-time FIR filter on Linux using ALSA (arecord / aplay).
#
# Prerequisites:
#   sudo apt install alsa-utils      # Debian / Ubuntu
#   sudo dnf install alsa-utils      # Fedora
#
# Usage:
#   chmod +x examples/audio/linux.sh
#   ./examples/audio/linux.sh
#
# To list available ALSA devices:
#   arecord -l
#   aplay  -l
#
# To use a specific device (e.g. card 1, device 0):
#   ALSA_IN="hw:1,0" ALSA_OUT="hw:1,0" ./examples/audio/linux.sh
#
# Press Ctrl-C to stop.

set -euo pipefail

SCRIPT="$(dirname "$0")/filter.rlab"
SR=44100
ALSA_IN="${ALSA_IN:-default}"
ALSA_OUT="${ALSA_OUT:-default}"

arecord -D "$ALSA_IN" -f FLOAT_LE -r "$SR" -c 1 \
  | rustlab run "$SCRIPT" \
  | aplay -D "$ALSA_OUT" -f FLOAT_LE -r "$SR" -c 1

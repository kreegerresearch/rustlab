#!/usr/bin/env bash
# Real-time FIR filter on macOS using sox.
#
# Prerequisites:
#   brew install sox
#
# Usage:
#   chmod +x examples/audio/macos.sh
#   ./examples/audio/macos.sh
#
# What it does:
#   sox captures the default microphone as raw mono f32-LE PCM on stdout,
#   rustlab filters it, and sox plays the result to the default output.
#
# Press Ctrl-C to stop.

set -euo pipefail

SCRIPT="$(dirname "$0")/filter.rlab"
SR=44100
FRAME=256

sox -d \
    -t raw -r "$SR" -e float -b 32 -c 1 - \
  | rustlab run "$SCRIPT" \
  | sox -t raw -r "$SR" -e float -b 32 -c 1 - -d

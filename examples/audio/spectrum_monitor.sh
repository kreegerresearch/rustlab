#!/usr/bin/env bash
# Real-time audio spectrum monitor.
#
# Captures the default microphone and displays a live terminal plot of the
# Hann-windowed FFT magnitude spectrum in dB (DC to Nyquist).
#
# Prerequisites:
#   macOS:  brew install sox
#   Linux:  sudo apt install alsa-utils   (Debian/Ubuntu)
#           sudo dnf install alsa-utils   (Fedora)
#
# Usage:
#   chmod +x examples/audio/spectrum_monitor.sh
#   ./examples/audio/spectrum_monitor.sh
#
# Hardware-free test (5 seconds of 440 Hz + 2 kHz):
#   ./examples/audio/spectrum_monitor.sh --test
#
# Press Ctrl-C to stop.

set -euo pipefail

SCRIPT="$(dirname "$0")/spectrum_monitor.rlab"
SR=44100

if [[ "${1:-}" == "--test" ]]; then
    echo "Generating 5 s synthetic test signal (440 Hz + 2 kHz) ..."
    python3 -c "
import struct, math, sys
sr = $SR; n = sr * 5
for i in range(n):
    s = 0.5*math.sin(2*math.pi*440*i/sr) + 0.5*math.sin(2*math.pi*2000*i/sr)
    sys.stdout.buffer.write(struct.pack('f', s))
" | rustlab run "$SCRIPT"
elif [[ "$(uname)" == "Darwin" ]]; then
    sox -d -t raw -r "$SR" -e float -b 32 -c 1 - 2>/dev/null \
      | rustlab run "$SCRIPT"
else
    ALSA_IN="${ALSA_IN:-default}"
    arecord -D "$ALSA_IN" -f FLOAT_LE -r "$SR" -c 1 -t raw 2>/dev/null \
      | rustlab run "$SCRIPT"
fi

#!/usr/bin/env bash
# Real-time audio spectrogram monitor.
#
# Captures the default microphone and displays a live scrolling
# spectrogram heatmap (rustlab-viewer if running, ratatui terminal
# fallback otherwise). Uses the Phase-4 streaming features from
# dev/plans/time_frequency.md: `stft_stream_init`, `stft_stream`,
# and `plot_update_heatmap`.
#
# Prerequisites:
#   macOS:  brew install sox
#   Linux:  sudo apt install alsa-utils   (Debian/Ubuntu)
#           sudo dnf install alsa-utils   (Fedora)
#
# Usage:
#   chmod +x examples/audio/spectrogram_monitor.sh
#   ./examples/audio/spectrogram_monitor.sh
#
# Hardware-free test (10 seconds of a 100 Hz → 8 kHz linear chirp):
#   ./examples/audio/spectrogram_monitor.sh --test
#
# Press Ctrl-C to stop.

set -euo pipefail

SCRIPT="$(dirname "$0")/spectrogram_monitor.rlab"
SR=44100

if [[ "${1:-}" == "--test" ]]; then
    echo "Generating 10 s synthetic test signal (100 Hz → 8 kHz chirp) ..."
    python3 -c "
import struct, math, sys
sr = $SR; dur = 10.0; n = int(sr * dur)
f0, f1 = 100.0, 8000.0
for i in range(n):
    t = i / sr
    # Linear chirp: instantaneous frequency f0 + (f1-f0)*t/dur
    phase = 2*math.pi*(f0*t + 0.5*(f1-f0)*t*t/dur)
    s = 0.5*math.sin(phase)
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

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
#   WSL2:   alsa-utils plus PulseAudio or PipeWire-pulse (e.g.
#           `sudo apt install alsa-utils pulseaudio-utils`). Windows 11
#           22H2+ exposes the host mic to WSL2 out of the box; on
#           older WSL2 builds run `wsl --update` first. If the default
#           ALSA device doesn't pick up the mic, set ALSA_IN=pulse or
#           ALSA_IN=hw:0 before invoking. WSL1 has no audio passthrough.
#
# Usage:
#   chmod +x examples/audio/spectrogram_monitor.sh
#   ./examples/audio/spectrogram_monitor.sh
#
# Hardware-free test (10 seconds of a 100 Hz → 8 kHz linear chirp;
# works on every platform including WSL1):
#   ./examples/audio/spectrogram_monitor.sh --test
#
# Press Ctrl-C to stop.

set -euo pipefail

SCRIPT="$(dirname "$0")/spectrogram_monitor.rlab"
SR=44100

# Pre-flight: this demo requires rustlab-viewer. The .rlab script's
# `figure_live` would otherwise fall back to ratatui, but a scrolling
# spectrogram heatmap is effectively unreadable inside the alt-screen
# (the terminal just looks blank). Fail fast with a clear hint instead.
#
# `figure_live` captures the alt-screen and raw mode before we could
# surface anything from inside the .rlab script, so the check has to
# happen here in the wrapper.
VIEWER_SOCK="${RUSTLAB_VIEWER_SOCK:-/tmp/rustlab-viewer-$(id -u).sock}"
if [ ! -S "$VIEWER_SOCK" ]; then
    echo "error: rustlab-viewer is not running." >&2
    echo "       Expected socket at: $VIEWER_SOCK" >&2
    echo "" >&2
    echo "       This demo renders a scrolling spectrogram heatmap, which" >&2
    echo "       only works in the interactive egui viewer. Start it in" >&2
    echo "       another terminal and re-run:" >&2
    echo "" >&2
    echo "         rustlab-viewer" >&2
    echo "" >&2
    echo "       Then in this terminal:" >&2
    echo "" >&2
    echo "         ./examples/audio/spectrogram_monitor.sh         # mic" >&2
    echo "         ./examples/audio/spectrogram_monitor.sh --test  # chirp" >&2
    echo "" >&2
    echo "       (To use a named session: \`rustlab-viewer --name foo\` and" >&2
    echo "       set RUSTLAB_VIEWER_SOCK=/tmp/rustlab-viewer-\$(id -u)-foo.sock.)" >&2
    exit 1
fi

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

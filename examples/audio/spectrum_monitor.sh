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
#   WSL2:   alsa-utils plus PulseAudio or PipeWire-pulse (e.g.
#           `sudo apt install alsa-utils pulseaudio-utils`). Windows 11
#           22H2+ exposes the host mic to WSL2 out of the box; on
#           older WSL2 builds run `wsl --update` first. If the default
#           ALSA device doesn't pick up the mic, set ALSA_IN=pulse or
#           ALSA_IN=hw:0 before invoking. WSL1 has no audio passthrough.
#
# Usage:
#   chmod +x examples/audio/spectrum_monitor.sh
#   ./examples/audio/spectrum_monitor.sh
#
# Hardware-free test (5 seconds of 440 Hz + 2 kHz; works on every
# platform including WSL1):
#   ./examples/audio/spectrum_monitor.sh --test
#
# Press Ctrl-C to stop.

set -euo pipefail

SCRIPT="$(dirname "$0")/spectrum_monitor.rlab"
SR=44100

# Pre-flight: rustlab-viewer connectivity hint. The .rlab script always works
# (it falls back to the in-terminal ratatui plot), but for the interactive
# egui window the user has to start rustlab-viewer first. The .rlab itself
# can't print this until *after* figure_live has captured the alt-screen and
# raw mode, by which point any message would be invisible — so we surface it
# from the wrapper before the pipeline starts.
VIEWER_SOCK="${RUSTLAB_VIEWER_SOCK:-/tmp/rustlab-viewer-$(id -u).sock}"
if [ ! -S "$VIEWER_SOCK" ]; then
    echo "Note: rustlab-viewer is not running (no socket at $VIEWER_SOCK)."
    echo "      Rendering in the terminal (ratatui) instead. For the"
    echo "      interactive egui GUI, run \`rustlab-viewer\` in another"
    echo "      terminal first, then re-run this script."
    echo ""
fi

# ── Audio capture tool pre-flight (live mic only) ──
if [[ "${1:-}" != "--test" ]]; then
    if [[ "$(uname)" == "Darwin" ]]; then
        if ! command -v sox &>/dev/null; then
            echo "error: sox is not installed." >&2
            echo "       Install it with:  brew install sox" >&2
            echo "" >&2
            echo "       To test without a microphone:" >&2
            echo "         $0 --test" >&2
            exit 1
        fi
    else
        if ! command -v arecord &>/dev/null; then
            echo "error: arecord (alsa-utils) is not installed." >&2
            echo "       Install it with:" >&2
            echo "         Debian/Ubuntu:  sudo apt install alsa-utils" >&2
            echo "         Fedora:         sudo dnf install alsa-utils" >&2
            echo "" >&2
            echo "       To test without a microphone:" >&2
            echo "         $0 --test" >&2
            exit 1
        fi
    fi
fi

# Capture audio-tool stderr so device errors surface instead of vanishing.
AUDIO_ERR="$(mktemp)"
_audio_err_cleanup() {
    local rc=$?
    if [ $rc -ne 0 ] && [ $rc -lt 128 ] && [ -s "$AUDIO_ERR" ]; then
        echo "" >&2
        echo "error: audio capture failed:" >&2
        sed 's/^/       /' "$AUDIO_ERR" >&2
        if [[ "$(uname)" == "Darwin" ]]; then
            echo "" >&2
            echo "       Tip: check System Settings > Privacy & Security > Microphone" >&2
            echo "       to ensure Terminal (or your terminal app) has mic access." >&2
        fi
    fi
    rm -f "$AUDIO_ERR"
}
trap _audio_err_cleanup EXIT

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
    sox -d -t raw -r "$SR" -e float -b 32 -c 1 - 2>"$AUDIO_ERR" \
      | rustlab run "$SCRIPT"
else
    ALSA_IN="${ALSA_IN:-default}"
    arecord -D "$ALSA_IN" -f FLOAT_LE -r "$SR" -c 1 -t raw 2>"$AUDIO_ERR" \
      | rustlab run "$SCRIPT"
fi

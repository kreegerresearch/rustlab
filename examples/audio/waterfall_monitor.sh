#!/usr/bin/env bash
# Real-time audio frequency-waterfall monitor.
#
# Captures the default microphone and displays a live two-panel view
# (current spectrum line plot on top, downward-scrolling frequency
# waterfall heatmap below — newest at the top of the heatmap, oldest
# at the bottom). Uses the streaming-waterfall builtins from
# dev/plans/waterfall.md: `waterfall_stream_init` and the combined-call
# `waterfall_stream`.
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
#   chmod +x examples/audio/waterfall_monitor.sh
#   ./examples/audio/waterfall_monitor.sh
#
# Hardware-free tests (work on every platform including WSL1):
#   ./examples/audio/waterfall_monitor.sh --chirp   # 100 Hz → 8 kHz linear sweep, 10 s
#   ./examples/audio/waterfall_monitor.sh --steps   # 300 / 1200 / 3500 Hz tone steps, 2 s each
#
# Press Ctrl-C to stop.

set -euo pipefail

SCRIPT="$(dirname "$0")/waterfall_monitor.rlab"
SR=44100

# Pre-flight: this demo requires rustlab-viewer. The .rlab script's
# `figure_live` would otherwise fall back to ratatui, but a 2-panel
# live figure with a scrolling heatmap below a spectrum line plot is
# effectively unreadable inside the alt-screen (the heatmap panel
# stays blank in the TUI — see `dev/plans/waterfall.md`). Fail fast
# with a clear hint instead.
#
# `figure_live` captures the alt-screen and raw mode before we could
# surface anything from inside the .rlab script, so the check has to
# happen here in the wrapper.
VIEWER_SOCK="${RUSTLAB_VIEWER_SOCK:-/tmp/rustlab-viewer-$(id -u).sock}"
if [ ! -S "$VIEWER_SOCK" ]; then
    echo "error: rustlab-viewer is not running." >&2
    echo "       Expected socket at: $VIEWER_SOCK" >&2
    echo "" >&2
    echo "       This demo renders a 2-panel live figure (spectrum + heatmap)," >&2
    echo "       which only works in the interactive egui viewer. Start it in" >&2
    echo "       another terminal and re-run:" >&2
    echo "" >&2
    echo "         rustlab-viewer" >&2
    echo "" >&2
    echo "       Then in this terminal:" >&2
    echo "" >&2
    echo "         ./examples/audio/waterfall_monitor.sh           # mic" >&2
    echo "         ./examples/audio/waterfall_monitor.sh --chirp   # 10 s chirp" >&2
    echo "         ./examples/audio/waterfall_monitor.sh --steps   # tone steps" >&2
    echo "" >&2
    echo "       (To use a named session: \`rustlab-viewer --name foo\` and" >&2
    echo "       set RUSTLAB_VIEWER_SOCK=/tmp/rustlab-viewer-\$(id -u)-foo.sock.)" >&2
    exit 1
fi

# ── Audio capture tool pre-flight (live mic only) ──
if [[ -z "${1:-}" ]]; then
    if [[ "$(uname)" == "Darwin" ]]; then
        if ! command -v sox &>/dev/null; then
            echo "error: sox is not installed." >&2
            echo "       Install it with:  brew install sox" >&2
            echo "" >&2
            echo "       To test without a microphone:" >&2
            echo "         $0 --chirp" >&2
            echo "         $0 --steps" >&2
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
            echo "         $0 --chirp" >&2
            echo "         $0 --steps" >&2
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

case "${1:-}" in
    --chirp)
        echo "Generating 10 s synthetic chirp (100 Hz → 8 kHz) ..."
        python3 -c "
import struct, math, sys
sr = $SR; dur = 10.0; n = int(sr * dur)
f0, f1 = 100.0, 8000.0
for i in range(n):
    t = i / sr
    phase = 2*math.pi*(f0*t + 0.5*(f1-f0)*t*t/dur)
    s = 0.5*math.sin(phase)
    sys.stdout.buffer.write(struct.pack('f', s))
" | rustlab run "$SCRIPT"
        ;;
    --steps)
        echo "Generating tone steps (300 → 1200 → 3500 Hz, 2 s each, repeat 3×) ..."
        python3 -c "
import struct, math, sys
sr = $SR; seg_dur = 2.0; freqs = [300.0, 1200.0, 3500.0]
n_seg = int(sr * seg_dur)
for cycle in range(3):
    for f in freqs:
        for i in range(n_seg):
            s = 0.5*math.sin(2*math.pi*f*i/sr)
            sys.stdout.buffer.write(struct.pack('f', s))
" | rustlab run "$SCRIPT"
        ;;
    "")
        if [[ "$(uname)" == "Darwin" ]]; then
            sox -d -t raw -r "$SR" -e float -b 32 -c 1 - 2>"$AUDIO_ERR" \
                | rustlab run "$SCRIPT"
        else
            ALSA_IN="${ALSA_IN:-default}"
            arecord -D "$ALSA_IN" -f FLOAT_LE -r "$SR" -c 1 -t raw 2>"$AUDIO_ERR" \
                | rustlab run "$SCRIPT"
        fi
        ;;
    *)
        echo "usage: $0 [--chirp | --steps]" >&2
        exit 2
        ;;
esac

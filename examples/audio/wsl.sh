#!/usr/bin/env bash
# Real-time FIR filter on WSL2 (Windows Subsystem for Linux).
#
# WSL2 does not have direct hardware audio access. Two approaches:
#
# ── Option A: PulseAudio (recommended for WSL2 on Windows 11) ─────────────────
#
#   Windows 11 ships a built-in PulseAudio server that WSL2 can reach.
#   No extra setup needed on most systems.
#
#   Prerequisites (inside WSL2):
#     sudo apt install pulseaudio-utils sox
#
#   Usage:
#     chmod +x examples/audio/wsl.sh
#     ./examples/audio/wsl.sh
#
# ── Option B: TCP bridge (works on Windows 10 / any WSL version) ─────────────
#
#   Run a PCM sender on the Windows side, receive it in WSL2 over TCP.
#   See tcp.sh for the network streaming approach.
#
# Press Ctrl-C to stop.

set -euo pipefail

SCRIPT="$(dirname "$0")/filter.rlab"
SR=44100

# Check for PulseAudio (pacat) first, then fall back to sox with pulse.
if command -v pacat &>/dev/null; then
    # pacat: PulseAudio raw stream tool — simplest on WSL2 + Windows 11
    pacat --record --format=float32le --rate="$SR" --channels=1 \
      | rustlab run "$SCRIPT" \
      | pacat --playback --format=float32le --rate="$SR" --channels=1
elif command -v sox &>/dev/null; then
    # sox with PulseAudio backend
    sox -t pulseaudio default \
        -t raw -r "$SR" -e float -b 32 -c 1 - \
      | rustlab run "$SCRIPT" \
      | sox -t raw -r "$SR" -e float -b 32 -c 1 - \
            -t pulseaudio default
else
    echo "ERROR: neither pacat nor sox found."
    echo "Install with:  sudo apt install pulseaudio-utils sox"
    echo "Or see tcp.sh for a network-based alternative."
    exit 1
fi

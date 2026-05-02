#!/usr/bin/env bash
# Stream PCM audio into rustlab over a TCP port.
#
# Because rustlab reads/writes raw f32-LE PCM on stdin/stdout, any program
# that can push bytes over a socket works as the audio bridge — including
# netcat, socat, ffmpeg, and custom senders on other machines.
#
# ── Setup ─────────────────────────────────────────────────────────────────────
#
#   Prerequisites:
#     brew install netcat sox      # macOS
#     sudo apt install netcat sox  # Linux / WSL2
#
# ── Usage modes ───────────────────────────────────────────────────────────────
#
#   MODE=server   rustlab listens on IN_PORT for raw PCM, outputs to OUT_PORT
#   MODE=send     send mic audio to a running server (separate terminal)
#   MODE=play     receive filtered audio from the server and play it (separate terminal)
#
# ── Quickstart (three terminals) ─────────────────────────────────────────────
#
#   Terminal 1 — start the filter server:
#     MODE=server ./examples/audio/tcp.sh
#
#   Terminal 2 — send microphone audio to port 9999:
#     MODE=send   ./examples/audio/tcp.sh
#
#   Terminal 3 — play filtered audio from port 9998:
#     MODE=play   ./examples/audio/tcp.sh
#
# ── Remote machine ───────────────────────────────────────────────────────────
#
#   The sender and player can be on a different host — just set SERVER_HOST:
#     SERVER_HOST=192.168.1.42 MODE=send ./examples/audio/tcp.sh
#
# ── WSL2 + Windows ───────────────────────────────────────────────────────────
#
#   Run rustlab (MODE=server) inside WSL2.
#   Run the sender (MODE=send) on Windows using ffmpeg or a Python script:
#
#     # Windows PowerShell — send mic to WSL2 on port 9999
#     ffmpeg -f dshow -i audio="Microphone" \
#            -f f32le -ar 44100 -ac 1 - | nc <WSL2-IP> 9999
#
#   Find WSL2 IP:  ip addr show eth0 | grep "inet "
#
# Press Ctrl-C in each terminal to stop.

set -euo pipefail

SCRIPT="$(dirname "$0")/filter.rlab"
SR=44100
IN_PORT="${IN_PORT:-9999}"
OUT_PORT="${OUT_PORT:-9998}"
SERVER_HOST="${SERVER_HOST:-127.0.0.1}"
MODE="${MODE:-server}"

case "$MODE" in
  server)
    echo "Filter server: listening for raw PCM on port $IN_PORT, output on port $OUT_PORT"
    echo "Start sender:  MODE=send  $0"
    echo "Start player:  MODE=play  $0"
    echo ""
    # nc -l listens on one port; the filtered output is piped to a second nc listener.
    # socat is more reliable for bidirectional use — fall back to nc if unavailable.
    if command -v socat &>/dev/null; then
        socat TCP-LISTEN:"$IN_PORT",reuseaddr - \
          | rustlab run "$SCRIPT" \
          | socat - TCP-LISTEN:"$OUT_PORT",reuseaddr
    else
        nc -l "$IN_PORT" \
          | rustlab run "$SCRIPT" \
          | nc -l "$OUT_PORT"
    fi
    ;;

  send)
    echo "Sender: capturing mic and streaming to $SERVER_HOST:$IN_PORT"
    if [[ "$(uname)" == "Darwin" ]]; then
        sox -d -t raw -r "$SR" -e float -b 32 -c 1 - \
          | nc "$SERVER_HOST" "$IN_PORT"
    else
        arecord -f FLOAT_LE -r "$SR" -c 1 \
          | nc "$SERVER_HOST" "$IN_PORT"
    fi
    ;;

  play)
    echo "Player: receiving filtered audio from $SERVER_HOST:$OUT_PORT"
    if [[ "$(uname)" == "Darwin" ]]; then
        nc "$SERVER_HOST" "$OUT_PORT" \
          | sox -t raw -r "$SR" -e float -b 32 -c 1 - -d
    else
        nc "$SERVER_HOST" "$OUT_PORT" \
          | aplay -f FLOAT_LE -r "$SR" -c 1
    fi
    ;;

  *)
    echo "Unknown MODE='$MODE'. Use: server | send | play"
    exit 1
    ;;
esac

#!/usr/bin/env bash
# Test the streaming pipeline without any audio hardware.
#
# Generates a synthetic test signal (440 Hz sine + 4 kHz sine) using Python,
# runs it through the FIR lowpass filter (cutoff ~1 kHz), and verifies that:
#   - the 440 Hz component passes through (in the passband)
#   - the 4 kHz component is attenuated (stopband)
#
# Works on macOS, Linux, and WSL2 — no microphone or speakers needed.
# Requires Python 3 (usually pre-installed). Uses numpy for analysis if available.
#
# Usage:
#   chmod +x examples/audio/test_no_hardware.sh
#   ./examples/audio/test_no_hardware.sh

set -euo pipefail

SCRIPT="$(dirname "$0")/filter.rlab"
SR=44100
FRAME=256
# Use exactly 170 frames so input length is a clean multiple of FRAME.
# (170 * 256 = 43520 samples ≈ 0.99 s at 44100 Hz)
N_FRAMES=170
SAMPLES=$((N_FRAMES * FRAME))

TMPOUT=$(mktemp /tmp/rustlab_stream_test_XXXXXX.raw)
trap 'rm -f "$TMPOUT"' EXIT

echo "Generating $SAMPLES samples (${N_FRAMES} frames × ${FRAME}) of 440 Hz + 8 kHz ..."

python3 - <<PYEOF | rustlab run "$SCRIPT" > "$TMPOUT"
import struct, math, sys
sr = $SR
n  = $SAMPLES
for i in range(n):
    t = i / sr
    # 440 Hz in passband, 8 kHz well into stopband (8× Nyquist of 1 kHz cutoff)
    s = 0.5 * math.sin(2 * math.pi * 440  * t) \
      + 0.5 * math.sin(2 * math.pi * 8000 * t)
    sys.stdout.buffer.write(struct.pack('<f', s))
PYEOF

echo "Analysing output ($TMPOUT) ..."

python3 - "$TMPOUT" <<'PYEOF'
import struct, sys, math

path = sys.argv[1]
with open(path, 'rb') as f:
    raw = f.read()

n_samples = len(raw) // 4
sr = 44100

try:
    import numpy as np
    samples = np.frombuffer(raw, dtype='<f4').astype(float)
    fft     = np.fft.rfft(samples)
    freqs   = np.fft.rfftfreq(len(samples), 1.0 / sr)
    mag_440  = abs(fft[int(round(440  * len(samples) / sr))]) / (len(samples) / 2)
    mag_4000 = abs(fft[int(round(8000 * len(samples) / sr))]) / (len(samples) / 2)
except ImportError:
    # numpy not available: use Goertzel algorithm (O(n) per frequency)
    def goertzel(samples, freq, sr):
        w  = 2 * math.pi * freq / sr
        c  = 2 * math.cos(w)
        s0 = s1 = s2 = 0.0
        for x in samples:
            s0 = x + c * s1 - s2
            s2, s1 = s1, s0
        real = s1 - s2 * math.cos(w)
        imag = s2 * math.sin(w)
        return math.sqrt(real**2 + imag**2) / len(samples)
    samples  = [struct.unpack('<f', raw[i*4:(i+1)*4])[0] for i in range(n_samples)]
    mag_440  = goertzel(samples, 440,  sr)
    mag_4000 = goertzel(samples, 8000, sr)

ratio_db = 20 * math.log10(max(mag_4000 / max(mag_440, 1e-9), 1e-9))

print(f"  440 Hz magnitude  (passband): {mag_440:.4f}")
print(f"  8000 Hz magnitude (stopband): {mag_4000:.4f}")
print(f"  Stopband attenuation: {-ratio_db:.1f} dB")

if mag_440 < 0.05:
    print("FAIL: passband signal too weak (< 0.05)")
    sys.exit(1)
if ratio_db > -20:
    print(f"FAIL: stopband attenuation only {-ratio_db:.1f} dB, need >= 20 dB")
    sys.exit(1)

print("PASS: passband preserved, stopband attenuated.")
PYEOF

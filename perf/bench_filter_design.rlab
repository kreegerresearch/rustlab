# Benchmark: FIR/IIR filter design cost
#
# Design functions do most of their work at call time (window multiplication,
# Parks-McClellan iteration, Butterworth pole placement).  This script measures
# how long each design call takes at increasing filter orders.

sr = 48000.0

# ── Windowed FIR — hann window ───────────────────────────────────────────────
print("fir_lowpass 64-tap hann")
h1 = fir_lowpass(64, 8000.0, sr, "hann");
print("  taps: ", len(h1))

print("fir_lowpass 512-tap hann")
h2 = fir_lowpass(512, 8000.0, sr, "hann");
print("  taps: ", len(h2))

print("fir_lowpass 1024-tap hann")
h3 = fir_lowpass(1024, 8000.0, sr, "hann");
print("  taps: ", len(h3))

# ── Kaiser FIR (cutoff_hz, transition_bw_hz, attenuation_db, sr) ─────────────
print("fir_lowpass_kaiser 60dB")
hk = fir_lowpass_kaiser(8000.0, 1000.0, 60.0, sr);
print("  taps: ", len(hk))

print("fir_lowpass_kaiser 80dB")
hk2 = fir_lowpass_kaiser(8000.0, 1000.0, 80.0, sr);
print("  taps: ", len(hk2))

# ── Parks-McClellan (n_taps, bands_vec, desired_vec) ─────────────────────────
print("firpm 63-tap lowpass")
f1 = firpm(63, [0.0, 0.20, 0.30, 1.0], [1.0, 1.0, 0.0, 0.0]);
print("  taps: ", len(f1))

print("firpm 127-tap lowpass")
f2 = firpm(127, [0.0, 0.20, 0.30, 1.0], [1.0, 1.0, 0.0, 0.0]);
print("  taps: ", len(f2))

# ── Butterworth IIR (order, cutoff_hz, sr) ───────────────────────────────────
print("butterworth_lowpass order 4")
b1 = butterworth_lowpass(4, 8000.0, sr);
print("  coeff count: ", len(b1))

print("butterworth_lowpass order 8")
b2 = butterworth_lowpass(8, 8000.0, sr);
print("  coeff count: ", len(b2))

print("done")

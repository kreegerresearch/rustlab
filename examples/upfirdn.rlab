# upfirdn — polyphase upsample / filter / downsample
#
# Demonstrates three use cases:
#   1. Interpolation (p=4, q=1)  — increase sample rate by 4x
#   2. Decimation   (p=1, q=4)  — reduce sample rate by 4x
#   3. Rational SRC (p=3, q=2)  — convert 2 kHz → 3 kHz (1.5x)
#
# All three share the same anti-aliasing / anti-imaging FIR filter.

sr   = 8000.0    # original sample rate (Hz)
n    = 64        # number of input samples
tone = 300.0     # test tone frequency (Hz)

# Build a time vector and a 300 Hz cosine
t = linspace(0.0, (n - 1) / sr, n);
x = real(cos(2.0 * pi * tone * t));

plot(x, "Input — 300 Hz at 8 kHz")

# ── 1. Interpolation by 4 ─────────────────────────────────────────────────────
# Anti-imaging filter: cutoff at sr/2/4 = 1 kHz (new Nyquist after 4x upsample)
p = 4
q = 1
h_interp = fir_lowpass(64, sr / 2.0 / p, sr, "hann");

y_up = upfirdn(x, h_interp, p, q);

print("Input length:        ", len(x))
print("Interpolated length: ", len(y_up))

plot(real(y_up), "Interpolated x4 (32 kHz)")

# ── 2. Decimation by 4 ───────────────────────────────────────────────────────
# Anti-aliasing filter: cutoff at sr/2/4 = 1 kHz (new Nyquist after 4x decimate)
p = 1
q = 4
h_decim = fir_lowpass(64, sr / 2.0 / q, sr, "hann");

y_down = upfirdn(x, h_decim, p, q);

print("Input length:      ", len(x))
print("Decimated length:  ", len(y_down))

plot(real(y_down), "Decimated x4 (2 kHz)")

# ── 3. Rational SRC: 2 kHz → 3 kHz  (p=3, q=2) ───────────────────────────────
# Cutoff at min(sr/2/p, sr/2/q) = sr/2/3 ≈ 1333 Hz
p = 3
q = 2
cutoff = (sr / 2.0) / p
h_src = fir_lowpass(128, cutoff, sr, "hann");

y_src = upfirdn(x, h_src, p, q);

print("Input length:    ", len(x))
print("SRC 3/2 length:  ", len(y_src))

plot(real(y_src), "Rate conversion 3/2")

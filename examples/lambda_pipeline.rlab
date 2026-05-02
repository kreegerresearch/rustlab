# Lambda pipeline: composable signal processing with anonymous functions
#
# Demonstrates how lambdas let you build a processing chain from small,
# reusable pieces — no named helper functions needed.

N  = 256
fs = 1000.0
t  = linspace(0, (N-1)/fs, N)

# ── Signal: two tones + noise ─────────────────────────────────────────────────
x = sin(2 * pi * 50 .* t) + 0.4 * sin(2 * pi * 200 .* t) + 0.1 * randn(N)

# ── Step 1: build window and normalise functions as lambdas ───────────────────
hann     = @(n) 0.5 - 0.5 .* cos(2 .* pi .* linspace(0, 1, n))
normalise = @(v) v ./ max(abs(v))

w        = hann(N)
x_win    = normalise(x .* w)

# ── Step 2: parametric gain stages as lambdas ─────────────────────────────────
make_gain = @(g) @(v) v .* g

boost  = make_gain(2.0)
attenu = make_gain(0.25)

x_boosted  = boost(x_win)
x_attenuated = attenu(x_win)

disp("peak after boost:")
max(abs(x_boosted))

disp("peak after attenuation:")
max(abs(x_attenuated))

# ── Step 3: FFT magnitude in dB via lambda ────────────────────────────────────
mag_db = @(sig) 20 .* log10(abs(fft(sig)) ./ N + 1e-12)

db_raw = mag_db(x)
db_win = mag_db(x_win)

disp("DC bin (raw):")
db_raw(1)

disp("DC bin (windowed):")
db_win(1)

# ── Step 4: arrayfun — apply the dB analyser to a batch of signals ────────────
# Build three variants of the signal at different gains and analyse them all
gains   = [0.5, 1.0, 2.0]
signals = arrayfun(@(g) mag_db(x .* g), gains)

disp("dB matrix shape is 3 rows x N cols (one spectrum per gain):")
size(signals)

# Peak bin of each spectrum (should be identical — gain shifts level, not freq)
peaks = arrayfun(@(row) max(signals(row, :)), 1:3)
disp("peak dB per gain level:")
peaks

# ── Step 5: compose lambdas into a named pipeline ─────────────────────────────
function y = apply_all(v, f1, f2, f3)
  y = f3(f2(f1(v)))
end

stage1 = @(v) v .* hann(length(v))
stage2 = @(v) v ./ (max(abs(v)) + 1e-12)
stage3 = @(v) 20 .* log10(abs(fft(v)) ./ length(v) + 1e-12)

result = apply_all(x, stage1, stage2, stage3)
disp("pipeline output: first 4 dB bins")
result(1:4)

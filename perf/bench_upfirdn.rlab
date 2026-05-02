# Benchmark: upfirdn polyphase filter
#
# Measures wall time for three common workloads:
#   - Short signal, long filter (filter-dominated)
#   - Long signal, short filter (throughput-dominated)
#   - Rational SRC on a large buffer

sr = 48000.0

# ── Workload 1: short signal, long filter ────────────────────────────────────
n1  = 256
h1  = fir_lowpass(512, sr / 8.0, sr, "hann");
x1  = randn(n1);

print("Workload 1: n=256, h=512-tap, 4x interp")
y1 = upfirdn(x1, h1, 4, 1);
print("  output length: ", len(y1))

# ── Workload 2: long signal, short filter ────────────────────────────────────
n2  = 48000     # 1 second at 48 kHz
h2  = fir_lowpass(64, sr / 6.0, sr, "hann");
x2  = randn(n2);

print("Workload 2: n=48000 (1s), h=64-tap, 3x decimate")
y2 = upfirdn(x2, h2, 1, 3);
print("  output length: ", len(y2))

# ── Workload 3: rational SRC on large buffer ─────────────────────────────────
n3  = 44100     # 1 second at 44.1 kHz
h3  = fir_lowpass(128, sr / 8.0, sr, "hann");
x3  = randn(n3);

print("Workload 3: n=44100 (1s at 44.1kHz), h=128-tap, SRC 3/2")
y3 = upfirdn(x3, h3, 3, 2);
print("  output length: ", len(y3))

print("done")

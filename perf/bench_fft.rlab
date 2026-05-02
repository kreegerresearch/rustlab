# Benchmark: FFT / IFFT round-trip

sr = 44100.0

# ── Small FFT ────────────────────────────────────────────────────────────────
n1 = 1024
x1 = randn(n1);
print("FFT/IFFT n=1024")
X1 = fft(x1);
x1r = ifft(X1);
print("  round-trip length: ", len(x1r))

# ── Medium FFT ───────────────────────────────────────────────────────────────
n2 = 16384
x2 = randn(n2);
print("FFT/IFFT n=16384")
X2 = fft(x2);
x2r = ifft(X2);
print("  round-trip length: ", len(x2r))

# ── Large FFT ────────────────────────────────────────────────────────────────
n3 = 131072
x3 = randn(n3);
print("FFT/IFFT n=131072")
X3 = fft(x3);
x3r = ifft(X3);
print("  round-trip length: ", len(x3r))

print("done")

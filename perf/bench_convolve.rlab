# Benchmark: convolve — direct-form vs overlap-add scaling

# ── Short × Short ────────────────────────────────────────────────────────────
print("convolve 256 * 64")
x1 = randn(256);
h1 = randn(64);
y1 = convolve(x1, h1);
print("  output length: ", len(y1))

# ── Medium signal × medium filter ────────────────────────────────────────────
print("convolve 4096 * 256")
x2 = randn(4096);
h2 = randn(256);
y2 = convolve(x2, h2);
print("  output length: ", len(y2))

# ── Long signal × short filter ───────────────────────────────────────────────
print("convolve 48000 * 64")
x3 = randn(48000);
h3 = randn(64);
y3 = convolve(x3, h3);
print("  output length: ", len(y3))

# ── Long signal × long filter ────────────────────────────────────────────────
print("convolve 48000 * 512")
x4 = randn(48000);
h4 = randn(512);
y4 = convolve(x4, h4);
print("  output length: ", len(y4))

print("done")

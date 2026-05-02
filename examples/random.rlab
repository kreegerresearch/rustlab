# Random number generation examples
# rand, randn, randi — and histograms to visualise their distributions

# ── Uniform noise ─────────────────────────────────────────────────────────────
u = rand(2000)
histogram(u, 20)

# ── Gaussian (normal) noise ───────────────────────────────────────────────────
n = randn(2000)
histogram(n, 30)

# ── Random integers ───────────────────────────────────────────────────────────
# Single integer in [1, 6] — like rolling a die
roll = randi(6)
print(roll)

# 5000 die rolls — should produce roughly equal counts in each bin
rolls = randi(6, 5000)
histogram(rolls, 6)

# Range with explicit lo/hi: integers in [-5, 5]
signed = randi([-5, 5], 1000)
histogram(signed, 11)

# ── Noisy sinusoid ────────────────────────────────────────────────────────────
t     = linspace(0.0, 1.0, 1000)
clean = cos(t * 2.0 * pi * 5.0)
noise = randn(1000) * 0.3
noisy = clean + noise

plot(real(noisy), "Noisy 5 Hz Sinusoid (SNR ≈ 10 dB)")

# Spectrum of the noisy signal
X = fft(real(noisy))
plot(abs(fftshift(X)), "Noisy Sinusoid Spectrum")

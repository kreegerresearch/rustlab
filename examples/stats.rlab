# Statistical aggregates and numerical integration
#
# Demonstrates: sum, cumsum, argmin, argmax, sort, trapz, bar, scatter

t  = linspace(0.0, 1.0, 256)
sr = 256.0

# ── Reference signal: two sinusoids + noise ───────────────────────────────────
sig = cos(t * 2.0 * pi * 5.0) * 2.0 + cos(t * 2.0 * pi * 20.0) + randn(256) * 0.3

# ── sum ──────────────────────────────────────────────────────────────────────
# Total energy proxy (sum of squared samples)
energy = sum(real(sig) .^ 2)
print(energy)

# ── cumsum ───────────────────────────────────────────────────────────────────
# Running total — useful for cumulative energy or CDF estimation
cs = cumsum(real(sig) .^ 2)
plot(real(cs), "Cumulative Signal Energy")

# Normalised CDF of a randn sample
n_samp = randn(512)
bins   = sort(n_samp)               # sort ascending — becomes the x-axis of the CDF
cdf    = cumsum(ones(512)) / 512.0  # uniform weights → empirical CDF
scatter(real(bins), real(cdf), "Empirical CDF of N(0,1)")

# ── argmin / argmax ───────────────────────────────────────────────────────────
# 1-based index of the extreme values
i_min = argmin(sig)
i_max = argmax(sig)
print(i_min)
print(i_max)
print(sig(i_min))
print(sig(i_max))

# ── sort ──────────────────────────────────────────────────────────────────────
# Ascending sort; useful for order statistics and top-K selection
s    = sort(real(sig))
p_lo = s(26)                         # ≈ 10th percentile  (index 26 of 256)
p_hi = s(230)                        # ≈ 90th percentile  (index 230 of 256)
print(p_lo)
print(p_hi)

# ── trapz ────────────────────────────────────────────────────────────────────
# Numerical integration by the trapezoidal rule.

# Area under one full period of a 5 Hz cosine over [0, 1] → should be ≈ 0
area_cos = trapz(t, real(cos(t * 2.0 * pi * 5.0)))
print(area_cos)                       # → ≈ 0.0

# Area under a triangle: f(x) = x on [0, 1] → 0.5
x_tri  = linspace(0.0, 1.0, 100)
f_tri  = x_tri
area_tri = trapz(x_tri, f_tri)
print(area_tri)                       # → ≈ 0.5

# RMS of the signal via trapz: sqrt(1/T ∫ s² dt)
rms = sqrt(trapz(t, real(sig) .^ 2))
print(rms)

# ── bar chart ─────────────────────────────────────────────────────────────────
# Energy in four equal-width time segments
seg  = 64
e1   = sum(real(sig(1:64))    .^ 2)
e2   = sum(real(sig(65:128))  .^ 2)
e3   = sum(real(sig(129:192)) .^ 2)
e4   = sum(real(sig(193:256)) .^ 2)
energies = [e1, e2, e3, e4]
bar(energies, "Energy per 64-sample segment")

# ── scatter plot ──────────────────────────────────────────────────────────────
# Scatter of signal vs its 1-sample-delayed version (phase portrait)
x_scatter = real(sig(1:255))
y_scatter = real(sig(2:256))
scatter(x_scatter, y_scatter, "Signal phase portrait (x[n] vs x[n-1])")

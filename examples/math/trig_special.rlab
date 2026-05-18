# Inverse trig and special polynomials
#
# Demonstrates: acos, asin, atan, laguerre, legendre

# ── Inverse trigonometry ──────────────────────────────────────────────────────
# Recover angles from ratios — element-wise on vectors.

angles_deg = linspace(0.0, 90.0, 7)       # 0°, 15°, 30°, 45°, 60°, 75°, 90°
angles_rad = angles_deg * pi / 180.0

c = cos(angles_rad)
s = sin(angles_rad)

# Round-trip: acos(cos(θ)) ≈ θ  (for θ ∈ [0, π])
theta_acos = acos(c)
print(theta_acos)
print(max(abs(theta_acos - angles_rad)))   # → ≈ 0  (round-trip error)

# Round-trip: asin(sin(θ)) ≈ θ  (for θ ∈ [-π/2, π/2])
theta_asin = asin(s)
print(max(abs(theta_asin - angles_rad)))   # → ≈ 0

# atan gives the angle of a complex number's real/imag components
# asin² + acos² = π/2  (identity holds element-wise)
check = asin(s) .^ 2 + acos(s) .^ 2      # should equal (π/2)² at each point?
# Actually: asin(x) + acos(x) = π/2 for |x| ≤ 1
identity = asin(s) + acos(s)
print(max(abs(real(identity) - pi/2.0)))  # → ≈ 0

# atan: inverse tangent — useful for unwrapped phase calculations
tangents = s ./ (c + 1e-12)              # tan(θ), avoid div-by-zero at 90°
theta_atan = atan(tangents)
print(max(abs(real(theta_atan) - angles_rad)))   # → ≈ 0  (for θ ∈ [0, π/2))

# Save the round-trip residual across a wider angle sweep
theta_wide = linspace(-pi/2.0, pi/2.0, 200)
residual   = real(asin(sin(theta_wide))) - real(theta_wide)
plot(residual, "asin(sin(θ)) − θ  round-trip error")

# ── Laguerre polynomials ──────────────────────────────────────────────────────
# laguerre(n, alpha, x) — generalised Laguerre polynomial L_n^alpha(x).
# Standard (alpha=0) Laguerre polynomials are orthogonal on [0, ∞) with
# weight e^(-x).

x_lag = linspace(0.0, 8.0, 200)

L0 = laguerre(0, 0.0, x_lag)             # L₀(x) = 1
L1 = laguerre(1, 0.0, x_lag)             # L₁(x) = 1 − x
L2 = laguerre(2, 0.0, x_lag)             # L₂(x) = 1 − 2x + x²/2
L3 = laguerre(3, 0.0, x_lag)             # L₃(x) = higher order

# Verify L₁(x) = 1 − x at a spot-check
x_spot = 2.5
print(laguerre(1, 0.0, x_spot))           # → 1 − 2.5 = -1.5
print(laguerre(0, 0.0, x_spot))           # → 1.0

plot(real(L2), "Laguerre L₂(x)  [0, 8]")
plot(real(L3), "Laguerre L₃(x)  [0, 8]")

# Generalised Laguerre with alpha=0.5: used in quantum harmonic oscillator
L2a = laguerre(2, 0.5, x_lag)
plot(real(L2a), "Generalised Laguerre L₂^(0.5)(x)")

# ── Legendre polynomials ──────────────────────────────────────────────────────
# legendre(l, m, x) — associated Legendre polynomial P_l^m(x).
# x must satisfy |x| ≤ 1.  m=0 gives the ordinary Legendre polynomials.

x_leg = linspace(-1.0, 1.0, 200)

P00 = legendre(0, 0, x_leg)              # P₀⁰(x) = 1
P10 = legendre(1, 0, x_leg)              # P₁⁰(x) = x
P20 = legendre(2, 0, x_leg)              # P₂⁰(x) = (3x² − 1)/2
P30 = legendre(3, 0, x_leg)              # P₃⁰(x) = (5x³ − 3x)/2

# Verify orthogonality: ∫₋₁¹ P₁(x) P₂(x) dx ≈ 0
integral_12 = trapz(x_leg, real(P10) .* real(P20))
print(integral_12)                        # → ≈ 0.0

# ‖P₂‖² = ∫₋₁¹ P₂² dx = 2/(2·2+1) = 2/5 = 0.4
norm_P20 = trapz(x_leg, real(P20) .^ 2)
print(norm_P20)                           # → ≈ 0.4

plot(real(P20), "Legendre P₂⁰(x)  (l=2, m=0)")
plot(real(P30), "Legendre P₃⁰(x)  (l=3, m=0)")

# Associated Legendre: P₂¹(x) — appears in spherical harmonics Y_l^m
P21 = legendre(2, 1, x_leg)
plot(real(P21), "Associated Legendre P₂¹(x)")

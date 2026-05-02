# Log-axis and polar plots.
# Demonstrates: loglog, semilogx, semilogy, polar (all as pre-transform
# shims over the existing plot() machinery).
#
# Files written:
#   /tmp/rustlab_loglog.html        power-law data on loglog axes
#   /tmp/rustlab_semilogy.html      exponential decay on semilogy
#   /tmp/rustlab_polar_rose.html    three-petal rose curve
#   /tmp/rustlab_polar_dipole.html  antenna-pattern-style |sin(theta)|

# ── 1. Loglog: power-law data → straight line ───────────────────
% y = x^2 should produce a straight line of slope 2 on log-log axes.
x = logspace(0, 3, 50);
y = x .^ 2;
figure();
loglog(x, y);
title("y = x^2 — slope 2 on log-log");
savefig("/tmp/rustlab_loglog.html");

# ── 2. Semilogy: exponential decay ──────────────────────────────
t = linspace(0, 5, 100);
% e^{-t} — straight line on semilogy.
y_decay = exp(-1 * t);
figure();
semilogy(t, y_decay);
title("Exponential decay e^{-t} on semilogy");
savefig("/tmp/rustlab_semilogy.html");

# ── 3. Polar: three-petal rose ──────────────────────────────────
theta = linspace(0, 2*pi, 360);
r_rose = 1 + 0.3 * cos(3 * theta);
figure();
polar(theta, r_rose);
title("Three-petal rose: r = 1 + 0.3 cos(3θ)");
savefig("/tmp/rustlab_polar_rose.html");

# ── 4. Polar: Hertzian-dipole-style pattern ─────────────────────
% |sin(theta)| is the canonical Hertzian-dipole radiation pattern.
theta_d = linspace(-1 * pi, pi, 360);
r_dipole = abs(sin(theta_d));
figure();
polar(theta_d, r_dipole);
title("Hertzian dipole pattern: |sin θ|");
savefig("/tmp/rustlab_polar_dipole.html");

print("done — open the .html files to view")

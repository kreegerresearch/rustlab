# 3D surface plotting — meshgrid + surf
#
# Demonstrates: meshgrid(x, y), surf(Z), surf(X, Y, Z), surf(X, Y, Z, cmap)
#
# Per-backend behaviour (automatic, no code changes needed):
#   - Terminal:      heatmap of Z
#   - Viewer on:     interactive 3D (drag rotate, scroll zoom, right-drag pan, R reset)
#   - .html savefig: Plotly 3D surface (drag in browser)
#   - .svg/.png:     static isometric wireframe

# ── meshgrid ──────────────────────────────────────────────────────────────────
# meshgrid(x, y) returns [X, Y] — two matrices whose elements are the x and
# y coordinates of every grid point. Shape is length(y) × length(x).
x = linspace(-3, 3, 40)
y = linspace(-3, 3, 40)
[X, Y] = meshgrid(x, y)
print(size(X))                          # → [40, 40]
print(size(Y))                          # → [40, 40]

# ── surf(X, Y, Z) — Gaussian bump ─────────────────────────────────────────────
Z = exp(-(X.^2 + Y.^2) / 2.0)
figure()
surf(X, Y, Z)
title("Gaussian: exp(-(x^2 + y^2) / 2)")
xlabel("x"); ylabel("y")
savefig("surf_gaussian.svg")
savefig("surf_gaussian.html")

# ── surf with a different colormap ────────────────────────────────────────────
# Colormaps: "viridis" (default), "jet", "hot", "gray"
figure()
Z2 = sin(X.^2 + Y.^2) ./ (X.^2 + Y.^2 + 0.1)
surf(X, Y, Z2, "jet")
title("sin(r^2)/r^2 ripples")
xlabel("x"); ylabel("y")
savefig("surf_ripples.svg")
savefig("surf_ripples.html")

# ── surf(Z) — no explicit axes (x = 1..cols, y = 1..rows) ─────────────────────
# Useful when the matrix itself is the thing you want to visualise.
figure()
M = outer(window("hann", 32), window("hann", 32))
surf(M)
title("Separable 2-D Hann window")
savefig("surf_hann2d.svg")
savefig("surf_hann2d.html")

# ── Saddle + HTML export ──────────────────────────────────────────────────────
# .html output emits a Plotly 3D surface — drag to rotate in the browser.
figure()
Zs = X.^2 - Y.^2
surf(X, Y, Zs, "viridis")
title("Saddle: x^2 - y^2")
xlabel("x"); ylabel("y")
savefig("surf_saddle.html")

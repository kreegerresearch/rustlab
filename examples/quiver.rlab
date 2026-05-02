# Quiver and streamplot — 2-D vector-field plots.
# Demonstrates: quiver(X, Y, U, V, ...), quiver(U, V), streamplot(X, Y, U, V, ...),
# custom seeds, hold-on overlay on imagesc / contour.
#
# Files written:
#   /tmp/rustlab_quiver_uniform.svg       quiver for a uniform flow
#   /tmp/rustlab_quiver_vortex.html       quiver for a vortex field (-y, x)
#   /tmp/rustlab_stream_vortex.html       streamplot for the same vortex
#   /tmp/rustlab_stream_saddle.html       streamplot for a saddle field (x, -y)
#   /tmp/rustlab_stream_seeds.html        streamplot with explicit seed matrix
#   /tmp/rustlab_heatmap_quiver.html      |E|² heatmap + quiver of E under hold
#   /tmp/rustlab_contour_stream.html      equipotential contours + streamlines
#
# Note: quiver and streamplot are NOT rendered to the terminal. Save to
# .svg or .html and open the file to view.

# ── 1. Uniform horizontal flow ──────────────────────────────────
[X, Y] = meshgrid(linspace(-1, 1, 12), linspace(-1, 1, 12));
U = ones(12, 12);
V = zeros(12, 12);
figure();
quiver(X, Y, U, V, "Uniform flow (1, 0)");
savefig("/tmp/rustlab_quiver_uniform.svg");

# ── 2. Vortex: (U, V) = (-Y, X) ─────────────────────────────────
[X, Y] = meshgrid(linspace(-2, 2, 16), linspace(-2, 2, 16));
U = -Y;
V = X;
figure();
quiver(X, Y, U, V, "Vortex field (-y, x)");
savefig("/tmp/rustlab_quiver_vortex.html");

# ── 3. Streamplot for the vortex ────────────────────────────────
figure();
streamplot(X, Y, U, V, "Vortex streamlines");
savefig("/tmp/rustlab_stream_vortex.html");

# ── 4. Saddle field (x, -y) ─────────────────────────────────────
[X, Y] = meshgrid(linspace(-2, 2, 30), linspace(-2, 2, 30));
U = X;
V = -Y;
figure();
streamplot(X, Y, U, V, "Saddle point (x, -y)");
savefig("/tmp/rustlab_stream_saddle.html");

# ── 5. Custom seeds (3×2 matrix of (x, y) points) ───────────────
figure();
seeds = [[-1.5, 0.5]; [0.0, 1.8]; [1.5, -1.2]];
streamplot(X, Y, U, V, seeds);
savefig("/tmp/rustlab_stream_seeds.html");

# ── 6. Overlay: |E|² heatmap + E quiver under hold on ───────────
# Canonical EM pattern. Build a field from a potential V = x^2 - y^2,
# then E = -grad(V).
[X, Y] = meshgrid(linspace(-2, 2, 25), linspace(-2, 2, 25));
Vpot = X .^ 2 - Y .^ 2;
[Ex, Ey] = gradient(Vpot);     % dV/dx, dV/dy on unit grid spacing
Ex = -Ex;
Ey = -Ey;
Emag = Ex .* Ex + Ey .* Ey;

figure();
hold on;
imagesc(Emag);
quiver(X, Y, Ex, Ey, "|E|² with E arrows");
hold off;
savefig("/tmp/rustlab_heatmap_quiver.html");

# ── 7. Overlay: equipotential contours + streamlines ────────────
figure();
hold on;
contour(X, Y, Vpot, 10, "k");
streamplot(X, Y, Ex, Ey, "Equipotentials and field lines");
hold off;
savefig("/tmp/rustlab_contour_stream.html");

# ── 8. Shortcut form: quiver(U, V) with default X, Y axes ───────
figure();
quiver(Ex, Ey, 0.5, "Indexed axes, scale = 0.5");
savefig("/tmp/rustlab_quiver_shortcut.svg");

print(1)   % sentinel for "we got this far without errors"

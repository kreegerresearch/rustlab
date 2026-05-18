# Contour plots — line contours, filled contours, and overlay on heatmap.
# Demonstrates: contour(X, Y, Z, ...), contourf(X, Y, Z, ...), hold on.
#
# Files written:
#   /tmp/rustlab_contour_lines.svg     line contours, default 10 auto levels
#   /tmp/rustlab_contour_lines.html    same, interactive Plotly
#   /tmp/rustlab_contour_fill.html     filled contours (12 bands)
#   /tmp/rustlab_contour_overlay.html  imagesc + contour overlay under hold on
#
# Note: contour/contourf are NOT rendered to the terminal. Save to .svg or
# .html and open the file to view.

# ── Build a scalar field ─────────────────────────────────────────
[X, Y] = meshgrid(linspace(-2, 2, 41), linspace(-2, 2, 41));
Z = X .^ 2 + Y .^ 2;          % radial paraboloid → concentric circle contours

# ── 1. Default line contours ────────────────────────────────────
figure();
contour(X, Y, Z);
savefig("/tmp/rustlab_contour_lines.svg");
savefig("/tmp/rustlab_contour_lines.html");

# ── 2. Explicit levels + line colour ────────────────────────────
figure();
contour(X, Y, Z, [0.5, 1, 2, 4], "k");   % four explicit levels in black
savefig("/tmp/rustlab_contour_explicit.svg");

# ── 3. Filled contours ──────────────────────────────────────────
figure();
contourf(X, Y, Z, 12);                     % 12 colour bands
savefig("/tmp/rustlab_contour_fill.html");

# ── 4. Overlay: heatmap + black line contours under hold on ─────
# This is the canonical EM equipotential pattern: a heatmap of |E| with
# contours of V on top.
figure();
hold on;
imagesc(Z);                                % heatmap (uses integer cell coords)
contour(X, Y, Z, 8, "k");                  % 8 levels overlaid; chart bounds
                                          % come from the contour, heatmap
                                          % rectangles auto-rescale to fit.
hold off;
savefig("/tmp/rustlab_contour_overlay.html");

# ── 5. Title and 1-arg form ─────────────────────────────────────
figure();
contour(Z, "Default-axis contour of x² + y²");
savefig("/tmp/rustlab_contour_titled.svg");

print(1)   % sentinel for "we got this far without errors"

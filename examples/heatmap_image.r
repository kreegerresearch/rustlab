# heatmap() and image() — labelled heatmaps and raw-pixel display.
# Demonstrates: heatmap(M), heatmap(xlabels, ylabels, M, ...),
#               image(M), image(M, cmap), image(R, G, B).
#
# Files written:
#   /tmp/rustlab_heatmap_simple.svg     unlabelled 6x6 confusion matrix
#   /tmp/rustlab_heatmap_simple.html    same, interactive Plotly
#   /tmp/rustlab_heatmap_labelled.svg   labelled with class names + colormap
#   /tmp/rustlab_heatmap_labelled.html
#   /tmp/rustlab_image_grayscale.svg    8x8 grayscale gradient
#   /tmp/rustlab_image_grayscale.html
#   /tmp/rustlab_image_colormap.svg     same data through a colormap
#   /tmp/rustlab_image_rgb.svg          true-colour RGB from three matrices
#   /tmp/rustlab_image_rgb.html
#
# Notes:
#   - heatmap renders row 0 at the top (image/data orientation).
#   - image clamps values to [0, 255] with NO min/max normalisation —
#     in contrast with imagesc, which auto-scales the data range.

# ── 1. Plain heatmap (no labels) ─────────────────────────────────
# A toy 6x6 confusion matrix: stronger diagonal = better classifier.
classes = {"cat", "dog", "fox", "owl", "bee", "ant"};
n = length(classes);
C = zeros(n, n);
for i = 1:n
  for j = 1:n
    if i == j
      C(i, j) = 80 + 5*sin(i);            # bright diagonal
    else
      C(i, j) = 8 * exp(-((i-j)^2) / 4);  # dim off-diagonal noise
    end
  end
end

figure();
heatmap(C, "raw confusion matrix");
savefig("/tmp/rustlab_heatmap_simple.svg");
savefig("/tmp/rustlab_heatmap_simple.html");

# ── 2. Heatmap with categorical axis labels ──────────────────────
figure();
heatmap(classes, classes, C, "labelled (default viridis)");
savefig("/tmp/rustlab_heatmap_labelled.svg");
savefig("/tmp/rustlab_heatmap_labelled.html");

# ── 3. Heatmap with explicit colormap ────────────────────────────
figure();
heatmap(classes, classes, C, "hot colormap", "hot");
savefig("/tmp/rustlab_heatmap_hot.html");

# ── 4. Grayscale image — values 0..255, no normalisation ─────────
# A diagonal gradient. In imagesc this would auto-normalise; image
# treats the same data as raw 0-255 brightness.
[II, JJ] = meshgrid(0:31, 0:31);
G = (II + JJ) * 4;     # values 0 .. 248

figure();
image(G);
savefig("/tmp/rustlab_image_grayscale.svg");
savefig("/tmp/rustlab_image_grayscale.html");

# ── 5. Single-channel through a colormap ─────────────────────────
# Same buffer as #4, but mapped through viridis. Compare with the
# grayscale render to see what the colormap argument does.
figure();
image(G, "viridis");
savefig("/tmp/rustlab_image_colormap.svg");

# ── 6. True-colour RGB from three real matrices ──────────────────
# Build a smooth procedural pattern: red rises with x, green rises with y,
# blue rises with x+y. The result is a familiar RGB-corner gradient.
R = II * 8;                      # 0 .. 248 along columns
B = JJ * 8;                      # 0 .. 248 along rows
Gc = (II + JJ) * 4;              # 0 .. 248 along the diagonal

figure();
image(R, Gc, B);
savefig("/tmp/rustlab_image_rgb.svg");
savefig("/tmp/rustlab_image_rgb.html");

# ── 7. Side-by-side: image vs imagesc on the same data ───────────
# This is the headline contrast: image keeps absolute brightness,
# imagesc rescales to fill the colormap.
figure();
subplot(1, 2, 1);
image(G);
title("image — clamped to 0..255");
subplot(1, 2, 2);
imagesc(G);
title("imagesc — auto-normalised");
savefig("/tmp/rustlab_image_vs_imagesc.html");

print(1)   % sentinel for "we got this far without errors"

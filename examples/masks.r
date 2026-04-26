# Geometry / shape rasterization masks.
# Demonstrates: rect_mask, disk_mask, polygon_mask, and how to compose them
# with element-wise math to build material-map-style geometry on a meshgrid.
#
# Files written:
#   /tmp/rustlab_mask_rect.html       single rectangle
#   /tmp/rustlab_mask_disk.html       single disk
#   /tmp/rustlab_mask_polygon.html    single polygon (triangle)
#   /tmp/rustlab_mask_compose.html    boolean composition: union, intersection,
#                                     complement, set difference
#   /tmp/rustlab_mask_device.html     a four-region "device" map built by
#                                     stacking masks with distinct material IDs
#   /tmp/rustlab_mask_pi.html         disk_mask integrated to estimate π

# ── Grid ────────────────────────────────────────────────────────
N = 200;
[X, Y] = meshgrid(linspace(-1.5, 1.5, N), linspace(-1.5, 1.5, N));

# ── 1. Single rectangle ─────────────────────────────────────────
R = rect_mask(X, Y, -0.8, -0.4, 1.2, 0.8);
figure();
imagesc(R);
savefig("/tmp/rustlab_mask_rect.html");

# ── 2. Single disk ──────────────────────────────────────────────
D = disk_mask(X, Y, 0.0, 0.0, 0.9);
figure();
imagesc(D);
savefig("/tmp/rustlab_mask_disk.html");

# ── 3. Single polygon (right triangle) ──────────────────────────
T = polygon_mask(X, Y, [-1.0,-1.0; 1.0,-1.0; 0.0,1.0]);
figure();
imagesc(T);
savefig("/tmp/rustlab_mask_polygon.html");

# ── 4. Boolean composition ──────────────────────────────────────
# Three classic set operations on 0/1 masks:
#   intersection  =  M1 .* M2
#   union         =  M1 + M2 - M1 .* M2   (inclusion–exclusion on 0/1)
#   complement    =  1 - M
#   set diff      =  M1 .* (1 - M2)
#
# Encode each region of (R, D) into a single matrix so one imagesc shows them:
#   0 = outside both, 1 = R only, 2 = D only, 3 = R ∩ D
both  = R .* D;
r_only = R .* (1 - D);
d_only = D .* (1 - R);
regions = 1 * r_only + 2 * d_only + 3 * both;

figure();
imagesc(regions);
savefig("/tmp/rustlab_mask_compose.html");

# ── 5. A four-region "device" geometry ──────────────────────────
# Substrate (large rectangle) with a circular via and a polygonal contact pad.
# Layered as material IDs 1..4 with later layers overwriting earlier ones via
# (1 - mask) gating — the standard idiom for stacking material maps.
substrate = rect_mask(X, Y, -1.2, -0.6, 2.4, 1.2);                # ID 1
via       = disk_mask(X, Y, -0.4, 0.0, 0.25);                     # ID 2
contact   = polygon_mask(X, Y, [0.2,-0.3; 0.9,-0.3; 0.9,0.3; 0.2,0.3]); # ID 3
trace     = rect_mask(X, Y, 0.2, -0.05, 0.7, 0.1);                # ID 4 (on top of contact)

device =        1 * substrate;
device = device .* (1 - via)     + 2 * via;
device = device .* (1 - contact) + 3 * contact;
device = device .* (1 - trace)   + 4 * trace;

figure();
imagesc(device);
savefig("/tmp/rustlab_mask_device.html");

# ── 6. Numerical sanity: disk_mask integrates to π ──────────────
# Build a fresh unit disk and integrate it. Cell area is step², so
# sum(D) * step² approximates the disk area π·r² = π.
unit_disk = disk_mask(X, Y, 0.0, 0.0, 1.0);
step = 3.0 / (N - 1);
area = sum(sum(unit_disk)) * step * step;
print(area)             # → ~3.14
print(area - pi)        # → small offset from finite-grid sampling

figure();
imagesc(unit_disk);
savefig("/tmp/rustlab_mask_pi.html");

print("done — open the .html files to view")

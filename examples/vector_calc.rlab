# Vector calculus on uniform 2-D grids
# Demonstrates: gradient(F), divergence(Fx, Fy), curl(Fx, Fy)
#
# Grid convention: F(i, j) ↔ position (x = (j-1)*dx, y = (i-1)*dy).
# Rows index y, columns index x — matches Octave / NumPy.
# Trailing `;` suppresses implicit echo for assignments.

# ── Build a uniform grid ─────────────────────────────────────────
dx = 0.1;
dy = 0.1;
xs = -1:dx:1;                          # 21 points across [-1, 1]
ys = -1:dy:1;
[X, Y] = meshgrid(xs, ys);              # both 21×21

# ── 1. gradient of a scalar field ───────────────────────────────
# F(x, y) = x² + y²  →  ∇F = (2x, 2y)
F = X .^ 2 + Y .^ 2;
[Fx, Fy] = gradient(F, dx, dy);

# Centre cell (x = y = 0): both components ≈ 0
print(Fx(11, 11))                      # ≈ 0
print(Fy(11, 11))                      # ≈ 0

# Top-right corner (x = 1, y = 1): boundary one-sided ≈ (2, 2)
print(Fx(21, 21))                      # ≈ 2
print(Fy(21, 21))                      # ≈ 2

# ── 2. divergence of a vector field ─────────────────────────────
# F = (x, y)  →  ∇·F = 2 everywhere
D = divergence(X, Y, dx, dy);
print(D(11, 11))                       # ≈ 2
print(D(1, 1))                         # ≈ 2 (boundary one-sided)

# ── 3. curl of a vector field ───────────────────────────────────
# F = (-y, x)  →  ∇×F · ẑ = 2 (solid-body rotation)
Cz = curl(-Y, X, dx, dy);
print(Cz(11, 11))                      # ≈ 2

# F = (x, y) is irrotational  →  curl = 0
Cz_irr = curl(X, Y, dx, dy);
print(Cz_irr(11, 11))                  # ≈ 0

# ── 4. Combine: ∇²V = ∇·(∇V)  (the Laplacian) ───────────────────
# V(x, y) = x² + y²  →  ∇²V = 4 everywhere
[Vx, Vy] = gradient(F, dx, dy);
laplV = divergence(Vx, Vy, dx, dy);
print(laplV(11, 11))                   # ≈ 4

# ── 5. Complex inputs (frequency-domain field) ──────────────────
# F(x, y) = exp(j*x)  →  ∂F/∂x = j*exp(j*x), ∂F/∂y = 0
Fc = exp(j * X);
[Fxc, Fyc] = gradient(Fc, dx, dy);
print(Fxc(11, 11))                     # ≈ j*exp(0) = 0 + j
print(Fyc(11, 11))                     # ≈ 0

# ── Notes ───────────────────────────────────────────────────────
# • Each axis must have length ≥ 3 (for the 2nd-order one-sided stencil).
# • If you omit dx and dy, both default to 1.0:
#     [Fx, Fy] = gradient(F)
# • Same defaults for divergence(Fx, Fy) and curl(Fx, Fy).

# ── 6. 3-D variants: gradient3 / divergence3 / curl3 ────────────
# 3-D grid convention: F(i, j, k) ↔ (x = (j-1)*dx, y = (i-1)*dy, z = (k-1)*dz).
# Inputs and outputs are Tensor3s of shape (ny, nx, nz).

nx3 = 5;
ny3 = 5;
nz3 = 5;
dx3 = 0.25;
dy3 = 0.25;
dz3 = 0.25;

# Build the per-page X, Y matrices once with meshgrid, then stack pages with cat(3, ...).
xs3 = (0:nx3-1) * dx3;
ys3 = (0:ny3-1) * dy3;
zs3 = (0:nz3-1) * dz3;
[Xp, Yp] = meshgrid(xs3, ys3);     # both ny3×nx3

X3 = Xp;
Y3 = Yp;
Z3 = zs3(1) * ones(ny3, nx3);
for k = 2:nz3
  X3 = cat(3, X3, Xp);
  Y3 = cat(3, Y3, Yp);
  Z3 = cat(3, Z3, zs3(k) * ones(ny3, nx3));
end

# F = x² + y² + z²  →  ∇F = (2x, 2y, 2z),  ∇²F = 6
F3 = X3 .^ 2 + Y3 .^ 2 + Z3 .^ 2;
[Fx3, Fy3, Fz3] = gradient3(F3, dx3, dy3, dz3);

# Centre cell (i=3, j=3, k=3): x = y = z = 0.5  →  ∇F = (1, 1, 1)
print(Fx3(3, 3, 3))        # ≈ 1
print(Fy3(3, 3, 3))        # ≈ 1
print(Fz3(3, 3, 3))        # ≈ 1

# Divergence of (X, Y, Z) is 3 everywhere
D3 = divergence3(X3, Y3, Z3, dx3, dy3, dz3);
print(D3(3, 3, 3))         # ≈ 3
print(D3(1, 1, 1))         # ≈ 3 (boundary one-sided)

# Curl of solid rotation (-Y, X, 0)  →  (0, 0, 2)
Zero3 = zeros3(ny3, nx3, nz3);
[Cx3, Cy3, Cz3] = curl3(-Y3, X3, Zero3, dx3, dy3, dz3);
print(Cx3(3, 3, 3))        # ≈ 0
print(Cy3(3, 3, 3))        # ≈ 0
print(Cz3(3, 3, 3))        # ≈ 2

# Laplacian via composition: ∇·(∇F) = ∇²F = 6 for F = x²+y²+z²
laplF3 = divergence3(Fx3, Fy3, Fz3, dx3, dy3, dz3);
print(laplF3(3, 3, 3))     # ≈ 6

# Notes for 3-D:
# • Inputs to gradient3 / divergence3 / curl3 must be Tensor3 (use reshape(...)
#   or cat(3, ...) to build them — there is no broadcasting from Matrix).
# • Each axis (rows, cols, pages) must have length ≥ 3.
# • dx, dy, dz default to 1.0 if all are omitted.

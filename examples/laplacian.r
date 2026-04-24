# Laplacian stencil — canonical Poisson solve.
# Demonstrates: laplacian_2d(nx, ny, dx, dy), ij2k, k2ij, column-major ordering,
# reshape / (:) / spsolve composition.
#
# Files written:
#   /tmp/rustlab_laplacian_solution.html   imagesc of V for analytic source
#   /tmp/rustlab_laplacian_point_source.html   point-charge style potential

# ── 1. Build the stencil ────────────────────────────────────────
nx = 33; ny = 25;
dx = 0.1;  dy = 0.1;
L = laplacian_2d(nx, ny, dx, dy);
print(issparse(L))       # → 1
print(nnz(L))

# ── 2. Column-major index convention ────────────────────────────
# V(i, j) → k = (j-1)*ny + i  (third arg to ij2k is ny, not nx).
k = ij2k(3, 4, ny);
print(k)                  # → (4-1)*25 + 3 = 78
[ri, rj] = k2ij(k, ny);
print(ri)                 # → 3
print(rj)                 # → 4

# ── 3. Analytic eigenfunction check ─────────────────────────────
# V(x, y) = sin(pi*x/Lx) * sin(pi*y/Ly) is zero on the boundary and an
# eigenfunction of the continuous Laplacian. Discretising on our grid
# gives back the same V (to numerical precision) when we apply L and
# solve. This is the cleanest self-consistency check.
Lx = (nx + 1) * dx;
Ly = (ny + 1) * dy;
V_exact = zeros(ny, nx);
for jj = 1:nx
  for ii = 1:ny
    V_exact(ii, jj) = sin(pi*ii*dx/Lx) * sin(pi*jj*dy/Ly);
  end
end

# V(:) in rustlab gives a ROW vector in column-major order; transpose to
# get a column vector suitable for L * v and spsolve.
v_exact = V_exact(:)';
rhs = full(L) * v_exact;
v_solved = spsolve(L, rhs);

# Relative residual should be < 1e-10.
rel_err = norm(v_solved' - v_exact) / norm(v_exact);
print(rel_err)

# Visualise the recovered solution.
figure();
V_solved = reshape(v_solved, ny, nx);
imagesc(V_solved);
savefig("/tmp/rustlab_laplacian_solution.html");

# ── 4. Point-charge style source ────────────────────────────────
# Deposit a unit source at the grid centre and solve ∇²V = -rho.
# Sign: L approximates +∇², so Poisson V = spsolve(L, -rho).
rho = zeros(ny, nx);
i_mid = round(ny / 2);
j_mid = round(nx / 2);
rho(i_mid, j_mid) = 1.0;

rhs2 = -rho(:)';
V2_flat = spsolve(L, rhs2);
V2 = reshape(V2_flat, ny, nx);

figure();
imagesc(V2, "jet");
savefig("/tmp/rustlab_laplacian_point_source.html");

print(1)   % sentinel for "we got this far without errors"

# Laplacian boundary conditions — Dirichlet, Neumann, Periodic.
# Demonstrates: laplacian_2d with each BC variant. Shows that Dirichlet
# is non-singular (solves directly with spsolve), while Neumann and
# Periodic have constants in their null space (verified numerically).
#
# Files written:
#   /tmp/rustlab_lap_bc_dirichlet.html   solution under V=0 boundary
#   /tmp/rustlab_lap_bc_dirichlet_diff.html
#                                        difference between Dirichlet and
#                                        a "padded" Neumann/Periodic check

# ── 1. Common grid + source ─────────────────────────────────────
nx = 60; ny = 60;
dx = 0.05; dy = 0.05;
src = zeros(ny, nx);
src(20, 20) =  1.0;
src(40, 40) = -1.0;
b = src(:)';

# ── 2. Dirichlet — V = 0 outside the grid ───────────────────────
# Standard non-singular system; spsolve auto-routes to Cholesky.
A_d = -1 * laplacian_2d(nx, ny, dx, dy, "dirichlet");
v_d = spsolve(A_d, b);
V_d = reshape(v_d, ny, nx);
figure();
imagesc(V_d);
title("Dirichlet — V=0 at boundary");
savefig("/tmp/rustlab_lap_bc_dirichlet.html");

# ── 3. Neumann — zero normal flux ───────────────────────────────
# The matrix is singular. Verify by checking that constants are in the
# null space: applying the Laplacian to a vector of all 1s gives 0.
# (A direct solve would need pin-and-solve; that's a script-level
# pattern not built into the dispatch yet.)
A_n = laplacian_2d(nx, ny, dx, dy, "neumann");
ones_vec = ones(ny * nx, 1);
test_n = full(A_n) * ones_vec;
print("Neumann null-space residual norm (should be ~0):");
print(norm(test_n));

# ── 4. Periodic — wrap-around ───────────────────────────────────
# Also singular. Same null-space property — constants are in the kernel.
A_p = laplacian_2d(nx, ny, dx, dy, "periodic");
test_p = full(A_p) * ones_vec;
print("Periodic null-space residual norm (should be ~0):");
print(norm(test_p));

# ── 5. Show the structural difference: nnz of each ──────────────
# Dirichlet drops cross-boundary entries (lowest nnz).
# Neumann keeps the diagonal but skips off-diagonal at boundaries.
# Periodic adds wrap entries (highest nnz).
print("nnz under each BC:");
print(nnz(A_d));         % drops boundary off-diagonals; same diag as interior
print(nnz(A_n));         % same off-diagonal pattern as Dirichlet
print(nnz(A_p));         % adds wrap-around entries

# ── 6. 1-D Laplacian — fastest sanity check ─────────────────────
# Also demonstrates laplacian_1d with each BC; useful for solving
# diffusion / heat / wave equations on a periodic ring.
L1_d = laplacian_1d(8, 1.0, "dirichlet");
L1_n = laplacian_1d(8, 1.0, "neumann");
L1_p = laplacian_1d(8, 1.0, "periodic");
print("1-D nnz: dirichlet, neumann, periodic:");
print([nnz(L1_d), nnz(L1_n), nnz(L1_p)]);

print("done — open /tmp/rustlab_lap_bc_dirichlet.html to view the Dirichlet solve")

# Sparse direct solve — auto-dispatched Cholesky vs dense LU.
# Demonstrates: spsolve(A, b), spsolve(A, b, "cholesky"), spsolve(A, b, "lu"),
# auto SPD detection, scaling on a Laplacian Poisson problem.
#
# Files written:
#   /tmp/rustlab_spsolve_solution.html   imagesc of V for a centred-source
#                                        Poisson on a 50x50 grid (sparse path)
#
# Note: the dense-LU fallback is exercised on a *small* 20x20 grid only,
# because dense Gaussian elimination on the 50x50 case is hundreds of
# seconds in a debug build. The whole point of the sparse Cholesky path
# is that it scales to grids the dense path can't.

# ── 1. Big SPD problem: 50x50 Laplacian, sparse Cholesky ────────
# laplacian_2d returns +∇²; negate so the matrix is SPD.
nx = 50;  ny = 50;
dx = 0.02; dy = 0.02;
L  = laplacian_2d(nx, ny, dx, dy);
A  = -1 * L;             % SPD: -∇² with homogeneous Dirichlet
print(issparse(A))       % → 1
print(nnz(A))            % → ~12k for a 5-point stencil with Dirichlet trims

# Centred point source.
rho = zeros(ny, nx);
rho(ny/2, nx/2) = 1.0;
b   = -1 * rho(:)';      % solving -∇² V = ρ → V = spsolve(A, b)

# Default dispatch: -L is SPD, so auto picks the sparse Cholesky path.
print("Solving 2500x2500 Poisson via auto dispatch (sparse Cholesky)...")
v_auto = spsolve(A, b);
print(length(v_auto))    % → 2500

# Force the Cholesky path explicitly — should agree.
v_chol = spsolve(A, b, "cholesky");
print(norm(v_auto - v_chol))    % → ~0  (auto and forced agree exactly)

# ── 2. Visualise the solution ───────────────────────────────────
V = reshape(v_auto, ny, nx);
figure();
imagesc(V);
title("Poisson solution: -∇² V = δ at grid centre");
savefig("/tmp/rustlab_spsolve_solution.html");

# ── 3. Small problem: compare all three modes side by side ──────
# 20x20 grid = 400 unknowns. Dense LU is fine at this size.
nx_s = 20; ny_s = 20;
A_s  = -1 * laplacian_2d(nx_s, ny_s);
b_s  = ones(nx_s * ny_s, 1);
v_a  = spsolve(A_s, b_s);
v_c  = spsolve(A_s, b_s, "cholesky");
v_l  = spsolve(A_s, b_s, "lu");
print(norm(v_a - v_c))      % → 0.0   (same path)
print(norm(v_a - v_l))      % → ~1e-13 (different fp accumulation order)

# ── 4. Cholesky catches non-SPD inputs ──────────────────────────
# A symmetric-but-indefinite matrix should NOT factor via Cholesky.
# auto detects this (via is_spd_estimate) and falls through to dense LU.
S = sparse([1, 2; 2, 1]);          % symmetric, eigenvalues 3 and -1
x = spsolve(S, [1; 1]);            % auto → falls back to LU, succeeds
print(x)                           % → [0.333..., 0.333...]

# Forcing cholesky on this matrix would error; the line is left
# commented out so the example runs to completion.
% x_bad = spsolve(S, [1; 1], "cholesky");   % would error: not SPD

print("done — open /tmp/rustlab_spsolve_solution.html to view")

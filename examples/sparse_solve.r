# Sparse direct solve — auto-dispatched Cholesky vs LU, both sparse.
# Demonstrates: spsolve(A, b), spsolve(A, b, "cholesky"), spsolve(A, b, "lu"),
# auto SPD detection, sparse LU on indefinite matrices, scaling.
#
# Files written:
#   /tmp/rustlab_spsolve_solution.html   imagesc of V for a centred-source
#                                        Poisson on a 50x50 grid (sparse path)
#   /tmp/rustlab_spsolve_indefinite.html imagesc for an indefinite assembly
#                                        solved via sparse LU

# ── 1. SPD problem: 50x50 Laplacian, sparse Cholesky ────────────
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
b   = -1 * rho(:)';

# Default dispatch: -L is SPD, so auto picks the sparse Cholesky path.
print("Solving 2500x2500 Poisson via auto dispatch (sparse Cholesky)...")
v_auto = spsolve(A, b);
print(length(v_auto))    % → 2500

# Force Cholesky explicitly — should agree.
v_chol = spsolve(A, b, "cholesky");
print(norm(v_auto - v_chol))    % → ~0

# Force the sparse LU path on the same matrix. LU always works on
# square non-singular matrices and gives the same answer (different fp
# accumulation, so a tiny relative-norm difference).
v_lu = spsolve(A, b, "lu");
print(norm(v_auto - v_lu))      % → ~1e-9 .. 1e-12

# ── 2. Visualise the SPD solution ───────────────────────────────
V = reshape(v_auto, ny, nx);
figure();
imagesc(V);
title("Poisson solution: -∇² V = δ at grid centre");
savefig("/tmp/rustlab_spsolve_solution.html");

# ── 3. Indefinite assembly: sparse LU does the work ─────────────
# A non-Hermitian or indefinite matrix can't go through Cholesky.
# Auto's SPD pre-check catches this and routes the solve through the
# hand-rolled sparse LU with partial pivoting.
nx_i = 30; ny_i = 30;
L_i = laplacian_2d(nx_i, ny_i);
B = -1 * L_i + 0.5 * speye(nx_i * ny_i);   % shifted Helmholtz-like, indefinite
b_i = ones(nx_i * ny_i, 1);

print("Solving 900x900 indefinite system via auto dispatch (sparse LU)...")
v_i = spsolve(B, b_i);
print(length(v_i))       % → 900

V_i = reshape(v_i, ny_i, nx_i);
figure();
imagesc(V_i);
title("Indefinite assembly solved via sparse LU");
savefig("/tmp/rustlab_spsolve_indefinite.html");

# ── 4. The "auto" dispatch is the easy path ─────────────────────
# Forcing cholesky on this matrix would error cleanly — useful when
# you want to assert the structure of an assembly during development.
# Auto silently falls through to LU, which is the right behaviour for
# scripts that mix SPD and indefinite solves.
% would_error = spsolve(B, b_i, "cholesky");

# ── 5. Tiny indefinite, all three modes ─────────────────────────
S = sparse([1, 2; 2, 1]);          % indefinite (eigenvalues 3, -1)
x_a = spsolve(S, [1; 1]);          % auto → LU (SPD pre-check rejects)
x_l = spsolve(S, [1; 1], "lu");    % explicit
print(x_a)                         % → [1/3, 1/3]
print(x_l)                         % → same

print("done — open the .html files to view")

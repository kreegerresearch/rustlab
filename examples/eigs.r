# Sparse partial eigensolver — Laplacian eigenmodes.
# Demonstrates: eigs(A, n, "sm"|"lm"), eigs(A, B, n) generalized form,
# reshape of eigenvectors to grid for visualization.
#
# Files written:
#   /tmp/rustlab_eigs_mode1.html    smallest eigenmode (m=n=1)
#   /tmp/rustlab_eigs_mode2.html    second smallest
#   /tmp/rustlab_eigs_mode3.html    third smallest

# ── 1. Build the SPD Laplacian on a non-square grid ─────────────
# Non-square breaks the m↔n symmetry so eigenvalues stay distinct
# (square grids have multiplicity-2 degeneracies that simple Lanczos
# can't enumerate without restart).
nx = 12; ny = 8;
dx = 0.05; dy = 0.05;
L = -1 * laplacian_2d(nx, ny, dx, dy);
n = nx * ny;
print(issparse(L))      % → 1
print(n)                % → 96

# ── 2. Solve for the four smallest eigenmodes ───────────────────
print("Solving for 4 smallest eigenmodes via sparse Lanczos...");
[V, D] = eigs(L, 4, "sm");
print(length(D))        % → 4

% Eigenvalues approximately:
%   λ_{m,n} = (2/dx²)(1 - cos(mπ/(nx+1))) + (2/dy²)(1 - cos(nπ/(ny+1)))
% smallest is at (m, n) = (1, 1).
print(D)

# ── 3. Visualise the eigenmodes ─────────────────────────────────
% Each column of V is an eigenvector flattened in column-major order
% (matching laplacian_2d's k = (j-1)*ny + i convention). Reshape back
% to the (ny, nx) grid for imagesc.
for k = 1:3
  mode = real(V(:, k));
  M = reshape(mode, ny, nx);
  figure();
  imagesc(M);
  title(sprintf("Mode %d (λ = %.4f)", k, real(D(k))));
  savefig(sprintf("/tmp/rustlab_eigs_mode%d.html", k));
end

# ── 4. Largest-magnitude eigenmode ──────────────────────────────
[Vlm, Dlm] = eigs(L, 1, "lm");
print(Dlm(1))           % largest eigenvalue

# ── 5. Generalized eigenproblem: A x = λ B x with B = 2*I ───────
% For B = c*I, the generalized eigenvalues are eigs(A) / c.
% Here we expect the smallest to be the smallest eigenvalue of L
% divided by 2.
B = 2 * speye(n);
[Vg, Dg] = eigs(L, B, 1, "sm");
print(Dg(1))            % ≈ D(1) / 2

print("done — open /tmp/rustlab_eigs_mode*.html to view")

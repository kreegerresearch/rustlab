# Complex-valued sparse solve — lossy Helmholtz / FDFD-style assembly.
# Demonstrates: spsolve auto-routing on a complex non-Hermitian matrix
# (sparse LU with partial pivoting), real-vs-complex auto-routing.
#
# Files written:
#   /tmp/rustlab_complex_real.html   real part of solution
#   /tmp/rustlab_complex_imag.html   imag part of solution
#   /tmp/rustlab_complex_mag.html    |V| magnitude

# ── 1. Build a complex shifted-Helmholtz operator on a 60x60 grid ──
# A_h = -∇² - (k² - j*α) I, where α models a small bulk loss (skin-depth
# style absorption). Hermitian-violating: imaginary part on the
# diagonal makes A non-Hermitian, so the SPD pre-check rejects it and
# auto routes through the sparse LU path.
nx = 60; ny = 60;
dx = 0.05; dy = 0.05;
L_neg = -1 * laplacian_2d(nx, ny, dx, dy);

n = nx * ny;
k0 = 4.0;             % wavenumber
alpha = 0.5;          % bulk-loss term
shift = -1 * (k0 * k0) + j * alpha;
A = L_neg + shift * speye(n);
print(issparse(A))    % → 1
print(nnz(A))         % → ~14k

# ── 2. A point source in the middle of the grid ──────────────────
% Lay the source out on a 2-D (ny, nx) grid and flatten column-major
% to match laplacian_2d's index convention.
src = zeros(ny, nx);
src(ny/2, nx/2) = 1.0;
b = src(:)';

# ── 3. Solve. Auto detects non-Hermitian and runs sparse LU. ─────
# Both factor and solve operate on Complex<f64> CSC storage. The
# real-vs-complex auto-routing kicks in inside the dispatch: since b
# happens to have zero imaginary part, only A's complex entries force
# the complex path.
print("Solving 3600x3600 complex non-Hermitian system via sparse LU...");
v = spsolve(A, b);
V = reshape(v, ny, nx);

# ── 4. Inspect the solution ──────────────────────────────────────
% Real, imag, and magnitude of V each carry physical meaning in
% frequency-domain wave problems: real and imag combine as the phasor
% amplitude; |V| is what an intensity sensor would measure.

figure();
imagesc(real(V));
title("Re(V) — standing-wave pattern with loss");
savefig("/tmp/rustlab_complex_real.html");

figure();
imagesc(imag(V));
title("Im(V) — out-of-phase response");
savefig("/tmp/rustlab_complex_imag.html");

figure();
imagesc(abs(V));
title("|V| — phasor magnitude");
savefig("/tmp/rustlab_complex_mag.html");

# ── 5. Verify the residual ───────────────────────────────────────
% norm(A*v - b) should be small. Cast v as a column vector for the
% sparse-matrix multiply.
r = A * transpose(v) - b;
print(norm(r));         % → tiny

print("done — open the .html files to view")

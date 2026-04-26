# Electrostatics — multi-source Poisson at curriculum scale.
# Demonstrates: laplacian_2d, spsolve (auto -> sparse Cholesky on SPD),
# gradient (electric field from potential), quiver overlay.
#
# Files written:
#   /tmp/rustlab_estatics_potential.html  imagesc of V on 100x100 grid
#   /tmp/rustlab_estatics_field.html      quiver of E = -∇V over imagesc(V)

# ── 1. Grid + sparse SPD operator ───────────────────────────────
nx = 100; ny = 100;
dx = 0.01; dy = 0.01;
A  = -1 * laplacian_2d(nx, ny, dx, dy);   % -∇² is SPD with Dirichlet
n  = nx * ny;

# ── 2. Multi-charge source distribution ─────────────────────────
# Four point charges in a grounded box: two positive, two negative.
# Source density rho lives on the (ny, nx) grid; flatten column-major
# for the linear solve: -∇² V = ρ → V = spsolve(A, ρ(:)').
rho = zeros(ny, nx);
rho(30, 30) =  1.0;
rho(30, 70) = -1.0;
rho(70, 30) = -1.0;
rho(70, 70) =  1.0;
b = rho(:)';

# ── 3. Solve. Auto dispatch picks the sparse Cholesky path. ─────
print("Solving 10000x10000 Poisson via sparse Cholesky...");
v = spsolve(A, b);
V = reshape(v, ny, nx);

# ── 4. Plot the potential ───────────────────────────────────────
figure();
imagesc(V);
title("Electrostatic potential — four-charge quadrupole");
savefig("/tmp/rustlab_estatics_potential.html");

# ── 5. Compute the electric field E = -∇V and overlay on V ──────
[Vx, Vy] = gradient(V, dx, dy);
Ex = -Vx;
Ey = -Vy;

# Coarsen the quiver grid so arrows aren't a forest of lines.
step = 10;
xs = (1:step:nx) * dx;
ys = (1:step:ny) * dy;
[Xc, Yc] = meshgrid(xs, ys);
Exc = Ex(1:step:ny, 1:step:nx);
Eyc = Ey(1:step:ny, 1:step:nx);

figure();
hold on;
imagesc(V);
quiver(Xc, Yc, Exc, Eyc);
title("Electric field E = -∇V over potential");
savefig("/tmp/rustlab_estatics_field.html");

print("done — open the .html files to view")

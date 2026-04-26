# Variable-coefficient Poisson — dielectric slab in vacuum.
# Demonstrates: laplacian_eps_2d with a piecewise eps_map, showing how
# the harmonic-mean half-cell discretization correctly handles the
# discontinuous interface.
#
# Files written:
#   /tmp/rustlab_dielectric_eps.html       eps_map (material distribution)
#   /tmp/rustlab_dielectric_v.html         potential V solved on this map
#   /tmp/rustlab_dielectric_field.html     |E| magnitude with E vectors
#                                          overlaid

# ── 1. Grid + dielectric slab geometry ──────────────────────────
# A 100x100 grid. Left half is vacuum (eps_r = 1), right half is a
# dielectric slab (eps_r = 4 — typical of glass). The interface runs
# vertically at j = nx/2.
nx = 100; ny = 100;
dx = 0.01; dy = 0.01;
eps0 = 8.854e-12;       % vacuum permittivity (SI)

eps_map = ones(ny, nx);
for jj = (nx/2):nx
  for ii = 1:ny
    eps_map(ii, jj) = 4.0;       % dielectric slab on the right half
  end
end

figure();
imagesc(real(eps_map));
title("Relative permittivity ε_r");
savefig("/tmp/rustlab_dielectric_eps.html");

# ── 2. Build the variable-coefficient operator ──────────────────
% Sign convention: laplacian_eps_2d returns +∇·(ε∇V), so for Poisson
% ∇·(ε∇V) = -ρ/eps0 we negate to get an SPD operator.
A = -1 * laplacian_eps_2d(eps_map, dx, dy);
print(issparse(A))         % → 1
print(nnz(A))              % → ~50k

# ── 3. Source: a positive point charge in the vacuum half ───────
rho = zeros(ny, nx);
rho(50, 25) = 1.0;             % point charge in the vacuum half
b = -1 * rho(:)' / eps0;       % RHS for ∇·(ε∇V) = -ρ/eps0

# ── 4. Solve. Auto detects SPD and routes to sparse Cholesky. ───
v = spsolve(A, b);
V = reshape(v, ny, nx);

figure();
imagesc(real(V));
title("Potential V — point charge in vacuum next to dielectric slab");
savefig("/tmp/rustlab_dielectric_v.html");

# ── 5. Electric field E = -∇V ───────────────────────────────────
% Note the field magnitude drops sharply at the interface (j = 50):
% inside the dielectric, the field is reduced by a factor of eps_r.
% This is the physical effect that the harmonic-mean discretization
% captures exactly across the material boundary.
[Vx, Vy] = gradient(V, dx, dy);
Ex = -Vx;
Ey = -Vy;
E_mag = sqrt(real(Ex) .^ 2 + real(Ey) .^ 2);

step = 8;
xs = (1:step:nx) * dx;
ys = (1:step:ny) * dy;
[Xc, Yc] = meshgrid(xs, ys);
Exc = real(Ex(1:step:ny, 1:step:nx));
Eyc = real(Ey(1:step:ny, 1:step:nx));

figure();
hold on;
imagesc(E_mag);
quiver(Xc, Yc, Exc, Eyc);
title("|E| with field arrows — interface at j = 50 visible as a discontinuity");
savefig("/tmp/rustlab_dielectric_field.html");

# ── 6. Sanity check: eps_map ≡ 1 reproduces laplacian_2d ────────
eps_unit = ones(ny, nx);
A_eps_unit = laplacian_eps_2d(eps_unit, dx, dy);
A_lap     = laplacian_2d(nx, ny, dx, dy);
diff = full(A_eps_unit) - full(A_lap);
print("max difference between eps=1 form and laplacian_2d:");
print(norm(diff(:)));        % → ~0

print("done — open the .html files to view")

# Variable-Coefficient Poisson — Dielectric Slab

For a piecewise-uniform dielectric medium, Poisson's equation becomes

$$-\nabla \cdot (\varepsilon \nabla V) = \rho$$

The naive approach — discretize $\nabla \cdot$ and $\nabla$ separately
with arithmetic-mean coefficients — produces artificial sources at
material interfaces. The physically correct discretization uses
**harmonic-mean half-cell coefficients**:

$$\varepsilon_{i, j+1/2} = \frac{2 \, \varepsilon(i, j) \, \varepsilon(i, j+1)}{\varepsilon(i, j) + \varepsilon(i, j+1)}$$

This preserves flux continuity at material boundaries — exactly what
Maxwell's equations demand. `laplacian_eps_2d` builds the resulting
sparse operator in one call.

## Geometry

A 100×100 grid with vacuum on the left half ($\varepsilon_r = 1$) and
a dielectric slab on the right half ($\varepsilon_r = 4$). The
material interface runs vertically through the middle of the grid.

```rustlab
clf
nx = 100; ny = 100;
dx = 0.01; dy = 0.01;
eps0 = 8.854e-12;       % vacuum permittivity (SI)

eps_map = ones(ny, nx);
for jj = (nx/2):nx
  for ii = 1:ny
    eps_map(ii, jj) = 4.0;
  end
end

imagesc(real(eps_map));
title("Relative permittivity ε_r")
```

The interface at $j = 50$ is sharp — a step from 1 to 4.

## Building the operator

```rustlab
A = -1 * laplacian_eps_2d(eps_map, dx, dy);    % SPD form
print(issparse(A))    % → 1
print(nnz(A))         % → ~50k
```

`laplacian_eps_2d` returns $+\nabla \cdot (\varepsilon \nabla)$, so we
negate to get an SPD operator that auto-routes to sparse Cholesky.

## Solving with a point charge in the vacuum half

```rustlab
rho = zeros(ny, nx);
rho(50, 25) = 1.0;             % point charge at (50, 25)
b = -1 * rho(:)' / eps0;
v = spsolve(A, b);
V = reshape(v, ny, nx);

clf
imagesc(real(V));
title("Potential V — point charge in vacuum next to dielectric slab")
```

Two things to notice:

1. The potential is asymmetric: it drops faster on the dielectric side
   because the dielectric "screens" the charge. This is the physically
   correct behaviour and would NOT come out right if we used arithmetic-
   mean coefficients.

2. The interface at $j = 50$ shows a kink in the contour lines —
   exactly where you'd expect a flux-continuous solution to bend.

## Electric field — the discontinuity is visible

```rustlab
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

clf
hold on;
imagesc(E_mag);
quiver(Xc, Yc, Exc, Eyc);
title("|E| with field arrows — interface jump at j = 50")
```

Inside the dielectric, the field magnitude is reduced by a factor of
$\varepsilon_r = 4$ relative to what it would be in pure vacuum. The
arrow density and length both drop sharply at the boundary — that's
the dielectric "screening" the charge as expected.

## Sanity check — `eps_map ≡ 1` reproduces `laplacian_2d`

```rustlab
eps_unit = ones(ny, nx);
A_eps_unit = laplacian_eps_2d(eps_unit, dx, dy);
A_lap     = laplacian_2d(nx, ny, dx, dy);
print(norm(full(A_eps_unit) - full(A_lap)))    % → 0
```

When the material is uniform, the harmonic mean of two equal values is
just the value itself, and the variable-coefficient form collapses
exactly to the constant-coefficient `laplacian_2d`. This isn't just an
approximation — they agree to machine precision.

## Other use cases

The same builder serves several physically-distinct problems by choice
of input:

| Operator | Physical meaning | Input |
|---|---|---|
| `laplacian_eps_2d(eps)` | electrostatics in a dielectric | $\varepsilon_r$ |
| `laplacian_eps_2d(1.0 ./ mu)` | magnetostatics with $A_z$ | $1/\mu_r$ |
| `laplacian_eps_2d(k)` | steady-state heat conduction | thermal conductivity $k$ |

Each is "the same operator with a different physical interpretation
of the coefficient". The harmonic-mean discretization is correct for
all of them — that's why it's the standard choice in computational
physics.

## Cheat sheet

| Form | Notes |
|---|---|
| `laplacian_eps_2d(eps_map)` | unit spacing, Dirichlet |
| `laplacian_eps_2d(eps_map, dx, dy)` | uniform spacing, Dirichlet |
| `laplacian_eps_2d(eps_map, bc)` | unit spacing with bc |
| `laplacian_eps_2d(eps_map, dx, dy, bc)` | full form |

`eps_map` is shape `(ny, nx)` — the meshgrid / imagesc convention.
Real or complex (lossy) entries supported. `bc` is one of
`"dirichlet"`, `"neumann"`, or `"periodic"`.

# Sparse Direct Solves — `spsolve`

The `spsolve` builtin solves `A·x = b` for a sparse coefficient matrix `A`.
Until recently it densified `A` internally and ran Gaussian elimination —
fine for toy problems but a hard wall at curriculum scale. A 100×100
Lesson-05 grid produces a 10⁴×10⁴ matrix; densifying it allocates roughly
800 MB and the solve runs in cubic time.

This notebook walks through the new sparse path: a hand-rolled
left-looking Cholesky for Hermitian-positive-definite matrices, with
auto-detection that routes SPD assemblies to the fast path and falls
back to dense LU for everything else.

## A Hermitian-positive-definite assembly

The 5-point Laplacian stencil with homogeneous Dirichlet boundaries is
the canonical SPD assembly in numerical PDE work. `laplacian_2d` returns
$+\nabla^2$, so the operator we want for an SPD Poisson solve is $-L$:

```rustlab
nx = 50;  ny = 50;
dx = 0.02; dy = 0.02;
L  = laplacian_2d(nx, ny, dx, dy);
A  = -1 * L;             % SPD: -∇² with homogeneous Dirichlet
n  = nx * ny;
print(issparse(A))       % → 1
print(nnz(A))            % → ~12k (5-point stencil, modulo boundary trims)
```

The matrix has $n = 2500$ unknowns and roughly 12 000 non-zeros — a 0.2 %
density. Densified, it's a 2500 × 2500 complex matrix taking about 80 MB.
Sparse, it fits in well under 1 MB.

## Default dispatch — `spsolve` picks the right path

`spsolve(A, b)` has a third optional argument `mode` that takes
`"auto"` (the default), `"cholesky"`, or `"lu"`. With `"auto"`, the
solver:

1. Tests whether `A` is Hermitian (mirrored entries match within
   tolerance) and whether all diagonals are real-positive — a cheap
   pre-filter that's necessary for SPD.
2. If both checks pass, factor with the hand-rolled sparse Cholesky.
3. If either check fails, or if the Cholesky factorization detects a
   non-positive pivot during elimination, fall back to the dense LU
   path.

```rustlab
% Centred point source.
rho = zeros(ny, nx);
rho(ny/2, nx/2) = 1.0;
b   = -1 * rho(:)';

% Default dispatch: -L is SPD, so the auto path picks Cholesky.
v_auto = spsolve(A, b);
print(length(v_auto))    % → 2500
```

## Forcing a path

When you know in advance which factorization you want — e.g. a hot loop
that avoids the SPD-detection cost on every call, or a stress test of
the fallback — pass the third argument explicitly.

```rustlab
% Force the sparse Cholesky path.
v_chol = spsolve(A, b, "cholesky");

% Force the dense LU fallback.
v_lu = spsolve(A, b, "lu");

% All three should agree to numerical precision.
print(norm(v_auto - v_chol))    % → ~0
print(norm(v_auto - v_lu))      % → small but nonzero (different fp paths)
```

The Cholesky and LU results differ at the 1e-7..1e-9 level not because
either is wrong, but because they accumulate rounding errors in
different orders.

## Real-vs-complex auto-routing

The Cholesky path goes one step further — it inspects the entries of
`A` and `b` and routes "essentially real" inputs (every imaginary part
below 10⁻¹²) into a real-only `f64` solver. Complex factorization is
roughly 4× the work of real, so this auto-routing matters for
throughput in lessons that build real-valued matrices but live in a
complex-typed language.

You don't need to do anything to opt in — the routing is internal.

## Visualising the solution

```rustlab
clf
V = reshape(v_auto, ny, nx);
imagesc(V);
title("Poisson solution: -∇² V = δ at grid centre")
```

The result is the canonical "potential of a point charge inside a
grounded box" pattern: the potential peaks at the source cell and
decays to zero at the boundary.

## What if the matrix isn't SPD?

A symmetric matrix can still be indefinite (negative eigenvalues). The
classic example is `[[1, 2], [2, 1]]`, eigenvalues 3 and -1.

Under `"auto"`, the SPD pre-filter catches this — the diagonals are
positive, but the off-diagonal magnitudes exceed the diagonal, which the
Cholesky path will fail at the first negative pivot. Auto silently falls
through to dense LU and the solve still succeeds:

```rustlab
S = sparse([1, 2; 2, 1]);
x = spsolve(S, [1; 1]);
print(x)        % → [0.333, 0.333]   (LU path)
```

Forcing `"cholesky"` on this matrix would error cleanly with an
"is not Hermitian positive definite" message — useful when you're
asserting the structure of an assembly during development.

## When the matrix really is singular

Singular systems are caught at factorization time and surface as a
clear error rather than silently returning garbage:

```rustlab
% Z = sparse(zeros(3, 3))
% spsolve(Z, [1; 2; 3])    % errors: matrix is singular
```

The dense LU fallback uses partial pivoting and reports the same kind
of error from a near-zero pivot.

## Background — why this is a hand-rolled solver

Per project policy (`AGENTS.md` Rule 9) core numerical algorithms in
rustlab are written in pure Rust without large library dependencies.
The factorization here follows Davis, *Direct Methods for Sparse Linear
Systems* (chapter 4), which is the standard reference for sparse
Cholesky with elimination-tree-based symbolic factorization.

Subsequent phases of the sparse-solver work add:
- A fill-reducing AMD ordering (chapter 7) for further speedup on
  larger Laplacian assemblies.
- Sparse LU with partial pivoting (chapter 6) for the indefinite path,
  which currently still uses the dense fallback.

See `dev/plans/sparse_solve_handroll.md` for the full implementation
plan and the queue of upcoming phases.

## Cheat sheet

| Form                                | Path                                          |
|-------------------------------------|------------------------------------------------|
| `spsolve(A, b)`                     | auto: Cholesky if SPD, else dense LU          |
| `spsolve(A, b, "auto")`             | same as default                                |
| `spsolve(A, b, "cholesky")`         | force sparse Cholesky; error if not SPD       |
| `spsolve(A, b, "lu")`               | force dense LU                                |

The sparse Cholesky path picks up real-vs-complex routing automatically
and is the recommended default for any symmetric Laplacian-style
assembly you encounter.

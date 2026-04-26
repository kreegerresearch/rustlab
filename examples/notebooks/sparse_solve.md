# Sparse Direct Solves — `spsolve`

The `spsolve` builtin solves `A·x = b` for a sparse coefficient matrix
`A`. The implementation is a hand-rolled pair of sparse direct methods:
left-looking sparse Cholesky for Hermitian-positive-definite assemblies,
Gilbert-Peierls sparse LU with partial pivoting for everything else,
both with a fill-reducing AMD column ordering. Auto-detection picks the
right path so the script-level call stays simple.

The previous implementation densified `A` and ran Gaussian elimination
in cubic time. A 100×100 Lesson-05 grid produces a $10^4 \times 10^4$
matrix; densifying it allocates roughly 800 MB and the solve is
noticeably slow even at that size. Sparse factorization stays sparse
end-to-end and unblocks problems an order of magnitude larger.

## A Hermitian-positive-definite assembly

The 5-point Laplacian stencil with homogeneous Dirichlet boundaries is
the canonical SPD assembly. `laplacian_2d` returns $+\nabla^2$, so for
an SPD Poisson solve we want $-L$:

```rustlab
nx = 50;  ny = 50;
dx = 0.02; dy = 0.02;
L  = laplacian_2d(nx, ny, dx, dy);
A  = -1 * L;             % SPD: -∇² with homogeneous Dirichlet
n  = nx * ny;
print(issparse(A))       % → 1
print(nnz(A))            % → ~12k (5-point stencil, modulo boundary trims)
```

The matrix has $n = 2500$ unknowns and roughly 12 000 non-zeros — a
0.2 % density. Densified, it's a 2500 × 2500 complex matrix taking
about 80 MB. Sparse, it fits in well under 1 MB.

## Default dispatch picks the right path

`spsolve(A, b)` accepts an optional third argument `mode` that takes
`"auto"` (the default), `"cholesky"`, or `"lu"`. With `"auto"`:

1. The solver tests whether `A` is Hermitian (mirrored entries match
   within tolerance) and whether all diagonals are real-positive — a
   cheap pre-filter that's necessary for SPD.
2. If both checks pass, factor with sparse Cholesky.
3. Otherwise, factor with sparse LU.

```rustlab
% Centred point source.
rho = zeros(ny, nx);
rho(ny/2, nx/2) = 1.0;
b   = -1 * rho(:)';

% Default dispatch: -L is SPD, so auto picks Cholesky.
v_auto = spsolve(A, b);
print(length(v_auto))    % → 2500
```

## Forcing a path

When you know in advance which factorization you want — to skip the
SPD-detection cost in a hot loop, or to assert the structure of an
assembly — pass the third argument explicitly.

```rustlab
% Force the sparse Cholesky path.
v_chol = spsolve(A, b, "cholesky");

% Force the sparse LU path. LU always works on square non-singular
% matrices, even when the SPD pre-check would have routed through Cholesky.
v_lu = spsolve(A, b, "lu");

print(norm(v_auto - v_chol))    % → ~0  (auto and forced agree exactly)
print(norm(v_auto - v_lu))      % → ~1e-12 (different fp accumulation)
```

## Sparse LU on indefinite matrices

A symmetric matrix can still be indefinite. The classic small example
is `[[1, 2], [2, 1]]`, eigenvalues 3 and -1.

`auto`'s SPD pre-filter detects the indefinite structure and routes
the solve through sparse LU, which doesn't care about positive-
definiteness. The factorization succeeds and the result is identical
to `"lu"` mode:

```rustlab
S = sparse([1, 2; 2, 1]);
x = spsolve(S, [1; 1]);
print(x)        % → [0.333, 0.333]   (LU path)
```

A larger indefinite assembly — say a shifted Helmholtz-like operator
$-\nabla^2 + \alpha I$ with $\alpha$ large enough that the eigenvalues
straddle zero — solves the same way:

```rustlab
nx_i = 30; ny_i = 30;
L_i = laplacian_2d(nx_i, ny_i);
B = -1 * L_i + 0.5 * speye(nx_i * ny_i);   % indefinite

v_i = spsolve(B, ones(nx_i * ny_i, 1));    % auto → LU
```

If you forced `"cholesky"` on `B` instead, you'd get a clear `NotSpd`
error — useful as an assertion during development.

## Real-vs-complex auto-routing

Both factorization paths inspect the entries of `A` and `b` and route
"essentially real" inputs (every imaginary part below $10^{-12}$) into
a real-only `f64` solver. Complex factorization is roughly 4× the work
of real, so this auto-routing matters for throughput in lessons that
build real-valued matrices but live in a complex-typed language.

You don't need to do anything to opt in — the routing is internal.

## Visualising the solutions

```rustlab
clf
V = reshape(v_auto, ny, nx);
imagesc(V);
title("Poisson solution: -∇² V = δ at grid centre")
```

The result is the canonical "potential of a point charge inside a
grounded box" pattern: the potential peaks at the source cell and
decays to zero at the boundary.

## Singular systems are caught at factorization time

```rustlab
% Z = sparse(zeros(3, 3))
% spsolve(Z, [1; 2; 3])    % errors: matrix is singular
```

Both paths use partial pivoting (or, for Cholesky, a positive-pivot
test) and report singularity at the offending column rather than
silently returning garbage.

## Background — why hand-rolled

Per project policy (`AGENTS.md` Rule 9) core numerical algorithms in
rustlab are written in pure Rust without large library dependencies.
The factorization here follows Davis, *Direct Methods for Sparse Linear
Systems*:

- Sparse Cholesky: up-looking left-looking algorithm (chapter 4).
- Sparse LU: Gilbert-Peierls with partial pivoting (chapter 6),
  threshold tolerance 0.1.
- AMD ordering: basic minimum-degree on the symmetric pattern of
  $A + A^T$ (chapter 7). The full Davis variant with external-degree
  refinement and supervariable detection is deferred — the basic
  minimum-degree implementation here is competitive with column-count
  ordering on regular grids and gives sensible permutations on
  irregular patterns.

See `dev/plans/sparse_solve_handroll.md` for the full implementation
plan and the queue of remaining enhancements.

## Cheat sheet

| Form                                | Path                                          |
|-------------------------------------|------------------------------------------------|
| `spsolve(A, b)`                     | auto: Cholesky if SPD, else sparse LU         |
| `spsolve(A, b, "auto")`             | same as default                                |
| `spsolve(A, b, "cholesky")`         | force sparse Cholesky; error if not SPD       |
| `spsolve(A, b, "lu")`               | force sparse LU                               |

Both factorization paths use AMD ordering by default. The script-level
defaults are sized for the curriculum's typical inputs; users who need
different defaults can build the factorizations directly via
`rustlab_core::sparse_solve` from Rust.

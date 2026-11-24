# Sparse Direct Solve — Design and Algorithm Reference

This document describes the end-to-end sparse linear solve pipeline:
how sparse `A \ b`, `spsolve(A, b)`, `chol(A)`, `decomposition(A)`,
`lu(A)`, and `solve(F, b)` flow from a script call to a numerical answer,
what choices are made along the way, and why. MATLAB's sparse `chol`
outputs and `decomposition` (0.3.88) are covered in
[Sparse `chol` outputs and `decomposition`](#sparse-chol-outputs-and-decomposition). The iterative `pcg` and its `ichol`
preconditioner (0.3.87) are covered in [Iterative solves](#iterative-solves--pcg-and-ichol).
Companion docs:

- `docs/functions.md` — user-facing API reference for each builtin.
- `docs/quickref.md` — one-line summaries.
- `perf/sparse_solve_phase1to4.md` — timing table across orderings.
- `dev/plans/closed/sparse_solve_handroll.md` — original implementation plan.
- `dev/plans/em_performance.md` — Phase 1 (factor reuse) and Phase 2 (ordering hint).
- `dev/plans/sparse_spd_solve.md` — row-singleton Cholesky (0.3.85), nested
  dissection for Cholesky and LU, `n×1` results (0.3.86), `pcg` and
  `ichol` (0.3.87), sparse `chol` outputs and `decomposition` (0.3.88),
  measurements, and decisions.
- `perf/sparse_pcg_phase3.rlab` — `pcg` / `ichol` against `\` on 3-D grids.

All sparse-solver code is **pure Rust**, hand-rolled per `AGENTS.md` Rule 9:
no FFI, no GPL/LGPL, no large libraries.

## Reference

Davis, Timothy A. *Direct Methods for Sparse Linear Systems*, SIAM, 2006.
Chapter numbers below refer to this book.

## Pipeline

```
                       script-layer call
                              │
              ┌───────────────┴───────────────┐
              │                               │
   A \ b,  spsolve(A, b, …),        chol(A) → R, p, P
   decomposition(A)                  lu(A) → handle
              │                               │
   sparse_auto::factor                        │
   (triangular? SPD? row singletons?          │
    else LU)                                  │
              └───────────┬───────────────────┘
                          │
        factor_sparse_*() / ReducedChol
                          │
              ┌───────────┼───────────┐
              │           │           │
         realness?   ordering?   factorization?
              │           │           │
              ▼           ▼           ▼
       SparseCsc<f64>   SpdAuto    SparseChol
       SparseCsc<C64>   Identity   ReducedChol
                        Amd, Nd    SparseLU
                          │
                  triangular solve
                          │
                          ▼
                 x  (n×1 for \ and spsolve)
```

Two layers:

1. **Script layer** (`crates/rustlab-script/src/eval/builtins.rs` and
   `sparse_auto.rs`). Parses `spsolve` / `chol` / `lu` / `solve` arguments
   (and sparse `\` from `linsys::mldivide`), decides the factorization
   (`auto`/`cholesky`/`lu`), the ordering (`auto`/`identity`/`amd`/`nd`),
   and the scalar type (`f64` if `A` is essentially real, else `C64`).
   Calls into the core layer.

2. **Core layer** (`crates/rustlab-core/src/sparse_solve/`). Implements
   `SparseCsc<T>`, the elimination tree, five orderings (`Identity`,
   `ColCount`, `Amd`, `NestedDissection`, `SpdAuto`), the row-singleton
   split (`RowSingletonSplit`, `ReducedChol`), `SparseChol<T>`, and
   `SparseLU<T>`. All algorithms are generic over `T: SparseScalar`,
   so the same code factors real and complex matrices.

## Stage 1 — argument parsing and dispatch

`builtin_spsolve` (and friends) accept up to four arguments:

```text
spsolve(A, b)
spsolve(A, b, mode)
spsolve(A, b, mode, ordering)
[R, p, P] = chol(A [, "lower"] [, "vector"] [, ordering])
decomposition(A [, "auto" | "chol" | "lu" | "qr"] [, "upper" | "lower"])
lu(A)                lu(A, ordering)
solve(F, b)
```

`mode` is `"auto" | "cholesky" | "lu"`. `ordering` is
`"auto" | "identity" | "natural" | "amd" | "nd"`. Both default to `"auto"`.
Sparse `A \ b` is `spsolve(A, b)` in `"auto"` mode, factored once for all
columns of `b`.

### Mode resolution (`spsolve` and sparse `\`)

`crates/rustlab-script/src/eval/sparse_auto.rs::factor` is shared by
`spsolve` and square sparse `A \ b` (one factorization for all columns
of `b`):

0. (`"auto"` only, 0.3.88) A diagonal, upper triangular, or lower
   triangular matrix whose diagonal entries are all stored and finite →
   substitution (`TriangularSolve`), as MATLAB's `\` checks for a
   triangular matrix first. This is what `R \ b` and `R' \ b` take for a
   `chol` factor. A zero on the diagonal falls through to the steps below
   unchanged. `A' \ b` and `A.' \ b` with such an `A` in a variable are
   fused in the evaluator (`Evaluator::eval_arith_binop` →
   `builtins::sparse_transposed_mldivide`): a column-oriented substitution
   on `A'` that reads `A`'s rows, so `A'` is never formed.
1. `SparseMat::is_spd_estimate(1e-10)` (`crates/rustlab-core/src/types.rs`)
   — Hermitian symmetry and a real-positive diagonal → Cholesky of `A`.
2. Otherwise `RowSingletonSplit::find` (`sparse_solve/singleton.rs`)
   looks for rows whose only nonzero is the diagonal — what
   `pin_dirichlet` writes. Ordering those unknowns first makes `A`
   block lower triangular:

   ```text
   [ D  0 ] [x_p]   [b_p]        x_p = D⁻¹ b_p
   [ C  F ] [x_f] = [b_f]   ⇒    F x_f = b_f − C x_p
   ```

   If the free block `F` is Hermitian (`SparseCsc::is_hermitian(1e-10)`),
   `ReducedChol` factors it by Cholesky. The pinned values are exactly
   `b(k) / A(k, k)`. UMFPACK, behind MATLAB's sparse `\`, removes
   singletons the same way.
3. Otherwise LU of the whole `A`.

A Cholesky failure (non-positive pivot) under `"auto"` falls through to
LU. Under explicit `"cholesky"`, steps 1–2 are the only options and any
failure, including "not Hermitian", is an error. `"lu"` is step 3 alone.

A real factor takes a complex `b` as two real solves (real and imaginary
parts) in this path. `solve(F, b)` on a handle still refuses it.

### Ordering resolution

`OrderingChoice` (in `builtins.rs`) has four variants: `Auto`, `Identity`,
`Amd`, `Nd`. `Auto` resolves differently per factorization:

- **Cholesky** (including the free block of step 2): `SpdAutoOrdering`
  computes the symbolic `nnz(L)` of nested dissection, then counts the
  natural order's `nnz(L)` with a cap at that number, and keeps the
  natural order if it is no larger. A band (1-D Laplacian) keeps its
  zero-fill natural order; a 3-D grid gets nested dissection. The
  matrix's `ordering_hint` is not consulted.
- **LU** (0.3.86): the same `SpdAutoOrdering`, on the pattern of
  `A + A^T`. With threshold partial pivoting the symmetric count is an
  estimate of the LU fill, not the exact figure. On the Lesson 10 FDFD
  operator (161², complex, no hint) this replaced the O(n²) basic minimum
  degree.

Explicit `Identity` / `Amd` / `Nd` pass through unchanged. `"auto"` does
not read `ordering_hint` for either factorization; MATLAB's `\` picks its
own ordering too.

`spsolve` and sparse `\` return an `n×1` column (a 1×1 system gives a
scalar); `solve(F, b)` returns a 1-D vector.

### Realness resolution

`sparse_all_real(sm, None)` scans `sm.entries` for `|im(...)| < 1e-12`.
If it passes, the factor is `SparseCsc<f64>`; otherwise `SparseCsc<C64>`.
The right-hand side does not decide: in `\` and `spsolve`, a real factor
takes a complex `b` as two real solves.
The real path is roughly 4× cheaper than complex because a real multiply
is one operation while a complex multiply is four real multiplies and
two real adds.

## Stage 2 — COO → CSC conversion

Storage in `SparseMat` is COO-with-sorted-entries (`crates/rustlab-core/src/types.rs`).
The factorization needs CSC. `SparseMat::to_csc::<T>()`
(`crates/rustlab-core/src/sparse_solve/csc.rs`) does the conversion
with the `FromComplex` trait providing the `C64 → T` projection (it's
the identity for `T = C64` and "drop the imaginary part" for `T = f64`,
with a tolerance check that rejects entries violating realness). The
result is `SparseCsc<T>` — three flat arrays `(col_ptr, row_idx, vals)`
in column-major order.

## Stage 3 — fill-reducing ordering

A *symmetric permutation* `P` is chosen before factoring so that
`P A P^T` has fewer fill-ins than `A`. All orderings live in
`crates/rustlab-core/src/sparse_solve/ordering.rs`.

Cholesky factor size and factor + solve time for the 200×200 Dirichlet
Laplacian (`chol(-laplacian_2d(200, 200), ordering)`, 40 000 unknowns,
0.3.86 release build, 4-core VM):

| Ordering | Cholesky factor nnz | Factor + solve time |
|---|---:|---:|
| `"nd"` (and `"auto"`) | 1 257 119 | 0.10 s |
| `"identity"` | 8 000 199 | 0.61 s |
| `"amd"` | 15 664 975 | 3.94 s |

### `IdentityOrdering` — natural ordering

`P = I`. No reordering. Costs nothing to compute.

The right choice for a band: a 1-D Laplacian has no fill at all in
natural order, and `"auto"` keeps it. On a 2-D grid numbered
column-major the natural order is a band of width √N, so the factor
has `O(N^{1.5})` entries — about 6× the nested-dissection factor at
200×200 (table above) — and on a 3-D grid it is worse still. Before
0.3.85 the `laplacian_*` builders' `ordering_hint` made this the default;
`"auto"` no longer reads the hint.

(`perf/sparse_solve_phase1to4.md` has the older identity-vs-AMD table.)

### `NestedDissectionOrdering` — level-set nested dissection

George (1973); George & Liu, *Computer Solution of Large Sparse Positive
Definite Systems*, ch. 8. Works on the pattern of `A + A^T`. For each
connected subgraph larger than 64 nodes: breadth-first search from a
pseudo-peripheral node (restart from the farthest node while the
eccentricity grows, at most four sweeps), take the level holding the
median node as the separator, move separator nodes with no neighbour in
the next level into the near half, order both halves recursively, and
number the separator last. Subgraphs of 64 nodes or fewer keep their
natural order. Disconnected pieces are split before bisection.
Deterministic; O(nnz · log n).

Each cut is a line on a 2-D grid and a plane on a 3-D grid: the
200×200 Laplacian factor has 1.26 M entries against 8.0 M natural (table
above), and the Lesson 18 41³ sphere (27 216 free unknowns) 4.6 M against
20.8 M, factoring in 1.1 s against 7.2 s. For LU it also replaced basic
minimum degree on the Lesson 10 FDFD operator (6.9 s → 0.23 s). See
`dev/plans/sparse_spd_solve.md`.

### `SpdAutoOrdering` — nested dissection or natural, by fill

The `"auto"` ordering for both Cholesky and LU, described under
*Ordering resolution*. It counts fill on the pattern of `A + A^T`
(reusing `A` itself when its pattern is already symmetric). The natural
count stops at the nested-dissection count, so the comparison costs no
more than the cheaper symbolic pass.

### `AmdOrdering` — basic approximate minimum degree

Davis ch. 7. Operates on the symmetric pattern of `A + A^T`. At each
step picks the column with the lowest *current* node degree, eliminates
it, updates neighbour degrees, and continues. The hand-rolled
implementation here is the basic form (~270 LoC); Davis-style external
degree refinement and supervariable detection (~700+ LoC) are deferred.

No longer the default for any path (it was the no-hint default before
0.3.85 for Cholesky and 0.3.86 for LU). Its pivot search is O(n²), so it
is slow past ~10⁴ unknowns, and on grid Laplacians it gives the largest
factor of the three (table above). Available as `"amd"`.

### `ColCountOrdering` — column-count heuristic

Davis ch. 4. Static reordering by ascending column count. Cheaper to
compute than AMD but produces more fill on most patterns. Kept around
as a baseline; not the default for any path in the script layer.

## Stage 4 — factorization

Two algorithms, both in `crates/rustlab-core/src/sparse_solve/`.

### `SparseChol::factor` — up-looking Cholesky

Davis ch. 4.6 (`cs_chol`). For Hermitian-positive-definite `A`,
factor `P A P^T = L L^H` where `L` is lower triangular with real-positive
diagonal.

The algorithm builds `L` one row at a time. At iteration `k`:

1. **Pattern.** Compute the nonzero pattern of row `k` of `L` —
   the columns `j < k` for which `L(k, j) != 0`. Use the column
   elimination tree to find the reachable set in topological order
   without scanning all of `A`. (`elimination_tree.rs` and `ereach`
   in `cholesky.rs`.)

2. **Triangular solve.** For each `j` in topological order, compute
   `L(k, j) = (A(k, j) - sum L(k, m) * conj(L(j, m))) / L(j, j)`,
   then propagate `L(k, j) * conj(L(r, j))` into the row-`k`
   accumulator for every below-diagonal entry `r > j` of column `j`.

3. **Diagonal pivot.** `L(k, k) = sqrt(A(k, k) - sum |L(k, m)|^2)`.
   If the radicand is non-positive or non-real, `A` is not SPD —
   return `SparseSolveError::NotSpd { col: k }`.

For `T = f64` the conjugate operation is a no-op and the sqrt is real;
for `T = C64` the diagonal must be real after the subtraction or the
matrix isn't actually Hermitian. The same code path covers both.

The implementation uses a two-pass design (Phase 6 of
`dev/plans/em_performance.md`):

1. **Symbolic pass.** For each `k`, run `ereach` to enumerate the row-`k`
   pattern of `L`. Count how many entries land in each column to get
   exact `col_count[j]`. Reuses the elimination tree and the same
   `mark`-vector machinery as the numeric pass; runs in `O(nnz(L))`.
2. **Numeric pass.** Same algorithm as classical up-looking Cholesky
   but writes directly into preallocated flat `Lp / Li / Lx` arrays
   via per-column write cursors. The diagonal goes in slot `Lp[j]`;
   below-diagonal entries fill `Lp[j]+1 ..` in increasing-row order.

This replaces an earlier `Vec<Vec<(usize, T)>>` accumulator pattern
that needed a final flatten step. Performance impact is modest
(5–11% factor speedup on grid Laplacians at 150–200) — the
architectural win is bigger than the wall-clock win because the
factor now lives in flat CSC end-to-end. See
`perf/em_performance_phase6.md` for the A/B numbers.

The symbolic phase is also exposed as a public free function
`symbolic_col_counts(a, ordering)` for callers that want to predict
column nnz without running the numeric pass — useful for fill
estimation, ordering selection, or pre-allocating downstream buffers
when factoring repeatedly with the same sparsity pattern but
different numeric values.

### `SparseLU::factor` — Gilbert-Peierls with partial pivoting

Davis ch. 6. For general (possibly non-symmetric, indefinite, or
complex) `A`, factor `P A Q = L U` where `P` is a row permutation
chosen by partial pivoting (threshold 0.1) and `Q` is the column
permutation chosen by `OrderingMethod`.

The Gilbert-Peierls algorithm computes one column of `L` and `U` at a
time:

1. Solve `L_k x = A(:, q_k)` where `L_k` is the partial factor so far
   and `q_k` is the next column under the chosen ordering. The pattern
   of `x` is computed via depth-first search on the directed graph of
   `L_k`, and the solve happens in topological order.

2. **Partial pivot.** Choose the row in `x` with `|x_i|` ≥ 0.1 ×
   max-below-diagonal, prefer the diagonal-pivot row when it's within
   threshold (Trefethen-Bau partial pivoting). If no acceptable pivot
   exists below the singularity tolerance, return
   `SparseSolveError::Singular`.

3. Split `x` into the column of `U` (above and including the pivot row)
   and the column of `L` (below the pivot row, scaled by `1 / pivot`).

The pivoting threshold of 0.1 is the standard from Trefethen and Bau,
*Numerical Linear Algebra*, lecture 21. Strict partial pivoting (`= 1`)
guarantees `|L| ≤ 1` but produces more fill; threshold pivoting trades
a small loss in numerical stability for a much sparser `L`.

## Stage 5 — back-solve

Both factor types expose `solve(b)`:

- **Cholesky:** `L y = P b` (forward), `L^H z = y` (backward), `x = P^T z`.
- **LU:** `L y = P b` (forward), `U z = y` (backward), `x = Q z`.

Each triangular solve is `O(nnz(L))` or `O(nnz(U))`. For a 200×200
grid Cholesky factor with identity ordering (`nnz = 8 × 10^6`), the
solve itself is in the millisecond range — three orders of magnitude
cheaper than the factor.

This is why `decomposition(A)` / `lu(A)` returning a reusable factor is
the recommended pattern for parameter sweeps and animations: factor
once, solve many.

## Phase 1 — `SparseFactor` (factor reuse)

`crates/rustlab-script/src/eval/value.rs` defines:

```rust
pub enum SparseFactor {
    CholReal(Arc<SparseChol<f64>>),
    CholComplex(Arc<SparseChol<C64>>),
    LuReal(Arc<SparseLU<f64>>),
    LuComplex(Arc<SparseLU<C64>>),
}
```

`Arc` so cloning a `Value::SparseFactor` is cheap. The four variants
mirror the realness × factorization cross-product so the back-solve
doesn't re-detect realness on every solve.

Since 0.3.88 only `lu(S)` returns this handle; a sparse `chol(S)` returns
MATLAB's sparse factor `R` (next section), and `decomposition(S)` holds the
reusable factor. `solve(F, b)` given a sparse matrix (an `R` from `chol`)
errors with the three replacements: `S \ b`, `decomposition(S, "chol")`,
or `[R, p, P] = chol(S); x = P * (R \ (R' \ (P' * b)))`.

`solve(F, b)` with a real factor refuses a complex `b` with a clear error
rather than silently dropping the imaginary part (`\` and `spsolve` split
it into two real solves instead). `solve(F, b)` returns a 1-D vector.

Implementation lives in `factor_sparse_cholesky` / `factor_sparse_lu` /
`solve_with_factor` in `builtins.rs`; `sparse_auto::factor` (used by `\`,
`spsolve`, and `decomposition`) and the `lu`/`solve` builtins delegate to
those helpers.

## Sparse `chol` outputs and `decomposition`

Phase 4 of `dev/plans/sparse_spd_solve.md` (0.3.88), decision 3: sparse
`chol(S)` returns MATLAB's factor.

**`chol` (`eval/sparse_chol.rs`).** The options are parsed in any order:
`"upper"` / `"lower"` (the triangle that is read), `"matrix"` / `"vector"`
(the form of `P`), and, with three outputs only, an ordering name. The
chosen triangle is mirrored into a Hermitian `SparseCsc`
(`hermitian_from_triangle`), so a non-symmetric input behaves as MATLAB's
(only that triangle counts) even after a symmetric permutation. Then
`SparseChol::factor_flagged` runs the same up-looking factorization as
`SparseChol::factor` (shared `up_looking` numeric pass, bit-identical
factor when nothing fails) with one difference: when pivot `q` is not
positive it records `q` and keeps going, reducing every later row against
columns `0 .. q` only. The result (`PartialChol`) is the `n × q` block
`L(:, 1:q)`, which is what MATLAB's sparse `[R, p] = chol(S)` returns as
`R = L(:, 1:q)'` (`q × n`), with `R' * R` equal to `S` on the first `q`
rows and columns. A dense `[R, p]` stays `q × q`, also as MATLAB.

| Outputs | Ordering | Result |
|---|---|---|
| `R = chol(S)` | natural | `R' * R = S`; error if not positive definite |
| `[R, p] = chol(S)` | natural | `p = 0` or the 1-based failed pivot |
| `[R, p, P] = chol(S)` | `SpdAutoOrdering` (or the named one) | `R' * R = P' * S * P`, `P(perm(k), k) = 1` |
| `[R, p, q] = chol(S, "vector")` | same | `R' * R = S(q, q)`, `q` a `1 × n` row |

`R` is built straight from the CSC columns of `L` (column `j` of `L` is
row `j` of `R`, already row-major), so no sort is needed; `"lower"` returns
`L` through one counting-sort transpose. The factor is a COO `SparseMat`
with `C64` entries (32 bytes per entry against 16 for the internal real
CSC); see the measurements below.

**Using `R`.** Sparse `\` step 0 makes `R \ b` and `R' \ b` substitutions.
Two sparse matrices multiply to a sparse matrix (`SparseMat::matmul`,
Gustavson row by row), so `R' * R` and `P' * S * P` can be checked at the
sizes where `chol` is used; `P * x` and `P' * b` with a dense `x`, `b` are
O(n) sparse-times-dense products.

**`decomposition` (`eval/decomposition.rs`).** `Value::Decomposition`
holds an `Arc` of either a sparse `AutoFactor` (the factor sparse `\`
would use; `"lu"` forces LU; `"chol"` is the mirrored triangle factored
with `SpdAutoOrdering`, an error if not positive definite) or a dense
factor (`linsys::DenseFactor`: triangular, Cholesky, LU, or pivoted QR,
chosen like dense `\`). `dA \ b` dispatches from `linsys::mldivide`;
`b / dA` from `mrdivide` (`"chol"` only, since `A' = A`). `dA.Type` reports
`chol`, `lu`, `qr`, `triangular`, or `diagonal`; a `pin_dirichlet` operator
reports `chol` (the reduced Cholesky). A real sparse factor takes a
complex `b` as two solves, which the `lu(S)` handle does not.

**Measurements** (release build, 4-core VM, `perf/sparse_chol_phase4.rlab`;
3-D Poisson `-laplacian_3d(n, n, n)`, `b` of ones and twos; the VM's
run-to-run spread is about ±8%).

| Grid (N, nnz(R)) | `S \ b` | `[R, p, P] = chol(S)` | `P*(R\(R'\(P'*b)))` | `decomposition(S, "chol")` | `dS \ b` | `R` as `SparseMat` / real CSC |
|---|---|---|---|---|---|---|
| 21³ (9,261, 0.98 M) | 0.12 s | 0.15 s | 0.010 s | 0.10 s | 0.002 s | 31 / 16 MB |
| 31³ (29,791, 4.9 M) | 1.2 s | 1.3 s | 0.073 s | 1.1 s | 0.014 s | 157 / 79 MB |
| 41³ (68,921, 15.9 M) | 5.9 s | 7.4 s | 0.33 s | 6.1 s | 0.052 s | 509 / 255 MB |

Peak RSS at 41³: `[R, p, P] = chol(S)` plus one idiom solve 801 MB (the
same as `chol` alone: `L` and `R` coexist while `R` is built),
`decomposition` plus one solve 316 MB, `S \ b` 303 MB (0.3.87's
`F = chol(S); solve(F, b)`: 317 MB, factor 6.5–6.8 s against 5.8–6.4 s
for `decomposition`). The idiom is two in-place substitutions over the
COO entries, about 0.16 s each at 41³; `P' * b` and `P * z` take 1–4 ms.
`R' \ c` is fused: with `R` (or any triangular sparse matrix with a full
diagonal) in the workspace, `R' \ c` and `R.' \ c` run a column-oriented
substitution on `R'`, which reads `R`'s rows in order (row `i` of `R` is
column `i` of `R'`), so `R'` is never formed. Before the fusion the
transpose of the 15.9 M entries took ~0.7 s and 509 MB, and one idiom
solve was 0.92–1.09 s with a 1,044 MB peak. Each unknown receives its
updates in the same order as the row-oriented solve on an explicit `R'`,
so the two agree bit for bit. `dS \ b` runs on the stored real CSC
factor. The
natural-order `R = chol(S)` on 25³ has 9.4 M entries and takes 2.8 s,
against 2.0 M and 0.4 s with `P`. `R' * R - P' * S * P` (sparse
products, max entry 7.5e-14) takes 27 s at 41³ because `R' * R` has
roundoff-sized entries everywhere fill occurred; use it on small grids.

## Phase 2 — `OrderingHint` (historical: no longer read by `"auto"`)

`crates/rustlab-core/src/types.rs` defines:

```rust
pub enum OrderingHint {
    Identity,
}

pub struct SparseMat {
    pub rows: usize,
    pub cols: usize,
    pub entries: Vec<(usize, usize, C64)>,
    pub ordering_hint: Option<OrderingHint>,
}
```

The hint is metadata that travels with the matrix, declaring "natural
ordering is best for me". Until 0.3.84 the ordering arg `"auto"`
consulted it. Since 0.3.85 (Cholesky) and 0.3.86 (LU), `"auto"` compares
nested dissection with the natural order by predicted fill instead, which
keeps the natural order where it is best (bands) and drops it on 2-D and
3-D grids, where it is not. The builders still set the hint, and the
operations below still carry it; nothing in the solver reads it now.

**Builders that set the hint:** `laplacian_1d`, `laplacian_2d`,
`laplacian_3d`, `laplacian_eps_2d`, `laplacian_eps_3d`. Anywhere we know the result has
a regular grid pattern.

**Operations that preserve the hint:** `scale`, `transpose`, negation
(via `Value::negate`), `set` (single-entry update). The nonzero pattern
is unchanged or trivially-changed in each case.

**Operations that drop the hint:** `add`, `sub`, `from_dense`. The
union of two patterns may not be grid-banded; the safe default is to
drop the claim.

User-built matrices via `sparse(I, J, V, m, n)` carry `None`. Any
ordering can still be forced with `spsolve(A, b, mode, "identity")`,
`chol(A, "nd")`, `lu(A, "amd")`, and so on.

## Realness fast path — design rationale

The fast path is *checked at the entries level*, not declared at the
type level. This keeps the public API stable (`SparseMat` is always
`C64`-typed in user-facing code) while avoiding the 4× tax of complex
arithmetic when it isn't needed.

For frequency-domain EM problems with truly complex coefficients
(lossy ε, PML stretches), the realness check fails on the first complex
entry and routes to the complex path. There's no penalty for "almost
real" inputs — the threshold is `1e-12`, far below any meaningful
physical signal.

Phase 5 of `dev/plans/em_performance.md` plans to push this realness
distinction *up* into the DSP layer (`vector_calc.rs`, `laplacian.rs`)
so we don't even build complex triplets for real inputs in the first
place.

## File map

```
crates/rustlab-core/src/
├── types.rs                        SparseMat (COO), OrderingHint
└── sparse_solve/
    ├── mod.rs                      public re-exports, SparseSolveError
    ├── csc.rs                      SparseCsc<T>, SparseScalar trait, FromComplex
    ├── ordering.rs                 OrderingMethod trait, Identity/ColCount/Amd,
    │                               NestedDissection, SpdAuto
    ├── elimination_tree.rs         column elimination tree + post-order
    ├── cholesky.rs                 SparseChol<T>::factor, ::factor_flagged
    │                               (PartialChol), ::solve
    ├── lu.rs                       SparseLU<T>::factor, ::solve
    ├── singleton.rs                RowSingletonSplit<T>, ReducedChol<T>
    ├── ichol.rs                    ichol (IC(0), ICT, michol, diagcomp)
    ├── pcg.rs                      pcg, PcgOperators, MatrixPcg, PcgOutcome
    ├── triangular.rs               TriangularSolve (diagonal / lower / upper)
    └── tests.rs                    integration tests across modules

crates/rustlab-script/src/eval/
├── value.rs                        SparseFactor enum, Value::SparseFactor,
│                                   Value::Decomposition
├── sparse_auto.rs                  triangular / cholesky / lu choice for spsolve,
│                                   sparse \, and decomposition
├── sparse_chol.rs                  MATLAB sparse chol outputs (R, p, P)
├── decomposition.rs                decomposition(A), dA \ b, dA.Type
├── iterative.rs                    pcg (on the evaluator: handles) and ichol builtins
└── builtins.rs                     spsolve / chol / lu / solve entry points
                                    factor_sparse_cholesky / _lu
                                    OrderingChoice, parse_ordering_arg
                                    sparse_all_real
                                    laplacian_* builtins (set ordering_hint, unused by "auto")
```

## Iterative solves — `pcg` and `ichol`

`pcg` is MATLAB's preconditioned conjugate gradients
(`rustlab_core::sparse_solve::pcg`), for a Hermitian positive definite
`A`. Unlike `\` it stops at a relative residual `tol`, so it is the right
tool when `tol`-accuracy is enough and the direct factor is large.

**Dispatch.** `pcg` is not a registry builtin: `A`, `M1`, and `M2` may be
function handles, so the evaluator intercepts the name in
`call_builtin_tracked_nargout` (which `@pcg`, `feval`, and multi-assign
all reach) and runs `Evaluator::eval_pcg` (`eval/iterative.rs`). That
parses MATLAB's argument list (`[]` placeholders, `tol` clamping with
warnings, `maxit = min(n, 20)`), builds the operators, runs the core
loop through the `PcgOperators` trait, and prints MATLAB's message when
fewer than two outputs are requested.

**Operators.**

| Argument | Built as | Per iteration |
|---|---|---|
| sparse or dense `A` | `SparseCsc<T>` (dense → sparse once) | one SpMV, `O(nnz)` |
| handle `afun` | the callable | one interpreter call |
| triangular or diagonal `M` (`ichol`'s `L`, `L'`, `spdiags`) | `TriangularSolve` | substitution, `O(nnz(M))` |
| any other matrix `M` | `sparse_auto::factor` once (Cholesky / reduced Cholesky / LU) | one factor solve |
| singular `M` | — | `NaN`, so flag 2 |
| handle `mfun` | the callable | one interpreter call |

Real `A`, `b`, `x0`, and matrix preconditioners run with `T = f64`. A
complex value or any handle runs with `T = C64`; inner products conjugate
the left vector, so complex Hermitian `A` works. A result whose imaginary
parts are all zero is returned with real storage.

**Iteration.** The loop follows MATLAB's `pcg.m` step for step: `z = M2 \ (M1 \ r)`,
flag 2 on a non-finite `z`, flag 4 when `ρ`, `β`, `α` are zero or
infinite or `p'·A·p ≤ 0`, a stagnation counter on
`‖p‖·|α| < eps·‖x‖`, the true residual recomputed whenever the recursive
one passes `tol` (or during stagnation "more steps", at most
`min(n/50, 5, n − maxit)`), and on failure the iterate with the smallest
residual is returned. `resvec` holds the recursive norms, with the true
norm substituted on the iterations where it was recomputed.

**`ichol`** (`rustlab_core::sparse_solve::ichol`) is left-looking in the
natural order. Column `j` scatters `A(j:n, j)` into a dense work vector,
then subtracts `L(j:n, k)·conj(L(j, k))` for each earlier column `k` with
`L(j, k) ≠ 0`. Those columns are found through linked lists keyed by
each column's next unprocessed row, so the cost is the flops of the
factor. IC(0) discards any update outside the pattern of `tril(A)`; ICT
keeps everything and drops `|L(i, j)| < droptol·‖A(j:n, j)‖₁` after the
column is complete. `michol` adds every dropped value to the diagonals
of its row and column (`L·L'·e = A·e`). `diagcomp` scales the diagonal
by `1 + α` as it is scattered. The factor is returned as a COO
`SparseMat`; `pcg` turns `L` and `L'` back into CSC triangular solves.

**Measurements** (release build, 4-core VM, `perf/sparse_pcg_phase3.rlab`;
`b = rand`, `tol = 1e-8`; setup is the `ichol` time, solve includes the
CSC conversion of `A`).

| Grid (N, nnz) | Method | Setup | Solve | Iterations | True relres |
|---|---|---|---|---|---|
| 21³ (9,261, 62,181) | sparse `A \ b` | — | 0.125 s | — | 4.3e-14 |
| | `pcg`, none | — | 0.007 s | 79 | 8.8e-9 |
| | `pcg`, `ichol` | 0.006 s | 0.008 s | 29 | 5.2e-9 |
| 31³ (29,791, 202,771) | sparse `A \ b` | — | 1.14 s | — | 1.1e-13 |
| | `pcg`, none | — | 0.034 s | 113 | 8.6e-9 |
| | `pcg`, `ichol` | 0.022 s | 0.036 s | 40 | 9.0e-9 |
| 41³ (68,921, 472,361) | sparse `A \ b` | — | 5.43 s | — | 2.3e-13 |
| | `pcg`, none | — | 0.107 s | 147 | 9.3e-9 |
| | `pcg`, Jacobi (`spdiags`) | 0.008 s | 0.117 s | 147 | 9.3e-9 |
| | `pcg`, `ichol` nofill | 0.051 s | 0.109 s | 48 | 8.4e-9 |
| | `pcg`, `ichol` michol | 0.056 s | 0.083 s | 39 | 8.1e-9 |
| | `pcg`, `ichol` ict 1e-2 (nnz(L) 270,800) | 0.048 s | 0.100 s | 48 | 8.4e-9 |
| | `pcg`, `ichol` ict 1e-3 (nnz(L) 911,849) | 0.193 s | 0.138 s | 27 | 6.7e-9 |

Jacobi changes nothing on a Laplacian (its diagonal is constant). One
`ichol` iteration costs about three unpreconditioned ones (SpMV plus two
triangular solves), so a 3× cut in iterations is roughly break-even in
time; `michol` and ICT with a small `droptol` cut further.

## Algorithmic complexity summary

| Stage | Cost | Notes |
|---|---|---|
| `is_spd_estimate` | `O(nnz)` | scan diagonal + Hermitian pairs |
| `to_csc` | `O(nnz log nnz)` | sort the COO triples |
| `IdentityOrdering` | `O(1)` | no work |
| `NestedDissectionOrdering` | `O(nnz · log n)` | BFS per level of the dissection |
| `SpdAutoOrdering` | ND + `O(min(nnz(L_nd), nnz(L_nat)))` | symbolic counts, natural count capped |
| `RowSingletonSplit` | `O(nnz)` | one pass to find pinned rows, one to split |
| `AmdOrdering` (basic) | `O(n^2)` worst case | `O(n nnz)` typical on grids |
| `column_elimination_tree` | `O(nnz log n)` | Davis ch. 4.3 |
| Cholesky factor | `O(Σ col_count²)` flops, `O(nnz(L))` memory | 2-D grid, ND: `nnz(L) = O(N log N)`; natural: `O(N^{1.5})` |
| LU factor | `O(nnz(L) + nnz(U))` flops + pivoting search | typically 2× Cholesky on the same SPD matrix |
| Triangular solve | `O(nnz(L))` per RHS | dominant cost in factor-many-solve scenarios |
| `ichol` nofill | `O(Σ_j Σ_{k: L(j,k)≠0} nnz(L(j:n, k)))` | `nnz(L) = nnz(tril(A))` |
| `pcg` iteration | one `A` product + the preconditioner solves + 5 vector ops | `O(√κ · log(1/tol))` iterations for CG |

Where `n` is the matrix dimension, `nnz` is the input nonzero count,
and `nnz(L)` / `nnz(U)` depend on ordering (see perf table).

## Numerical tolerances

Locked at the values below; users who need different ones build the
factorizations directly via `rustlab_core::sparse_solve` from Rust.

| Tolerance | Value | Where | Rationale |
|---|---:|---|---|
| Real-vs-complex threshold | `1e-12` | `sparse_all_real` | well below f64 noise floor |
| SPD diagonal check | `1e-10` | `is_spd_estimate` | gives clear non-SPD signal without false negatives |
| Free-block Hermitian check | `1e-10` absolute | `ReducedChol::factor` (`HERMITIAN_TOL`) | same as the whole-matrix check |
| Cholesky pivot floor | `eps · n · max\|A\|` | `SparseChol::factor` | relative, so `1e-16 * I` is still SPD |
| LU pivot threshold | `0.1` | `SparseLU::factor` | Trefethen-Bau standard partial pivoting |
| LU singularity threshold | `eps · n · max\|A\|` | `SparseLU::factor` | relative, so a well-conditioned tiny-scaled `A` is not singular |

## What's deliberately not here

- **Iterative solves behind `\`.** `\` and `spsolve` stay direct.
  `pcg` / `ichol` are separate builtins (see below); GMRES and BiCGSTAB
  for non-symmetric systems are not implemented.
- **Block factorization** (supernodal) and threads. The hand-rolled
  algorithms here are scalar; a supernodal, parallel Cholesky is Phase 5
  of `dev/plans/sparse_spd_solve.md` (deterministic for a fixed thread
  count).
- **Pre-factorization scaling** (row/column equilibration). The matrices
  the curriculum builds are well-scaled by construction.
- **Mixed-precision iterative refinement**. Single-precision factor +
  double-precision residual updates would help on huge problems, but
  the realness fast path covers most of the speedup we'd want from
  that.

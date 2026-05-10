# Sparse Direct Solve — Design and Algorithm Reference

This document describes the end-to-end sparse linear solve pipeline:
how `spsolve(A, b)`, `chol(A)`, `lu(A)`, and `solve(F, b)` flow from a
script call to a numerical answer, what choices are made along the way,
and why. Companion docs:

- `docs/functions.md` — user-facing API reference for each builtin.
- `docs/quickref.md` — one-line summaries.
- `perf/sparse_solve_phase1to4.md` — timing table across orderings.
- `dev/plans/closed/sparse_solve_handroll.md` — original implementation plan.
- `dev/plans/em_performance.md` — Phase 1 (factor reuse) and Phase 2 (ordering hint).

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
       spsolve(A, b, …)                chol(A) / lu(A)
              │                               │
              └───────────┬───────────────────┘
                          │
                  factor_sparse_*()
                          │
              ┌───────────┼───────────┐
              │           │           │
         realness?   ordering?   factorization?
              │           │           │
              ▼           ▼           ▼
       SparseCsc<f64>   Identity   SparseChol
       SparseCsc<C64>   AMD        SparseLU
                          │
                  triangular solve
                          │
                          ▼
                          x
```

Two layers:

1. **Script layer** (`crates/rustlab-script/src/eval/builtins.rs`).
   Parses `spsolve` / `chol` / `lu` / `solve` arguments, decides the
   factorization (`auto`/`cholesky`/`lu`), the ordering (`auto`/`identity`/`amd`),
   and the scalar type (`f64` if `A` and `b` are essentially real, else `C64`).
   Calls into the core layer.

2. **Core layer** (`crates/rustlab-core/src/sparse_solve/`). Implements
   `SparseCsc<T>`, the elimination tree, three orderings, `SparseChol<T>`,
   and `SparseLU<T>`. All algorithms are generic over `T: SparseScalar`,
   so the same code factors real and complex matrices.

## Stage 1 — argument parsing and dispatch

`builtin_spsolve` (and friends) accept up to four arguments:

```text
spsolve(A, b)
spsolve(A, b, mode)
spsolve(A, b, mode, ordering)
chol(A)              chol(A, ordering)
lu(A)                lu(A, ordering)
solve(F, b)
```

`mode` is `"auto" | "cholesky" | "lu"`. `ordering` is
`"auto" | "identity" | "natural" | "amd"`. Both default to `"auto"`.

### Mode resolution (`spsolve` only)

`"auto"` calls `SparseMat::is_spd_estimate(1e-10)` (`crates/rustlab-core/src/types.rs`).
That helper checks Hermitian symmetry and that every diagonal entry is
real-positive. Cheap pre-check; correctness still depends on numerical
factorization succeeding. If the SPD check passes, route to Cholesky;
otherwise route to LU.

Cholesky failure under `"auto"` falls through silently to LU. Failure
under explicit `"cholesky"` raises an error to the script layer.

### Ordering resolution

`OrderingChoice` (in `builtins.rs`) has three variants: `Auto`, `Identity`,
`Amd`. `resolve_ordering(sm, choice)` collapses `Auto` against the
matrix's `ordering_hint`:

- `sm.ordering_hint == Some(Identity)` → `Identity`.
- `sm.ordering_hint == None` → `Amd`.
- Any explicit `Identity`/`Amd` from the user passes through unchanged.

The hint is set by builders that know their output is structurally
regular — the `laplacian_*` family in particular. User-built sparse
matrices (from `sparse(I, J, V, m, n)`) carry `None` and route through
AMD by default.

### Realness resolution

`sparse_all_real(sm, b)` scans `sm.entries` and `b` for `|im(...)| < 1e-12`.
If both pass, route through `SparseCsc<f64>`; otherwise `SparseCsc<C64>`.
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
`P A P^T` has fewer fill-ins than `A`. All three orderings live in
`crates/rustlab-core/src/sparse_solve/ordering.rs`.

### `IdentityOrdering` — natural ordering

`P = I`. No reordering. Costs nothing to compute.

The right choice when `A`'s nonzero pattern is already in fill-friendly
order. The 5-point 2-D Laplacian numbered column-major is the canonical
example: identity ordering gives an `O(N^{1.5})`-fill factor, which is
optimal for this stencil up to constants (Hoffman-Martin-Rose, 1973).
AMD on the same matrix searches a permutation space and lands on
something *worse* than identity because the input pattern is already
near-optimal — the search is pure overhead, plus AMD's heuristic isn't
guaranteed to recover the natural order on banded matrices.

Concrete factor-nnz numbers for the 200×200 Dirichlet Laplacian
(40 000 unknowns):

| Ordering | Cholesky factor nnz | Factor + solve time |
|---|---:|---:|
| Identity | 8 000 199 | 0.42 s |
| AMD | 15 664 975 | 2.34 s |

(See `perf/sparse_solve_phase1to4.md` for the full table.)

### `AmdOrdering` — basic approximate minimum degree

Davis ch. 7. Operates on the symmetric pattern of `A + A^T`. At each
step picks the column with the lowest *current* node degree, eliminates
it, updates neighbour degrees, and continues. The hand-rolled
implementation here is the basic form (~270 LoC); Davis-style external
degree refinement and supervariable detection (~700+ LoC) are deferred.

Safe default for matrices with unknown structure. On grid Laplacians
it's strictly worse than Identity (see table above); on irregular FEM
patterns it beats Identity by a wide margin.

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

This is why `chol(A)` / `lu(A)` returning a reusable factor handle is
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

`builtin_chol` errors on non-SPD `A` with no auto fallback to LU —
the user explicitly chose Cholesky. Call `lu(A)` or `spsolve(A, b)`
for auto dispatch.

A real factor refuses a complex `b` at solve time with a clear error
message rather than silently dropping the imaginary part. To solve
complex `b` against a real `A`, refactor with the complex matrix.

Implementation lives in `factor_sparse_cholesky` / `factor_sparse_lu` /
`solve_with_factor` in `builtins.rs`; `try_sparse_cholesky` /
`try_sparse_lu` (used by `spsolve`) and the `chol`/`lu`/`solve`
builtins all delegate to those helpers.

## Phase 2 — `OrderingHint` (auto-pick identity for grids)

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
ordering is best for me". The script-layer ordering arg `"auto"`
consults the hint; explicit `"identity"` / `"amd"` overrides it.

**Builders that set the hint:** `laplacian_1d`, `laplacian_2d`,
`laplacian_3d`, `laplacian_eps_2d`. Anywhere we know the result has
a regular grid pattern.

**Operations that preserve the hint:** `scale`, `transpose`, negation
(via `Value::negate`), `set` (single-entry update). The nonzero pattern
is unchanged or trivially-changed in each case.

**Operations that drop the hint:** `add`, `sub`, `from_dense`. The
union of two patterns may not be grid-banded; the safe default is to
drop the claim.

User-built matrices via `sparse(I, J, V, m, n)` carry `None` and route
to AMD. Users who know their matrix is grid-shaped can opt in
explicitly via `spsolve(A, b, mode, "identity")` or `chol(A, "identity")`.

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
    ├── ordering.rs                 OrderingMethod trait, Identity/ColCount/Amd
    ├── elimination_tree.rs         column elimination tree + post-order
    ├── cholesky.rs                 SparseChol<T>::factor, ::solve
    ├── lu.rs                       SparseLU<T>::factor, ::solve
    └── tests.rs                    integration tests across modules

crates/rustlab-script/src/eval/
├── value.rs                        SparseFactor enum, Value::SparseFactor
└── builtins.rs                     spsolve / chol / lu / solve dispatch
                                    factor_sparse_cholesky / _lu
                                    try_sparse_cholesky / _lu  (spsolve callees)
                                    OrderingChoice, parse_ordering_arg, resolve_ordering
                                    sparse_all_real
                                    laplacian_* builtins (set ordering_hint)
```

## Algorithmic complexity summary

| Stage | Cost | Notes |
|---|---|---|
| `is_spd_estimate` | `O(nnz)` | scan diagonal + Hermitian pairs |
| `to_csc` | `O(nnz log nnz)` | sort the COO triples |
| `IdentityOrdering` | `O(1)` | no work |
| `AmdOrdering` (basic) | `O(n^2)` worst case | `O(n nnz)` typical on grids |
| `column_elimination_tree` | `O(nnz log n)` | Davis ch. 4.3 |
| Cholesky factor | `O(nnz(L))` flops, `O(nnz(L))` memory | grid: `O(N^{1.5})` with identity |
| LU factor | `O(nnz(L) + nnz(U))` flops + pivoting search | typically 2× Cholesky on the same SPD matrix |
| Triangular solve | `O(nnz(L))` per RHS | dominant cost in factor-many-solve scenarios |

Where `n` is the matrix dimension, `nnz` is the input nonzero count,
and `nnz(L)` / `nnz(U)` depend on ordering (see perf table).

## Numerical tolerances

Locked at the values below; users who need different ones build the
factorizations directly via `rustlab_core::sparse_solve` from Rust.

| Tolerance | Value | Where | Rationale |
|---|---:|---|---|
| Real-vs-complex threshold | `1e-12` | `sparse_all_real` | well below f64 noise floor |
| SPD diagonal check | `1e-10` | `is_spd_estimate` | gives clear non-SPD signal without false negatives |
| Cholesky pivot floor | `1e-12` | `checked_sqrt_real_pos` | keeps `sqrt` from amplifying floating-point noise |
| LU pivot threshold | `0.1` | `SparseLU::factor` | Trefethen-Bau standard partial pivoting |
| LU singularity threshold | `1e-14` | `SparseLU::factor` | matches dense LU convention |

## What's deliberately not here

- **Iterative solvers** (CG, GMRES, BiCGSTAB). The curriculum is built
  around direct solvers; iterative methods are a separate teaching
  arc and a separate code arc.
- **Block factorization** (supernodal). The hand-rolled algorithms
  here are scalar; a supernodal upgrade is plausible future work but
  not currently planned.
- **Pre-factorization scaling** (row/column equilibration). The matrices
  the curriculum builds are well-scaled by construction.
- **Mixed-precision iterative refinement**. Single-precision factor +
  double-precision residual updates would help on huge problems, but
  the realness fast path covers most of the speedup we'd want from
  that.

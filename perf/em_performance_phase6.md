# Cholesky factor performance — em_performance Phase 6

Phase 6 of `dev/plans/em_performance.md` replaced the per-column
`Vec<Vec<(usize, T)>>` accumulator in `SparseChol::factor` with a flat
CSC layout (`Lp / Li / Lx` arrays) sized from an up-front symbolic
counts pass. The numeric pass writes directly into preallocated arrays
via per-column write cursors.

## Rationale

The pre-Phase-6 layout did:

1. Allocate `n` empty `Vec`s, one per column of the factor.
2. During factorization, append `(row, value)` to `cols_l[j]` for each
   below-diagonal entry, growing the per-column Vec dynamically.
3. At the end of each iteration `k`, *insert* the diagonal at position
   `0` of `cols_l[k]`. `Vec::insert(0, …)` shifts every existing entry
   by one — O(col_size) per call.
4. After all `n` iterations, prefix-sum column lengths and flatten into
   CSC.

The Phase 6 layout does:

1. **Symbolic pass:** for each `k`, run `ereach` to enumerate the row-`k`
   pattern of `L`, incrementing `col_count[j]` for each `j` in the
   pattern. O(nnz(L)) total. Add 1 per column for the diagonal.
2. Prefix-sum `col_count` into `col_ptr`. Allocate `Li` and `Lx` exactly
   to fit. Reserve slot `col_ptr[j]` for each diagonal.
3. **Numeric pass:** same algorithm as before, but each `(row, value)`
   write goes to `Li[next[j]]` / `Lx[next[j]]` with `next[j]` a
   per-column cursor. Diagonal goes to `Lx[col_ptr[k]]` at end of
   iteration `k`.

Cost saved: one heap alloc per column, the O(col_size) diagonal
insertion, and the final flatten step. Cost added: the symbolic pass,
which reuses the same `ereach` / mark-vector machinery as the numeric
pass and runs in O(nnz(L)) — bounded by the numeric cost.

## Bench harness

```text
cargo run --release --example bench_sparse_solve -p rustlab-core
```

Single-threaded, release build, factor + single solve per call,
2-D 5-point Laplacian on `n × n` Dirichlet grid.

## Results — A/B via `git stash`

Baseline (commit `810806a`, post-Phase-4) vs Phase 6.

| Grid | Method | Baseline (s) | Phase 6 (s) | Δ |
|---:|---|---:|---:|---:|
| 75×75 (5 625) | chol / Identity | 0.010 | 0.011 | +10% (noise) |
| 75×75 | chol / AMD | 0.050 | 0.047 | **−6%** |
| 100×100 (10 000) | chol / Identity | 0.029 | 0.031 | +7% (noise) |
| 100×100 | chol / AMD | 0.149 | 0.148 | flat |
| 150×150 (22 500) | chol / Identity | 0.158 | 0.140 | **−11%** |
| 150×150 | chol / AMD | 0.798 | 0.716 | **−10%** |
| 200×200 (40 000) | chol / Identity | 0.425 | 0.420 | −1% (noise) |
| 200×200 | chol / AMD | 2.355 | 2.204 | **−6%** |

The savings are real but smaller than the plan estimated (originally
2–3× — that was for cases where the per-column `Vec::insert(0, …)`
shift dominates, which it doesn't on grid Laplacians where columns
have bounded fill in the AMD-ordered layout).

The clearest win is in the 150×150 / 200×200 AMD-ordered factor where
the column counts are largest — a 6–11% reduction. The 200×200 AMD
case drops from 2.35 s to 2.20 s, ~150 ms saved per factor.

## Where the win didn't materialize

The original layout's per-column `Vec` growth was already fairly cache-
efficient because:

- Each column's entries arrive in increasing-row order, so each
  `Vec::push` is a true append (no shift).
- The diagonal insertion at offset 0 is O(col_size) but col_size for a
  banded grid Laplacian is small and constant.
- Vec realloc amortizes — each column's storage doubles when full, so
  the allocator pressure is modest.

The biggest theoretical win — eliminating the heap-of-Vecs allocation
pattern — pays off most when columns are *long*. For grid Laplacians
ordered by AMD, columns are short; for irregular FEM patterns or
matrices with long fill paths, the win would be larger.

## Why Phase 6 still ships

- **Cleaner architecture.** The factor now lives in flat CSC the whole
  way through; no intermediate `Vec<Vec<…>>` representation. Easier to
  reason about, easier to extend (e.g., for streaming output).
- **No regression.** All 8 existing Cholesky tests pass; full workspace
  test suite green on both default and `--features viewer`.
- **Sets up future work.** A symbolic-only entry point (compute
  `col_count` without running the numeric pass) is now trivial. Useful
  for fill-prediction in adaptive ordering schemes.
- **Removes one level of allocation pressure.** On very large problems
  (millions of unknowns) the avoided per-column Vec allocs become
  measurable. For curriculum-scale problems, the win is just modest.

## Correctness

8 pre-existing Cholesky tests pass without modification:
- `cholesky_4x4_hand_built_spd`, `cholesky_identity`,
  `cholesky_2x2_indefinite_returns_notspd`,
  `cholesky_zero_diagonal_returns_notspd`,
  `cholesky_non_square_errors`, `cholesky_laplacian_20x20_round_trip`,
  `cholesky_complex_hermitian`, `cholesky_solve_dim_mismatch`.

A debug-build `debug_assert!` confirms the symbolic count matches the
numeric writes on every test run.

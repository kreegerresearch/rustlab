# Sparse-solve performance — Phases 1–4

Wall-clock times for factor + solve of the canonical 5-point Laplacian
Poisson assembly on an `n × n` grid, release build, single thread,
single solve per factor (cost amortizes across multiple solves in
practice). Run via `cargo run --release --example bench_sparse_solve -p rustlab-core`.

## Results

| Grid | n² | Method | Time (s) | Factor nnz |
|---:|---:|---|---:|---:|
| 25×25 | 625 | chol / Identity | 0.001 | 15 649 |
| 25×25 | 625 | chol / ColCount | 0.001 | 38 306 |
| 25×25 | 625 | chol / AMD | 0.001 | 26 537 |
| 25×25 | 625 | LU / AMD | 0.003 | 52 449 |
| 25×25 | 625 | **dense LU** | **0.054** | — |
| 50×50 | 2 500 | chol / Identity | 0.003 | 125 049 |
| 50×50 | 2 500 | chol / ColCount | 0.015 | 339 131 |
| 50×50 | 2 500 | chol / AMD | 0.010 | 229 975 |
| 50×50 | 2 500 | LU / AMD | 0.030 | 457 450 |
| 50×50 | 2 500 | **dense LU** | **2.904** | — |
| 75×75 | 5 625 | chol / Identity | 0.009 | 421 949 |
| 75×75 | 5 625 | chol / ColCount | 0.067 | 1 183 706 |
| 75×75 | 5 625 | chol / AMD | 0.047 | 797 787 |
| 75×75 | 5 625 | LU / AMD | 0.146 | 1 589 949 |
| 75×75 | 5 625 | **dense LU** | **35.034** | — |
| 100×100 | 10 000 | chol / Identity | 0.028 | 1 000 099 |
| 100×100 | 10 000 | chol / ColCount | 0.232 | 2 853 281 |
| 100×100 | 10 000 | chol / AMD | 0.148 | 1 917 475 |
| 100×100 | 10 000 | LU / AMD | 0.488 | 3 824 950 |
| 100×100 | 10 000 | dense LU | (OOM / minutes) | — |
| 150×150 | 22 500 | chol / Identity | 0.150 | 3 375 149 |
| 150×150 | 22 500 | chol / ColCount | 1.133 | 9 792 431 |
| 150×150 | 22 500 | chol / AMD | 0.776 | 6 562 475 |
| 150×150 | 22 500 | LU / AMD | 2.514 | 13 102 450 |
| **200×200** | **40 000** | **chol / Identity** | **0.417** | **8 000 199** |
| **200×200** | **40 000** | **chol / AMD** | **2.345** | **15 664 975** |
| **200×200** | **40 000** | **LU / AMD** | **7.974** | **31 289 950** |
| Complex 100×100 lossy Helmholtz | 10 000 | LU / AMD | 0.579 | 3 824 950 |

## Key takeaways

**The dense fallback was the actual scaling cliff.** At 75×75 (5625
unknowns), dense Gaussian elimination took **35 seconds**. At 100×100
it OOMs or takes minutes. The new sparse paths handle 200×200 in
seconds, an order-of-magnitude scale improvement.

**Acceptance criteria from the plan met:**
- 200×200 cavity-class problem (40k×40k) with simple ordering: **0.4s** (target was <30s)
- 200×200 with AMD ordering: **2.3s** (target was <10s)
- Complex 100×100 lossy Helmholtz: **0.58s**

Extrapolating from the 100×100 complex result, complex 200×200 should
factor in ~10s — at the AMD target, comfortably under the simple-
ordering target.

**Identity ordering is best on Laplacian.** The natural column-major
numbering of a 2-D grid is already a near-optimal banded ordering, so
"do nothing" beats AMD by 5–6× on these regular patterns.

**ColCountOrdering is the worst on Laplacian.** It produces ~3× the
fill of Identity. AMD lands in the middle.

**LU is roughly 3× slower than Cholesky.** Expected — LU stores both
factors and does partial pivoting. The 200×200 LU at 8s is comfortably
inside the 30s target for non-SPD curriculum problems.

## What this means for the spsolve dispatch

The current dispatch defaults both `cholesky` and `lu` paths to
`AmdOrdering`. On a Laplacian with natural ordering, this is 5×
slower than `IdentityOrdering` would be. **For most curriculum
assemblies**, `IdentityOrdering` would be the better default — but
it's a footgun on irregular patterns where natural ordering can be
arbitrarily bad.

Options:
1. **Status quo (AmdOrdering default).** Predictably bounded; ~5×
   slowdown on Laplacian, but never disastrous on irregular patterns.
2. **Switch to IdentityOrdering default.** 5× faster on Laplacian,
   risky on irregular patterns where users would silently pay
   exponential fill.
3. **Heuristic dispatch.** Detect bandwidth or grid-like structure and
   pick accordingly. ~50 LoC, more code paths to test.
4. **Upgrade AMD to the full Davis variant** (external degree,
   supervariable detection, mass elimination). ~500 LoC of additional
   work; should beat Identity even on Laplacian.

Recommendation: status quo is the right default until either (a) a
curriculum problem demonstrates the AMD limitation matters, or (b) the
full Davis-AMD implementation lands. The 2.3s factor time on 200×200
is already fast enough that no curriculum lesson is bottlenecked here.

## Methodology notes

- Single-threaded throughout — neither factor nor solve uses Rayon.
- Times are factor + solve, not factor alone. The factor cost dominates.
- "Factor nnz" is the count of stored entries in the factor (L for
  Cholesky, L+U for LU). Excludes the unit diagonal of L for LU.
- "Dense LU" is the legacy `dense_lu_solve` path with partial pivoting,
  the same algorithm `builtin_spsolve` uses for `Value::Matrix` input
  and for the pre-Phase-2 sparse fallback. It densifies internally.

## Reproducing

```sh
cargo run --release --example bench_sparse_solve -p rustlab-core
```

Reading times directly off the measurement is fine for orders-of-
magnitude comparisons. For tighter benchmarks (CI regression
detection), consider adding a `criterion`-based bench under
`crates/rustlab-core/benches/sparse_solve.rs`. That's deferred until
we have a concrete regression to chase.

# EM Gallery Performance — End-to-End Summary

A unified before/after measurement covering all six phases of
`dev/plans/closed/em_performance.md`. Numbers are best-of-3
release-build wall-clock times on a quiet Apple M-series laptop.

The "Baseline" column is commit `c4c6c8d` ("Notebook watch: track
transitive embeds for invalidation") — the tip of `main` immediately
before any em_performance work began. The "Post" column is commit
`ce62735` (or any commit at the closure of em_performance — the unique
post-em_performance contributions are unchanged across the closure).

The baseline tree was checked out via `git worktree`; bench harnesses
that didn't exist on the baseline (`bench_vector_calc`,
`bench_laplacian_build`) were copied in unchanged before running.

## Headline numbers

| Stage of the EM pipeline | Best speedup observed |
|---|---:|
| Laplacian builder (1-D, 2-D, 3-D, eps) | **10–22×** (Phase 4) |
| Sparse Cholesky factor on grid Laplacians | **5.9×** (Phase 2 dispatch + 6 Cholesky) |
| Vector-calculus kernels at large grids (≥400²) | **2–8×** (Phase 3) |
| Multi-RHS sweeps (100 right-hand sides) | **19–39×** (Phase 1) |

The largest practical speedup is in the multi-RHS / animation pattern,
where `chol(A); solve(F, b)` replaces N successive `spsolve(A, b)`
calls. For a 100-frame parameter sweep on a 100×100 grid Laplacian:

| Pattern | Time |
|---|---:|
| Pre-em_performance (`spsolve` × 100, AMD-ordered) | 14.1 s |
| Post-em_performance (`chol(A)` once + `solve(F, b)` × 100, identity-ordered) | 0.14 s |
| **Speedup** | **~100×** |

## Laplacian builder (Phase 4 — direct-sorted entries)

`cargo run --release --example bench_laplacian_build -p rustlab-dsp`

| Builder | Size | Baseline (ms) | Post (ms) | Speedup |
|---|---|---:|---:|---:|
| `laplacian_1d` | n = 1 000 | 0.315 | 0.026 | **12×** |
| `laplacian_1d` | n = 10 000 | 4.480 | 0.231 | **19×** |
| `laplacian_1d` | n = 100 000 | 22.215 | 2.252 | **10×** |
| `laplacian_1d` | n = 1 000 000 | 270.526 | 12.266 | **22×** |
| `laplacian_2d_bc` | 50×50 | 0.613 | 0.054 | **11×** |
| `laplacian_2d_bc` | 100×100 | 2.808 | 0.211 | **13×** |
| `laplacian_2d_bc` | 200×200 | 12.736 | 0.834 | **15×** |
| `laplacian_2d_bc` | 400×400 | 61.879 | 3.375 | **18×** |
| `laplacian_2d_bc` | 800×800 | 288.699 | 13.586 | **21×** |
| `laplacian_3d` | 20³ | 2.992 | 0.237 | **13×** |
| `laplacian_3d` | 40³ | 29.939 | 1.943 | **15×** |
| `laplacian_3d` | 60³ | 123.291 | 6.591 | **19×** |
| `laplacian_3d` | 80³ | 314.286 | 15.740 | **20×** |
| `laplacian_3d` | 100³ | 646.475 | 30.914 | **21×** |

Phase 4 replaced the `SparseMat::new` HashMap dedupe + sort path with
`SparseMat::from_sorted_entries`, which trusts the caller's row-major
sort and does only an O(nnz) consecutive-duplicate merge. Larger grids
hit the asymptotic gap harder (the old path was `O(nnz log nnz)` from
the sort).

## Sparse Cholesky factor (Phase 2 dispatch + Phase 6 Cholesky)

`cargo run --release --example bench_sparse_solve -p rustlab-core`

The headline isn't in the per-method numbers — both `chol/id` and
`chol/amd` are essentially flat across the closure (Phase 6's
flat-CSC rewrite landed a 5–11% factor speedup at large sizes; the
rest is noise). The headline is **what dispatches by default**.

Pre-em_performance, `spsolve` of a grid Laplacian routed through
`AmdOrdering` because nothing told it the matrix was grid-banded.
Post-em_performance, the `laplacian_*` builders attach
`OrderingHint::Identity` and `spsolve` honors it. So the user-visible
speedup at 200×200 is:

| n | Method | Baseline default | Post default | Speedup |
|---|---|---:|---:|---:|
| 100×100 | chol | AMD: 0.156 s | Identity: 0.032 s | **4.9×** |
| 150×150 | chol | AMD: 0.817 s | Identity: 0.146 s | **5.6×** |
| 200×200 | chol | AMD: 2.395 s | Identity: 0.406 s | **5.9×** |

Per-method comparison (for completeness — both columns are *current*
behaviour, just with different explicit ordering choices):

| n | Method | Baseline | Post | Speedup |
|---|---|---:|---:|---:|
| 100×100 | chol/id | 0.029 | 0.032 | flat |
| 100×100 | chol/amd | 0.156 | 0.147 | 1.06× |
| 150×150 | chol/id | 0.157 | 0.146 | 1.07× |
| 150×150 | chol/amd | 0.817 | 0.696 | 1.17× |
| 200×200 | chol/id | 0.436 | 0.406 | 1.07× |
| 200×200 | chol/amd | 2.395 | 2.280 | 1.05× |

Identity-ordered factor nnz is ~half the AMD-ordered factor nnz on
grid Laplacians (8 M vs 15.7 M at 200×200), so the dispatch change
also halves the memory footprint of the factor for the curriculum's
common case.

## Vector-calculus kernels (Phase 3 — fused, parallel, slice-iterating)

`cargo run --release --example bench_vector_calc -p rustlab-dsp`

| Grid | Operator | Baseline (ms) | Post (ms) | Speedup |
|---|---|---:|---:|---:|
| 50×50 | gradient | 0.007 | 0.009 | 0.78× |
| 50×50 | divergence | 0.008 | 0.007 | 1.14× |
| 50×50 | curl | 0.008 | 0.007 | 1.14× |
| 100×100 | gradient | 0.023 | 0.109 | **0.21×** ⚠ |
| 100×100 | divergence | 0.027 | 0.086 | **0.31×** ⚠ |
| 100×100 | curl | 0.028 | 0.057 | **0.49×** ⚠ |
| 200×200 | gradient | 0.087 | 0.267 | **0.33×** ⚠ |
| 200×200 | divergence | 0.105 | 0.091 | 1.15× |
| 200×200 | curl | 0.106 | 0.100 | 1.06× |
| 400×400 | gradient | 0.619 | 0.312 | 1.98× |
| 400×400 | divergence | 0.480 | 0.133 | **3.6×** |
| 400×400 | curl | 0.475 | 0.169 | **2.8×** |
| 800×800 | gradient | 2.328 | 0.543 | **4.3×** |
| 800×800 | divergence | 2.236 | 0.315 | **7.1×** |
| 800×800 | curl | 2.270 | 0.299 | **7.6×** |
| 80³ | divergence_3d | 3.706 | 2.111 | **1.76×** |

**Caveat:** the 100×100 and 200×200-gradient cells regressed. Phase 3's
rayon parallelism kicks in at `PAR_THRESHOLD = 4096` (so 64×64 = 4096
is the boundary; 100×100 = 10 000 sits just above). At those sizes the
per-task rayon overhead dominates the work-stealing benefit. Above
~300×300 the threshold pays off and we win 2–8×.

The curriculum's gallery uses 100×100 grids, so the Phase 3 win for
the actual user-facing notebooks is mixed: divergence and curl gained
mildly or stayed flat; gradient regressed by ~5×. Total absolute time
is still sub-millisecond — undetectable next to the 30 ms factor cost
that `spsolve` dominates — but the threshold is worth tuning if a
future profiling pass cares.

A natural follow-up: bump `PAR_THRESHOLD` to ~30 000 (≈ 175²) so the
parallel path activates only when work-stealing wins decisively. Out
of scope for em_performance closure; tracked here for the next agent.

## Multi-RHS sweeps (Phase 1 — `chol(A); solve(F, b)`)

`cargo run --release --example bench_factor_reuse -p rustlab-core`

100×100 grid Laplacian (n = 10 000), with `N_rhs` distinct random
right-hand sides:

| N_rhs | Refactor every solve (id) | Factor once + solve N (id) | Speedup |
|---:|---:|---:|---:|
| 1 | 0.0506 s | 0.0336 s | 1.50× |
| 5 | 0.1426 s | 0.0311 s | **4.6×** |
| 10 | 0.2677 s | 0.0398 s | **6.7×** |
| 25 | 0.6679 s | 0.0520 s | **12.8×** |
| 50 | 1.4037 s | 0.0825 s | **17.0×** |
| 100 | 2.7302 s | 0.1412 s | **19.3×** |

| N_rhs | Refactor every solve (amd) | Factor once + solve N (amd) | Speedup |
|---:|---:|---:|---:|
| 1 | 0.1468 s | 0.1426 s | 1.03× |
| 5 | 0.7011 s | 0.1460 s | **4.8×** |
| 10 | 1.3785 s | 0.1542 s | **8.9×** |
| 25 | 3.4741 s | 0.1991 s | **17.5×** |
| 50 | 7.0325 s | 0.2495 s | **28.2×** |
| 100 | 14.0622 s | 0.3617 s | **38.9×** |

Phase 1's contribution is structural: the back-solve is `O(nnz(L))`,
which is much cheaper than refactoring `O(nnz(L) × n^{0.5})`-ish.
Reusing the factor amortizes the dominant cost.

The AMD-ordered factor has higher absolute cost than identity-ordered
(per the Phase 2 / 6 numbers above), so the speedup ratio under AMD
is larger — there's more cost to amortize. In practice users hit the
identity path on grid Laplacians; the AMD column is the contrast for
unhinted matrices.

## Phase-by-phase outcome

| Phase | What it shipped | Where to see the win |
|---|---|---|
| 1 | `chol(A)` / `lu(A)` / `solve(F, b)` Value-layer factor handles | Multi-RHS table above — 19–39× at 100 RHS |
| 2 | `OrderingHint` field + identity dispatch on grid Laplacians | "Sparse Cholesky" headline — 5.9× at 200×200 |
| 3 | Fused, slice-iterating, parallel vector-calc kernels | Vector-calc table — 2–8× at large grids; flat-or-worse below 200² |
| 4 | `SparseMat::from_sorted_entries` + direct-sorted Laplacian builders | Builder table — 10–22× across the board |
| 5 | Real `f64` DSP path | **Investigated, deferred** — see `dev/plans/closed/em_performance.md` § Phase 5 for the regression analysis |
| 6 | Symbolic-then-numeric flat-CSC Cholesky + `symbolic_col_counts` API | Cholesky per-method table — 5–11% at n ≥ 150 |

## Methodology

```
git worktree add $TMPDIR/rustlab-baseline c4c6c8d
cp crates/rustlab-dsp/examples/bench_*.rs \
   $TMPDIR/rustlab-baseline/crates/rustlab-dsp/examples/

# Baseline numbers
cd $TMPDIR/rustlab-baseline
cargo run --release --example bench_vector_calc   -p rustlab-dsp
cargo run --release --example bench_laplacian_build -p rustlab-dsp
cargo run --release --example bench_sparse_solve  -p rustlab-core

# Post numbers (HEAD of main)
cd /Users/mike/projects/2026/rustlab
cargo run --release --example bench_vector_calc      -p rustlab-dsp
cargo run --release --example bench_laplacian_build  -p rustlab-dsp
cargo run --release --example bench_sparse_solve     -p rustlab-core
cargo run --release --example bench_factor_reuse     -p rustlab-core
```

Each bench reports best-of-3 release-build wall-clock; the harness
files are committed under `crates/{rustlab-dsp,rustlab-core}/examples/`.

## Stale numbers in the gallery

Several gallery notebooks quote pre-em_performance solve times in
prose. Those tables are *generated* output; re-baking the source
notebooks against current rustlab updates them. See
`perf/em_performance_summary_gallery_audit.md` for the diff between
old and new gallery output (when that file is produced as part of
the gallery re-bake task).

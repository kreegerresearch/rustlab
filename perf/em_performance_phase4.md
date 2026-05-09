# Laplacian builder performance — em_performance Phase 4

Phase 4 of `dev/plans/em_performance.md` rewrote the four `laplacian_*`
builders to emit entries in row-major-then-column-major sorted order
and call `SparseMat::from_sorted_entries`, skipping the HashMap dedupe
+ `O(nnz log nnz)` sort that `SparseMat::new` does.

The cost being skipped:
- `HashMap<(usize, usize), C64>` insert + lookup for every triplet
  (~50 ns / entry plus alloc churn).
- Full `entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)))`
  pass — `O(nnz log nnz)`.

The cost added:
- A tiny per-row sort of at most 5 (2-D) or 7 (3-D) `(col, val)`
  pairs in a stack-allocated buffer — effectively `O(nnz)` overall.
- A linear-time consecutive-duplicate merge in
  `from_sorted_entries` (handles the periodic-BC corner case where
  the wrap column coincides with an interior column at minimum grid
  sizes; harmless and free for typical sizes).

## Bench harness

```text
cargo run --release --example bench_laplacian_build -p rustlab-dsp
```

Best-of-3 wall-clock times; release build; quiet laptop (Apple M-series).

## Results

| Builder | Size | Pre-Phase-4 (ms) | Post-Phase-4 (ms) | Speedup |
|---|---|---:|---:|---:|
| `laplacian_1d` | n = 1 000 | 0.337 | 0.025 | 13× |
| `laplacian_1d` | n = 10 000 | 3.288 | 0.236 | 14× |
| `laplacian_1d` | n = 100 000 | 20.138 | 2.336 | 9× |
| `laplacian_1d` | n = 1 000 000 | 255.197 | 12.827 | 20× |
| `laplacian_2d_bc` | 50×50 | 0.605 | 0.053 | 11× |
| `laplacian_2d_bc` | 100×100 | 2.646 | 0.212 | 12× |
| `laplacian_2d_bc` | 200×200 | 12.062 | 0.888 | 14× |
| `laplacian_2d_bc` | 400×400 | 56.810 | 3.217 | 18× |
| `laplacian_2d_bc` | 800×800 | 273.270 | 14.214 | 19× |
| `laplacian_3d` | 20³ | 2.877 | 0.226 | 13× |
| `laplacian_3d` | 40³ | 28.493 | 1.895 | 15× |
| `laplacian_3d` | 60³ | 121.340 | 6.474 | 19× |
| `laplacian_3d` | 80³ | 305.177 | 14.863 | 21× |
| `laplacian_3d` | 100³ | 634.486 | 28.887 | **22×** |

The 100³ build is the headline: **635 ms → 29 ms**. That's the
difference between a build cost that dominates the gallery's 3-D
example and one that disappears into the noise of the surrounding
solve.

## Why the win grows with size

The HashMap dedupe is `O(nnz)` with a large constant (hash, lookup,
allocation). The COO sort is `O(nnz log nnz)`. Together they're
super-linear. Phase 4 replaces both with a single linear pass through
the sorted output, plus per-row sorts of bounded size (5 or 7
entries). The asymptotic gap explains why 100³ shows a 22× win where
1000-cell 1-D shows "only" 13×.

For curriculum-scale problems (100×100 grids, 100³ cubes), this puts
the build cost firmly below 30 ms in every case. Scripts that
iterate over a parameter sweep — varying `dx`, `eps_map`, or grid
resolution — now spend negligible time in builders.

## Correctness

4 new tests in `crates/rustlab-dsp/src/laplacian.rs::tests`:

- `lap_2d_direct_sorted_matches_legacy_coo_dirichlet` — for several
  `(nx, ny)` shapes, the new direct-sorted builder produces a
  `SparseMat` whose `entries` field equals the legacy
  `SparseMat::new`-via-HashMap result, entry by entry, to `1e-15`
  per value. Same for `_neumann` and `_periodic` variants.
- `lap_1d_periodic_minimum_size_dedupes` — at `n=2` the periodic
  wrap column coincides with the interior right-neighbour column at
  row 0. The new path emits two `(0, 1, +1/dx²)` entries; the
  consecutive-duplicate merge in `from_sorted_entries` sums them to
  `(0, 1, +2/dx²)`. Matches legacy behaviour.

All 14 pre-existing `laplacian_*` tests still pass without
modification.

## What's not in this phase

`SparseMat` storage is unchanged — still COO with row-major sorted
entries. The `to_csc` conversion that runs inside `try_sparse_*` is
also unchanged. A future phase could push CSC further upstream
(have builders return `SparseCsc<T>` directly), but that's a much
larger refactor and isn't on the critical path for the gallery's
runtime numbers.

The 22× build speedup at 100³ is far more than enough to justify
stopping here.

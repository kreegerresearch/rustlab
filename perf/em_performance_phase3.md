# Vector-calculus kernel performance — em_performance Phase 3

Phase 3 of `dev/plans/em_performance.md` rewrote `gradient_2d`,
`divergence_2d`, `curl_2d`, `divergence_3d`, and `curl_3d` (and their
internal kernels `d_dx`, `d_dy`, `d_along_axis_3d`) to:

1. Iterate via row slices (`as_slice`) instead of `[[i, j]]` indexed
   access — eliminates per-element bounds checks and lets LLVM
   autovectorize the inner loop on AVX2 / NEON.
2. Fuse `divergence` and `curl` into a single sweep that writes
   directly into the output, instead of allocating intermediate per-axis
   derivative tensors and summing them.
3. Parallelize the outer axis with `rayon::par_chunks_mut` when the
   grid has at least `PAR_THRESHOLD = 4096` elements.

A defensive `as_standard_layout` guard at each public entry point makes
non-contiguous inputs (e.g. ndarray slice views) transparently work.

## Bench harness

```text
cargo run --release --example bench_vector_calc -p rustlab-dsp
```

Best-of-3 wall-clock times; release build; quiet laptop (Apple M-series,
8 cores).

## 2-D results

| Grid `N×N` | `gradient_2d` (ms) | `divergence_2d` (ms) | `curl_2d` (ms) |
|---:|---:|---:|---:|
| 50×50 | 0.008 | 0.006 | 0.006 |
| 100×100 | 0.121 | 0.054 | 0.026 |
| 200×200 | 0.192 | 0.132 | 0.114 |
| 400×400 | 0.287 | 0.204 | 0.153 |
| 800×800 | 0.513 | 0.307 | 0.331 |

The 100×100 case is the canonical gallery size (see
`gallery/electrostatics.md`, `gallery/dielectric.md`). 100×100 gradient
in 0.12 ms means a 50-frame parameter sweep across the same operator
spends ~6 ms total in the vector-calc post-process — well below the
factor-and-solve cost dominated by `spsolve` / `solve(F, b)`.

## 3-D results

| Cube `N³` | `divergence_3d` (ms) |
|---:|---:|
| 20³ (8 000) | 0.058 |
| 40³ (64 000) | 0.289 |
| 60³ (216 000) | 0.557 |
| 80³ (512 000) | 2.034 |

The 80³ case is past the parallel threshold (512 000 ≫ 4096). The 3-D
fused kernel is currently the page-parallel path that builds per-page
slabs in worker tasks and copies them back into the output tensor — a
correctness-first design that avoids unsafe `axis_chunks_iter_mut`
juggling. A follow-up could remove the slab copy by structuring `out`
as `axis_chunks_iter_mut(Axis(2))` so each task writes directly into
its page — expected ~30% additional improvement at 80³ but not on the
critical path for any current curriculum example.

## What dropped

Per-call allocations:

| Operator | Before | After |
|---|---:|---:|
| `divergence_2d` | 3 × CMatrix (`d_dx`, `d_dy`, `+`) | 1 × CMatrix (`out`) |
| `curl_2d` | 3 × CMatrix | 1 × CMatrix |
| `divergence_3d` | 4 × CTensor3 (3 × `d_along_axis_3d` + sum) | 1 × CTensor3 (+ per-page slabs in parallel path) |

For 200×200 the savings are 2 × 320 KB = 640 KB per call. For 80³ they
are 3 × 8 MB = 24 MB per call — the larger-grid wins are bigger in
absolute terms because intermediates scale with grid volume.

## Correctness

5 new tests in `crates/rustlab-dsp/src/tests.rs::vector_calc_phase3_tests`:

- `fused_div_2d_matches_compose`, `fused_curl_2d_matches_compose` — the
  fused single-sweep kernels match `d_dx(fx) + d_dy(fy)` /
  `d_dx(fy) - d_dy(fx)` to `1e-12` on RNG-seeded inputs.
- `parallel_matches_serial_2d`, `parallel_matches_serial_3d` — with the
  test-only `__test_set_par_threshold(Some(1))` knob forcing the
  parallel path on a tiny grid, parallel and serial outputs match
  *bit-exactly* (same operations in the same order, just sharded
  across threads). 2-D is direct equality; 3-D is `< 1e-12` because
  the per-page slab path differs in scratch usage.
- `non_contiguous_input_still_works` — slice-of-slice inputs don't
  break the kernel.

All 11 pre-existing vector-calc tests (analytic checks: paraboloid
gradient, radial divergence, vortex curl, etc.) still pass without
modification.

## Threshold tuning

`PAR_THRESHOLD = 4096` is conservative — a 64×64 grid is right at the
boundary where rayon's per-task overhead starts paying off on a
modern multicore. Below it the kernel runs serially and costs <0.1 ms
per call; above it the speedup tracks core count up to memory-bandwidth
saturation.

A future tuning sweep could chase the optimal threshold per kernel
(pure-stride kernels like `gradient` benefit at smaller sizes than
fused kernels with branch-heavy inner loops), but the current single
threshold is fine for the curriculum's grid sizes.

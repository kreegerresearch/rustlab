# Perf flywheel iteration 02

**Status:** in progress
**Current phase:** item 5 (R4 real-storage design)
**Branch:** `feature/perf-iter02` (one PR, a commit per item, version 0.3.11)
**Source request:** `../rustlab_llm/perf/iterations/02-requests.md`
(measured on rustlab 0.3.10, `1ebb762`, Apple M4 Max, 2026-10-18; training
step 9,542–9,967 tok/s; success at 14,000 tok/s with the gate passing).
Protocol: `../rustlab_llm/docs/perf-flywheel.md`. Previous iteration:
[`closed/perf_flywheel_iter01.md`](closed/perf_flywheel_iter01.md).

Self-check (records nothing):

```sh
cargo build --release -p rustlab-cli
RUSTLAB=$PWD/target/release/rustlab make -C ../rustlab_llm perf-check
```

## Diagnosis

The 0.3.10 op profile of `perf/profile_step.rlab` shows the remaining cost is
mostly copying. A dense value is copied in full whenever a variable or field
is read into a builtin call, a struct is passed to a user function, a lambda
captures it (once per `parmap` task), or `x = y` runs. `numel(S.E)` copies
`S.E` to count it; `reshape(theta, …)` copies its argument, gathers it in
column order, then copies again; `[a, b, …]` grows its output by
reallocation. Argument copies are charged to the caller's self time, so they
are also part of R2's "interpreter work". The root fix is copy-on-write array
storage (roadmap C11, `eval_perf_value_borrow.md`).

## Order (approved 2026-10-18)

| # | Item | Request | Status |
|---|---|---|---|
| 1 | Transposed operands to dgemm as views | R3 | done |
| 2 | Copy-on-write `Vector` / `Matrix` / `Tensor3` storage | R1, R2 (C11) | done |
| 3 | Single-pass `reshape`, preallocated `[…]`, zeroed allocation for `zeros` | R1 | done (parallel ratio missed) |
| 4 | Per-statement interpreter overhead, from a sampling profile | R2 | done |
| 5 | Real-valued storage: design document only (`real_storage.md`) | R4 | not started |

## 1. R3: transposed operands (done)

`eval_arith_binop` sends `Mul` with a `'` / `.'` operand to
`eval_transposed_matmul`. Each side is evaluated once (a variable is
borrowed); when both are matrices and the inner dimensions agree, the product
is `rustlab_core::matmul::matmul_conj(view, conj, view, conj)` on the
transposed views. Otherwise the transposes are written out and the ordinary
path runs, so vectors and the dimension-mismatch error are unchanged.

`matmul_conj`:
- one pass per operand converts to `f64` and checks the imaginary parts at
  once, in the operand's memory order (`real_parts`), replacing `is_real` +
  `mapv` (two passes, and the check did not vectorize);
- writes into a row-major output with `general_mat_mul` (`dot` follows the
  operands' memory order, so a transposed operand gave a column-major
  product that the elementwise fast paths reject);
- conjugates a complex operand under `'` as a copy inside the product (the
  same values the written-out transpose held).

Tests: `matmul::tests::transposed_views_are_bit_identical_and_row_major`
(real and complex, three transpose forms, five shapes), and
`tests::transposed_matmul` (every evaluator form bit-identical to writing the
transpose out first, same mismatch error, no `[transpose]` profile row).

Self-check, two runs (0.3.10 → now): `prim.matmul_A_Bt_129x128x128`
37.6 → 47.0–47.3 GFLOP/s (target ≥ 45); `prim.matmul_At_B_512x129x512`
53.5 → 55.3 (target ≥ 53); other product keys +3–14 % from the fused check;
no `[transpose]` rows in `--profile=ops perf/profile_step.rlab` (were 784
worker + 294 serial). `e2e.train_step_B8` 9,872–10,420 tok/s. Gate PASS,
trajectory exact.

Machine-state caveat (2026-10-19): the next day the R3 binary itself
measured `prim.matmul_A_Bt_129x128x128` at 42.2 GFLOP/s, and so did the
item 2 build (same 0.093 ms for the product, side by side). The key is
`X * W1(:, 1:128)'`, so it also pays the 5.5 µs slice; the product alone
is 45 GFLOP/s. Re-measure before reporting the target as met.

## 2. Copy-on-write arrays (C11) (done)

`Value::Vector`, `Matrix` and `Tensor3` hold `eval::shared::Shared<T>`, an
`Arc` with `Deref`. Every `Value` clone now shares the buffer: variable
reads, builtin and user-function arguments, `S.field` reads, lambda
captures, struct copies, `parmap` captures and results.

- Writes call `Shared::make_mut` once per write site (25 sites: index
  assignment, region and row writes, negation, `cumsum`, `ellipke`,
  `pin_dirichlet`, the elementwise in-place kernels). There is no
  `DerefMut`: the first version had one, and `m[[r, c]] = x` in a loop then
  ran `Arc::make_mut` (an atomic check) per element, which made
  `prim.region_write_128x384` 1.9× slower. Removing it made the compiler
  list every write site.
- `elementwise::binop` writes into an owned temporary only when its buffer
  is unique; a shared one (a clone of a variable) is read like a borrowed
  operand, so the variable is never written and no copy is made.
- Construction sites became `Value::Vector(arr.into())` (363 of them,
  applied from rustc's suggestions); places that need the array itself use
  `(**v).clone()` or `Shared::into_inner`.

Tests: `shared::tests` (copy on write, in-place when unique, `into_inner`),
`tests::copy_on_write` (`b = a` shares then separates on write; `+=`,
growth, index lists, region and row writes, negation, Tensor3; a function
writing its argument; lambda and `parmap` snapshots; struct copies and field
reads; a `for` loop over a vector it modifies; unique buffers written in
place), and `elementwise::tests::check` now runs every operator with
borrowed, unique-owned and shared-owned operands on each side (9 mixes),
checking bit identity with `Value::binop` and that shared operands are not
written. Workspace: 3,423 passed, 0 failed.

Self-check, R3 binary vs item 2 on the same machine and day (2026-10-19):

| Key | R3 | Item 2 (two runs) |
|---|---:|---:|
| `e2e.train_step_B8` | 9,197 tok/s | 13,230–13,610 tok/s |
| `par.grad_flat_B8` | 79.2 ms | 54.0–54.5 ms |
| `model.grad_flat` | 42.5 ms | 35.0–35.2 ms |
| `model.forward` | 14.8 ms | 12.5–12.6 ms |
| `model.backward` | 20.1 ms | 17.5–17.6 ms |
| `prim.call_struct_arg_13MB` | 0.208 ms | 0.002 ms |
| `prim.region_write_128x384` | | 0.076–0.077 ms (0.3.10: 0.083) |

`par.grad_flat_B8` / `model.grad_flat` is 1.54× (target ≤ 1.3×, item 3).
Gate PASS both runs, trajectory exact.

## 3. R1: reshape, concatenation, zeros (done; parallel ratio missed)

- `reshape` of a vector or matrix (`builtins::reshape_2d`): storage is
  row-major and `reshape` walks column-major, so a matrix's column-major
  order is its transpose. One `elementwise::par_transpose` pass each way,
  none when the order does not change (vector to vector or 1×N / N×1, same
  shape: the value is shared). Tensor3 sources and outputs keep the old
  walk.
- `par_transpose`: 32 × 32 tiles, parallel chunks from `PAR_MIN_LEN` (a
  chunk's partial first and last output rows go element by element). One
  side of a tile is strided; a stride with many factors of two maps the
  tile to a few L1 sets, so the kernel strides the side with fewer
  (contiguous writes on a tie). Measured: always-strided reads made
  `prim.transpose_129x512` 1.5× slower (stride 512), always-strided writes
  made `reshape(theta, 1584, 512)` 2× slower (stride 512); a buffered tile
  was slower than both. `Value::transpose_matrix` uses it, so large `A'`
  is parallel.
- One-row `[…]` of scalars, vectors and 1×N matrices
  (`Value::concat_one_row`): total length first, then `par_fill`; `[v]`
  shares `v`. Taller blocks keep the general path.
- `zeros`: unchanged. `zeros(811008)` already takes 0.045 ms (the
  allocation is lazily zeroed), so a zeroed allocation would not measure
  faster.

Tests: `elementwise::tests::par_transpose_matches_the_definition` (serial
and parallel, partial rows, conjugate), `tests::reshape_and_concat`
(every vector/matrix reshape against the column-major walk, serial and
parallel sizes, the error text, sharing until written, one-row concat
against appending with pieces across chunk boundaries, `[v]` shared,
taller blocks). Workspace: 3,430 passed, 0 failed.

Self-check, two runs (item 2 → now): `flat.reshape_811k` 3.66 →
0.14–0.16 ms (target ≤ 0.5); `flat.flatten_811k` 2.4 → 0.41–0.48 ms
(≤ 1.5); `flat.unflatten_811k` 4.0 → 1.53–2.00 ms (≤ 2; the slices
`v(i:i+n-1)` are a copy before the reshape); `e2e.train_step_B8`
13,230–13,610 → 17,030–17,500 tok/s; `prim.transpose_129x512` 0.024 ms
(unchanged). Gate PASS, trajectory exact.

**Missed:** `par.grad_flat_B8` / `model.grad_flat` is 46.7–49.6 / 30.9–31.3
= 1.5–1.6× (target ≤ 1.3×). What it is not:
- the allocator: a local build with `mimalloc` as the global allocator
  (not committed) left `par.grad_flat_B8` at 45.7–46.3 ms. It did make the
  training step 18 % faster (49.4–49.8 ms, 20,570–20,740 tok/s), from the
  serial part of the step; a dependency decision for later, with a
  trade-off study under Rule 10.
- efficiency cores: this M4 Max has 10 performance and 4 efficiency
  cores; B1 equals serial (31.5 ms), B8 is 1.58×, B14 is 2.1×. Eight
  windows fit on the performance cores.
Likely shared cache and memory bandwidth (eight windows each touch the
13 MB of parameters and their activations). Moving fewer bytes is the
lever: real-valued storage (R4) halves them.

## 4. R2: interpreter overhead (done)

Sampling profile (`sample`, symbols kept via `CARGO_PROFILE_RELEASE_STRIP=none`)
of 400 serial forward + backward passes after item 3: matrix multiply ~45 %,
`exec_index_assign` self time ~12 %, `tanh` (inside `gelu` / `gelu_grad`)
~11 %, elementwise kernels and `real_parts` most of the rest. Hashing,
`eval_expr` and `Value` clones were under 2 %: the "interpreter work" in the
request was mostly what the statements do, not dispatching them. The
`--profile=ops` breakdown per pass: `[matmul]` 14.7 ms, `[index-assign]`
3.8 ms (≈ 293 writes at 12.4 µs), `gelu` + `gelu_grad` 3.8 ms.

- Region writes `M(rows, cols) = val` (`Evaluator::region_write_runs`):
  when `cols` is one consecutive run, `M` row-major and `val` already
  region-shaped, each target row is one `copy_from_slice` / `fill`; else
  the general path (and its errors). They had built a temporary and then
  scattered it, both element by element with bounds-checked 2-D indexing,
  at ~10 GB/s; now 77–88 GB/s.
- Scalar-function builtins (`apply_scalar_fn_promoting`: `tanh`, `exp`,
  `gelu`, `gelu_grad`, trig, …) map contiguous arrays with
  `elementwise::par_map`, parallel from `PAR_MAP_MIN_LEN` = 16,384 elements
  in 4,096-element chunks (`drive_new_sized`, the same rayon instantiation
  as `drive_new`). Per-element results are unchanged. Inside `parmap`
  workers this adds nested tasks, which did not slow `par.grad_flat_B8`.
- Scalar loop (`acc = acc + k`): its profile was SipHash, `malloc` / `free`
  and `memmove` of a new `String` key per assignment.
  `eval/fast_hash.rs` (hand-rolled Fx hash; the variable names are the
  script's own, so flooding resistance is not needed) for `env` and lambda
  `captured_env`, and `Evaluator::set_env`, which overwrites an existing
  entry in place (assignments, the loop variable, the `end` binding).

Tests: `fast_hash::tests`, `tests::interpreter_fast_paths` (region writes
through both paths against element-by-element writes, the shared copy
unchanged, shape errors unchanged; `gelu`, `gelu_grad`, complex `tanh`,
`exp` above and below the parallel threshold bit-identical to the scalar
call; rebinding in a loop). Workspace: 3,436 passed, 0 failed.

Self-check, two runs (item 3 → now): `model.forward` 12.5 → 9.48–9.49 ms
(target ≤ 10); `model.backward` 17.4–17.6 → 14.6–14.9 ms (≤ 15, close);
`prim.scalar_loop_1e5` 13.1 → 7.9–8.1 ms (≤ 10);
`prim.region_write_128x384` 0.076 → 0.009 ms; `model.grad_flat` 31 →
24.7–24.8 ms; `e2e.train_step_B8` 17.0–17.5k → 18.05–18.52k tok/s. Gate
PASS, trajectory exact. `par.grad_flat_B8` 43.8–44.2 ms, now 1.78× the
serial `model.grad_flat`: the serial side gained the parallel math
builtins, which a `parmap` worker cannot use (the cores are already busy).

## 5. R4: real-valued storage (design only)

`dev/plans/real_storage.md`: representation (inside `Shared<T>`), when values
become complex, builtin migration, `parmap` transfer, `save`/`load`. First
slice deferred to iteration 03.

## Response note (filled in at the end)

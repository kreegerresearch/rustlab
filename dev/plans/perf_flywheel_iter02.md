# Perf flywheel iteration 02

**Status:** in progress
**Current phase:** item 3 (R1 reshape, concatenation, zeros)
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
| 3 | Single-pass `reshape`, preallocated `[…]`, zeroed allocation for `zeros` | R1 | not started |
| 4 | Per-statement interpreter overhead, from a sampling profile | R2 | not started |
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

## 3. R1: reshape, concatenation, zeros

`reshape` as one tiled (parallel when large) pass into row-major order, free
for vector ↔ 1×N / N×1; `[…]` sized once with parallel block copies; `zeros`
from zeroed allocation if it measures faster. Targets: `flat.reshape_811k`
≤ 0.5 ms, `flat.flatten_811k` ≤ 1.5 ms, `flat.unflatten_811k` ≤ 2 ms,
`par.grad_flat_B8` ≤ 1.3 × `model.grad_flat`.

## 4. R2: interpreter overhead

Sampling profile of serial forward / backward after item 2, then fix what it
shows (candidates: a hand-rolled fast hasher for the variable map, no `String`
allocation for `end` and call environments, shared `UserFn` / lambda bodies).
Targets: `model.forward` ≤ 10 ms, `model.backward` ≤ 15 ms,
`prim.scalar_loop_1e5` ≤ 10 ms.

## 5. R4: real-valued storage (design only)

`dev/plans/real_storage.md`: representation (inside `Shared<T>`), when values
become complex, builtin migration, `parmap` transfer, `save`/`load`. First
slice deferred to iteration 03.

## Response note (filled in at the end)

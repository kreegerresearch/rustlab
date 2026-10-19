# Perf flywheel iteration 02

**Status:** in progress
**Current phase:** item 2 (copy-on-write arrays)
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
| 2 | Copy-on-write `Vector` / `Matrix` / `Tensor3` storage | R1, R2 (C11) | not started |
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

## 2. Copy-on-write arrays (C11)

`Value::Vector`, `Matrix` and `Tensor3` hold `Shared<T>` (an `Arc` with
`Deref`); writes go through `make_mut`, which copies only when shared. The
elementwise in-place reuse of temporaries applies only to a unique buffer, so
the bit-identity tests still hold. Tests: aliasing (`b = a; b(1) = 5` leaves
`a`), a function modifying its argument, lambda snapshots, `parmap`, struct
writes after a copy.

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

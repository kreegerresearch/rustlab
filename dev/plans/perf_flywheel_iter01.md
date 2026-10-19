# Perf flywheel iteration 01

**Status:** complete (pending PR review)
**Current phase:** done — response note below
**Branch:** `feature/perf-iter01` (one PR, version 0.3.10)
**Source request:** `../rustlab_llm/perf/iterations/01-requests.md`
(measured on rustlab 0.3.9, `65d6507`, Apple M4 Max, 2026-10-18).
Protocol: `../rustlab_llm/docs/perf-flywheel.md`.

Self-check after each item, without recording anything on the
`rustlab_llm` side:

```sh
cargo build --release -p rustlab-cli
RUSTLAB=$PWD/target/release/rustlab make -C ../rustlab_llm perf-check
```

The gate (gradient check < 1e-6, 25-step trajectory within relative 1e-6)
must pass on every run.

## Decisions (approved 2026-10-18)

- One PR, one commit per request, in the order R3 → R1 → R2 → R4.
- R1 uses `matrixmultiply::dgemm`, already in the tree through ndarray.
  BLAS / Accelerate is declined: Rule 10 forbids FFI for core numerics.
- R4 may include a narrow slice of roadmap C11 (borrowing a variable's
  value instead of cloning it on hot read paths). The full C11 refactor and
  real-valued storage stay deferred.

## R3: `parmap` without per-task environment clones

**Status:** done (target partly met)

- `clone_for_parallel_lambda(&func)` builds a worker template whose `env`
  holds only the built-in constants (the caller's values for those names, so
  a shadowed `pi` behaves as in a sequential call). Lambdas swap in their
  captured env; user functions seed a fresh scope. `@feval` keeps the full
  env, because `feval` resolves names there. — done
- `LocalRayonBackend` uses `map_init`: one worker per rayon split, reused
  across its tasks. — done
- `pack_vectors_as_matrix` validates rows, then copies them in parallel into
  an uninitialised output. — done
- Found while measuring: `S.field` evaluated `S` by deep-copying the whole
  struct. `Expr::Field` now borrows a variable or a field chain
  (`borrow_field_chain`) and clones only the field. `lib/gpt.rlab` reads
  ~30 fields of the 13 MB parameter struct per window. — done

Tests: `parmap_worker_env` (template drops caller variables, captured values
visible, user-fn handle chains, user fn cannot see caller variables like the
sequential call, shadowed constant, `feval` inside a lambda, 500 tasks on
reused workers) and `struct_field_borrow` (nested chain, copy not alias,
missing-field errors, tables).

| Key | 0.3.9 | After R3 | Target |
|---|---:|---:|---|
| `e2e.train_step_B8` | 456 ms (2,246 tok/s) | 295–314 ms (3,257–3,466 tok/s) | — |
| `model.grad_flat` | 152 ms | 145 ms | (R1) ≤ 80 ms |
| `par.grad_flat_B8` | 322 ms | 195–198 ms (1.35× serial) | ≤ 1.3× serial — **partly met** |
| `par.grad_flat_B14` tok/s vs B8 | 2,844 < 3,177 | 5,945–6,606 > 5,182–5,244 | ≥ B8 — **met** |
| trivial 8-task parmap, 104 MB caller env | 6.0 ms | 0.13 ms | within 2× of small env (0.14 ms) — **met** |

Remaining gap to 1.3×: each task still deep-copies the lambda's captured
values once, and the window body itself clones variables on every read
(C11), which costs more under 8 concurrent threads. R4's borrow slice should
close more of it.

## R1: real-valued matrix multiply fast path

**Status:** done (all targets met)

- `rustlab_core::matmul::{matmul, is_real}`: if both operands have
  all-zero imaginary parts, multiply the real parts with ndarray's `f64`
  `dot` (`matrixmultiply::dgemm`, single-threaded, no new dependency) and
  widen; otherwise the complex `dot`. — done
- `Matrix * Matrix` and row vector × matrix (previously a hand loop over
  strided columns) both call it. — done
- Tests in `rustlab-core` (`matmul::tests`): real path vs naive triple loop
  on square, 1×N, N×1 and model shapes; complex and mixed operands;
  transposed views; NaN propagation; `Inf` stays real; `-0.0` imaginary
  counts as real. `make octave-compare`: 148/148.

| Key | 0.3.9 | After R1 | Target |
|---|---:|---:|---|
| `prim.matmul_512x512x512` | 8.6 GFLOP/s | 50.8 GFLOP/s | ≥ 40 — **met** |
| `prim.matmul_129x128x512` | 8.6 GFLOP/s | 31.3 GFLOP/s | ≥ 25 — **met** |
| `model.grad_flat` | 152 ms | 75.9 ms | ≤ 80 ms — **met** |
| `par.grad_flat_B8` | 322 ms | 145 ms (1.91× serial) | (R3) ≤ 1.3× serial |
| `e2e.train_step_B8` | 2,246 tok/s | 4,194 tok/s | — |

Gate PASS; the 25-step trajectory still matches the reference exactly.
With the window twice as fast, parmap efficiency fell to 52 %: the
remaining per-window cost is copies and allocation, which contend across
threads. That is R4's territory.

## R2: profiler that sees inside user functions

**Status:** done (all targets met)

- `eval/profile.rs` rewritten around a frame stack: `enter(name)` /
  `exit(name, in, out)` record inclusive time (outermost activation only)
  and self time (minus profiled children). The old `higher_order_depth`
  suppression and `should_track` are gone. — done
- Pseudo-rows: `[top-level]` (root frame from `enable`; restarted by each
  report), `[matmul]` always, and under `enable_ops()` operator rows,
  `[transpose]`, `[index]`, `[index-assign]` (`enter_op` / `exit_op`; the
  `IndexAssign` arm moved verbatim into `exec_index_assign`). — done
- `parmap` workers get `Profiler::for_worker()`; `ParmapBackend::run`
  returns `ParmapOutput { values, profiles }` and the caller merges each
  task's rows as `parmap:<name>` with `parallel = true`, excluded from
  `TOTAL`. — done
- `rustlab run --profile[=calls|ops]` (`require_equals`, so a bare
  `--profile script.rlab` still works). `rustlab run` without `--profile`
  now prints the report at exit when the script enabled `profile()`. — done
- Tests: 9 new profiler unit tests (nested split with sleeps, self sum =
  wall, recursion, `[matmul]` vs ops rows, index rows, parmap merge,
  whitelist) replacing the two that asserted suppression; CLI
  `tests/profile_report.rs` (exit report, bare flag, `=ops`, no report
  when off).

| Acceptance | Result |
|---|---|
| `--profile profile_step.rlab` lists `gpt_forward`, `gpt_backward`, `gpt_ln_fwd`, `gpt_gelu_grad`, a matmul row | **met** (main-thread rows plus `parmap:` rows) |
| Self times sum to within 10 % of run time | **met** by construction: `TOTAL` = Σ self = `[top-level]` inclusive |
| Overhead ≤ 10 % on `model.grad_flat` | **met**: 73.4–73.8 ms off, 73.7 ms `--profile`, 73.9–74.2 ms `--profile=ops` |
| In-script `profile()` reports at exit | **met** |

Cost with profiling off: `prim.scalar_loop_1e5` 16.75 → 17.2 ms (~3 %,
repeatable, below the 10 % noise rule). It does not move with `#[inline]`
on the disabled-path checks, so it is most likely code layout in the larger
`eval_expr`; R4 reworks this path.

**Contract note for `rustlab_llm`:** the table gained a `Self (ms)` column
after `Total (ms)`, so `perf/run.py`'s seven-number regex no longer matches.
An eight-column version:
`\s+(\S+)\s+(\d+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*$`
with groups name, calls, total ms, self ms, avg µs, in KB, out KB, Mbit/s.

Findings from the first nested profile (for iteration 02): inside each
window, `gpt_backward` self time (236 ms CPU over 8 windows) and
`gpt_forward` self (147 ms) are interpreter work outside builtins;
`gpt_gelu_grad` is 110 ms; `gpt_flatten` runs per window (64 ms);
`zeros` allocations 29 ms; `gpt_sinusoidal_pe` calls `sin`/`cos` 33,024
times each.

## R4: fast paths for elementwise ops and reductions

**Status:** done (5 of 6 targets met; unflatten needs a library change)

The `--profile=ops` table showed the flat work was copies and allocation,
not arithmetic, so the changes are about not copying:

- `eval/elementwise.rs`: operands borrowed when they are array variables,
  owned temporaries reused for the result, one monomorphised kernel per
  operator (a function pointer had blocked inlining), rayon chunks from
  131,072 elements. Bit-identical to `Value::binop`, including the
  real-output rule (fused flags, fix-up pass only when it changes bits).
  `Matrix * Matrix` also borrows. — done
- Scalar operands keep the direct `Value::binop` path with one lookup per
  variable (the first version cost the scalar loop 40 %). — done
- Indexing borrows: `Value::index_ref(&self)` (owned `index` delegates),
  `IndexShape` for `end`; the call branch moved to `eval_var_index`
  (`#[inline(never)]`, since growing `eval_expr`'s frame overflowed the
  2 MB test stack in a recursive test). — done
- Consecutive subscripts: `resolve_index_dim` and `index_1d` skip the
  per-element checks for a run; `index_2d` copies a run block as a slice,
  in parallel when large. Integer ranges fill `s + k·inc` directly (exact,
  equal to the accumulation loop). — done
- `sum(M, 1)` adds rows (parallel column chunks when large), same order;
  `matrix_axis_collect_then` reuses one buffer. — done
- User functions move outputs out of their scope instead of cloning. — done
- `gelu_grad(x)` builtin, docs, help, quickref, AGENTS. — done

Tests: `elementwise::tests` (every operator × shape × ownership vs
`binop`, bit for bit, small and parallel sizes; buffer reuse; parallel
copies and column sums vs sequential), `integer_range_fill`,
`gelu_grad_*` (finite difference, written-out formula). Workspace 3,405
passed; `make octave-compare` 28 + 148 passed.

| Key | 0.3.9 | After R4 | Target |
|---|---:|---:|---|
| `flat.adamw_811k` | 19.5 ms | 4.6–5.0 ms | ≤ 5 ms — **met** (at the edge) |
| `flat.batch_reduce_8x811k` | 29.4 ms | 2.7 ms | ≤ 5 ms — **met** |
| `flat.axpy_811k` | 457 Melem/s | 2,843–3,111 Melem/s | ≥ 1,500 — **met** |
| `flat.unflatten_811k` | 20.5 ms | 18.1–18.3 ms | ≤ 5 ms — **not met** (see below) |
| `gelu_grad` builtin, 129×512 | — | 0.35 ms | ≤ 0.5 ms — **met** |
| `e2e.train_step_B8` | 2,246 tok/s | 7,967–8,269 tok/s | — |

`flat.unflatten_811k`: 19 of its ~18–27 ms is `gpt_sinusoidal_pe`, a
16,512-iteration scalar loop that `gpt_unflatten` re-runs on every call
although the table is a constant. Without it the rest takes about 4 ms.
That is a library change (compute PE once, or vectorise it); it is in the
response note.

Scalar interpreter: after R4, `prim.scalar_loop_1e5` was 17.4 ms against
16.6 ms for the 0.3.9 binary (~5 % slower: R2's larger `eval_expr` plus the
operand pre-check). Fixed with a direct path in `eval_arith_binop` for two
real scalars and `+ - * .* / ./` when profiling is off, the same `f64`
operation `Value::binop`'s Scalar∘Scalar arm performs (test
`scalar_fast_path::matches_binop_bit_for_bit`, NaN / ±Inf / −0 included).
Side by side with `main`: 16.4–16.5 → 13.3–13.6 ms (~18 % faster);
`bench_interpreter` 5.64 → 5.10 ms; harness `prim.scalar_loop_1e5` 1.13×.

## Response note (filled in at the end)

## Perf loop iteration 01: response
Revision: `feature/perf-iter01` (rustlab 0.3.10), not yet merged or installed — 2026-10-18.

| # | Status | What changed | Self-check numbers (two runs) |
|---|---|---|---|
| R1 | landed | Real operands of `A * B` and row-vector × matrix use `f64` dgemm (`rustlab_core::matmul`). No new dependency; BLAS/Accelerate declined (rustlab forbids FFI in core numerics). | 512³ 50.8 GFLOP/s; 129×128×512 31.3 GFLOP/s; `model.grad_flat` 49–50 ms |
| R2 | landed | Call-stack profiler: nested rows with Total (inclusive) and Self; `[top-level]`, `[matmul]`; `--profile=ops` operator/index rows; `parmap:<name>` worker rows; in-script `profile()` reports at exit. | all four acceptance checks met; overhead < 1 % |
| R3 | partial | Workers start without the caller's variables, are reused per rayon split, rows pack in parallel; struct field reads copy one field. | B8 86–89 ms = 1.75× serial (target 1.3×); B14 14,000 tok/s > B8 11,500–11,900; trivial parmap 0.13 ms with a 104 MB caller env (was 6.0) |
| R4 | partial | Borrowed operands and indexing, in-place temporaries, parallel kernels ≥ 131k elements, block slices, integer range fill, row-wise `sum(M,1)`, moved function outputs, `gelu_grad` builtin. | adamw 4.6–5.0 ms; reduce 2.7 ms; axpy 2,843–3,111 Melem/s; gelu_grad 0.35 ms; unflatten 18 ms (not met) |

Training step: **2,246 → 7,967–8,269 tok/s (3.6–3.7×)**, gate PASS on every
run with the 25-step trajectory matching the reference exactly (worst
relative difference 0).

Notes for `rustlab_llm`:
- **Breaking: the profile table has eight columns.** `Self (ms)` sits after
  `Total (ms)`, so `perf/run.py`'s seven-number regex matches nothing and
  the report drops its profile section. Eight-column regex:
  `\s+(\S+)\s+(\d+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*$`
  (name, calls, total ms, self ms, avg µs, in KB, out KB, Mbit/s). `TOTAL`
  is now Σ main-thread self; `parmap:*` rows are worker CPU time.
- **Adopt `gelu_grad(x)`** in `gpt_gelu_grad` (run `01b`): 0.35 ms vs
  0.92 ms for the library formula on 129×512; matches it to < 1e-14.
- **`gpt_unflatten` recomputes `gpt_sinusoidal_pe` every call** (~19 ms of
  the unflatten key, and it runs inside every serial `gpt_grad_flat` in
  `profile_step.rlab`). PE is a constant; computing it once (or with
  `sin`/`cos` on a matrix of `t·ω`) should bring `flat.unflatten_811k`
  under 5 ms.
- From the nested profile: per window, `gpt_backward` and `gpt_forward`
  self time (interpreter work between builtins) and `gpt_flatten` running
  per window are the next large rows; `parmap` efficiency is ~57 % at B8.
- Behaviour changes: matrix products of real operands can differ in the
  last digits (BLAS-style reordering); a real `Inf` operand in `A * B` no
  longer yields `NaN` imaginary parts. Elementwise, indexing, range and
  reduction changes are bit-identical.
- `prim.scalar_loop_1e5` is ~18 % faster than 0.3.9 (direct path for real
  scalar `+ - * /`); `prim.transpose_129x512` reads 0.89× (54 vs ~48 µs),
  close to the noise floor at that size.

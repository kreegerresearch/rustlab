# Perf flywheel iteration 01

**Status:** in progress
**Current phase:** R2 (profiler)
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

**Status:** not started

Call-stack profiler with inclusive and self time; `[matmul]` pseudo-row;
`--profile=ops` for elementwise and index rows; parmap worker stats merged;
fix the missing exit report for in-script `profile()` under `rustlab run`.

## R4: real fast paths for elementwise ops and reductions

**Status:** not started

Borrow `Expr::Var` operands in `BinOp`, reuse owned temporaries, `Zip` /
`mapv_into` kernels, real-only kernels where they cut arithmetic, fast
slicing / reshape / row reduction, `gelu_grad(x)` builtin. The ≤ 5 ms
AdamW and ≥ 1,500 M elements/s axpy targets probably need real-valued
storage; report actual numbers.

## Response note (filled in at the end)

# Handoff: perf flywheel iteration 01 (2026-10-18)

State of the work, for an agent picking it up. The detailed plan, per-item
results, and the response note for `rustlab_llm` are in
[`perf_flywheel_iter01.md`](perf_flywheel_iter01.md). Read that first; this
page covers where things stand and what to do next.

## Where it stands

- **Branch:** `feature/perf-iter01`, pushed to origin.
- **PR:** https://github.com/kreegerresearch/rustlab/pull/77 (open, base `main`). Only the user merges PRs. Never merge it yourself.
- **Version:** 0.3.10, bumped in the first commit (`Cargo.toml` + `Cargo.lock`).
- **Commits on the branch, oldest first:**

  | Commit | Request | Summary |
  |---|---|---|
  | `df2d756` | R3 | `parmap` workers start without the caller's variables; `map_init` worker reuse; parallel row packing; `S.field` borrows the struct. Version bump. |
  | `28c1e78` | R1 | `rustlab_core::matmul`: real operands use `f64` dgemm |
  | `f1f6acb` | R2 | Call-stack profiler: inclusive and self time, `[top-level]`, `[matmul]`, `--profile=ops`, `parmap:` rows, exit report |
  | `322c7bc` | R4 | `eval/elementwise.rs` (borrowed operands, in-place temporaries, parallel kernels); borrowed indexing; block slices; integer range fill; row-wise `sum(M,1)`; moved function outputs; `gelu_grad` builtin |

- **This handoff file is not committed.** The user has not yet approved a commit for it.

## Results (from `perf-check`, gate PASS every run, trajectory exact)

| Key | 0.3.9 | Now |
|---|---:|---:|
| `e2e.train_step_B8` | 2,246 tok/s | 7,967–8,269 tok/s |
| `model.grad_flat` | 152 ms | 49–50 ms |
| `par.grad_flat_B8` | 322 ms | 86–89 ms |
| `prim.matmul_512x512x512` | 8.6 GFLOP/s | 50.8 GFLOP/s |
| `flat.adamw_811k` | 19.5 ms | 4.6–5.0 ms |
| `flat.batch_reduce_8x811k` | 29.4 ms | 2.7 ms |
| `flat.axpy_811k` | 457 Melem/s | 2,843–3,111 Melem/s |
| `flat.unflatten_811k` | 20.5 ms | 18 ms (target missed; library cause, below) |

Targets not met:
- **R3: `par.grad_flat_B8` ≤ 1.3 × serial.** It is at 1.75×. What remains is the per-task copy of the lambda's captured values, plus user-function argument copies. Both need copy-on-write arrays, which is roadmap C11.
- **R4: `flat.unflatten_811k` ≤ 5 ms.** `gpt_unflatten` in `../rustlab_llm/lib/gpt.rlab` recomputes `gpt_sinusoidal_pe` (a 16,512-iteration scalar loop, ~19 ms) on every call. That is a library change, and rustlab never edits `rustlab_llm`.

Declined: the ≥ 100 GFLOP/s Accelerate/BLAS target. Rule 10 forbids FFI for core numerics.

## What is left to do

1. **PR review / CI on #77.** Address any review comments on the branch with new commits. Never force-push.
2. **Not yet run from the pre-release checklist:**
   - `cargo build/test --workspace --features viewer`;
   - `cargo clippy --workspace --features viewer`, which is advisory;
   - `make perf`, twice. This writes `perf/report.md`, and AGENTS.md then requires an `## AI Analysis` section in that file. Plan for that.
   - Do **not** run `cargo fmt` on the repo; see Gotchas.
3. **After the user merges and installs** (`make install`):
   - Fill in the response note's `Revision:` line in `perf_flywheel_iter01.md` with the merged SHA.
   - Hand the note to `rustlab_llm` by giving the user its text or path. The note already lists everything they need:
     - the new eight-column profile regex for `perf/run.py`;
     - adopting `gelu_grad`;
     - fixing the positional-encoding recompute;
     - the behavior changes.
   - Then `rustlab_llm` runs `make perf LABEL=iter01` on their side.
4. **Close out the plan.** Move `perf_flywheel_iter01.md` to `dev/plans/closed/`, update its row in AGENTS.md "Active Plans" and the C11 row in `dev/plans/roadmap.md`.
5. **Iteration 02** waits for `rustlab_llm`'s next request file. Likely candidates, from the nested profile:
   - interpreter self time in `gpt_forward` / `gpt_backward`, and the copies of builtin and user-function arguments (both C11);
   - parmap efficiency (~57 % at B8);
   - real-valued storage, which the request file deferred.
   - The scalar-loop regression is fixed (now ~18 % faster than 0.3.9). The transpose flag was baseline noise (`main` and the branch both ran `H'` in ~53 µs); transpose now borrows its operand and writes row-major output in 32 × 32 tiles, 26 µs (1.82×). Binary growth was cut from +596 KB to +149 KB (+1.3 %) by routing all elementwise kernels through two non-generic `&dyn` drivers (see `perf/report.md`, second addendum). Keep new parallel loops on `elementwise::drive` / `drive_new`: every distinct closure handed to rayon costs ~10–20 KB of rayon machinery.

## How to measure

```sh
cargo build --release -p rustlab-cli
RUSTLAB=$PWD/target/release/rustlab make -C ../rustlab_llm perf-check   # ~15 s, records nothing
target/release/rustlab run --profile=ops some.rlab                     # op-level profile
```

Run `perf-check` twice before believing a change under 10 %; the harness noise is about 1.5 % median, 10 % worst. For a micro-benchmark against the old release, `~/.local/bin/rustlab` is still the installed 0.3.9 binary.

## Gotchas met in this session

- **Sandbox:** `git fetch/push` and `gh` need the sandbox off (the SSH proxy refuses the connection). So do the notebook server tests, which bind ports.
- **`rustlab-notebook` tests fail when `TMPDIR` is a symlink** (5 listing tests, `StripPrefixError`). That is environmental. Run the workspace tests with a canonical temp dir:
  ```sh
  mkdir -p target/tmp-canon && TMPDIR="$(cd target/tmp-canon && pwd -P)" cargo test --workspace --no-fail-fast
  ```
- **Every test run writes `examples/plot/assets/plotly.min.js`** (a side effect of #76). Delete it before staging; never commit it.
- **Don't run `cargo fmt`:** the local rustfmt reformats unrelated files (see memory).
- **Stack depth:** growing `Evaluator::eval_expr`'s locals overflowed the 2 MB test-thread stack in a recursive debug test. Put new heavy branches in `#[inline(never)]` helpers, as `eval_var_index` and `eval_arith_binop` are.
- **Elementwise fast paths must stay bit-identical to `Value::binop`.** That includes the real-in → real-out imaginary zeroing, whose conditions differ for Vector (only `.* ./ .^`) and Matrix (all ops). Extend `elementwise::tests::check` for any new case.
- **Other agents may push to PR branches.** Fetch before pushing to `feature/perf-iter01`, and integrate any new commits with a merge (never force-push).
- **Commits and PRs carry no AI attribution trailer or footer** (user rule). The user approves every commit, push, and PR; they merge PRs themselves.

# Perf flywheel iteration 03 (`rustlab_llm` requests R1–R4)

**Status:** in progress on `feature/perf-flywheel-iter03` (worktree
`../rustlab-iter03`). Approved 2026-10-23 as one PR for the whole
iteration, with the `run` fix included ("Fix the bug along with this
iteration"), like iterations 01 and 02. Version 0.3.17.
**Current phase:** complete (phases 0–3); phase 4 measured and left open (see the outcome table).
**Source request:** `../rustlab_llm/perf/iterations/03-requests.md`
(revised 2026-10-23), measured on rustlab 0.3.11 and 0.3.16.
**Goal set by the request:** 26,000 tok/s on `e2e.train_step_B8` with the
gate passing (today 18,660–19,310). Their long-term target is 35,000, parity
with PyTorch `float64` on the same CPU.
**Self-check:** `RUSTLAB=$PWD/target/release/rustlab make -C ../rustlab_llm perf-check`.
rustlab never edits `../rustlab_llm`; the response is a note plus an
installed binary.

## Evaluation of the requests

| # | Request | Verdict | Reason |
|---|---|---|---|
| R1 | `float64` matrix multiply on the SME matrix unit | **Do it, hand-rolled** (phase 2) | Largest lever (145 of 322 worker CPU ms). Fits Rule 10 with no FFI: SME runs from stable `asm!`. Probed below. |
| R1 alt | Accelerate behind a feature flag | **Decline** | FFI into a closed C framework for core numerics is a Rule 10 hard limit, as in iteration 01. |
| R2 | Real-valued storage, first slice | **Do it, as already planned** (phase 3) | `dev/plans/real_storage.md` steps 0–6, unchanged. Biggest change in this plan. |
| R3 | Allocation that does not slow down under `parmap` | **Measure after phase 3, then decide** (phase 4) | Real storage halves the sizes first. The allocator is infrastructure, so a crate is allowed with a trade-off study, which needs your call. |
| R4 | Serial optimizer tail | **`sum` now (phase 1), the rest after phase 3** | `sum` is a cheap, confirmed fix. AdamW and unflatten are mostly bytes, which phase 3 halves. |
| opt. | Implicit expansion as fast as same-shape | **Fold into phase 3** | The `f64` elementwise drivers are rewritten there anyway; one row kernel and one column kernel. |

### What was checked (2026-10-23, this machine)

- **SME works from stable Rust 1.95.** A probe using `smstart`, `fmopa za0.d`, `st1d` from ZA, and `smstop` in one `asm!` block produced the right 8 × 8 outer product. The assembler needs `.arch_extension sme` and `.arch_extension sme-f64f64` at the top of the block. No crate and no `target_feature` attribute are involved.
- **The hardware ceiling, register-only `fmopa` loop:**

  | Threads | GFLOP/s, all threads together |
  |---:|---:|
  | 1 | 483–496 |
  | 2 | 826 |
  | 4 | 1,020 |
  | 8 | 1,038 |

  - The M4 Max has two units, so 8 `parmap` workers share about 1 TFLOP/s, roughly 130 GFLOP/s each. That is still 2.5× today's 42–56 per core on the vector units.
  - The request's single-call target of 400 GFLOP/s at 512³ is about 80 % of one unit's peak. That is ambitious for a first kernel; 250–350 is the realistic first-cut range.
  - The model's per-step products are about 6.5 GFLOP. At an effective 600 GFLOP/s across all workers they cost about 11 ms of CPU, against 145 today.
- **Detection.** `is_aarch64_feature_detected!("sme")` is still unstable (`stdarch_aarch64_feature_detection`). Detection will read `hw.optional.arm.FEAT_SME_F64F64` through `sysctlbyname` on macOS, cached once. That is a libc call for feature detection, which is infrastructure. Linux aarch64 (`getauxval(AT_HWCAP2) & HWCAP2_SME_F64F64`) can follow; until then other targets keep today's path.
- **`sum` is slow for two reasons.** `dispatch_matrix_reduction` copies the whole vector into a `Vec<C64>` before reducing, which is 13 MB at 811 k elements. It then adds in one sequential chain (`iter().sum()`), so each add waits on the previous one. Fixing both explains the gap between `sum` at 1.12 ms and `norm` at 0.44 ms.

## Phases

The whole iteration ships as one PR off `main` at 0.3.17 (the open notebook-diff branch holds 0.3.16). The order puts the low-risk, high-value items first.

### Phase 0: `run` with a relative directory path (blocking bug)

**Status:** complete. `exec_run_path` makes the path absolute (`std::path::absolute`) before changing directory. The test `run_resolves_relative_paths_from_the_calling_directory` covers the seven path forms plus nested `run`, and fails on the old code (`sub/x.rlab`). rustlab_llm's `tutorial/sherlock/doctor_probe.rlab` fails on 0.3.16 and prints 4656 on the new build.

The request file's revision of 2026-10-23 14:09 added this bug, which has been there since 0.3.14. It stops their training workload from running at all: `make sherlock-quick`, and 45 of 106 lesson scripts.

- **Cause, confirmed in `eval/line_io.rs::exec_run_path`.** It changes into `safe.parent()`, then reads `safe`, which is still relative, from the new directory.
- **What fails.** Any relative path with a directory part, not only `../../`. For example, `run "sub/x.rlab"` opens `sub/sub/x.rlab`. `../lib/x.rlab` works only by coincidence, because `lib/../lib` resolves back to the same folder.
- **Why tests missed it.** The one test passes an absolute path.
- **Fix.** Read the source before changing directory. Also make the path absolute first, so error messages and the directory change agree.
- **Tests.** A temp tree with `run` statements for these paths:
  - `"y.rlab"`
  - `"sub/x.rlab"`
  - `"../lib/x.rlab"`
  - `"../../lib/x.rlab"`
  - `"a/b/../../lib/x.rlab"`
  - an absolute path
  - nested `run` from inside a `run`

  Each case must also show that the working directory is restored afterwards. These tests touch the process working directory, so they go through the existing serialized cwd test helper, if there is one, or run in their own test binary.
- **Files:** `rustlab-script/src/eval/line_io.rs`, `CHANGELOG.md` (Fixed).
- **Version:** 0.3.17, a small PR that can land ahead of everything else.
- **Acceptance:** `make -C ../rustlab_llm sherlock-quick` and `make -C ../rustlab_llm lesson-02` run on the new build.

### Phase 1: fast `sum` (R4, part)

**Status:** complete. `elementwise::sum_c64` uses 4,096-element chunks with 16 `f64` lanes, and the partials are added in chunk order (rayon from `PAR_MIN_LEN`). `dispatch_matrix_reduction` passes the borrowed slice when contiguous. `sum` and `mean` of 811 k elements: 1.75 → 0.09 ms; `flat.sumsq_811k` 1.42–1.54 → 0.28 ms; `flat.clip_811k` → 0.42 ms. Tests cover integer exactness, thread-count independence (1 and 8 threads, bit for bit) and the error bound against Kahan.

- **`sum` without the copy.** It reduces the borrowed slice, and the same goes for `mean` and `prod`, which share `dispatch_matrix_reduction`.
- **The reduction order:**
  - Fixed 8-lane accumulators within fixed 4,096-element chunks, so the inner loop vectorizes.
  - Chunk partials are combined in chunk order.
  - Chunks run in parallel (rayon) from 131,072 elements, the existing `PAR_MIN_LEN`.
  - The result is the same on every run and every thread count, because the chunks are fixed.
- **`sum(g .^ 2)`** gets faster through `sum` alone. A fused `sumsq` pattern match is not planned unless the targets miss.
- **Behavior change.** Sums of more than 8 elements round differently from 0.3.16, in the last digits. This goes in the CHANGELOG under Breaking, with migration guidance: exact-integer sums are unchanged, and floating-point results may move by round-off.
- **Files:**
  - `rustlab-script/src/eval/builtins.rs` (`dispatch_matrix_reduction`, `builtin_sum`, `builtin_mean`)
  - `eval/elementwise.rs` (a `par_sum` driver next to `par_column_sums`)
- **Tests:**
  - Integer-valued vectors sum exactly.
  - The result is identical across `RAYON_NUM_THREADS` 1, 4 and 14.
  - Relative error against a compensated (Kahan) reference stays within `n·eps`.
  - Empty, NaN and Inf inputs.
  - `sum` of complex inputs.
- **Targets:** `flat.sumsq_811k` ≤ 0.4 ms, `flat.clip_811k` ≤ 0.6 ms.

### Phase 2: SME `float64` matrix multiply (R1)

**Status:** complete (first cut). The code is `rustlab-core/src/matmul/sme.rs`.
- **Blocks.** 16 × 16 blocks in four ZA tiles. Panels are packed straight from the complex storage with raw strides, and the loop order is chosen by stride (a 4 KB stride mapped every read to one L1 set and was 4.6× slower). Per-thread scratch buffers are reused and never cleared.
- **Wider blocks gained nothing.** 16 × 32 blocks using all eight tiles, and A-panel-outer order, both measured the same, so the simpler kernel stayed.
- **Threshold.** `SME_MIN_MACS` = 16,384: 16³ is 13 % slower on SME, 32³ 28 % faster.
- **Rust-level rates.** 512³ about 300 GFLOP/s, 129×128×512 about 240, 129×128×128 about 178. Inside the kernel alone, small shapes run at 440–460 GFLOP/s and 512³ at about 260. 512³ looks bound by L2 bandwidth to the matrix unit.
- **Tests.** Bit-identical to a `mul_add` k-order reference on 605 shapes, plus strided and transposed operands, NaN and Inf, exact integers, empty dimensions, and 8 concurrent threads.
- **perf-check on phases 0–2** (2026-10-23, 0.3.16 run back to back under the same background load): `e2e.train_step_B8` 53.9 → 43.7 ms (23,140 tok/s) and `parmap:[matmul]` 145 → 64 ms. Gate PASS, trajectory rel diff 1.42e-15. At script level `prim.matmul_129x128x512` is 134 GFLOP/s and `prim.matmul_512x512x512` 255, below the 200 / 400 targets; the gap to the Rust-level rate is widening the output to complex and allocating it, which phase 3 removes.

- **Where the code goes.** The new file is `rustlab-core/src/matmul/sme.rs`, and the current `matmul.rs` becomes `matmul/mod.rs`. `matmul_conj`'s real path calls the SME kernel when all of these hold:
  - The host has `FEAT_SME_F64F64`.
  - `m·n·k` is above a measured threshold. The fixed `smstart` / `smstop` cost decides that threshold; the attention-score product 129 × 32 × 129 is the smallest shape that matters.
  - `RUSTLAB_NO_SME` is unset. It is a kill switch for differential testing and bug reports.

  Otherwise `matmul_conj` calls `matrixmultiply`, as today.
- **The kernel:**
  - **Packing.** A is packed into 8-row panels and B into 8-column panels. Packing reads any strides, so the transposed views from 0.3.11 cost nothing extra.
  - **Accumulation.** A 16 × 16 block of C is accumulated in 4 ZA tiles over the full k loop, with one `fmopa` per tile per k. It is stored once, scaled by the existing contents only when β ≠ 0, which is never today.
  - **Edges.** Edges in m and n (129 is not a multiple of 8) use `whilelt` predicates instead of padding the output.
  - **Streaming mode.** All streaming-mode code sits inside one `asm!` block per micro-tile row. The clobbers are z0–z31, p0–p15, ZA and v8–v15 (`smstart` zeroes them), plus `clobber_abi("C")`.
- **Rounding.** Each element is accumulated in k order with fused multiply-add. The last digits differ from `matrixmultiply`. CHANGELOG Breaking, the response note reports the trajectory move, and anything above 1e-10 relative gets explained.
- **Complex products stay on the existing path.** The workload is real; a complex SME kernel waits for evidence.
- **Files:**
  - `rustlab-core/src/matmul/{mod,sme}.rs`
  - `rustlab-core/Cargo.toml`: `libc` for `sysctlbyname`, if it is not already in the tree for this target
  - `perf/` gets a matmul bench row
- **Tests:**
  - **Shapes.** Every shape `m, n, k` in {1, 7, 8, 9, 31, 32, 33, 129, 512} is checked against `matrixmultiply`, with relative error ≤ `k·eps·max|a||b|`.
  - **Exactness.** Products of small-integer matrices are bit-identical (exact in both kernels).
  - **Operand layouts.** Transposed and strided operands, empty dimensions, NaN and Inf propagation.
  - **Deterministic.** The same inputs give bit-identical output across repeated calls and from 8 `parmap` threads at once.
  - **Other hosts.** On a host without SME the tests exercise the fallback and the detection returns false.
  - **Gate.** `perf-check` must pass.
- **Targets:**
  - `prim.matmul_512x512x512` ≥ 400 GFLOP/s, stretch; first cut 250+.
  - `prim.matmul_129x128x512` ≥ 200.
  - `prim.matmul_A_Bt_129x128x128` ≥ 150.
  - `parmap:[matmul]` ≤ 30 ms per 8 windows.
- **Risks:**
  - **Tile layout is new code.** The kernel is the riskiest code in this plan; it needs a careful review of the tile layout and edge predicates.
  - **Hardware support.** macOS is the only host with SME in hand. Linux SME hardware is not available for testing, so Linux stays on the fallback until it is.
  - **Shared units.** The two SME units are shared, so 8 workers queue on them. If `par.grad_flat_B8` gains less than the single-call numbers suggest, the request's "batched step" prototype on their side is the follow-up.

### Phase 3: real-valued storage, first slice (R2, plus optional expansion)

**Status:** complete. As built:
- **Storage layer.** `eval/shared.rs` gained real storage inside `Shared` with a cached widening for `Deref`, real constructors on `Value`, non-widening shape helpers, the `[widen]` / `[promote]` profile rows, and `RUSTLAB_COMPLEX_STORAGE`.
- **Producers.** `zeros`, `ones`, `rand` and `randn` build real storage.
- **Operators.** `eval/real_ops.rs` covers operators, broadcasts, `*` and comparisons, with in-place reuse of owned temporaries.
- **Indexing.** `eval/real_index.rs` handles reads. Writes go through `try_real_index_assign`, which also accepts complex sources with all-`+0.0` imaginary parts.
- **Builtins.** `reshape`, transpose, one-row `[…]`, `sum` and `mean`, the `apply_scalar_fn_*` family, `softmax`, `outer`, and `parmap` packing are converted. Large copies and column sums run in parallel, and new buffers are written once with no zero-fill.
- **Not converted.** Tensor3 kernels and the less common builtins; they stay correct through widening.
- **New rule.** Array ∘ real scalar and outer products of real inputs now follow the real-output rule, so both storages give the same values. This is a CHANGELOG Breaking entry.
- **Tests.**
  - Every operator and shape is checked against the complex path.
  - Every read form is checked.
  - In-place reuse is checked by pointer.
  - `sum_f64` equals the real part of `sum_c64`.
  - `softmax_slice_f64` equals the complex kernel.
  - The full suite passes in both modes.
  - `crates/rustlab-cli/tests/storage_modes.rs` runs every example in both modes: 70 byte-identical, and 9 skipped because they use unseeded `rand`.

- **Scope.** Exactly `dev/plans/real_storage.md` § "The first slice", steps 0–6, with that plan's tests and its bit-identical target. The target applies against a phase 1 + 2 build, since those phases change rounding on their own.
- **With phase 2 in.** Real-stored operands go straight into the SME kernel, with no `real_parts` copy.
- **The optional expansion item.** The `f64` elementwise drivers get a row-broadcast and a column-broadcast kernel that walk the full operand contiguously. Results stay bit-identical to the same-shape operation.
  - **Target:** `prim.expand_row_129x128`, `prim.expand_col_129x128` ≤ `prim.ewise_same_129x128`.
- **Version:** one bump at the end. The slice lands in several commits on one branch, and `perf-check` runs after each step.
- **Targets:** `parmap` out ≈ 50,688 KB, no `[widen]` row above 1 ms, `par.grad_flat_B8` ≤ 1.4 × `model.grad_flat`.

### Phase 4: allocation under `parmap` (R3) and the rest of the tail (R4)

**Status:** measured; no further change in this PR.
- **Allocation.** Real storage plus write-once buffers met R3's profile target (`parmap:zeros` 24–26 → 1.8 ms) and halved `par.alloc_window_B8` (1.47 → 0.75 ms; target 0.6). No allocator dependency is proposed.
- **AdamW.** 2.70 → 1.79–1.91 ms. It is about 14 elementwise passes, each near memory bandwidth; the 1.5 ms target needs expression fusion or a cache of freed buffers. Each is its own design.
- **Unflatten.** 1.63 → 1.10 ms. The 0.8 ms target needs range slices that share the parent buffer, a copy-on-write design change.

- **Re-measure first** `par.alloc_window_B8`, `parmap:zeros`, `flat.adamw_811k` and `flat.unflatten_811k` on the phase 3 build.
- **If allocation still misses its target** (`par.alloc_window_B8` ≤ 0.6 ms), there are two options:
  - (a) `mimalloc`, MIT. The iteration 02 note measured +18 % end to end. An allocator is infrastructure, but the dependency still gets `dev/plans/mimalloc-tradeoff.md` and your decision.
  - (b) A hand-rolled per-thread cache of large freed array buffers, in `eval/shared.rs`. No dependency, but subtle around memory growth.

  The recommendation is written once the numbers are in.
- **If AdamW or unflatten still miss:**
  - AdamW: confirm that each chained operator reuses its uniquely owned temporary.
  - Unflatten: let a contiguous range slice `v(i:j)` share its parent's buffer as an offset view in `Shared`. That is a design change to copy-on-write, so it gets a short design section of its own before any code.

## Deferred (agreed with the request)

`f32` mode (after phase 3, needs its own gate tolerance), batched multiply on `Tensor3` (after their batched-step prototype), fused attention, threaded dgemm (superseded by SME, whose units do not scale with threads).

## Decisions (Michael, 2026-10-24)

1. Phase order 0 → 1 → 2 → 3 kept, shipped as **one PR** at 0.3.17 (`run` fix included).
2. Accelerate feature flag **declined** (Rule 10 hard limit on FFI in core numerics).
3. Allocation and AdamW for iteration 04: **hand-rolled first** (per-thread freed-buffer cache, AdamW fusion). A `mimalloc` trade-off study only if that falls short.
4. All three behavior changes **accepted**: fixed-order `sum` / `mean`, matrix-unit rounding (on by default, `RUSTLAB_NO_SME=1` kill switch), and the real-output rule for array ∘ real scalar and outer products. Binary size +167 KB accepted.
5. 0.3.17 installed for `rustlab_llm` before the merges.

## Outcome (2026-10-23, perf-check, M4 Max, same background load as 0.3.16 run back to back)

| # | Target | 0.3.16 | 0.3.17 | Status |
|---|---|---:|---:|---|
| Goal | 26,000 tok/s (`e2e.train_step_B8`) | 55.8 ms, ≈ 18,400 tok/s | 28.8–29.1 ms, 35,210–35,610 tok/s | met; at PyTorch `float64` 10-thread parity (34,300–37,200) |
| Gate | PASS | PASS | PASS, trajectory rel diff 1.42e-15 | met |
| Bug | `run "../../lib/x.rlab"` | 45 of 106 lessons fail | 106 of 106 run | fixed |
| R1 | `prim.matmul_512x512x512` ≥ 400 GFLOP/s | 57 | 296–387 | close |
| R1 | `prim.matmul_129x128x512` ≥ 200 | 51 | 178–231 | met in the better run |
| R1 | `prim.matmul_A_Bt_129x128x128` ≥ 150 | 47 | 135–140 | close |
| R1 | `prim.attn_scores_129x32x129` faster | 0.028 ms | 0.011–0.015 ms | met |
| R1 | `parmap:[matmul]` ≤ 30 ms / 8 windows | 145 | 49–52 | not met: the two SME units are shared by 8 workers |
| R2 | `parmap` out ≈ 50,688 KB | 101,376 | 50,688 | met |
| R2 | no `[widen]` row above 1 ms | — | 0.8 ms (`parmap:[widen]`) | met |
| R2 | `par.grad_flat_B8` ≤ 1.4 × `model.grad_flat` | 42.5 / 27.3 = 1.56× | 23.6–24.2 / 10.3–10.5 = 2.3× | not met as a ratio: the serial window got 2.6× faster and the parallel one 1.8× |
| R3 | `par.alloc_window_B8` ≤ 0.6 ms | 1.47 | 0.75 | close |
| R3 | `parmap:zeros` self ≤ 8 ms | 24–26 | 1.8 | met |
| R4 | `flat.sumsq_811k` ≤ 0.4 ms | 1.30 | 0.23–0.24 | met |
| R4 | `flat.clip_811k` ≤ 0.6 ms | 1.60 | 0.31–0.32 | met |
| R4 | `flat.adamw_811k` ≤ 1.5 ms | 2.70 | 1.79–1.91 | not met (fusion) |
| R4 | `flat.unflatten_811k` ≤ 0.8 ms | 1.63 | 1.10–1.11 | not met (slice views) |
| opt. | `prim.expand_row/col_129x128` ≤ `prim.ewise_same_129x128` | 0.026 / 0.013 | 0.003–0.004 / 0.004 | met; the library can switch from `outer` to implicit expansion |

## Response note for `rustlab_llm` (iteration 03)

rustlab 0.3.17 (`feature/perf-flywheel-iter03`):
- **The `run` bug is fixed.** All 106 lesson scripts run, and `tutorial/sherlock/doctor_probe.rlab` passes.
- **Matrix products** run on the M4 matrix unit (in-tree `asm!`, no FFI). `RUSTLAB_NO_SME=1` gives the old kernel. It rounds differently from `matrixmultiply`; the trajectory moved 1.42e-15.
- **Real numbers are stored as real numbers.** This is bit-identical; the trajectory did not move beyond the SME change. `RUSTLAB_COMPLEX_STORAGE=1` gives complex storage.
- **New profile rows.** `[widen]` / `[promote]` show where real data still takes a complex detour.
- **`sum` / `mean`** are fast with a fixed order. That rounding is included in the 1.42e-15.
- **Ready for this repo's 03b items.** Implicit expansion (the optional item is met) and `sum(Gm, 1)`. The batched-step prototype is now worth measuring, since the per-window matrix products are bound by the two shared SME units.
- **Open for iteration 04:** AdamW fusion, unflatten slice views, the `parmap` ratio, real Tensor3, and SME detection on Linux.

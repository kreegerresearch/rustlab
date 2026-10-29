# MATLAB compatibility migration

**Status:** plan only — no code in this change
**Current phase:** not started
**Base:** `0.3.23` (`ef3a2d4`, core math audit). Workspace version `0.3.23` has shipped, so each phase that changes observable behavior bumps the version in the same PR (workflow rule 12).
**Date:** 2026-10-29

Break existing script behavior where that is what it takes to match MATLAB. This is pre-1.0. Do not take a performance regression on a hot path to buy a match that scripts do not need. Core math stays hand-rolled Rust. No new dependencies (workflow rule 10).

This plan starts from the "Core math audit (2026-10-28)" table and open item **#5** in [`octave_compat_divergences.md`](octave_compat_divergences.md), plus the operator and factorization gaps named below. It does not reopen the ten divergences that table already marks shipped (`eig` shapes, `sum`/`mean` along columns, `find`, deletion assignment, and the rest).

## How to read a row

Each item has the same six parts. "Proposed change" is the recommendation. Items in [Do not change](#do-not-change) are still spelled out, so a later reader can see the rejection and not relitigate it.

**Measure.** Before and after, on the same machine, two runs:

- `make perf` (release binary, `perf/bench_*.rlab`, thresholds in `perf/run_perf.sh`). A number over the threshold fails the phase.
- `rustlab run --profile=ops` on the small scripts named in the item. Watch wall time and the `[widen]`, `[promote]`, and `[matmul]` rows. A real-storage fast path that starts widening is a failure even when the clock looks fine.
- `cargo test --workspace` and, for anything Octave actually agrees with, one new case in `tests/octave/` (`rustlab_full.rlab`, `reference_full.m`, `compare_full.m`).

**Acceptance, unless the item says otherwise.**

- Elementwise work (`./`, `/` by a scalar, `sum`, a length-`1e5` `randn` that is still a vector): within 10% of the before run, and the same `[widen]` / `[promote]` counts.
- Square `linsolve` and sparse `spsolve` keep today's algorithm and today's timing (no new QR on those paths).
- `norm(A)` with one argument keeps today's Frobenius timing.

Where Octave and MATLAB disagree, do not lock the octave harness to the MATLAB answer. Put a hand-checked fixture in `crates/rustlab-script/src/math_audit.rs` and say so in the test.

Gallery markdown is generated. Sweep `examples/` and `examples/notebooks/`, then regenerate `gallery/`. Do not hand-edit gallery copies. Counts below are sources, not gallery mirrors.

---

## Phase order

Separate PRs. Do not start the next phase until the current one is green, including the perf check. Suggested branch names are illustrative.

| Phase | PR | Why this order |
|---|---|---|
| 1 | `std` / `var` `w`, empty → NaN; `median` on complex and empty; `sort` of complex and NaN; `trace` on a non-square | Small, localized, no new algorithm, no hot-path kernel change. Fixes wrong answers. |
| 2 | `zeros` / `ones` / `rand` / `randn` / `randi` single-arg shape (open item #5) | Highest-value break. Also the easiest way to OOM a bench. The sweep is the work. |
| 3 | `.\` only | New syntax. Reuse the `./` kernel. Nothing old changes. |
| 4 | Householder QR and `[Q, R, P] = qr(...)` | Building block for `\`. New surface, so no silent break. Memory policy is the risk. |
| 5 | `\` and matrix `/`, column RHS, rectangular solve | Depends on phase 4. Square `linsolve` stays Gaussian elimination. |
| 6 | Dense `chol` and dense `lu` with a permutation vector | Sparse `chol` / `lu` handles stay. Do not route sparse through these. |
| 7 | `polyfit` / `polyval` `[p, S, mu]`; `interp1` linear, nearest, pchip, makima, fill | New surface. No existing script changes. Numerics are real but not on the training path. |
| 8 | `freqz` grid includes Nyquist | Reopens a decision locked in 0.3.23. Same cost, different samples. Octave's `freqz` is the wrong oracle. |

`sort(M)` / `sort(M, dim)` (the leftover of octave item #11) is a follow-on after phase 1, not part of it. Column sort changes every 2-D `sort` from an error into a per-column result. Ship the complex key first.

---

## 1. `zeros(n)` and the other single-arg constructors

Open item #5. The audit left it open on purpose.

**Current.** `zeros(n)`, `ones(n)`, `rand(n)`, and `randn(n)` return a length-`n` `Value::Vector`. `size` reports `[1, n]`. `zeros(m, n)` and `zeros([m, n])` are matrices. `zeros(1, n)` is a 1×n **matrix**, not a vector (`language_v0_3_4.md` documents that, and plot/indexing tests depend on it). `eye(n)` is already n×n. `randi(imax)` is a scalar. `randi(imax, n)` and `randi([lo, hi], n)` are length-`n` vectors. `zeros(n, "int32")` is a 1×n integer array (`peel_int_class` + `m.unwrap_or(1)` in `builtin_zeros`).

**MATLAB.** `zeros(n)`, `ones(n)`, `rand(n)`, `randn(n)`, and `randi(imax, n)` are n×n. A row is `zeros(1, n)`. A column is `zeros(n, 1)`. `randi(imax)` stays a scalar. `eye(n)` already matches.

**Proposed change.** One integer argument (and `randi`'s length argument) allocates n×n. Two-argument `zeros(m, n)` is unchanged. Integer-class forms follow the same shape rule. `eye` stays as it is. No deprecation release: 0.3.23 has shipped, and this migration is allowed to break scripts. The old plan's warning cycle conflicts with that; see [Conflicts](#conflicts-with-other-plans).

The in-tree sweep is part of the same PR. A call that wanted a length-`n` sequence becomes `zeros(1, n)` / `randn(1, n)` / `randi(imax, 1, n)`, **after** the benchmark below. Do not rewrite `zeros(n)` to `zeros(1, n)` and assume the fast path came along. Today those are different types.

**Breaking impact.** Single-arg calls with no comma, counted on this tree (gallery excluded):

| Call | Calls | Files | Hot ones |
|---|---:|---:|---|
| `zeros(n)` | 62 | 17 | `examples/math/mandelbrot.rlab` (`zeros(nx)` three times per row, `nx = 800`, inside `parmap`), `examples/audio/spectrum_monitor.rlab`, `perf/bench_interpreter.rlab` `zeros(1000)`, notebooks `mandelbrot`, `waterfall`, `language_v0_3_4`, `viterbi_fec` |
| `ones(n)` | 17 | 9 | `examples/stats/stats.rlab` |
| `rand(n)` | 22 | 11 | notebooks and docs |
| `randn(n)` | 85 | 44 | `perf/bench_builtins.rlab` `randn(100000)`, `bench_convolve.rlab` `randn(48000)`, `bench_fft.rlab`, `bench_upfirdn.rlab`, `bench_interpreter.rlab` |
| `randi(..., n)` | a handful | `examples/math/random.rlab` (`randi(6, 5000)`), `examples/notebooks/seed.md` | same OOM class |

Plus rust strings in `crates/rustlab-script/src/tests.rs` and `math_audit.rs`, REPL help, `docs/functions.md`, `docs/quickref.md`, `AGENTS.md`. `randn(64, 64)` in `perf/bench_linalg.rlab` is already 2-D and stays.

**Performance.** The kernel is still one allocation. The cost is the shape.

- `randn(100000)` today is 1e5 doubles. As n×n it is 1e10 doubles. `bench_convolve.rlab`'s `randn(48000)` is about 18 GB. The PR edits those benches to `randn(1, N)` **before** anyone runs `make perf` on the new binary.
- `mandelbrot_row` would allocate three 800×800 matrices per row, 600 times. Rewrite to a length-`nx` row first.
- `zeros(1, n)` is a matrix today. `zeros(n)` is a vector, and the real-storage `.*` / `sum` kernels are written for that vector. Blindly sweeping to `zeros(1, n)` can add a copy or a `[widen]`.

Before rewriting call sites, time `a = zeros(n); a .* a` against `b = zeros(1, n); b .* b` at `n = 1e5` and `n = 1e6` with `--profile=ops`. Acceptance: the 1×n form is within 15% and records no `[widen]` or `[promote]` on real data. If it fails, teach `real_ops` to treat a contiguous real 1×n matrix as that vector (no copy) in this same phase, then sweep. Do not change `zeros(1, n)` into a `Value::Vector`. That type is documented and tested as a matrix (`language_v0_3_4.md`, indexing tests, `real_storage.md`'s `zeros(1, 3)` example).

**Tests.** `size(zeros(3))` is `[3, 3]`. `zeros(1, 3)` shape and type stay a 1×3 matrix. `zeros(3, 1)` is 3×1. `eye(3)` stays 3×3. `randn` distribution is unchanged, only the shape. `zeros(3, "int32")` is 3×3. Octave case `size(zeros(3))` and the same for `ones` / `eye`. A rust test that `randn(1, 4)` is still the matrix type it is today.

---

## 2. Vectors are rows, and the right-hand side of `\`

**Current.** A `Value::Vector` is stored as a 1-D buffer. `size` reports `[1, n]` (a row). `[1, 2, 3]` and `1:3` are vectors. `[1; 2; 3]` is a 3×1 matrix. `*` of two vectors errors and tells you to write `v'` or `dot`. `M * v` treats `v` as a row, so the usual "matrix times a column" is `M * v'`. `to_cmatrix_arg` (used by some linalg helpers) promotes a vector to n×1. `to_cmatrix_arg_row` promotes it to 1×n. `linsolve(A, b)` accepts a vector, a row, or a column via `to_cvector`, requires `A` square, and returns a `Value::Vector` (so `size` says `[1, n]`, not `[n, 1]`). There is no `\` operator.

**MATLAB.** There is no separate vector type. `[1, 2, 3]` is 1×3. `[1; 2; 3]` is 3×1. `A\b` needs `b` to have as many rows as `A`. A row of the right length is a dimension error. The solution of one right-hand side is n×1. Several columns in `b` are several solutions.

**Proposed change.** Do not merge `Vector` and `Matrix`. See [Do not change](#do-not-change). For the new `\` only:

- A `Value::Vector` on the right is one column. Read it in place. Do not clone into an n×1 matrix first.
- An explicit 1×n matrix on the right is a dimension error when `n != 1`, matching MATLAB. An n×1 matrix is one column. An n×k matrix is k right-hand sides.
- One solution is returned as an n×1 matrix, so `size` matches MATLAB. `k` solutions are n×k.
- `linsolve` grows the same rectangular and multi-column behavior, and the same n×1 shape for one column. That is a break for callers that use `size(linsolve(...))` or concatenate the result as a row. In-tree `linsolve` calls are about 14, mostly tests and docs. `spsolve` is a different function and stays on the sparse solver; its return packing stays a vector unless a profile says the n×1 shape is free.

**Breaking impact.** Every script that does `x = linsolve(A, b)` and then treats `x` as a row (`size`, `plot` of columns, concatenation with a row). Examples use `spsolve` far more than `linsolve` (about 20 `spsolve` calls in `examples/*.rlab` plus the PDE notebooks). Those stay. The new `\` will not accept a row matrix as a column; the README-level migration note is `A \ b` for a vector and `A \ b'` only when `b` is an explicit row matrix that the user already transposed the other way.

**Performance.** This is the risk in returning n×1.

The next line is often `y = x .* h` with `h` a vector (FIR, residual, time series). An n×1 matrix may miss `real_ops` on vectors and show up as `[widen]` or a copy. Acceptance for phase 5: on a real length-`1e6` residual, `x .* h` with `x` the n×1 solution and `h` a real vector is within 15% of the same multiply where both are vectors, and `[widen]` / `[promote]` stay 0. If that fails, special-case a contiguous real n×1 in `real_ops` the same way phase 2 special-cases a real 1×n. Do not widen.

Building the column view must be a shape flag on the existing buffer, not an allocate-and-copy of `b`.

**Tests.** `size(A \ b)` is `[n, 1]` for a vector `b`. `A \ row_matrix` errors when the row is not 1×1. `A \ B` with `B` n×2 returns n×2 and matches two separate solves. Octave agrees on the square single-RHS values; compare those. The shape (n×1 matrix versus Octave's column, which is the same shape) can go in the octave suite. Bit-exactness is not required; residual under the existing `T_EXACT` is.

---

## 3. `norm(A)` with no `p`

**Current.** `norm(v)` on a vector is the 2-norm. `norm(A)` on a matrix, dense or sparse, is Frobenius. `norm(A, 2)` is the largest singular value (`svd_full`: Gram matrix, then Hermitian eig). `norm(A, 1)` and `norm(A, Inf)` are the column and row sums. Sparse `norm(S, 2)` densifies and refuses past 512×512. The octave fixture writes `norm(matrix, 'fro')` on the Octave side and `norm(matrix)` on the rustlab side (`tests/octave/reference_full.m`, `rustlab_full.rlab`).

**MATLAB.** `norm(A)` for a matrix is the 2-norm. `norm(A, "fro")` is Frobenius. Vector `norm(v)` is already the 2-norm.

**Proposed change.** Leave the default. `norm(A, 2)` is the MATLAB default for anyone who wants it.

**Breaking impact.** None if we leave it. If we flipped it, these call sites change meaning and cost: `examples/pde/dielectric.rlab`, `laplacian.rlab`, `laplacian_bc.rlab`, `examples/notebooks/parallel_montecarlo.md` (`norm` of a tensor slice difference), `examples/linalg/eig.rlab` (residual `norm(A*V - V*D)`), and the matching notebooks. One-arg `norm` appears 73 times in 27 files; many are vectors, where 2-norm and Frobenius match. The matrix residuals are the ones that would move.

**Performance.** Frobenius is one pass, O(mn) or O(nnz). The 2-norm is a Gram matrix plus a Hermitian eigensolve, O(min(m, n)³) after an O(max(m, n) · min(m, n)²) product, and it squares the condition. `svd_full` is that path. Sparse spectral norm already exists and is capped because it densifies. Putting that on every unqualified `norm(A)` makes residual checks in the PDE examples and in `math_audit.rs` an SVD. `perf/bench` does not call `norm(A)`, so `make perf` would stay green while real notebooks got slower. That is the wrong trade.

**Tests.** Keep the current Frobenius fixtures, including the octave pair that already passes `'fro'` only on the Octave side. Add nothing that expects `norm(A) == norm(A, 2)` for a non-vector.

---

## 4. `[V, D] = eigs(...)` and `D`'s shape

**Current.** `eigs` always returns a tuple `(V, D)` with `D` a length-`k` vector. `D(k)` is the k-th eigenvalue. `eig`'s two-output `D` is already a diagonal matrix, with `"vector"` / `"matrix"` overrides. `eigs` is not nargout-aware: a bare `e = eigs(A, k)` still builds the tuple.

**MATLAB.** `[V, D] = eigs(A, k)` returns `D` as a k×k diagonal. `e = eigs(A, k)` returns the eigenvalues as a column. `diag(D)` recovers the vector.

**Proposed change.** Two outputs: `D` is k×k diagonal. One output: column of k eigenvalues, and skip packaging `V` for the caller (Lanczos still reorthogonalizes internally; do not invent a vector-free solver in this phase). Add the same `"vector"` flag `eig` has, so `D(k)` keeps working when someone asks for it. Default follows MATLAB.

**Breaking impact.** `examples/sparse/eigs.rlab` uses `length(D)`, `D(k)`, `real(D(k))`, `Dlm(1)`, `Dg(1)`. `D(1)` still hits the (1,1) entry. `D(k)` for `k > 1` becomes a structural zero under column-major linear indexing. That is the footgun the audit cited, and it is real: the example's plot titles would show λ = 0. Same pattern in `examples/notebooks/eigs.md`, `gallery/eigs.md` (regenerated), `math_audit.rs` (`D(1)` on the sparse path), `tests.rs`, `docs/functions.md`, REPL help. About 8 files, one of them a worked example. `length(D)` of a k×k matrix is still k. Update the example to `D(k, k)` or `diag(D)`.

**Performance.** `k` is the requested count, not the matrix order. The example asks for 4 and for 1. A k×k diagonal next to a Lanczos run on a 96×96 Laplacian is noise. Do not allocate an n×n diagonal of the original matrix. One-output mode should not clone `V` into a script value. Acceptance: `eigs` on that 12×8 Laplacian is within 10% of today (the factorization dominates). No change to Lanczos complexity.

**Tests.** Residual `A*V ≈ V*D` with diagonal `D`, in-process and against Octave if Octave's `eigs` returns a diagonal (it does in current Octave). One-output length and values match `diag` of the two-output form. `"vector"` round-trips today's `D(k)` indexing. A test that `D(2)` on the default diagonal is 0 documents the footgun so it does not get "fixed" later by accident.

---

## 5. `sort` and `median` of complex values, and NaNs

**Current.** `sort` orders by the real part. NaNs compare equal (`partial_cmp` → `Equal`), so they stay where the sort leaves ties. The real path uses `sort_unstable_by`. A 2-D matrix is an error. `median` orders by the real part and ignores the imaginary part. A NaN real part returns NaN (0.3.23). An empty input is an error. A 1×1 or a vector works.

**MATLAB.** Complex `sort` orders by magnitude, then by phase (angle). Real `sort` is numeric order, not magnitude (negatives stay below positives). NaNs go to the end. `sort` is stable on ties. `sort(M)` sorts each column; `sort(M, dim)` picks the axis. `median` of a complex array is an error. `median([])` is NaN.

**Proposed change.**

- Real data: keep numeric order. Move NaNs to the end.
- Any nonzero imaginary part: order by `norm`, then `atan2(im, re)`. The "any imaginary" scan already exists; do not pay `hypot` on a real vector.
- `median` of a value with a nonzero imaginary part errors. Pure-real storage, including a complex number whose imaginary part is exactly 0, stays on today's path. Empty `median` returns NaN instead of erroring.
- Do not land `sort(M, dim)` in this PR. It is a follow-on. The comparator has to be the one above when that lands.

Stable versus unstable is a performance question, not a numeric one for distinct keys. See below. Recommendation: stable only if the benchmark passes; otherwise document that ties keep today's unstable order and NaNs still move to the end (a linear partition, then the existing sort on the finite prefix).

**Breaking impact.** Scripts that `sort` a genuinely complex vector and expect real-part order. In-tree `sort(` shows up in docs, octave fixtures (real data), and tests; examples are few (`sort` is not a common example call). `median` of complex is unlikely in examples; `math_audit.rs` covers real medians. Empty `median` currently errors, so changing it to NaN breaks tests that expect the error, not user scripts that succeeded.

**Performance.** Real path: a NaN partition is O(n), then the same sort. Acceptance: `sort` of 1e6 and 1e7 real doubles (no NaNs) within 10% of today if we keep `sort_unstable`. If we switch to a stable sort, the ceiling is 2× on 1e6 doubles; over that, keep unstable and record the decision in the PR. Complex path is O(n log n) `hypot`+`atan2` compares instead of one `f64` compare. That path is not in `bench_builtins`. Do not route real data through it. No allocation beyond today's sorted output. No `[widen]` on a real vector: sort the `f64` slice, then write a real vector back.

**Tests.** Real vector with negatives and NaNs: NaNs at the end, negatives at the start, ties keep relative order only if we went stable. Complex: `sort([1+1j, -2, 1-1j])` matches magnitude-then-phase against a hand-computed expected vector (Octave's complex `sort` matches this; add the octave case). `median(1+1j)` errors. `median([])` is NaN. Real median fixtures stay bit-identical, including the even-length average.

---

## 6. `std` / `var` weight flag, and empty input

**Current.** `std` and `var` are the sample forms, divide by `n - 1` only. There is no `w`. Empty and length-1 both return 0 (`std_of_slice`). A matrix reduces down columns to a 1×n row, same as `mean`. Complex data uses `Σ|x − mean|² / (n − 1)`, which already matches MATLAB's complex `std`. `var` is `std²` in the same pass structure. 0.3.23 changelog: "There is no divide-by-n flag."

**MATLAB.** `std(x)` and `var(x)` default to `w = 0` (divide by `n - 1`). `std(x, 1)` and `var(x, 1)` divide by `n`. `std(x, w, dim)` selects the axis. `std([])` is NaN. A length-1 input is 0. A weight **vector** is a third form (`std(x, w)` with `w` the same length as `x`); leave that out of this phase.

**Proposed change.** Optional second argument `w` is 0 or 1. A vector in that slot is an error with a message that names the missing weight-vector form, not a silent sample variance. `dim` stays the argument after `w` when present (`std(M, w, dim)`). Empty input returns NaN (a 1×ncols or nrows×1 row of NaNs when reducing an empty matrix along an axis, not a scalar 0). Length 1 stays 0. Complex formula unchanged.

**Breaking impact.** `std(` appears about 33 times in 15 files, `var(` about 17 times in 5, mostly docs and tests. Sample-variance callers (`w` omitted) keep their numbers. Empty→0 callers flip to NaN; `math_audit.rs` and any test that asserts `std` of empty is 0 must move. Examples under `examples/stats/` use non-empty data.

**Performance.** The divisor is one scalar chosen before the loop. Do not add a second pass. Do not rewrite `std` onto a new real-storage kernel in this phase: `std_of_slice` already gathers `&[C64]`. Acceptance: `std` of a length-1e6 real vector within 10% of today. If a profile shows this gather as a `[widen]`, that is pre-existing; do not make it worse, and do not block the flag on a storage rewrite.

**Tests.** `std([1, 2, 3])` unchanged (sample). `std([1, 2, 3], 1)` is the population value, checked against Octave (`std(x, 1)`). `var` the same. `std([])` is NaN. `w = 2` and a weight vector error. Complex `std` fixture stays.

---

## 7. `trace` of a non-square matrix

**Current.** `trace` sums `min(nrows, ncols)` diagonal entries. `trace([1, 2, 3; 4, 5, 6])` is 6. A scalar passes through. A vector is a type error.

**MATLAB.** Non-square is an error. Square trace is the same sum.

**Proposed change.** Error when `nrows != ncols`. Square results unchanged. Scalar stays a scalar (it is 1×1 in every sense that matters here).

**Breaking impact.** The rectangular case is asserted in `math_audit.rs`. About 20 `trace(` mentions in 8 files, mostly docs and the square tests. No example script depends on the rectangular sum.

**Performance.** One compare before the existing sum. Acceptance: not a perf item. Do not allocate.

**Tests.** Square fixture unchanged, including the octave case if one exists. Rectangular call errors. The old `math_audit` assertion is deleted, not loosened.

---

## 8. `diag` of a length-1 vector

**Current.** `diag` of a length-1 vector or a scalar returns a scalar (`exact_scalar_value`). Longer vectors return a matrix. Extracting a diagonal returns a vector. `kron` and one-element `linsolve` also collapse 1×1 to a scalar.

**MATLAB.** `diag(5)` and `diag([5])` are 1×1 matrices. They print like a scalar. `size` is `[1, 1]`.

**Proposed change.** Leave it. A 1×1 matrix here would disagree with the collapse used by `kron` and `linsolve`, and it would push a scalar off the `f64` fast path (`Value::binop`'s scalar arm) onto a matrix kernel for `diag(v) + 1` when `v` has one entry.

**Breaking impact.** None if we leave it. The displayed number already matches.

**Performance.** The collapse is the cheap path. The MATLAB shape is a type change with no numeric benefit. Not worth a sweep of every 1-element result.

**Tests.** Keep `diag([5])` as a scalar. Document in the function help that a length-1 diagonal is a scalar.

---

## 9. `freqz` and Nyquist

**Current.** `n` points on `[0, Nyquist)`: `f_k = k · fs / (2n)`, Nyquist excluded. That is the first `n` bins of a length-`2n` DFT. A filter longer than `2n` is summed directly on that same grid (0.3.23). `freqz(h, 512, sr)` did not move in 0.3.23 when `2n` was already the FFT length. Return value is a 2×n matrix (frequencies, complex H), not MATLAB's two outputs `[H, w]`.

**MATLAB.** `freqz(b, a, n)` uses `linspace(0, π, n)`, which **includes** Nyquist. Spacing is `π / (n - 1)`, not `π / n`. `n = 1` is the single point `π` (`linspace(0, π, 1)` is the stop value).

**Octave's `freqz` matches today's half-open grid.** `tests/octave/compare.m` checks the Hz axis and the magnitude against Octave. A MATLAB grid fails that check. 0.3.23 changelog calls the half-open grid a fix.

**Proposed change.** Phase 8, not earlier. For `n > 1`, evaluate at `k · fs / (2(n - 1))` for `k = 0 .. n-1`, so the last point is Nyquist. The FFT length becomes `2(n - 1)`; take `n` bins. The long-filter direct sum uses those frequencies. `n = 1` returns Nyquist, matching `linspace`. Keep the 2×n matrix return. Do not switch the calling convention to `[H, f] = freqz(...)` in this phase (that is a second break, and `plotdb` reads row 0 / row 1).

**Breaking impact.** Every `freqz` call changes interior bins, not only the last one, because the spacing changes. About 62 calls in 25 source files: DSP examples (`lowpass`, `bandpass`, `firpm`, `kaiser_fir`, `fixed_point`), notebooks (`filter_analysis`, `firpm_quantization`, `template_interpolation`, `notebook_directives`), `docs/examples.md`, `docs/functions.md`, `README.md`. `freqz(h, 512, sr)` moves; 0.3.23 said that particular call was unchanged. Plots shift by less than a bin at the right edge and by a growing amount toward Nyquist.

**Performance.** Same `n` evaluations. FFT length drops by 2. Acceptance: `freqz` of a 64-tap filter at `n = 4096`, and of a filter longer than the FFT (direct sum), within 10% of today. No algorithmic change beyond the grid.

**Tests.** Replace the Octave oracle for this one function with a fixture: DC equals `sum(h)`, the last frequency equals `fs/2`, and `H` at the new grid matches a direct sum (already the long-filter path). Leave a comment in `compare.m` that Octave's `freqz` is not the reference. Update `docs/validate_octave_report.md` only if that file is still described as current; it is a 2026-04 snapshot, so a one-line pointer from this plan is enough.

---

## 10. Butterworth, one output

**Current.** `b = butterworth_lowpass(order, cutoff_hz, sr)` returns the numerator vector. `[b, a] = butterworth_lowpass(...)` returns both. Highpass is the same. Cutoff is in Hz, with a sample rate. There is no `butter` name and no normalized `Wn`.

**MATLAB.** `b = butter(n, Wn)` returns only `b`. `[b, a] = butter(n, Wn)` returns both. `Wn` is a fraction of Nyquist. `butter(n, Wn, "high")` selects the type.

**Proposed change.** Do not change the one-output value. It already matches MATLAB nargout. Do not rename to `butter(n, Wn)` in this migration: every caller passes Hz and `sr`, and a normalized frequency would silently design the wrong filter. An alias can wait until someone asks for the `Wn` form. Two-output is already shipped in 0.3.23.

**Breaking impact.** None.

**Performance.** None.

**Tests.** Keep the existing one-output and two-output tests. Add none that require `butter(...)`.

---

## 11. Sparse drop below `1e-15`

**Current.** `SPARSE_ZERO_TOL` in `crates/rustlab-core/src/types.rs` drops entries with `norm < 1e-15` on construction, `set`, arithmetic, and `from_dense`. `1e-16 * speye(n)` is an empty pattern. `1e-14 * speye(n)` is above the tolerance and, as of 0.3.23, factors. The audit recorded this as intentional.

**MATLAB.** `sparse` stores every nonzero, including `1e-16`. Only exact zeros are structural zeros.

**Proposed change.** Leave the tolerance on construction and on arithmetic.

An opt-in `sparse(..., "exact")` that drops only exact zeros is possible later. It is not in these phases. Default path must stay byte-identical.

**Breaking impact.** None if we leave it. If we removed the drop, `1e-16 * speye(n)` would become a factorization, and any subtraction that leaves roundoff would add fill. Laplacian `spsolve` (the PDE examples and `perf` sparse notes) depends on that pattern staying sparse.

**Performance.** Fill-in is not a constant factor. A residual subtract on an n×n grid can turn O(n) nonzeros into O(n²). AMD, sparse Cholesky, and sparse LU then dominate. `make perf` does not cover that grid; a removed drop could pass `make perf` and still slow `examples/pde/laplacian.rlab`. Acceptance for "we did not touch it": `nnz(1e-16 * speye(4))` stays 0, and a profile of `spsolve` on `laplacian_2d` matches the before run within 10%.

**Tests.** Keep the 0.3.23 factor-of-`1e-14 * speye` test and the empty-pattern test for `1e-16`.

---

## 12. `.\`

**Current.** `./` is elementwise right division (`BinOp::ElemDiv`) on the real and complex fast paths (`real_ops`, `elementwise::drive`). There is no `.\` token. `A .\ B` is a parse error.

**MATLAB.** `A .\ B` is elementwise `B ./ A`, with the same broadcasting.

**Proposed change.** Lex `.\` as one token, same as `./`. Implement it as elementwise division with the operands swapped inside the existing kernel. Do not form a reciprocal and multiply (two passes, one temporary). Do not clone either side. Scalar `a .\ b` is `b / a`.

**Breaking impact.** None. The token is new. Scripts that wrote `.\` as an error start to run.

**Performance.** Must be the `./` kernel. Acceptance: real length-1e7 `b .\ a` within 10% of `a ./ b`, zero `[widen]` / `[promote]`, no extra allocation proportional to length. Same `PAR_MIN_LEN` cutoff. Add the op to the existing elementwise bit-identical tests rather than a new driver.

**Tests.** ` [2, 4] .\ [2, 8] ` is `[1, 2]`. Broadcasting matches `./` with arguments swapped, including the real-storage cases in `tests/storage_modes.rs` if that harness walks operators by list (add `.\` to the list). Octave `.\` agrees; add one compare case.

---

## 13. `/` as `mrdivide`

**Current.** `BinOp::Div` on a scalar and an array is a scale (the same kernel as `./` for that pair). `M / 2` works. `2 / v` divides the scalar by each element. Matrix `/` matrix and vector `/` vector error and tell you to use `./` or `inv`. There is no solve.

**MATLAB.** `B / A` solves `X * A = B`.

- Array `/` scalar is a scale. Same as rustlab.
- Scalar `/` array is a dimension error unless the array is a column that makes `X * A = scalar` legal. It is **not** elementwise. Elementwise is `./`.
- Matrix `/` matrix and row `/` matrix are solves. `B/A = (A' \ B')'` with conjugate transpose.

**Proposed change.** Phase 5, using the QR solver from phase 4.

- Array or vector divided by a scalar stays on today's elementwise kernel. Detect a scalar (or 1×1) **before** any factor. This is the hot path (`x / fs`, `M / 2`).
- Two non-scalar arrays: solve `X * A = B`. Implement by running the same left-divide on `A'` and `B'` without writing both transposes out when a strided read can feed the solver. If the first cut does transpose, it must use the existing tiled `par_transpose`, and the PR notes the copy.
- Scalar `/` vector and scalar `/` matrix become dimension errors, matching MATLAB. That is the break. Elementwise stays `./`.

Do not send `/` by a scalar through `inv`.

**Breaking impact.** `2 / v` and `1 / M` with `/` change from elementwise to an error. A crude count of `/` that is not `./` is hundreds of hits in examples and docs, almost all scalar/scalar or array/scalar, which stay. The dangerous pattern is scalar-over-array. The phase searches `examples/`, `perf/`, notebooks, and tests for `/` whose right-hand side is not a scalar literal or a known scalar name, and fixes the ones that wanted `./`. `tests.rs` has explicit `BinOp::Div` cases that must be updated. Octave will flag any fixture we add; existing fixtures use `./` or scalar `/`.

**Performance.** Array/scalar must not allocate a factor. Acceptance: the `M / 2` path in a length-1e6 real vector (today's `Div` arm) within 5% and bit-identical. Matrix/matrix `/` for n = 512 is allowed to cost a solve; compare it to `A' \ B'` once `\` exists and keep it within 1.5× of that, so we do not grow a second algorithm. No `normal equations`.

**Tests.** `M / 2` bit-identical to today. `[1, 2] / 2` unchanged. `2 / [1, 2]` errors. A 1×n row divided by an n×n matrix matches `linsolve` on the transposed system, residual under `1e-9`. Hand-checked complex case: conjugate transpose, not plain transpose.

---

## 14. `[Q, R, P] = qr`

**Current.** No `qr`. `svd` and `rank` cover singular vectors. `dense_eig` has Householder reflectors for the Hessenberg form; they are not a public QR. `svd_full` goes through a Gram matrix and is the wrong tool for a QR (extra condition, extra flops).

**MATLAB.** `[Q, R] = qr(A)` is a full QR: `Q` is m×m, `R` is m×n. `[Q, R] = qr(A, 0)` or `"econ"` is the thin factor. `[Q, R, P] = qr(A)` column-pivots; `P` is a permutation **matrix**. `[Q, R, p] = qr(A, "vector")` returns a permutation vector. `A(:, p) = Q*R` for the vector form, or `A*P = Q*R` for the matrix form.

**Proposed change.** Hand-rolled Householder QR in `rustlab-core` (new module, complex Householder with an explicit phase, tested against a dense dot product). Store reflectors. Form `Q` only when the caller asks for it.

Default `[Q, R] = qr(A)` is the **economy** factor (`Q` is m×k, `k = min(m, n)`), not MATLAB's full `Q`. Full `Q` is `qr(A, "full")`. Reason: a tall 10⁵×10 matrix must not allocate a 10⁵×10⁵ `Q`. This is a deliberate mismatch. The help text says economy is the default and `"full"` is the MATLAB-default shape.

`[Q, R, p] = qr(A)` column-pivots and returns `p` as a permutation **vector** (length n, 1-based). `"matrix"` returns the n×n permutation matrix for people porting `A*P`. Vector is the default because it is O(n) to store and O(n) to apply, and phase 5's solver wants the vector. See open questions.

One output `R = qr(A)` returns `R` only, without forming `Q`.

**Breaking impact.** None. New name.

**Performance.** Unpivoted Householder is about `2mn² − 2n³/3` flops. Forming `Q` is extra; skip it for one output and inside `\`. Column pivoting adds an O(mn) column-norm update per column. That is the rank-revealing factor `\` needs for a rank-deficient system; do not also implement a second unpivoted solver unless a profile shows the pivot search dominating a tall least-squares case. Economy `Q` for m = 1e5, n = 10 is fine. Full `Q` at that size must error with a clear message if `m * m` would exceed a fixed cap (propose 4096×4096 for the formed full factor, not for the reflector storage). Reflectors for a virtual full QR can still exist; the cap is on the explicit `Q` matrix.

Do not call `svd_full`. Do not link a Fortran library.

**Tests.** `Q*R ≈ A` and `Q'*Q ≈ I` on a 5×3 and a 3×5, real and complex, residual under `1e-12`. Column pivot: `A(:, p)` matches `Q*R`, and `p` moves a zero column to the end on a constructed rank-deficient matrix. `"full"` on a 4×2 gives a 4×4 `Q`. One-output `R` matches the two-output `R`. Octave's economy `qr(A, 0)` is the compare case; Octave's default full `qr` is not.

---

## 15. `\` , including underdetermined systems

**Current.** No `\` token. Square solve is `linsolve` (Gaussian elimination, partial pivoting, relative pivot test from 0.3.23) or `spsolve` / `solve` on a factor. Non-square is an error. `pinv(A)*b` is the Moore-Penrose solution, which **is** minimum-norm.

**MATLAB.** `A\b` is `mldivide`.

- Square, full rank: the usual solve. MATLAB's LAPACK path is not bit-identical to our elimination. Keep our square solver.
- Overdetermined, full column rank: least squares. Not the normal equations.
- Underdetermined, full row rank: a **basic** solution from QR with column pivoting (at most `rank` nonzeros), **not** the minimum-norm solution. Minimum-norm is `pinv(A)*b`.
- Rank-deficient: a basic solution, with a warning. Tolerance is on the order of `max(m, n) * eps * norm(A)`, the same family as our `rank` cutoff (`max(m, n) · eps · σ_max`). Use the QR diagonal after pivoting, not a fresh SVD, inside `\`.
- Several columns on the right: solve each, sharing the factorization.

**Octave.** Underdetermined `\` is minimum-norm. A compare against Octave would **fail** a correct MATLAB basic solution, and would **pass** a min-norm solution that is not MATLAB. Do not use the octave harness as the oracle for the underdetermined case.

**Proposed change.** Phase 5.

- Lex `\`.
- Square dense: call the existing elimination (today's `linsolve`). Do not replace it with QR. Residuals and timings stay. Relative pivot test stays.
- Square sparse: call `spsolve`'s dispatch (Cholesky when it already would, else sparse LU). Do not densify.
- Rectangular dense: economy QR with column pivoting from phase 4, applied to `b` from the reflectors. No explicit `Q`. No `A' * A`.
- Underdetermined: that same pivoted QR, basic solution, matching MATLAB. Document that `pinv(A)*b` remains the minimum-norm answer. Do not implement Octave's min-norm `\` unless question 1 below is answered the other way.
- Rank-deficient: basic solution, `ScriptError` is wrong (MATLAB warns and returns). Print a one-line warning. Do not invent a new warning framework; `eprintln!` once per call is enough, same spirit as other numeric warnings.
- Multi-RHS: factor once, apply many.
- Right-hand side shape: section 2.

**Breaking impact.** New operator, so old scripts that did not parse `\` start to run. `linsolve` of a square system keeps its values. `linsolve` of a non-square system starts to succeed. `linsolve` return shape for one RHS becomes n×1 (section 2). `spsolve` unchanged.

**Performance.** Square dense and square sparse must show the same `--profile=ops` time as today on `perf/bench_linalg.rlab`'s 256×256 multiply-and-solve pattern and on `laplacian_2d` + `spsolve`. Rectangular QR is new work; time a 2000×50 least squares and a 50×200 underdetermined solve once and paste the numbers in the PR. No acceptance ceiling on the first measurement. Ceiling after that: no regression on the square benches. Applying reflectors is O(mn) per right-hand side after the factor, not a refactor per column.

**Tests.** Square: existing `linsolve` / octave cases unchanged (values). Overdetermined: residual orthogonal to the columns (`A' * (A*x - b)` small), compared to Octave's `\` which agrees with MATLAB on full-rank overdetermined problems. Underdetermined: a hand-built 2×4 system with a known basic solution (the pivoted QR one, written out in the test) and a **different** `pinv` solution, so the test fails if someone "fixes" `\` to be min-norm. Rank-deficient 3×3 with a zero row: finite `x`, warning path covered by capturing stderr in the test or by a core-level flag. Sparse square: existing `spsolve` tests still pass, and `sparse \ b` matches `spsolve` on one Laplacian.

---

## 16. Dense `chol` and `lu` with a permutation vector

**Current.** `chol(A)` and `lu(A)` require a sparse matrix and return a factor handle for `solve(F, b)`. That handle is the factor-once path (parameter sweeps, animations). A dense matrix is a type error. Ordering strings `"auto"`, `"identity"`, `"amd"` are the sparse API. There is no dense Cholesky and no returned permutation.

**MATLAB.** `R = chol(A)` is upper triangular, `A = R' * R`, and errors if `A` is not SPD. `[R, p] = chol(A)` does not error: `p = 0` on success, otherwise `p` is the failing pivot index. `chol(A, "lower")` returns `L`. `[L, U, P] = lu(A)` returns a permutation **matrix**, `P*A = L*U`. `[L, U, p] = lu(A, "vector")` returns a vector, `A(p, :) = L*U`.

**Proposed change.** Phase 6.

- Dense `chol(A)` returns upper-triangular `R` (a matrix). `[R, p] = chol(A)` matches MATLAB's non-erroring form. `"lower"` returns `L`.
- Dense `[L, U, p] = lu(A)` returns `p` as a **vector** by default (cheaper, and it is what callers apply). `"matrix"` returns `P`. One output on a dense matrix is an error (MATLAB's one-output `lu` is a packed factor we do not need to invent). Two outputs `[L, U]` use the same pivot and fold it into `L` the way MATLAB's two-output form does, **or** we require three outputs. Recommendation: support two and three. Two-output `L` is the permuted unit lower triangle, `L*U = P*A` is **not** the two-output contract; `L*U = A(p,:)` only when `p` is returned. Document this in one sentence in the help so nobody compares two-output `L` to Octave's three-output `L`.
- Sparse `chol` / `lu` keep returning the handle. A sparse matrix must not start returning a dense triangle. The handle is the point of those functions.

Dense Cholesky is the standard algorithm (Golub & Van Loan), real symmetric and complex Hermitian, in `rustlab-core`. Dense LU can share the elimination already in `linsolve` if the permutation is recorded; do not maintain two pivot tolerances. Use the 0.3.23 relative pivot test.

Do not switch square `linsolve` over to `chol` in this phase. A faster SPD path is a later optimization with its own bit-level residual check. Different rounding would move octave cases.

**Breaking impact.** None for current callers: they pass sparse matrices and get a handle. A dense matrix that used to error now returns a factor. Search `chol(` / `lu(` (about 36 and 22 hits, mostly docs and tests) and update help that says "must be sparse".

**Performance.** Dense chol is about half the flops of GE and only for SPD. It is not on `bench_linalg` today. Acceptance: sparse `chol` + `solve` on the existing tests is within 10% (same code). Dense `chol` of a 256×256 SPD matrix is timed once and reported, not gated, unless it regresses a bench we already have. Do not densify a sparse matrix to call it.

**Tests.** `R' * R ≈ A` on a small SPD matrix, real and complex Hermitian. `[R, p] = chol` on a diagonal with a negative entry returns `p != 0` and does not error. `A(p, :) ≈ L*U` for dense `lu`. Sparse handle tests unchanged. Octave: `chol` of a 4×4 SPD matrix and `lu` with permutation vector. Octave's `lu` permutation is a matrix; convert with a one-line `P` builder on the Octave side or compare `A(p,:)` reconstructed in rustlab only. Prefer the in-process residual so we do not depend on Octave's `P` shape.

---

## 17. `polyfit` `[p, S, mu]` and `polyval`

**Current.** Not implemented. `roots` exists. `em_lesson_review_2026_07.md` lists `polyfit` / `polyval` as not done. `dev/requests` has `poly` from roots as a separate small item; this phase does not have to ship `poly`.

**MATLAB.** `p = polyfit(x, y, d)` is a row of coefficients, highest degree first, unscaled Vandermonde. `[p, S] = polyfit(x, y, d)` adds a struct `S` with `R` (the triangular factor), `df`, and `normr`, for `polyval` error estimates. `[p, S, mu] = polyfit(x, y, d)` **changes** `p`: it centers and scales `x` by `mu = [mean(x), std(x)]` and fits in that variable. `polyval(p, x)`, `polyval(p, x, S)`, and `polyval(p, x, S, mu)` undo that. One-output and three-output coefficients are not interchangeable.

**Proposed change.** Phase 7. Solve the Vandermonde system with the phase-4 QR (least squares), real data. Complex `x` or `y` is an error in v1 (MATLAB allows complex; we can add it once the real path matches, and the QR is already complex, so this is a product cut not an algorithm cut).

- One output: unscaled basis.
- Two outputs: `[p, S]` with fields `R`, `df`, `normr`.
- Three outputs: centered and scaled, `mu` a length-2 vector.
- `polyval` with 2, 3, or 4 arguments as above. The error-estimate output of `polyval` (`delta`) waits; not required to check `[p, S, mu]`.

No silent degree cap that changes a high-degree fit into a lower one. Document that the unscaled basis is ill-conditioned, which is why the three-output form exists. A huge `n` (propose `n * (d+1) > 1e8` entries in the Vandermonde) errors before allocating. That cap is about memory, not about clipping the answer.

**Breaking impact.** None. New names.

**Performance.** O(n d²) for the QR, plus the Vandermonde build. Not on the training or DSP bench path. Acceptance: `make perf` unchanged. A one-shot fit of `n = 1e5`, `d = 5` is reported in the PR. Do not form a normal-equation Gram matrix of the Vandermonde.

**Tests.** Exact fit of `1 + 2x + 3x²` on 5 points recovers `p` to `1e-10` (Vandermonde is not bit-exact). Three-output `p` differs from one-output `p` on a non-centered `x`, and `polyval(p, x, S, mu)` matches `y` on those nodes. Compare one-output `p` to Octave's `polyfit` (same unscaled convention). Compare `mu` to `mean` and `std` (sample `std`, which matches MATLAB's `std` default and our phase-1 `std`). `S.df = length(x) - (d+1)`.

---

## 18. `interp1` with pchip, makima, and a fill value

**Current.** Not implemented. `interp_freq` linearly interpolates S-parameter traces and **rejects** extrapolation. Do not overload it. Different rules, different callers.

**MATLAB.** `interp1(x, v, xq)` is linear. Methods include `"linear"`, `"nearest"`, `"next"`, `"previous"`, `"pchip"`, `"makima"`, `"spline"`. Out of range is NaN. A trailing numeric argument is the fill value. `"extrap"` uses the end piece of the interpolant. `x` must be monotonic.

**Proposed change.** Phase 7, same PR as `polyfit` only if that PR stays reviewable; otherwise `interp1` is the next PR. Ship:

- `"linear"` (default), `"nearest"`, `"next"`, `"previous"`
- `"pchip"` (Fritsch–Carlson slopes, one O(n) pass, then Hermite)
- `"makima"` (modified Akima, one O(n) pass)
- fill: omitted → NaN outside `[x(1), x(end)]`; a numeric extrap argument; `"extrap"` for linear and for the piecewise cubics

Do not ship `"spline"` in this phase (tridiagonal solve, not requested as firmly as pchip/makima). `x` non-monotonic is an error. `xq` is not sorted in place; output lines up with `xq`. If `xq` happens to be sorted, a two-finger walk is fine; otherwise binary search. Slopes are computed once per call, not per query.

Real data in v1. Complex `v` can follow if it is "interpolate real and imag separately" with no extra design; that matches MATLAB. Include it if it falls out of the real kernel. Do not block the PR on it.

**Breaking impact.** None. New name.

**Performance.** O(n) setup + O(nq log n) eval, or O(n + nq) when both are sorted. Acceptance: `make perf` unchanged. One-shot: `n = 1e5`, `nq = 1e5`, linear and pchip, reported in the PR. No crate.

**Tests.** Linear: exact on a line, including a query on a node. Outside range is NaN, and `interp1(..., 0)` fills 0. pchip: reproduces a cubic's values at nodes, and a known Fritsch–Carlson slope on a 4-point fixture written out in the test (not "close to spline"). makima: a fixture with a flat run and a spike where modified Akima and ordinary Akima differ, so a wrong slope formula fails. Octave has `interp1` with `"pchip"`; add that compare. Check makima against a short published vector in `math_audit.rs` if Octave's makima is missing or named differently. `"spline"` absent: calling it errors with the list of supported methods.

---

## 19. Vector versus matrix, as a type unification

**Current.** `Value::Vector` and `Value::Matrix` are different. Real storage, `real_ops`, the elementwise drivers, `sum` lanes, `parmap` packing, and copy-on-write all special-case the vector. `real_storage.md` counts about 1,150 `Vector` / `Matrix` / `Tensor3` match sites. `size` of a vector is already `[1, n]`. Broadcasting already treats a vector as a row (octave item #7).

**MATLAB.** Only matrices. A "vector" is 1×n or n×1.

**Proposed change.** Do not unify the types. Shape bugs get a local rule (section 2 for `\`, section 1 for constructors). A unification would touch every match arm and put every DSP buffer on the matrix path. That is the performance risk this plan is not willing to take for a shape story `size` already tells.

**Breaking impact.** None.

**Performance.** Unifying is the hazard: extra rank checks, missed `as_slice` fast paths, accidental widens. Leaving the types alone costs nothing.

**Tests.** None new. Phase 2 and phase 5 tests are the guards that 1-D kernels still run.

---

## Do not change

| Item | Why it stays |
|---|---|
| `norm(A)` default (Frobenius) | 2-norm is `svd_full`. Matrix residuals in the PDE examples and `eig.rlab` would become SVDs and change value. `norm(A, 2)` already exists. Octave fixture depends on the Frobenius default. |
| Sparse `1e-15` drop | Exact-zero-only storage lets roundoff densify `chol` / `lu`. Laplacian `spsolve` is the path that would pay. `1e-14 * speye` already factors (0.3.23). |
| `Vector` vs `Matrix` unification | About 1,150 match sites. Real-storage elementwise, SIMD, and `parmap` assume a 1-D buffer. `size` already says a vector is a row. |
| `diag` of length 1 as a scalar | Same collapse as `kron` and one-element `linsolve`. A 1×1 matrix misses the scalar `f64` arm. The printed value already matches. |
| Butterworth one-output = `b` | That is MATLAB nargout. The Hz + sample-rate signature stays; `butter(n, Wn)` would silently retune every filter. |
| Sparse `chol` / `lu` return a handle | The handle is the factor-once path. A dense triangle would copy and throw that away. Dense factors are a new overload, not a replacement. |
| Square `linsolve` and square `spsolve` algorithms | QR or a min-norm formula would change rounding and timing on the path benches already cover. Rectangular `\` is QR. Square stays. |
| Normal equations for least squares or `polyfit` | Squares the condition. Phase 4 QR is the solver. |
| `sort(M, dim)` inside phase 1 | Real follow-on, but it turns an error into per-column results. Ship the complex key first so the two breaks are not one PR. |
| `std` / `var` weight **vectors** | Normalization rules are a second feature. `w = 0` or `1` is the MATLAB default switch. A vector `w` errors until a later plan. |
| `"spline"` in `interp1` | Not required for the pchip / makima / fill request. |
| `freqz` return shape (2×n matrix) | Including Nyquist is the MATLAB numeric change. `[H, f] = freqz(...)` is a second break on top of `plotdb`. |
| Real-storage layout, SME matmul, elementwise driver structure | Out of scope. Phases may add a 1×n or n×1 fast-path **branch** that calls those kernels. They may not replace them. |

---

## Conflicts with other plans

Checked before writing this. Implementation PRs should update the pointed-to status line when they land; this plan does not edit those files.

| Document | Conflict |
|---|---|
| [`octave_compat_divergences.md`](octave_compat_divergences.md) "Core math audit" | Several "why it stays" rows are proposals to change here (`eigs` `D`, complex `sort`, `std`/`var` empty and `w`, `trace`, `freqz`, and eventually `qr` / `polyfit` / dense `chol`). The ones that still stay (`norm` default, sparse drop, `diag` length-1, butter one-output) are restated above so the two docs do not silently diverge. |
| Same file, item #5 | Asks for a deprecation warning and a release of bake time before `zeros(n)` flips. This plan flips in the same PR as the sweep. Pre-1.0, and the migration is the decision. |
| [`roadmap.md`](roadmap.md) C12 | `zeros(n)` "needs a decision". Phase 2 is that decision. Mark C12 done when phase 2 lands. |
| 0.3.23 changelog | Locks the half-open `freqz` grid, Frobenius `norm(S)`, the `1e-15` drop, butter one-output = `b`, and "`var` has no divide-by-n flag". Phase 1 changes the `var` sentence. Phase 8 changes the `freqz` sentence. The others stay, on purpose. |
| [`real_storage.md`](real_storage.md), perf flywheel iterations 01–03 | Vector kernels and "do not widen" are the acceptance tests for phases 2 and 5. A phase that shows a new `[widen]` on real data is not done. |
| [`closed/matrix_vector_coercion.md`](closed/matrix_vector_coercion.md) | Vectors are rows; `randn(1, n)` is a matrix. Phase 2 must not collapse that matrix into a vector to make the sweep easier. |
| [`integer_types.md`](integer_types.md) | Hybrid MATLAB, with two named deviations (integer+double, opt-in wrap). Do not reopen those. Do include `zeros(n, "int32")` in the shape flip. The integer plan never specified single-arg shape. |
| [`closed/sparameters.md`](closed/sparameters.md) "MATLAB-as-reference policy", and [`examples_notebooks_bug_hunt.md`](examples_notebooks_bug_hunt.md) POL1 | Shipped docs, REPL help, and examples should describe the behavior (shapes, flags, errors) without calling it a MATLAB port. This plan is allowed to say MATLAB because it is the spec. 0.3.23 help text already contains the word in a few `eig` lines. See open question 8. |
| [`closed/rename_r_to_rlab.md`](closed/rename_r_to_rlab.md) | rustlab is not a MATLAB dialect. This migration is numeric behavior of existing and new math functions, not syntax cosplay and not a language rename. `break` / `continue` stay unsupported. Notebook-only features stay rustlab's. |
| [`em_lesson_review_2026_07.md`](em_lesson_review_2026_07.md) | `polyfit` / `polyval` were deferred. Phase 7 is that work, with `[p, S, mu]`, not a one-output-only fit. |
| Workflow rule 9 (plot backends) | No plot-backend change in these phases. `freqz` moves sample locations; `plotdb` reads the same 2×n layout. Confirm one DSP notebook render after phase 8, not a four-backend rewrite. |
| Workflow rule 10 | QR, Cholesky, LU, pchip, and makima are hand-rolled. No LAPACK, no Fortran, no new crate. |
| Workflow rule 12 | `0.3.23` has shipped. Each phase that changes results, shapes, parse acceptance, or errors bumps the workspace version. |

---

## Open questions for Michael

1. **Underdetermined `\`.** MATLAB returns a basic solution (QR with column pivoting, at most `rank` nonzeros). Octave returns the minimum-norm solution. `pinv(A)*b` already is minimum-norm. This plan follows MATLAB. Confirm, or say the word and phase 5 will follow Octave instead.
2. **Solution shape.** One right-hand side comes back as n×1, not as today's row-sized vector. Confirm even if phase 5 has to teach `real_ops` about n×1 to stay inside the 15% / no-widen gate.
3. **`zeros(n)` without a warning release.** Confirm we skip the deprecation cycle in the octave plan and break in one PR.
4. **`freqz` includes Nyquist** even though 0.3.23 and Octave's `freqz` use the half-open DFT grid. Confirm phase 8 is wanted. If not, drop phase 8 and leave the audit row as written.
5. **`norm(A)` stays Frobenius.** Confirm. The alternative is an SVD on every unqualified matrix norm.
6. **Sparse drop stays `1e-15`.** Confirm. The alternative is denser factorizations after roundoff.
7. **`2 / v` with `/`.** Plan: dimension error, like MATLAB; elementwise is `./`. The alternative is to keep elementwise `/` for `Value::Vector` only, which is nicer for old scripts and a permanent MATLAB mismatch.
8. **The word "MATLAB" in shipped help.** POL1 and the S-parameter plan say shipped docs should not name MATLAB. This plan uses it as the spec. Implementation PRs will describe shapes and flags in rustlab's words unless you want the help text to say MATLAB explicitly. 0.3.23 already says it in a few `eig` lines.
9. **`qr` default is economy, third output is a vector.** Full `Q` and a permutation matrix are flags. MATLAB's defaults are the expensive ones. Confirm the cheap defaults.
10. **`sort` stability.** NaNs move to the end either way. Stable tie-break only if `sort` of 1e6 real doubles stays within 2× of today's unstable sort. Confirm that gate.
11. **`randi(imax, n)` becomes n×n** together with `randn(n)`. Confirm. `randi(6, 5000)` in `examples/math/random.rlab` has to become `randi(6, 1, 5000)`.
12. **`interp1` without `"spline"` in v1.** Confirm pchip, makima, linear, nearest, next, previous, and fill are enough.
13. **Complex `polyfit`.** v1 is real only. The QR can take complex later. Confirm that cut.

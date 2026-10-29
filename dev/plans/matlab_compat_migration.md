# MATLAB compatibility migration

**Status:** plan only — no code in this change
**Current phase:** not started
**Base:** `0.3.24` (`9e7c6ad`, PR #100), merged into this branch. Workspace version `0.3.24` has shipped, so each phase that changes observable behavior bumps the version in the same PR (workflow rule 12).
**Date:** 2026-10-29

Break existing script behavior where that is what it takes to match MATLAB. This is pre-1.0. Do not take a performance regression on a hot path to buy a match that scripts do not need. Core math stays hand-rolled Rust. No new dependencies (workflow rule 10).

This plan starts from the "Core math audit (2026-10-28)" table and open item **#5** in [`octave_compat_divergences.md`](octave_compat_divergences.md). It does not reopen the ten divergences that table already marks shipped (`eig` shapes, `sum`/`mean` along columns, `find`, deletion assignment, and the rest). It also does not rebuild what `0.3.24` already shipped. Phases 3–7 below are only the gaps left against that release.

## Already on main (`0.3.24`)

Leave these alone. The audit table on `main` already records them.

| Shipped | What it does | Not in this plan |
|---|---|---|
| `A \ b`, `mldivide` | Square nonsingular: LU with partial pivoting. Otherwise column-pivoted QR. Singular or ill-conditioned: warning, then a **basic** solution (free variables are 0). A scalar on the left divides element-wise. | A new solver. Min-norm is a question, not a rewrite, see phase 5. |
| `B / A`, `mrdivide` | Two matrices, or a row, solve `X * A = B`. A scalar on either side of `/` stays element-wise. Two vectors with `/` still error (`./`). | Making `2 / v` an error. `0.3.24` kept the element-wise scalar. |
| `qr` | Unpivoted Householder `[Q, R]`. `qr(A, 0)` / `"econ"` / `"economy"` is the thin factor. One output is `R`. `diag(R)` is real and non-negative. | Rebuilding QR. Phase 4 only adds column pivoting and `P`. |
| Dense `lu` | `[L, U, P] = lu(A)` with `P` a permutation **matrix**, `P*A = L*U`. `[L, U]` absorbs the pivot so `L*U = A`. One dense output errors. Sparse `lu` is still the factor handle. | A second LU. Phase 6 adds a permutation **vector** and dense `chol`. |
| `polyfit`, `polyval` | QR least squares, coefficients in descending powers, one row. `polyval` is Horner and keeps the shape of `x`. `x` is real; `y` may be complex. | `[p, S, mu]`. Phase 7. |
| `interp1` | `linear` (default), `nearest`, `previous`, `next`, `spline` (not-a-knot). Outside the range: `NaN`, unless `"extrap"`. | `pchip`, `makima`, and a numeric fill. Phase 7. The spline stays. |
| `null`, `orth` | Orthonormal SVD bases, same cutoff as `rank`. | Matching a particular MATLAB sign pattern. Check `A*Z ≈ 0` and `Z'*Z ≈ I`. |

`\` already pivots inside `rustlab_core::factor::qr` (`qr(a, true)`). The public `qr` builtin passes `false` and rejects a third output. Phase 4 exposes that pivot. It does not write a new factorization.

## How to read a row

Each item has the same six parts. "Proposed change" is the recommendation. Items in [Do not change](#do-not-change) are still spelled out, so a later reader can see the rejection and not relitigate it.

**Measure.** Before and after, on the same machine, two runs:

- `make perf` (release binary, `perf/bench_*.rlab`, thresholds in `perf/run_perf.sh`). A number over the threshold fails the phase.
- `rustlab run --profile=ops` on the small scripts named in the item. Watch wall time and the `[widen]`, `[promote]`, and `[matmul]` rows. A real-storage fast path that starts widening is a failure even when the clock looks fine.
- `cargo test --workspace` and, for anything Octave actually agrees with, one new case in `tests/octave/` (`rustlab_full.rlab`, `reference_full.m`, `compare_full.m`).

**Acceptance, unless the item says otherwise.**

- Elementwise work (`./`, `/` by a scalar, `sum`, a length-`1e5` `randn` that is still a vector): within 10% of the before run, and the same `[widen]` / `[promote]` counts.
- Square `A \ b` keeps the `0.3.24` LU path, and sparse `spsolve` keeps today's algorithm and timing. Do not put a new QR on those paths.
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
| 3 | `.\` only | Still no token. Reuse the `./` kernel. Nothing old changes. |
| 4 | `[Q, R, P] = qr(A)` on the existing column-pivoted QR | `0.3.24` already returns unpivoted `[Q, R]` and economy `qr(A, 0)`. `\` already pivots and discards `P`. This PR returns that permutation. |
| 5 | Right-hand side shape of `\`, and the min-norm question | `0.3.24` treats a vector as a row (`b'`) and returns a basic solution. This phase only changes that if you say so. Default recommendation: leave both. |
| 6 | Dense `chol`, and a permutation **vector** from dense `lu` | `[L, U, P]` with a permutation matrix already shipped. Sparse handles stay. |
| 7 | `polyfit` `[p, S, mu]`; `interp1` `pchip` / `makima` / numeric fill | One-output `polyfit` / `polyval` and `interp1` (`linear`, `nearest`, `previous`, `next`, `spline`, `"extrap"`) already shipped. Do not retune them. |
| 8 | `freqz` grid includes Nyquist | Reopens a decision locked in 0.3.23. Same cost, different samples. Octave's `freqz` is the wrong oracle. |

`sort(M)` / `sort(M, dim)` (the leftover of octave item #11) is a follow-on after phase 1, not part of it. Column sort changes every 2-D `sort` from an error into a per-column result. Ship the complex key first.

---

## 1. `zeros(n)` and the other single-arg constructors

Open item #5. The audit left it open on purpose.

**Current.** `zeros(n)`, `ones(n)`, `rand(n)`, and `randn(n)` return a length-`n` `Value::Vector`. `size` reports `[1, n]`. `zeros(m, n)` and `zeros([m, n])` are matrices. `zeros(1, n)` is a 1×n **matrix**, not a vector (`language_v0_3_4.md` documents that, and plot/indexing tests depend on it). `eye(n)` is already n×n. `randi(imax)` is a scalar. `randi(imax, n)` and `randi([lo, hi], n)` are length-`n` vectors. `zeros(n, "int32")` is a 1×n integer array (`peel_int_class` + `m.unwrap_or(1)` in `builtin_zeros`).

**MATLAB.** `zeros(n)`, `ones(n)`, `rand(n)`, `randn(n)`, and `randi(imax, n)` are n×n. A row is `zeros(1, n)`. A column is `zeros(n, 1)`. `randi(imax)` stays a scalar. `eye(n)` already matches.

**Proposed change.** One integer argument (and `randi`'s length argument) allocates n×n. Two-argument `zeros(m, n)` is unchanged. Integer-class forms follow the same shape rule. `eye` stays as it is. No deprecation release: 0.3.24 has shipped, and this migration is allowed to break scripts. The old plan's warning cycle conflicts with that; see [Conflicts](#conflicts-with-other-plans).

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

Phase 5. `0.3.24` already chose the row.

**Current (`0.3.24`).** A `Value::Vector` is a 1-D buffer. `size` reports `[1, n]`. `value_to_matrix` in `eval/linsys.rs` turns that vector into a **1×n row** before `A \ b`. A length mismatch says `hint: a vector is a row; pass a column as b'`. An n×1 matrix on the right is already one column. `pack` returns a scalar for 1×1, a vector for a single row, and a matrix otherwise, so one column of a tall solution is already n×1. Several columns stay n×k. `linsolve` is still the square elimination. `spsolve` is unchanged.

**MATLAB.** `A\b` needs as many rows as `A`. A 1×n row is a dimension error. People write `A\b` with `b` an n×1 column.

**Proposed change.** Do not merge `Vector` and `Matrix`. Recommendation: **keep the `0.3.24` row rule.** It matches `size`, it matches `M * v`, and it is what the changelog and `backslash_row_count_mismatch_names_the_column` just locked. Flipping a vector on the right to a column one release later breaks every script that followed `b'`.

If you answer question 2 with "make it a column", the change is only the right-hand operand of `\` (not `/`, not `*`):

- Read the vector as n×1 in place. Do not clone it into a matrix first.
- An explicit 1×n matrix stays a row and still errors against a tall `A`, matching MATLAB.
- The existing `pack` already returns n×1 for that solution. Do not also change `linsolve`'s return shape in this phase.

**Breaking impact.** None if we keep the row. If we flip: every `A \ v` that today errors, or that today treats `v` as a one-row system, changes. The hint test and the `0.3.24` changelog sentence have to move. `spsolve` stays.

**Performance.** Only if we flip. The next line is often `y = x .* h` with `h` a vector. An n×1 matrix may miss `real_ops` and show up as `[widen]`. Acceptance: on a real length-`1e6` residual, `x .* h` is within 15% of the same multiply where both are vectors, and `[widen]` / `[promote]` stay 0. If that fails, special-case a contiguous real n×1 in `real_ops` the same way phase 2 special-cases a real 1×n. Do not widen. Do not allocate a copy of `b`.

**Tests.** If we keep the row: leave `solve_factor.rs` as it is. If we flip: `size(A \ v)` is `[n, 1]` for a length-n vector, an explicit 1×n matrix still errors, and `A \ B` for an n×2 matrix is unchanged. Square residuals stay under the existing tolerance. Do not retune the LU path.

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

**Current.** `./` is elementwise right division (`BinOp::ElemDiv`) on the real and complex fast paths (`real_ops`, `elementwise::drive`). `0.3.24` added `\` and matrix `/`. It did not add `.\`. `A .\ B` is still a parse error.

**MATLAB.** `A .\ B` is elementwise `B ./ A`, with the same broadcasting.

**Proposed change.** Lex `.\` as one token, same as `./`. Implement it as elementwise division with the operands swapped inside the existing kernel. Do not form a reciprocal and multiply (two passes, one temporary). Do not clone either side. Scalar `a .\ b` is `b / a`.

**Breaking impact.** None. The token is new. Scripts that wrote `.\` as an error start to run.

**Performance.** Must be the `./` kernel. Acceptance: real length-1e7 `b .\ a` within 10% of `a ./ b`, zero `[widen]` / `[promote]`, no extra allocation proportional to length. Same `PAR_MIN_LEN` cutoff. Add the op to the existing elementwise bit-identical tests rather than a new driver.

**Tests.** ` [2, 4] .\ [2, 8] ` is `[1, 2]`. Broadcasting matches `./` with arguments swapped, including the real-storage cases in `tests/storage_modes.rs` if that harness walks operators by list (add `.\` to the list). Octave `.\` agrees; add one compare case.

---

## 13. `[Q, R, P] = qr`

Phase 4. Unpivoted QR already shipped.

**Current (`0.3.24`).** `rustlab_core::factor::qr` is a Householder QR. `qr(a, false)` is what the builtin returns: full `[Q, R]` by default, thin when the second argument is `0` / `"econ"` / `"economy"`, `R` alone for one output. `diag(R)` is real and non-negative. Three outputs error (`qr: returns at most [Q, R]`). `qr(a, true)` already column-pivots and is what `\` uses. That permutation is not returned to the script. `A[:, piv] = Q R` is the pivoted identity in the core.

**MATLAB.** `[Q, R, P] = qr(A)` column-pivots. `P` is a permutation matrix and `A*P = Q*R`. `[Q, R, p] = qr(A, "vector")` returns a vector, `A(:, p) = Q*R`.

**Proposed change.** Do not rewrite the reflector. When `nargout == 3`, call `qr(a, true)` and return `p` as a **1-based permutation vector** (length n). `"matrix"` returns the n×n permutation matrix for `A*P`. Two-output `qr(A)` stays unpivoted and full, which is what `0.3.24` tests lock (`qr_full_is_orthonormal_and_reconstructs`, `qr_economy_is_thin`). Do not flip the default to economy.

**Breaking impact.** None for one- and two-output calls. A new third output starts to succeed.

**Performance.** Pivoting is an O(mn) column-norm scan per column, already paid inside `\`. Forming `Q` stays on the explicit path only. Do not form `Q` for `\`. A tall full `Q` is still m×m; that is the existing two-output default, not something this phase adds. No new crate.

**Tests.** On a rank-deficient matrix, `A(:, p)` matches `Q*R` and a zero column moves to the end. Two-output `qr(A)` stays bit-identical to today's unpivoted factor (reuse `solve_factor.rs`). `"matrix"` satisfies `A*P ≈ Q*R`. Economy plus pivot, `qr(A, 0)` with three outputs, returns a thin `Q`.

---

## 14. Underdetermined `\` is a basic solution

Phase 5, together with section 2. The operator itself shipped.

**Current (`0.3.24`).** Square nonsingular `A \ b` uses `factor::lu`. Rectangular, and a square matrix that is singular to working precision, uses column-pivoted QR (`least_squares`). Free variables are 0. A warning is printed. `pinv(A)*b` is still the minimum-norm solution, and it is a different vector on a wide full-rank system. Overdetermined full-rank least squares matches MATLAB and Octave. Octave's underdetermined `\` is minimum-norm, so the octave harness must not be the oracle for that case.

**MATLAB.** Same basic solution as `0.3.24`. Minimum-norm is `pinv`.

**Proposed change.** Leave the basic solution. Switching to minimum-norm would match Octave and break MATLAB, and it would replace a factored triangular solve with a wider pseudoinverse. Do it only if question 1 says so. If it does: change only the `m < n` full-row-rank branch, keep free-variable behavior for rank-deficient systems documented, and do not form `A * A'`.

**Breaking impact.** None if we leave it. If we switch: every underdetermined `A \ b` changes, including `singular_consistent_system_still_solves` only when that fixture is wide. Square and tall least-squares tests stay.

**Performance.** A min-norm switch is a second factorization (QR of `A'` or an SVD). Acceptance if it happens: square `\` LU time unchanged, and `make perf` unchanged. Do not densify sparse `spsolve`.

**Tests.** Keep a hand-built wide system whose basic solution and `pinv` solution differ, so a silent switch fails the test. That fixture belongs next to `solve_factor.rs`.

---

## 15. Dense `chol`, and a permutation vector from `lu`

Phase 6. Dense `[L, U, P] = lu` already shipped. `P` is a matrix.

**Current (`0.3.24`).** Dense `lu` returns `[L, U]` (permutation absorbed, `L*U = A`) or `[L, U, P]` (`P*A = L*U`, `P` from `lu::permutation_matrix`). One dense output errors and points at `lu(sparse(A))` for the handle. Sparse `chol` and sparse `lu` still return factor handles. There is no dense Cholesky. `factor::lu` already stores `perm: Vec<usize>`.

**MATLAB.** `R = chol(A)` is upper triangular, `A = R'*R`. `[R, p] = chol(A)` does not error: `p = 0` on success, otherwise `p` is the failing pivot. `chol(A, "lower")` returns `L`. `[L, U, p] = lu(A, "vector")` returns a vector with `A(p, :) = L*U`. The no-flag third output is the matrix `0.3.24` already returns.

**Proposed change.**

- Add dense `chol` in `rustlab-core` (Golub & Van Loan), real symmetric and complex Hermitian. `R = chol(A)` errors if `A` is not SPD. `[R, p] = chol(A)` uses MATLAB's non-erroring `p`. `"lower"` returns `L`.
- Do not change the default third output of `lu`. Add `"vector"` so `[L, U, p] = lu(A, "vector")` returns the 1-based `perm` already in the factor, instead of building `permutation_matrix`. Two-output `lu` stays the absorbed form.
- Sparse `chol` / `lu` keep returning the handle.

Do not route square `A \ b` through dense `chol`. The LU path just shipped. A faster SPD `\` is a later optimization with its own residual check.

**Breaking impact.** None for current `lu` calls: they do not pass `"vector"`. Dense `chol` is a new success on a matrix that today errors with "must be sparse". Update help that says dense `chol` does not exist. Sparse call sites stay on the handle.

**Performance.** Dense chol is about half the flops of LU and only for SPD. Acceptance: sparse `chol` + `solve` within 10% (same code). Time a 256×256 dense `chol` once and report it. `"vector"` skips an n×n matrix of zeros and ones; that is the win, and it must not change the numeric `L` and `U`. Do not densify a sparse matrix.

**Tests.** `R'*R ≈ A` on a small SPD matrix, real and complex Hermitian. `[R, p] = chol` on a negative diagonal returns `p != 0` and does not error. `lu(A, "vector")` satisfies `A(p, :) ≈ L*U` and matches today's `P` applied as a matrix. Existing `lu_permutation_identity` stays green. Sparse handle tests unchanged.

---

## 16. `polyfit` `[p, S, mu]`

Phase 7. One-output `polyfit` / `polyval` already shipped.

**Current (`0.3.24`).** `polyfit(x, y, n)` builds a Vandermonde matrix and solves it with the pivoted QR. Coefficients come back highest power first, as a row for one series. `polyval(p, x)` is Horner and keeps the shape of `x`. `x` is real (`real_samples`). `y` may be complex. There is no `S` and no `mu`. `polyfit_recovers_an_exact_quadratic` locks the unscaled coefficients.

**MATLAB.** `[p, S] = polyfit(x, y, d)` adds `S.R`, `S.df`, `S.normr`. `[p, S, mu] = polyfit(x, y, d)` **changes** `p`: it centers and scales by `mu = [mean(x), std(x)]`. `polyval(p, x, S, mu)` undoes that. One-output `p` and three-output `p` are not interchangeable.

**Proposed change.** Keep the one-output unscaled fit bit-identical, including the existing quadratic test. Add `[p, S]` and `[p, S, mu]` on the same QR. `S` is a struct with `R`, `df`, `normr`. `mu` is a length-2 vector. `polyval(p, x, S, mu)` evaluates in the centered variable. The `polyval` error-estimate output (`delta`) waits.

Do not form a normal-equation Gram matrix. Do not center the one-output form. A Vandermonde larger than `n * (d+1) > 1e8` entries errors before allocating. That cap is memory, not a silent degree cut. `x` stays real.

**Breaking impact.** None for `p = polyfit(x, y, n)` and `polyval(p, x)`.

**Performance.** Same QR the one-output path already runs, plus a cheap column scale when `mu` is requested. Acceptance: `make perf` unchanged. Report one fit at `n = 1e5`, `d = 5`. The one-output path must not allocate `S`.

**Tests.** The existing exact quadratic stays. Three-output `p` differs from one-output `p` on a non-centered `x`, and `polyval(p, x, S, mu)` matches `y` at the nodes. `mu` matches `mean` and sample `std`. `S.df = length(x) - (d+1)`.

---

## 17. `interp1` pchip, makima, and a numeric fill

Phase 7, or the PR after `polyfit` if that review is already large. The methods below already exist. Do not retune them.

**Current (`0.3.24`).** `interp1(x, y, xi)` is linear. Also `"nearest"`, `"previous"`, `"next"`, and `"spline"` (not-a-knot: a sampled cubic is reproduced, two points are a line, three points are the quadratic). Outside the range the result is `NaN` unless the call passes `"extrap"`. A numeric fill value is an error (`the fifth argument must be "extrap"`). `x` must be strictly monotonic. `interp_freq` is a different function and stays that way.

**MATLAB.** Also `"pchip"` and `"makima"`. A trailing number is the value used outside the range, instead of `NaN`.

**Proposed change.** Add `"pchip"` (Fritsch–Carlson, one O(n) slope pass, then Hermite) and `"makima"` (modified Akima, one O(n) pass). A numeric extrapolation argument fills that value outside the range. `"extrap"` stays the end-piece extrapolant. Linear, nearest, previous, next, and the not-a-knot spline stay bit-identical, including `interp1_spline_reproduces_a_cubic`.

Slopes are computed once per call. Do not recompute them per query. No crate.

**Breaking impact.** None for existing method names and for `"extrap"`. A call that passed a number used to error; it will fill. Nothing in-tree does that.

**Performance.** O(n) setup plus the existing query loop. Acceptance: `make perf` unchanged. Report `n = 1e5`, `nq = 1e5`, pchip versus today's linear. Linear must not get slower by building unused slopes.

**Tests.** Existing linear / nearest / previous / next / extrap / spline tests stay. `interp1(..., method, 0)` is 0 outside the range and unchanged inside. pchip: values at the nodes match `y`, plus one Fritsch–Carlson slope written out in the test so it cannot pass by being "close to spline". makima: a flat run next to a spike where modified Akima and ordinary Akima differ. Octave `"pchip"` if that method exists there; otherwise the written slope is the oracle.

---

## 18. Vector versus matrix, as a type unification

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
| Sparse `chol` / `lu` return a handle | The handle is the factor-once path. Dense `chol` is a new overload. Dense `lu` already returns factors. |
| Square `A \ b` LU, and sparse `spsolve` | `0.3.24` just switched the square operator to LU. Do not replace it with chol or a min-norm SVD. Rectangular `\` stays pivoted QR. |
| Scalar `/` and scalar `\` | `0.3.24` kept both element-wise (`scalar_divide_matrix_stays_elementwise`, `backslash_scalar_is_elementwise`). MATLAB `2/A` is a solve. Reopening that now fights a release that just landed. `.\` is the element-wise left divide that is still missing. |
| Unpivoted `qr` default (full `Q`) | `0.3.24` tests lock it. Phase 4 adds a third output. It does not flip two-output `qr(A)` to economy. |
| Dense `lu` third output is a matrix | That is MATLAB's default and what `lu_permutation_identity` locks. `"vector"` is opt-in. |
| `null` / `orth` signs | `0.3.24` says the basis is some orthonormal SVD basis. Check the products, not the entries. |
| Not-a-knot `interp1` `"spline"` and `"extrap"` | Shipped. Phase 7 adds methods. It does not retune the cubic. |
| One-output `polyfit` / `polyval` | Shipped, unscaled. `[p, S, mu]` must not change that row. |
| Normal equations | Squares the condition. The existing QR is the solver for `\` and for `polyfit`. |
| `sort(M, dim)` inside phase 1 | Real follow-on, but it turns an error into per-column results. Ship the complex key first so the two breaks are not one PR. |
| `std` / `var` weight **vectors** | Normalization rules are a second feature. `w = 0` or `1` is the MATLAB default switch. A vector `w` errors until a later plan. |
| `freqz` return shape (2×n matrix) | Including Nyquist is the MATLAB numeric change. `[H, f] = freqz(...)` is a second break on top of `plotdb`. |
| Real-storage layout, SME matmul, elementwise driver structure | Out of scope. Phases may add a 1×n or n×1 fast-path **branch** that calls those kernels. They may not replace them. |

---

## Conflicts with other plans

Checked before writing this. Implementation PRs should update the pointed-to status line when they land; this plan does not edit those files.

| Document | Conflict |
|---|---|
| [`octave_compat_divergences.md`](octave_compat_divergences.md) "Core math audit" | Updated on `main` for `0.3.24`. Phases 3–7 match the gaps that table still lists (`.\`, `[Q,R,P]`, dense `chol`, `lu` vector, `[p,S,mu]`, pchip/makima/fill, row RHS, basic vs min-norm). Phases 1, 2, and 8 still propose changing rows that table marks as staying (`eigs` `D`, complex `sort`, `std`/`var`, `trace`, `zeros(n)`, `freqz`). `norm`, the sparse drop, length-1 `diag`, and butter one-output stay in both docs. |
| Same file, item #5 | Asks for a deprecation warning and a release of bake time before `zeros(n)` flips. This plan flips in the same PR as the sweep. Pre-1.0, and the migration is the decision. |
| [`roadmap.md`](roadmap.md) C12 | `zeros(n)` "needs a decision". Phase 2 is that decision. Mark C12 done when phase 2 lands. |
| 0.3.23 changelog | Locks the half-open `freqz` grid, Frobenius `norm(S)`, the `1e-15` drop, butter one-output = `b`, and "`var` has no divide-by-n flag". Phase 1 changes the `var` sentence. Phase 8 changes the `freqz` sentence. The others stay, on purpose. |
| [`real_storage.md`](real_storage.md), perf flywheel iterations 01–03 | Vector kernels and "do not widen" are the acceptance tests for phases 2 and 5. A phase that shows a new `[widen]` on real data is not done. |
| [`closed/matrix_vector_coercion.md`](closed/matrix_vector_coercion.md) | Vectors are rows; `randn(1, n)` is a matrix. Phase 2 must not collapse that matrix into a vector to make the sweep easier. |
| [`integer_types.md`](integer_types.md) | Hybrid MATLAB, with two named deviations (integer+double, opt-in wrap). Do not reopen those. Do include `zeros(n, "int32")` in the shape flip. The integer plan never specified single-arg shape. |
| [`closed/sparameters.md`](closed/sparameters.md) "MATLAB-as-reference policy", and [`examples_notebooks_bug_hunt.md`](examples_notebooks_bug_hunt.md) POL1 | Shipped docs, REPL help, and examples should describe the behavior (shapes, flags, errors) without calling it a MATLAB port. This plan is allowed to say MATLAB because it is the spec. 0.3.23 help text already contains the word in a few `eig` lines. See open question 7. |
| [`closed/rename_r_to_rlab.md`](closed/rename_r_to_rlab.md) | rustlab is not a MATLAB dialect. This migration is numeric behavior of existing and new math functions, not syntax cosplay and not a language rename. `break` / `continue` stay unsupported. Notebook-only features stay rustlab's. |
| [`em_lesson_review_2026_07.md`](em_lesson_review_2026_07.md) | `polyfit` / `polyval` were deferred. `0.3.24` shipped the coefficient row. Phase 7 is only `[p, S, mu]`. |
| Workflow rule 9 (plot backends) | No plot-backend change in these phases. `freqz` moves sample locations; `plotdb` reads the same 2×n layout. Confirm one DSP notebook render after phase 8, not a four-backend rewrite. |
| Workflow rule 10 | QR and LU are already in-tree. Dense Cholesky, pchip, and makima stay hand-rolled. No LAPACK, no Fortran, no new crate. |
| Workflow rule 12 | `0.3.24` has shipped. Each phase that changes results, shapes, parse acceptance, or errors bumps the workspace version. |

---

## Open questions for Michael

`0.3.24` already answered these, so they are not questions anymore: `\` and matrix `/` exist; underdetermined `\` is a basic solution; a vector on the right is a row; scalar `/` stays element-wise; `qr` without a flag is a full unpivoted factor; dense `lu`'s third output is a permutation matrix; `polyfit` returns one unscaled row; `interp1` has `spline` and `"extrap"`.

1. **Min-norm `\`.** Leave the basic solution (MATLAB, and what just shipped), or switch the wide full-rank case to minimum-norm (Octave)? Recommendation: leave it. `pinv(A)*b` stays the min-norm answer.
2. **Column right-hand side.** Leave "a vector is a row, pass `b'`", or treat a `Value::Vector` on the right of `\` as a column? Recommendation: leave the row. It matches `size` and the `0.3.24` tests. A yes means phase 5 breaks that hint and may have to teach `real_ops` about n×1.
3. **`zeros(n)` without a warning release.** Confirm we skip the deprecation cycle in the octave plan and break in one PR.
4. **`freqz` includes Nyquist** even though 0.3.23 and Octave's `freqz` use the half-open DFT grid. Confirm phase 8 is wanted. If not, drop phase 8.
5. **`norm(A)` stays Frobenius.** Confirm. The alternative is an SVD on every unqualified matrix norm.
6. **Sparse drop stays `1e-15`.** Confirm. The alternative is denser factorizations after roundoff.
7. **The word "MATLAB" in shipped help.** POL1 and the S-parameter plan say shipped docs should not name MATLAB. This plan uses it as the spec. Implementation PRs will describe shapes and flags in rustlab's words unless you want the help text to say MATLAB explicitly.
8. **`sort` stability.** NaNs move to the end either way. Stable tie-break only if `sort` of 1e6 real doubles stays within 2× of today's unstable sort. Confirm that gate.
9. **`randi(imax, n)` becomes n×n** together with `randn(n)`. Confirm. `randi(6, 5000)` in `examples/math/random.rlab` has to become `randi(6, 1, 5000)`.
10. **`interp1` additions.** Confirm phase 7 adds `pchip`, `makima`, and a numeric fill, and does not change the not-a-knot spline or `"extrap"`.

# MATLAB compatibility migration

**Status:** plan only — no code in this change
**Current phase:** not started
**Base:** `0.3.24` (`9e7c6ad`, PR #100), merged into this branch. Workspace version `0.3.24` has shipped, so each phase that changes observable behavior bumps the version in the same PR (workflow rule 12).
**Date:** 2026-10-29

**Direction (Michael, 2026-10-29):** match MATLAB whenever that is possible, including `\`. A behavior that is not MATLAB's stays only when this plan states a quantified performance reason. Every MATLAB-matching change still has a cost note and a mitigation. This is pre-1.0, so breaks are allowed. Core math stays hand-rolled Rust. No new dependencies (workflow rule 10).

This plan starts from the "Core math audit (2026-10-28)" table and open item **#5** in [`octave_compat_divergences.md`](octave_compat_divergences.md). It does not reopen the ten divergences that table already marks shipped (`eig` shapes, `sum`/`mean` along columns, `find`, deletion assignment, and the rest). It also does not rebuild what `0.3.24` already shipped. Later phases are only the gaps left against that release, plus the MATLAB matches decided below.

## Already on main (`0.3.24`)

Leave these alone. The audit table on `main` already records them.

| Shipped | What it does | Not in this plan |
|---|---|---|
| `A \ b`, `mldivide` | Square nonsingular: LU. Rectangular, and a square matrix the LU calls singular: column-pivoted QR, free variables 0. A vector on the right is a **row** (`b'`). A sparse matrix is densified (`docs/functions.md`). A scalar on the left divides element-wise. | The rectangular basic solution stays (it is MATLAB). Phase 5 changes the vector right-hand side, the square-singular `Inf` case, and square sparse dispatch. |
| `B / A`, `mrdivide` | Two matrices, or a row, solve `X * A = B`. A scalar on either side of `/` stays element-wise. Two vectors with `/` still error (`./`). The transpose is non-conjugate (`.'`). | Phase 3 makes a scalar numerator over an array into `mrdivide`, and uses conjugate transpose. Array `/` scalar stays element-wise. |
| `qr` | Unpivoted Householder `[Q, R]`. `qr(A, 0)` / `"econ"` / `"economy"` is the thin factor. One output is `R`. `diag(R)` is real and non-negative. | Rebuilding QR. Phase 4 only adds column pivoting and `P`. |
| Dense `lu` | `[L, U, P] = lu(A)` with `P` a permutation **matrix**, `P*A = L*U`. `[L, U]` absorbs the pivot so `L*U = A`. One dense output errors. Sparse `lu` is still the factor handle. | A second LU. Phase 6 adds a permutation **vector** and dense `chol`. |
| `polyfit`, `polyval` | QR least squares, coefficients in descending powers, one row. `polyval` is Horner and keeps the shape of `x`. `x` is real; `y` may be complex. | `[p, S, mu]`. Phase 7. |
| `interp1` | `linear` (default), `nearest`, `previous`, `next`, `spline` (not-a-knot). Outside the range: `NaN`, unless `"extrap"`. | `pchip`, `makima`, and a numeric fill. Phase 7. The spline stays. |
| `null`, `orth` | Orthonormal SVD bases, same cutoff as `rank`. | Matching a particular MATLAB sign pattern. Check `A*Z ≈ 0` and `Z'*Z ≈ I`. |

`\` already pivots inside `rustlab_core::factor::qr` (`qr(a, true)`). The public `qr` builtin passes `false` and rejects a third output. Phase 4 exposes that pivot. It does not write a new factorization.

## How to read a row

Each item has the same six parts. "Proposed change" is the recommendation. Items in [Kept, with a quantified reason](#kept-with-a-quantified-reason) are the only non-MATLAB behaviors this plan still accepts.

**Measure.** Before and after, on the same machine, two runs:

- `make perf` (release binary, `perf/bench_*.rlab`, thresholds in `perf/run_perf.sh`). A number over the threshold fails the phase.
- `rustlab run --profile=ops` on the small scripts named in the item. Watch wall time and the `[widen]`, `[promote]`, and `[matmul]` rows. A real-storage fast path that starts widening is a failure even when the clock looks fine.
- `cargo test --workspace` and, for anything Octave actually agrees with, one new case in `tests/octave/` (`rustlab_full.rlab`, `reference_full.m`, `compare_full.m`).

**Acceptance, unless the item says otherwise.**

- Elementwise work (`./`, `/` with a scalar denominator, `sum`, a length-`1e5` `randn` that is still a vector): within 10% of the before run, and the same `[widen]` / `[promote]` counts. A scalar numerator over a non-scalar is not this path (phase 3).
- `spsolve` on a 5-point Laplacian stays within 10%. Square dense `\` may get a faster Cholesky or triangular branch (phase 6); it must not get slower, and it must not grow a second rectangular solver.
- Sparse `norm(S)` with one argument keeps today's Frobenius timing. Dense `norm(A)` does not (phase 9).

Where Octave and MATLAB disagree, do not lock the octave harness to the MATLAB answer. Put a hand-checked fixture in `crates/rustlab-script/src/math_audit.rs` and say so in the test.

Gallery markdown is generated. Sweep `examples/` and `examples/notebooks/`, then regenerate `gallery/`. Do not hand-edit gallery copies. Counts below are sources, not gallery mirrors.

---

## Phase order

Separate PRs. Do not start the next phase until the current one is green, including the perf check. Suggested branch names are illustrative.

| Phase | PR | Why this order |
|---|---|---|
| 1 | `std` / `var` (`w` = 0 or 1, then a weight vector), empty → NaN; `median`; `sort` (complex, NaN, stable, then `sort(M, dim)`); `trace`; `diag` of length 1 → 1×1 | Small. No hot-path kernel change except the sort stability gate below. |
| 2 | `zeros` / `ones` / `rand` / `randn` / `randi` single-arg shape, including `randi(imax, n)` → n×n | Decided. The sweep is the work, and the OOM risk. |
| 3 | `.\`, and `/` when the numerator is a non-scalar's counterpart (scalar numerator over an array becomes `mrdivide`) | `./` and array/scalar `/` stay on the elementwise kernel. |
| 4 | `[Q, R, P] = qr(A)` on the existing column-pivoted QR | Two-output `qr(A)` stays full and unpivoted. |
| 5 | `\` matches MATLAB: vector right-hand side is a column, one solution is n×1, rectangular solve stays the basic QR solution, square singular follows LU (including `Inf`), square sparse uses `spsolve` | Not a min-norm rewrite. MathWorks' published vectors are the oracle. |
| 6 | Dense `chol`, `lu(..., "vector")`, and the faster square branches (SPD Cholesky, triangular solve) | `[L, U, P]` with a matrix already shipped. Sparse factor handles stay. |
| 7 | `polyfit` `[p, S, mu]`; `interp1` `pchip` / `makima` / numeric fill | Do not retune the shipped one-output fit or the not-a-knot spline. |
| 8 | `freqz` includes Nyquist | Decided. Octave's `freqz` is the wrong oracle. Same cost. |
| 9 | Dense `norm(A)` with no `p` becomes the 2-norm | Sparse `norm(S)` stays Frobenius. The reason is quantified below. |
| 10 | Sparse drop becomes exact zero only | Pivot tests inside `chol` / `lu` stay. Timing gate on a grid Laplacian. |
| 11 | `xcorr` / `xcov` | Not implemented. New surface. FFT when the lag span is long, direct when `maxlag` is short. |

`sort(M, dim)` is the second commit of phase 1, not a separate decision. Ship the complex key and the NaN rule first so a 2-D sort does not land on the old comparator.

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

## 2. Vector right-hand side of `\`

Phase 5. Decided: match MATLAB. A rustlab vector on the right is a column, and one solution is n×1.

**Current (`0.3.24`).** A `Value::Vector` is a 1-D buffer and `size` reports `[1, n]`. `value_to_matrix` in `eval/linsys.rs` turns that vector into a **1×n row** before `A \ b`. A length mismatch says `hint: a vector is a row; pass a column as b'`. An explicit n×1 matrix is already one column. `pack` returns a scalar for 1×1, a vector for a single row, and a matrix otherwise, so a tall single solution is already n×1 once the right-hand side has n rows. `docs/functions.md` documents the row rule. `*` of two vectors and `M * v` stay row-shaped; this phase does not change them.

**MATLAB.** `A` and `B` must have the same number of rows (`mldivide` reference). A 1×n row against a tall `A` is a dimension error. One right-hand side returns n×1. MathWorks' square example is `magic(3) \ [15;15;15]`, a 3×1 of ones. Several columns of `B` return n×p.

**Proposed change.** Only the right-hand operand of `\` (not `*`, not a vector on the left of `/`):

- Read a `Value::Vector` as n×1 in place. Do not clone it into a matrix.
- An explicit 1×n matrix stays a row and still errors against a tall `A`. That is the MATLAB row.
- `pack` already returns n×1 for that solution. `linsolve` of one vector should return the same n×1 shape, by calling this path, so the two square solvers do not disagree.
- `b'` where `b` is a vector is already n×1, so scripts that followed the 0.3.24 hint keep working. Scripts that wrote `A \ v` and got the row error start to solve.

**Breaking impact.** `backslash_row_count_mismatch_names_the_column` and the `docs/functions.md` sentence flip. A vector that today is a one-row system (a wide `A` with one row, or `scalar \ vector` which is element-wise and must stay element-wise) is the case to re-test: a length-1 vector is still a scalar-like 1×1, which `scalar_like` already special-cases. Do not route a scalar left operand through the column rule.

**Performance.** The next line is often `y = x .* h` with `h` a vector. An n×1 matrix can miss `real_ops` and show up as `[widen]`. Acceptance: a real length-1e6 `x .* h`, with `x` the n×1 solution, is within 15% of the same multiply of two vectors, and `[widen]` / `[promote]` stay 0. If that fails, special-case a contiguous real n×1 in `real_ops` in this same phase (the same idea as a real 1×n in phase 2). Do not widen. Do not allocate a copy of `b`.

**Tests.** `size(A \ v)` is `[n, 1]` for a length-n vector. `A \ [1, 2, …]` as an explicit row errors. `magic`-style `A \ column` matches the ones vector. `A \ B` for n×2 is unchanged. Square residuals stay under the existing tolerance. The LU factor for a nonsingular square matrix stays; this phase does not replace it.

---

## 3. `norm(A)` with no `p`

**Current.** `norm(v)` on a vector is the 2-norm. `norm(A)` on a matrix, dense or sparse, is Frobenius. `norm(A, 2)` is the largest singular value (`svd_full`: Gram matrix, then Hermitian eig). `norm(A, 1)` and `norm(A, Inf)` are the column and row sums. Sparse `norm(S, 2)` densifies and refuses past 512×512. The octave fixture writes `norm(matrix, 'fro')` on the Octave side and `norm(matrix)` on the rustlab side (`tests/octave/reference_full.m`, `rustlab_full.rlab`).

**MATLAB.** `norm(A)` for a matrix is the 2-norm. `norm(A, "fro")` is Frobenius. Vector `norm(v)` is already the 2-norm.

**Proposed change.** Phase 9. Dense `norm(A)` with no `p` becomes the 2-norm, the same value as `norm(A, 2)`. `norm(A, "fro")` stays the one-pass residual and is what the swept examples should call when they want today's number. Vector `norm(v)` is already the 2-norm.

Sparse `norm(S)` **stays Frobenius.** Reason, quantified: `norm(S, 2)` densifies and already errors when a side is over 512. A 512×512 complex Gram is `512² × 16` bytes ≈ 4 MB, and the Hermitian eig on top is O(n³) ≈ 1×10⁸ flops. A 2-D Laplacian on a 2000×2000 grid has `n = 4×10⁶` nodes and about `5n` nonzeros (tens of megabytes). The dense matrix for a 2-norm is `n² × 16` bytes, about 256 TB, so that call cannot run. Frobenius on the same sparse matrix is one pass over the nonzeros. There is no sparse 2-norm in tree that avoids the densify. `norm(S, 2)` remains available under the 512 cap for people who want MATLAB's number on a small pattern.

**Breaking impact.** Dense matrix residuals change value and cost. Known sites: `examples/pde/dielectric.rlab`, `laplacian.rlab`, `laplacian_bc.rlab`, `examples/notebooks/parallel_montecarlo.md`, `examples/linalg/eig.rlab` (`norm(A*V - V*D)`), and the matching notebooks. One-arg `norm` appears 73 times in 27 files; vector calls do not move. Rewrite residual checks that want a cheap number to `norm(..., "fro")` in the same PR. The octave pair that passes `'fro'` only on the Octave side (`reference_full.m` versus `rustlab_full.rlab`) must pass `'fro'` on both sides, or the rustlab side must expect the 2-norm.

**Performance.** Frobenius is one pass, O(mn). The dense 2-norm is `svd_full`: a Gram product O(max(m, n)·min(m, n)²) plus a Hermitian eig O(min(m, n)³). At n = 256 that Gram is about `2×256³ ≈ 3×10⁷` flops, which `perf/bench_linalg.rlab` already does as a multiply, plus the eig. `make perf` does not call unqualified `norm(A)`, so the bench threshold is not the guard. Guard: time one `norm(A)` and one `norm(A, "fro")` at 256×256 and record both in the PR. Do not call `norm(A)` inside a per-step training loop; nothing in `perf/` does. Sparse `norm(S)` timing must be unchanged.

**Tests.** Dense `norm([1, -2; -3, 4])` equals `norm(..., 2)`. `norm(..., "fro")` keeps today's value. The octave fixture is explicit about which norm it compares. Sparse `norm(speye(4))` stays `2` (Frobenius of `I₄`), and `norm(S, 2)` on that matrix still matches. A side of 513 still errors for `norm(S, 2)`.

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
- The second commit of this phase adds `sort(M)` / `sort(M, dim)` on that comparator. `sort(M)` sorts each column.
- `sort` is stable on ties, which is MATLAB. NaNs are partitioned to the end first so the stable sort does not have to order NaN keys.

**Breaking impact.** Scripts that `sort` a genuinely complex vector and expect real-part order. In-tree `sort(` shows up in docs, octave fixtures (real data), and tests; examples are few (`sort` is not a common example call). `median` of complex is unlikely in examples; `math_audit.rs` covers real medians. Empty `median` currently errors, so changing it to NaN breaks tests that expect the error, not user scripts that succeeded.

**Performance.** Real path: a NaN partition is O(n), then a stable sort. Acceptance: `sort` of 1e6 real doubles (no NaNs) within 2× of today's `sort_unstable`, and 1e7 within that same ratio. If the stable sort is slower than that, stop and put the timings in the PR instead of silently shipping unstable order. That gate is the mitigation, not a license to skip MATLAB on the first try. Complex path is O(n log n) `hypot`+`atan2` and is not in `bench_builtins`. Do not route real data through it. No `[widen]` on a real vector.

**Tests.** Real vector with negatives and NaNs: NaNs at the end, negatives at the start, ties keep input order. Complex: `sort([1+1j, -2, 1-1j])` matches magnitude-then-phase (Octave agrees; add the case). `sort(M)` column fixture. `median(1+1j)` errors. `median([])` is NaN. Real median fixtures stay bit-identical, including the even-length average.

---

## 6. `std` / `var` weight flag, and empty input

**Current.** `std` and `var` are the sample forms, divide by `n - 1` only. There is no `w`. Empty and length-1 both return 0 (`std_of_slice`). A matrix reduces down columns to a 1×n row, same as `mean`. Complex data uses `Σ|x − mean|² / (n − 1)`, which already matches MATLAB's complex `std`. `var` is `std²` in the same pass structure. 0.3.23 changelog: "There is no divide-by-n flag."

**MATLAB.** `std(x)` and `var(x)` default to `w = 0` (divide by `n - 1`). `std(x, 1)` and `var(x, 1)` divide by `n`. `std(x, w, dim)` selects the axis. `std([])` is NaN. A length-1 input is 0. A weight vector `w` the same length as `x` is the normalized weighted form.

**Proposed change.** `w` is 0 or 1, or a weight vector of the same length. `dim` is the argument after `w` (`std(M, w, dim)`). Empty input returns NaN (a row or column of NaNs when an empty matrix is reduced along an axis, not a scalar 0). Length 1 stays 0. Complex formula unchanged. The weight vector is the same phase: it is one extra multiply in the sum, not a second algorithm, so there is no performance reason to omit it.

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

**Proposed change.** Phase 1. `diag` of a length-1 vector or a scalar returns a 1×1 matrix, so `size(diag(5))` is `[1, 1]`. Do not change the 1×1 collapse in `kron` or in `pack` for a 1×1 `\` solution in this phase. Those are separate call sites; a length-1 `diag` is not on the scalar hot path.

**Breaking impact.** `diag([5]) + 1` and any test that expects a `Value::Scalar` move. The printed number can stay the same. Search `diag(` (about 82 mentions in 12 files, mostly square diagonals of length greater than 1).

**Performance.** One 1×1 allocation per call. Not a loop. The scalar `f64` arm in `Value::binop` stays for actual scalars. A 1×1 matrix plus a scalar already has a broadcast path; acceptance is that `diag([5]) + 1` is correct, not that it matches scalar `5+1` in nanoseconds. No `[widen]` on a real 1×1.

**Tests.** `size(diag(5))` and `size(diag([5]))` are `[1, 1]`, and the entry is 5. `diag([1, 2])` stays 2×2. `kron` of two scalars stays a scalar.

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

**Proposed change.** Phase 10. Drop only exact zeros, on construction and after arithmetic. There is no measured reason to keep `1e-15`: a 5-point Laplacian never stores those entries, so its `nnz` does not depend on the tolerance. The old "fill becomes O(n²)" claim is not what this cutoff prevents. A cancelled stored entry becomes a tiny stored entry and keeps the same index. That is MATLAB.

Mitigation, because a tiny pivot must not become a healthy factor: the relative pivot tests inside sparse `chol` / `lu` (0.3.23) stay. `1e-16 * speye(n)` is stored and then fails or warns on the pivot test if it is scaled like a singular matrix; it is not deleted before the factor sees it. Acceptance: `spsolve` on `laplacian_2d(64, 64)` is within 10% of today and `nnz` of that Laplacian is unchanged. If that gate fails, stop and report the numbers. Do not silently put the tolerance back.

**Breaking impact.** `nnz(1e-16 * speye(n))` becomes `n`, not 0. The 0.3.23 note that `1e-16 * speye` is an empty pattern moves. `1e-14 * speye` already factored and still should.

**Performance.** One compare per entry either way (`== 0` instead of `< 1e-15`). No extra asymptotic term on a clean Laplacian. The gate above is the check that roundoff stored in a residual does not blow the factor. `make perf` does not cover that grid, so the phase times `spsolve` itself.

**Tests.** Keep the 0.3.23 factor-of-`1e-14 * speye` test and the empty-pattern test for `1e-16`.

---

## 12. `.\`

**Current.** `./` is elementwise right division (`BinOp::ElemDiv`) on the real and complex fast paths (`real_ops`, `elementwise::drive`). `0.3.24` added `\` and matrix `/`. It did not add `.\`. `A .\ B` is still a parse error.

**MATLAB.** `A .\ B` is elementwise `B ./ A`, with the same broadcasting.

**Proposed change.** Lex `.\` as one token, same as `./`. Implement it as elementwise division with the operands swapped inside the existing kernel. Do not form a reciprocal and multiply (two passes, one temporary). Do not clone either side. Scalar `a .\ b` is `b / a`.

**Breaking impact.** None. The token is new. Scripts that wrote `.\` as an error start to run.

**Performance.** Must be the `./` kernel. Acceptance: real length-1e7 `b .\ a` within 10% of `a ./ b`, zero `[widen]` / `[promote]`, no extra allocation proportional to length. Same `PAR_MIN_LEN` cutoff. Add the op to the existing elementwise bit-identical tests rather than a new driver.

**Tests.** ` [2, 4] .\ [2, 8] ` is `[1, 2]`. Broadcasting matches `./` with arguments swapped, including the real-storage cases in `tests/storage_modes.rs` if that harness walks operators by list (add `.\` to the list). Octave `.\` agrees; add one compare case.

Same phase, because it is the other half of MATLAB division and it is not a new kernel: a **scalar numerator** over a non-scalar (`2 / v`, `2 / M`) becomes `mrdivide`, not element-wise. Array or vector divided by a scalar stays on today's `./` scale path. Detect a scalar denominator before any factor. MATLAB `2/M` for a square `M` is a dimension error (`X*M` cannot be 1×1). `M/2` stays a scale. `scalar_divide_matrix_stays_elementwise` changes. Complex `mrdivide` must use conjugate transpose `(A'\B')'`; `0.3.24` uses `.'` (`linsys.rs`). Conjugate is one pass, not a reason to keep the mismatch. Two vectors with `/` stay an error (`./`), which matches MATLAB for two rows.

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

## 14. Underdetermined `\` is MATLAB's basic solution

Phase 5, with section 2. Verified against MathWorks, not against Octave.

**What MATLAB does.** The `mldivide` reference says a rectangular solve "may not be the minimum-norm solution" and points minimum-norm at `lsqminnorm` or `pinv(A)*B`. The linear-equations guide says an underdetermined system returns a **basic** solution with at most `m` nonzero components, from QR with column pivoting: the first selected columns are solved, the rest stay 0. Two published vectors:

- `A = [1 2 0; 0 4 3]`, `b = [8; 18]` → `[0; 4; 2/3]`. Column 1 is the free variable.
- `R = [6 8 7 3; 3 5 4 1]`, `b = [7; 8]` → `[0; 17/7; 0; -29/7]`. Nonzeros sit on the columns picked by remaining norm (column 2, then column 4).

A square singular example on the same page is different: `[1 0; 0 0] \ [1; 1]` warns "singular to working precision" and returns `[1; Inf]`, which is LU dividing by zero, not a finite QR basic solution. `magic(4)` warns "close to singular" and still returns a finite LU solution.

**Current (`0.3.24`).** `least_squares` in `factor/qr.rs` already does the basic solution: column-norm pivoting, back-substitution on the accepted rank, free variables 0 (`y` is allocated as zeros). That is the MATLAB rectangular path. It is **not** minimum-norm. `pinv` remains the min-norm function. Do not add `lsqminnorm` in this phase, and do not switch `\` to `pinv`. Octave's underdetermined `\` is minimum-norm, so `compare_full.m` must not be the oracle here.

The square path does **not** match the singular example. If LU reports singular, rustlab warns and falls through to QR, which returns a finite basic solution instead of `[1; Inf]`.

Sparse `A \ b` calls `to_dense()` (`value_to_matrix`). MATLAB keeps a sparse square matrix on a sparse factor.

**Proposed change.**

- Rectangular: keep QR with column pivoting and free variables 0. Adjust the rank tolerance only if the two published vectors disagree (our cutoff is `eps * max(m,n) * max|entry|`). Do not form `A*A'` and do not call the SVD.
- Square singular: stay on LU and return its result, including `Inf`, with the singular warning. Do not replace that result with the QR basic solution. Nonsingular square LU is unchanged.
- Square sparse: do not densify. Call the existing `spsolve` dispatch (Cholesky when it already would, else sparse LU).

**Breaking impact.** Wide systems that already matched the basic solution do not move. Square singular results can change from a finite QR vector to LU's `Inf`/`NaN`. Sparse square `\` stops allocating a dense matrix. The row-hint test moves with section 2.

**Performance.** Rectangular cost stays one pivoted QR, which is what we already pay. A min-norm SVD would be a second, slower factor; we are not adding it. Square sparse is the win: a 128×128 grid is `n = 16384` unknowns, and a dense complex matrix is `n² × 16` bytes ≈ 4 GB before the LU factor. Sparse chol does not allocate that. Acceptance: `spsolve` time on `laplacian_2d` unchanged (same solver), and a dense 64×64 nonsingular `\` within 10% of today. Rectangular sparse has no sparse QR in tree (SuiteSparseQR is C++ and out of rule 10). Densify those only when `m*n <= 2_000_000` (about 32 MB of complex entries); larger rectangular sparse `\` errors and names the cap. That cap is the quantified reason not to pretend we have a sparse QR.

**Tests.** Both MathWorks vectors, exactly (rationals, not a loose residual). A third wide system where `pinv(A)*b` differs, so a min-norm "fix" fails. `[1 0; 0 0] \ [1; 1]` is `[1; Inf]` plus the singular warning. Nonsingular square residuals stay. One Laplacian `sparse \ b` matches `spsolve` and does not call `to_dense`.

---

## 15. Dense `chol`, and a permutation vector from `lu`

Phase 6. Dense `[L, U, P] = lu` already shipped. `P` is a matrix.

**Current (`0.3.24`).** Dense `lu` returns `[L, U]` (permutation absorbed, `L*U = A`) or `[L, U, P]` (`P*A = L*U`, `P` from `lu::permutation_matrix`). One dense output errors and points at `lu(sparse(A))` for the handle. Sparse `chol` and sparse `lu` still return factor handles. There is no dense Cholesky. `factor::lu` already stores `perm: Vec<usize>`.

**MATLAB.** `R = chol(A)` is upper triangular, `A = R'*R`. `[R, p] = chol(A)` does not error: `p = 0` on success, otherwise `p` is the failing pivot. `chol(A, "lower")` returns `L`. `[L, U, p] = lu(A, "vector")` returns a vector with `A(p, :) = L*U`. The no-flag third output is the matrix `0.3.24` already returns.

**Proposed change.**

- Add dense `chol` in `rustlab-core` (Golub & Van Loan), real symmetric and complex Hermitian. `R = chol(A)` errors if `A` is not SPD. `[R, p] = chol(A)` uses MATLAB's non-erroring `p`. `"lower"` returns `L`.
- Do not change the default third output of `lu`. Add `"vector"` so `[L, U, p] = lu(A, "vector")` returns the 1-based `perm` already in the factor, instead of building `permutation_matrix`. Two-output `lu` stays the absorbed form.
- Sparse `chol` / `lu` keep returning the handle.

After dense `chol` exists, square `A \ b` uses it when the matrix is real symmetric or complex Hermitian and the factor succeeds. That is MATLAB's full-matrix dispatch, and it is faster: Cholesky is `n³/3` flops against LU's `2n³/3` (about half). At `n = 256` that is about `5.6×10⁶` versus `1.1×10⁷` flops. If Cholesky fails, fall back to today's LU. A triangular matrix uses a triangular solve, `O(n²)`: at `n = 1024` that is about `10⁶` flops against about `7×10⁸` for LU. The scan that detects the triangle is `O(n²)` and is cheap next to LU. General nonsingular squares stay on LU. Do not send a rectangular system through Cholesky.

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

**Performance.** This is the one representation we do not merge, and the reason is measured. `real_storage.md` records the `rustlab_llm` training step going from about 56 ms to about 29 ms in 0.3.17 by keeping a 1-D real buffer (8 bytes) instead of a complex one (16). The same note counts about 1,150 `Vector` / `Matrix` / `Tensor3` match sites. Collapsing the two types would put every DSP buffer through the matrix arms and risks a `[widen]` on the elementwise kernels that produced that speedup. MATLAB's visible shapes are done locally: `size` of a vector is already `[1, n]`, phase 2 makes `zeros(n)` square, phase 5 makes a vector on the right of `\` a column. None of those need one storage type.

**Tests.** None new for the type split. Phase 2 and phase 5 tests are the guards that the 1-D kernels still run.

---

## 19. `xcorr` and `xcov`

Phase 11. They do not exist. Checked the builtin registry, `docs/quickref.md`, and the tree: no `xcorr`, `xcov`, `corrcoef`, or other correlation builtin. `convolve` is linear convolution, length `len(x)+len(h)-1`, direct `O(n·m)` (`crates/rustlab-dsp/src/convolution.rs`). `overlap_add` is the long-filter FFT path for convolution. Neither returns lags, a scale option, or a zero-lag-centered correlation. Do not overload `convolve` into `xcorr`.

**MATLAB.** `r = xcorr(x)` and `r = xcorr(x, y)`. Output length is `2N-1` when `maxlag` is omitted (`N` is the longer input; the shorter is zero-padded). `xcorr(x, y, maxlag)` returns length `2*maxlag+1`. Lags run from `-maxlag` to `+maxlag`, zero lag in the middle. `[r, lags] = xcorr(...)` returns that lag axis in the same orientation as `r`. Positive lag `m` is `sum_n x(n+m) conj(y(n))`.

`scaleopt`, default `'none'`:

| Flag | Scale |
|---|---|
| `'none'` | raw sums |
| `'biased'` | divide by `N` |
| `'unbiased'` | divide by `N - abs(m)` |
| `'normalized'` or `'coeff'` | divide so the zero-lag autocorrelation is 1; for two inputs, divide by `sqrt(r_xx(0) r_yy(0))` |

A column input returns a column; a row returns a row. A matrix is one signal per column, and `xcorr(X)` returns every column-pair (width `ncols²`). `xcov` is `xcorr` after each signal has its mean removed. The same `maxlag` and `scaleopt` apply.

**Proposed change.** New `xcorr` and `xcov` in `rustlab-dsp`, pure Rust, no crate. `xcov` subtracts the mean and calls the same kernel. One output is `r`. Two outputs are `[r, lags]`. A rustlab `Value::Vector` returns a `Value::Vector` (size `[1, 2N-1]`, the row MATLAB returns for a row). An n×1 matrix returns an `(2n-1)×1` matrix. Real inputs stay real (`conj` is a no-op); do not widen.

**Breaking impact.** None. New names. `convolve` stays.

**Performance.** Direct correlation is `O(N · L)` with `L = 2·maxlag+1`. FFT correlation zero-pads to at least `2N-1`, multiplies `fft(x)` by `conj(fft(y))` (with `y` placed so the lag shift is a rotation, not a flipped temporary of length `2N`), and `ifft`s. That is `O(N log N)` and uses the in-tree FFT, including Bluestein when the length is not a power of two.

Use direct when `N * maxlag` is the smaller bill, and FFT otherwise. Concrete switch: direct when `maxlag <= 32` or `N < 256`; FFT when `maxlag` is the default `N-1` and `N >= 256`. At `N = 4096` and full lags, direct is about `4096² ≈ 1.7×10⁷` multiply-adds and an 8192-point FFT is a few times `8192 × 13` butterflies, so FFT wins. At `N = 10⁵` and `maxlag = 20`, direct is about `2×10⁶` operations and an FFT of length `2×10⁵` is wasted work. Do not implement full-lag `xcorr` by calling `convolve` on a reversed copy: that is the direct algorithm plus an extra length-`N` buffer, and it cannot apply `maxlag` without computing lags that are then thrown away.

Acceptance: `make perf` unchanged (nothing in `perf/` calls `xcorr`). Report one full-lag FFT run at `N = 10⁵` and one `maxlag = 20` direct run at the same `N`. The two algorithms must match on `N = 256` to `1e-9` relative, including `'unbiased'` at the ends. Real data: zero `[widen]`.

**Tests.** A 4-point hand sum for lags `-3:3`, including `conj` on a complex `y`. Length `2N-1`, zero lag at index `N` (1-based). `[r, lags]` lags equal `-maxlag:maxlag`. `'biased'` is `r/N`. `'coeff'` has zero-lag 1 on an autocorrelation. `xcov` of a constant is zero. Column in, column out. Octave's `xcorr` (signal package) matches MATLAB's lag order; add one compare case if `xcorr` exists in the Octave we run, otherwise the hand sum is the oracle. Do not compare against `conv`, which is not centered.

---

## Kept, with a quantified reason

Everything from the old "leave it" list that is not in this table is a phase above.

| Item | Why it stays |
|---|---|
| `Vector` and `Matrix` stay two types | Measured. Real 1-D storage cut the training step from about 56 ms to about 29 ms (`real_storage.md`, 0.3.17) and there are about 1,150 match sites. MATLAB shape is applied locally (phases 2 and 5), not by merging the types. |
| Sparse `norm(S)` stays Frobenius | A 2000×2000 grid is `4×10⁶` nodes. Dense storage for a 2-norm is `n² × 16` bytes ≈ 256 TB. `norm(S, 2)` already refuses a side over 512, where the Gram alone is about 4 MB. Dense `norm(A)` does change (phase 9). |
| Rectangular sparse `\` may densify, under a cap | No sparse QR in tree. SuiteSparseQR is C++ (rule 10). Cap `m*n <= 2_000_000` (~32 MB complex). Square sparse `\` does not densify (phase 5). |
| Sparse `chol` / `lu` still return a handle | The handle is factor-once. Numeric Cholesky of a 2-D grid is `O(n^{1.5})`; each `solve` is much smaller. Returning only a numeric factor would repeat that factor on every right-hand side. Dense `chol` is a separate overload (phase 6). |
| Butterworth one-output stays `b` | This already is MATLAB nargout. It is not a kept divergence. The Hz + sample-rate arguments stay because a silent switch to normalized `Wn` would design the wrong filter. That is a unit bug, not a speed tradeoff. |
| `null` / `orth` signs | The subspace is what MATLAB defines. A particular sign is not unique. Tests check `A*Z ≈ 0` and `Z'*Z ≈ I`. |
| Normal equations | They square the condition. `\` and `polyfit` already use QR. |
| `freqz` stays a 2×n matrix | Phase 8 moves the samples to include Nyquist. `[H, f] = freqz(...)` would also break `plotdb`, which reads row 0 and row 1. One break at a time. |
| Real-storage layout, SME matmul, elementwise drivers | Phases may add a 1×n or n×1 branch that calls those kernels. They may not replace them. |

Unpivoted full `qr(A)` and dense `lu`'s permutation matrix stay because they already match MATLAB's defaults. `"vector"` and `[Q,R,P]` are additions (phases 4 and 6), not replacements.

---

## Conflicts with other plans

Checked before writing this. Implementation PRs should update the pointed-to status line when they land; this plan does not edit those files.

| Document | Conflict |
|---|---|
| [`octave_compat_divergences.md`](octave_compat_divergences.md) "Core math audit" | Its "why it stays" rows for `norm`, the sparse drop, length-1 `diag`, row `\` , and the min-norm note are superseded here: dense `norm` and the drop become phases 9 and 10, `diag` becomes phase 1, and `\` follows MATLAB (column right-hand side, basic QR, not min-norm). Sparse `norm(S)` still stays, with the size argument above. |
| Same file, item #5 | Asks for a deprecation warning and a release of bake time before `zeros(n)` flips. This plan flips in the same PR as the sweep. Pre-1.0, and the migration is the decision. |
| [`roadmap.md`](roadmap.md) C12 | `zeros(n)` "needs a decision". Phase 2 is that decision. Mark C12 done when phase 2 lands. |
| 0.3.23 changelog | Locks the half-open `freqz` grid, Frobenius `norm`, the `1e-15` drop, butter one-output = `b`, and "`var` has no divide-by-n flag". Phases 1, 8, 9, and 10 change the `var`, `freqz`, dense-`norm`, and drop sentences. Sparse `norm(S)` and butter's one-output value stay. Butter stays because it already matches MATLAB, not because the changelog froze a divergence. |
| [`real_storage.md`](real_storage.md), perf flywheel iterations 01–03 | Vector kernels and "do not widen" are the acceptance tests for phases 2 and 5. A phase that shows a new `[widen]` on real data is not done. |
| [`closed/matrix_vector_coercion.md`](closed/matrix_vector_coercion.md) | Vectors are rows; `randn(1, n)` is a matrix. Phase 2 must not collapse that matrix into a vector to make the sweep easier. |
| [`integer_types.md`](integer_types.md) | Hybrid MATLAB, with two named deviations (integer+double, opt-in wrap). Do not reopen those. Do include `zeros(n, "int32")` in the shape flip. The integer plan never specified single-arg shape. |
| [`closed/sparameters.md`](closed/sparameters.md) "MATLAB-as-reference policy", and [`examples_notebooks_bug_hunt.md`](examples_notebooks_bug_hunt.md) POL1 | Shipped docs and REPL help should describe shapes and flags without naming MATLAB. This plan is the spec and does name it. See the one open question below. |
| [`closed/rename_r_to_rlab.md`](closed/rename_r_to_rlab.md) | rustlab is not a MATLAB dialect. This migration is numeric behavior of existing and new math functions, not syntax cosplay and not a language rename. `break` / `continue` stay unsupported. Notebook-only features stay rustlab's. |
| [`em_lesson_review_2026_07.md`](em_lesson_review_2026_07.md) | `polyfit` / `polyval` were deferred. `0.3.24` shipped the coefficient row. Phase 7 is only `[p, S, mu]`. |
| Workflow rule 9 (plot backends) | No plot-backend change in these phases. `freqz` moves sample locations; `plotdb` reads the same 2×n layout. Confirm one DSP notebook render after phase 8, not a four-backend rewrite. |
| Workflow rule 10 | QR and LU are already in-tree. Dense Cholesky, pchip, and makima stay hand-rolled. No LAPACK, no Fortran, no new crate. |
| Workflow rule 12 | `0.3.24` has shipped. Each phase that changes results, shapes, parse acceptance, or errors bumps the workspace version. |

---

## Decided

Michael's direction (match MATLAB whenever possible) closes the earlier questions:

- `\` with a vector on the right is a column, and one solution is n×1 (phase 5).
- Underdetermined `\` stays a basic solution from QR with column pivoting. MathWorks documents that this is not the minimum-norm solution; `pinv` / `lsqminnorm` are. Do not implement min-norm `\` (phase 5).
- `zeros(n)` / `randi(imax, n)` flip to n×n in one PR, no warning release (phase 2).
- `freqz` includes Nyquist (phase 8).
- Dense `norm(A)` becomes the 2-norm (phase 9). Sparse `norm(S)` stays Frobenius for the size reason above.
- The `1e-15` sparse drop becomes exact-zero only, with a Laplacian timing gate (phase 10).
- `diag` of length 1 becomes a 1×1 matrix (phase 1).
- `sort` is stable unless the 2× gate fails, in which case the PR stops and reports the number instead of quietly keeping unstable order.
- `std` / `var` gain `w` and the weight-vector form (phase 1).
- `interp1` gains `pchip`, `makima`, and a numeric fill without retuning `"spline"` (phase 7).
- Scalar-numerator `/` becomes `mrdivide` (phase 3). Array divided by a scalar stays element-wise, which is also MATLAB.

## Open question

1. **The word "MATLAB" in shipped help.** POL1 and the S-parameter plan say REPL help and `docs/functions.md` should not name MATLAB. Implementation PRs will describe the shapes and flags in rustlab's words unless you want those pages to say MATLAB. This plan file is the spec and keeps the name.

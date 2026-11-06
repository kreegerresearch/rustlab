# MATLAB compatibility migration

**Status:** record only. This branch is the plan. It does not change the interpreter, the docs users read, or the workspace version.
**Current phase:** phases 1–7 and 9–11 are on `main`. Phase 8 (`freqz`) and the `eigs` `D` shape are the exceptions. See [Still open](#still-open).
**Checked against:** `origin/main` `42b5370`, workspace **0.3.58**, PR #138. That commit merged into this branch on 2026-11-06 (merge, not rebase). The merge was clean. The only file this branch adds is this one.
**First written:** 2026-10-29, against 0.3.24 (`9e7c6ad`, PR #100), when every phase below was still "not started".
**Date of this record:** 2026-11-06.

**Direction (Michael, 2026-10-29):** match MATLAB whenever that is possible, including `\`. A behavior that is not MATLAB's stays only with a quantified performance reason in this file. Core math stays hand-rolled Rust. No new dependencies.

This refresh does not bump `Cargo.toml`. A plan rewrite does not change what a script does, so it is not a release. Draft PR #139 (`cursor/coding-guidelines-adfd`) is the draft that adds `docs/coding-guidelines.md` and claims 0.3.59. That file is not part of this branch. Script style for `.rlab` is already on `main` in [`docs/agent-guide.md`](../../docs/agent-guide.md) (PR #134, 0.3.54).

Live behavior is in [`CHANGELOG.md`](../../CHANGELOG.md), [`docs/functions.md`](../../docs/functions.md), and [`docs/quickref.md`](../../docs/quickref.md). [`octave_compat_divergences.md`](octave_compat_divergences.md) already records the same shipped rows. This file is the phase map and the remaining decisions. It does not restate the function reference.

## Shipped phase map

PR numbers below are the merged pull requests (`gh pr view`), not the October ordering notes. Versions are the `CHANGELOG.md` sections. The squash subject of #104 says `(0.3.26)`; the `Cargo.toml` in that commit, and the changelog, are **0.3.27**.

| Phase | What landed | PR | Version |
|---|---|---|---|
| baseline | `A \ b`, unpivoted `qr`, dense `lu` with a permutation matrix, one-output `polyfit` / `polyval`, `interp1` linear / nearest / previous / next / not-a-knot spline, `null` / `orth` | #100 | 0.3.24 |
| 1 | `std` / `var` weight and dimension, empty → NaN; `median` rejects complex and empty is NaN; `sort` by magnitude then phase, stable, `sort(M, dim)`; `trace` requires square; `diag` of one element is 1×1 | #102 | 0.3.25 |
| 2 | `zeros(n)` / `ones(n)` / `rand(n)` / `randn(n)` / `randi(imax, n)` are n×n. A row is `zeros(1, n)` and stays a matrix | #113 | 0.3.26 |
| 3 | `A .\ B` is `B ./ A`. A scalar numerator `2 / A` is right division, via conjugate transpose `(A' \ B')'` | #104 | 0.3.27 |
| 4 | `[Q, R, p] = qr(A)` column-pivots. `p` is a 1-based vector. `"matrix"` returns `P` | #105 | 0.3.28 |
| 5 | `\` reads a vector as a column (one solution is n×1). Square singular stays on LU (`Inf` and the singular warning). Square sparse uses `spsolve`. Rectangular stays the basic QR solution. Rectangular sparse densifies only up to 2,000,000 entries | #106 | 0.3.29 |
| 6 | Dense `chol`. `[L, U, p] = lu(A, "vector")`. Hermitian positive-definite `\` uses Cholesky. A triangular `\` uses a triangular solve | #107 | 0.3.30 |
| 7 | `[p, S, mu] = polyfit`. `interp1` `"pchip"` / `"makima"` and a numeric fill | #108 | 0.3.31 |
| 8 | `freqz` includes Nyquist | — | **not shipped.** Closed PR #109. See [freqz](#9-freqz-and-nyquist) |
| 9 | Dense `norm(A)` with no `p` is the 2-norm. Sparse `norm(S)` stays Frobenius | #110 | 0.3.40 |
| 10 | Sparse storage drops only an exact zero | #111 | 0.3.32 |
| 11 | `xcorr` and `xcov` | #112 | 0.3.33 |

Follow-ups that changed a shipped phase, still on `main`:

| Follow-up | PR | Version |
|---|---|---|
| `interp1` non-finite queries, and dense `chol` of a non-real diagonal (a scalar `chol(4)` is `2`) | #114 | 0.3.34 |
| Constructors refuse a size that cannot be allocated (`2^48 − 1` elements, byte length fits in `isize`) | #118 | 0.3.35 |
| `std(Inf)` / `var(Inf)` are NaN; `sort` treats `-0` and `+0` as equal; `xcorr([])` is an empty row; matrix `xcorr` accepts only scale `"none"`; `lu(..., "vector")` returns a 1×n row | #119 | 0.3.39 |
| Dense 2-norm stops after a Lanczos cap (a 512×512 that used to take minutes). `xcorr` / `xcov` reject a `maxlag` that does not fit, instead of panicking | #122 | 0.3.42 |
| A 1×n or n×1 matrix is a subscript. A 1×1 matrix is a scalar where a scalar is accepted | #124 | 0.3.44 |

Later MATLAB-facing work that this October list did not contain, also on `main`:

| What | PR | Version | Live doc |
|---|---|---|---|
| Logical arrays for comparisons and masks | #137 | 0.3.56 | [`docs/functions.md`](../../docs/functions.md) § Logical arrays |
| One cell of a logical array stays logical. Scalar `true` selects the first element; `false` selects nothing | #138 | 0.3.58 | same, and the 0.3.58 changelog |
| `ind2sub` / `sub2ind` | #136 | 0.3.57 | [`docs/functions.md`](../../docs/functions.md), [`examples/linalg/ind2sub.rlab`](../../examples/linalg/ind2sub.rlab) |

## Numbers the October notes mixed up

Checked with `gh pr view`. The merged phase branches were deleted. Two closed branches are still on the remote; neither is a dependency of this plan.

| Note | What that number actually is |
|---|---|
| #103 constructors, about 0.3.3x | **Closed, not merged** (2026-10-30). Title claimed 0.3.25. Superseded by **#113 in 0.3.26**. Branch `cursor/matlab-phase2-constructors-1f33` is that closed attempt. #113 says it re-applies the same n×n change onto main after #102 had already taken 0.3.25. |
| #110 constructor follow-up, 0.3.40 | **#110 is dense `norm(A)` → 2-norm (0.3.40).** There is no second constructor PR. The square shape shipped once, in #113. |
| #115 `freqz`, 0.3.38 | **#115 is help and impurity drift guards (0.3.36).** It does not change `freqz`. |
| #116 norm | **#116 shares FIR, Bode, and Laplacian helpers (0.3.37).** Internal. Results unchanged. Dense norm is #110. |
| #117 sparse drop, or #117 `mldivide` | **#117 logs scripted axis limits on a log axis (0.3.38).** Sparse drop is **#111 (0.3.32)**. Vector `\` is **#106 (0.3.29)**. |
| #109 `freqz` | **Closed, not merged** (2026-10-30). Branch `cursor/matlab-phase8-freqz-1f33`. The Nyquist grid is not on `main`. See [freqz](#9-freqz-and-nyquist). |

## Already on main before the phases (0.3.24, #100)

Leave the defaults these shipped. Later phases extended them; they did not replace them.

| Shipped in 0.3.24 | Still true on 0.3.58 |
|---|---|
| Unpivoted `[Q, R] = qr(A)`, economy `qr(A, 0)`, one output is `R`, `diag(R)` real and non-negative | Phase 4 added a third output. One and two outputs stay this factor. |
| Dense `[L, U, P] = lu(A)` with `P` a matrix, `P*A = L*U`. `[L, U]` absorbs the pivot. One dense output errors. Sparse `lu` is the factor handle | Phase 6 added `"vector"`. The matrix form is still the default third output. |
| `polyfit` / `polyval` one output: QR, descending powers, Horner, shape of `x` | Phase 7 added `S` and `mu`. One output stays the unscaled fit. |
| `interp1` linear (default), nearest, previous, next, not-a-knot spline. Outside the range: `NaN`, unless `"extrap"` | Phase 7 added `pchip`, `makima`, and a numeric fill. The spline was not retuned. |
| `null` / `orth` orthonormal SVD bases | Signs are not unique. Tests check `A*Z ≈ 0` and `Z'*Z ≈ I`. |

## 1. Single-argument constructors

**Shipped.** PR #113, 0.3.26. Closed PR #103 is not the release.

`zeros(n)`, `ones(n)`, `rand(n)`, `randn(n)`, and `randi(imax, n)` (also `randi([lo, hi], n)`) allocate n×n. A row is `zeros(1, n)` / `randn(1, n)` / `randi(imax, 1, n)` and stays a matrix, not a `Value::Vector`. `rand()` and `randi(imax)` stay scalars. `eye(n)` was already n×n. Integer forms follow the same shape (`zeros(n, "int32")` is n×n). In-tree scripts and `perf/` benches that wanted a length-n sequence pass a row count of 1. `perf/bench_builtins.rlab` uses `randn(1, N)` with `N = 100000`. `bench_convolve.rlab` uses `randn(1, 48000)`.

The deprecation warning in the old octave note was not used. [`roadmap.md`](roadmap.md) C12 is marked done in 0.3.26. [`octave_compat_divergences.md`](octave_compat_divergences.md) item 5 says the same.

**Why the kernel did not change.** PR #103 measured `a .* a` for a length-n vector against a real 1×n matrix, release, no `[widen]` or `[promote]`: 1.02× at n = 1e5 (0.589 ms vs 0.603 ms) and 0.96× at n = 1e6 (7.44 ms vs 7.13 ms). #113 re-applied that shape change and did not add a special case in `real_ops`. `zeros(1, n)` remains a matrix ([`closed/matrix_vector_coercion.md`](closed/matrix_vector_coercion.md)).

Impossible sizes are a later, separate fix: PR #118, 0.3.35. `zeros(2^32, "int8")` errors before allocating. The cap is `2^48 − 1` elements, and the byte length must fit in `isize`.

## 2. Vector on the right of `\`

**Shipped.** PR #106, 0.3.29, together with [section 14](#14-underdetermined--is-the-basic-solution).

A `Value::Vector` on the right of `\` is a column. One solution is n×1. An explicit one-row matrix stays a row and does not match a tall `A`. A scalar on the left still divides element-wise. `linsolve` uses the same path. A real n×1 column combined with a same-length vector is element-wise (`x .* h` has length n; it does not expand to n×n). `x * h` is still the outer product. Docs: [`docs/functions.md`](../../docs/functions.md) `mldivide`.

## 3. `norm(A)` with no `p`

**Shipped for dense matrices.** PR #110, 0.3.40. The iteration bound is PR #122, 0.3.42.

Dense `norm(A)` and `norm(A, 2)` are the largest singular value (Lanczos on the smaller of `A'A` and `AA'`, no singular vectors). `norm(A, "fro")` is the one-pass Frobenius norm, which is what an unqualified dense `norm(A)` used to return. A vector was already the 2-norm. Empty is 0. `NaN` in the matrix makes the 2-norm `NaN`; `Inf` with no `NaN` makes it `Inf`. `svd` still errors on a non-finite matrix.

PR #122 capped the Lanczos. The changelog records a random 512×512: **256 s** in a debug build before the cap, **0.27 s** debug and **0.02 s** release after. Sides up to 96 can still fall back to a Gram Jacobi. Past that, the Ritz value is the answer. Ctrl+C during the computation returns to the REPL.

**Sparse `norm(S)` stays Frobenius.** That is the quantified exception, and it is what [`docs/functions.md`](../../docs/functions.md) and [`octave_compat_divergences.md`](octave_compat_divergences.md) say now.

`norm(S, 2)` densifies. A side longer than 512 is an error. A 512×512 complex Gram is `512² × 16` bytes, about 4 MB. A 2-D Laplacian on a 2000×2000 grid has `n = 4×10⁶` nodes. Dense storage for a 2-norm of that matrix is `n² × 16` bytes, about 256 TB, so the call cannot run. Frobenius is one pass over the nonzeros. There is still no sparse 2-norm in tree that avoids the densify.

## 4. `[V, D] = eigs(...)` and `D`'s shape

**Not shipped.** This was never one of the numbered phases. It is still open. See [Still open](#still-open).

On 0.3.58, `[V, D] = eigs(...)` returns `D` as a length-k vector. `D(k)` is the k-th eigenvalue. `docs/functions.md` says that. Dense `eig` is different: two outputs already return a diagonal matrix, with `"vector"` / `"matrix"` overrides (that surface shipped in 2026-05, octave item 8).

The 2026-10-29 text recommended a k×k diagonal for `eigs`, plus the same `"vector"` flag, so the default would match MATLAB. [`octave_compat_divergences.md`](octave_compat_divergences.md) on current `main` still lists the vector as intentional: a diagonal would make `D(2)` a structural zero under column-major linear indexing, and the examples use `D(k)`. No pull request after 2026-10-29 changed it. Implicit restart and shift-invert are still deferred in `dev/plans/em_requests_queue.md`; they are a different piece of work.

## 5. `sort` and `median`

**Shipped.** PR #102, 0.3.25. Edges in #119, 0.3.39.

Real `sort` is numeric order. Any nonzero imaginary part orders by magnitude, then phase. NaNs sort above `+Inf`. Ties keep input order, so the stable sort shipped; the 2× timing gate in the old text did not stop it. `-0` and `+0` compare equal (0.3.39) and the stable sort keeps the sign bit. `sort(M)` sorts each column. `sort(M, dim)` and `sort(A, dim, "descend")` select the axis and the direction. A vector is a row.

`median` of a nonzero imaginary part is an error. `median([])` is NaN. Real medians, including the even-length average, stayed.

## 6. `std` / `var`

**Shipped.** PR #102, 0.3.25. `std(Inf)` / `var(Inf)` in #119, 0.3.39.

`std(x)` is still the sample form (divide by `n − 1`). `std(x, 1)` divides by `n`. A nonnegative weight vector divides by the sum of the weights. The dimension is the third argument: `std(M, 0, 2)`. `std(M, 2)` is an error, because `2` is not a weight. Empty input is NaN. One finite observation is 0. `std(Inf)` and `var(Inf)` are NaN. Complex data is still `Σ|x − mean|²` over the same divisor. `var` is `std` before the square root.

## 7. `trace`

**Shipped.** PR #102, 0.3.25. A non-square matrix is an error. A square trace is the same sum. A scalar stays a scalar.

## 8. `diag` of one element

**Shipped.** PR #102, 0.3.25. `diag(5)` and `diag([5])` are 1×1 matrices, `size` `[1, 1]`, entry 5. `diag([1, 2])` is still 2×2. The 1×1 collapse in `kron` was not part of this change. A 1×1 matrix still adds and multiplies with a scalar. PR #124 (0.3.44) later made a 1×1 matrix a scalar wherever a scalar is accepted; `size` of that 1×1 stays `[1, 1]`.

## 9. `freqz` and Nyquist

**Not shipped.** The grid on `main` is still the 0.3.23 grid.

`n` points, `f_k = k · fs / (2n)`, on `[0, Nyquist)`. Nyquist itself is not a sample. The return is still a 2×n matrix (row 1 frequencies, row 2 complex H). `plotdb` reads those two rows. [`docs/functions.md`](../../docs/functions.md) `freqz`, [`docs/quickref.md`](../../docs/quickref.md), and the `freqz` row in [`octave_compat_divergences.md`](octave_compat_divergences.md) all say this. Octave's `freqz` uses the same half-open grid, which is why the octave harness can still check it.

The 2026-10-29 decision was to include Nyquist: for `n > 1`, `f_k = k · fs / (2(n − 1))`, last point `fs/2`, `n = 1` is Nyquist alone, return stays 2×n. PR #109 implemented that on a branch from 0.3.24 and was **closed without merge** on 2026-10-30. Its body records why the 10% gate failed:

| | 64-tap `freqz`, `n = 4096` | 9000-tap direct sum, `n = 64` | last bin at `fs = 8000` |
|---|---:|---:|---:|
| 0.3.30 (shipped grid) | 0.298 ms | 10.233 ms | 3999.023 Hz |
| #109 grid | 1.985 ms, repeat 1.998 ms | 8.512 ms, repeat 8.978 ms | 4000 Hz |

The direct-sum path was inside the gate. The FFT path was about **6.7×**. `n = 4096` had been an 8192-point radix-2 FFT. Length `2(n − 1) = 8190` is not a power of two, so the transform is Bluestein. No power-of-two FFT has those exact bins.

`cursor/matlab-phase8-freqz-1f33` is that closed branch. It is not on `main`, it is not a base for this plan, and merging it would bring the 0.3.24-era patch plus the Bluestein cost. A later attempt starts from current `main` and re-measures. The open choice is in [Still open](#still-open).

## 10. Butterworth, one output

**Unchanged, on purpose.** One output of `butterworth_lowpass` / `butterworth_highpass` is the numerator. Two outputs return `[b, a]`. That already matches MATLAB nargout. There is no `butter` builtin and no normalized `Wn`. Callers pass Hz and a sample rate. A silent switch to a fraction of Nyquist would design a different filter. An alias waits until someone asks for the `Wn` form. [`octave_compat_divergences.md`](octave_compat_divergences.md) records the one-output row.

## 11. Sparse drop below `1e-15`

**Shipped.** PR #111, 0.3.32. Not #117.

An entry is structural only when it is an exact zero, including `+0` and `-0`, and including a cancellation to an exact zero. `nnz(1e-16 * speye(n))` is `n`. NaN and Inf stay stored. The relative pivot tests inside sparse `chol` and `lu` stayed, so `1e-14 * speye(n)` still factors and a genuinely singular matrix still fails. The old "fill becomes O(n²)" reason for the `1e-15` cutoff was not what a 5-point Laplacian depends on: that pattern does not store those entries, so its `nnz` does not move when the cutoff goes away.

## 12. `.\` and scalar-numerator `/`

**Shipped.** PR #104, 0.3.27.

`A .\ B` is element-wise `B ./ A` on the existing `./` kernel, including broadcasting. A scalar `a .\ b` is `b / a`. A scalar on the left of `/` (`2 / M`, `2 / v`) is right division. A scalar denominator stays an element-wise scale. Two vectors with `/` are still an error; use `./` or `.\`. Right division is `(A' \ B')'`. The 0.3.24 reduction used `.'`. Real matrices are unchanged. `eye(2) / [1, j; 0, 1]` is `[1, -j; 0, 1]`.

## 13. `[Q, R, p] = qr`

**Shipped.** PR #105, 0.3.28.

Three outputs column-pivot and return a 1-based permutation vector, `A(:, p) = Q*R`. `"vector"` is the same vector. `"matrix"` returns `P` with `A*P = Q*R`. Economy plus three outputs is the thin factor plus `p`. One and two outputs stay the unpivoted factor from 0.3.24. MATLAB's unmarked three-output form is a permutation matrix; rustlab's unmarked three-output form is the vector, and `"matrix"` is how you ask for `P`. That is what this plan asked for, and [`octave_compat_divergences.md`](octave_compat_divergences.md) records it.

## 14. Underdetermined `\` is the basic solution

**Shipped** with phase 5, PR #106, 0.3.29.

Rectangular `\` is QR with column pivoting and free variables 0. It is not the minimum-norm solution. MathWorks documents that, and points minimum-norm at `lsqminnorm` or `pinv`. `lsqminnorm` was not added. `pinv` remains the min-norm function. Octave's underdetermined `\` is minimum-norm, so the octave harness is not the oracle.

The locked vectors are in `underdetermined_backslash_is_the_basic_solution` ([`crates/rustlab-script/src/solve_factor.rs`](../../crates/rustlab-script/src/solve_factor.rs)):

- `A = [1 2 0; 0 4 3]`, `b = [8; 18]` → `[0; 4; 2/3]`.
- `R = [6 8 7 3; 3 5 4 1]`, `b = [7; 8]` → `[0; 17/7; 0; -29/7]`.

The same file checks that the first vector differs from `pinv(A)*b`. `singular_square_backslash_divides_by_zero` checks `[1 0; 0 0] \ [1; 1]` → `[1; Inf]` and the warning `matrix is singular to working precision`.

Square sparse `A \ b` calls `spsolve` and does not densify. Rectangular sparse has no sparse QR in tree. SuiteSparseQR is C++ and is out of the pure-Rust rule. Those systems densify only when `m * n <= 2_000_000` (`RECT_SPARSE_CELLS` in [`crates/rustlab-script/src/eval/linsys.rs`](../../crates/rustlab-script/src/eval/linsys.rs)), about 32 MB of complex entries. A larger rectangular sparse `\` errors and names the cap. That cap is the quantified reason there is no sparse QR.

## 15. Dense `chol`, and `lu(..., "vector")`

**Shipped.** PR #107, 0.3.30. Edges in #114 (0.3.34) and #119 (0.3.39).

`R = chol(A)` is upper triangular, `A = R'*R`. `chol(A, "lower")` returns `L`. One output errors when a pivot is not positive, including a diagonal that is not real (0.3.34). `[R, p] = chol(A)` does not error: `p` is 0 on success, otherwise the 1-based failing pivot. A scalar is 1×1, so `chol(4)` is `2`. Sparse `chol` and sparse `lu` still return factor handles for `solve(F, b)`.

`[L, U, p] = lu(A, "vector")` returns a 1-based row, `size` `[1, n]` (0.3.39), with `A(p, :) = L*U`. `[L, U, P] = lu(A)` is still the permutation matrix.

Square `\` uses Cholesky when the matrix is Hermitian positive-definite, and a triangular solve when it is triangular. A Hermitian matrix that is not positive definite stays on LU. The value of a successful Cholesky solve can move in the last digits versus the old LU result. Rectangular `\` does not go through Cholesky.

The handle stays for sparse factors because the numeric Cholesky of a 2-D grid is the expensive part (`O(n^{1.5})`) and each `solve` is much smaller. Returning only a numeric factor would repeat that factor on every right-hand side.

## 16. `polyfit` `[p, S, mu]`

**Shipped.** PR #108, 0.3.31.

One output is the unscaled fit. `[p, S]` adds `S.R`, `S.df`, `S.normr` and the same `p`. `[p, S, mu]` changes `p`: `mu = [mean(x), std(x)]` with the sample standard deviation, and the coefficients are for the centered variable. `polyval(p, x, S, mu)` undoes that. `S` is not used for the value. A Vandermonde larger than 100,000,000 entries errors before it is allocated. Docs: [`docs/functions.md`](../../docs/functions.md) `polyfit` / `polyval`.

**Still absent, and not requested again:** the `polyval` error-estimate output (`delta`). The 2026-10-29 text left it waiting. [`docs/functions.md`](../../docs/functions.md) says `S` is not used for the value.

## 17. `interp1` pchip, makima, and a numeric fill

**Shipped.** PR #108, 0.3.31. Non-finite queries in #114, 0.3.34.

`"pchip"` is Fritsch–Carlson. `"makima"` is modified Akima. A numeric fifth argument fills queries outside the range. `"extrap"` still extends the end piece. Linear, nearest, previous, next, and the not-a-knot spline stayed. A `NaN` query stays `NaN` (the fill does not replace it). `Inf` is outside the interval.

## 18. `Vector` versus `Matrix`

**Still two types.** This is the representation exception, and the measurements are unchanged.

[`real_storage.md`](real_storage.md) counts about 1,150 `Value::Vector` / `Matrix` / `Tensor3` match sites. Real storage shipped in 0.3.17 (perf flywheel iteration 03). [`perf_flywheel_iter03.md`](perf_flywheel_iter03.md) records the training step at 55.8 ms before that storage and 28.8–29.1 ms after. [`AGENTS.md`](../../AGENTS.md) summarizes the perf-check band as 54–56 ms → 29–35 ms. Collapsing the two types would put every DSP buffer on the matrix arms and risks a `[widen]` on the kernels that produced that speedup.

MATLAB's visible shapes were applied locally, and those local rules have shipped: `size` of a vector is `[1, n]`, `zeros(n)` is n×n (phase 2), a vector on the right of `\` is a column (phase 5), and a 1×n or n×1 matrix is a legal subscript (PR #124, 0.3.44). None of those merged the types. `zeros(1, n)` is still a matrix.

## 19. `xcorr` and `xcov`

**Shipped.** PR #112, 0.3.33. Edges in #119 (0.3.39) and the overflow check in #122 (0.3.42).

Zero lag is in the middle. Positive lag `m` is `sum_n x(n+m) conj(y(n))`. Length is `2N−1` when `maxlag` is omitted (`N` is the longer input). `[r, lags]` returns lags from `-maxlag` to `+maxlag`. `scale` is `"none"` (default), `"biased"`, `"unbiased"`, or `"normalized"` / `"coeff"`. A vector stays a row. An n×1 matrix returns a column. Several columns are one signal each; the result has one column per pair, and a multi-column matrix accepts only scale `"none"`. `xcorr([])` is an empty row. `xcov` removes each signal's mean first. `convolve` is still linear convolution and was not overloaded.

A `maxlag` whose `2*maxlag+1` overflows or exceeds the array cap is an error (`requested … array exceeds the maximum possible variable size`), not a panic. Docs: [`docs/functions.md`](../../docs/functions.md) `xcorr`, [`docs/quickref.md`](../../docs/quickref.md).

## Kept, with a quantified reason

These are the non-MATLAB behaviors this record still accepts. Each one is also what current `main` does.

| Item | Why it stays | Where it is written now |
|---|---|---|
| `Vector` and `Matrix` stay two types | Training step 55.8 ms → 28.8–29.1 ms with real 1-D storage (0.3.17), and about 1,150 match sites. Shapes were fixed locally (phases 2 and 5, and #124). | [`real_storage.md`](real_storage.md), [`perf_flywheel_iter03.md`](perf_flywheel_iter03.md) |
| Sparse `norm(S)` stays Frobenius | A 2000×2000 grid is `4×10⁶` nodes. Dense storage for a 2-norm is about 256 TB. `norm(S, 2)` errors when a side is over 512. Dense `norm(A)` did change (#110). | [`docs/functions.md`](../../docs/functions.md) `norm` |
| Rectangular sparse `\` densifies under a cap | No sparse QR in tree. SuiteSparseQR is C++. Cap `m*n <= 2_000_000`. Square sparse `\` uses `spsolve` (#106). | [`linsys.rs`](../../crates/rustlab-script/src/eval/linsys.rs) `RECT_SPARSE_CELLS` |
| Sparse `chol` / `lu` return a handle | Factor once. A 2-D grid Cholesky is `O(n^{1.5})`; each `solve` is much smaller. Dense `chol` is a separate overload (#107). | [`docs/functions.md`](../../docs/functions.md) `chol` / `lu` |
| Butterworth arguments stay Hz + sample rate | One output is already the numerator. Normalized `Wn` would design a different filter. | [`octave_compat_divergences.md`](octave_compat_divergences.md) |
| `null` / `orth` signs | The subspace is defined. A particular sign is not. | same file |
| `\` and `polyfit` use QR | Normal equations square the condition number. | #100, #106, #108 |
| `freqz` stays a 2×n matrix | `plotdb` reads row 1 and row 2. The Nyquist question is only the sample locations. | [`docs/functions.md`](../../docs/functions.md) `freqz` |
| 1×n real multiply needed no new kernel | #103 measured 1.02× at 1e5 and 0.96× at 1e6, no `[widen]`. #113 kept that. | phase 2 above |

## Later features: limits `main` already documents

These shipped after the October list. They are not unfinished phases of that list. The limits below are the current contract, written in the changelog and in [`docs/functions.md`](../../docs/functions.md). This plan does not schedule them.

| Limit | Since | Where |
|---|---|---|
| Writing into a logical array (`L(i) = false`) is not supported | 0.3.56, #137 | `docs/functions.md` indexed assignment; CHANGELOG 0.3.56 |
| `~=` is not a token, and there is no ampersand or bar operator token. `!=`, `and`, `or`, and `xor` are the element-wise forms. `!` is what MATLAB writes as `~` | 0.3.56, #137 | `docs/functions.md` § Logical arrays |
| `parmap` packs scalar bools and numeric arrays. It does not pack a logical array returned from a worker | 0.3.56, #137 | CHANGELOG 0.3.56. The `parmap` section of `docs/functions.md` describes scalar, vector, and matrix packing |
| `ind2sub` / `sub2ind` error when the index is outside the array. MATLAB and Octave `ind2sub` do not range-check; they can return a subscript past the last dimension | 0.3.57, #136 | `docs/functions.md` `ind2sub` |
| A logical element is one byte, not a bit pack. A release scan of 1e6 flags (about one third set), best of five: count 364 µs byte vs 92 µs packed; true-position list 1.10 ms vs 695 µs. Bytes stay | 0.3.56, #137 | `docs/functions.md` § Logical arrays, `rustlab-core` `LogicalArray` |

`max` / `min` of complex values still break ties by first occurrence. MATLAB breaks equal magnitudes by phase. That difference is older than this migration and is already stated in `docs/functions.md`. It was not a phase here.

## Still open

Two items from the 2026-10-29 text did not land. One wording question was left open then and is still open.

1. **`freqz` grid.** `main` samples `[0, Nyquist)` at `k · fs / (2n)` and documents that as the length-`2n` DFT bins, including when `2n` is not a power of two. The 2026-10-29 decision was to include Nyquist with spacing `fs / (2(n − 1))`. PR #109 did that and closed unmerged because the FFT path measured about 6.7× (0.298 ms → 1.985 ms at `n = 4096`) on a Bluestein length. The choice still to make: keep the half-open grid, accept that Bluestein cost, or hit Nyquist with an evaluation whose length stays a power of two. The return stays 2×n in every option. Do not merge `cursor/matlab-phase8-freqz-1f33`.

2. **`eigs` `D`.** `main` returns a vector so `D(k)` is the k-th eigenvalue, and [`octave_compat_divergences.md`](octave_compat_divergences.md) calls that intentional. The 2026-10-29 text recommended a diagonal plus a `"vector"` flag, which is what dense `eig` already does. Those two documents disagree. This record does not pick a side and does not start the change.

3. **The word "MATLAB" in shipped help.** [`closed/sparameters.md`](closed/sparameters.md) ("MATLAB-as-reference policy") and POL1 in [`examples_notebooks_bug_hunt.md`](examples_notebooks_bug_hunt.md) say shipped artifacts should not name MATLAB. POL1 is still an open checkbox there, aimed at `pwelch` / `stft` / `cwt`. [`docs/functions.md`](../../docs/functions.md) does name MATLAB in several places (gradient weights, `max` tie-break, `pwelch` / `stft` defaults, `ind2sub` range check, `lu` permutation shape, logical `~`, `imagesc`). This plan file keeps the name because it is the spec. Implementation docs were not edited here.

Explicitly left waiting, and still absent, so they are not blocked decisions: `polyval`'s `delta` output, a `butter(n, Wn)` alias, and `lsqminnorm`. `\` stays the basic solution.

## Closed branches that are not dependencies

| Branch | PR | State |
|---|---|---|
| `cursor/matlab-phase2-constructors-1f33` | #103 | Closed 2026-10-30. Superseded by #113, which is on `main` at 0.3.26. Do not revive the branch. |
| `cursor/matlab-phase8-freqz-1f33` | #109 | Closed 2026-10-30, never merged. The Nyquist grid is not on `main`. The branch is stale (cut from 0.3.24) and is not a base for the next attempt. |

Every other `cursor/matlab-phase*-1f33` branch from this series was merged and deleted (#102, #104, #105, #106, #107, #108, #110, #111, #112).

## Docs this record agrees with

Checked on 2026-11-06 against the merged `main`. This plan does not edit them.

| Document | Agreement |
|---|---|
| [`CHANGELOG.md`](../../CHANGELOG.md) 0.3.25 through 0.3.44, 0.3.56 through 0.3.58 | The phase map above. |
| [`docs/functions.md`](../../docs/functions.md), [`docs/quickref.md`](../../docs/quickref.md) | Current signatures for `std`, `qr`, `\`, `chol`, `lu`, `polyfit`, `interp1`, `norm`, `freqz`, `xcorr`, `eigs`, logical arrays, `ind2sub`. |
| [`octave_compat_divergences.md`](octave_compat_divergences.md) | Item 5 and the "what remains different on purpose" table match shipped behavior, including the vector `D` from `eigs` and the half-open `freqz` grid. The 2026-10-29 file had meant to supersede those two rows. It did not: they are the open items above. |
| [`roadmap.md`](roadmap.md) C12 | Done in 0.3.26. |
| [`real_storage.md`](real_storage.md), [`perf_flywheel_iter03.md`](perf_flywheel_iter03.md) | The type split and the training-step numbers. |
| [`docs/agent-guide.md`](../../docs/agent-guide.md) § Script style for agents | On `main` via #134. Not this branch. |
| Draft #139, `docs/coding-guidelines.md` | Not this branch. That draft also edits `Cargo.toml`, `CHANGELOG.md`, and `AGENTS.md` for 0.3.59. This branch does not. |

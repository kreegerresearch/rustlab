# Octave/MATLAB Compatibility Divergences

**Status:** Mostly shipped — 10 of 11 divergences addressed (see snapshot below). Only #5 (`zeros(n)` returns 1×n instead of n×n) remains open.
**Date opened:** 2026-05-02
**Source:** Audit run at HEAD `6fba40b`. Numeric baseline: `bash tests/octave/run_compare.sh` — all 150 cases (compare.m + compare_full.m) pass at machine precision (max err ≤ 4.4e-16). Behavioral divergences below were found by spot-checking matlab/octave idioms that the numeric suite doesn't exercise.

## Status snapshot

| # | Divergence | Severity | Status |
|---|---|---|---|
| 1 | Matrix literal requires commas; spaces rejected | High (parser) | **shipped 2026-05-02** |
| 2 | `sum(M)` collapses to scalar instead of column-reducing | **High** | **shipped 2026-05-02** |
| 3 | `mean`/`max`/`min`/`std` on matrix collapse to scalar | **High** | **shipped 2026-05-02** (sum/mean/max/min/prod/std/median/cumsum) |
| 4 | `sum(M, dim)` axis-selector form not supported | **High** | **shipped 2026-05-02** (all reducers + max/min via `[]` placeholder + argmin/argmax dim) |
| 5 | `zeros(n)` returns `1×n` row vector instead of `n×n` | **High** | open |
| 6 | `length(M)` returns `nrows` instead of `max(nrows, ncols)` | High | **shipped 2026-05-02** |
| 7 | Matrix + row/column vector implicit expansion errors | High | **shipped 2026-05-02** |
| 8 | Eig family: `eig` and `eigsys` are split, no dense generalized `eig(A, B)`, `D` shape, eigenvalue orientation, `eigsys` correctness bug — see §8 detail | High | **shipped 2026-05-02** (PR-1 through PR-6 all landed; eig now matches matlab's full surface) |
| 9 | `find(M)` on dense matrix errors (sparse-only) | Medium | **shipped 2026-05-02** (single-output form; multi-output `[I, J, V] = find(M)` deferred until nargout) |
| 10 | `v(2:3) = []` and `M(i, :) = []` deletion errors | Medium | **shipped 2026-05-02** (vector + matrix row/column forms) |
| 11 | `sort(v, "descend")` 2-arg form not supported | Medium | **shipped 2026-05-02** (string-flag form; numeric-dim form deferred until vector-type unification) |

## Things that already match (no work needed)

Confirmed by audit + numeric suite. Listed here so a future reader doesn't go re-checking these:

- 1-based indexing throughout
- Column-major storage
- `mod` sign convention (`mod(-7, 3) == 2`)
- `linspace(a, b, 1)` returns `[b]`
- `ndims` always returns 2 (octave convention; documented at `builtins.rs:1471`)
- `reshape` flattens column-major (documented at `builtins.rs:1279`)
- `end` keyword in indexing (`A(end)`, `A(end-1:end)` work)
- `size(M)` returns `[nrows, ncols]`
- `sort(v)` ascending default
- 1-based `argmin` / `argmax`
- `j` as imaginary unit
- `imagesc(M)` y-axis orientation — row 1 at the top, y-axis labels read `0` at the top and `nrows` at the bottom (image convention with reversed y-axis). Matches MATLAB / Octave `imagesc` exactly across both SVG and HTML/Plotly backends. **Fixed 2026-05-16** — earlier versions rendered row 1 at the top but labeled the y-axis bottom-up (`0` at bottom, `nrows` at top), so the conventions silently disagreed. Note: `contour(X, Y, F)` follows physics convention (uses its own X/Y vectors) — overlaying contour on imagesc requires being mindful of the convention difference, same as MATLAB.
- All 150 numeric octave-compare cases pass at ≤4.4e-16

## Per-item detail

### 1. Matrix literal requires commas ✅ shipped 2026-05-02

The lexer now tracks `[...]` and `{...}` depth. When whitespace appears between an operand-ending token (number, identifier, string, `)`, `]`, `}`, `'`, `.'`, `end`) and the start of a new operand, it emits a synthetic `Comma` so the parser sees the same shape as the explicit-comma form.

The unary `+`/`-` disambiguation matches octave: `[1 -2]` is `[1, -2]` (whitespace before `-`, no whitespace after → unary) while `[1 - 2]` is `[-1]` (whitespace on both sides → binary). The parser also gained a unary `+` pass-through so `[1 +2]` works.

Inside `{...}` (string-array literals) the same rule fires, so `{"a" "b" "c"}` is now equivalent to `{"a", "b", "c"}`.

7 new in-process tests cover space-separated rows, semicolon-separated 2-D matrices, both flavours of unary minus, unary plus, the binary-minus-with-spaces-on-both-sides case, and the brace-literal form. All 148 octave-compare cases still pass.

### 2/3/4. Matrix axis reductions — partial ✅ shipped 2026-05-02

`sum`, `mean`, `prod`, `max`, `min` now follow the octave/matlab "first non-singleton dim" reduction rule on matrix input:

- `sum(M)` for an `M×N` matrix with `M > 1`, `N > 1`: returns a `1×N` row matrix of column sums (default = dim 1).
- `sum(Vector)` or `sum(Matrix(1, N))` or `sum(Matrix(N, 1))`: scalar (the 1-D-shaped reduction). matlab's `sum(sum(M))` idiom for "total" still works.
- `sum(M, 1)` and `sum(M, 2)`: explicit dim selector. `dim=1` reduces columns, `dim=2` reduces rows. Same for `mean` and `prod`.
- `min(M)` / `max(M)`: column min/max → row matrix. **`min(M, dim)` and `max(M, dim)` not yet supported** — the 2-arg form `min(a, b)` (elementwise scalar comparison) ambiguates with the dim arg in matlab too (matlab uses `min(M, [], 2)` with an empty placeholder). The 2-scalar form continues to work; matrix dim selector deferred.

Two helpers `parse_reduction_dim` and `complex_to_value` are shared across these reducers.

Tests: 11 in-process tests (sum/mean/prod/max/min default + dim 1/2 forms, `sum(sum(M))` idiom, `min(scalar, scalar)` 2-arg form, error path for invalid dim). 7 new octave-compare cases — `sum(M) default`, `sum(M, 2)`, `mean(M) default`, `mean(M, 2)`, `prod(M) default`, `max(M) default`, `min(M) default` — all match octave at machine precision.

**Update 2026-05-02 (continued):** `std`, `median`, `cumsum` axis reductions also shipped. `median(M)` and `std(M)` produce per-column scalars in a `1×ncols` row; `cumsum(M)` produces a same-shape matrix of running totals along the chosen dim. All three accept the `dim` arg. Two helpers (`median_of_real_slice`, `std_of_slice`) factored out of the per-column logic.

**Update 2026-05-02 (continued):** the remaining axis-form items shipped via path 1 (matlab `[]`-placeholder convention).

- `min(M, [], 1)`, `min(M, [], 2)`, `max(M, [], 1)`, `max(M, [], 2)`: 3-arg form with the empty-matrix placeholder selects the reduction axis. The 2-scalar `min(a, b)` form still works.
- `argmin(M)`, `argmin(M, 1)`, `argmin(M, 2)`, `argmax(M)`, `argmax(M, 1)`, `argmax(M, 2)`: matrix → row vector of per-column argmins (default dim 1) or column vector of per-row argmins (dim 2). Vector and 1-D-matrix inputs continue to return a scalar.
- Two helpers (`is_empty_matrix_placeholder`, `matrix_axis_extremum`, `matrix_axis_argmin_argmax`) factored out of the per-column logic.

5 new in-process tests (axis form for min/max, argmin/argmax default + dim 2, error path for non-empty middle arg) plus three new octave-compare cases land at machine precision.

**Still open in this divergence cluster:**
- Multi-output `[m, i] = min(M)` / `[m, i] = max(M)` for combined value + index. Nargout plumbing is already in place (see §8 PR-3); this is a small follow-on. Same for `[m, i] = min(v)` on a vector.

### 5. `zeros(n)` is a row vector, not a square matrix

```
rustlab> size(zeros(3))
[1 3]               % rustlab returns 1×3 row vector
```

**Octave/matlab:** `zeros(3)` → `3×3` matrix. `zeros(3, 1)` for a column, `zeros(1, 3)` for a row.

#### Why this is deferred

This is a hard breaking change. A user script that today says `randn(1000)` allocates 1,000 cells; after the fix it would allocate 1,000,000. Every example, notebook, gallery, test, and external user script that calls `zeros(N)` / `ones(N)` / `rand(N)` / `randn(N)` with a single integer arg expecting a row vector would silently change behavior — and in the random-matrix case, blow up memory. **Pickup needs a deprecation cycle, not an opportunistic flip.**

#### Pickup roadmap

Three-step plan, ordered for safety:

1. **Add a deprecation warning, but don't change behavior.** Emit `eprintln!("warning: zeros(N) currently returns a 1×N row vector; this will change to N×N in a future release. Use zeros(1, N) for a row vector or zeros(N, N) for a square matrix to make the intent explicit.")` once per process from `builtin_zeros` / `builtin_ones` / `builtin_rand` / `builtin_randn` when called with a single integer arg. Same warning text in each. Use a `OnceCell` or atomic flag so the warning fires once per builtin per process. Land this and let it bake for at least one release cycle so users see it.

2. **Sweep the in-tree call sites.** Known callers as of 2026-05-02 (audit baseline):

   | File | Call | Intended shape |
   |---|---|---|
   | `examples/audio/spectrum_monitor.rlab:59` | `ring = zeros(fft_size)` | row vector — change to `zeros(1, fft_size)` |
   | `examples/stats.rlab:24` | `ones(512)` | row vector — change to `ones(1, 512)` |
   | `crates/rustlab-script/src/tests.rs:856` | `v = zeros(5)` | row vector — change to `zeros(1, 5)` |
   | `crates/rustlab-script/src/tests.rs:868` | `v = ones(4)` | row vector — change to `ones(1, 4)` |
   | `crates/rustlab-script/src/tests.rs:896` | `v = ones(7)` | row vector — change to `ones(1, 7)` |
   | `crates/rustlab-script/src/tests.rs:6417` | `zeros(3);` (no assertion) | n/a — leave |
   | `crates/rustlab-script/src/tests.rs:6425` | `ones(3);` (no assertion) | n/a — leave |

   Re-run `grep -rEn "zeros\([0-9]+\)\|zeros\([a-zA-Z_]+\)\|ones\([0-9]+\)\|ones\([a-zA-Z_]+\)\|randn\([0-9]+\)\|rand\([0-9]+\)"` against `examples/`, `tests/`, `crates/rustlab-script/src/tests.rs`, and `tests/octave/rustlab_full.rlab` at pickup time — the list above could grow.

3. **Flip the default.** Change `builtin_zeros`, `builtin_ones`, `builtin_rand`, `builtin_randn` (and consider `randi`) so a single integer arg returns an `n×n` matrix instead of a `1×n` vector. `builtin_eye` is already `n×n` for one arg — leave unchanged. Drop the deprecation warning. Document the breaking change clearly in the commit message and the next release notes.

#### Code locations

- `crates/rustlab-script/src/eval/builtins.rs:1081` — `builtin_zeros`
- `crates/rustlab-script/src/eval/builtins.rs:1091` — `builtin_ones`
- `crates/rustlab-script/src/eval/builtins.rs:1125` — `builtin_rand`
- `crates/rustlab-script/src/eval/builtins.rs:1143` — `builtin_randn`
- `crates/rustlab-script/src/eval/builtins.rs:4604` — `builtin_eye` (already correct)
- All four use the helper `unpack_size_args(&args, name)` — that function is the disambiguation point.

#### Tests to add at flip time

- `zeros(3)` shape is `[3, 3]` (currently `[1, 3]`).
- `zeros(N, M)` 2-arg form unchanged.
- `eye(3)` continues to be `3×3` identity.
- An octave-compare case `zeros_single_arg` with `csvwrite('ref2_zeros_single.csv', size(zeros(3)))` etc.

### 6. `length(M)` returns wrong dimension ✅ shipped 2026-05-02

`builtin_len` (which both `len` and `length` register against) now returns `max(nrows, ncols)` for a `Value::Matrix`, matching octave/matlab's "longest dimension" convention. `Value::Vector` is unchanged (already returned its length). 3 in-process tests cover the matrix, column-vector-matrix, and vector cases. No octave-compare drift across all 137 cases.

### 7. Implicit expansion (matrix + row/col vector) ✅ shipped 2026-05-02

`Value::binop` now broadcasts:

- Matrix-matrix elementwise (`+`, `-`, `.*`, `./`, `.^`, `.^`) with implicit expansion: each dim must match between the two, or one of them must be `1` (the singleton repeats to fill the other).
- Matrix + Vector (and Vector + Matrix) for the same op set: the `Value::Vector` is promoted to a `1×N` row matrix and then broadcast against the matrix shape — so `M(2×3) + [10, 20, 30]` and `M(2×3) .* [10, 20, 30]` both work.
- Outer-shape ops via singleton broadcasting: `[1; 2] + [10, 20, 30]` → `2×3` matrix.

Implementation: two new helpers on `Value` — `broadcast_pair(a, b)` does the dim-compatibility check and shape expansion, and `elementwise_with_broadcast(a, b, op)` runs the elementwise op on the expanded pair (with the existing real-real-collapses-imag noise rule preserved). The `Vector` arm uses a small `vector_to_row_matrix` helper to promote.

Tests: 5 new in-process tests (M + row, M + col, col + row, `.*` broadcast, incompatible dims error) plus three new octave-compare cases (`bcast M + row`, `bcast M + col`, `bcast col + row`). All match octave at machine precision.

Scalar-vector broadcasting already worked and is unchanged. Vector-Vector broadcasting (without explicit row/col distinction) is also unchanged for now — `Value::Vector` is row-shaped by convention, so `[1, 2, 3] + [10; 20; 30]` works because the `Value::Matrix(3, 1)` lhs triggers the new broadcast path.

### 8. Eig family: collapse to one name and align with octave/matlab

This is the largest item in the plan. Several smaller divergences cluster here, and the cleanest fix is to redesign the family in one PR rather than as eight standalone changes. The shipped `eigsys(A)` (commit `0f3337d`, 2026-05-01) was a stepping stone — it filled the eigenvector gap, but it's a rustlab-only name and matlab users won't find it.

#### Current state

| Form | Today | Notes |
|---|---|---|
| `e = eig(A)` | works | returns `1×N` **row** vector |
| `[V, D] = eig(A)` | **errors** | "multi-assign: expected 2 values, function returned 1" |
| `[V, D] = eigsys(A)` | works | rustlab-only name; D is a row vector |
| `e = eig(A, B)` (dense generalized) | **does not exist** | — |
| `[V, D] = eig(A, B)` (dense generalized) | **does not exist** | — |
| `[V, D] = eigs(A, n)` (sparse partial) | works | D is a vector |
| `[V, D] = eigs(A, B, n)` (sparse generalized) | works | D is a vector |

#### Octave/matlab equivalents

| Form | Octave/matlab |
|---|---|
| `e = eig(A)` | `N×1` **column** vector |
| `[V, D] = eig(A)` | V matrix + D **diagonal matrix** |
| `e = eig(A, B)` | column vector of generalized eigenvalues |
| `[V, D] = eig(A, B)` | V + D diagonal matrix (generalized) |
| `eig(A, "vector")` | force D to vector even with two outputs |
| `eig(A, "matrix")` | force D to diagonal matrix even with one output |
| `[V, D] = eigs(A, n)` | V + D **diagonal matrix** |

#### Sub-divergences

1. **`eig` and `eigsys` are split.** matlab uses `eig` for both 1- and 2-output forms; rustlab forces a name choice. Porting matlab code that does `[V, D] = eig(A)` errors out and the user has to know to rename to `eigsys`.
2. **`eig(A)` orientation.** Returns `1×N` row; matlab returns `N×1` column. Breaks `length(eig(A))` (rustlab returns `nrows == 1`, matlab returns N) and `eig(A) * x` chain expressions. Tied to divergence #6 in this plan.
3. **No dense generalized `eig(A, B)`.** Available only via the sparse path. matlab users porting code expect the dense form to exist for small N where dense is faster than going through sparse machinery.
4. **`D` shape.** matlab's 2-output `D` is a diagonal matrix; rustlab's `eigsys` and `eigs` return D as a vector. The matlab idiom `diag(D)` to extract eigenvalues won't work — users would write `D` directly, but their existing code does `diag(D)`.
5. **`eigsys` exists at all.** Once `eig` overloads, `eigsys` is redundant. Either remove (small breaking change since it just shipped) or keep as a one-release alias to ease the transition.
6. **`inverse_iteration_cx` correctness bug** — discovered during this audit. For upper-triangular (or near-triangular) inputs, the helper starts from `e_0 = [1; 0; ..., 0]`, which lies in the invariant subspace of the inverse for triangular matrices, so iteration never converges to the right eigenvector except for the eigenpair whose eigenvector *is* e_0. Reproducer:

   ```
   A = [4,1,0; 0,2,1; 0,0,5]    % upper triangular, eigs = 4, 2, 5
   [V, D] = eigsys(A)
   % residuals ‖A·V_k − D_k·V_k‖ are 1.0, 0, 2.0 — should all be ~1e-15
   ```

   The core helper at `crates/rustlab-core/src/sparse_eig/hessenberg_eig.rs:241-244` already uses a sine-of-index initial vector (`((i+1)*0.7).sin()`) to dodge this. The local `inverse_iteration_cx` at `crates/rustlab-script/src/eval/builtins.rs:6854-6855` uses `e_0`, which is wrong for triangular inputs. **One-line fix.** Must land before any eig refactor or the new `[V, D] = eig(A)` will inherit the bug.

#### Blocker: nargout awareness

`eig` overloading on output count requires the builtin to know whether the caller is doing `e = eig(A)` (single-output) or `[V, D] = eig(A)` (two-output). Today `BuiltinFn = fn(Vec<Value>) -> Result<Value, ScriptError>` — no nargout context. Three approaches:

- **A. Full nargout refactor.** Change `BuiltinFn` to `fn(Vec<Value>, nargout: usize) -> ...`; thread the assignment context through the evaluator. Touches every builtin signature even though most ignore nargout.
- **B. Two-tier registry.** Keep current `BuiltinFn` as the default; add a second `BuiltinFnNargout` variant for the few that need it (`eig`, `eigs`, `svd`, `sort`, `find`). Registry holds an enum `BuiltinKind { Stateless(BuiltinFn), Nargout(BuiltinFnNargout) }`. Smallest blast radius. **Recommended.**
- **C. Always-tuple + first-element extraction.** Make `eig` always return `Tuple([D, V])`, with the runtime auto-extracting the first element on single-assign. Reverses the `[V, D]` order users expect. Reject.

Option B is contained: changes to the registry, the evaluator's call dispatch site, and just the five or six functions that benefit. The other ~170 builtins keep their current signature.

#### Target API (post-fix)

```
e         = eig(A)               % column vector of eigenvalues
[V, D]    = eig(A)               % V matrix, D diagonal matrix
e         = eig(A, B)            % generalized: A·v = λ·B·v
[V, D]    = eig(A, B)            % generalized eigenvectors and eigenvalues

[V, D]    = eigs(A, n)           % sparse partial — same shape contract
[V, D]    = eigs(A, B, n)        % sparse generalized

% Optional, low priority — flags to override default D shape:
%   eig(A, "vector") → e even with two outputs
%   eig(A, "matrix") → diag(e) even with one output
```

`eigsys` is dropped (or aliased to `eig` for one release with a deprecation warning).

#### Suggested PR sequence

1. **PR-1: Fix the inverse-iteration bug.** ✅ **Shipped 2026-05-02.** Initial vector in `inverse_iteration_cx` switched from `e_0` to the sine-of-index pattern the core helper uses. Regression test `eig_upper_triangular_residuals_near_zero` covers the `[4,1,0; 0,2,1; 0,0,5]` case (residuals all < 1e-9). (Originally landed with the test named `eigsys_…`; renamed when `eigsys` was removed in step 7.)
2. **PR-2: Eigenvalue orientation.** ✅ **shipped 2026-05-02.** `eig(A)` returns an `N×1` column matrix (octave/matlab orientation) instead of a `1×N` row vector.

   PR-2a (vector-type unification) landed alongside this for the most-common idioms — `sort`, `argmin`, `argmax`, `min`, `max` now accept `Matrix(N, 1)` and `Matrix(1, N)` as 1-D-shaped inputs. `sort` preserves the column/row shape on output; argmin/argmax return a scalar 1-based position in storage order; min/max already worked on matrices (flat reduction).

   The full PR-2a sweep (~30 vector-accepting builtins) is still in progress as a follow-on. The currently-shipped subset is enough to unblock the matlab `sort(eig(A))` idiom and similar pipelines. Functions yet to be migrated include `sum`, `mean`, `std`, `prod`, `cumsum`, `median`, `norm`, `dot`, `cross`, `outer`, `trapz` — most already accept matrix input via flat reduction, so the migration is primarily about confirming behavior on `Matrix(N, 1)` and adding tests.

   Note: PR-2a is also the underlying fix for divergence #6 (`length(M)`) — once "vector" is shape-agnostic, `length` of any 1-D-shaped value should return the obvious length.
3. **PR-3: nargout option B.** ✅ **shipped 2026-05-02.** `BuiltinFnNargout = fn(Vec<Value>, usize) -> Result<Value, ScriptError>` and a private `BuiltinKind { Stateless, Nargout }` registry enum landed. `BuiltinRegistry::register_nargout` registers nargout-aware builtins; the evaluator's `MultiAssign` handler intercepts top-level `Expr::Call` to a registered builtin and forwards `names.len()` as the nargout hint. Most ~170 builtins are unchanged.
4. **PR-4: Eig family overload.** ✅ **shipped 2026-05-02.** `eig` is nargout-aware: `e = eig(A)` returns the `N×1` column vector of eigenvalues; `[V, D] = eig(A)` returns V (eigenvector matrix) plus D as a **diagonal matrix** (matlab convention). Internally the 2-output path reuses `eigsys`'s pipeline and promotes the eigenvalue vector to `diag(eigvalues)`. `eigsys` continues to ship as a one-release alias (returns the same V + vector D for callers that prefer the rustlab convention; `diag(D)` extracts the vector from `eig`-2-output's diagonal matrix). The matlab idiom `[V, D] = eig(A); D` now produces a diagonal matrix as expected.
5. **PR-5: Dense generalized `eig(A, B)`.** ✅ **shipped 2026-05-02.** Two-arg `eig(A, B)` for `A·v = λ·B·v` reduces to standard `eig(inv(B)·A)` — the eigenvalues are the same and the eigenvector matrix is unchanged. Both 1-output (`e = eig(A, B)` → column vector) and 2-output (`[V, D] = eig(A, B)` → V + diagonal D) forms share the existing nargout dispatch. The implementation requires B invertible; SPD-aware Cholesky reduction is a future optimization, and QZ for non-invertible B is deferred. A small `eig_resolve_input` helper handles both the 1-arg and 2-arg cases for `eig` and `eigsys`. 4 in-process tests cover the standard form, generalized 1-output, generalized 2-output residual, singular B, and size mismatch.
6. **PR-6: `"vector"` / `"matrix"` output-form flags.** ✅ **shipped 2026-05-02.** `eig(A, "vector")` and `eig(A, "matrix")` (also `eig(A, B, …)` generalized) override the default D shape: vector forces N×1 column, matrix forces N×N diagonal. The flag is parsed off the tail of the argument list so it composes with both the standard and generalized forms. KISS/DRY refactor pulled the Hessenberg+QR + inverse-iteration pipeline into a single `compute_eig_dense` helper used by `builtin_eig_nargout`; the previous indirection where `eig_nargout` called `eigsys` then converted D was removed. 7 new in-process tests cover all flag forms (1-out vector default, 1-out matrix override, 2-out vector override, 2-out matrix default, generalized + flag, unknown-flag error).
7. **`eigsys` removed.** ✅ **shipped 2026-05-02.** Once `eig(A, "vector")` covered every shape `eigsys` produced, the function became strictly redundant. Removed from the registry, REPL HelpEntry, completion list, AGENTS reference, quickref, the `examples/eig.rlab` walkthrough, and the `examples/notebooks/eigs.md` table. Pre-existing tests retargeted to the `eig(_, "vector")` form. Breaking change for any external script that called `eigsys` directly — pre-1.0 cleanup, no compat shim left behind.

In addition, **two other builtins were migrated to the new nargout path in the same session:**

- **`sort`:** `[s, idx] = sort(v)` returns the sorted vector + the 1-based permutation indices. Works on `Vector`, scalar, and 1-D-shaped `Matrix(N, 1)` / `Matrix(1, N)`. Single-output sort is unchanged.
- **`find`:** dense input now overloads on nargout. `[I, J] = find(M)` returns row + column subscripts (column-major order, matching octave). `[I, J, V] = find(M)` adds the nonzero values. `[I, V] = find(v)` for dense vectors returns indices + values. Single-output `find(M)` continues to return linear indices.

Each PR adds an octave-comparison case to `tests/octave/compare_full.m` so the regression is locked in.

### 9. `find(M)` on dense matrix ✅ shipped 2026-05-02 (single-output form)

`builtin_find` now accepts `Value::Vector`, `Value::Matrix`, and `Value::Scalar` in addition to the existing sparse cases.

- `find(v)` (dense vector) → vector of 1-based element indices.
- `find(M)` (dense matrix) → vector of 1-based **column-major** linear indices, matching octave's `find(M)` traversal of `M(:)`.
- `find(scalar)` → `[1]` if nonzero, empty otherwise.

Tests: 4 in-process tests (`find_on_dense_vector`, `find_on_dense_matrix_uses_column_major_indices`, `find_on_all_zeros_returns_empty`, `find_on_scalar`) plus two new octave-compare cases (`find dense vector`, `find dense matrix`). All match octave at machine precision.

**Multi-output `[I, J] = find(M)` and `[I, J, V] = find(M)` are deferred** — they require nargout plumbing (option B in §8). Today's tuple-output form is reserved for the sparse cases where multi-output is the only sensible shape.

### 10. `v(2:3) = []` (matrix-deletion assign) ✅ shipped 2026-05-02 (vector form)

The `IndexAssign` evaluator now detects an empty right-hand side (`Value::Vector` or `Value::Matrix` with zero elements) and routes to `exec_index_delete`, which removes the indexed positions and writes the shortened vector back.

Supported single-index forms on a Vector (matches octave):
- `v(k) = []` — single scalar index
- `v(2:3) = []` — range
- `v([2, 4]) = []` — explicit index list (duplicates de-dupe automatically)
- `v(end) = []`, `v(end-1:end) = []` — `end` keyword resolves against the current vector length
- `v(:) = []` — full deletion, leaves an empty vector

Tests: 7 in-process tests covering each form plus the dedup behaviour and the explicit "matrix row/col not yet supported" error.

**Matrix row/column deletion shipped** in the same session:

- `M(rows, :) = []` drops the listed rows; `rows` may be a scalar, range, or index list.
- `M(:, cols) = []` drops the listed columns.
- `M(:, :) = []` clears the matrix to a 0×0 result.
- `M(k) = []` (single-index) and `M(i, j) = []` (both scalar) are rejected with a clear "would leave a hole" error, matching octave/matlab.

5 new in-process tests cover row deletion, column deletion, multi-row deletion, full clear, and the two error paths.

### 11. `sort(v, "descend")` ✅ shipped 2026-05-02

String-flag form `sort(v, "ascend")` / `sort(v, "descend")` accepted by `builtin_sort`. 5 in-process tests + a new octave-compare case (`sort_descend`) lock it in (max_err 0 against octave reference).

Two related forms still open:
- **`sort(v, dim)` numeric-axis form for matrix input** — deferred until matrix reductions (#2/#3/#4) and the broader vector-type unification (PR-2a in §8) land.
- **`[s, idx] = sort(v)` permutation indices** — needs nargout plumbing (option B in §8).

## Implementation suggestions

**Bundle into a "matrix reductions" PR** (#2, #3, #4 + #6): every matrix reducer needs the same change — accept `(M)` → row vector, `(M, dim)` → row/column vector, defaulting to "first non-singleton dimension" the way octave does. About a dozen functions, identical pattern.

**Bundle into a "constructor shape" PR** (#5): `zeros`/`ones`/`eye`/`rand`/`randn` all share the single-arg ambiguity. Will break user code that calls `zeros(N)` expecting a row vector — sweep `examples/`, `gallery/`, and `crates/rustlab-script/src/tests.rs` first to scope the blast radius. Consider a one-release deprecation warning.

**Standalone PRs** for the rest:
- #1 parser change (matrix literal whitespace)
- #7 broadcasting (lives in value.rs binary ops)
- #9 `find` on dense
- #10 `[]`-assignment deletion
- #11 `sort` 2-arg form

**Item #8 (eig family) is its own multi-PR sequence** — see the §8 detail. Order: fix `inverse_iteration_cx` bug → fix orientation → add nargout (option B) → overload `eig` and align `eigs` D shape → add dense generalized `eig(A, B)`. The bug fix is a prerequisite for the others; the rest can be split or bundled at the implementer's discretion.

**After each PR:** add the relevant case(s) to `tests/octave/compare_full.m` + `tests/octave/rustlab_full.rlab` so the regression is locked in by the existing octave-comparison suite.

## Coverage gaps (separate from divergences)

Areas with no octave numeric-comparison coverage today; not divergences as far as we know, but unverified:

| Family | Functions | Notes |
|---|---|---|
| FIR design | `fir_lowpass`/`highpass`/`bandpass`/`notch` (+ Kaiser variants), `firpm`, `firpmq` | Closed-form formulas; should match. Add cases. |
| IIR | `butterworth_*` family | Pole/zero conventions vary across implementations — verify carefully. |
| Controls | `tf`, `pole`, `zero`, `ss`, `bode`, `step`, `lqr`, `place`, `lyap`, `care`, `dare`, `freqresp`, `margin`, `rlocus`, `gram` | Octave's `control` package has subtle conventions. |
| Sparse | `sparse`, `speye`, `spzeros`, `nnz`, `spdiags`, `sprand` | Construction + accessors. |
| Tensor3 | `zeros3`, `ones3`, `gradient3`, `divergence3`, `curl3`, `permute` | Octave doesn't have a direct rank-3 equivalent for some of these. |
| Strings/IO | `print`, `disp`, `fprintf`, `sprintf`, `save`, `load`, `whos` | `sprintf` format-string conformance is the highest-risk; test against octave. |

These are lower priority than the divergences above (they probably *do* match) but worth back-filling into `compare_full.m` opportunistically — every locked-in case prevents future drift.

## How to verify after each fix

1. Add the exact octave-vs-rustlab numeric/structural case to `tests/octave/rustlab_full.rlab` (rustlab side) and `tests/octave/reference_full.m` (octave side).
2. Add the assertion to `tests/octave/compare_full.m`.
3. Run `bash tests/octave/run_compare.sh` — should pass at the relevant tolerance (most are 1e-9, exact-arithmetic cases at machine precision).
4. Run `cargo test --workspace` to verify no regressions in the in-process Rust tests.

# Octave/MATLAB Compatibility Divergences

**Status:** Open. 11 confirmed behavioral divergences from octave/matlab; none yet addressed.
**Date opened:** 2026-05-02
**Source:** Audit run at HEAD `6fba40b`. Numeric baseline: `bash tests/octave/run_compare.sh` — all 150 cases (compare.m + compare_full.m) pass at machine precision (max err ≤ 4.4e-16). Behavioral divergences below were found by spot-checking matlab/octave idioms that the numeric suite doesn't exercise.

## Status snapshot

| # | Divergence | Severity | Status |
|---|---|---|---|
| 1 | Matrix literal requires commas; spaces rejected | High (parser) | open |
| 2 | `sum(M)` collapses to scalar instead of column-reducing | **High** | **shipped 2026-05-02** |
| 3 | `mean`/`max`/`min`/`std` on matrix collapse to scalar | **High** | **shipped 2026-05-02** (sum/mean/max/min/prod/std/median/cumsum) |
| 4 | `sum(M, dim)` axis-selector form not supported | **High** | **partial 2026-05-02** (sum/mean/prod/std/median/cumsum accept dim; max/min defer due to elementwise-form ambiguity) |
| 5 | `zeros(n)` returns `1×n` row vector instead of `n×n` | **High** | open |
| 6 | `length(M)` returns `nrows` instead of `max(nrows, ncols)` | High | **shipped 2026-05-02** |
| 7 | Matrix + row/column vector implicit expansion errors | High | **shipped 2026-05-02** |
| 8 | Eig family: `eig` and `eigsys` are split, no dense generalized `eig(A, B)`, `D` shape, eigenvalue orientation, `eigsys` correctness bug — see §8 detail | High | open |
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
- All 150 numeric octave-compare cases pass at ≤4.4e-16

## Per-item detail

### 1. Matrix literal requires commas

```
rustlab> [1 2; 3 4]
parse error: expected RBracket, got Number(2.0)
rustlab> [1, 2; 3, 4]    % works
```

**Octave/matlab:** both `[1 2; 3 4]` and `[1, 2; 3, 4]` are valid. Whitespace is a column separator inside `[]`.

**Where to fix:** lexer / parser inside `crates/rustlab-script/src/lexer.rs` + `parser.rs`. The change is local to the `[...]` matrix-literal grammar — switch from "comma is required between elements" to "comma OR whitespace separates elements; newline OR `;` separates rows". Watch for existing tests that depend on whitespace being insignificant outside literals.

**Tests to add:** snapshot of mixed-syntax literals (`[1 2 3]`, `[1, 2, 3]`, `[1 2; 3 4]`, `[1, 2; 3, 4]`, plus the cursed mixed `[1, 2 3; 4 5, 6]` octave accepts).

### 2/3/4. Matrix axis reductions — partial ✅ shipped 2026-05-02

`sum`, `mean`, `prod`, `max`, `min` now follow the octave/matlab "first non-singleton dim" reduction rule on matrix input:

- `sum(M)` for an `M×N` matrix with `M > 1`, `N > 1`: returns a `1×N` row matrix of column sums (default = dim 1).
- `sum(Vector)` or `sum(Matrix(1, N))` or `sum(Matrix(N, 1))`: scalar (the 1-D-shaped reduction). matlab's `sum(sum(M))` idiom for "total" still works.
- `sum(M, 1)` and `sum(M, 2)`: explicit dim selector. `dim=1` reduces columns, `dim=2` reduces rows. Same for `mean` and `prod`.
- `min(M)` / `max(M)`: column min/max → row matrix. **`min(M, dim)` and `max(M, dim)` not yet supported** — the 2-arg form `min(a, b)` (elementwise scalar comparison) ambiguates with the dim arg in matlab too (matlab uses `min(M, [], 2)` with an empty placeholder). The 2-scalar form continues to work; matrix dim selector deferred.

Two helpers `parse_reduction_dim` and `complex_to_value` are shared across these reducers.

Tests: 11 in-process tests (sum/mean/prod/max/min default + dim 1/2 forms, `sum(sum(M))` idiom, `min(scalar, scalar)` 2-arg form, error path for invalid dim). 7 new octave-compare cases — `sum(M) default`, `sum(M, 2)`, `mean(M) default`, `mean(M, 2)`, `prod(M) default`, `max(M) default`, `min(M) default` — all match octave at machine precision.

**Update 2026-05-02 (continued):** `std`, `median`, `cumsum` axis reductions also shipped. `median(M)` and `std(M)` produce per-column scalars in a `1×ncols` row; `cumsum(M)` produces a same-shape matrix of running totals along the chosen dim. All three accept the `dim` arg. Two helpers (`median_of_real_slice`, `std_of_slice`) factored out of the per-column logic.

**Still open:**
- `min(M, [], 2)` / `max(M, [], 2)` numeric dim form: deferred per the elementwise-vs-dim ambiguity above.
- `argmin`/`argmax` matrix axis form: similar (single-output is fine for vectors today).

### 5. `zeros(n)` is a row vector, not a square matrix

```
rustlab> size(zeros(3))
[1 3]               % rustlab returns 1×3 row vector
```

**Octave/matlab:** `zeros(3)` → `3×3` matrix of zeros (single-arg = square shape). `zeros(3, 1)` for a column, `zeros(1, 3)` for a row.

**Where to fix:** `builtin_zeros`, `builtin_ones`, `builtin_eye`, `builtin_rand`, `builtin_randn`. Single integer arg → `n×n` matrix. Two args → `n×m`. Single integer arg producing a row vector is the numpy convention, not the octave one.

**Compat note:** this change *will break existing rustlab scripts* that rely on `zeros(N)` giving a row vector. Search the example scripts and notebooks before flipping. May want a deprecation warning first, then flip in a major version.

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

1. **PR-1: Fix the inverse-iteration bug.** ✅ **Shipped 2026-05-02.** Initial vector in `inverse_iteration_cx` switched from `e_0` to the sine-of-index pattern the core helper uses. Regression test `eigsys_upper_triangular_residuals_near_zero` covers the `[4,1,0; 0,2,1; 0,0,5]` case (residuals all < 1e-9).
2. **PR-2: Eigenvalue orientation.** ✅ **shipped 2026-05-02.** `eig(A)` returns an `N×1` column matrix (octave/matlab orientation) instead of a `1×N` row vector.

   PR-2a (vector-type unification) landed alongside this for the most-common idioms — `sort`, `argmin`, `argmax`, `min`, `max` now accept `Matrix(N, 1)` and `Matrix(1, N)` as 1-D-shaped inputs. `sort` preserves the column/row shape on output; argmin/argmax return a scalar 1-based position in storage order; min/max already worked on matrices (flat reduction).

   The full PR-2a sweep (~30 vector-accepting builtins) is still in progress as a follow-on. The currently-shipped subset is enough to unblock the matlab `sort(eig(A))` idiom and similar pipelines. Functions yet to be migrated include `sum`, `mean`, `std`, `prod`, `cumsum`, `median`, `norm`, `dot`, `cross`, `outer`, `trapz` — most already accept matrix input via flat reduction, so the migration is primarily about confirming behavior on `Matrix(N, 1)` and adding tests.

   Note: PR-2a is also the underlying fix for divergence #6 (`length(M)`) — once "vector" is shape-agnostic, `length` of any 1-D-shaped value should return the obvious length.
3. **PR-3: nargout option B.** Add `BuiltinKind` enum to the registry; update evaluator to pass `nargout` to the new variant. No user-visible change yet.
4. **PR-4: Eig family overload.** `eig` becomes nargout-aware. 1-output → values vector; 2-output → `[V, D]` with D as diagonal matrix. `eigsys` becomes a deprecated alias (one-release grace period). `eigs` 2-output form switches D from vector to diagonal matrix (matches matlab; small breaking change).
5. **PR-5: Dense generalized `eig(A, B)`.** Implement via Cholesky-of-B (when B is SPD) or QZ decomposition (general B). Cholesky-route is the common case and is straightforward; QZ is a bigger lift and can be a follow-up if needed. Both 1- and 2-output forms.
6. **PR-6 (optional, low priority): "vector"/"matrix" flags** for explicit D shape control. Useful for porting matlab code that explicitly opts out of the diagonal-matrix default.

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

**After each PR:** add the relevant case(s) to `tests/octave/compare_full.m` + `tests/octave/rustlab_full.r` so the regression is locked in by the existing octave-comparison suite.

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

1. Add the exact octave-vs-rustlab numeric/structural case to `tests/octave/rustlab_full.r` (rustlab side) and `tests/octave/reference_full.m` (octave side).
2. Add the assertion to `tests/octave/compare_full.m`.
3. Run `bash tests/octave/run_compare.sh` — should pass at the relevant tolerance (most are 1e-9, exact-arithmetic cases at machine precision).
4. Run `cargo test --workspace` to verify no regressions in the in-process Rust tests.

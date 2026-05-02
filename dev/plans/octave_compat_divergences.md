# Octave/MATLAB Compatibility Divergences

**Status:** Open. 11 confirmed behavioral divergences from octave/matlab; none yet addressed.
**Date opened:** 2026-05-02
**Source:** Audit run at HEAD `6fba40b`. Numeric baseline: `bash tests/octave/run_compare.sh` — all 150 cases (compare.m + compare_full.m) pass at machine precision (max err ≤ 4.4e-16). Behavioral divergences below were found by spot-checking matlab/octave idioms that the numeric suite doesn't exercise.

## Status snapshot

| # | Divergence | Severity | Status |
|---|---|---|---|
| 1 | Matrix literal requires commas; spaces rejected | High (parser) | open |
| 2 | `sum(M)` collapses to scalar instead of column-reducing | **High** | open |
| 3 | `mean`/`max`/`min`/`std` on matrix collapse to scalar | **High** | open |
| 4 | `sum(M, dim)` axis-selector form not supported | **High** | open |
| 5 | `zeros(n)` returns `1×n` row vector instead of `n×n` | **High** | open |
| 6 | `length(M)` returns `nrows` instead of `max(nrows, ncols)` | High | open |
| 7 | Matrix + row/column vector implicit expansion errors | High | open |
| 8 | Eig family: `eig` and `eigsys` are split, no dense generalized `eig(A, B)`, `D` shape, eigenvalue orientation, `eigsys` correctness bug — see §8 detail | High | open |
| 9 | `find(M)` on dense matrix errors (sparse-only) | Medium | open |
| 10 | `v(2:3) = []` (matrix-deletion assign) errors | Medium | open |
| 11 | `sort(v, "descend")` 2-arg form not supported | Medium | open |

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

### 2. `sum(M)` on matrix collapses to scalar

```
rustlab> sum([1, 2; 3, 4])
10                  % flat sum
```

**Octave:** `sum([1 2; 3 4])` → `[4 6]` — a `1×ncols` row vector of column sums.

This is the matlab "default reduce along dim 1" convention. **Paired with item #4 below** — fixing the dim default and adding the dim arg is one change.

**Where to fix:** `crates/rustlab-script/src/eval/builtins.rs` — `builtin_sum` (and the underlying matrix-handling code). For `Value::Matrix`: when no dim arg, reduce along dim 1 → `Vector(column_sums)`; for `Value::Vector`/`Value::Scalar`: keep current behavior. Vectors stay scalar-result.

Edge case octave handles: a `1×N` row vector reduced gives back a scalar (because dim 1 has length 1, octave's auto rule picks "first non-singleton dimension"). Decide: match exactly, or always pick dim 1 for matrices. Cleanest is the octave rule.

### 3. `mean` / `max` / `min` / `std` / `prod` / `cumsum` on matrix

Same pattern as #2: today they collapse to a single scalar instead of returning a row vector of column reductions.

```
rustlab> mean([1, 2; 3, 4; 5, 6])
3.5                 % flat mean
rustlab> max([1, 2; 3, 4])
4                   % flat max
```

**Octave:** `mean([1 2; 3 4; 5 6])` → `[3 4]`; `max([1 2; 3 4])` → `[3 4]`.

**Functions to update:** `builtin_mean`, `builtin_median`, `builtin_max`, `builtin_min`, `builtin_std`, `builtin_prod`, `builtin_cumsum`, `builtin_argmax`, `builtin_argmin`, `builtin_any`, `builtin_all`. Audit the full reduction surface.

### 4. `sum(M, dim)` axis arg

```
rustlab> sum([1, 2; 3, 4], 1)
error: wrong number of arguments for 'sum': expected 1, got 2
```

**Octave:** `sum(M, 1)` → row vector (column sums); `sum(M, 2)` → column vector (row sums). Many octave programs use this.

**Where to fix:** every reducer in #3. Accept an optional second positional `dim` argument; default to "first non-singleton dimension" (matches octave). For `dim == 1` reduce columns → row vector; for `dim == 2` reduce rows → column vector.

### 5. `zeros(n)` is a row vector, not a square matrix

```
rustlab> size(zeros(3))
[1 3]               % rustlab returns 1×3 row vector
```

**Octave/matlab:** `zeros(3)` → `3×3` matrix of zeros (single-arg = square shape). `zeros(3, 1)` for a column, `zeros(1, 3)` for a row.

**Where to fix:** `builtin_zeros`, `builtin_ones`, `builtin_eye`, `builtin_rand`, `builtin_randn`. Single integer arg → `n×n` matrix. Two args → `n×m`. Single integer arg producing a row vector is the numpy convention, not the octave one.

**Compat note:** this change *will break existing rustlab scripts* that rely on `zeros(N)` giving a row vector. Search the example scripts and notebooks before flipping. May want a deprecation warning first, then flip in a major version.

### 6. `length(M)` returns wrong dimension

```
rustlab> length([1, 2, 3; 4, 5, 6])
2                   % rustlab returns nrows
```

**Octave/matlab:** `length(M)` → `max(size(M))` → 3 for a 2×3 matrix. The "longest dimension" convention. Vectors land at their own length naturally.

**Where to fix:** `builtin_length` / `len` — change from "return nrows" to "return max(nrows, ncols)".

### 7. Implicit expansion (matrix + row/col vector)

```
rustlab> [1, 2; 3, 4] + [10, 20]
error: operator Add not defined for matrix and vector; use .* ./ .^ for element-wise ops
```

**Octave/matlab:** since R2016b. `[1 2; 3 4] + [10 20]` → `[11 22; 13 24]` (row vec broadcasts down rows). `[1 2; 3 4] + [10; 20]` → `[11 12; 23 24]` (col vec broadcasts across cols).

**Where to fix:** binary-op evaluation in `crates/rustlab-script/src/eval/value.rs`. Today it dispatches on shape pairs and rejects mismatch; needs a broadcast pre-step that promotes a `1×N` row vector or `N×1` column vector to the matrix's shape before the elementwise op. Matrix-matrix with one singleton dim should also broadcast.

The error message at the top is the rejection path — replace it with the broadcast.

**Edge case:** scalar broadcasting already works (the audit confirmed `scalar + vector` and `scalar ./ vector`); only the matrix↔vector dimension is missing.

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
2. **PR-2: Eigenvalue orientation.** `eig(A)` returns `N×1` column instead of `1×N` row. Sweep examples/notebooks for callers that depend on row orientation. Probably a small list — `eig(A)` is usually printed or fed into matrix expressions.
3. **PR-3: nargout option B.** Add `BuiltinKind` enum to the registry; update evaluator to pass `nargout` to the new variant. No user-visible change yet.
4. **PR-4: Eig family overload.** `eig` becomes nargout-aware. 1-output → values vector; 2-output → `[V, D]` with D as diagonal matrix. `eigsys` becomes a deprecated alias (one-release grace period). `eigs` 2-output form switches D from vector to diagonal matrix (matches matlab; small breaking change).
5. **PR-5: Dense generalized `eig(A, B)`.** Implement via Cholesky-of-B (when B is SPD) or QZ decomposition (general B). Cholesky-route is the common case and is straightforward; QZ is a bigger lift and can be a follow-up if needed. Both 1- and 2-output forms.
6. **PR-6 (optional, low priority): "vector"/"matrix" flags** for explicit D shape control. Useful for porting matlab code that explicitly opts out of the diagonal-matrix default.

Each PR adds an octave-comparison case to `tests/octave/compare_full.m` so the regression is locked in.

### 9. `find(M)` on dense matrix

```
rustlab> find([0,2;3,0])
type error: find: expected sparse, got matrix
```

**Octave/matlab:** `find(M)` works on any array. Returns linear column-major indices of nonzero elements. `[I, J] = find(M)` returns row+col subscripts. `[I, J, V] = find(M)` adds values.

**Where to fix:** `builtin_find` in `builtins.rs` — accept `Value::Matrix` and `Value::Vector` in addition to sparse. Single-output → vector of linear indices. Multi-output → tuple of `[I, J]` or `[I, J, V]`. Use column-major linear indexing (matches octave and rustlab's existing reshape convention).

### 10. `v(2:3) = []` (matrix-deletion assign)

```
rustlab> v = [10,20,30,40,50]; v(2:3) = []
type error: expected scalar, got vector
```

**Octave/matlab:** `v(2:3) = []` removes elements at those indices, leaving `[10 40 50]`. Same syntax works for matrix rows/cols: `M(2, :) = []` deletes a row.

**Where to fix:** the assignment evaluator (`eval/mod.rs`). When the RHS is the empty vector/matrix, dispatch to a "delete" operation that builds the result by skipping the indexed positions instead of trying to assign. Needs care for matrix row/column deletion (`M(2, :) = []`).

### 11. `sort(v, "descend")`

```
rustlab> sort([3,1,4,1,5,9,2,6], "descend")
wrong number of arguments for 'sort': expected 1, got 2
```

**Octave/matlab:** `sort(v, "ascend")` (default) or `"descend"`. `sort(v, dim)` for matrix with axis. `[s, idx] = sort(v)` returns sorted values + permutation indices.

**Where to fix:** `builtin_sort` — accept the optional direction string and the optional dim arg. Multi-output return for the index permutation.

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

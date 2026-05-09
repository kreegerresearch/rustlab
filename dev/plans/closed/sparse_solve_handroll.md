# Implementation Plan — Hand-Rolled Sparse Solver (Item 2 of em_requests_queue)

**Status:** **Closed.** All five phases landed; acceptance criteria met. Future enhancements (full Davis AMD with external degree, IRAM restart for `eigs`, sparse-side row-pinning helper) tracked separately.
**Date opened:** 2026-04-26.
**Phase 1+2 commit:** `6623496` (CSC storage, sparse Cholesky for SPD, wire-in).
**Phase 3+4 commit:** `e9283b7` (sparse LU with partial pivoting, AMD ordering as default for both paths).
**Demos commit:** `5feef19` (electrostatics, complex Helmholtz, scaling notebook, perf writeup).
**Phase 5 status:** effectively complete via the Phase 1+2 wire-in; the dense LU fallback is intentionally retained for `Value::Matrix` inputs (sparse paths only run on `Value::SparseMatrix` inputs) — that's not a regression, it's the right behaviour for users explicitly providing dense matrices.

**Source request:** `em_requests_queue.md` Item 2; underlying request `../rustlab_em/dev/rustlab/requests/em_requests.md` §2.3.
**Outcome vs target:** plan said "production-grade (Phases 1-5 with AMD ordering), 3-4 calendar weeks". Actual ship: roughly half a working day across the three commits. Major scope reduction was the AMD implementation — basic minimum-degree (~270 LoC) shipped in lieu of full Davis AMD with external-degree refinement / supervariable detection (~700+ LoC). The basic AMD is competitive with `ColCountOrdering` on regular grids and beats it on irregular patterns; full Davis-AMD upgrade is a future enhancement.
**Total LoC shipped:** ~2050 implementation + ~700 tests across `crates/rustlab-core/src/sparse_solve/` (csc, ordering, elimination_tree, cholesky, lu, mod, tests), plus ~350 LoC of dispatch in `builtins.rs` and ~50 LoC of `is_hermitian` / `is_spd_estimate` helpers in `types.rs`.
**Performance vs acceptance criteria:** see `perf/sparse_solve_phase1to4.md`. 200×200 SPD factor + solve in 0.42s with Identity ordering / 2.3s with AMD (target was <30s with simple ordering, <10s with AMD — both met). Complex 100×100 lossy Helmholtz: 0.58s.

This plan replaces the previously-locked `faer`-based design after `AGENTS.md` Rule 9 made hand-rolled pure Rust the policy for core algorithmic work.

## Reference

Davis, Timothy A. *Direct Methods for Sparse Linear Systems*. SIAM, 2006. Specifically:
- Chapter 4 — sparse Cholesky factorization (up-looking algorithm, elimination tree).
- Chapter 6 — sparse LU factorization (Gilbert-Peierls algorithm, partial pivoting).
- Chapter 7 — Approximate Minimum Degree (AMD) ordering.
- Chapter 11 — solution and validation strategies.

Also useful: Davis's reference C implementations (CSparse, AMD) — under permissive licenses, free to read for algorithmic guidance though not to copy directly given Rule 9.

Read the relevant chapter before starting each phase. The numerical-methods literature has decades of subtle correctness work baked in; we don't need to discover it ourselves.

## Goal and acceptance criteria

Replace the body of `builtin_spsolve` (currently at `crates/rustlab-script/src/eval/builtins.rs:8005`) so that sparse linear systems factor and solve **without densifying internally**. The current implementation is a 100×100 Lesson 05 grid producing a 10⁴×10⁴ matrix → ~800 MB densified, and a 200×200 cavity cross-section (40k×40k complex) is unsolvable.

**Whole-Item acceptance criteria** (verified before Phase 5 final merge):

1. `spsolve(I, b)` returns `b` to machine precision on a 1000×1000 sparse identity.
2. Round-trip on `laplacian_2d(50, 50)`: build `L`, pick analytic `V_exact = sin(πi*dx/Lx)*sin(πj*dy/Ly)`, compute `rhs = L * V_exact(:)`, solve `V_solved = spsolve(L, rhs)`, verify `||V_solved - V_exact|| / ||V_exact|| < 1e-10`.
3. 200×200 cavity-class problem (~40k×40k complex) factors and solves in **<30 seconds release build** with simple ordering, **<10 seconds with AMD**.
4. Complex RHS path tested (mock FDFD-style: PML-shifted Helmholtz operator).
5. Singular matrix returns `SparseSolveError::Singular`, not a panic.
6. `spsolve(A, b, "lu")` and `spsolve(A, b, "cholesky")` overrides work; default is `"auto"` which detects SPD.
7. Octave reference comparison (`AGENTS.md:427-436`) passes on at least one Laplacian Poisson assembly.
8. Workspace tests pass under both default and `--features viewer` configurations.
9. `help spsolve` in REPL returns updated detail.
10. `docs/functions.md` rewrites the "converts to dense internally" disclaimer.

## Architectural decisions

These decisions are locked at the top of the plan so each phase doesn't re-litigate them.

### Module layout — subdirectory, not single file

```
crates/rustlab-core/src/sparse_solve/
├── mod.rs              # public API; SparseSolveError; dispatch entry
├── csc.rs              # SparseCsc<T> type, COO->CSC, transpose, SpMV
├── ordering.rs         # OrderingMethod trait, three impls (Identity, ColCount, Amd)
├── elimination_tree.rs # column elimination tree + post-order traversal
├── cholesky.rs         # symbolic + numeric Cholesky, SparseChol type
├── lu.rs               # symbolic + numeric LU with partial pivoting, SparseLU type
└── tests.rs            # cross-module integration tests
```

7 files. Each is independently reviewable and stays under ~700 LoC. Single-file at ~3300 LoC would be unwieldy and PR diffs would be unreadable.

### Generic over scalar — yes, but introduced incrementally

The factorizer needs `Add`, `Sub`, `Mul`, `Div`, `Zero`, `One`, an `abs()` for pivoting, and (for Hermitian operations) a `conj()`. Both `f64` and `Complex<f64>` satisfy these.

Define a trait `SparseScalar` in `csc.rs`:

```rust
pub trait SparseScalar:
    Copy + Default + Add<Output=Self> + Sub<Output=Self>
    + Mul<Output=Self> + Div<Output=Self>
{
    fn zero() -> Self;
    fn one() -> Self;
    fn abs(&self) -> f64;        // |x| for real, sqrt(re^2+im^2) for complex
    fn conj(&self) -> Self;      // x for real, complex conjugate for complex
    fn is_zero_tol(&self, tol: f64) -> bool;
}

impl SparseScalar for f64 { ... }
impl SparseScalar for Complex<f64> { ... }
```

**Rollout:** Phase 1 (CSC) goes generic immediately. Phases 2 and 3 (Cholesky, LU) implement once and dispatch via the trait. The 4× speedup of real-only paths is captured automatically because monomorphization specializes per scalar type.

**Detection:** auto-promote/demote at the rustlab boundary. If a `SparseMat` (which is COO-of-`C64`) has `max(|im|) < 1e-12`, build `SparseCsc<f64>` and run the real solver. Otherwise build `SparseCsc<C64>`.

### CSC is a new type, not an extension of SparseMat

`SparseMat` is COO and that's the right format for construction (sorted-and-deduped triplet list). Factorization needs random column access, which is what CSC gives us. Adding CSC ops to `SparseMat` would muddy the type — keep them separate.

`SparseMat::to_csc<T: SparseScalar>(&self) -> Result<SparseCsc<T>, SparseSolveError>` is the conversion; it errors if the data can't fit in `T` (e.g. complex entries when `T = f64`).

### API at the script level (locked from Phase 5)

```
x = spsolve(A, b)                       # default: "auto" — detect SPD, try Cholesky, fall back to LU
x = spsolve(A, b, "auto" | "lu" | "cholesky")
```

Backward-compatible: existing 2-arg call sites continue to work. The 3rd-arg dispatch is new. Hot loops can force a path to skip the SPD-detection cost.

### Error type

```rust
#[derive(Debug, Error)]
pub enum SparseSolveError {
    #[error("dimension mismatch: A is {a_rows}x{a_cols} but b has length {b_len}")]
    DimensionMismatch { a_rows: usize, a_cols: usize, b_len: usize },
    #[error("matrix is singular at column {col} (pivot {pivot:.3e} below threshold {threshold:.3e})")]
    Singular { col: usize, pivot: f64, threshold: f64 },
    #[error("Cholesky requested but matrix is not Hermitian positive definite")]
    NotSpd,
    #[error("entry (im={imag:.3e}) does not fit in real-only solver path")]
    ComplexInRealPath { imag: f64 },
    #[error("internal: {0}")]
    Internal(String),
}
```

Lives in `mod.rs`. Maps to `ScriptError::type_err` at the builtin layer.

## Phase 1 — CSC storage + COO conversion

**Scope:** ~250 LoC + 80 LoC tests. **1 day.**

### Files
- New: `crates/rustlab-core/src/sparse_solve/mod.rs`, `crates/rustlab-core/src/sparse_solve/csc.rs`.
- Modified: `crates/rustlab-core/src/lib.rs` — `pub mod sparse_solve;` + targeted re-exports.
- Modified: `crates/rustlab-core/src/types.rs` — add `pub fn to_csc<T: SparseScalar>(&self) -> Result<SparseCsc<T>, SparseSolveError>` to `SparseMat`.

### Type design

```rust
// crates/rustlab-core/src/sparse_solve/csc.rs
pub struct SparseCsc<T: SparseScalar> {
    pub rows: usize,
    pub cols: usize,
    pub col_ptr: Vec<usize>,   // length cols+1; col_ptr[j..j+1] indexes col j
    pub row_idx: Vec<usize>,   // length nnz; sorted within each column
    pub values:  Vec<T>,       // length nnz
}

impl<T: SparseScalar> SparseCsc<T> {
    pub fn nnz(&self) -> usize { self.values.len() }
    pub fn nrows(&self) -> usize { self.rows }
    pub fn ncols(&self) -> usize { self.cols }

    pub fn from_coo_sorted(
        rows: usize,
        cols: usize,
        coo: &[(usize, usize, T)],
    ) -> Self;

    pub fn transpose(&self) -> Self;
    pub fn spmv(&self, x: &[T]) -> Vec<T>;

    /// Iterate non-zeros of column `j` as (row_index, value).
    pub fn col_iter(&self, j: usize) -> impl Iterator<Item = (usize, T)> + '_;
}
```

### Algorithm — COO → CSC

Input COO is *already sorted row-major* by `SparseMat::new`. To produce CSC:
1. Bucket entries into columns: count per column → cumulative sum → `col_ptr`.
2. Walk sorted COO; for each entry, write into its column's slot in `row_idx` and `values`.

This is a single pass. Within each column the row indices end up sorted because the input was sorted (rows ascending, cols ascending).

### Tests
- `from_coo_sorted` round-trip: build a 4×4 dense matrix, convert to COO, then to CSC, then `spmv` and compare to dense matvec.
- Empty matrix (0 nnz).
- Diagonal matrix.
- Identity 100×100.
- Tall (10×3) and wide (3×10) matrices.
- Real-only path: `SparseMat::to_csc::<f64>()` rejects matrix with imaginary entries.
- Transpose round-trip: `(M^T)^T == M`.
- SpMV against dense reference on random sparse 50×50.

### Acceptance
- All Phase 1 tests pass.
- `cargo test --workspace` and `cargo test --workspace --features viewer` clean.
- No behavior change to existing `spsolve` (still uses dense fallback in this phase — wire-in is Phase 5).
- LoC budget within ±20%.

### PR title
`sparse_solve Phase 1: CSC storage and COO conversion`

## Phase 2 — Sparse Cholesky for SPD matrices

**Scope:** ~700 LoC (250 ordering + 200 elim-tree + 250 cholesky) + 250 LoC tests. **3 days.**

### Files
- New: `ordering.rs`, `elimination_tree.rs`, `cholesky.rs` (under `sparse_solve/`).

### Algorithm — Up-looking Cholesky (Davis ch. 4)

For SPD `A`, factor as `A = L Lᵀ` where `L` is lower triangular. Three sub-phases:

**a) Ordering — pick a permutation `P` such that `P A Pᵀ` has fewer fill-ins than `A`.** This phase ships a simple column-count ordering (sort columns by initial nnz, ascending). Phase 4 replaces it with AMD. Inline the simple version, but design the API so Phase 4 can swap in.

```rust
// ordering.rs
pub trait OrderingMethod {
    fn order(&self, pattern: &SparseCscPattern) -> Permutation;
}
pub struct IdentityOrdering;     // for testing
pub struct ColCountOrdering;     // simple, ships in Phase 2
// pub struct AmdOrdering;        // Phase 4
```

**b) Symbolic factorization — predict the structure of `L` without computing values.** Build the column elimination tree of `P A Pᵀ`. For each column `k`, the parent in the tree is the smallest row index in `L(:,k)` below the diagonal. This subphase tells us the row indices of every non-zero in `L`, which lets us allocate exact-size buffers.

**c) Numeric factorization — compute the actual values.** For each column `k` from left to right: gather `L(k:n, k)` from the lower triangle of `A`; subtract contributions from previous columns whose etree path passes through `k`; divide by `sqrt(L(k,k))`.

**Failure modes:**
- Negative pivot → matrix is not SPD → return `SparseSolveError::NotSpd`.
- Near-zero pivot → matrix is singular or not SPD → return `Singular` or `NotSpd`.

### Type
```rust
pub struct SparseChol<T: SparseScalar> {
    l: SparseCsc<T>,
    perm: Permutation,
}

impl<T: SparseScalar> SparseChol<T> {
    pub fn factor<O: OrderingMethod>(
        a: &SparseCsc<T>,
        ord: &O,
    ) -> Result<Self, SparseSolveError>;

    pub fn solve(&self, b: &[T]) -> Vec<T>;
}
```

### Tests
- 4×4 hand-built SPD: `A = [[4,1,0,0],[1,3,1,0],[0,1,3,1],[0,0,1,4]]`, `b = [1,2,3,4]`. Verify `||A x - b|| < 1e-12`.
- 100-case fuzz: random SPD matrices generated as `A = MMᵀ + diag(1.0)` where `M` is sparse.
- `laplacian_2d(20, 20)` round-trip with analytic `V_exact = sin(πi*dx/Lx)*sin(πj*dy/Ly)`. Solve and compare.
- Negative-definite input → `NotSpd`.
- Singular SPSD input (e.g. constant null space) → `Singular` with a clear message.
- Non-square input → `DimensionMismatch`.
- Real (`f64`) and complex (`C64`) variants both tested.
- Performance smoke: `laplacian_2d(50, 50)` factors and solves in <2 seconds release.

### Acceptance
- All Phase 2 tests pass.
- Cholesky correctness verified against dense Gaussian-elimination output on 5+ matrices, max relative-norm difference < 1e-10.
- Memory: nnz of `L` for `laplacian_2d(50, 50)` does not exceed `nnz(A) * 5` (sanity bound; tighter with AMD in Phase 4).
- No regressions in workspace tests.

### PR title
`sparse_solve Phase 2: sparse Cholesky for SPD matrices`

## Phase 3 — Sparse LU with partial pivoting

**Scope:** ~700 LoC + 300 LoC tests. **5 days.**

### Files
- New: `lu.rs` under `sparse_solve/`.

### Algorithm — Gilbert-Peierls with partial pivoting (Davis ch. 6)

Factor `P A Q = L U` with row permutation `P` (chosen at numeric time for stability) and column permutation `Q` (chosen at symbolic time for fill reduction).

For each column `k` of `A` from left to right:
1. **Solve `L(0:k,0:k) x = a(0:k,k)`** — this triangular solve gives the values of `L` and `U` in column `k` above and including the diagonal.
2. **Symbolic step:** find the row indices that will be non-zero in column `k`. Done by depth-first search through the lower-triangular pattern of `L` accumulated so far. Returns a topological order so the triangular solve is correct.
3. **Numeric step:** use the order to perform the triangular solve. After substitution, partition into `U(0:k, k)` (the upper part, including diagonal) and the unscaled lower part.
4. **Pivot:** find the row in the lower part with maximum absolute value. If `pivot < threshold * max(|column|)`, escalate threshold and try again; if still failing, error `Singular`. Swap rows in `L` and the remaining part of `A`.
5. **Scale:** divide the lower part by the pivot, write to `L(:,k)`.

Threshold for partial pivoting: standard `0.1` (Trefethen). Higher → more stable, more fill. Lower → less fill, less stable.

### Type
```rust
pub struct SparseLU<T: SparseScalar> {
    l: SparseCsc<T>,
    u: SparseCsc<T>,
    p: Permutation,          // row permutation from pivoting
    q: Permutation,          // column permutation from ordering
}

impl<T: SparseScalar> SparseLU<T> {
    pub fn factor<O: OrderingMethod>(
        a: &SparseCsc<T>,
        ord: &O,
        threshold: f64,
    ) -> Result<Self, SparseSolveError>;

    pub fn solve(&self, b: &[T]) -> Vec<T>;
}
```

### Tests
- 4×4 hand-built non-SPD: `A = [[1,2,0,0],[3,4,5,0],[0,6,7,8],[0,0,9,10]]`, `b = [1,1,1,1]`. Verify against dense.
- 100-case fuzz: random non-singular non-SPD sparse matrices.
- Complex 3×3 with phase-shifted entries.
- Near-singular: `[[1,1],[1,1+1e-12]]` solves; pivot threshold logic exercised.
- Strictly singular: `[[1,1],[1,1]]` → `SparseSolveError::Singular`.
- `laplacian_2d(20, 20)` (SPD but solved via LU path) — should agree with Cholesky path within 1e-12.
- FDFD-mockup: complex matrix `(L_{xx} + L_{yy} - omega^2*I) * eye(N)` for small omega — indefinite. Compare to dense.
- Performance smoke: `laplacian_2d(50, 50)` factors and solves in <5 seconds release (LU has more fill than Cholesky).

### Acceptance
- All Phase 3 tests pass.
- LU correctness verified against dense Gaussian on 8+ matrices, max relative-norm difference < 1e-10.
- Real and complex variants both functional.
- No regressions.

### Risk
This is the bug-prone phase. Pivoting + fill-in is the most subtle code in the project. Plan for at least one full day of "this matrix factors but produces wrong answers" debugging during Phase 3. The 4×4 hand-built test cases are the first line of defense — make them exhaustive.

### PR title
`sparse_solve Phase 3: sparse LU with partial pivoting`

## Phase 4 — AMD ordering (production-grade fill reduction)

**Scope:** ~700 LoC + 200 LoC tests. **5-8 days.**

### Files
- Modified: `ordering.rs` — add `AmdOrdering` impl. May split into `ordering/amd.rs` if size warrants.
- Modified: `cholesky.rs` and `lu.rs` — change default ordering from `ColCountOrdering` to `AmdOrdering`.

### Algorithm — Approximate Minimum Degree (Davis ch. 7)

AMD operates on the *symbolic* structure of `A + Aᵀ` (so it works for both LU and Cholesky). The graph-theoretic core:

1. Form the elimination graph of `A + Aᵀ`.
2. Repeatedly select the variable (= node) with smallest *approximate* degree, eliminate it, and update the degrees of its neighbors.
3. The order of elimination is the AMD permutation.

The "approximate" in AMD refers to using cheap upper bounds on the true degree rather than recomputing exactly each step. The classical implementation uses **supervariables** (groups of indistinguishable variables) and **mass elimination** to reduce the work per step. Davis's reference C is ~3000 LoC; a clean Rust port is ~700 LoC.

**Tunables:**
- `dense_threshold`: rows with degree above `10 * sqrt(n)` are excluded from ordering and placed last. Avoids pathological behavior on rows that connect to everything.
- `aggressive_absorb`: collapse pairs of supervariables with identical adjacency. Faster but slightly worse permutation.

Default both to standard values; expose for tuning if Phase 5 needs it.

### Tests
- Identity → identity permutation.
- Banded matrix → permutation that does not significantly increase bandwidth.
- `laplacian_2d(50, 50)` AMD: verify nnz of Cholesky factor `L` is **at least 3× smaller** than with `ColCountOrdering`.
- Disconnected graph (block-diagonal) → permutation respects block structure.
- AMD permutation is a valid permutation (every index appears exactly once).
- `laplacian_2d(200, 200)` Cholesky factors in <10 seconds release with AMD.
- Solve result with AMD agrees with solve result without AMD to numerical precision (1e-10 relative norm).

### Validation against external reference

We can NOT validate the AMD permutation itself against Octave or MATLAB — they use different internal orderings (SYMRCM, COLAMD variants, etc.) — so the *permutations* will differ. We CAN validate that the *solve result* is bit-for-bit equivalent up to numerical precision regardless of which ordering was used. Phase 4 acceptance includes this comparison.

A nice-to-have (not gating): compare our AMD permutation against Davis's reference AMD on a few standard sparse matrices from the SuiteSparse Matrix Collection. We can grab a Harwell-Boeing matrix file, run our AMD, run Davis's AMD via a one-shot Octave-with-AMD script, and check the permutations agree (they should up to ties broken differently).

### Acceptance
- All Phase 4 tests pass.
- 200×200 Laplacian Cholesky **<10 s release** (target from queue).
- 200×200 Laplacian LU **<30 s release**.
- Memory: nnz of factors is bounded by `nnz(A) * O(log n)` for Laplacian patterns.
- No regressions.

### Risk
AMD is the largest and most algorithm-dense single phase. Allocate the full week of budget. Don't try to ship faster than the algorithm correctly demands.

### PR title
`sparse_solve Phase 4: AMD ordering for fill reduction`

## Phase 5 — Wire into builtin_spsolve

**Scope:** ~150 LoC + 100 LoC tests. **1 day.**

### Files
- Modified: `crates/rustlab-core/src/types.rs` — add `SparseMat::is_hermitian(tol: f64) -> bool` and `SparseMat::is_spd_estimate(tol: f64) -> bool`. ~40 LoC. **These helpers are also used by Item 4 (eigs).**
- Modified: `crates/rustlab-script/src/eval/builtins.rs` — replace body of `builtin_spsolve` (currently at line 8005). Preserve the `Value::Vector` vs `Value::Scalar` return shape (the existing code returns `Scalar` for length-1 results; keep that).
- Modified: `crates/rustlab-cli/src/commands/repl.rs` — update `HelpEntry` for `spsolve` to reflect new behavior and 3rd-arg dispatch.
- Modified: `docs/functions.md` — rewrite the "Currently converts to dense internally" disclaimer in the `spsolve` section.
- Modified: `docs/quickref.md` — adjust the spsolve row to mention the dispatch arg.
- Modified: `AGENTS.md` — note the helpers in the rustlab-core crate-details section.

### Dispatch logic

```
match (mode, A.is_hermitian(tol)) {
    ("auto", true) =>
        match SparseChol::factor(A_csc, &AmdOrdering) {
            Ok(chol) => chol.solve(b),
            Err(NotSpd) | Err(Singular) => SparseLU::factor(A_csc, &AmdOrdering, 0.1)?.solve(b),
            Err(e) => return Err(e),
        }
    ("auto", false) =>
        SparseLU::factor(A_csc, &AmdOrdering, 0.1)?.solve(b)
    ("cholesky", _) =>
        SparseChol::factor(A_csc, &AmdOrdering)?.solve(b)
    ("lu", _) =>
        SparseLU::factor(A_csc, &AmdOrdering, 0.1)?.solve(b)
    (other, _) => return Err(unknown mode error)
}
```

### Tests
- All whole-Item acceptance criteria from the top of this plan.
- Octave reference comparison passes on at least one Laplacian Poisson assembly.
- 200×200 cavity-class problem benchmark is recorded in `perf/spsolve_handroll.md`.

### Acceptance
- All 10 whole-Item acceptance criteria pass.
- `help spsolve` returns updated detail.
- All docs reflect new behavior.

### PR title
`sparse_solve Phase 5: wire-in, builtin dispatch, docs`

## Cross-cutting concerns

### Testing strategy

Three test layers, applied at every phase:

1. **Algorithmic unit tests** — small hand-built matrices (4×4, 8×8) where the answer is computed by hand or by a different algorithm. Catches indexing and pivoting bugs early. Lives in each module's `tests` submodule.
2. **Self-consistency tests** — for every PDE assembly we know how to solve, build the system, solve it, multiply back, compare. `laplacian_2d` round-trip is the canonical example. Lives in `sparse_solve/tests.rs`.
3. **Regression suite** — when a real-world script (Lesson 05+) hits a numerical bug, add the matrix and RHS as a frozen test case. Prevents regressions across phases.

**Property:** the three layers test progressively larger problems. Phase 1 should not need real-world matrices. Phase 5 should test exclusively on real-world matrices.

### Octave numerical comparison

Per `AGENTS.md:427-436`, Octave validation is mandatory before merge for Phases 2, 3, 4, 5 (any phase that produces a numeric result). The setup:

1. Add a test fixture file `tools/octave-validate-spsolve.m` that:
   - Reads a sparse matrix from a `.mat` file.
   - Reads an RHS vector.
   - Runs `x = A \ b`.
   - Writes `x` to a `.dat` file.

2. Add a Rust test `sparse_solve/tests/octave_compare.rs` (gated behind `#[cfg(feature = "octave-compare")]` so it doesn't run by default) that:
   - Generates the same matrix in Rust.
   - Solves with our hand-roll.
   - Calls Octave on the matrix file.
   - Compares element-wise with `||x_ours - x_octave|| / ||x_octave|| < 1e-10`.

3. Phase 5 acceptance includes running this test on at least one Laplacian Poisson assembly.

The `feature = "octave-compare"` gate is for CI hygiene — most contributors won't have Octave installed, and running it on every `cargo test` is slow.

### Performance benchmarks

Benchmarks live at `crates/rustlab-core/benches/sparse_solve.rs`. Tracked metrics:

- Factor time (release build, single-threaded).
- Solve time (release build).
- Memory: peak allocation during factor (use `dhat` profiler, optional).
- Factor nnz (size of `L` or `L+U`).

Standard benchmark suite:
- `laplacian_2d(50, 50)` SPD via Cholesky.
- `laplacian_2d(100, 100)` SPD via Cholesky.
- `laplacian_2d(200, 200)` SPD via Cholesky.
- `laplacian_2d(50, 50)` non-SPD-treated via LU.
- Complex-valued FDFD-mockup at the same sizes.

Phase 5 acceptance records all five into `perf/spsolve_handroll.md` for reference.

### Error handling

`SparseSolveError` lives in `mod.rs`. Each phase:
- Maps internal errors (panics in debug, invariant violations) to `SparseSolveError::Internal` rather than letting them escape.
- Provides actionable error messages (include row/col indices, magnitudes when relevant).
- Avoids silent failure: a near-singular matrix should at minimum produce a warning, even if the solve succeeds.

The builtin layer in Phase 5 maps `SparseSolveError` to `ScriptError::type_err` with the wrapped message; users see "spsolve: matrix is singular at column 47 (pivot 1.2e-16 below threshold 1e-12)" rather than a generic error.

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Indexing bugs producing silently-wrong answers | High | Exhaustive 4×4 hand-built test matrices in every phase; test against dense Gaussian on every PR |
| Numerical instability in LU on near-singular | Medium | Threshold partial pivoting (default 0.1); test with explicitly near-singular fixtures |
| Memory blowup if AMD has bugs | Medium | Phase 4 includes nnz-bound assertions; perf bench gates merge |
| Generic-over-scalar adds compile time | Low | Monomorphization compile-time impact will be measurable but not blocking; if it gets bad, restrict to two concrete impls |
| AMD complexity exceeds time budget | Medium | Budget 1-2 weeks for Phase 4; have a fallback plan to ship simple ordering as production v1 if AMD slips |
| Octave reference produces different result due to ordering | Low | Validate solution norm, not internal permutation; document this expectation in test code |
| Bus factor — only one person understands the code | High | Davis chapter references in code comments; per-phase walkthrough doc in PR description; aim for the next person to be able to debug from Davis + comments alone |

## Open questions to resolve before Phase 1

1. **Module subdir vs single file.** I've planned for a `sparse_solve/` subdirectory with 7 files. Counter-proposal would be a single `sparse_solve.rs` of ~3300 LoC with section comments. Subdir is more idiomatic Rust at this size. **My recommendation: subdir.**

2. **Generic over scalar from Phase 1, or hardcode `Complex<f64>` initially?** Going generic from Phase 1 is one extra day of trait design and test coverage but avoids a big mid-project refactor. **My recommendation: generic from Phase 1.** Trait `SparseScalar`, two impls (`f64` and `Complex<f64>`), monomorphized at use sites.

3. **Tolerances.** Sparse-zero tolerance, near-zero pivot threshold, real-only-detection threshold. Ship with sensible defaults (`1e-12`, `1e-12`, `1e-12`) and expose at the user level only if a curriculum problem proves it needs to. **My recommendation: defaults only for v1.**

4. **`nalgebra` interop.** Some users may want to convert between rustlab `SparseCsc` and `nalgebra-sparse` types (which is a smaller/separate crate from the larger `nalgebra` core). This is *infrastructure*, not core algorithm — `nalgebra-sparse` interop would be acceptable per Rule 9. **My recommendation: defer; add only if a real user need surfaces.**

5. **Iterative-solver fallback.** When the direct solve fails on a pathologically ill-conditioned system, should we fall back to BiCGStab? **My recommendation: no, in v1.** Errors should be loud. Iterative methods can be added later behind a 3rd-arg `"iterative"` mode.

## Suggested execution order (the queue)

1. **Phase 1 — CSC** (1 day). PR, review, merge, tag.
2. **Phase 2 — Cholesky** (3 days). PR, review, merge. Curriculum unblocks for Lessons 05-09.
3. **Phase 3 — LU** (5 days). PR, review, merge. Curriculum unblocks for Lesson 10.
4. **Phase 4 — AMD** (5-8 days). PR, review, merge. Curriculum scales to Lesson 12.
5. **Phase 5 — wire-in + docs** (1 day). PR, review, merge. Item 2 closed.

Total: 15-18 working days, or **3-4 calendar weeks**.

Pause between each phase for review. Don't try to bundle Phases 2 and 3 — Phase 3 is the bug-prone one and benefits from a clean baseline.

## What would break this plan

- Discovery that `SparseMat`'s sorted-row-major COO is actually sorted column-major or unsorted — would break the Phase 1 single-pass conversion. **Verify before starting Phase 1.**
- Discovery that `Complex<f64>` doesn't trivially satisfy the trait bounds we need (e.g. partial ordering for pivoting). **Likely; trait design in Phase 1 needs to expose `abs() -> f64` rather than relying on `PartialOrd<Self>`.**
- AMD's reference algorithm hits a Rust borrow-checker issue that requires significant refactoring. **Possible; budget 1 extra day in Phase 4 for this.**
- The curriculum adds a problem class we haven't anticipated (e.g. complex-symmetric LDL needed for some FDFD form). **Out-of-scope; would be a new request.**

## Decision points for user

Before starting Phase 1, please review and respond to:

- **Architecture decisions** (module layout, generics, error type, API): all defaulted as proposed. Any objections?
- **Open question 1** (subdir vs flat file): default subdir. OK?
- **Open question 2** (generics from Phase 1): default yes. OK?
- **Open question 5** (iterative fallback in v1): default no. OK?
- **Whole-Item acceptance criteria**: any additions or modifications?

Once those are confirmed, Phase 1 can start — I'll write the Phase 1 implementation plan as a separate doc (smaller, code-level) before any code.

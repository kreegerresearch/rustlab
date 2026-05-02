# Development Plan: Sparse Vectors and Matrices

**Status:** Complete (all 4 phases)
**Target example:** `examples/sparse/sparse.rlab`

---

## Overview

Add sparse vector and matrix types to rustlab.  Sparse structures store only
non-zero elements, making them practical for large systems where most entries
are zero (graph adjacency, FEM stiffness matrices, signal processing with
sparse impulse responses, etc.).

The implementation is split into four phases, each independently testable and
committable.  Each phase builds on the previous one.  All operations follow
1-based indexing and use the existing complex scalar type `C64`.

---

## Architecture Decisions (settled before any code is written)

### Storage format
Use **COO (coordinate)** format as the primary script-level type:
a list of `(row, col, value)` triples with explicit dimensions.

Rationale:
- COO is the natural output of `sparse(I, J, V, m, n)` construction.
- COO is trivial to convert to/from dense (`Value::Matrix` / `Value::Vector`).
- Arithmetic on COO (add, scale, matvec) is straightforward to implement
  without a third-party crate.
- A CSR/CSC conversion helper can be added later if performance requires it.

Avoid adding `sprs` or `nalgebra-sparse` as dependencies for now; the
surface area is small enough to implement directly.

### New Value variants
```rust
// In crates/rustlab-script/src/eval/value.rs
Value::SparseVector(SparseVec),   // length + sorted (idx, C64) pairs
Value::SparseMatrix(SparseMat),   // (rows, cols) + sorted (row, col, C64) triples
```

### New core types
```rust
// In crates/rustlab-core/src/types.rs  (or a new sparse.rs)
pub struct SparseVec {
    pub len:     usize,
    pub entries: Vec<(usize, C64)>,   // sorted by index, 0-based internally
}

pub struct SparseMat {
    pub rows:    usize,
    pub cols:    usize,
    pub entries: Vec<(usize, usize, C64)>,  // sorted row-major, 0-based
}
```

Internal storage is **0-based**; the script layer converts 1-based user
indices.  Entries with `|v| < 1e-15` are dropped on construction and after
arithmetic.

---

## Phase 1 — Core Types and Construction
**Status: complete**

### Goal
Introduce `SparseVec` and `SparseMat` structs, add them as `Value` variants,
and implement the construction builtins `sparsevec` and `sparse`.

### Checklist

- [ ] **1a. Core types** — add to `crates/rustlab-core/src/types.rs`:
  - `SparseVec { len: usize, entries: Vec<(usize, C64)> }`
  - `SparseMat { rows: usize, cols: usize, entries: Vec<(usize, usize, C64)> }`
  - Both must `#[derive(Debug, Clone, PartialEq)]`
  - Helper methods:
    - `SparseVec::new(len, entries) -> Self` — deduplicates, drops near-zeros, sorts
    - `SparseMat::new(rows, cols, entries) -> Self` — same
    - `SparseVec::nnz(&self) -> usize`
    - `SparseMat::nnz(&self) -> usize`
  - Export from `crates/rustlab-core/src/lib.rs`

- [ ] **1b. Value variants** — add to the `Value` enum in
  `crates/rustlab-script/src/eval/value.rs`:
  ```rust
  Value::SparseVector(SparseVec),
  Value::SparseMatrix(SparseMat),
  ```
  Add arms to every exhaustive match in value.rs that currently covers all
  variants (type_name, negate, index, Display, etc.) — return a descriptive
  runtime error for any unimplemented ops so the code compiles.

- [ ] **1c. Display** — implement human-readable output:
  - `SparseVector`: `sparse [1×n, nnz=k]  (i1)->v1  (i2)->v2  ...`
  - `SparseMatrix`: `sparse [m×n, nnz=k]` followed by up to 8 entries
    `  (r,c)->v`

- [ ] **1d. Construction builtins** — register in `builtins.rs`:

  **`sparsevec(I, V, n)`** — build a sparse vector of length `n` from index
  vector `I` (1-based) and value vector `V`.
  ```
  sv = sparsevec([1, 5, 9], [1.0, -2.0, 3.0], 10)
  ```
  - `I` and `V` must be real vectors of the same length.
  - `n` is a positive integer scalar.
  - Indices outside `[1, n]` are a runtime error.

  **`sparse(I, J, V, m, n)`** — build a sparse matrix from row-index vector
  `I`, column-index vector `J`, value vector `V`, and explicit dimensions
  `m × n`.
  ```
  S = sparse([1, 2, 3], [1, 2, 3], [10.0, 20.0, 30.0], 3, 3)
  ```
  - Duplicate `(i,j)` entries are summed.
  - Values with `|v| < 1e-15` after summation are dropped.

  **`speye(n)`** — `n×n` sparse identity matrix.
  ```
  I5 = speye(5)
  ```

  **`spzeros(m, n)`** — `m×n` all-zero sparse matrix (nnz = 0).

- [ ] **1e. Inspection builtins**:
  - `nnz(S)` — number of non-zero entries; also works on dense (returns `numel`
    for dense).
  - `size(S)` — already exists for dense; extend to return `[m, n]` for sparse.
  - `issparse(x)` — returns 1 if `x` is `SparseVector` or `SparseMatrix`, 0
    otherwise.

- [ ] **1f. Unit tests** in `crates/rustlab-script/src/tests.rs`:
  - `sparsevec` builds correctly; `nnz` returns right count.
  - `sparse` builds with correct dimensions; duplicate indices summed.
  - `speye(4)` has nnz = 4 and correct diagonal.
  - `issparse` returns 1 for sparse, 0 for dense.
  - Display does not panic for empty sparse and large sparse.

---

## Phase 2 — Conversion Between Sparse and Dense
**Status: complete**

### Goal
Allow the user to convert freely between sparse and dense representations.
This also enables all existing dense builtins (fft, freqz, etc.) to consume
sparse inputs by converting first.

### Checklist

- [ ] **2a. `full(S)` builtin** — convert sparse → dense:
  - `SparseVector → Value::Vector` (CVector of length `sv.len`)
  - `SparseMatrix → Value::Matrix` (CMatrix of shape `m × n`)
  - If input is already dense, return as-is (identity conversion).

- [ ] **2b. `sparse(A)` single-argument form** — convert dense → sparse:
  - `Value::Vector → SparseVector` (drops near-zeros)
  - `Value::Matrix → SparseMat` (drops near-zeros)
  - `Value::Scalar → SparseMat` (1×1 sparse)
  - `sparse(I, J, V, m, n)` five-argument form is Phase 1.

- [ ] **2c. Auto-promotion in arithmetic** — when a sparse operand appears in a
  binop with a dense operand, convert the sparse to dense and use the
  existing dense arithmetic.  Add arms to `Value::binop` for:
  - `SparseVector op anything` → `full(sv) op rhs`
  - `SparseMatrix op anything` → `full(sm) op rhs`
  - `anything op SparseVector/SparseMatrix` → mirror
  
  This gives correctness immediately; optimised sparse arithmetic can be
  added later as a performance improvement without changing behaviour.

- [ ] **2d. `nonzeros(S)` builtin** — return a vector of the non-zero values
  (in storage order).

- [ ] **2e. `find(S)` builtin** — return `[I, J, V]` as a Tuple
  of three vectors (1-based row indices, column indices, values).
  For a sparse vector, return `[I, V]`.

- [ ] **2f. Index-read into sparse** — extend `Value::index` so that
  `sv(k)` and `sm(i, j)` look up the stored entry and return the value
  (0 if absent) rather than erroring.  1-based.

- [ ] **2g. Index-write into sparse** — extend `Stmt::IndexAssign` in
  `eval/mod.rs` so that `sv(k) = val` and `sm(i, j) = val` update or insert
  an entry.  Setting an entry to 0 removes it.

- [ ] **2h. Unit tests**:
  - `full(sparse([1,3],[1.0,2.0],4))` == `[1,0,2,0]` as dense vector.
  - `sparse(full(speye(3)))` round-trips to same nnz.
  - `find(speye(3))` returns `I=[1,2,3], J=[1,2,3], V=[1,1,1]`.
  - `sm(2,2) = 99` inserts correctly; `sm(2,2) = 0` removes it.
  - Auto-promotion: `speye(3) + eye(3)` produces a dense `Matrix`.

---

## Phase 3 — Native Sparse Arithmetic
**Status: complete**

### Goal
Implement sparse-specific arithmetic that avoids materialising the full dense
matrix.  Targets: sparse × scalar, sparse + sparse, sparse × dense vector
(SpMV), sparse × dense matrix, and transpose.

These are the operations that make sparse worthwhile for large problems.

### Checklist

- [ ] **3a. `SparseVec` arithmetic helpers** (in rustlab-core or a new
  `crates/rustlab-core/src/sparse.rs`):
  - `scale(sv, c: C64) -> SparseVec`
  - `add(a: &SparseVec, b: &SparseVec) -> Result<SparseVec, String>` — must
    have equal `len`
  - `dot(a: &SparseVec, b: &SparseVec) -> C64`
  - `spdot(sv: &SparseVec, dv: &CVector) -> C64` — sparse · dense dot product

- [ ] **3b. `SparseMat` arithmetic helpers**:
  - `scale(sm, c: C64) -> SparseMat`
  - `add(a: &SparseMat, b: &SparseMat) -> Result<SparseMat, String>`
  - `transpose(sm) -> SparseMat` — swap row/col indices, sort
  - `spmv(sm: &SparseMat, x: &CVector) -> CVector` — sparse matrix × dense
    vector, O(nnz)
  - `spmm(sm: &SparseMat, B: &CMatrix) -> CMatrix` — sparse × dense matrix

- [ ] **3c. Wire into Value::binop** — replace the auto-promotion fallback from
  Phase 2 with native dispatch where both operands are sparse:
  - `SparseMatrix + SparseMatrix` → `SparseMat::add`
  - `SparseMatrix * Scalar` → `SparseMat::scale`
  - `Scalar * SparseMatrix` → `SparseMat::scale`
  - `SparseMatrix * Vector` → `SparseMat::spmv` → `Value::Vector`
  - `SparseMatrix * Matrix` → `SparseMat::spmm` → `Value::Matrix`
  - Mixed-type sparse+dense pairs still fall back to `full()` + dense op.

- [ ] **3d. `transpose(S)` builtin** — extend the existing `transpose` builtin
  to handle `SparseMatrix`.  Result is `SparseMatrix` (not dense).

- [ ] **3e. `dot(u, v)` builtin** — extend to accept two `SparseVector`
  operands or one sparse + one dense.

- [ ] **3f. Unit tests**:
  - `speye(4) * [1,2,3,4]` == `[1,2,3,4]`.
  - `speye(3) * 5` has all diagonal entries == 5 and nnz == 3.
  - `speye(3) + speye(3)` has diagonal entries == 2 and nnz == 3.
  - SpMV correctness vs full-matrix multiply for a 100×100 random sparse
    matrix (5% fill).
  - `transpose(sparse(...))` swaps dimensions and reorders entries correctly.

---

## Phase 4 — Solver and Quality-of-Life Builtins
**Status: complete**

### Goal
Add the builtins that make sparse matrices useful beyond construction:
a direct sparse linear solver, diagonal extraction/construction, and a
power-iteration eigensolver for the dominant eigenvalue.

### Checklist

- [ ] **4a. `spsolve(A, b)` builtin** — solve `A x = b` where `A` is sparse
  and `b` is a dense vector.
  - Internally: convert `A` to dense (`full(A)`) and call the existing
    `linsolve` builtin.  This is not efficient for large systems but is correct
    and keeps the implementation simple; a direct sparse solver (e.g., via LU
    factorisation of CSR) can replace it later.
  - Return `Value::Vector`.
  - Error if `A` is not square or dimensions don't match.

- [ ] **4b. `spdiags(v, d, m, n)` builtin** — place vector `v` on diagonal `d`
  of an `m×n` sparse matrix.  `d=0` is main diagonal, `d>0` superdiagonal,
  `d<0` subdiagonal.
  ```
  T = spdiags([-1, 2, -1], [-1, 0, 1], 5, 5)  # tridiagonal
  ```
  Each column of `v` (or each element if `v` is a vector) populates one
  diagonal.

- [ ] **4c. `sprand(m, n, density)` builtin** — generate a random sparse matrix
  with approximately `density * m * n` non-zeros, values uniformly in [0,1].
  Uses the existing `rand` dependency.

- [ ] **4d. `norm(S, p)` extension** — extend the existing `norm` builtin to
  accept `SparseMatrix` for `p = 1` (max column sum) and `p = Inf` (max row
  sum).  Convert to dense for `p = 2` (spectral norm).

- [ ] **4e. `whos` extension** — the existing `whos` builtin prints variable
  types and sizes; extend it to show `sparse [m×n, nnz=k, fill=p%]` for
  sparse values.

- [ ] **4f. Example script** `examples/sparse.rlab`:
  ```
  # Construction
  S = sparse([1,2,3,1], [1,2,3,3], [10.0,20.0,30.0,5.0], 3, 3)
  print(S)
  print(nnz(S))
  
  # Identity and arithmetic
  I3 = speye(3)
  T = S + I3
  print(full(T))
  
  # SpMV
  x = [1.0, 2.0, 3.0]
  y = S * x
  print(y)
  
  # Convert and back
  D = full(S)
  S2 = sparse(D)
  print(issparse(S2))
  
  # Tridiagonal system
  T = spdiags([-1, 2, -1], [-1, 0, 1], 5, 5)
  b = [1.0, 0.0, 0.0, 0.0, 1.0]
  x = spsolve(T, b)
  print(x)
  ```

- [ ] **4g. Unit tests**:
  - `spsolve(speye(3), [1,2,3])` == `[1,2,3]`.
  - `spdiags([1,2,3,4,5], 0, 5, 5)` produces sparse diagonal; converting to
    dense matches `diag([1,2,3,4,5])`.
  - `sprand(10, 10, 0.1)` has nnz approximately 10 (within ±5).
  - `whos` output contains "sparse" for sparse variables.

---

## Implementation Order and Session Boundaries

Each phase is one natural session.  Within a phase, work the checklist items
top-to-bottom — later items depend on earlier ones.

```
Session 1 → Phase 1 (types, construction, inspection)
Session 2 → Phase 2 (conversion, indexing, auto-promotion)
Session 3 → Phase 3 (native arithmetic, SpMV)
Session 4 → Phase 4 (solver, spdiags, sprand, example script)
```

The project compiles and all existing tests pass at the end of every session.

---

## Key Files Modified Per Phase

| File | Phase |
|---|---|
| `crates/rustlab-core/src/types.rs` | 1 |
| `crates/rustlab-core/src/lib.rs` | 1 |
| `crates/rustlab-script/src/eval/value.rs` | 1, 2, 3 |
| `crates/rustlab-script/src/eval/builtins.rs` | 1, 2, 3, 4 |
| `crates/rustlab-script/src/eval/mod.rs` | 2 (index-write) |
| `crates/rustlab-core/src/sparse.rs` *(new)* | 3 |
| `crates/rustlab-script/src/tests.rs` | 1, 2, 3, 4 |
| `examples/sparse.rlab` *(new)* | 4 |
| `docs/quickref.md` | 4 |
| `AGENTS.md` | 4 |

---

## Design Rules for the Implementing Agent

1. **No new crate dependencies** for Phases 1–3.  All sparse logic is
   implemented with `Vec<(usize, usize, C64)>` and standard Rust.

2. **1-based indexing at the script boundary**, 0-based internally.  The
   conversion happens in builtins and `Value::index` / `IndexAssign`,
   never inside `SparseVec`/`SparseMat` methods.

3. **Near-zero threshold**: drop entries where `c.norm() < 1e-15` after any
   arithmetic or construction.

4. **Entries must always be sorted** (row-major for matrix, ascending for
   vector) after every mutation.  The `::new` constructors enforce this.

5. **Auto-promotion to dense is always safe**: if a sparse op is unimplemented,
   converting to dense and falling back is correct even if slow.  Never
   return a wrong result.

6. **All existing tests must still pass** after each phase.  Run `cargo test`
   before committing.

7. **Follow the existing commit style** — no `Co-Authored-By` lines, no force
   push.  One commit per phase is fine; multiple are OK if logical.

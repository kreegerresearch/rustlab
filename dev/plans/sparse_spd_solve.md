# Development Plan: Fast sparse SPD solves (`\`, `spsolve`, `chol`, `pcg`, `ichol`)

**Status:** Phase 1 in progress (0.3.85).
**Current phase:** 1 — singleton reduction + nested-dissection Cholesky behind `\` / `spsolve`.
**Asked by:** Michael, 2026-11-23. Motivating workload: `kreegerresearch/rustlab_em` Lesson 18 (`make lesson-18`), `irregular_capacitor_3d.rlab`.

Phases after Phase 1 need an explicit go-ahead. Pure Rust, in-repo (Workflow Rule 10); no new crate dependency in any phase.

---

## 1. Measurements (rustlab 0.3.84 = `main` at `0d60175`, release build, 4-core VM)

### 1.1 Lesson 18 under `rustlab run --plot none --profile`

| Script | Wall | `spsolve` | Everything else |
|---|---|---|---|
| `irregular_capacitor_3d.rlab` | 37.1 s | **36.75 s** (4 calls) | `pin_dirichlet` 0.28 s, `laplacian_eps_3d` 0.022 s, post-processing < 0.05 s |
| `gauss_any_surface.rlab` | 0.86 s | 0.71 s (6 calls) | `pin_dirichlet` 0.11 s |
| `current_density_resistor.rlab` | 0.04 s | 0.024 s | — |
| `surface_charge.rlab` | 0.03 s | 0.023 s | — |
| **Lesson 18 total** | **~38 s** | | |

Inside `irregular_capacitor_3d`, the script's own `tic`/`toc`: sphere n = 21 0.2 s, n = 31 3.2 s, **n = 41 27.4 s**, L block 6.4 s. The operator build is not the problem; the solve is 99% of the run.

Also measured: Lesson 10 `fdfd_2d_tmz.rlab` (2-D TMz with SC-PML, complex, not symmetric, not Hermitian): 7.0 s wall, **6.90 s in `spsolve`**.

### 1.2 Which path `\` / `spsolve(A, b)` takes today

`\` on a square sparse matrix (`linsys::mldivide` → `builtins::sparse_square_mldivide`) and `spsolve(A, b)` share one auto dispatch:

1. `SparseMat::is_spd_estimate(1e-10)` (Hermitian pattern and values, real positive diagonal) → sparse up-looking Cholesky (`sparse_solve/cholesky.rs`).
2. Otherwise sparse Gilbert–Peierls LU, threshold 0.1 (`sparse_solve/lu.rs`).
3. Ordering: `OrderingHint::Identity` (stamped by `laplacian_*`, kept by `pin_dirichlet`) → natural order; no hint → `AmdOrdering`, which is the basic minimum-degree variant with an O(n²) pivot search (no quotient graph, no approximate degree). The lesson's own comment ("Natural ordering beats AMD by more than 10×, 26 s vs 459 s") is that O(n²) search.

The Lesson 18 matrices are **not symmetric**: `pin_dirichlet` replaces a pinned row with `d·e_k` but leaves column `k` in the neighbours' rows. So step 1 fails and **every Lesson 18 solve is a natural-order sparse LU**. No dense fallback is involved.

### 1.3 The 41³ sphere system

| Quantity | Value |
|---|---|
| Size | 68,921 × 68,921 (`n = 41`), `OrderingHint::Identity` |
| `nnz(A)` | 232,217 |
| Pinned rows (only a diagonal entry) | 41,705 (inner sphere + everything outside the shell) |
| Free unknowns | 27,216; the free block has 181,548 nonzeros and is **exactly symmetric, positive definite** |
| Current LU (natural) | factor **27.6 s**, `nnz(L) = 23.0 M`, `nnz(U) = 21.7 M` (716 MB of factor), peak RSS 1.59 GB, relative residual 2.9e-15 |

### 1.4 Prototype numbers (scratch probe, same matrices, same machine)

The probe builds the exact Lesson 18 operators through the interpreter, then calls the core solvers. "Reduce" = eliminate the row singletons (`x_k = b_k / a_kk`, move their columns to the right-hand side); the remaining block is the free unknowns.

| System | Method | Time | `nnz(L)` | Peak RSS | Rel. residual (full system) |
|---|---|---|---|---|---|
| Sphere 41³ | current LU, natural | 27.6 s | 44.8 M (L+U) | 1.59 GB | 2.9e-15 |
| | reduce + Cholesky, natural | 7.18 s | 20.8 M | 413 MB | 2.9e-15 |
| | reduce + Cholesky, nested dissection | **1.12 s** | 4.6 M | 161 MB | 9.2e-16 |
| | reduce + PCG, Jacobi, tol 1e-10 | 0.019 s (71 it) | — | — | 1.3e-11 |
| | reduce + PCG, IC(0), tol 1e-10 | 0.020 s (34 it) | — | — | 9.2e-12 |
| L block 29×29×15 | current LU, natural | 6.43 s | 13.0 M (L+U) | 493 MB | 2.2e-15 |
| | reduce + Cholesky, natural | 1.54 s | 6.0 M | | 2.4e-15 |
| | reduce + Cholesky, nested dissection | **0.08 s** | 0.93 M | | 5.4e-16 |
| | reduce + PCG, IC(0) | 0.006 s (31 it) | — | | 1.4e-11 |
| Sphere 31³ | current LU, natural | 3.33 s | 10.5 M | 366 MB | 1.5e-15 |
| | reduce + Cholesky, nested dissection | 0.17 s | 1.4 M | | 6.6e-16 |
| FDFD 161² (complex, 25,921) | current: basic min-degree LU | ~6.9 s | | | |
| | LU, natural | 1.32 s | 8.3 M | | 2.0e-14 |
| | LU, nested dissection | **0.20 s** | 1.6 M | | 1.9e-13 |

The nested-dissection prototype is a ~100-line level-set bisection: BFS from a pseudo-peripheral node, the middle level is the separator, recurse on both sides, order separators last, leaves of ≤ 64 nodes in natural order. Ordering time is 9 ms at 41³.

---

## 2. Options compared

| Option | 41³ solve | Exact? | MATLAB fit | Cost / risk |
|---|---|---|---|---|
| A. Sparse Cholesky + fill-reducing ordering (ND), after singleton reduction | 1.1 s (scalar up-looking kernel); est. 0.2–0.4 s with a supernodal kernel | Direct, residual ~1e-15 | `\` on sparse SPD is CHOLMOD Cholesky; UMFPACK also strips singletons first | Small: reduction ~150 lines, ND ~200 lines. Reuses `SparseChol` unchanged |
| B. PCG with Jacobi / IC(0) | 0.02 s | Iterative, residual = tolerance | MATLAB `\` never iterates; `pcg` / `ichol` are separate functions | Must not sit behind `\` (changes `\` from direct to approximate). Right as new builtins |
| C. Better LU ordering only (ND or real AMD on `A + Aᵀ`) | not measured on the sphere; FDFD 6.9 → 0.2 s | Direct | UMFPACK uses COLAMD | Needed for non-symmetric systems (FDFD). Does not use the symmetry the free block has |
| D. Real AMD (Davis quotient graph, approximate degree, supervariables) | — | — | CHOLMOD's default is AMD, falling back to METIS ND | ~600 lines. On 3-D grids ND gives less fill than AMD; AMD wins on irregular graphs |

**Recommendation:** A for `\` / `spsolve` now (Phase 1), C for non-symmetric LU next, B as new MATLAB-compatible builtins, then a supernodal / parallel kernel and real AMD if a workload needs them. Phase 1 alone takes the 41³ solve from 27.6 s to ~1.1 s, inside the 3 s target, without making `\` approximate.

---

## 3. MATLAB-compatible API

| Call | MATLAB | Plan |
|---|---|---|
| `x = A \ b` (sparse, square) | Flowchart: banded / triangular / permuted triangular → Hermitian with positive real diagonal → CHOLMOD (falls back on failure) → UMFPACK LU (singletons removed first) | Phase 1: same SPD test, plus: if `A` is not Hermitian, strip row singletons and test the free block; Hermitian-positive-diagonal → Cholesky on the free block, else (or on Cholesky failure) the unchanged LU on the full `A`. Result is still a direct solve |
| `spsolve(A, b [, mode] [, ordering])` | (rustlab-only) | Same auto path. `"cholesky"` also uses the reduction, and now **errors** when the matrix (or its free block) is not Hermitian instead of returning a wrong answer. `ordering` adds `"nd"` |
| `R = chol(S)`, `[R, p, P] = chol(S)`, `chol(S, "lower")`, `"vector"` | Sparse factor, `R'*R = P'*S*P` | Phase 4. Today `chol(S)` returns an opaque handle; MATLAB returns the sparse factor. Keep the handle form for `solve(F, b)`, add the sparse outputs |
| `x = pcg(A, b, tol, maxit, M1, M2, x0)`, `[x, flag, relres, iter, resvec] = pcg(...)` | Defaults `tol = 1e-6`, `maxit = min(n, 20)`; `A` may be a function handle; `M` a matrix or handle; prints a message when `flag ≠ 0` and `nargout < 2` | Phase 3, same signature and flags (0 converged, 1 maxit, 2 ill-conditioned M, 3 stagnated, 4 scalar too small/large) |
| `L = ichol(A)`, `ichol(A, opts)` | `opts.type = "nofill"` (IC(0), default) / `"ict"` with `droptol`, `opts.michol`, `opts.diagcomp`; returns lower `L` with `A ≈ L*L'`, reads the lower triangle | Phase 3: `"nofill"`, `"ict"`, `michol`, `diagcomp` |

---

## 4. Phases

### Phase 1 — `\` / `spsolve` fast path for sparse SPD (0.3.85)

**Status:** in progress

| # | Item | Status |
|---|---|---|
| 1 | Failing tests first: singleton split, ND ordering validity and fill, `\` and `spsolve` on pinned Laplacians (residual and agreement with the 0.3.84 LU), `"cholesky"` error on a non-Hermitian matrix, multi-RHS `\` | not started |
| 2 | `rustlab-core/src/sparse_solve/singleton.rs`: `RowSingletonSplit<T>` — rows whose only stored nonzero is a nonzero diagonal; free block, coupling block, `reduce_rhs`, `assemble` | not started |
| 3 | `ordering.rs`: `NestedDissectionOrdering` (level-set bisection on the pattern of `A + Aᵀ`, deterministic, disconnected components handled, leaves ≤ 64 natural) | not started |
| 4 | `ordering.rs`: `SpdAutoOrdering` — compute the symbolic `nnz(L)` of ND, then of the natural order with a cap at ND's count, pick the smaller (tie → natural). Replaces the hint / basic-AMD choice for **Cholesky** in `"auto"` | not started |
| 5 | `builtins.rs`: shared `sparse_auto_solve` used by `\` (one and many columns) and `spsolve` auto / `"cholesky"`; `"nd"` ordering string; LU path and LU ordering unchanged | not started |
| 6 | Docs (`functions.md`, `sparse_solve.md`, `quickref.md` if needed, REPL help for `spsolve` / `chol`), `AGENTS.md`, CHANGELOG, version 0.3.85 | not started |
| 7 | Measure: Lesson 18 scripts before/after, `make perf`'s sparse benches, the dense `\` path unchanged | not started |

Not in Phase 1: LU ordering, a new factor kernel, threads, `pcg`, `ichol`, sparse `chol` outputs.

### Phase 2 — non-symmetric LU ordering (FDFD)

Use `NestedDissectionOrdering` (on `A + Aᵀ`) instead of the O(n²) basic minimum degree when there is no hint, and choose between natural and ND by a fill estimate when there is one. Target: `fdfd_2d_tmz` spsolve 6.9 s → under 0.5 s. Changes the last digits of every no-hint sparse LU result (version bump, CHANGELOG).

### Phase 3 — `pcg` and `ichol`

MATLAB signatures above. Function-handle `A` and `M`. Tests: convergence on a Poisson matrix in the iteration count bound, `flag` values, `ichol` `L*L'` equals `A` on the pattern, `"ict"` with `droptol = 0` equals the full Cholesky factor.

### Phase 4 — sparse `chol` outputs

`R = chol(S)`, `[R, p] = chol(S)` (p = 0 or the failing column), `[R, p, P] = chol(S)` and `"vector"` with the fill-reducing ordering, `"lower"`. What one-output `chol(S)` returns is open question 3.

### Phase 5 — faster factor kernel

Supernodal left-looking Cholesky (dense panel updates through the existing `matmul_real`), rayon over independent elimination subtrees (ND makes the tree balanced), bit-identical to one thread. Real Davis AMD for irregular graphs if a workload needs it. Target: 41³ sphere factor < 0.4 s.

---

## 5. Accuracy tests (all phases)

- Relative residual `‖A x − b‖ / ‖b‖ ≤ 1e-12` on: 1-D/2-D/3-D Laplacians, `laplacian_eps_3d` with a 4.4 / 1 interface, the Lesson 18 sphere at n = 21 (pinned inside and outside), random SPD (`B'B + nI`), complex Hermitian.
- Agreement with the 0.3.84 natural-order LU: `‖x_new − x_lu‖ / ‖x_lu‖ ≤ 1e-12`.
- Exactness of the reduction: pinned entries equal `b_k / a_kk` bit for bit.
- Every ordering is a permutation; ND fill on a 20³ Laplacian is at most half the natural fill.
- Fallbacks: a free block that is Hermitian but indefinite falls back to LU (same answer as 0.3.84); a non-Hermitian matrix with no singletons takes the unchanged LU.
- Lesson 18 `irregular_capacitor_3d` prints the same capacitances to the printed digits.

## 6. Performance targets

| Workload | 0.3.84 | Phase 1 target |
|---|---|---|
| Lesson 18 sphere 41³ solve | 27.4 s | < 3 s (prototype 1.1 s) |
| Lesson 18 L block | 6.4 s | < 0.5 s |
| Lesson 18 four scripts total | ~38 s | < 5 s |
| `fdfd_2d_tmz` | 7.0 s | unchanged in Phase 1; < 1 s after Phase 2 |
| Other sparse / dense `\` | — | no slowdown beyond noise (measure `make perf` and a dense 500×500 `\`) |

---

## 7. Open questions for Michael

1. **Hint vs auto.** In `"auto"`, Phase 1 ignores `OrderingHint::Identity` for Cholesky and picks natural or ND by predicted fill (natural still wins on 1-D and thin bands). `"identity"` / `"natural"` still force natural order. OK?
2. **`pin_dirichlet` symmetric form.** Should `pin_dirichlet` get an option that also zeroes the pinned columns (moving `A(:, k)·value` into `b`), so `A` is SPD before any solve and `chol(A)` / `pcg(A, …)` apply directly? Phase 1 does not need it (`\` strips the singletons itself). `pcg` on the unpinned-column form is non-symmetric, so Phase 3 would want it or a doc note.
3. **Sparse `chol` return type.** MATLAB `R = chol(S)` returns the sparse factor; rustlab returns a solve handle. Change `chol(S)` to MATLAB's form (breaking) and move the handle to `F = decomposition(S, "chol")` (MATLAB's name), or keep the handle and add MATLAB's outputs only for `nargout ≥ 2`?
4. **LU ordering change (Phase 2)** changes the last digits of FDFD results. Fine with a CHANGELOG note?
5. **`pcg` no-convergence message.** MATLAB prints a message when `flag ≠ 0` and `nargout < 2`. Match it (stderr warning), or error?
6. **Threads in Phase 5.** Require bit-identical results for any thread count (as FDTD does), at some cost in parallel speedup?

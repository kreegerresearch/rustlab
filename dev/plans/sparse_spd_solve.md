# Development Plan: Fast sparse SPD solves (`\`, `spsolve`, `chol`, `pcg`, `ichol`)

**Status:** Phase 1 complete (0.3.85, PR #166). Phase 2 complete (0.3.86). Phases 3–5 not started.
**Current phase:** 3 — `pcg` and `ichol`; waiting for Michael's go-ahead.
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

**Status:** complete

| # | Item | Status |
|---|---|---|
| 1 | Failing tests first: singleton split, ND ordering validity and fill, `\` and `spsolve` on pinned Laplacians (residual and agreement with the 0.3.84 LU), `"cholesky"` error on a non-Hermitian matrix, multi-RHS `\` | done |
| 2 | `rustlab-core/src/sparse_solve/singleton.rs`: `RowSingletonSplit<T>` — rows whose only stored nonzero is a nonzero diagonal; free block, coupling block, `reduce_rhs`, `assemble` | done |
| 3 | `ordering.rs`: `NestedDissectionOrdering` (level-set bisection on the pattern of `A + Aᵀ`, deterministic, disconnected components handled, leaves ≤ 64 natural) | done |
| 4 | `ordering.rs`: `SpdAutoOrdering` — compute the symbolic `nnz(L)` of ND, then of the natural order with a cap at ND's count, pick the smaller (tie → natural). Replaces the hint / basic-AMD choice for **Cholesky** in `"auto"` | done |
| 5 | `eval/sparse_auto.rs::factor`, used by `\` (one and many columns) and `spsolve` auto / `"cholesky"`; `"nd"` ordering string; LU path and LU ordering unchanged | done |
| 6 | Docs (`functions.md`, `sparse_solve.md`, `quickref.md` if needed, REPL help for `spsolve` / `chol`), `AGENTS.md`, CHANGELOG, version 0.3.85 | done |
| 7 | Measure: Lesson 18 scripts before/after, `make perf`'s sparse benches, the dense `\` path unchanged | done |

Not in Phase 1: LU ordering, a new factor kernel, threads, `pcg`, `ichol`, sparse `chol` outputs.

#### Phase 1 results (release, same 4-core VM, best of 2 runs)

| Workload | 0.3.84 | 0.3.85 |
|---|---|---|
| Lesson 18 sphere 41³ solve (script `toc`) | 27.4 s | 1.3 s |
| Lesson 18 L block (script `toc`) | 6.4 s | 0.1 s |
| `irregular_capacitor_3d.rlab` | 36.3–37.8 s | 1.78 s |
| Lesson 18, four scripts | 37.3–38.8 s | 2.05 s |
| Lesson 15 `parasitic_bondpad` / `lumped_C_parallel_plate` / `MIM_capacitor_3d` | 29.6 / 8.7 / 8.4 s | 0.87 / 0.36 / 0.36 s |
| Lesson 13 `twin_wire_impedance` | 1.05 s | 0.14 s |
| `-laplacian_2d(200, 200) \ b` / `-laplacian_3d(30, 30, 30) \ b` | 0.61 / 7.9 s | 0.12 / 0.98 s |
| `fdfd_2d_tmz` (LU, unchanged path) | 7.5 s | 7.7 s (noise) |
| dense 500×500 `\` SPD / LU, ×5 | 0.115 / 0.199 s | 0.114 / 0.203 s |
| `perf/bench_*.rlab` (7 scripts) | 4–126 ms | same within 2 ms |

Every rustlab_em lesson script that calls a solver (27, plus `_shared/em.rlab`) and every repo example with a solve exits 0 on both versions. Printed results agree to roundoff (worst relative difference 7e-12 on a value above 1e-9), except Lesson 05 `parallel_plate.rlab` at 1.6e-10: it pins with hand-written identity rows next to `1/dx²` stencil entries, and 0.3.85 is the more accurate one (relative residual 1.06e-8 → 3.8e-9, pinned-value error 3.7e-9 → 0).

Left for later: `examples/notebooks/sparse_scaling.md` and `sparse_solve.md` still describe AMD as the `spsolve` default and carry 0.3.x benchmark tables (already stale before this phase, since the `laplacian_*` hint); updating them means regenerating their gallery output. Sparse `\` returns a 1-D vector where MATLAB returns an `n×1` column (dense `\` already returns `n×1`), unchanged here.

### Phase 2 — non-symmetric LU ordering (FDFD)

**Status:** complete (0.3.86)

`"auto"` LU ordering becomes the Phase 1 rule applied to the pattern of `A + Aᵀ`: nested dissection or the natural order, whichever predicts the smaller symmetric factor, whatever the hint (decision 1). That replaces the O(n²) basic minimum degree for matrices without a hint (FDFD) and the natural order for `laplacian_*` matrices. Partial pivoting (threshold 0.1) can move rows, so the symmetric count is an estimate, not the LU fill. Target: `fdfd_2d_tmz` solve 6.9 s → under 0.5 s. Last digits of auto sparse LU results change (decision 4).

| # | Item | Status |
|---|---|---|
| 1 | Failing tests first: auto LU fill equals the nested-dissection fill on a no-hint non-symmetric grid and the natural fill on a band; FDFD-style complex system (`fdfd_tmz_pml_2d`) solved by `\\` to residual 1e-10 and agreeing with the 0.3.85 AMD LU; `size(A \\ b)` and `size(spsolve(A, b))` are `[n, 1]` on every path | done |
| 2 | `SpdAutoOrdering` counts fill on the pattern of `A + Aᵀ` (identical for symmetric input), so the LU path can use it | done |
| 3 | `factor_sparse_lu`: `"auto"` → that ordering, hint ignored | done |
| 4 | `spsolve_pack` and the one-column `\\` path return `n×1` (real storage when real); a 1×1 result stays a scalar | done |
| 5 | Fix callers in tests, examples, notebooks that assumed a 1-D result; check rustlab_em | done |
| 6 | Docs, REPL help, AGENTS.md, CHANGELOG, version 0.3.86 | done |
| 7 | Measure FDFD scripts before/after, accuracy vs 0.3.84, shape-fix cost | done |

#### Phase 2 results (release, same 4-core VM)

| Workload | 0.3.84 | 0.3.85 | 0.3.86 |
|---|---|---|---|
| `fdfd_2d_tmz` solve (161², complex, 25,921 unknowns, 128,961 nonzeros) | 6.89 s | — | 0.225 s |
| `fdfd_2d_tmz.rlab` | 7.13 s | 7.20 s | 0.35 s |
| `fdfd_pml_demo.rlab` | 2.58 s | 2.79 s | 0.27 s |
| `fdfd_resonator.rlab` | 1.47 s | 1.38 s | 0.77 s |
| `fdfd_1d_layers.rlab` | 0.09 s | 0.09 s | 0.09 s |
| Lesson 15 `cap_matrix_microstrip` / `cap_matrix_three_trace`, Lesson 17 `ind_matrix_microstrip` | 0.75 / 0.18 / 0.18 s | 0.79 / 0.18 / 0.16 s | 0.38 / 0.09 / 0.09 s |
| Lesson 18, four scripts | ~41 s | 2.18 s | 2.25 s |

Accuracy: `fdfd_2d_tmz` relative residual 1.9e-14 (0.3.84) → 1.1e-13; the solution differs from the 0.3.84 LU by 8.5e-14 relative. Every printed result of the 27 rustlab_em solver scripts agrees with 0.3.84 to roundoff (FDFD lessons 3e-14 or better); all exit 0. All 18 rustlab_em notebooks render under `--strict` with no cell error.

Shape fix: no rustlab_em script or notebook needed a change (they use `reshape` or `x(:)`). In this repo, `examples/pde/laplacian.rlab`, `examples/sparse/sparse_complex.rlab`, `docs/examples.md`, and the `laplacian` / `sparse_complex` notebooks transposed the 1-D result and were changed (gallery regenerated for the two notebooks). Eight unit tests and one integration test used the same idioms; one of them (`V = zeros(1, N)` minus the solve) broadcast silently to N×N rather than erroring, which is the hazard to watch for downstream.

Cost of the `n×1` result (rule in §7): none measurable at size; a 50×50 diagonal `\` in a 50,000-iteration loop is 5.9 → 6.2 µs per call (+0.4 µs, building the real `n×1` value). An early version of the LU ordering built the `A + Aᵀ` pattern even for symmetric input and cost 45 ms on a 10⁶-row diagonal solve; it now reuses `A` when the pattern is already symmetric (1.16 s for five solves, same as 0.3.85).

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

## 7. Decisions (Michael, 2026-11-23)

**Rule: match MATLAB unless doing so costs significant speed or memory.** When a MATLAB-matching choice (explicit `chol` factor, `n×1` shape, `pcg` semantics, ordering, determinism) would cost significant time or memory, do not silently pick a side: measure both, keep the fast path, and report the trade-off with numbers so Michael can decide.

1. **Hint vs auto — yes, ignore the hint.** MATLAB's `\` picks its own fill-reducing ordering. `"auto"` may ignore `OrderingHint::Identity` for Cholesky (Phase 1) and for LU (Phase 2). `"identity"` / `"natural"` still force the natural order.
2. **`pin_dirichlet` — unchanged.** It keeps its MATLAB-compatible output (pinned rows, columns left in place). No symmetric option; the Phase 1 singleton reduction handles the solve. `pcg` docs (Phase 3) note that a `pin_dirichlet` operator is not symmetric.
3. **Sparse `chol(S)` returns the upper factor `R` with `R'*R = S`, as in MATLAB**, including `[R, p] = chol(S)` and `[R, p, P] = chol(S)` (`R'*R = P'*S*P`, fill-reducing `P`) and `"vector"` / `"lower"`. Phase 4 scope.
   - *Migration for the solve handle.* `F = chol(S); x = solve(F, b)` becomes either the MATLAB idiom `[R, p, P] = chol(S); x = P * (R \ (R' \ (P' * b)))`, or `x = S \ b` (one solve), or MATLAB's reusable form `dS = decomposition(S, "chol"); x = dS \ b`, which Phase 4 adds and which keeps today's factor-once behaviour. `lu(S)` handles and `solve(F, b)` stay until `decomposition` covers them; a sparse `F = chol(S)` passed to `solve` errors with a message naming the replacement. CHANGELOG gets a breaking-change entry with these three lines.
   - *To measure in Phase 4 (rule above):* returning `R` as a `SparseMat` (COO, `C64` entries) needs roughly twice the memory of the internal real CSC factor (41³ sphere: ~150 MB vs ~74 MB), and `R \ (R' \ b)` must hit a sparse triangular solve, not LU.
4. **Last-digit changes in FDFD results are acceptable.** MATLAB does not promise bit-identical results across versions.
5. **`pcg` matches MATLAB.** With fewer than two outputs, a non-converged run warns (stderr) and returns the best iterate; with `flag` requested, it returns `flag`, `relres`, `iter`, `resvec` without a warning.
6. **Parallel factor (Phase 5).** Results need not be bit-identical across thread counts. They must be deterministic for a fixed thread count and meet the accuracy tolerance.
7. **Shape fix:** sparse `\` and `spsolve` return an `n×1` column, as MATLAB does (Phase 2, 0.3.86).

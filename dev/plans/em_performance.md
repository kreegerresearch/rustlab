# Implementation Plan — EM Gallery Performance

> **For the next agent:** This is both the *reference* doc (rationale, decisions, file landmarks) and the *action* doc (status table, per-phase steps). Read top to bottom, find the next phase whose Status is `pending`, follow its checklist.
> Source analysis: turn-3 of session 2026-05-09; user asked "in the EM examples what are the bottlenecks and what can be improved computationally?" Answer enumerated 7 bottlenecks; user replied "fix them fix them all" → "lets plan this in phases" → this document.
> Companion plan: `dev/plans/em_requests_plan.md` (closed work that built the foundations this plan optimizes — sparse Cholesky, sparse LU, AMD, real-only Cholesky fast path).

**Date opened:** 2026-05-09
**Plan status:** Phase order locked; no phase started.

## Status snapshot

| # | Phase | Status | Risk | Win | Commit |
|---|---|---|---|---|---|
| 1 | Reusable Cholesky factor (`chol(A)` / `lu(A)` / `solve(F, b)`) | **shipped** | low | 10–100× on sweeps/animations | `7311bf1` |
| 2 | Identity-ordering fast path for grid Laplacians | **shipped** | low | ~5× on grid solves | `ddb78f8` |
| 3 | Fused, parallel `gradient` / `divergence` / `curl` | **shipped** | low–med | 3–8× on postprocess | `0e70b1a` |
| 4 | Direct CSC build in `laplacian_*` builders | **shipped** | low | 13–22× builder speedup | `810806a` |
| 5 | Real `f64` path for `vector_calc.rs` + Laplacian builders | **investigated, deferred** | — | regression on this kernel — see notes | — |
| 6 | Symbolic-then-numeric Cholesky on flat CSC | **awaiting commit** | med | 5–11% factor speedup at n≥150 | — |

*Status legend:* `pending` (not started) · `in progress` (branch open) · `awaiting commit` (code/tests/docs landed locally, not yet committed — user approval required) · `blocked` (note why) · `shipped` (commit hash).
When you advance a phase, update its row **and** the per-phase section's Status field.

## Decisions already locked (do not re-litigate)

1. **Sparse LU is already shipped** (`e9283b7`). Dense LU fallback is intentional for `Value::Matrix` inputs. Do not propose replacing it.
2. **Real-only Cholesky fast path is already shipped** — see `try_sparse_cholesky` in `builtins.rs:10433` which auto-detects `all_real` and routes through `SparseCsc<f64>`. Phase 5 is *not* about adding this; it's about extending the same idea to the **DSP layer** (`vector_calc.rs`, `laplacian.rs` builder output) so the entries handed to Cholesky aren't pre-promoted to `C64`.
3. **AMD is the current default ordering** in both `try_sparse_cholesky` and `try_sparse_lu`. Phase 2 keeps AMD as the safe default and adds an explicit Identity opt-in plus a builder-side hint that auto-selects Identity for grid Laplacians.
4. **Pure-Rust hand-roll, no FFI, no third-party numerics libraries** (`AGENTS.md` Rule 9). The *math itself* — stencils, factorizations, orderings, eigensolvers — is hand-rolled. **`rayon` is acceptable infrastructure** (user-confirmed 2026-05-09): it provides parallel orchestration, not numerics, so importing it for outer-axis parallelism on a kernel we wrote ourselves is fine. `std::simd` (nightly) is out — keep on stable. Compiler autovectorization via slice iteration is the SIMD strategy.
5. **No new public crate.** All work lands in existing crates: `rustlab-core`, `rustlab-dsp`, `rustlab-script`, `rustlab-cli`.
6. **`docs/sparse_solve.md`** is the canonical end-to-end design doc for the sparse-solve pipeline (dispatch chain, ordering hint, factor reuse, Davis algorithms). Update it when phases land that change pipeline behavior.

## Six mandatory workflow rules (apply to every phase)

Per `feedback_workflow.md` and `AGENTS.md`:
1. **Plan first** — this doc *is* the plan. Per-phase tweaks need user approval if they change scope.
2. **Tests in the same commit** — algorithm tests in their owning crate (`rustlab-dsp/src/tests.rs`, `rustlab-core/src/sparse_solve/tests.rs`); builtin tests in `rustlab-script/src/tests.rs`. Run `cargo test --workspace` *and* `cargo test --workspace --features viewer` before declaring done.
3. **No commit without explicit approval** — present a summary, wait for "commit" / "push".
4. **Update `AGENTS.md`** function table (`AGENTS.md:817-925` per the companion plan; re-verify line range).
5. **Update `docs/quickref.md`** for every new builtin.
6. **Update REPL help** — `HelpEntry { name, brief, detail }` in `crates/rustlab-cli/src/commands/repl.rs` *and* the relevant `categories` slice in `print_help_list`. `help foo` must work before declaring done.

A feature is **not done** until `help foo` returns a useful answer.

## Verified file:line landmarks (re-verify if more than ~14 days old; this list captured 2026-05-09)

- `rustlab-core/src/sparse_solve/mod.rs` — public API; re-exports `OrderingMethod`, `IdentityOrdering`, `ColCountOrdering`, `AmdOrdering` at line ~31.
- `rustlab-core/src/sparse_solve/cholesky.rs` — `SparseChol<T>::factor` at line 39; numeric loop using `cols_l: Vec<Vec<(usize, T)>>` at line 58.
- `rustlab-dsp/src/vector_calc.rs` — `d_dx` (61), `d_dy` (82), `gradient_2d` (103), `divergence_2d` (113), `curl_2d` (124), `d_along_axis_3d` (155), `gradient_3d` (223), `divergence_3d` (240), `curl_3d` (263). All operate on `CMatrix` / `CTensor3` (C64-only).
- `rustlab-dsp/src/laplacian.rs` — `laplacian_1d` (83), `laplacian_2d_bc` (118), `laplacian_3d` (195), `laplacian_eps_2d` (337). All emit `Vec<(usize, usize, C64)>` triplets and call `SparseMat::new`.
- `rustlab-script/src/eval/builtins.rs` — `builtin_spsolve` at 10342; `try_sparse_cholesky` at 10433 (this is where the `all_real` fast path lives and where Phase 1 / Phase 2 dispatch will sit); `r.register("spsolve", …)` at 295.
- `rustlab-cli/src/commands/repl.rs` — `HelpEntry` struct around line 13; `categories` table around 813; `print_help_list` around the same area.

## Cross-cutting test fixtures

Each phase reuses these. If you make a fixture, put it in `crates/rustlab-dsp/src/tests.rs` with a `pub(crate)` helper.

- **Grid Poisson reference:** 50×50 Dirichlet Laplacian with analytic solution `V_exact = sin(πi*dx/Lx)*sin(πj*dy/Ly)`, RHS = `L * V_exact(:)`. Round-trip `spsolve` should match `V_exact` to `< 1e-10` relative. (Already used in the closed `sparse_solve_handroll.md` plan — reuse the helper.)
- **Quadrupole RHS sweep:** 100×100 grid, four point charges at `(30,30), (30,70), (70,30), (70,70)` with alternating signs. Used in `gallery/electrostatics.md`. Phase 1 will solve this with both `spsolve` (refactor each call) and `chol(A); solve(F, b)` (one factor); both must agree to `< 1e-12`.
- **Vector-calc analytic checks:** `F = x²+y²` → `∇F = (2x, 2y)`; `F = (x, y)` → `div = 2`; `F = (-y, x)` → `curl_z = 2`. Already in `gallery/vector_calculus.md`; the equivalence test for Phase 3 is "fused output equals reference output to bit precision on identical inputs."

---

## Phase 1 — Reusable Cholesky factor

**Status:** shipped — commit `7311bf1` (2026-05-09)
**Goal:** expose the existing `SparseChol::factor` to the script layer so users can factor once and solve many RHS. This is the highest-impact change for the gallery's stated "parameter sweeps, animations, embedding" use cases (see `gallery/electrostatics.md:155-157`).

**Implementation log (2026-05-09):**
- Added `SparseFactor` enum (`CholReal`/`CholComplex`/`LuReal`/`LuComplex`, each `Arc<…>`-wrapped) and `Value::SparseFactor(SparseFactor)` variant in `crates/rustlab-script/src/eval/value.rs`. Display impl reports `<chol factor N×N, real, nnz=…>` form.
- Refactored `try_sparse_cholesky`/`try_sparse_lu` in `builtins.rs` into reusable `factor_sparse_cholesky` / `factor_sparse_lu` / `solve_with_factor` helpers. The original `try_*` functions now delegate to those helpers, so `spsolve` behavior is unchanged.
- Registered `chol`, `lu`, `solve` builtins in the registry (next to `spsolve` at builtins.rs:295). `chol` errors on non-SPD with no auto fallback. `lu` works on indefinite/non-Hermitian/complex. `solve` runs the cached back-solve.
- Tests in `crates/rustlab-script/src/tests.rs` (sparse_tests submodule): `chol_then_solve_matches_spsolve`, `chol_factor_reused_for_two_rhs`, `lu_factor_reused_for_complex_rhs`, `chol_on_non_spd_errors_cleanly`, `solve_rejects_non_factor_first_arg`, `solve_rejects_dim_mismatch`, `chol_real_path_real_factor` — all 7 pass.
- Docs: REPL `HelpEntry`s for chol/lu/solve added next to spsolve; added to "Sparse" categories slice. AGENTS.md function table updated. docs/quickref.md and docs/functions.md updated.

**Scope:**
- Add a new `Value::SparseChol` variant (or equivalent) wrapping `SparseChol<f64>` *or* `SparseChol<C64>`. Auto-pick real path when input is `all_real` (mirror the dispatch in `try_sparse_cholesky` at `builtins.rs:10433`).
- Add a `Value::SparseLu` variant the same way, wrapping `SparseLu<…>`. Symmetry with Cholesky; users will want it.
- New builtin `chol(A)` returning `Value::SparseChol`. Errors if `A` is not SPD (mode = "cholesky" — no auto fallback, since the user explicitly asked for a Cholesky factor).
- New builtin `lu(A)` returning `Value::SparseLu`. Symmetric.
- New builtin `solve(F, b)` that dispatches on the factor variant and runs the cached triangular solve. (Don't overload `\` — keep the surface explicit; `\` overload can be a Phase 1.5 if requested.)
- Existing `spsolve(A, b)` is unchanged.

**Files affected:**
- `crates/rustlab-script/src/value.rs` — new variants. Print form, type name, equality (probably `false`-by-default for opaque factor handles).
- `crates/rustlab-script/src/eval/builtins.rs` — register `chol`, `lu`, `solve`; refactor `try_sparse_cholesky` so the factor and the back-solve are separable functions reused by both `builtin_spsolve` and `builtin_solve`.
- `crates/rustlab-cli/src/commands/repl.rs` — three new `HelpEntry` records; add to the "Sparse" category slice.
- `AGENTS.md` function table.
- `docs/quickref.md` Sparse section.
- `crates/rustlab-script/src/tests.rs` — round-trip + multi-RHS test.

**Tests (must ship in same commit):**
1. `chol_then_solve_matches_spsolve` — 50×50 grid Poisson; `spsolve(A, b)` vs `solve(chol(A), b)` agree to `1e-12`.
2. `chol_factor_reused_for_two_rhs` — factor once, solve `b1` and `b2`, both agree with `spsolve(A, b_i)`.
3. `lu_factor_reused_for_complex_rhs` — symmetric for LU on a small complex non-Hermitian matrix.
4. `chol_on_non_spd_errors_cleanly` — calling `chol` on an indefinite matrix returns `SparseSolveError::NotSpd` propagated as a script error, not a panic.
5. `help_chol_returns_text` and `help_solve_returns_text` (REPL help integration test pattern — see existing tests in `rustlab-cli/tests/`).

**Acceptance:**
- A 100×100 quadrupole sweep across 50 RHS configurations runs in `< 1s` factor + `< 0.5s` total solves on a quiet laptop. Compare to baseline (50× `spsolve`) which is `~50 × 0.028s = 1.4s` (per the table in `gallery/electrostatics.md:131-137`). Expected speedup ~2–5×, larger for bigger grids.
- `gallery/electrostatics.md` gets a new "Parameter sweep with cached factor" subsection demonstrating the API. (Update gallery in the same commit or a follow-up; user choice.)

**Risk:** low. Pure additive API; no existing tests should break. Main risk is `Value` variant churn — verify the printer / equality / serialization paths don't assume an exhaustive match (`#[non_exhaustive]` or `_ =>` arms).

**Estimated size:** ~250 LoC implementation + ~150 LoC tests. One session.

---

## Phase 2 — Identity-ordering fast path for grid Laplacians

**Status:** shipped — commit `ddb78f8` (2026-05-09)

**Implementation log (2026-05-09):**
- Added `OrderingHint` enum (single variant `Identity` for now) in `crates/rustlab-core/src/types.rs` and re-exported from the crate root.
- Added `SparseMat::ordering_hint: Option<OrderingHint>` field. `SparseMat::new` and `from_dense` default it to `None`. `scale`, `transpose`, and the script-layer negation preserve the hint (structure-preserving). `add` / `sub` go through `SparseMat::new`, dropping the hint (correctly — the union of patterns may not be grid-banded). New builder method `with_ordering_hint(self, h) -> Self`.
- All four `laplacian_*` builtins now attach `OrderingHint::Identity` to their results (in `builtins.rs` next to each builder call).
- New internal `OrderingChoice` enum (`Auto` / `Identity` / `Amd`) in `builtins.rs`. `parse_ordering_arg` parses the script string. `resolve_ordering` collapses `Auto` against the matrix hint (`Identity` if set, else `Amd`).
- `factor_sparse_cholesky` and `factor_sparse_lu` now take an `OrderingChoice` and dispatch to `IdentityOrdering` or `AmdOrdering` as resolved.
- `spsolve(A, b, mode, ordering)` — added optional 4th arg.
- `chol(A, ordering)` and `lu(A, ordering)` — added optional 2nd arg.
- Tests in `crates/rustlab-script/src/tests.rs` (sparse_tests submodule): `laplacian_2d_carries_identity_hint`, `user_built_sparse_has_no_hint`, `negation_preserves_hint`, `add_drops_hint`, `spsolve_with_identity_hint_correctness`, `spsolve_explicit_identity_works`, `spsolve_unknown_ordering_errors`, `chol_with_explicit_amd_overrides_hint`, `lu_with_natural_alias_works` — all 9 pass; existing 19 sparse tests still pass.
- Docs: REPL HelpEntry for spsolve/chol/lu updated. AGENTS.md and docs/quickref.md function tables updated. docs/functions.md ordering section added.
**Goal:** make `spsolve(laplacian_2d(...), b)` use natural ordering by default. `gallery/laplacian_bc.md:140` already documents that natural identity is ~5× faster than AMD on grids — so the default penalizes the documented common case.

**Scope (two parts):**
- **(2a) Builder-side hint.** Add a `SparseMat` field `ordering_hint: Option<&'static str>` (or a richer enum) defaulted to `None`. Set it to `Some("identity")` in every `laplacian_*` builder in `rustlab-dsp/src/laplacian.rs`. Empty for COO inputs the user assembles by hand.
- **(2b) Dispatch consumes the hint.** In `try_sparse_cholesky` and `try_sparse_lu` (`builtins.rs:10433` and the LU sibling), check the hint; if `Some("identity")`, use `IdentityOrdering` instead of `AmdOrdering`. Explicit user opt-in via `spsolve(A, b, "identity")` overrides the hint either way.
- **(2c) Optional explicit ordering arg.** Extend the mode parameter to accept ordering: `spsolve(A, b, "auto")`, `spsolve(A, b, "cholesky")`, `spsolve(A, b, "lu")`, *plus* `spsolve(A, b, "auto", "identity"|"amd")`. Or use `"cholesky:identity"` syntax. **Decision needed before implementing — flag for user input at start of Phase 2.**

**Files affected:**
- `crates/rustlab-core/src/types.rs` — `SparseMat` field. Default in `SparseMat::new`.
- `crates/rustlab-dsp/src/laplacian.rs` — set hint in all four builders.
- `crates/rustlab-script/src/eval/builtins.rs` — dispatch consumes hint; mode parser if (2c) lands.
- `AGENTS.md`, `docs/quickref.md`, REPL help.

**Tests:**
1. `laplacian_2d_carries_identity_hint` — round-trip the builder, inspect the hint.
2. `spsolve_with_identity_hint_correctness` — 100×100 grid Poisson with hint vs without; results agree to `1e-12`.
3. `spsolve_with_explicit_amd_overrides_hint` (if 2c lands).
4. Perf check (not a unit test — add a benchmark or a numbered comment in `perf/`): 100×100 SPD with hint should be `≥ 3×` faster than without on the test machine.

**Acceptance:**
- All 100×100 examples in `gallery/electrostatics.md`, `gallery/dielectric.md`, `gallery/laplacian_bc.md` re-bake with their reported solve times dropping to roughly the "Identity ordering" column of `perf/sparse_solve_phase1to4.md`. Update the perf table in `electrostatics.md:131-137` if the new defaults change the headline numbers.

**Risk:** low. Behavioural change but only on grid-built matrices; user-assembled `SparseMat` from triplets is unchanged. Edge case: a user mutates a `SparseMat` returned from `laplacian_2d` (multiplies by `-1`, adds a perturbation) — the hint should propagate through arithmetic operators that preserve sparsity pattern; if mutation breaks structure it should be invalidated. Audit `Mul`, `Add`, `Sub` impls on `SparseMat`.

**Estimated size:** ~150 LoC + tests. Half a session.

---

## Phase 3 — Fused, parallel `gradient` / `divergence` / `curl`

**Status:** shipped — commit `0e70b1a` (2026-05-09)
**Goal:** rewrite the finite-difference kernels so they (a) iterate via slice views (no `[[i,j]]` bounds checks per element), (b) fuse `divergence` and `curl` into single-pass kernels that don't allocate intermediate `CMatrix` / `CTensor3`, and (c) parallelize over the outer axis with `rayon` for large grids.

**Implementation log (2026-05-09):**
- Added `rayon = "1.10"` to `Cargo.toml` workspace deps and to `crates/rustlab-dsp/Cargo.toml`. Acceptable infrastructure (parallel orchestration, not numerics) per user-confirmed clarification of `AGENTS.md` Rule 9.
- Rewrote `crates/rustlab-dsp/src/vector_calc.rs` (~250 → ~600 lines) end-to-end. The 2-D path now operates on row slices via `as_slice` / `as_slice_mut` after an `as_standard_layout` guard at each public entry. Fused `divergence_2d` and `curl_2d` write directly to the output in a single sweep. The 3-D path retains `[[i,j,k]]` indexed access (mixed strides per axis make slice views complicated) but the fused `divergence_3d` writes the output in one sweep instead of allocating three per-axis derivatives plus two summation temporaries.
- Parallelism: `PAR_THRESHOLD = 4096`. 2-D kernels use `out_slice.par_chunks_mut(nx)` over rows. 3-D `divergence_3d` uses page-parallel `par_iter` over `0..p` collecting per-page slabs (correctness-first; full `axis_chunks_iter_mut` redesign deferred). Test-only `__test_set_par_threshold(Some(1))` knob forces the parallel path for tests.
- Fixed a `Fn`-vs-`FnMut` issue in tests by pre-generating LCG values into a Vec.
- Tests: 5 new tests in `vector_calc_phase3_tests` — fused-vs-naive equivalence (2-D div + curl), parallel-vs-serial bit-equality (2-D), parallel-vs-serial < 1e-12 agreement (3-D), and non-contiguous input handling. All pass; the 11 pre-existing analytic-check tests still pass.
- Bench: new `cargo run --release --example bench_vector_calc -p rustlab-dsp` example. Numbers in `perf/em_performance_phase3.md`. 100×100 gradient: 0.12 ms; 200×200 divergence: 0.13 ms; 800×800 gradient: 0.51 ms; 80³ divergence: 2.03 ms.
- Allocation savings per call: `divergence_2d` 3→1 CMatrix, `curl_2d` 3→1 CMatrix, `divergence_3d` 4→1 CTensor3 (plus per-page scratch in the parallel path).
- Workspace tests pass under both default and `--features viewer`.

**Scope:**
- Rewrite `d_dx`, `d_dy` in `vector_calc.rs:61-97` using `f.row(i).as_slice().unwrap()` for stride-1 access and direct indexed write into the output's row slice. Same for `d_along_axis_3d` (155). Defensive `as_standard_layout()` guard at the entry point of each public function so non-contiguous inputs don't break the inner loop.
- Add fused private helpers `fused_div_2d`, `fused_curl_2d`, `fused_div_3d`, `fused_curl_3d` that compute the result in one sweep without creating per-axis derivative matrices. The current `d_dx(fx) + d_dy(fy)` pattern at `vector_calc.rs:120` allocates 3 full CMatrices; the fused version allocates 1.
- Add a parallelism threshold: when `n*m >= 4096` (cheap heuristic, tune empirically), use `rayon::par_iter` over the outer axis. Below the threshold, stay serial — rayon overhead dominates on tiny grids.
- Public API (`gradient_2d`, `divergence_2d`, `curl_2d`, `gradient_3d`, `divergence_3d`, `curl_3d`) keeps its current signature. Internal implementation routes to fused/parallel paths.
- Compiler autovectorization is the SIMD strategy. Stride-1 slice loops with simple arithmetic on `Complex<f64>` (decomposed to two `f64`s in the inner loop where it helps) let LLVM lower to AVX2/NEON on its own.
- `rayon` is acceptable infrastructure (it parallelizes our hand-rolled kernel, it doesn't *do* the numerics). `std::simd` (nightly) is out.

**Files affected:**
- `crates/rustlab-dsp/src/vector_calc.rs` — full rewrite of the kernel section.
- `crates/rustlab-dsp/Cargo.toml` — add `rayon` if not already a workspace dep (verify before assuming).
- `crates/rustlab-dsp/src/tests.rs` — keep all existing tests passing; add fused-vs-naive and parallel-vs-serial equivalence tests.

**Tests:**
1. All existing vector-calc tests (the gallery's analytic checks: paraboloid gradient, radial divergence, vortex curl) keep passing.
2. `fused_div_matches_compose` — `divergence_2d(fx, fy)` gives the same result as the old `d_dx(fx) + d_dy(fy)` to bit precision on a fixed RNG-seeded input.
3. `fused_curl_matches_compose` — same for curl.
4. `parallel_matches_serial_above_threshold` — set the threshold low artificially in a test build, verify parallel and serial paths agree to bit precision on a 100×100 grid.
5. `non_contiguous_input_still_works` — pass a non-standard-layout matrix (a slice of a larger one) through `gradient_2d`; verify equivalence.

**Acceptance:**
- On a 200×200 grid, `gradient` + `divergence` of a real-valued field is `≥ 3×` faster than current. (Bench in `perf/`; doesn't need to be a unit test.)
- Memory high-water mark on a 200×200×200 cube `divergence3` drops from `~6 × 128 MB` of intermediates to `~2 × 128 MB` (one fused output + one input ref).
- Existing notebooks (`gallery/vector_calculus.md`, `gallery/dielectric.md`) re-bake with no output diff except the heatmap render times.

**Risk:** low–medium. Pure refactor with strong existing test coverage. Main risks: (a) stride-1 slicing assumptions — handled by the `as_standard_layout` guard; (b) accidental data races in the parallel path — handled by the Rust borrow checker (rayon can't compile a racy version).

**Estimated size:** ~400 LoC rewrite + ~150 LoC tests. One session.

---

## Phase 4 — Direct-CSC build in Laplacian builders

**Status:** shipped — commit `810806a` (2026-05-09)
**Goal:** skip the COO sort. The builders in `laplacian.rs` (lines 132, 214, 351) all `Vec::push` triplets in column-major order with predictable per-column counts, then hand the list to `SparseMat::new` which sorts/dedupes. For a 3-D 100³ build that's 7M entries through `O(nnz log nnz)` sort.

**Implementation log (2026-05-09):**
- Did *not* push CSC all the way upstream — kept `SparseMat` as COO with row-major sorted entries, since downstream `to_csc` consumers depend on that storage layout. Instead added `SparseMat::from_sorted_entries(rows, cols, entries)` in `crates/rustlab-core/src/types.rs`: a fast-path constructor for callers that already produce row-major-then-column-major sorted entries. Single linear pass to merge consecutive duplicates and drop near-zeros — no HashMap, no full sort.
- Added a tiny shared helper `flush_row::<MAX>(out, row, buf, len)` in `crates/rustlab-dsp/src/laplacian.rs` that takes a stack-allocated `(col, val)` buffer of bounded size, sorts by column, and emits row-prefixed triples.
- Rewrote `laplacian_1d`, `laplacian_2d_bc`, `laplacian_3d`, `laplacian_eps_2d`: each now collects its at-most-3/5/7 stencil entries into `buf`, calls `flush_row`, and at the end calls `from_sorted_entries`. The per-row sort is constant-time (5 elements at most for 2-D, 7 for 3-D).
- The `from_sorted_entries` consecutive-duplicate merge handles the periodic-BC corner case where wrap col coincides with interior col at minimum sizes (`n=2` 1-D periodic, `ny=2` or `nx=2` 2-D periodic, etc.). Verified with `lap_1d_periodic_minimum_size_dedupes`.
- Tests: 4 new equivalence tests in `laplacian.rs::tests` cross-check the new direct-sorted output against the legacy `SparseMat::new` HashMap-then-sort path, entry-by-entry, for Dirichlet/Neumann/Periodic at several grid shapes. All 14 pre-existing `laplacian_*` tests still pass.
- Bench: new `cargo run --release --example bench_laplacian_build -p rustlab-dsp`. **Pre-vs-post via git-stash A/B:** 1-D n=1M: 255 ms → 13 ms (20×). 2-D 800×800: 273 ms → 14 ms (19×). 3-D 100³: 634 ms → 29 ms (22×). Numbers in `perf/em_performance_phase4.md`.
- Workspace `cargo test --workspace` and `cargo test --workspace --features viewer` both green; dsp test count went 172 → 176.

**Scope:**
- Add `SparseMat::from_csc_parts(rows, cols, col_ptr, row_idx, vals)` constructor in `rustlab-core/src/types.rs` (or `sparse.rs` if it has been split out) — assumes inputs are already in CSC and skips the sort.
- Rewrite each `laplacian_*` builder as two passes: (1) compute exact column counts from the stencil and BC, (2) write `col_ptr`, `row_idx`, `vals` directly. The 2-D Dirichlet stencil contributes 5 nnz per interior cell, fewer at edges/corners — formula is closed-form per BC.
- Internal-only optimization; no script-layer API change.

**Files affected:**
- `crates/rustlab-core/src/types.rs` (or wherever `SparseMat` lives now — re-verify) — new constructor.
- `crates/rustlab-dsp/src/laplacian.rs` — rewrite all four builders. Keep the COO-emitting path under `#[cfg(test)]` for cross-check.

**Tests:**
1. `laplacian_2d_csc_matches_coo` — for a few `(nx, ny, dx, dy, bc)` combinations, the new direct-CSC builder produces a `SparseMat` that compares equal to the old COO-then-sort path.
2. Same for 1-D, 3-D, eps variants.
3. `laplacian_3d_100_cubed_builds_in_under_5_seconds` (perf-flavoured test, skip in CI if flaky) — rough sanity check.

**Acceptance:**
- All existing `laplacian_*` tests keep passing.
- 3-D 100³ Laplacian build is `≥ 2×` faster than current.
- 2-D 100² shows a small but measurable improvement (target: any positive Δ).

**Risk:** low. Internal change with tight equivalence test coverage. Main risk: getting the column-count formula wrong for Neumann/Periodic edge cells — write the formula on paper before coding.

**Estimated size:** ~300 LoC + ~200 LoC tests. One session.

---

## Phase 5 — Real `f64` path for `vector_calc.rs` + Laplacian builders

**Status:** investigated 2026-05-09, **deferred** — implementation regressed performance, reverted before commit.

**Investigation log (2026-05-09):**
Implemented Option C (internal realness fast path: `is_real_matrix` check, `extract_real` to Vec<f64>, f64-typed kernels writing to CMatrix output). Wrote `d_dx_real`, `d_dy_real`, `divergence_2d_real`, `curl_2d_real`, plus 5 correctness tests that all passed. Then benched against the Phase 3 baseline:

| Grid | Phase 3 baseline | Phase 5 attempt | Δ |
|---|---:|---:|---:|
| 100×100 div | 0.075 ms | 0.188 ms | **2.5× slower** |
| 200×200 div | 0.091 ms | 0.272 ms | **3× slower** |
| 800×800 div | 0.281 ms | 1.39 ms | **5× slower** |
| 800×800 grad | 0.576 ms | 0.89 ms | 1.5× slower |

The premise — that real path saves bandwidth — was wrong on this kernel. Two issues:
1. `extract_real` allocates and writes a `Vec<f64>` of size N (5–6 MB at 800×800) per call. That extra memory traffic dominates the savings from f64 vs C64 arithmetic.
2. CMatrix reads pull 16-byte cachelines regardless of whether we use both halves of each Complex<f64>. We can't actually halve input bandwidth without changing the upstream storage type — which would mean Option A (parallel `_real` Value-layer API), the heavy refactor we explicitly chose to avoid.

**Conclusion:** The realness fast path is a net loss for our finite-difference kernels because (a) the arithmetic is already cheap relative to memory traffic, and (b) the kernel is bandwidth-bound, not compute-bound. The Cholesky path benefits because factorization is compute-bound on its `nnz` × `n` flop count.

**Reverted:** `crates/rustlab-dsp/src/vector_calc.rs` and tests restored to post-Phase-4 state. No commit. Phase 5 status changed from "pending" to "investigated, deferred" in the snapshot.

**If revisited:** the only way to actually win on real input is Option A — separate real-typed `Value::Matrix(Array2<f64>)` and `SparseMat<f64>` types at the script layer, with parallel `gradient_real`/`laplacian_2d_real`/etc. APIs. Larger refactor; defer until profiling shows finite-difference kernels are a bottleneck on real-EM curriculum work (currently they aren't — the gallery's bottlenecks are factor + solve, addressed by Phase 1/2/4/6).
**Goal:** stop promoting real EM data to C64 in the DSP layer. The Cholesky / LU layer already detects "essentially real" inputs and routes to `SparseCsc<f64>` (`builtins.rs:10433`); the DSP layer above it pre-promotes everything to C64, which means even a real-valued `eps_map` produces complex triplets that then have to be detected and demoted. Cut out the round-trip.

**Scope decision required at start of phase — flag to user:**
- **Option A (parallel API):** add `gradient_2d_real(&Matrix<f64>) -> (Matrix<f64>, Matrix<f64>)` etc. alongside the existing `_2d(&CMatrix)`. Builtin layer dispatches on the input's `Value::Matrix` (real-only `Array2<f64>`) vs `Value::Matrix` (complex `CMatrix` — re-verify which `Value` variant carries which type).
- **Option B (generic over scalar):** make `gradient_2d<T: Float>(&Array2<T>)` etc. generic. More invasive but cleaner.
- **Option C (just promote at boundary):** keep DSP API as C64, add real-only post-detection in `try_sparse_cholesky` is *already done* — so do nothing here. Deferred until profiling shows the DSP-side promotion is a real cost.

**Default if no user input:** Option A. It mirrors how the script layer's `Value` enum already works (separate Matrix and CMatrix variants in some layers) and keeps the C64 path stable for frequency-domain users.

**Scope (assuming Option A):**
- `crates/rustlab-dsp/src/vector_calc.rs` — add `_real` variants of every public function.
- `crates/rustlab-dsp/src/laplacian.rs` — add `_real` variants returning `SparseMat<f64>` (new — verify the type-parameterized SparseMat actually exists; if not, add a `SparseMatF64` newtype).
- `crates/rustlab-core/src/types.rs` — possibly a real-typed `SparseMat<T>` if it's not generic yet. The closed `sparse_solve_handroll.md` plan added `SparseCsc<T>`; check whether `SparseMat` itself is also generic now.
- `crates/rustlab-script/src/eval/builtins.rs` — script layer dispatches by inspecting `Value` to call real or complex variant.

**Tests:**
1. `gradient_real_matches_complex_on_real_input` — bit-precision agreement.
2. `laplacian_2d_real_matches_complex` — same.
3. `dielectric_solve_real_path_faster` — measure end-to-end time on the dielectric example using real path; expect `≥ 1.5×` speedup vs forced-complex path. (Acceptance criterion, not a unit test.)

**Acceptance:**
- Memory footprint for a 100×100 dielectric solve drops by `~40%` (factor stays 16 B/nnz on the Cholesky `f64` path it already used; the new savings come from input matrices and DSP intermediates).
- `gallery/electrostatics.md` and `gallery/dielectric.md` re-bake with no numerical-output diff (they were already quoting real numbers).

**Risk:** medium. Cross-cutting type change. Largest risk is API surface duplication — if Option A doubles every `vector_calc` function, the maintenance burden grows. Re-evaluate Option B once Option A is in if the duplication is painful.

**Estimated size:** ~600 LoC + ~300 LoC tests. **Probably two sessions, possibly bundled with Phase 6 if both touch core types.**

---

## Phase 6 — Symbolic-then-numeric Cholesky on flat CSC

**Status:** awaiting commit (2026-05-09)

**Implementation log (2026-05-09):**
- Replaced `cols_l: Vec<Vec<(usize, T)>>` in `crates/rustlab-core/src/sparse_solve/cholesky.rs::SparseChol::factor` with a flat-CSC two-pass design.
- Symbolic pass: walks the elimination tree via `ereach`, accumulates `col_count[j]` per column. Adds 1 per column for the diagonal. Prefix-sum into `col_ptr`. Allocate `row_idx` and `values` exactly once.
- Numeric pass: same algorithm as before, but writes go to `values[next[j]]` / `row_idx[next[j]]` with per-column write cursor. Diagonal goes in the reserved slot at `col_ptr[k]` at iteration end. `debug_assert!` confirms symbolic count matches numeric writes.
- All 8 existing Cholesky tests pass unchanged. Full workspace and `--features viewer` test suites green.
- A/B via git stash: chol/AMD at 150×150 went 0.798 → 0.716 s (−10%); 200×200 went 2.355 → 2.204 s (−6%). Smaller grids are essentially flat (within noise). Win is real but more modest than the plan estimated, because the original `Vec<Vec<…>>` layout was already cache-friendly for grid Laplacians where columns have bounded fill.
- Numbers in `perf/em_performance_phase6.md`. `docs/sparse_solve.md` updated to describe the two-pass design.
**Goal:** kill the `Vec<Vec<(usize, T)>>` per-column accumulator in `cholesky.rs:58` and replace it with a flat CSC build. The numeric pass already iterates in topological order with the elimination tree (`cholesky.rs:52`) — adding a symbolic-counts pass first lets us preallocate `Lp / Li / Lx`.

**Scope:**
- Add `symbolic_cholesky(c: &SparseCsc<T>, parent: &[usize]) -> SymbolicChol` in `rustlab-core/src/sparse_solve/cholesky.rs`. Returns column counts (`L_colcount[j]`) and the row pattern of `L` (or just the counts; pattern is rederivable but pre-storing it speeds the numeric pass).
- Rewrite the numeric pass to write directly into `Vec<usize> Lp`, `Vec<usize> Li`, `Vec<T> Lx` rather than `cols_l: Vec<Vec<(usize, T)>>`. The existing topological-order iteration is preserved; only the storage changes.
- Existing public API (`SparseChol::factor`, `SparseChol::solve`) is unchanged; this is internal.

**Files affected:**
- `crates/rustlab-core/src/sparse_solve/cholesky.rs` — full rewrite of the numeric loop.
- `crates/rustlab-core/src/sparse_solve/tests.rs` — keep all existing tests passing.

**Tests:**
1. All existing Cholesky tests pass unchanged (this is an internal change).
2. `cholesky_symbolic_counts_match_actual_nnz` — for a handful of fixtures, the symbolic pass's counts equal the actual nnz of `L` after numeric factorization.
3. Stress test: 200×200 Dirichlet Laplacian factors in `≤ 0.5s` with AMD, `≤ 0.1s` with Identity. (Compare to the table in `perf/sparse_solve_phase1to4.md` which currently reports 0.42s / unspecified — Phase 6 should reclaim ~2× of that 0.42s.)

**Acceptance:**
- All existing tests pass.
- 200×200 Identity-ordered factor `≥ 2×` faster.
- Cache-miss rate on the factor (measured with `cargo flamegraph` or `perf`) drops noticeably — qualitative; capture before/after flamegraphs in `perf/em_performance_phase6.md`.

**Risk:** medium. Cholesky numerics are finicky — easy to introduce silent corruption that test fixtures don't catch. Mitigation: run with `RUSTFLAGS="-C debug-assertions=on"` during development; run the full `cargo test --workspace` and `cargo test --workspace --features viewer` matrix; cross-check against a Davis-CSparse reference output on at least one ill-conditioned matrix.

**Estimated size:** ~500 LoC + ~150 LoC tests. **One focused session — do not bundle with other phases.**

---

## Phase ordering rationale

- **1 → 2 → 3 → 4** can ship in any order; they don't share files. Bundle 1+2 for a single PR if both go fast.
- **5** touches `vector_calc.rs` and `laplacian.rs`; if it lands before **3** and **4**, those phases need to handle both real and complex paths. Easier to land 3 and 4 first (still C64) and then have 5 add the parallel `_real` API.
- **6** is independent of 1–5; can slot anywhere. Recommend last, after the test suite has been hardened by phases 1–5.

## What's *not* in this plan

- **Sparse LU rewrite** — already shipped (`e9283b7`).
- **Real-only Cholesky dispatch** — already shipped at `builtins.rs:10433` (`try_sparse_cholesky`'s `all_real` branch). Phase 5 extends the *upstream* DSP layer to stop generating C64 in the first place.
- **Auto-pinning for Neumann/Periodic singular systems** — out of scope; tracked in `dev/plans/em_requests_plan.md` as a separate ask.
- **GPU offload, SIMD beyond compiler autovectorization** — explicitly out of scope per Rule 9 (no FFI, no large libraries, pure Rust). `std::simd` could be revisited in a follow-up plan if Phase 3's autovectorization leaves clear wins on the table.

## When this plan closes

When phases 1–6 are all `shipped`. Move this file to `dev/plans/closed/em_performance.md` and drop a one-line summary into `dev/plans/em_requests_plan.md` Status snapshot referencing the closure.

# Implementation Plan — `em_requests.md` (nine asks)

**Status:** Proposed
**Date opened:** 2026-04-26
**Source:** `../rustlab_em/dev/rustlab/requests/em_requests.md`
**Companion plan:** `dev/plans/rustlab_em_requests.md` (Phases 1-4 landed) — covers the *original* five EM requests; this plan covers the *additional* nine asks identified in the §2026-04 sweep.

This plan turns `../rustlab_em/dev/rustlab/requests/em_requests.md` §2-§5 into a sized, sequenced work program against the rustlab tree at `/Users/mike/projects/2026/rustlab`.

## Licensing policy (applies to every dep choice in this plan)

No controversial-licensing dependencies. Prefer pure-Rust MIT/Apache-2.0; reject GPL/LGPL/AGPL/copyleft; FFI to Fortran/C++ off the table by default. If no clean pure-Rust option exists, flag the gap and ask before proceeding rather than reaching for a non-Rust shim.

This rules out, by name:
- **UMFPACK** (GPL) — disqualified for §2.3.
- **`arpack-ng-sys`** (BSD-licensed but Fortran FFI) — disqualified for §2.4.
- **MKL / Intel-licensed numerics** — disqualified anywhere.

Confirmed acceptable:
- **`faer`** (MIT-or-Apache-2.0, pure Rust) — primary dep for §2.3.
- **`sprs`**, **`russell_sparse`**, **`lobpcg`**, **`ndarray-linalg`** — pure-Rust fallbacks if needed.

## Architectural facts the plan rests on

- Single `f64` matrix/vector storage type is complex (`CVector = Array1<C64>`, `CMatrix = Array2<C64>`) — `crates/rustlab-core/src/types.rs:5-15`. There is no separate real-typed array variant.
- `SparseMat` / `SparseVec` are pure COO with sorted entries — `crates/rustlab-core/src/types.rs:151-310`. No CSC/CSR storage form yet.
- All builtins register in `crates/rustlab-script/src/eval/builtins.rs` (~8 260 lines) via `r.register(name, fn)` calls.
- Current `spsolve` densifies and runs Gaussian elimination at lines 7909-7996.
- Dense `eig` at 5011-5036 uses an in-tree Hessenberg/QR algorithm from 4796-5006 (no LAPACK).
- `laplacian_2d` is column-major Dirichlet-only at 8107-8170; `ij2k`/`k2ij` at 8175-8224.
- `crates/rustlab-dsp/src/vector_calc.rs` is the precedent for real algorithm code (`gradient_2d`, `divergence_2d`, `curl_2d` at 103-263). New Laplacian work follows the same split: heavy code in `rustlab-dsp`, thin builtin wrapper in `rustlab-script`.
- Plotting backends: `plotters` for SVG/PNG (`rustlab-plot/src/file.rs`, `build_cartesian_2d` at line 282), Plotly via HTML, ratatui for terminal.
- Workspace already has an unused `linalg` feature flag in `rustlab-core/Cargo.toml:14-21` pulling in `ndarray-linalg`. Not directly used here — `faer` is independent of it.
- REPL help: `crates/rustlab-cli/src/commands/repl.rs` holds `HelpEntry` records (lines 13-510) and the `categories` table at 813-1002 used by `print_help_list`. Every new builtin must appear in both places.

## Workflow obligations (apply per request, not repeated below)

Six mandatory rules per `feedback_workflow.md` and `AGENTS.md:165-173`:
1. **Plan first** — written plan presented and approved.
2. **Tests in same commit** — `crates/rustlab-script/src/tests.rs` for builtins; algorithm tests in their owning crate. Run `cargo test --workspace` *and* `cargo test --workspace --features viewer`.
3. **No commit without explicit approval.**
4. **Update `AGENTS.md`** — function table at lines 817-925.
5. **Update `docs/quickref.md`** — every new function in its category.
6. **Update `docs/functions.md`** + REPL `HelpEntry` + category list in `repl.rs`.

## Housekeeping (not a plan item, ~15 min)

Before any new feature work, flip Status fields on the four already-shipped request files in `../rustlab_em/dev/rustlab/requests/`:
- `vector-calculus-operators.md` → **Landed**
- `quiver-and-streamplot.md` → **Landed**
- `contour-plots.md` → **Landed**
- `laplacian-stencil-builder.md` → **Landed (partial — Dirichlet 2-D only; see §2.1)**

Update the priority table in `dev/rustlab/requests/README.md:9-15` so items 1-3 read **Landed** and item 4 reads "Landed (partial)". Single `rustlab_em` PR, separate from upstream rustlab work.

---

## §2.3 — Real `spsolve` (Critical — scaling cliff)

**Where it goes:**
- New module `crates/rustlab-core/src/sparse_solve.rs` with `SparseMat::lu_factor() -> SparseLU`, `SparseLU::solve(&CVector) -> CVector`, `SparseMat::cholesky() -> SparseChol` for SPD inputs.
- Replace body of `builtin_spsolve` at `builtins.rs:7909-7996` with a thin call. API and signature unchanged; `Value::Vector`/`Value::Scalar` return shape preserved (the trailing `if x.len()==1` block at 7986-7995 stays).

**Dependency: `faer`** (locked in — pure Rust, MIT-or-Apache-2.0, no FFI).
- Add to `Cargo.toml [workspace.dependencies]`: `faer = "0.20"` (or current).
- Wire into `rustlab-core/Cargo.toml` under a new `sparse-solve` feature; default-on in cli/script crates so casual users still get the speedup without opt-in.

**Conversion layer:** `SparseMat::to_faer_csc(&self) -> faer::sparse::SparseColMat<C64>` — straightforward sort-and-rebuild from the existing entries vector. Detect "real-only" by scanning imag parts (`max(|im|) < 1e-12`) and route to a real solver when possible; complex factorization is ~4× the work.

**SPD detection / path selection:** Add `SparseMat::is_hermitian()` and `SparseMat::is_spd_estimate()` helpers in `rustlab-core/src/types.rs` (~40 LoC, reused by §2.4). Default `spsolve(A, b)` does symmetry + diagonal-positivity check, tries Cholesky first, falls back to LU on failure. Add explicit dispatch override `spsolve(A, b, "lu" | "cholesky" | "auto")` so users can force the path on hot loops.

**Codebase impact: L (~700 LoC + tests)**
- `sparse_solve.rs`: ~350 LoC.
- `builtins.rs`: net **-60 LoC** (–90 dense fallback + 30 dispatch).
- Tests: ~250 LoC. Invariants:
  - `spsolve(I, b) == b`.
  - Round-trip on `laplacian_2d(20,20)` matches Lesson-05 finite-difference reference within tolerance.
  - 2×2 toy by hand.
  - Density sweep (5%, 1%, 0.1%).
  - Complex-RHS path on a complex laplacian (FDFD-style).
  - Singular-matrix returns clear error.

**Risks / open questions:**
- **Tiny-problem regression.** faer has fixed setup cost; for n<100 the dense path is faster. **Decision:** always use faer to keep the code simple; revisit only if `perf/report.md` flags a regression.
- **Determinism.** Confirm bit-stable across runs; set deterministic config flag if needed.
- **Symmetry-test cost.** Don't walk all entries on every solve — gate behind a one-time check inside the factorization or rely on the explicit-mode opt-in.
- **Complex Cholesky.** faer requires Hermitian SPD. Don't silently downgrade complex SPD claims.

**Ship-as: own PR, foundational. Sized: 3-5 days senior, +2 days for perf comparison and Octave reference run.**

---

## §2.2 — `laplacian_eps_2d(eps_map, dx, dy)` (High)

**Where it goes:**
- New algorithm in `crates/rustlab-dsp/src/laplacian.rs` (new module). Public function `laplacian_eps_2d(eps_map: &CMatrix, dx: f64, dy: f64) -> SparseMat`. Mirrors `vector_calc.rs:103` (`gradient_2d`).
- New builtin wrapper directly under `builtin_laplacian_2d` near `builtins.rs:8170`. Register at line 273 alongside `r.register("laplacian_2d", …)`.

**Algorithm:** Flux-conservative 5-point stencil, harmonic-mean half-cell coefficients:
```
ε_e = 2·ε(i,j)·ε(i,j+1) / (ε(i,j) + ε(i,j+1))
```
and equivalent for west/north/south. Diagonal is the negative sum of the four half-cell coefficients (operator is `+∇·(ε∇)`, not `-`). Same column-major flat indexing `k = j·ny + i` as `laplacian_2d`. Boundary cells use ghost-cell Dirichlet (drop off-diagonal across boundary; diagonal still includes that direction's coefficient). Same builder serves the magnetostatic `∇·(μ⁻¹∇A_z)` form by passing `1/μ`.

**Dependencies:** None new.

**Codebase impact: S (~260 LoC + tests)**
- Algorithm: ~120 LoC.
- Builtin wrapper: ~40 LoC.
- Tests: ~100 LoC. Invariants:
  - `eps_map ≡ 1.0` makes `laplacian_eps_2d == laplacian_2d` exactly.
  - Flux conservation: interior row sums == 0.
  - `1/μ` form gives correct magnetostatic operator on hand-checked 4×4.

**Risks / open questions:**
- ε at cell centres (standard) — document explicitly; arithmetic-mean is the wrong-but-tempting alternative.
- Allow complex `eps_map` for lossy materials (Lesson 10 FDFD with PML needs this).
- Accept 1-arg `(eps_map)` as well as 3-arg `(eps_map, dx, dy)` to match `laplacian_2d`'s pattern.
- Apply `bc` parameter from §2.1 here too (Cross-cutting #2 below).

**Ship-as: bundle with §2.1. Sized: 1-2 days standalone; 3 days bundled.**

---

## §2.4 — `eigs(A, n)` and `eigs(A, B, n)` (High)

**Where it goes:**
- New module `crates/rustlab-core/src/sparse_eig.rs`:
  - `pub fn eigs(a: &SparseMat, n: usize, which: Which, sigma: Option<C64>) -> Result<(CMatrix, CVector), …>`
  - `pub fn eigs_gen(a: &SparseMat, b: &SparseMat, n: usize, which: Which, sigma: Option<C64>) -> Result<…>`
- Builtin `builtin_eigs` next to `builtin_eig` at `builtins.rs:5011`. Register near line 190 alongside `r.register("eig", builtin_eig)`.

**Algorithm: hand-rolled implicit-restart Arnoldi (IRAM) on top of `faer` LU.** Pure Rust, no FFI. Decision locked: no `arpack-ng-sys` fallback even if convergence is hard — `faer` LU does the shift-invert factorization for us, the Krylov outer loop is well-trodden numerical code, and we want full control over convergence diagnostics for the curriculum (`info.iterations`, `info.residual` so the lessons can teach the math).

For SPD problems (cavity modes), specialize to **Lanczos** instead of Arnoldi (half the storage, better numerics). Detect SPD via `is_spd_estimate()` (added in §2.3) and dispatch internally.

**API:**
- `[V, D] = eigs(A, n)` — n smallest-magnitude eigenpairs, standard problem.
- `[V, D] = eigs(A, B, n)` — generalized: `A x = λ B x`.
- `[V, D] = eigs(A, n, "sm" | "lm")` — smallest/largest magnitude.
- `[V, D] = eigs(A, n, sigma)` — shift-invert around numeric `sigma`.
- Real and complex inputs supported.
- `V` is dense `CMatrix` (n eigenvectors as columns), `D` is length-n `CVector`. Matches `eig`'s vector-of-eigenvalues convention.

**Codebase impact: L (~1200 LoC + tests)**
- `sparse_eig.rs`: ~700 LoC including Arnoldi iteration, implicit restart (IRAM), Lanczos specialization, SPD detection, shift-invert orchestration via faer LU, convergence diagnostics, generalized-problem reduction.
- Builtin: ~80 LoC.
- Tests: ~300 LoC. Invariants:
  - `A = laplacian_2d(20,20)` lowest 4 eigenvalues match analytic π²(m²+n²)/L² to <1%.
  - `eigs(A, A, n)` returns 1.0 with multiplicity n.
  - SPD path agrees with general path within machine precision.
  - Convergence info populated and sensible.

**Risks / open questions:**
- **Convergence on hard problems.** Waveguide eigenproblems with absorbing boundaries are ill-conditioned. Plan ~150 LoC for restart logic; budget extra time for tuning. **No FFI escape hatch:** if the hand-rolled path stalls on a real curriculum problem, the answer is to read Saad chapters 6-8 and improve the algorithm, not to bring in Fortran ARPACK.
- **Generalized problem reduction.** For B-SPD: factor B once with Cholesky, transform to standard form `B^{-1/2} A B^{-1/2} y = λ y`, recover eigenvectors. For B-indefinite: requires generalized Schur, defer; document the restriction.
- **Performance target.** A 40 000×40 000 cavity problem asking for 10 eigenpairs at sigma=0 should solve in seconds, not minutes. If it doesn't, debug the implementation rather than reach for FFI.

**Ship-as: own PR, depends on §2.3. Sized: 5-8 days senior with prior Krylov experience; 2-3 weeks otherwise.** Read Saad's *Iterative Methods for Sparse Linear Systems* before starting if unfamiliar.

---

## §2.5 — `rect_mask`, `disk_mask`, `polygon_mask` (High)

**Where it goes:**
- New module `crates/rustlab-dsp/src/rasterize.rs`. Three pure functions returning `CMatrix` of 0.0/1.0.
- Three builtins in `builtins.rs`, registered together near line 273.

**Algorithm:**
- `rect_mask(X, Y, x0, y0, w, h)` — element-wise comparison.
- `disk_mask(X, Y, xc, yc, r)` — `(X-xc)² + (Y-yc)² ≤ r²`.
- `polygon_mask(X, Y, verts)` — even-odd ray-casting per cell. ~30 LoC of standard code.

Output is `ny×nx` to match meshgrid's row=y/col=x convention.

**Dependencies:** None.

**Codebase impact: S (~290 LoC + tests)**
- Algorithm: ~90 LoC.
- Builtins: ~120 LoC (mostly arg validation: verts must be Nx2, X/Y same shape).
- Tests: ~80 LoC. Invariants:
  - `disk_mask(X,Y,0,0,1)` summed over a 100×100 fine grid approximates π to ~1%.
  - polygon == rect for square verts.
  - Edge cases — empty, single-vertex, collinear vertices.

**Risks / open questions:**
- Anti-aliased / area-weighted masks deferred — em_requests.md flags as user-space concern, not part of this request.
- 3-D `box_mask` / `ball_mask` deferred; trivial follow-up if Lesson 14 needs them.

**Ship-as: one PR, three builtins. Sized: half a day to one day. Smallest high-priority item — do first as warm-up.**

---

## §2.1 — `laplacian_2d` BC extensions + `laplacian_1d` + `laplacian_3d` (Medium)

**Where it goes:**
- Extend the §2.2 `laplacian.rs` module. New functions `laplacian_2d_bc(nx, ny, dx, dy, bc)`, `laplacian_1d(n, dx, bc)`, `laplacian_3d(nx, ny, nz, dx, dy, dz, bc)`. `bc` is enum `BoundaryCondition { Dirichlet, Neumann, Periodic }`.
- Modify `builtin_laplacian_2d` at `builtins.rs:8107` to accept optional fifth string arg (`"dirichlet"` default, `"neumann"`, `"periodic"`). Backwards compatible: change `check_args_range("laplacian_2d", &args, 2, 4)` to `2, 5`.
- Add `builtin_laplacian_1d` and `builtin_laplacian_3d`, register at `builtins.rs:273-275`.

**Boundary semantics:**
- **Dirichlet** (current) — homogeneous-ghost; edge cells keep diagonal, skip off-diagonal across boundary.
- **Neumann** — zero-flux. Edge cells get *increased* diagonal (the missing off-diagonal is added back). One extra `entries.push((k, k, …))` per boundary cell.
- **Periodic** — wrap. Edge cells point to wrap-around neighbours.

3-D ordering: column-major-of-pages, `k = (kk·nx + j)·ny + i`. Be careful — `Tensor3` shape `(rows, cols, pages) = (ny, nx, nz)` per `rustlab-core/src/types.rs:11`. Document the flat ordering exactly the way `laplacian_2d` documents it (`functions.md:1256`).

**Dependencies:** None.

**Codebase impact: M (~600 LoC + tests)**
- Algorithm in `laplacian.rs`: ~250 LoC.
- Builtins: ~150 LoC.
- Tests: ~200 LoC. Per-BC invariants:
  - Dirichlet `λ_min` matches analytic for N×N grid.
  - Neumann has zero eigenvalue (constant null-space).
  - Periodic has 2-D Fourier-mode eigenvalues `4 sin²(πk/N)`.

**Risks / open questions:**
- **Periodic + spsolve = singular system.** Constant null-space → `spsolve` fails. Document the row-pinning workaround: zero row 1, set `(1,1)=1`, and pin RHS. **Cross-cutting:** §2.4's `eigs` solves this naturally (smallest-magnitude pair = constant mode), but `spsolve` users need the pinning idiom in the example.
- **3-D index sugar.** Add `ijk2k`/`k2ijk` alongside the existing 2-D pair. ~30 LoC, free with this work.
- `bc` as string keyword (not enum at the language level) for ergonomic match with `imagesc(M, "viridis")` etc.

**Ship-as: bundle with §2.2. Sized: 2-3 days standalone; 3-4 days bundled.**

---

## §2.7 — `polar`, `loglog`, `semilogx`, `semilogy` (Medium)

**Where it goes:**
- `crates/rustlab-plot/src/figure.rs` — extend `SubplotState` (line 183) with `x_scale: AxisScale` and `y_scale: AxisScale` (`Linear | Log10`); add `polar: bool` (or factor as separate plot kind — see Risks).
- `crates/rustlab-plot/src/file.rs` — at `build_cartesian_2d` (line 282), branch on `x_scale`/`y_scale` to use `LogCoord` from plotters' coord system. ~80 LoC of branching.
- `crates/rustlab-plot/src/html.rs` — Plotly natively supports `xaxis: { type: 'log' }`. ~20 LoC.
- `crates/rustlab-plot/src/ascii.rs` — log-transform data before passing to ratatui (gnuplot's `dumb` terminal approach); label axis "log10(x)".
- Builtins: `builtin_loglog`, `builtin_semilogx`, `builtin_semilogy`, `builtin_polar` in `builtins.rs`. Each is a thin shim that sets the scale flags on the current subplot then calls the `plot` codepath.

**Dependencies:** None new (plotters already supports `LogCoord`; Plotly handles via JSON config).

**Codebase impact: M (~480 LoC + tests)**
- `figure.rs`: ~30 LoC.
- `file.rs` log-axis branching: ~120 LoC (the `build_cartesian_2d` generic dance — `LogCoord<f64>` doesn't compose cleanly with `f64..f64`, so the chart-builder type changes).
- `html.rs`: ~20 LoC.
- `ascii.rs`: ~30 LoC.
- Builtins: ~200 LoC across four functions.
- Tests: ~80 LoC.

**Risks / open questions:**
- **Polar is structurally different.** Coord system `(r, θ)`, not just a scaled axis. Recommendation: build it as a special plot kind with its own renderer rather than retrofitting through `SubplotState.x_scale`. Adds ~150 LoC but keeps the abstraction clean. Plotly: `'type': 'scatterpolar'`. Plotters: pre-transform `(theta, r) → (r·cos θ, r·sin θ)` and add radial gridlines as additional series.
- **Negative or zero data on log axes.** Plotters will panic. Add a clear error: `"loglog: data must be strictly positive (got minimum -0.5)"`.
- **HTML / SVG style divergence on log axes.** Both correct, just stylistically different. Document.

**Ship-as: one PR bundling all four. If polar slips, ship `loglog`/`semilogx`/`semilogy` first and follow with polar. Sized: 2-3 days for log axes, +2 days for polar = 4-5 days total.**

---

## §2.6 — Yee curl-curl + SC-PML (Medium — Option 1 to start, with graduation trigger)

The home decision is now resolved as a **two-phase plan** rather than a binary choice:

### Phase 1 (now): script library in `rustlab_em`

Ship the helpers as scripted rustlab in `lessons/_shared/em.r` (or equivalent path; standardize so every EM lesson can `run("../_shared/em.r")` consistently). Implementations:

```r
# in lessons/_shared/em.r
function [Ce, Ch] = yee_curl_2d(nx, ny, dx, dy)
  # ... build sparse curl operators in script
end

function [sx, sy] = scpml_stretch(nx, ny, npml, omega, sigma_max)
  # ... return diagonal stretching factors as length-nx and length-ny complex vectors
end
```

**Why script first, not native crate:**
- These are *one-time* assembly builders, not inner-loop kernels. They build a sparse matrix once at the start of an FDFD/FDTD simulation; that matrix is then handed to `spsolve` / `eigs` / `eig` which are already native Rust.
- Assembly time for a 100k-cell Yee matrix in scripted rustlab: ~1-5 seconds. In native Rust: ~50-200ms. For a curriculum simulation that runs in the solver for minutes, the assembly time is irrelevant.
- The risk of a workspace `rustlab-em` crate is committing the rustlab maintainer to EM as a permanent toolbox direction without the curriculum having proven it needs that depth.
- Iterating on the API in scripted form is *much* faster than churning a workspace crate.

**Concretely for Phase 1:**
- File spec upstream as `dev/rustlab/requests/yee-and-pml-builders.md`, `Status: Discussion`. Captures the conversation and the proposed API even before any code lands.
- Implement `yee_curl_2d` and `scpml_stretch` as scripted functions in `../rustlab_em/lessons/_shared/em.r`.
- Add a `lessons/_shared/README.md` explaining the import pattern.
- Lessons 10/11/13 use `run("../_shared/em.r")` to import.
- **Zero rustlab upstream changes.**

### Phase 2 (later, if triggered): promote to workspace crate

**Graduation triggers** (any one is sufficient):
- Lesson 14 capstone needs 3-D Yee (`yee_curl_3d`) — assembly cost scales 100×, scripted version starts to hurt.
- Any lesson's Yee assembly takes >5 seconds end-to-end.
- A second physics curriculum (controls, fluids, etc.) starts asking for similar finite-difference builders — at that point, factor a shared `rustlab-physics`-style crate.
- The scripted assembly hits a language-feature wall (e.g., needs sparse-matrix construction patterns that rustlab-script doesn't express well).

**If Phase 2 is triggered:** new crate `rustlab-em` in the workspace, gated behind feature flag `em` (default-off — opt-in for users who want EM-specific builders). ~700 LoC including Yee discretization, SC-PML coordinate stretching, basic tests, integration with `SparseMat`. Builtins (in `rustlab-script`) ~150 LoC. Tests ~250 LoC.

**Codebase impact:**
- **Phase 1: XS upstream (~10 LoC of `dev/rustlab/requests/yee-and-pml-builders.md` only) + ~300 LoC of scripted `em.r` in rustlab_em.**
- Phase 2 (if triggered): L (~1100 LoC + tests upstream).

**Risks / open questions:**
- SC-PML implementations have many small wrong-sign-or-wrong-axis bugs. Validate against an Octave reference port (per `AGENTS.md:285-303`) before declaring Phase 1 done.
- The `lessons/_shared/em.r` import path becomes a soft contract — once Lessons 10/11/13 depend on it, breaking changes in the script API need a deprecation path.
- If Phase 2 is triggered late (Lesson 14 has already shipped scripted Yee), the upgrade migrates lesson scripts from script-imports to native builtins. Plan a one-PR migration when the time comes.

**Ship-as: Phase 1 is curriculum-side work, no upstream rustlab PR. Phase 2 (if triggered) gets its own PR. Sized: Phase 1 is 2-3 days in `rustlab_em`. Phase 2 is ~1 week upstream when triggered.**

---

## §4 — Real-typed `./`, `.*`, `.^` (Low / cosmetic — review)

The em_requests.md ask, taken literally, is "when both operands of `./` (or `.*`, `.^`) are real-typed, the result should be real-typed." rustlab has no separate real-typed array variant today (`Value::Vector`/`Value::Matrix` are `CVector`/`CMatrix` per `value.rs:36-102`), so the literal interpretation is a large refactor. Three options:

### Option A — 4-line guard in elem-op arms (pragmatic fix)

Detect that both operands have `max(|im|) < f64::EPSILON` and zero out imag in the result before returning. Applied in the Vector-Vector arms at `value.rs:864-882` and Matrix-Matrix arms at 974-996.

```rust
// in ElemDiv arm (sketch):
let result: CVector = a / b;
if all_real(a) && all_real(b) {
    result.iter_mut().for_each(|c| c.im = 0.0);
}
```

- **Codebase impact:** XS, ~50 LoC including tests.
- **Pros:** trivial; eliminates the curriculum's visible-noise pain today.
- **Cons:**
  - Heuristic threshold (`f64::EPSILON`) — what if upstream rounding produced `1e-17` imag noise? Tunable, but it's a heuristic.
  - Doesn't fix matrix-multiply, `inv`, `fft`, etc. — only the three elem-ops. Three years from now, someone hits a different op with the same symptom and is confused why this fix doesn't apply.
  - No performance gain — still complex zgemm under the hood for matrix mul.

### Option B — Type-tagged `Value::Vector { data: CVector, is_real: bool }` (medium-term)

Storage stays complex; `is_real` bit propagates through ops. Every binop arm sets the output's `is_real` based on inputs' bits and whether the op preserves realness.

- **Codebase impact:** ~500-800 LoC. Touches every binop arm and every builtin that constructs a `Value::Vector`/`Value::Matrix`.
- **Pros:** uniform fix across all ops; `real()` is a true no-op when the bit is set.
- **Cons:** still no perf gain (storage unchanged); large enough that it deserves its own plan.

### Option C — Fully real-typed storage `Value::RealVector(Array1<f64>)` (long-term)

Real perf wins (dgemm, real FFT), cleanest semantics.

- **Codebase impact:** ~2000 LoC + significant ndarray-linalg path work.
- **Pros:** real performance gains; uniform clean semantics.
- **Cons:** huge refactor; most rustlab builtins need real+complex variants; dominates a sprint cycle. Driven by performance, not curriculum correctness.

### Recommendation: ship Option A now, file Options B/C as separate plans

Rationale:
- The em_requests.md curriculum pain is *visible 1e-11 imaginary noise after `./`*. That's solvable today with Option A.
- Option B is the right medium-term answer but is multi-week work that doesn't belong inside this plan — bundling it would balloon the timeline from 2-3 months to 5-6 months with most of the new work unrelated to EM.
- Option C is performance-driven, orthogonal to the curriculum's actual ask.

**Concrete plan:**
1. Ship Option A in this plan as the §4 deliverable.
2. Document Option A as a temporary measure in `docs/functions.md`'s "Type behaviour" section: "elem-ops between real-typed inputs return real-typed output via post-hoc imag zeroing; this will be replaced by a type-tagged value system in a future release."
3. File a separate request `dev/plans/real_typed_values.md` covering Options B/C scoping, to be picked up in a subsequent cycle.

**Codebase impact: XS (~50 LoC + tests).** Tests:
- `[1,2,3] ./ [4,5,6]` → all `im == 0.0` exactly.
- `[1+0i, 2+0i] .* [3+0i, 4+0i]` → all `im == 0.0` (the input *was* complex-typed, but values are real).
- `[1+1i, 2] ./ [3, 4]` → preserves the imag part of input 1 (op input had nonzero imag, fix doesn't apply).

**Ship-as: standalone tiny PR or tucked into §2.5. Sized: half a day.**

---

## Summary table — ship-in-one-PR vs split

| # | Request | LoC | T-shirt | PR strategy |
|---|---|---|---|---|
| §2.5 | rect/disk/polygon masks | ~290 | S | One PR, three builtins |
| §2.3 | real spsolve (faer) | ~700 | L | **Own PR — foundational** |
| §2.2 | laplacian_eps_2d | ~260 | S | **Bundle with §2.1** |
| §2.1 | BC + 1-D + 3-D | ~600 | M | **Bundle with §2.2** |
| §2.4 | eigs(A,B,n) hand-rolled IRAM | ~1200 | L | **Own PR** |
| §4 | real-typed elem-ops (Option A) | ~50 | XS | Bundle with anything |
| §2.7 | polar + loglog | ~480 | M | One PR, four builtins |
| §2.6 Phase 1 | Yee + SC-PML script library | ~310 | S | Curriculum-side, no upstream |
| §2.6 Phase 2 | Yee + SC-PML native crate (if triggered) | ~1100 | L | Own PR when graduation triggered |
| §1 | Status sweep | ~10 | XS | rustlab_em housekeeping |

Total upstream-rustlab work for items 1-7: ~3580 LoC of implementation + ~1100 LoC of tests + docs/REPL updates.

## Suggested implementation order

1. **§2.5 masks** — half a day, smallest, no deps, unblocks Lesson 04.
2. **§2.3 real `spsolve` (faer)** — foundational; everything below depends on scaling.
3. **§2.2 + §2.1** bundled — same `rustlab-dsp/src/laplacian.rs` module, same `bc` plumbing.
4. **§2.4 `eigs(A, B, n)`** — depends on §2.3's LU for shift-invert.
5. **§4 real-typed elem-ops (Option A)** — half a day, tucked alongside any of the above.
6. **§2.7 polar / log axes** — independent; schedule when convenient.
7. **§2.6 Phase 1** (script library) — curriculum-side, no upstream PR.
8. **§2.6 Phase 2** — only if graduation trigger fires.
9. **Animation export** — out of scope for this plan; lives in `dev/rustlab/requests/animation-export.md`.

## Total estimate

**~2-3 weeks** for items 1-5 by a senior implementer. **~6-8 weeks** to ship items 1-7 cleanly with tests + docs + Octave validation. **~3 months** allows for one round of "the eigensolver doesn't converge on this real problem" debugging — and per the no-FFI policy, debugging means improving the hand-rolled Arnoldi, not falling back to ARPACK.

## Cross-cutting concerns

1. **`faer` for §2.3 unlocks §2.4.** The same LU object is the inner loop of shift-invert Arnoldi. Single biggest force-multiplier in the plan and the strongest reason to do §2.3 first.
2. **`bc` parameter pattern from §2.1 generalizes to §2.2.** Once `laplacian_2d` accepts the BC string, give `laplacian_eps_2d` the same fifth-arg signature. Trivial; document so Lesson 06 (`iron_core_shielding.r`) can use Neumann + variable-ε.
3. **Column-major ordering and `ij2k`/`k2ij` consistency.** Every new Laplacian variant uses the same `k = (j-1)·ny + i` flattening so user scripts can compose with `reshape(V, ny, nx)`. Add `ijk2k`/`k2ijk` when shipping `laplacian_3d`.
4. **SPD detection in one place.** `SparseMat::is_hermitian()` and `SparseMat::is_spd_estimate()` in `rustlab-core/src/types.rs`, used by both §2.3 (Cholesky path) and §2.4 (Lanczos vs Arnoldi). ~40 LoC.
5. **All Laplacian variants and `laplacian_eps_2d` benefit from §2.3 landing first.** Without real `spsolve`, users on a 200×200 grid still can't solve the systems even if the matrix is built correctly. Don't ship the Laplacian work in advance of the solver fix or you create a "broken when scaled" footgun.
6. **`docs/quickref.md` Sparse section grows.** Currently 26 lines (301-327); after this plan ~40. Consider splitting into "Sparse Construction", "Sparse Solve & Eigs", "Sparse Stencils" if past a screenful.
7. **`AGENTS.md §All builtin functions` table** at lines 817-925 grows by 12-15 entries. Update per Workflow Rule 4 in each PR; re-sort by category at end if drift.
8. **Octave numerical comparison** (`AGENTS.md:285-303`). §2.3 and §2.4 are the two items where an Octave reference run is most valuable as a correctness check, not just a release-time gate. Run before merging each.

## Critical files for implementation

- `crates/rustlab-script/src/eval/builtins.rs` — every new builtin registers and dispatches here. Existing `builtin_spsolve` (7909-7996) and `builtin_laplacian_2d` (8107-8170) are the closest analogues to extend.
- `crates/rustlab-core/src/types.rs` — `SparseMat`/`SparseVec` definitions; needs LU/Cholesky factorization API and faer-conversion helpers for §2.3, plus `is_hermitian`/`is_spd_estimate` for §2.4.
- `crates/rustlab-dsp/src/vector_calc.rs` — pattern reference (and adjacent file location) for the new `laplacian.rs` module hosting §2.1 + §2.2.
- `crates/rustlab-cli/src/commands/repl.rs` — every new builtin needs a `HelpEntry` (lines 19-510) and an entry in the `categories` table (lines 813-1002).
- `crates/rustlab-plot/src/figure.rs` and `crates/rustlab-plot/src/file.rs` — `SubplotState` (figure.rs:183) and `build_cartesian_2d` (file.rs:282) are the touchpoints for §2.7's log-axis and polar work.
- `crates/rustlab-script/src/eval/value.rs:864-996` — Vector-Vector and Matrix-Matrix elem-op arms for §4 Option A.

## Decisions locked in this plan

1. **§2.3 sparse solver:** **hand-rolled, pure Rust, in `rustlab-core`.** `faer` was the original plan but was rejected as too large a library for core work. Per `AGENTS.md` Rule 9, core algorithms must be hand-rolled. Item 2 is now a multi-phase build: CSC storage → Cholesky (SPD) → LU (general) → simple fill-reducing ordering → wire-in. Reference: Davis, *Direct Methods for Sparse Linear Systems*. See the per-phase breakdown in `dev/plans/em_requests_queue.md` Item 2.
2. **§2.6 home:** Phase 1 = scripted library in `rustlab_em`; Phase 2 = upstream `rustlab-em` workspace crate, only if graduation triggers fire.
3. **§2.4 fallback:** none. Pure-Rust hand-rolled IRAM on top of the §2.3 hand-rolled LU/Cholesky. No `arpack-ng-sys`, no Fortran FFI.
4. **§4 scope:** Option A (4-line pragmatic fix) ships in this plan. Options B (type-tagged value variant) and C (fully real-typed storage) deferred to a separate plan.
5. **Dependency policy** (`AGENTS.md` Rule 9): **core functionality must be written in pure Rust.** Libraries acceptable only for infrastructure (graphics, plotting, terminal UI, I/O, parsing). Any proposal to use a library on core work requires a written trade-off study at `dev/plans/<topic>-tradeoff.md` before code lands. Hard limits: no GPL/LGPL/copyleft, no Fortran/C++ FFI, no "large library", no vendored solvers the curriculum is supposed to teach.

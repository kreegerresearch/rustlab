# Implementation Queue — `em_requests` (action doc for AI agent handoff)

> **For the next agent:** This is the *action doc*. Read this first.
> The *reference doc* is `dev/plans/em_requests_plan.md` — it has the rationale, decisions, and risks. Don't re-litigate decisions in the reference doc; just execute.
> Source request: `../rustlab_em/dev/rustlab/requests/em_requests.md` (read for curriculum context).

**Last updated:** 2026-04-26
**Status of plan:** Proposed; awaiting kickoff approval.
**Next item to start:** **Item 1 — §2.5 rasterization masks** (warm-up, half-day, no deps).

---

## Decisions already locked (do not revisit)

1. **Sparse solver dep:** `faer` (pure Rust, MIT-or-Apache-2.0). Not UMFPACK (GPL). Not MKL.
2. **Sparse eigensolver:** hand-rolled implicit-restart Arnoldi on `faer` LU; Lanczos for SPD. **No FFI.** Not `arpack-ng-sys`.
3. **Yee + SC-PML home:** scripted library in `rustlab_em/lessons/_shared/em.r` (Phase 1). Workspace crate only on graduation trigger (Phase 2).
4. **Real-typed elem-ops:** Option A (4-line guard zeroing imag when both inputs essentially real). Options B/C deferred.
5. **General licensing policy:** no GPL/LGPL/AGPL/copyleft, no Fortran/C++ FFI by default. Pure-Rust MIT/Apache-2.0 or stop and ask.

---

## Six mandatory workflow rules (apply to every item)

Per `feedback_workflow.md` and `AGENTS.md`:
1. **Plan first** — present a written plan and wait for approval before code.
2. **Tests in the same commit** — `crates/rustlab-script/src/tests.rs` for builtins; algorithm tests in their owning crate. Run `cargo test --workspace` *and* `cargo test --workspace --features viewer` before declaring done.
3. **No commit without explicit approval** — present summary, wait for "commit" / "push".
4. **Update `AGENTS.md`** — function table at lines 817-925 in same commit.
5. **Update `docs/quickref.md`** — every new function in its category in same commit.
6. **Update `docs/functions.md`** + REPL `HelpEntry` (struct at `repl.rs:13`) + `categories` table (`repl.rs:813`) in same commit.

A feature is **not done** until `help foo` in the REPL returns a useful answer.

---

## Verified file:line landmarks (re-verify if more than ~14 days old)

| What | Path | Line |
|---|---|---|
| `fn builtin_eig` (dense) | `crates/rustlab-script/src/eval/builtins.rs` | 5011 |
| `fn builtin_spsolve` (densifies, replace body) | `crates/rustlab-script/src/eval/builtins.rs` | 7909 |
| `fn builtin_laplacian_2d` (extend) | `crates/rustlab-script/src/eval/builtins.rs` | 8107 |
| `r.register("eig", …)` | `crates/rustlab-script/src/eval/builtins.rs` | 190 |
| `r.register("spsolve", …)` | `crates/rustlab-script/src/eval/builtins.rs` | 270 |
| `r.register("laplacian_2d", …)` | `crates/rustlab-script/src/eval/builtins.rs` | 273 |
| `r.register("ij2k", …)` | `crates/rustlab-script/src/eval/builtins.rs` | 274 |
| `pub struct SparseVec` | `crates/rustlab-core/src/types.rs` | 22 |
| `pub struct SparseMat` | `crates/rustlab-core/src/types.rs` | 151 |
| `impl SparseMat` | `crates/rustlab-core/src/types.rs` | 157 |
| `pub struct SubplotState` | `crates/rustlab-plot/src/figure.rs` | 183 |
| `build_cartesian_2d` call sites | `crates/rustlab-plot/src/file.rs` | 282, 845 |
| `pub struct HelpEntry` | `crates/rustlab-cli/src/commands/repl.rs` | 13 |
| `let categories = [` | `crates/rustlab-cli/src/commands/repl.rs` | 813 |
| `fn print_help_list` | `crates/rustlab-cli/src/commands/repl.rs` | 804 |
| Vector-Vector elem-op arm | `crates/rustlab-script/src/eval/value.rs` | 864 |
| Matrix-Matrix elem-op arm | `crates/rustlab-script/src/eval/value.rs` | 974 |

Re-verify any of these with `grep -n` before editing.

---

## Queue

Status legend: `[ ]` not started · `[~]` in progress · `[✓]` shipped · `[B]` blocked

### `[ ]` Item 1 — §2.5 rasterization masks (`rect_mask`, `disk_mask`, `polygon_mask`)

**Priority: HIGH (warm-up)** · **Size: S (~290 LoC + tests)** · **Time: 0.5-1 day** · **Deps: none**

**Why first:** smallest item, no dependencies, unblocks Lesson 04 (the curriculum pivot lesson — currently has only a notebook draft, no `lessons/04-*` directory yet).

**Acceptance criteria:**
- `disk_mask(meshgrid output, 0, 0, 1)` summed × cell area on a 100×100 grid approximates π to ~1%.
- `polygon_mask(X, Y, [0 0; 1 0; 1 1; 0 1])` equals `rect_mask(X, Y, 0, 0, 1, 1)` exactly.
- Empty / single-vertex / collinear polygons return all-zero matrix without panicking.

**File checklist:**
- [ ] Create `crates/rustlab-dsp/src/rasterize.rs` (~90 LoC algorithm: ray-casting + element-wise comparisons).
- [ ] Add `mod rasterize;` to `crates/rustlab-dsp/src/lib.rs`.
- [ ] Add three builtins (`builtin_rect_mask`, `builtin_disk_mask`, `builtin_polygon_mask`) in `builtins.rs` near existing geometry builtins.
- [ ] Register all three near `builtins.rs:273` (alongside `laplacian_2d`).
- [ ] Add three `HelpEntry` records in `repl.rs` (after line 13).
- [ ] Add the three names to the appropriate category slice in `repl.rs:813` `categories` table.
- [ ] Tests in `crates/rustlab-dsp/src/tests.rs` (algorithm) and `crates/rustlab-script/src/tests.rs` (builtin contract).
- [ ] Update `docs/functions.md`, `docs/quickref.md`, `AGENTS.md` function table.

**Verification command:** `cargo test --workspace -- rasterize` then `cargo test --workspace --features viewer`.

---

### `[ ]` Item 2 — §2.3 real `spsolve` (faer)

**Priority: CRITICAL — scaling cliff** · **Size: L (~700 LoC + tests)** · **Time: 3-5 days senior + 2 days perf/Octave** · **Deps: none upstream; pulls in `faer` 0.20**

**Why second:** foundational. Every Laplacian/eigs item below depends on this scaling. Currently `spsolve` densifies internally — a 100×100 Lesson 05 grid produces a 10⁴×10⁴ matrix → ~800 MB densified.

**Acceptance criteria:**
- `spsolve(I, b) == b` on a 1000×1000 sparse identity within machine precision.
- Round-trip on `laplacian_2d(50,50)` Poisson solve matches the dense reference.
- 200×200 cavity-cross-section problem (~40k×40k) runs in <10s, doesn't OOM.
- Complex-RHS path tested (FDFD-style).
- Singular matrix returns clear error, not a panic.
- Octave reference comparison (`AGENTS.md:285-303`) passes for at least one PDE assembly.

**File checklist:**
- [ ] Add `faer = "0.20"` to `Cargo.toml [workspace.dependencies]`. **Verify license: MIT-or-Apache-2.0.**
- [ ] Add `sparse-solve` feature to `crates/rustlab-core/Cargo.toml` (default-on in cli/script crate features).
- [ ] Create `crates/rustlab-core/src/sparse_solve.rs`:
  - `SparseMat::to_faer_csc()` conversion.
  - `SparseMat::lu_factor() -> SparseLU` and `SparseLU::solve()`.
  - `SparseMat::cholesky() -> SparseChol` (SPD path).
  - Real-vs-complex auto-detection (`max |im| < 1e-12` → real path).
- [ ] Add `SparseMat::is_hermitian()` and `SparseMat::is_spd_estimate()` helpers in `types.rs` (~40 LoC, **shared with Item 4**).
- [ ] Replace body of `builtin_spsolve` at `builtins.rs:7909-7996` with thin call. Preserve the `if x.len()==1` scalar-return shape at lines 7986-7995.
- [ ] Add optional 3rd-arg dispatch: `spsolve(A, b, "auto" | "lu" | "cholesky")`.
- [ ] Tests in `crates/rustlab-core/src/tests.rs` (factor/solve algorithm) and `crates/rustlab-script/src/tests.rs` (builtin contract).
- [ ] Update `docs/functions.md` (replace the "converts to dense internally" note at the existing `spsolve` section), `docs/quickref.md`, `AGENTS.md`, REPL help.

**Watch out for:**
- faer's deterministic-mode flag (set it for reproducibility).
- Symmetry detection cost — gate behind one-time check inside factorization, not every solve.
- Tiny problems (n<100) may regress vs current dense path; accept it unless `perf/report.md` flags.

**Verification command:** `cargo test --workspace -- spsolve` and run `lessons/05-poisson-laplace-bvp/*.r` (when those scripts exist).

---

### `[ ]` Item 3 — §2.2 + §2.1 bundled (Laplacian variants)

**Priority: HIGH** · **Size: M (~860 LoC + tests combined)** · **Time: 3-4 days bundled** · **Deps: should land after Item 2 ships (otherwise users can build the matrix but can't solve it at scale)**

Bundles two requests because they touch the same module and same `bc` plumbing:
- **§2.2** `laplacian_eps_2d(eps_map [, dx, dy] [, bc])` — variable-coefficient flux-conservative.
- **§2.1** `laplacian_2d` BC extensions (4th arg `"dirichlet"` default | `"neumann"` | `"periodic"`) + `laplacian_1d` + `laplacian_3d`.

**Acceptance criteria:**
- `laplacian_eps_2d` with `eps_map ≡ 1.0` equals `laplacian_2d` exactly.
- Flux conservation: interior row sums of `laplacian_eps_2d` are 0.
- Dirichlet `λ_min` matches analytic π²(m²+n²)/L² for `laplacian_2d_bc(20,20,_,_,"dirichlet")`.
- Neumann variant has zero eigenvalue (constant null-space).
- Periodic variant has 2-D Fourier-mode eigenvalues `4 sin²(πk/N)`.
- `laplacian_3d` round-trip on a known analytic test case.
- Existing `laplacian_2d` 2-arg and 4-arg call sites still work (backwards-compat).

**File checklist:**
- [ ] Create `crates/rustlab-dsp/src/laplacian.rs` (model on `vector_calc.rs:103`).
- [ ] Add `mod laplacian;` to `crates/rustlab-dsp/src/lib.rs`.
- [ ] Implement: `laplacian_eps_2d`, `laplacian_2d_bc`, `laplacian_1d`, `laplacian_3d`.
  - Column-major flat indexing `k = j·ny + i` (2-D); `k = (kk·nx + j)·ny + i` (3-D).
  - **Verify Tensor3 axis convention** in `rustlab-core/src/types.rs:11` — `(rows, cols, pages) = (ny, nx, nz)`.
- [ ] Extend `builtin_laplacian_2d` at `builtins.rs:8107` to accept optional 5th string arg. Change `check_args_range("laplacian_2d", &args, 2, 4)` → `2, 5`.
- [ ] Add `builtin_laplacian_eps_2d`, `builtin_laplacian_1d`, `builtin_laplacian_3d`.
- [ ] Register all four near `builtins.rs:273`.
- [ ] Add `ijk2k` / `k2ijk` 3-D index sugar (~30 LoC).
- [ ] HelpEntry + categories for each new builtin.
- [ ] Tests in `rustlab-dsp/src/tests.rs` (5 invariants above).
- [ ] Update `docs/functions.md` (rewrite the "Neumann and periodic … not implemented in v1" disclaimer near the `laplacian_2d` section), `docs/quickref.md`, `AGENTS.md`.

**Watch out for:**
- **Periodic + spsolve = singular.** Document the row-pinning workaround in `docs/functions.md`: zero row 1, set `(1,1)=1`, pin RHS.
- **Harmonic mean must be at cell faces, not arithmetic mean.** Document explicitly.
- Lesson 06 (`iron_core_shielding.r` — not yet drafted) wants Neumann + variable-ε — make sure `laplacian_eps_2d` accepts the same `bc` arg as `laplacian_2d_bc`.

---

### `[ ]` Item 4 — §2.4 `eigs(A, n)` and `eigs(A, B, n)`

**Priority: HIGH** · **Size: L (~1200 LoC + tests)** · **Time: 5-8 days senior with Krylov experience; 2-3 weeks otherwise** · **Deps: Item 2 (uses faer LU for shift-invert)**

**Read before starting:** Saad, *Iterative Methods for Sparse Linear Systems*, ch. 6-8 (Arnoldi / IRAM / Lanczos).

**Acceptance criteria:**
- `eigs(laplacian_2d(20,20), 4, "sm")` returns 4 lowest eigenvalues matching analytic π²(m²+n²)/L² to <1%.
- `eigs(A, A, n)` returns 1.0 with multiplicity n for any non-singular A.
- SPD path (Lanczos) agrees with general path (Arnoldi) within machine precision on a hand-built SPD test matrix.
- Convergence info populated: `info.iterations`, `info.residual`.
- 40 000×40 000 cavity problem at sigma=0 returns 10 eigenpairs in seconds, not minutes.

**File checklist:**
- [ ] Create `crates/rustlab-core/src/sparse_eig.rs`:
  - `pub fn eigs(a, n, which, sigma) -> (CMatrix, CVector)` (standard).
  - `pub fn eigs_gen(a, b, n, which, sigma)` (generalized).
  - Hand-rolled Arnoldi w/ implicit restart (~150 LoC just for restart).
  - Lanczos specialization for SPD detected via `is_spd_estimate()` from Item 2.
  - Generalized-problem reduction for B-SPD case (Cholesky factor B, transform `B^{-1/2} A B^{-1/2} y = λ y`). Defer B-indefinite case; document the restriction.
- [ ] Add `builtin_eigs` next to `builtin_eig` at `builtins.rs:5011`.
- [ ] Register near `builtins.rs:190` alongside `eig`.
- [ ] HelpEntry + categories.
- [ ] Tests covering all 5 acceptance criteria.
- [ ] Update `docs/functions.md`, `docs/quickref.md`, `AGENTS.md`, REPL help.
- [ ] **Octave reference comparison** before merge.

**Watch out for:**
- **No FFI escape hatch.** If hand-rolled Arnoldi stalls on a real curriculum problem, the answer is to read more Saad and improve the algorithm. Do NOT bring in `arpack-ng-sys`.
- **API:** return `D` as a length-n `CVector` (matches `eig`'s convention), not diagonal sparse.
- **`which` arg:** accept `"sm"`, `"lm"`, or numeric `sigma` (shift-invert). Default `"sm"`.
- Real and complex inputs both supported.

---

### `[ ]` Item 5 — §4 real-typed elem-ops (Option A pragmatic fix)

**Priority: LOW (cosmetic, but every-lesson friction)** · **Size: XS (~50 LoC + tests)** · **Time: half a day** · **Deps: none — bundle with anything**

**Acceptance criteria:**
- `[1,2,3] ./ [4,5,6]` → result vector with all `c.im == 0.0` exactly.
- `[1+0i, 2+0i] .* [3+0i, 4+0i]` → all `im == 0.0`.
- `[1+1i, 2] ./ [3, 4]` → preserves the imag part of input 1 (input had nonzero imag, fix doesn't apply).
- All curriculum scripts that currently wrap with `real(...)` for elem-op output can drop the wrapper and still print the same values.

**File checklist:**
- [ ] In `crates/rustlab-script/src/eval/value.rs`:
  - Vector-Vector elem-op arm at line 864 — add `all_real(a) && all_real(b)` guard, zero imag in result.
  - Matrix-Matrix elem-op arm at line 974 — same guard.
  - (Scalar-broadcast arms at 907-929 already preserve realness because the iterator preserves zero imag — verify, no change expected.)
  - Helper: `fn all_real(a: &CVector) -> bool` checks `a.iter().all(|c| c.im.abs() < f64::EPSILON)`.
- [ ] Tests in `crates/rustlab-script/src/tests.rs` (3 invariants).
- [ ] Update `docs/functions.md` "Type behaviour" section: document this as a temporary measure pending a future type-tagged value system. Reference the deferred plan placeholder.
- [ ] Update `AGENTS.md` if there's a relevant subsection.

**Watch out for:**
- Threshold is `f64::EPSILON` (≈2.2e-16). Document that.
- Don't apply to ops other than `./` `.*` `.^` — em_requests.md §4 only asks for elem-ops.
- Don't try to fix matrix-multiply, `inv`, `fft` etc. in this item — that's the deferred Option B/C work.

---

### `[ ]` Item 6 — §2.7 polar / log-axis plots

**Priority: MEDIUM** · **Size: M (~480 LoC + tests)** · **Time: 4-5 days** · **Deps: none — independent of numerics**

**Acceptance criteria:**
- `loglog([1 10 100], [1 100 10000])` produces a straight line on log-log axes (SVG, HTML, terminal all show log scale).
- `semilogx`, `semilogy` work on standard signal-processing test cases.
- `polar(linspace(0, 2*pi, 100), ones(100, 1))` produces a unit circle.
- Negative or zero data on log axes returns clear error, not panic.

**File checklist:**
- [ ] Extend `SubplotState` at `crates/rustlab-plot/src/figure.rs:183` with `x_scale: AxisScale`, `y_scale: AxisScale` (`Linear | Log10`), and `polar: bool` (or factor polar as a separate plot kind — recommended).
- [ ] Branch on `x_scale`/`y_scale` at the two `build_cartesian_2d` call sites in `crates/rustlab-plot/src/file.rs` (282, 845). Use plotters' `LogCoord<f64>`. ~120 LoC; the chart-builder type signature changes.
- [ ] Add Plotly axis config (`xaxis: { type: 'log' }`) in `crates/rustlab-plot/src/html.rs`. ~20 LoC.
- [ ] Log-transform data for ratatui terminal in `crates/rustlab-plot/src/ascii.rs`; label axis "log10(x)". ~30 LoC.
- [ ] Polar renderer (recommended as own plot kind):
  - Plotters: pre-transform `(theta, r) → (r·cos θ, r·sin θ)` + radial gridlines as additional series.
  - Plotly: use `'type': 'scatterpolar'`.
  - ~150 LoC.
- [ ] Four builtins: `builtin_loglog`, `builtin_semilogx`, `builtin_semilogy`, `builtin_polar` in `builtins.rs`. Each is a thin shim that sets the scale flags then calls the `plot` codepath.
- [ ] HelpEntry + categories for each.
- [ ] Tests (mainly contract tests; visual diff covered by existing snapshot infrastructure).
- [ ] Update `docs/functions.md`, `docs/quickref.md`, `AGENTS.md`.

**Watch out for:**
- **Polar is structurally different.** It's a coord system (r, θ), not a scaled axis. Build as a separate plot kind, don't retrofit through `x_scale`.
- **Negative/zero on log:** plotters will panic. Add explicit error: `"loglog: data must be strictly positive (got minimum -0.5)"`.
- **HTML/SVG style divergence on log axes.** Both correct, just stylistically different. Document.
- If polar slips schedule, ship `loglog`/`semilogx`/`semilogy` first and follow with polar in a second PR.

---

### `[ ]` Item 7 — §2.6 Phase 1: Yee + SC-PML scripted library

**Priority: MEDIUM** · **Size: S (~310 LoC scripted)** · **Time: 2-3 days** · **Deps: none upstream; lives in `rustlab_em`, not in rustlab**

**This item is curriculum-side work. Zero rustlab upstream PR.**

**Acceptance criteria:**
- `yee_curl_2d(50, 50, 0.01, 0.01)` returns two sparse curl operators `Ce`, `Ch` such that `Ch * Ce` applied to a smooth field reproduces `-∇²` to discretization order on the interior.
- `scpml_stretch(50, 50, 8, 1e9, 1.0)` returns length-50 complex vectors that are 1.0 outside the PML region and have monotonically growing imaginary part inside.
- Octave reference comparison (`AGENTS.md:285-303`) for at least one assembly.

**File checklist:**
- [ ] Create `../rustlab_em/lessons/_shared/em.r` with `yee_curl_2d` and `scpml_stretch` scripted implementations.
- [ ] Create `../rustlab_em/lessons/_shared/README.md` documenting the import pattern.
- [ ] File spec upstream as `../rustlab_em/dev/rustlab/requests/yee-and-pml-builders.md` with `Status: Discussion` — captures the API even though no upstream code lands.
- [ ] When Lessons 10/11/13 draft, they `run("../_shared/em.r")` to import.

**Graduation triggers (escalate to Phase 2 / native crate if any fire):**
- Lesson 14 capstone needs 3-D Yee.
- Any lesson's Yee assembly takes >5s end-to-end.
- A second physics curriculum needs similar builders.
- Scripted assembly hits a language-feature wall.

---

### `[B]` Item 8 — §2.6 Phase 2: Yee + SC-PML native workspace crate

**Status: BLOCKED on graduation trigger.** Do not start until one of Item 7's graduation triggers fires.

**Priority: MEDIUM (when triggered)** · **Size: L (~1100 LoC + tests)** · **Time: ~1 week**

When triggered: new `crates/rustlab-em` workspace crate behind feature flag `em` (default-off). Migrate Lessons 10/11/13 from script-imports to native builtins in a follow-up PR.

---

### Out of scope for this queue

- **§1 housekeeping sweep** (flip Status fields on the four already-shipped request files + update README priority table). One-PR `rustlab_em` edit, not part of upstream rustlab work. Do whenever convenient.
- **Animation export** — covered by existing `dev/rustlab/requests/animation-export.md`, separate scoping.
- **Deferred:** real-typed elem-ops Options B (~500-800 LoC, type-tagged value variant) and C (~2000 LoC, fully real-typed storage). File as `dev/plans/real_typed_values.md` when that work cycle starts.

---

## Suggested execution order (ordered, not parallel)

1. **Item 1** — masks (warm-up, half-day).
2. **Item 2** — spsolve (foundational).
3. **Item 3** — Laplacian variants bundled.
4. **Item 4** — eigs.
5. **Item 5** — real-typed elem-ops (slot in alongside any of the above).
6. **Item 6** — polar / log axes (independent, schedule when convenient).
7. **Item 7** — Yee Phase 1 (curriculum-side, no upstream PR).
8. **Item 8** — only if graduation trigger fires.

**Estimate:** 2-3 weeks for items 1-5 (senior); 6-8 weeks for items 1-7 with Octave validation; ~3 months allowing for one round of eigensolver convergence debugging.

---

## Cross-cutting reminders

- **Item 2's `is_hermitian` / `is_spd_estimate` helpers are shared by Item 4.** Implement them once in `types.rs`.
- **Item 2's faer LU is the inner loop of Item 4's shift-invert Arnoldi.** Item 4 cannot start until Item 2 ships.
- **Item 3's `bc` parameter pattern applies to Item 3's `laplacian_eps_2d` too.** Once `laplacian_2d_bc` accepts the BC string, give `laplacian_eps_2d` the same fifth-arg signature so Lesson 06 can use Neumann + variable-ε.
- **Items 3 and 4 land Laplacian variants and eigensolvers users can't actually use at scale until Item 2 lands.** Don't ship Items 3/4 in advance of Item 2 or you create a "broken when scaled" footgun.
- **Octave numerical comparison** (`AGENTS.md:285-303`) is a *correctness* checkpoint, not just a release-time gate, for Items 2 and 4. Run before merging each.
- **REPL help is not optional.** A feature is not done until `help foo` returns a useful answer.

---

## Pre-flight checklist before each item

- [ ] Re-read decisions section above. Don't re-litigate.
- [ ] Re-verify file:line landmarks if more than ~14 days have passed since last update.
- [ ] Check `git log --oneline -20` for any recent landings that affect the item.
- [ ] Read the corresponding section of `dev/plans/em_requests_plan.md` for full rationale.
- [ ] Read the relevant `../rustlab_em/dev/rustlab/requests/em_requests.md` section for curriculum context.
- [ ] Write a short plan, present, wait for approval (Workflow Rule 1).

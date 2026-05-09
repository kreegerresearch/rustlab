# Implementation Queue — `em_requests` (action doc for AI agent handoff)

> **For the next agent:** This is the *action doc*. Read this first.
> The *reference doc* is `dev/plans/em_requests_plan.md` — it has the rationale, decisions, and risks. Don't re-litigate decisions in the reference doc; just execute.
> Source request: `../rustlab_em/dev/rustlab/requests/em_requests.md` (read for curriculum context).

**Last updated:** 2026-04-26
**Status of plan:** All upstream-rustlab items shipped. Curriculum-side
items (§2.6 Phase 1) deferred to when lessons need them; the spec is
filed.

**Shipped:**
- Item 1 (masks): `5791ec0`
- Item 2 (sparse solve, Phases 1+2): `6623496`
- Item 2 (sparse solve, Phases 3+4): `e9283b7`
- Item 2 demos (electrostatics, complex Helmholtz, scaling): `5feef19`
- Item 3 (Laplacian BC + 1-D/3-D + eps + doc audit): `26954a3`
- Item 4 (sparse `eigs(A, n)` / `eigs(A, B, n)`): `7eb5672`
- Item 5 (real-typed elem-ops Option A) and Item 6 (log/polar plot
  shims): pending commit (about to land)

**Filed (no upstream code):**
- Item 7 spec → `rustlab_em/dev/rustlab/requests/yee-and-pml-builders.md`
  with `Status: Discussion`. Phase 1 (scripted library in
  `rustlab_em/lessons/_shared/em.rlab`) lands when curriculum drafts
  Lessons 10/11/13. Phase 2 (upstream `rustlab-em` crate) waits for a
  graduation trigger (3-D Yee, >5s assembly, second physics
  curriculum, language-feature wall).

**Item 8 (Phase 2 native crate):** blocked on graduation triggers;
not currently scheduled.

---

## Decisions already locked (do not revisit)

1. **Sparse solver:** hand-rolled, pure Rust, in `rustlab-core`. **`faer` is rejected** (too large a library — see `AGENTS.md` Rule 9). UMFPACK rejected (GPL). MKL rejected. No FFI. Item 2 is now a multi-phase hand-roll, not a wrapper around an existing solver — see Item 2 for the breakdown.
2. **Sparse eigensolver:** hand-rolled Arnoldi / Lanczos on top of the rustlab-core hand-rolled LU/Cholesky from Item 2. **No FFI.** Not `arpack-ng-sys`.
3. **Yee + SC-PML home:** scripted library in `rustlab_em/lessons/_shared/em.rlab` (Phase 1). Workspace crate only on graduation trigger (Phase 2).
4. **Real-typed elem-ops:** Option A (4-line guard zeroing imag when both inputs essentially real). Options B/C deferred.
5. **Dependency policy** (`AGENTS.md` Rule 9): **core functionality must be written in pure Rust.** Libraries acceptable only for infrastructure (graphics, plotting, terminal UI, I/O, parsing). Any proposal to use a library on core work requires a written trade-off study at `dev/plans/<topic>-tradeoff.md` before code lands. Hard limits (override even a good trade-off study): no GPL/LGPL/copyleft, no Fortran/C++ FFI, no "large library", no vendored solvers the curriculum is supposed to teach.

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

## Shipped — archive

Detailed implementation plans have been removed (work is in main; per-item file checklists and watch-outs were one-time guides). One-line summaries with commit references and gallery links are kept here.

| § | Item | Commit(s) | Notebook(s) |
|---|---|---|---|
| §2.5 | rasterization masks (`rect_mask`, `disk_mask`, `polygon_mask`) | `5791ec0` | `gallery/masks.md` |
| §2.3 | sparse `spsolve` (5 phases hand-rolled: CSC, Cholesky, LU, AMD, wire-in) | `6623496`, `e9283b7`, `5feef19` | `gallery/sparse_solve.md`, `gallery/sparse_scaling.md`, `gallery/electrostatics.md`, `gallery/sparse_complex.md` |
| §2.1 + §2.2 | Laplacian BC selector + 1-D / 3-D variants + `laplacian_eps_2d` + `ijk2k` / `k2ijk` (plus a doc-audit pass) | `26954a3` | `gallery/laplacian_bc.md`, `gallery/dielectric.md` |
| §2.4 | sparse partial eigensolver `eigs(A, n)` / `eigs(A, B, n)` (Lanczos + Arnoldi + Jacobi + Hessenberg-QR) | `7eb5672` | `gallery/eigs.md` |
| §4 + §2.7 | real-typed elem-ops (Option A) and log/polar plot shims (`loglog`, `semilogx`, `semilogy`, `polar`) | (pending) | `gallery/log_polar.md` (pending) |

Per-phase plan for §2.3 lives at `dev/plans/closed/sparse_solve_handroll.md` (now closed). Performance writeup at `perf/sparse_solve_phase1to4.md`.

---

## Queue

Status legend: `[ ]` not started · `[~]` in progress · `[✓]` shipped · `[B]` blocked

### `[✓]` Item 4 — §2.4 `eigs(A, n)` and `eigs(A, B, n)`

**Shipped in commit `7eb5672`** (2026-04-26). Hand-rolled Lanczos for SPD inputs, Arnoldi for general; small dense subproblem via cyclic Jacobi (symmetric) or shifted QR (Hessenberg). Generalized form `A x = λ B x` reduces via `SparseChol(B)`. Implicit restart and shift-invert deferred to follow-up. See `gallery/eigs.md` for the walkthrough notebook.

**Priority: HIGH** · **Size: L (~1200 LoC + tests)** · **Deps: Item 2 (uses rustlab-core `SparseLU::factor` and `SparseChol::factor` for shift-invert)**

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
- [ ] Create `../rustlab_em/lessons/_shared/em.rlab` with `yee_curl_2d` and `scpml_stretch` scripted implementations.
- [ ] Create `../rustlab_em/lessons/_shared/README.md` documenting the import pattern.
- [ ] File spec upstream as `../rustlab_em/dev/rustlab/requests/yee-and-pml-builders.md` with `Status: Discussion` — captures the API even though no upstream code lands.
- [ ] When Lessons 10/11/13 draft, they `run("../_shared/em.rlab")` to import.

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

1. **Item 1** — masks. **✓ shipped** in `5791ec0`.
2. **Item 2** — spsolve. **✓ shipped** in `6623496` + `e9283b7` + `5feef19`.
3. **Item 3** — Laplacian variants. **✓ shipped** in `26954a3`.
4. **Item 4** — eigs. **← next**.
5. **Item 5** — real-typed elem-ops (slot in alongside any of the above).
6. **Item 6** — polar / log axes (independent, schedule when convenient).
7. **Item 7** — Yee Phase 1 (curriculum-side, no upstream PR).
8. **Item 8** — only if graduation trigger fires.

**Estimate (revised against actuals):** Items 1, 2, 3 took roughly one calendar day of focused work end-to-end (masks ~1 hour, sparse solve ~half-day, Laplacian variants ~2 hours). The original 6-8 week estimate assumed senior-with-context productivity at standard pace; the actual pace was faster because: (a) hand-rolling sparse Cholesky/LU was algorithmic-port work rather than greenfield design, (b) the `SparseMat::is_hermitian` / `is_spd_estimate` helpers from Item 2 directly served Item 3, (c) the basic-AMD compromise side-stepped the longest-tail phase. Item 4 (eigs) is expected to take half-day to one day in the same mode; the deferred enhancements (full Davis AMD, IRAM restart, shift-invert) would each be a separate sub-day cycle later.

---

## Cross-cutting reminders

- **Item 2's `is_hermitian` / `is_spd_estimate` helpers are shared by Item 4.** Implemented in `types.rs` during Item 2 wire-in.
- **Item 4 was originally planned to use faer LU for shift-invert; with the hand-roll, the inner loop is the rustlab-core `SparseLU::factor` and the rustlab-core `SparseChol::factor`.** Both are available now from Item 2.
- **Item 3's `bc` parameter pattern** applies to `laplacian_eps_2d` too — implemented and tested.
- **Octave numerical comparison** (`AGENTS.md:285-303`) is a *correctness* checkpoint for Items 2 and 4. Item 2's hand-roll was validated against rustlab's own dense Gaussian path on multiple matrices; Octave runs are deferred to a release-prep gate.
- **REPL help is not optional.** A feature is not done until `help foo` returns a useful answer. Verified post-Item-3 for every registered builtin.

---

## Pre-flight checklist before each item

- [ ] Re-read decisions section above. Don't re-litigate.
- [ ] Re-verify file:line landmarks if more than ~14 days have passed since last update.
- [ ] Check `git log --oneline -20` for any recent landings that affect the item.
- [ ] Read the corresponding section of `dev/plans/em_requests_plan.md` for full rationale.
- [ ] Read the relevant `../rustlab_em/dev/rustlab/requests/em_requests.md` section for curriculum context.
- [ ] Write a short plan, present, wait for approval (Workflow Rule 1).

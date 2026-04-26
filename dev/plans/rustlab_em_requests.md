# Development Plan: `rustlab_em` Feature Requests

**Status:** Phases 1, 1.5, 2, 3, 4 landed — Phase 5 (animation export) remaining.
**Date opened:** 2026-04-22
**Source:** `../rustlab_em/dev/rustlab/requests/*.md` (5 originals)
**Current rustlab version at plan creation:** `0.1.9`
**Current rustlab version:** `0.1.11` (after Phase 4)

**Follow-on work:** the curriculum's second sweep against rustlab is tracked in `dev/plans/em_requests_plan.md` + `dev/plans/em_requests_queue.md` (the §2026-04 sweep, nine asks). Phase 4 of *this* plan (laplacian stencil builder) was extended in the follow-on plan with BC selectors, 1-D / 3-D variants, and `laplacian_eps_2d` — see commit `26954a3`. Phase 5 (animation export) remains the lone un-shipped piece of this original five-request set.

---

## Purpose

The `rustlab_em` curriculum has filed five standalone feature requests against
rustlab. Each request is a self-contained proposal with motivation, proposed
API, and semantics. This plan sequences the work into phases a follow-up
agent can execute independently.

The request files live at:

```
../rustlab_em/dev/rustlab/requests/
├── README.md                       # priority table
├── vector-calculus-operators.md    # #1 High
├── quiver-and-streamplot.md        # #2 High
├── contour-plots.md                # #3 High
├── laplacian-stencil-builder.md    # #4 Medium
└── animation-export.md             # #5 Low
```

Read the individual request files before starting each phase — they contain
the canonical API spec. This plan only sequences and gates them; it does
not duplicate their contents.

---

## Related plans

- **`dev/plans/tensor3.md`** — foundational `Value::Tensor3` work. ✅
  Complete (2026-04-22). 3D numerical operators (`gradient3`,
  `divergence3`, `curl3`) are now unblocked as a Phase 1.5 follow-up.
  Phase 1 below remains scoped to 2D to keep the rustlab_em request unblocks
  small and independently reviewable.

## Prerequisites already verified (2026-04-22)

These were checked against the current `main` branch so follow-up agents do
not need to re-verify:

| Dependency | Status | Location |
|------------|--------|----------|
| Sparse matrix type | ✅ present | `rustlab-core` — see `dev/plans/sparse.md` (marked Complete) |
| `spsolve(A, b)` | ✅ registered | `crates/rustlab-script/src/eval/builtins.rs:249` |
| `spdiags(V, D, m, n)` | ✅ registered | `crates/rustlab-script/src/eval/builtins.rs:250` |
| `hold` on/off | ✅ registered | `crates/rustlab-script/src/eval/builtins.rs:132` |
| `imagesc`, `surf`, `plot`, `stem` | ✅ registered | `crates/rustlab-script/src/eval/builtins.rs:126–144` |
| `meshgrid` | ✅ registered | `crates/rustlab-script/src/eval/builtins.rs` (`builtin_meshgrid`) |

**Implication:** Phase 4 (Laplacian builder) is *not* blocked on new core
infra — `spsolve` and `spdiags` already exist. Phase 2 (contour overlay)
can assume `hold on` semantics work end-to-end.

---

## Open decisions — resolve in Phase 0 before coding

The user (Mike) should confirm these before a follow-up agent begins Phase 1:

1. **Scope of this round.** All five requests, or only the three High-priority
   items (#1 vector calculus, #2 quiver/streamplot, #3 contour)?
2. **Commit cadence.** One commit per phase (recommended), or one bundled
   commit at the end?
3. **`rustlab_em` lesson updates.** After each phase lands, the corresponding
   request file in `rustlab_em/dev/rustlab/requests/` should be marked
   `Status: Landed` and the lesson scripts updated to use the new builtin.
   Is this in-scope here, or left to Mike to do in the `rustlab_em` repo?
4. **Node-ordering convention for Phase 4.** Request #4 specifies row-major
   `V(i, j) → (i-1)*nx + j`. Confirm this matches what any existing rustlab
   helpers expect; if rustlab uses column-major elsewhere, flag the
   inconsistency to Mike before coding.
5. **Animation (Phase 5) GIF path.** Request #5 has Option A (Plotly HTML,
   simpler) and Option B (GIF, needs `gif` crate). Default to Option A only
   unless Mike asks for GIF.

Until these are answered, **do not start Phase 1.**

---

## Mandatory workflow rules (from Mike's feedback memory)

Every phase below must comply with these:

1. **Plan-first.** Do not start coding without an agreed plan. This document
   is the plan for the overall effort, but each phase may need a brief
   sub-plan agreed with Mike before execution.
2. **Test with features.** Add tests alongside each new builtin — interior
   stencil correctness, boundary behaviour, error paths.
3. **No commit without approval.** `git add` freely, but do NOT `git commit`
   or `git push` without Mike's explicit go-ahead. Never force push.
4. **No Co-Authored-By line** in commit messages.
5. **Update `AGENTS.md`** in the same change as the feature — builtin tables,
   grammar notes, crate details. See `AGENTS.md:225–236`.
6. **REPL `HelpEntry` + category** for every new builtin — add to
   `crates/rustlab-cli/src/commands/repl.rs` `HelpEntry { name, brief,
   detail }` and to the appropriate category slice in `print_help_list()`.
   See `AGENTS.md:192–193, 875`.

All six are non-negotiable. Violations have burned Mike before.

---

## Phase 0 — Alignment & verification

**Goal:** answer the open decisions above and do any remaining prerequisite
checks.

Tasks:
- Confirm scope (all five vs. top three) with Mike.
- Confirm commit cadence and `rustlab_em`-side updates.
- Confirm node-ordering convention for Phase 4.
- Re-read the five request files end-to-end — they are short and
  self-contained.
- Skim `AGENTS.md` (especially §192, §225, §420, §460, §875) for current
  workflow conventions.

**Exit criteria:** open decisions resolved; no code written yet.

---

## Phase 1 — Vector-calculus operators (request #1)

**Reference:** `../rustlab_em/dev/rustlab/requests/vector-calculus-operators.md`

**Why first:** smallest blast radius (numerical only, no plotting), and is a
dependency of meaningful demos in later phases. Unblocks `rustlab_em` Lessons
01, 02, 03, 07, 08.

**Surface area (this phase: 2D only):**
- `[Fx, Fy] = gradient(F, dx, dy)` and `gradient(F)` (default dx=dy=1)
- `D = divergence(Fx, Fy, dx, dy)`
- `Cz = curl(Fx, Fy, dx, dy)` — returns z-component scalar in 2D

**3D variants deferred to Phase 1.5** — `gradient3`, `divergence3`,
`curl3` require a rank-3 `Value` type that does not yet exist. Adding that
is scoped separately in `dev/plans/tensor3.md`. Ship Phase 1.5 after the
tensor3 plan's Phase 7 lands.

**Semantics (from request):**
- Interior: 2nd-order central differences.
- Boundary: 2nd-order one-sided. Output same shape as input. NumPy
  convention.
- Grid: rows index `y`, columns index `x`. `F(i, j) ↔ (x=(j-1)*dx,
  y=(i-1)*dy)`.
- Complex inputs supported.

**Implementation sketch:**
- New module `crates/rustlab-dsp/src/vector_calc.rs` holding the pure
  numerical kernels (generic over real/complex).
- Expose from `rustlab-dsp/src/lib.rs`.
- Register script builtins in `crates/rustlab-script/src/eval/builtins.rs`
  alongside the other DSP primitives (~line 126–250 area).
- REPL `HelpEntry` + category (maths/numerics) per workflow rule 6.
- Tests in `crates/rustlab-dsp/src/tests.rs` (or a sibling test module):
  - `gradient` of `x^2 + y^2` → `(2x, 2y)`, verify interior + boundary.
  - `divergence` of `(x, y)` → `2` everywhere.
  - `curl` of `(-y, x)` → `2` everywhere.
  - Complex input: `gradient(exp(i*x))` → `i*exp(i*x)` (modulo discretisation).
- Update `AGENTS.md` builtin table.

**Exit criteria:** `cargo test -p rustlab-dsp -p rustlab-script` passes;
`rustlab` REPL `help gradient` shows an entry; `AGENTS.md` updated; Mike
approves before commit.

---

## Phase 2 — Contour plots (request #3)

**Reference:** `../rustlab_em/dev/rustlab/requests/contour-plots.md`

**Why before quiver:** contour is the smaller plotting surface, and
`contourf` exercises the polygon-fill path that `quiver` arrowheads will
reuse. Also unblocks the "contour overlay on imagesc" pattern that many
later phases assume works.

**Surface area:**
- `contour(X, Y, Z)` and variants with `nlevels`, explicit `levels`, title,
  `Z`-only form.
- `contourf(X, Y, Z)` and same variants.
- Both must honour `hold on` to overlay on `imagesc` / `quiver`.

**Semantics:**
- Marching squares for each level.
- Line contours default to black; filled default to colormap.
- Auto level-placement picks round-number levels (Octave-compatible).
- Label placement is v2 — do not ship in first cut.

**Backends:**
- Notebook (Plotly): `plotly.graph_objects.Contour` direct mapping.
- SVG/PNG (plotters): marching-squares line segments; for `contourf`, fill
  polygons between adjacent level sets.
- Viewer (egui): flat fallback or no-op initially — document as limitation.

**Files likely touched:**
- `crates/rustlab-plot/src/figure.rs` — add `Trace::Contour { ... }` variant.
- `crates/rustlab-plot/src/html.rs` — Plotly emission.
- `crates/rustlab-plot/src/file.rs` — SVG/PNG emission (marching squares).
- `crates/rustlab-plot/src/ascii.rs` — terminal fallback if trivial.
- `crates/rustlab-script/src/eval/builtins.rs` — `contour` + `contourf`
  registration + `HelpEntry`.
- `AGENTS.md` — builtin table + plot-section notes.

**Tests:**
- Contour of `x^2 + y^2` → concentric circles (verify level geometry in
  emitted SVG).
- `hold on; imagesc(...); contour(...); hold off` produces a figure with
  both traces.

**Exit criteria:** all three High-priority request demos from `rustlab_em`
Lessons 03 / 04 / 08 renderable; tests pass; help entries present; AGENTS
updated; Mike approves.

---

## Phase 3 — Quiver and streamplot (request #2)

**Reference:** `../rustlab_em/dev/rustlab/requests/quiver-and-streamplot.md`

**Why after contour:** relies on `hold on` overlay (verified in Phase 2)
and on Phase 1's gradient (for `streamplot` on `-∇V` fields in lesson
examples).

**Surface area:**
- `quiver(X, Y, U, V)` + optional scale, title, `quiver(U, V)` shortcut.
- `streamplot(X, Y, U, V)` + optional density, title, custom seeds matrix.

**Semantics:**
- Quiver auto-scale: longest arrow ≤ nearest-neighbour cell distance
  (Octave convention).
- NaN in U or V → skip that cell.
- Optional `quiver(..., C)` with scalar colour field — **defer to v2**
  unless trivial.
- Streamplot: RK4 forward + backward from seed grid, clip at domain bounds,
  arrowhead at midpoint.

**Implementation notes:**
- Plotly has no native quiver — emit many small line+triangle scatter
  traces (matches `plotly.figure_factory.create_quiver`).
- Plotters: draw line + small triangular polygon directly.
- Seed grid for streamplot: default density ≈ 1 seed per unit area, like
  Octave.
- For integration: implement RK4 inline (small, 20-ish lines).

**Files likely touched:**
- `crates/rustlab-plot/src/figure.rs` — `Trace::Quiver { ... }`,
  `Trace::Streamline { ... }`.
- `crates/rustlab-plot/src/{html,file,ascii}.rs` — backend emitters.
- `crates/rustlab-script/src/eval/builtins.rs` — two new builtins.
- `AGENTS.md` — builtin table.

**Tests:**
- Uniform field `(1, 0)` → horizontal arrows / streamlines.
- Vortex `(-y, x)` → circular streamlines.
- NaN entries skipped.

**Exit criteria:** at minimum `rustlab_em` Lessons 01/02/03/04/05 can render
their canonical vector-field figures; tests pass; help entries present;
AGENTS updated; Mike approves.

---

## Phase 4 — Laplacian stencil builder (request #4)

**Reference:** `../rustlab_em/dev/rustlab/requests/laplacian-stencil-builder.md`

**Prerequisite status:** `spsolve` and `spdiags` confirmed present
(verified 2026-04-22). No core-infra detour required.

**Surface area (first cut):**
- `L = laplacian_2d(nx, ny, dx, dy)` — Dirichlet BC only.
- `L = laplacian_2d(nx, ny)` — `dx = dy = 1` default.
- Sugar: `k = ij2k(i, j, nx)`, `[i, j] = k2ij(k, nx)`.
- Document the reshape convention: `V_grid = reshape(V_flat, ny, nx)`,
  `V_flat = V_grid(:)`.

**Deferred to follow-up:**
- Neumann and periodic BC (request spec §Boundary conditions).
- `laplacian_1d`, `laplacian_3d`.

**Semantics (from request):**
- Node ordering: row-major lexicographic. `V(i, j) → (i-1)*nx + j`.
  **Confirm this matches any existing rustlab helper in Phase 0** —
  inconsistent ordering is the #1 bug source.
- Sign: `L` approximates `+∇²` (Poisson: `V = spsolve(L, -rho/eps0)`).
- Returns sparse matrix compatible with `spsolve`.

**Files likely touched:**
- `crates/rustlab-script/src/eval/builtins.rs` — `laplacian_2d`, `ij2k`,
  `k2ij` registration (leverage existing `spdiags` under the hood, or build
  the COO triples directly).
- `AGENTS.md` — builtin table + a note on the row-major convention.

**Tests:**
- Construct `L`, solve Poisson with known analytic solution (e.g. source
  term producing a Gaussian potential), verify `||Lx - b||` small.
- `ij2k` / `k2ij` round-trip for random `(i, j)` on a grid.

**Exit criteria:** `rustlab_em` Lesson 04 Poisson demo runs via `laplacian_2d`
+ `spsolve`; tests pass; help entries present; AGENTS updated; Mike
approves.

---

## Phase 5 — Animation export (request #5)

**Reference:** `../rustlab_em/dev/rustlab/requests/animation-export.md`

**Lowest priority** — a per-frame SVG loop workaround already exists and
is documented in `rustlab_em` lessons 08–09.

**Surface area (Option A only unless Mike requests Option B):**
- `frame()` — snapshot current figure into internal frame buffer, clear
  figure for next iteration (like `hold off` + capture).
- `saveanim(path, fps=30)` — flush buffer to Plotly animation HTML.
- `figure()` — must reset the frame buffer (no state leaks across figures).

**Implementation notes:**
- Plotly `frames` block in the emitted JSON is the data model.
- Extend the figure state in `crates/rustlab-plot/src/figure.rs` with a
  `frames: Vec<FigureSnapshot>` field.
- `saveanim` is close to an extended variant of the existing HTML writer.

**Option B (GIF) — deferred** unless Mike opts in. Needs the `gif` crate
dependency; otherwise same data flow.

**Tests:**
- `figure(); frame(); frame(); saveanim(tmp)` produces valid HTML with
  two frames in its `frames` array.
- `figure()` called mid-sequence clears the buffer.

**Exit criteria:** a short time-loop test from `rustlab_em` Lesson 08 renders
as an animated HTML page; tests pass; help entries present; AGENTS
updated; Mike approves.

---

## Phase 6 — Wrap-up

- `cargo test --workspace` passes.
- `AGENTS.md` reflects all new builtins, conventions, and help entries.
- `PLAN.md` top-level updated if it tracks roadmap.
- Bump workspace version in `Cargo.toml:15` (currently `0.1.9` →
  `0.1.10` or `0.2.0` depending on scope landed). Confirm semver choice
  with Mike.
- For each request that landed, edit its file in
  `../rustlab_em/dev/rustlab/requests/` to change
  `**Status**: Proposed` → `**Status**: Landed` and (separately, in the
  `rustlab_em` repo) update the lesson scripts to use the new builtin instead
  of the workaround.

---

## Quick-reference: where things live

| Thing | Path |
|-------|------|
| Script builtins registration | `crates/rustlab-script/src/eval/builtins.rs` (~line 126 for plot funcs, ~line 249 for sparse) |
| Plot figure/traces | `crates/rustlab-plot/src/figure.rs` |
| Plotly/HTML backend | `crates/rustlab-plot/src/html.rs` |
| SVG/PNG backend | `crates/rustlab-plot/src/file.rs` |
| ASCII backend | `crates/rustlab-plot/src/ascii.rs` |
| Viewer backend | `crates/rustlab-plot/src/viewer_*.rs` |
| DSP numerics | `crates/rustlab-dsp/src/` |
| REPL help registry | `crates/rustlab-cli/src/commands/repl.rs` (`HelpEntry`, `print_help_list`) |
| Project-wide agent doc | `AGENTS.md` (read §192, §225, §420, §460, §875) |
| Version number | `Cargo.toml:15` |

---

## Handoff notes for the next agent

1. Start with Phase 0 — do **not** skip the alignment step even if it feels
   redundant. Mike's feedback memory explicitly says plan-first, and the
   five open decisions above are real.
2. Re-read each request file before starting the corresponding phase. This
   plan is a sequencer; the request files are the spec.
3. Workflow rules in the "Mandatory workflow rules" section are not
   suggestions — they come from Mike's feedback memory and have been the
   source of past friction.
4. If any phase's prerequisite verification turns up a surprise (e.g. node
   ordering inconsistency, missing sparse op), stop and flag to Mike
   rather than inventing a workaround.
5. When a phase is ready to commit: `git add` the relevant files (never
   `-A` / `.`), draft a commit message following repo convention
   (`git log` for style), and wait for explicit approval before
   `git commit`. Never `git push --force`.

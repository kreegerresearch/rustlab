# Development Plan: `Value::Tensor3` — Rank-3 Tensor Support

**Status:** Complete (all 7 phases)
**Date opened:** 2026-04-22
**Date completed:** 2026-04-22
**Target version bump:** unknown (likely a minor: `0.1.9` → `0.2.0` given new `Value` variant)
**Driver:** `rustlab_em` Lesson 09 (FDTD) and the 3D variants of request #1 (`gradient3`, `divergence3`, `curl3`). See sibling plan `dev/plans/rustlab_em_requests.md`.
**Total estimate:** 8–12 focused days, dominated by indexing (Step 3).

---

## Purpose

Add a first-class rank-3 tensor value type so that 3D scalar/vector fields can
be passed around as a single script value. Today `Value` has only `Scalar`,
`Complex`, `Vector`, `Matrix` (plus sparse and non-numeric variants). 3D
numerical builtins have no clean way to receive or return a volumetric field;
workarounds (list-of-matrices, flat vectors + explicit shape) are ugly and
fragile.

This is the foundation that unlocks:

- `gradient3`, `divergence3`, `curl3` on uniform 3D grids
- FDTD field volumes `Ex(:, :, :, t)` (Lesson 09)
- Animation frame buffers (`saveanim` input is a rank-3 array of frames)
- Any future volumetric imaging / waveguide / mode-decomposition work

Because the touchpoints span the entire `value.rs` and `builtins.rs` surface,
this deserves its own plan and commit rather than riding inside a feature
phase.

---

## Prerequisites — verified 2026-04-22

| Item | Status | Notes |
|------|--------|-------|
| `ndarray::Array3` available | ✅ | `ndarray` is already a direct dep of `rustlab-core` (via `Array1`/`Array2`) |
| `Array3<C64>` | ✅ | `num_complex::Complex<f64>` works with `ndarray::Array3` out of the box |
| No existing `Array3` / `Tensor3` references anywhere in the tree | ✅ | `grep -r "Array3\|Tensor3"` returns nothing — greenfield |
| `rustlab-core::types.rs` is the canonical type-alias home | ✅ | `CVector = Array1<C64>` and `CMatrix = Array2<C64>` already live there |
| Existing benchmarks in `perf/` for pre/post-landing comparison | ✅ | `perf/bench_*.rlab` + `perf/run_perf.sh` |

---

## Counted blast radius (as of 2026-04-22)

These are exact grep counts so the follow-up agent doesn't need to re-measure:

| File | `Value::Matrix/Vector/Scalar/Complex` refs | Role |
|------|--------------------------------------------|------|
| `crates/rustlab-script/src/eval/builtins.rs` | **402** | Per-builtin type dispatch |
| `crates/rustlab-script/src/tests.rs` | 171 | Existing tests — unaffected; new tests added alongside |
| `crates/rustlab-script/src/eval/value.rs` | 152 | The enum + intrinsic methods (binop, index, display, etc.) |
| `crates/rustlab-script/src/eval/mod.rs` | 88 | Expression evaluation, indexing dispatch |
| `crates/rustlab-cli/src/commands/repl.rs` | 10 | Display/help |
| `crates/rustlab-script/src/eval/toml_io.rs` | 7 | `save`/`load` serialization |
| `rustlab-plot` / `rustlab-viewer` / `rustlab-notebook` | 6 | Minor; most plotting works on `&CMatrix` directly |

**Implication:** most builtins already have a final `_ => Err(...)` wildcard
arm, so adding the `Value::Tensor3` variant is *additive* — existing builtins
keep erroring politely on Tensor3 until they explicitly opt in. This is the
key reason the blast radius is manageable despite the large ref count.

---

## Mandatory workflow rules

Same as the parent plan (`dev/plans/rustlab_em_requests.md`). Summarised here so
this doc stands alone:

1. **Plan-first.** Phase 0 below must complete before any code is written.
2. **Test with features.** Every new variant/builtin has tests in the same
   change.
3. **No commit without approval.** `git add` freely, but wait for explicit
   go-ahead before `git commit`. Never force push.
4. **No Co-Authored-By line** in commit messages.
5. **Update `AGENTS.md`** in the same change — the new type deserves its own
   section in the "Types" overview, plus the builtin table rows.
6. **REPL `HelpEntry` + category** for every new builtin.

Full workflow rule detail is in `AGENTS.md` §192, §225, §420, §460, §875.

---

## Open decisions — resolve in Phase 0

The follow-up agent **must** get explicit answers from Mike on these before
writing code. Many of them are one-word answers but each commits us to
non-trivial semantics.

1. **Indexing return-type rule.** When the user writes `A(:, :, k)`, does the
   result come back as `Matrix` (Octave-style — drop trailing singleton
   dimension) or as `Tensor3` with shape `(m, n, 1)` (NumPy-style)?
   **Recommended: Matrix.** It makes `imagesc(A(:, :, k))` work trivially.
2. **Broadcasting rule.** For `Matrix + Tensor3`, is the matrix broadcast
   along the 3rd dimension (NumPy) or is it an error requiring explicit
   `repmat`/`cat` (Octave)? **Recommended: error initially**, broadcasting
   as a follow-up once a lesson actually needs it.
3. **Matrix-multiply semantics.** `Tensor3 * Matrix` and `Tensor3 * Tensor3`
   with `*` operator — error (Octave's position; introduces `pagemtimes` for
   the batched case) or define it? **Recommended: error.**
4. **`reshape` variadic extension.** `reshape(A, m, n)` is arity-3 today
   (`check_args("reshape", &args, 3)` at `builtins.rs:2922`). Promoting to
   `reshape(A, m, n, p, ...)` — is that OK as an additive API change?
   **Recommended: yes**, switch to `check_args_range("reshape", &args, 3,
   N)` with whatever `N` covers up to rank-4 for future-proofing.
5. **Storage order.** ndarray defaults to C-order (row-major). Octave is
   column-major. `CMatrix = Array2<C64>` is already row-major in the
   Rust-side storage but rustlab presents column-major-indexed semantics
   (`reshape` is column-major — `builtins.rs:2920,2928,2954`). Keep the same
   presentation for `Tensor3`: index with `(i, j, k)` 1-based, `reshape`
   column-major. Confirm.
6. **Sparse rank-3?** No — out of scope. Sparse stays 1D/2D.
7. **Display truncation.** For large Tensor3 values in the REPL, what's the
   abbreviation rule? **Recommended:** show the first slice
   `A(:, :, 1) = ...` in full up to existing matrix truncation rules, then
   a `(p-1 more slices omitted)` footer.

---

## Phase 0 — Alignment & verification

Before writing code:

- Get explicit Mike answers on the 7 open decisions.
- Run `perf/run_perf.sh` on current `main` and save the output as the
  baseline. Re-run after Step 7 and diff; require <2% regression on all
  benchmarks or flag to Mike before merging.
- Skim `crates/rustlab-script/src/eval/value.rs` end-to-end — the file is
  1837 lines. All intrinsic methods (`negate`, `type_name`, `transpose`,
  `index`, `binop`, `format_display`, `Display` impl) need updating.

**Exit criteria:** decisions resolved, baseline benchmarks captured, value.rs
mentally mapped.

---

## Phase 1 — Type + trivial intrinsics

**Target:** the Tensor3 type exists, prints, round-trips, and errors politely
everywhere else.

### Files touched

- `crates/rustlab-core/src/types.rs` — add `pub type CTensor3 = Array3<C64>;`
- `crates/rustlab-script/src/eval/value.rs`:
  - Import `CTensor3` and `Array3`.
  - Add `Value::Tensor3(CTensor3)` variant.
  - Extend `type_name()` with `"tensor3"`.
  - Extend `negate()` with the obvious `Array3` arm.
  - Add error arms to `to_cvector`, `to_scalar`, `to_usize`, `to_str`,
    `to_string_array` (just error with `type_name()`).
  - Add `Display` arm — first-cut: print as
    `tensor3 [m×n×p, complex]` followed by `A(:, :, 1) =` and the first
    slice using the existing matrix formatter. Respect `NumberFormat`.
  - Add `format_display` arm mirroring the above.
- `crates/rustlab-script/src/eval/mod.rs` — grep for `Value::Matrix` pattern
  matches with no wildcard and either add a `Value::Tensor3` arm or confirm
  there's a wildcard that catches it.

### Tests

- Round-trip: build via `zeros3(2, 3, 4)` (constructor from Phase 2), print,
  parse display string manually for sanity.
- `type_name` returns `"tensor3"`.
- `negate` works element-wise.

### Exit criteria

`cargo build --workspace` clean. `cargo test --workspace` still green. The
new variant is inert (no way to construct from script yet — constructors
come in Phase 2).

---

## Phase 2 — Constructors and size/shape builtins

**Target:** users can build a Tensor3 from the script layer.

### New builtins (register in `builtins.rs`)

- `zeros3(m, n, p)` — `Tensor3` of zeros.
- `ones3(m, n, p)` — `Tensor3` of ones.
- `rand3(m, n, p)` — uniform [0,1).
- `randn3(m, n, p)` — standard normal.
- Extend `size(A)` — if `A` is `Tensor3`, return `[m, n, p]` row-vector.
  Current code path at ~line 1064 in `eval/mod.rs` and wherever `size` is
  implemented in `builtins.rs`.
- Extend `size(A, dim)` — support `dim=3`.
- Extend `numel(A)` — `m*n*p`.
- Extend `ndims(A)` — returns `3` for `Tensor3`.

### Tests

- `size(zeros3(2,3,4))` → `[2, 3, 4]`.
- `numel(ones3(2,3,4))` → `24`.
- `ndims(zeros3(2,3,4))` → `3`.

### Exit criteria

Script can now create a Tensor3 and ask about its shape. Still no
arithmetic, no indexing.

---

## Phase 3 — Indexing (the hard part)

**Target:** `A(i, j, k)`, `A(:, :, k)` slicing, and assignment work
correctly.

### Files touched

- `crates/rustlab-script/src/eval/value.rs`:
  - Add `fn index_3d(self, i: Value, j: Value, k: Value) -> Result<Value, String>`
    mirroring the structure of `index_2d` at line 512.
  - Extend `pub fn index(self, indices: Vec<Value>)` at line 291 to
    dispatch on `indices.len() == 3` to `index_3d`.
- `crates/rustlab-script/src/eval/mod.rs`:
  - Find the indexing-assignment paths (grep for `Value::Matrix` in
    assignment context, ~line 525, 693, 909) and add Tensor3 branches.
  - Assignment `A(:, :, k) = M` where `M` is a `Matrix` — write the slice.
  - Assignment `A(i, j, k) = s` — write a scalar.

### Semantics (all 1-based)

- `A(i, j, k)` → `Scalar` (or `Complex` if complex)
- `A(:, :, k)` → `Matrix` of shape `(m, n)` — **the canonical idiom**
- `A(i, :, :)` → `Matrix` of shape `(n, p)`
- `A(:, j, :)` → `Matrix` of shape `(m, p)`
- `A(:, :, :)` → `Tensor3` unchanged (clone — maybe optimise later)
- `A(a:b, :, :)` → `Tensor3` with the row range
- `A(:, :, a:b)` → `Tensor3` with the page range
- Out-of-bounds → `Err` with a clear message

### Tests

- Extract every corner: `A(1,1,1)`, `A(m,n,p)`, `A(:, :, 1)`, `A(:, :, p)`.
- Assignment: build `A = zeros3(2, 2, 3)`, set `A(:, :, 2) = [[1,2],[3,4]]`,
  verify `A(1, 2, 2) == 2`.
- Range slices: `A(1:2, :, 1)` returns `(2, n)` matrix.
- Out-of-bounds errors.

### Exit criteria

`imagesc(A(:, :, k))` renders a 2D slice of a Tensor3 as a heatmap. This is
the single most important smoke test — it is how FDTD / animation code
will actually consume Tensor3 in practice.

---

## Phase 4 — Arithmetic in `binop`

**Target:** `+`, `-`, `.*`, `./`, `.^` work element-wise on Tensor3.

### Files touched

- `crates/rustlab-script/src/eval/value.rs:607+` — `binop` function.

### New match arms (following the resolved decisions from Phase 0)

- `Scalar ± Tensor3` and reverse → element-wise, returns `Tensor3`.
- `Complex ± Tensor3` and reverse → element-wise, returns `Tensor3`.
- `Tensor3 ± Tensor3` → element-wise, shape-matched, returns `Tensor3`.
  Shape mismatch is an `Err`.
- `Matrix ± Tensor3` → **error** (per Phase 0 decision #2), clear message
  suggesting `repmat` or explicit iteration.
- `*` / `\` / `/` with Tensor3 operands → **error** (per Phase 0 decision
  #3), message mentions Octave's `pagemtimes` as a future direction.
- `.^` element-wise power with scalar exponent → supported.
- `.^` element-wise power with Tensor3 exponent → supported (element-wise).

### Tests

- `A + B` where both are `zeros3(2,2,2)` → zero tensor.
- `A .* 2` scales every element.
- Shape-mismatch error path.
- Matrix + Tensor3 error path has a helpful message.

### Exit criteria

`cargo test --workspace` passes. `perf/run_perf.sh` still within 2% of
Phase 0 baseline (existing benchmarks don't use Tensor3, so this mostly
catches accidental regressions in unrelated paths).

---

## Phase 5 — Ergonomic builtins

**Target:** common reshape/cat/permute idioms work.

### New / extended builtins

- `reshape(A, m, n, p)` — variadic extension (confirm per Phase 0 decision
  #4). Column-major order (consistent with existing 2D `reshape`).
  Implementation: accept 3..=4 arity, switch on arity for rank. If Phase 0
  #4 says no, gate behind a new name `reshape3`.
- `permute(A, [order])` — new. `order` is a 3-element permutation of
  `[1, 2, 3]`. Returns a new `Tensor3` with axes reordered. Octave-standard.
- `squeeze(A)` — drop singleton dims. `Tensor3(m, 1, p)` → `Matrix(m, p)`,
  etc. Returns whatever lower-rank variant is appropriate.
- `cat(dim, A, B, ...)` — extend to `dim=3`. For `dim=3`, inputs may be
  matrices (all same `(m, n)`) stacked into a `Tensor3(m, n, p)`, or
  tensors concatenated along the page axis.

### Tests

- `reshape([1..24], 2, 3, 4)` — build a known tensor, verify
  `A(1, 1, 1) = 1`, `A(2, 3, 4) = 24` (column-major walk order).
- `permute(A, [2, 1, 3])` swaps first two axes.
- `squeeze(zeros3(3, 1, 4))` returns a `3×4` matrix.
- `cat(3, M1, M2)` where `M1`, `M2` are matrices → `Tensor3(m, n, 2)`.

### Exit criteria

Lesson 08–09 style code can build FDTD field volumes via `cat(3, ...)` or
`reshape`.

---

## Phase 6 — I/O and display polish

**Target:** `save`/`load` work, REPL display is usable on realistic volumes.

### Files touched

- `crates/rustlab-script/src/eval/toml_io.rs`:
  - Encoding choice: nested TOML arrays are deep and ugly at rank-3. Prefer
    a flat `data = [...]` array plus `shape = [m, n, p]` metadata. Document
    the format in `AGENTS.md` §I/O section.
  - Add `Value::Tensor3` encode/decode arms (mirrors the existing Matrix
    path at line 58).
- `crates/rustlab-script/src/eval/value.rs`:
  - Tighten `Display` truncation: cap total slices shown to ~3 (first, last,
    "N omitted" in the middle) for large tensors.
  - Honour `NumberFormat::{Short, Long, Hex, Commas}` per slice.

### Tests

- `save("tmp.toml", A); B = load("tmp.toml"); assert A == B`.
- Very large tensor (`zeros3(10, 10, 100)`) produces truncated display,
  not megabytes of output.

### Exit criteria

A user can persist and retrieve Tensor3 values via `save`/`load` with no
workaround.

---

## Phase 7 — Docs, help, AGENTS.md, examples

**Target:** discoverable.

- `AGENTS.md`:
  - New subsection under "Types" documenting `Value::Tensor3`, the indexing
    rules (especially the `A(:, :, k) → Matrix` rule), reshape column-major
    convention, and the matrix/tensor3 broadcast policy.
  - Builtin table rows for `zeros3`, `ones3`, `rand3`, `randn3`, `permute`,
    `squeeze`, `cat` (dim=3 note), `reshape` (variadic note).
- `crates/rustlab-cli/src/commands/repl.rs`:
  - `HelpEntry { name, brief, detail }` for each new builtin.
  - Add each name to the appropriate category slice in `print_help_list()`.
- `examples/tensor3/` directory with a short `.rlab` script exercising build,
  slice, arithmetic, save/load.

---

## Performance expectations

**Summary: near-zero impact on existing operations.**

1. **`sizeof(Value)`.** Rust enums are `max(sizeof(variant)) + tag`.
   `Array3<C64>` has the same layout as `Array2<C64>` (three `usize` shape
   triple + `Vec<C64>` ptr vs. two `usize` + `Vec<C64>` ptr — an 8-byte
   delta in the variant body, but enum size is determined by the largest
   existing variant which is one of the complex structs like `StateSpace`
   holding four `CMatrix`). Expect **no change** to `sizeof(Value)`.
2. **Pattern-match jump tables.** One new arm per `match` site → one extra
   jump-table entry. Not measurable.
3. **Inner numerical loops.** All hot paths (FFT, matmul, elementwise) run
   on `Array1`/`Array2` buffers after a one-time `Value::Matrix(m)` peel.
   Adding Tensor3 doesn't change a single line of inner-loop code.
4. **Compile time.** 402 pattern-match sites in `builtins.rs`. Most have
   wildcard `_ => Err(...)` arms and need zero changes. The follow-up agent
   should grep `Value::Matrix\s*(\w+)\s*=>` without a trailing `_ =>` and
   audit — expected count is small (<30 sites).

**Verification protocol:**
- Capture baseline via `perf/run_perf.sh` in Phase 0.
- Re-run at the end of Phase 4 (arithmetic) and Phase 7 (final).
- Require max regression <2% across `bench_builtins.rlab`, `bench_convolve.rlab`,
  `bench_fft.rlab`, `bench_linalg.rlab`. Larger deltas → stop and flag to Mike.

---

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Exhaustive-match sites without wildcards break compilation | Medium | Step 1 builds and fixes all compile errors before proceeding |
| Display format for large tensors produces screen floods | High (default would) | Truncation rules in Phase 6 are non-negotiable; test on `zeros3(100, 100, 100)` |
| Indexing edge cases leak scalars where matrices are expected (or vice versa) | High | Phase 3 tests enumerate all 7 slice shapes explicitly |
| Performance regression on unrelated benchmarks | Low | Phase 0 baseline + post-Phase-4/7 diff |
| Scope creep into 4D / N-D | Medium | Explicit non-goal: rustlab is 1D/2D/3D only. Reject 4D requests; reshape cap is arity-4 for vector reinterpretation only |
| Reshape arity change breaks user scripts | Low | Only extends, doesn't restrict — `reshape(A, m, n)` still works |

---

## Explicit non-goals

- **Rank-4 and higher.** Keep `Value` enum compact. If a future use case
  genuinely needs it, open a new plan.
- **Sparse rank-3.** Out of scope.
- **Broadcasting.** Start with strict shape-match semantics. Broadcasting can
  be added later without breaking strict-shape users.
- **GPU / SIMD paths specific to Tensor3.** Ndarray's existing SIMD
  machinery is sufficient for initial release.
- **`pagemtimes`-style batched matrix multiply.** Nice-to-have; gate behind
  a separate request when a lesson demands it.
- **Parser literal syntax for rank-3.** No `[[[...]]]` or similar. Users
  build tensors via `zeros3` / `cat(3, ...)` / `reshape`. Matches Octave.

---

## Handoff notes for the next agent

1. **Start with Phase 0.** The 7 open decisions above have real downstream
   consequences. Do not guess; get answers.
2. **Read `dev/plans/rustlab_em_requests.md` first.** This plan is a dependency
   of the rustlab_em Phase 1.5 (3D vector calculus) — context from the parent
   plan may shape priorities.
3. **Benchmark discipline.** Capture baseline before writing code, diff at
   the end of Phase 4 and Phase 7. Do not skip this step — it's cheap and
   it's the only defence against silent regressions in the 402-match-arm
   dispatch layer.
4. **Don't over-engineer `Display`.** First cut can be ugly. Phase 6 polish
   catches it.
5. **Stop at surprises.** If a pattern-match site doesn't have a wildcard
   and the right answer for Tensor3 is non-obvious, stop and flag to Mike
   rather than inventing semantics. The `binop` file is full of nuanced
   decisions.
6. **Commit cadence.** Default to one commit per phase (seven commits
   total). Each phase is independently useful and reviewable; a single
   mega-commit for the whole plan is discouraged. Confirm with Mike before
   starting.
7. **Never `git push --force`.** Standard repo rule; see global `CLAUDE.md`.
8. **After Phase 7 lands**, update `dev/plans/rustlab_em_requests.md` Phase 1
   to re-enable the deferred 3D operators (`gradient3`, `divergence3`,
   `curl3`), and mark this plan `Status: Complete`.

---

## Quick-reference: where things live

| Thing | Path |
|-------|------|
| Core type aliases (`CVector`, `CMatrix`) | `crates/rustlab-core/src/types.rs:7,9` |
| `Value` enum | `crates/rustlab-script/src/eval/value.rs:36–99` |
| `Value::binop` | `crates/rustlab-script/src/eval/value.rs:607+` (~600 lines) |
| `Value::index` / `index_1d` / `index_2d` | `value.rs:291,304,512` |
| `Value::Display` impl | `value.rs:1372` |
| `Value::format_display` (NumberFormat-aware) | `value.rs:1620` |
| TOML I/O | `crates/rustlab-script/src/eval/toml_io.rs` |
| Builtins registration | `crates/rustlab-script/src/eval/builtins.rs:~line 125–260` |
| `reshape` implementation | `builtins.rs:2920` |
| REPL help registry | `crates/rustlab-cli/src/commands/repl.rs` (`HelpEntry`, `print_help_list`) |
| Benchmarks | `perf/run_perf.sh` + `perf/bench_*.rlab` |
| Parent plan (rustlab_em requests) | `dev/plans/rustlab_em_requests.md` |

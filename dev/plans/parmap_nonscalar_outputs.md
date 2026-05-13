# Implementation Plan — `parmap` vector/matrix outputs

Extend `parmap(f, xs)` so the lambda may return a vector or a matrix, not just a scalar. Today the call hard-errors with `"parmap: lambda must return a scalar (got <type> at index N); vector/matrix return values are not yet supported"` (`crates/rustlab-script/src/eval/parmap.rs:208–229`).

Triggering request: `rustlab_llm/AGENTS.md:362–383` ("parmap should accept vector/matrix-returning lambdas") and `rustlab_llm/PLAN.md` Phase-8 follow-ups. The four named blocked sites are per-row attention softmax (lessons 08/13/14), per-position FFN (lesson 11), multi-head attention (lesson 09), and batched autoregressive sampling (lesson 21).

**Plan status:** Draft, awaiting approval. No implementation until user signals "go."

## Status snapshot

| # | Phase | Status | Risk | Win |
|---|---|---|---|---|
| 1 | Vector-output stacking → `N × d` Matrix | pending | low | unblocks softmax/FFN/sampling — 3 of 4 named sites |
| 2 | Matrix-output stacking → `Tensor3` | pending | low-medium | unblocks multi-head attention — last named site |
| 3 | Tests, docs, REPL help, AGENTS row, gallery | pending | low | meets the six mandatory workflow rules |

Phases 1 and 2 are independent in code (different match arms in `pack_results`) but should ship together so the user-facing rule "all outputs same shape" reads cleanly. Phase 3 is the close-out.

## Decisions to lock before implementing

These are the design questions the user should sign off on. Defaults proposed in **bold**; alternatives noted.

1. **Vector-output layout.** **Per-call index is the row axis** — `parmap(f, 1:N)` where each `f(i)` returns a row of length `d` yields an `N × d` Matrix. This matches `arrayfun`'s existing rule (`mod.rs:1647–1675`) so the only difference between `arrayfun` and `parmap` stays "parallel vs sequential." No knob for column-stacking; users can `'` if they want a column layout.

2. **Matrix-output layout.** **Per-call index is the *trailing* (pages) axis** — `parmap(f, 1:N)` where each `f(i)` returns an `m × n` matrix yields a `Tensor3` with shape `(m, n, N)`. Users retrieve the `i`-th per-call matrix as `result(:, :, i)`, which matches the existing Tensor3 docstring (`eval/value.rs:93–94`: "`A(:, :, k)` returns a Matrix"). The user's request phrased the shape as `n_heads × T × d_v` (per-call axis leading), but that would force `result(i, :, :)` slicing which doesn't currently drop to a Matrix the same way and is asymmetric with `arrayfun`. **Open to flipping** if you'd rather optimize for the request's literal shape over the Tensor3 indexing convention.

3. **Mixed-shape outputs.** **Hard error**, message identifies the first index that diverged: `"parmap: trial 3 returned vector of length 5 but trial 1 returned scalar; all trials must return the same shape"`. Matches `arrayfun`'s behavior (`mod.rs:1654–1660`).

4. **Empty input.** Already correct (`pack_results` returns empty `Vector`). Keep that; do *not* try to infer a "shape" for an empty parmap — user has no expectation here.

5. **Length-1 input with a matrix-output lambda.** Returns a `Tensor3` of shape `(m, n, 1)`, not a plain Matrix. Consistency beats convenience: a one-element parmap should produce the same shape as a 1000-element parmap.

6. **Struct / Tuple / String / FirState etc. returns.** Still error. Same message shape as today, just without the "not yet supported" tail. The pure-lambda contract already forbids `FirState` mutations.

7. **API name.** **No rename.** `parmap(f, xs)` stays. No new builtin for matrix output — same builtin, smarter packer.

## Why this design over alternatives

- **Reuse `pack_results`.** The packing decision is independent of the worker loop in `LocalRayonBackend::run`. A single function deciding row-stack vs page-stack from the *first* result keeps the trait surface untouched (good for the deferred Phase 6 cluster backend) and matches `arrayfun_inner`'s structure (`mod.rs:1626–1681`).
- **No cell array.** rustlab `Value` has no `Cell` variant. Adding one for parmap alone is scope creep; the matrix-form output covers all four named use cases.
- **Pages-trailing for Tensor3** keeps `result(:, :, i)` as the slice idiom users already know from `cat(3, ...)`. The flip is a one-line shape change if you call it differently in design Q2.

## Phase 1 — Vector-output stacking

**Status:** pending
**Goal:** `parmap(f, xs)` with vector-returning `f` returns an `N × d` Matrix.

**Scope:**
- Extend `pack_results` in `crates/rustlab-script/src/eval/parmap.rs` to peek at the first result and branch:
  - `Scalar`/`Complex`/`Bool` → current behavior (Vector of complex).
  - `Vector(v)` → stack into `Matrix` of shape `(N, v.len())`, error on per-element length mismatch with a message naming the divergent index.
  - Anything else → fall through to existing error.
- Reuse `ndarray::Array2::from_shape_vec` exactly as `eval_arrayfun_inner` does (`mod.rs:1672`). No new helper functions — keep the two implementations parallel.

**Tests** (in `crates/rustlab-script/src/eval/parmap.rs#tests` and `crates/rustlab-script/src/tests.rs`):
- Unit: `pack_results` accepts a `vec![Vector([1,2,3]), Vector([4,5,6])]` and returns a `(2, 3)` Matrix; bit-identical to what `arrayfun` would produce on the same elements.
- Unit: `pack_results` errors on `vec![Vector(len=3), Vector(len=4)]` with a message that names "trial 2" and the lengths.
- Integration: `parmap(@(t) softmax(S(t, :)), 1:T)` where `S` is a `T × T` matrix — assert the result is a `T × T` Matrix and is bit-identical to a sequential `for` loop that row-writes `A(t) = softmax(S(t, :))`.
- Integration: `parmap` with vector output + `seed(N)` → bit-identical across runs (regression on the Phase-3 RNG contract; vector output must not affect determinism).

**Files affected:**
- `crates/rustlab-script/src/eval/parmap.rs` (+~30 LoC core, +~30 LoC tests)
- `crates/rustlab-script/src/tests.rs` (+~40 LoC for the integration tests)

**Risk:** low. Mirrors an existing, tested code path in `arrayfun`.

**Estimated size:** half-session.

## Phase 2 — Matrix-output stacking

**Status:** pending
**Goal:** `parmap(f, xs)` with matrix-returning `f` returns a Tensor3 of shape `(m, n, N)`.

**Scope:**
- Add a `Matrix(m)` arm to the `pack_results` peek. Build a `CTensor3` of shape `(rows, cols, N)` by inserting each result as page `i`. Error on per-element shape mismatch (rows or cols), message names the divergent index.
- `arrayfun` does **not** support matrix output today (`mod.rs:1676–1680` errors on `unsupported type`). This phase intentionally puts `parmap` ahead of `arrayfun` — the four use cases live in `parmap`, not `arrayfun`. If matching is desired later, a thin follow-up plan can extend `arrayfun` once `pack_results`-style matrix packing is shaken out here.

**Tests:**
- Unit: `pack_results` accepts `vec![Matrix(2×3), Matrix(2×3), Matrix(2×3)]` and returns a Tensor3 of shape `(2, 3, 3)` with page `k` equal to the k-th input Matrix.
- Unit: `pack_results` errors on `vec![Matrix(2×3), Matrix(2×4)]` with a message naming "trial 2" and both shapes.
- Integration: per-head attention sketch — `parmap(@(h) Q*K' * V_h, 1:n_heads)` produces a Tensor3 whose `(:,:,h)` page matches a sequential reference.
- Integration: `result(:, :, i)` round-trip — extract page `i` and assert it equals `f(xs[i])` exactly.

**Files affected:**
- `crates/rustlab-script/src/eval/parmap.rs` (+~30 LoC core, +~30 LoC tests)
- `crates/rustlab-script/src/tests.rs` (+~50 LoC integration)

**Risk:** low-medium. The Tensor3 page-stacking code is small but new — no equivalent path exists in `arrayfun`. The risk is in the design-Q2 layout choice; once locked, the code is mechanical.

**Estimated size:** half-session.

## Phase 3 — Tests, docs, REPL help, AGENTS row, gallery

**Status:** pending
**Goal:** the six mandatory workflow rules.

**Scope:**
- `AGENTS.md` row 963 (`parmap` entry): update to mention the new vector/matrix output shapes — `(N, d)` Matrix from vector return, `(m, n, N)` Tensor3 from matrix return. Keep the pure-lambda contract bullet unchanged.
- `docs/quickref.md` "Parallelism" section: extend the `parmap` row with the two new output shapes.
- `docs/functions.md`: same.
- `crates/rustlab-cli/src/commands/repl.rs:476–477`: extend the `parmap` `HelpEntry::detail` with the new "Output rules" subsection mirroring `arrayfun`'s wording (`repl.rs:475`). Re-use the wording style.
- Gallery: extend `examples/notebooks/parallel_montecarlo.md` (or add a sibling) with one concrete vector-output example (per-row softmax) and one matrix-output example (multi-head attention sketch). Keep them short — the existing gallery convention is "one screen of code, one figure."
- `rustlab_llm/AGENTS.md:362–383`: cross-repo note — mark the request as satisfied once shipped (this is in the sibling repo; not in scope for this plan to *edit*, but flag for the user to update there).
- Plan archival: move `dev/plans/parmap_nonscalar_outputs.md` → `dev/plans/closed/` with status updated and commit hashes filled in, per workflow rule 6.

**Files affected:**
- `AGENTS.md`
- `docs/quickref.md`, `docs/functions.md`
- `crates/rustlab-cli/src/commands/repl.rs`
- `examples/notebooks/parallel_montecarlo.md` (extend; or new sibling)

**Risk:** low.

**Estimated size:** ~80 LoC of docs + the gallery example. Quarter-session.

## Out of scope

- **Tensor3-input parmap** (`parmap(f, T3)`). Not requested; would need a "what does iteration mean for a 3-D input?" design pass.
- **Higher-rank output stacking** (each call returns a Tensor3 → 4-D result). rustlab has no Tensor4 type; would block on that.
- **Cell-array output.** Same reason as above — no `Value::Cell`.
- **`arrayfun` matrix-output parity.** Mentioned above; deferred until a use case asks.
- **Per-thread Evaluator caching.** Orthogonal optimisation called out in the original plan; not affected here.

## Closure conditions

1. Phase 1 and Phase 2 unit tests + integration tests pass on `main`.
2. Full workspace test run green (`cargo test --workspace`).
3. End-to-end smoke: rebuild `target/release/rustlab`, run a rustlab_llm-style snippet (e.g., `parmap(@(t) softmax(S(t, :)), 1:T)` against a known-good `S`) and confirm it produces the same matrix the sequential loop would.
4. AGENTS row, REPL `HelpEntry`, quickref, functions.md, and gallery example all updated.
5. Plan moved to `dev/plans/closed/` with phase commit hashes recorded.
6. Version bump (likely `0.3.2 → 0.3.3`) so rustlab_llm can pin.

## Pending items (post-approval)

- Confirm decision Q2 (matrix layout) — pages-trailing vs leading. Default in this plan is pages-trailing.
- Confirm whether `arrayfun` should also get matrix-output stacking in the same pass, or if that's a follow-up.
- Confirm the version bump cadence — bump per closure, or batch with other work.

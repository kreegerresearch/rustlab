# Rustlab roadmap / backlog (living)

**Status:** living planning doc. Ranks are judgment calls pending Michael's clarifications. This file does not authorize implementation.
**Organized:** 2026-09-24. **Refreshed:** 2026-09-26 against `main` at `5e351f1` (security hardening H1–H5 merged as [#48](https://github.com/kreegerresearch/rustlab/pull/48); notebook cell colouring merged as [#50](https://github.com/kreegerresearch/rustlab/pull/50)).

**Sources folded in:** Michael's notes list (A1–D3, original IDs kept), every open request in `dev/requests/`, and every open plan in `dev/plans/`. Rows that came from a file link it. "Verified" in a Notes cell means reproduced against the 0.3.7 release binary on 2026-09-26.

## Suggested attack order

1. Verified bugs with a repro in hand: A6, A8, A9, then the lesson fixes A1, A3, A4
2. Clarify, then fix: A2, A5, A7 (see "Needs clarification")
3. Plot / REPL / watch correctness: B2–B5, B8–B10
4. Docs / DX: B6, B7, C5
5. Small toolbox adds: C7, C9, C10, then C1, C2
6. Approved plans with no code yet: C8, C11 — each needs an explicit go before work starts
7. Park D1–D3 unless they are redefined
8. Housekeeping H1–H4 whenever a session already touches the file in question

## Tier A — Fix soon (bugs / broken teaching materials)

| ID | Item | Feature / use case | Value | Source | Notes |
|----|------|--------------------|-------|--------|-------|
| A1 | Low-pass filter analysis notebook: magnitude and impulse response wrong (looks like residual in plots) | Correct DSP teaching plots | High | notes | |
| A2 | Dual viewer bug | Two viewer windows / reconnect behave correctly | High | notes | Needs a repro |
| A3 | EM lesson 5: sharp corners of the upper-right box show up lower-right | Fix EM lesson figure geometry | High | notes | |
| A4 | Lesson 3: Gauss formula bad | Fix formula | High | notes | |
| A5 | Lesson 9: LLM error | Reproduce and fix | Medium–High | notes | Vague until the lesson is opened |
| A6 | `printf` / `sprintf` / `${x:%+.3f}` ignore the `+` flag on positive values | Format strings honor `+` | Medium | notes | **Verified:** `sprintf("%+.3f", 1.5)` → `1.500`, `sprintf("%+d", 7)` → `7`; negatives print correctly. Check that `fprintf`, `sprintf`, and the notebook `${x:fmt}` template share one formatter so the fix lands once. |
| A7 | Time-frequency / wavelet "issueq" | Fix wavelet / T–F plot or API | Medium–High | notes | Needs clarification |
| A8 | `A^k` on a matrix is element-wise, not matrix power | Correct `A^k` for square matrices (controls lessons) | High | `dev/requests/matrix-power-vs-elementwise.md` | **Verified:** `[0,1;1,0]^2` returns itself, not the identity. Silent wrong answers. Behaviour change → version bump + CHANGELOG migration note (Rule 12); request suggests a one-release deprecation warning first. |
| A9 | `<!-- solution -->` emits unclosed `<details>` in markdown output | Exercise / solution directives usable in `rustlab-notebook render -f markdown` | High | `dev/requests/solution-directive-unclosed-details.md` | **Verified on 0.3.7:** one `<details` opened, zero `</details>` closed. Blocks `quantum_lab` adoption since 2026-06-11. |

## Tier B — High-value product polish

| ID | Item | Feature / use case | Value | Source | Notes |
|----|------|--------------------|-------|--------|-------|
| B1 | `hold on` persists across notebook blocks | Hold across cells | High | notes | **Moved to Needs clarification.** Batch render already preserves the figure across blocks when hold is on (`crates/rustlab-notebook/src/execute.rs`, `run_code_block_capturing`). Need the failing case: watch server, scoped re-render, HTML, or something else. |
| B2 | Error bars on plots | `errorbar` / yerr | High | notes | Verified: no `errorbar` builtin exists. Cross-backend rule (Rule 9) applies. |
| B3 | Blue on black | Fix unreadable theme contrast | High | notes | Viewer vs HTML vs terminal TBD |
| B4 | Display the entire vector | Don't truncate (or opt-in full dump) | Medium–High | notes | |
| B5 | `ans` should be the last result | `ans` bound to the last unassigned expression | Medium | notes | Verified: no `ans` binding exists in the evaluator. |
| B6 | Watch navigation: README → docs folder; self-discover | Discoverable help | Medium | notes | |
| B7 | Single page description for AI agents | One agents / llms page | Medium | notes | `llms.txt` and `docs/agent-guide.md` already exist. Rescope to the specific gap (consolidation? reachable from the watch server?) or close. |
| B8 | `watch` does not notice files created or deleted during the session | New `.md` appears in the listing and is served; deleted one drops out | Medium–High | `dev/requests/watch-live-collection-changes.md` | Notebook set and path→slug map are startup snapshots. |
| B9 | `--obsidian` auto-generated `index.md` freezes after the first render | Vault home page reflects notebooks added later | Medium | `dev/requests/obsidian-generated-index-freezes.md` | Run 2 treats the generated file as an authored index. |
| B10 | Served index title / H1 does not refresh when `index.md`'s title changes | Live title under `watch` | Low–Medium | `dev/requests/index-title-live-refresh.md` | Body already refreshes; title is a plain `String` read once. |

## Tier C — Features worth scoping

| ID | Item | Feature / use case | Value | Source | Notes |
|----|------|--------------------|-------|--------|-------|
| C1 | CIC filter | Cascaded integrator–comb in the DSP toolbox | Medium–High | notes | Verified: nothing CIC-related in `rustlab-dsp` or the builtins. Pure Rust (Rule 10). |
| C2 | `loadfig()` | Reload a saved figure into the viewer / notebook | Medium | notes | Verified: no builtin. Semantics TBD. |
| C3 | Fixed-point floats | Fixed-point / quantization helpers | Medium | notes | Rescope: `qfmt`, `quantize`, `qadd`, `qmul`, `qconv`, `firpmq` and an `examples/dsp` fixed-point script already ship. State what is missing beyond `QFmt`. |
| C5 | Language best practices (e.g. lambdas) | Docs and examples for idiomatic `.rlab` | Medium | notes | |
| C6 | Conformal area weighting | EM / numerics method | Medium | notes | Niche; confirm the lesson and API |
| C7 | `c2d(sys, dt)` / `d2c(sys, dt)` | Continuous ↔ discrete conversion (ZOH, Tustin) instead of hand-rolled `expm` at every use site | Medium–High | `dev/requests/c2d-d2c-discretization.md` | Verified: neither is registered; `expm` exists. Controls curriculum. |
| C8 | `.rlab` unit-testing framework | Assertion builtins, `rustlab test` runner, TAP/JSON output | Medium–High | `dev/plans/rlab_unit_testing.md` | Plan approved, **no code written**; the named branch `feature/rlab-unit-testing` does not exist on origin. |
| C9 | `poly(r)` | Polynomial coefficients from roots (inverse of `roots`) | Medium | `dev/requests/poly-from-roots.md` | Verified: not registered. Small. |
| C10 | `lqr(A, B, Q, R)` raw-matrix overload | Accept `(A, B)` directly instead of only `ss()` structs | Medium | `dev/requests/lqr-raw-matrix-overload.md` | Small; `care` already does the math. |
| C11 | Evaluator perf: stop cloning `Value` on hot paths | 3–5× on `arrayfun`-heavy scripts | Medium–High | `dev/plans/eval_perf_value_borrow.md` | Plan approved 2026-06-13, **no code written**; branch `feature/eval-perf-borrow` does not exist on origin. Needs benchmarks first. |
| C12 | `zeros(n)` returns 1×n instead of n×n | Last open Octave divergence | Medium | `dev/plans/octave_compat_divergences.md` (#5) | Breaking change; needs a decision, then Rule 12. |
| C13 | Notebook follow-ups P1 (snapshot memory cap) and P3 (`strip_render_artifacts` single pass) | Renderer memory / speed | Low | `dev/plans/notebook_followups_2026_05_16.md` | Deferred until profiling justifies. |
| C14 | Persistent function cache Phase 6 polish (`#[memoize]`, `%%cache`, zstd, per-function hashing) | Cache ergonomics | Low | `dev/plans/persistent_function_cache.md` | None blocking. |

## Tier D — Stretch / research (defer or redefine)

| ID | Item | Feature / use case | Value | Notes |
|----|------|--------------------|-------|-------|
| D1 | Distributed computing | — | — | Impractical near-term without a concrete target (`parmap` threading and audio streams already exist). Redefine or park. |
| D2 | Finite element | — | — | Huge scope. Park unless the ask is a thin 1D/2D demo only. |
| D3 | Exponential quantum advantage | — | — | Vague research claim, not an engineering feature. Put it in curriculum elsewhere, or drop it. |

## Needs clarification (blocked)

1. **Dual viewer bug (A2)** — what is the exact failure mode?
2. **Lesson 9 LLM error (A5)** — error text, or the notebook path?
3. **Wavelet "issueq" (A7)** — typo? Which notebook or API?
4. **`loadfig()` (C2)** — live viewer, HTML, or both?
5. **Blue on black (B3)** — viewer, HTML theme, or terminal?
6. **Display the entire vector (B4)** — always, or a flag / `format long` style?
7. **`hold on` across blocks (B1)** — batch render already does this; which path fails?
8. **Single agent page (B7)** — what is missing from `llms.txt` + `docs/agent-guide.md`?
9. **Fixed-point (C3)** — what beyond the existing `QFmt` builtins?

## Done / parked

Marked complete. Do not reopen unless asked. Pointers added where the shipping artefact is unambiguous.

- `prod()` and `filtfilt` — builtins on main
- Stats calls: time in function and I/O — `tic` / `toc`, `profile()`, `--profile`
- Stream or events — audio streaming plan (`dev/plans/closed/audio_streaming.md`)
- Threading — `parmap` / `nproc`
- Controls rustlab — `dev/plans/closed/controls.md`
- Presentation mode: HTML and Plotly
- Stats perf flag
- `mag2db` in live plot — `dev/plans/closed/live_plot.md`
- Licensing — `THIRD_PARTY_NOTICES.md`, `dev/plans/notebook_interactive_server-tradeoff.md`
- Version in notebooks
- Output format on CLI help
- Plotting and viewer simplify — `dev/plans/plot_output_simplification.md`
- Remove redundant functions (`savefig` replaced)
- S-parameter — `dev/plans/closed/sparameters.md`
- Render into other notebooks like Jupyter
- Subplots in notebooks
- Inline formulas / math rendering — KaTeX in `rustlab-notebook`
- rustlab viewer consuming CPU — event-driven repaint model (see `AGENTS.md` § `rustlab-viewer`)
- **C4** PDF syntax highlighting — shipped in #50 (HTML, watch, and PDF)
- Security hardening H1–H5 — merged as #48; threat model in `docs/security.md`
- Integer types — `dev/plans/integer_types.md`, phases 1–4 complete; `int8` … `uint64` builtins on main
- Log-scale axes + legend fixes — `dev/plans/log_axis_and_legend_fix.md`; true log axes with clean decade ticks in SVG and HTML (follow-up 2026-07-24)
- Waterfall plot — `waterfall`, `waterfall_stream_init`, `waterfall_stream` on main
- EM requests items 1–4 (masks, sparse solve, Laplacian variants, `eigs`) — `dev/plans/em_requests_queue.md` says all upstream items shipped

## Housekeeping (plan-file hygiene, no code)

| ID | Item | Notes |
|----|------|-------|
| H1 | Move completed plans to `dev/plans/closed/` | `notebook_cell_execution`, `notebook_interactive_widgets`, `notebook_interactive_server` (+ its `-tradeoff`), `notebook_future`, `em_lesson_review_2026_07`, `time_frequency`, `examples_notebooks_bug_hunt`, `integer_types`, `log_axis_and_legend_fix` — all complete per `AGENTS.md` or their own logs. |
| H2 | `dev/plans/waterfall.md` still says "proposed, awaiting approval" | The builtins are on main; update the status line and close. |
| H3 | `dev/plans/em_requests_plan.md` says "Item 4 next" while `em_requests_queue.md` (2026-04-26) says all upstream items shipped and `eigs` is registered | Reconcile both files and the `AGENTS.md` Active Plans row. |
| H4 | Stray `docs/lectures/week3.rcache` in the working tree | Function-result cache from a lecture run. `.gitignore` only covers `.rustlab/`; delete it or add an ignore rule. |

## Process

After clarifications, break Tier A and Tier B into smaller implementation tickets and PRs. Update this doc when an item moves (started, shipped, parked, or dropped), and move the source request or plan file to its `closed/` directory in the same PR.

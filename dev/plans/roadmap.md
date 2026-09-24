# Rustlab roadmap / backlog (living)

**Status:** living planning doc. Ranks are judgment calls pending Michael’s clarifications. This file does not authorize implementation.
**Organized:** 2026-09-24
**Security:** hardening H1–H5 stays on draft PR [#48](https://github.com/kreegerresearch/rustlab/pull/48) (`cursor/security-notebook-hardening-6373`) for review. Do not merge that branch from work that only updates this doc.

## Suggested attack order (after #48 review)

1. Teaching / notebook bugs: A1 → A3 → A4 → A5 → A7
2. Plot / REPL correctness: A6, B1–B5
3. Docs / DX: B6, B7, C5
4. DSP adds: C1, then C2
5. Park D1–D3 unless they are redefined

## Tier A — Fix soon (bugs / broken teaching materials)

| ID | Item | Feature / use case | Value | Notes |
|----|------|--------------------|-------|-------|
| A1 | Low-pass filter analysis notebook: magnitude and impulse response wrong (looks like residual in plots) | Correct DSP teaching plots | High | |
| A2 | Dual viewer bug | Two viewer windows / reconnect behave correctly | High | Needs a repro |
| A3 | EM lesson 5: sharp corners of the upper-right box show up lower-right | Fix EM lesson figure geometry | High | |
| A4 | Lesson 3: Gauss formula bad | Fix formula | High | |
| A5 | Lesson 9: LLM error | Reproduce and fix | Medium–High | Vague until the lesson is opened |
| A6 | `printf` / `${x:%+.3f}` ignores the `+` flag on positive values | Format strings honor `+` | Medium | |
| A7 | Time-frequency / wavelet “issueq” | Fix wavelet / T–F plot or API | Medium–High | Needs clarification |

## Tier B — High-value product polish

| ID | Item | Feature / use case | Value | Notes |
|----|------|--------------------|-------|-------|
| B1 | `hold on` persists across notebook blocks | MATLAB-like hold across cells | High | |
| B2 | Error bars on plots | `errorbar` / yerr | High | |
| B3 | Blue on black | Fix unreadable theme contrast | High | Viewer vs HTML vs terminal TBD |
| B4 | Display the entire vector | Don’t truncate (or opt-in full dump) | Medium–High | |
| B5 | `ans` should be the last result | MATLAB-style `ans` | Medium | |
| B6 | Watch navigation: README → docs folder; self-discover | Discoverable help | Medium | |
| B7 | Single page description for AI agents | One agents / llms page | Medium | May partially exist (`llms.txt`, `docs/agent-guide.md`) |

## Tier C — Features worth scoping

| ID | Item | Feature / use case | Value | Notes |
|----|------|--------------------|-------|-------|
| C1 | CIC filter | Cascaded integrator–comb in the DSP toolbox | Medium–High | |
| C2 | `loadfig()` | Reload a saved figure into the viewer / notebook | Medium | Semantics TBD |
| C3 | Fixed-point floats | Fixed-point / quantization helpers | Medium | Large design surface |
| C4 | PDF syntax highlighting | Highlight code in PDF notebooks | Medium | |
| C5 | Language best practices (e.g. lambdas) | Docs and examples for idiomatic `.rlab` | Medium | |
| C6 | Conformal area weighting | EM / numerics method | Medium | Niche; confirm the lesson and API |

## Tier D — Stretch / research (defer or redefine)

| ID | Item | Feature / use case | Value | Notes |
|----|------|--------------------|-------|-------|
| D1 | Distributed computing | — | — | Impractical near-term without a concrete target (threading and streams already exist). Redefine or park. |
| D2 | Finite element | — | — | Huge scope. Park unless the ask is a thin 1D/2D demo only. |
| D3 | Exponential quantum advantage | — | — | Vague research claim, not an engineering feature. Put it in curriculum elsewhere, or drop it. |

## Needs clarification (blocked)

1. **Dual viewer bug (A2)** — what is the exact failure mode?
2. **Lesson 9 LLM error (A5)** — error text, or the notebook path?
3. **Wavelet “issueq” (A7)** — typo? Which notebook or API?
4. **`loadfig()` (C2)** — live viewer, HTML, or both?
5. **Blue on black (B3)** — viewer, HTML theme, or terminal?
6. **Display the entire vector (B4)** — always, or a flag / `format long` style?

## Done / parked

Marked complete. Do not reopen unless asked.

- `prod()` and `filtfilt`
- Stats calls: time in function and I/O
- Stream or events
- Threading
- Controls rustlab
- Presentation mode: HTML and Plotly
- Stats perf flag
- `mag2db` in live plot
- Licensing
- Version in notebooks
- Output format on CLI help
- Plotting and viewer simplify
- Remove redundant functions (`savefig` replaced)
- S-parameter
- Render into other notebooks like Jupyter
- Subplots in notebooks
- Inline formulas / math rendering
- rustlab viewer consuming CPU

## Process

After clarifications, break Tier A and Tier B into smaller implementation tickets and PRs. Update this doc when an item moves (started, shipped, parked, or dropped).

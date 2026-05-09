# Notebook Bugfixes — Implementation Plan

**Status:** Complete — both underlying bugs fixed on `main` (math
backslash: feb2fb0/6a4f618; TUI suppression: 597b95d). The verification
+ regression-test phases described below ran during the closeout.

References:
- `dev/issues/notebook-math-backslash-escape.md`
- `dev/issues/notebook-tui-suppression.md`

## TL;DR — Both bugs are already fixed on `main`

Repo exploration shows both proposals have already been implemented and
shipped. The remaining work is a small verification + gap-fill pass, not a
feature build.

Evidence:

1. **Math backslash bug — fixed.** `crates/rustlab-notebook/src/render.rs`
   `protect_math` / `restore_math` (~lines 957–1095), called at lines 59–67
   (Markdown blocks) and 194–201 (Callout blocks). 9 unit tests under
   `protect_math_*` and 2 under `render_html_*_preserves*` are green on
   `main`. Landed in commits `feb2fb0` "Protect math spans from CommonMark
   backslash escapes" and `6a4f618` "Fix notebook math backslash stripping,
   bump to v0.1.8".

2. **TUI suppression bug — fixed.** Implemented as `PlotContext`
   (process-level, thread-local enum) in
   `crates/rustlab-plot/src/figure.rs:327–354` — same shape as the issue's
   `BATCH_MODE: Cell<bool>` proposal under a clearer name. Wiring:
   - `default_new_output()` (`figure.rs:458–468`) checks
     `plot_context() == Notebook` first and returns
     `FigureOutput::Html(String::new())`.
   - `execute_notebook()` (`crates/rustlab-notebook/src/execute.rs:52`)
     calls `set_plot_context(PlotContext::Notebook)`. The comment notes
     "PlotContext::Notebook is sticky: figure() calls cannot override it."
   - `render_figure_terminal` / `render_heatmap_tui` / `render_image_tui` /
     surface render all early-return on `Notebook | Headless`
     (`ascii.rs:314–320, 425–431, 504–510, 613–619`).
   - `rustlab run --plot none` already exists
     (`crates/rustlab-cli/src/commands/run.rs:6–14, 74–118`) and sets
     `PlotContext::Headless` — same kill-switch as the issue's `--batch`
     proposal under a different name.
   - Landed in commit `597b95d` "Add PlotContext to fix TUI plot
     suppression in notebook rendering".

So the plan below is a closeout: confirm coverage, add the specific
edge-case tests the issues called for that don't yet exist, decide on the
naming question, and archive the issues.

---

## Phase 1 — Verify the math fix against the issue's edge-case list

### Existing tests in `crates/rustlab-notebook/src/render.rs`

| Issue edge case | Existing test | Status |
|---|---|---|
| `\\` in `$$\begin{pmatrix}…\\…\end{pmatrix}$$` survives | `protect_math_display_preserves_double_backslash`, `render_html_preserves_matrix_row_separator` | covered |
| `\\` in inline `$\begin{smallmatrix}a\\b\end{smallmatrix}$` survives | `protect_math_inline_basic` covers the inline path | thin — add explicit smallmatrix test |
| `\$` in prose does not open math | `protect_math_respects_escaped_dollar` | covered |
| `` `$x$` `` in inline code untouched | `protect_math_skips_inside_inline_code` | covered |
| Math inside fenced code untouched | `protect_math_skips_inside_fenced_code` | covered |
| Aligned with multiple `\\` rows each on own line | none explicitly | **missing** |
| Multi-line `$$\n…\n$$` | `protect_math_multiline_display` | covered |
| Round-trip restore | `restore_math_round_trip` | covered |
| Callout-block math | `render_html_callout_preserves_math_backslashes` | covered |
| Unclosed `$$` left alone | `protect_math_unclosed_display_left_alone` | covered |
| Whitespace-padded `$ 5` not opened | `protect_math_skips_whitespace_padded_dollars` | covered |
| Closing `$` followed by digit | `protect_math_skips_prices` | covered |

### Tests to add

In `crates/rustlab-notebook/src/render.rs` `mod tests`:

1. `protect_math_aligned_environment_preserves_each_row` — feed
   `$$\begin{aligned} a &= 1 \\ b &= 2 \\ c &= 3 \end{aligned}$$`; assert
   the stash's single entry contains exactly three `\\` substrings.
2. `protect_math_inline_smallmatrix_preserves_separator` — inline
   `$\begin{smallmatrix}a \\ b\end{smallmatrix}$`; assert stash has one
   entry containing `\\`.
3. `protect_math_cases_preserves_each_branch` —
   `$$f(x) = \begin{cases} 0 & x<0 \\ 1 & x \ge 0 \end{cases}$$`; assert
   two `\\` survive.
4. `protect_math_empty_display_span` — `$$$$` round-trips without panic;
   asserts current behaviour explicitly so a regression is loud.

Each is ~6 lines, mirrors existing `protect_math_*` style, no new deps.

### Manual smoke check

Render the notebooks the issue named in `notebook-math-backslash-escape.md`
and grep the produced HTML for the matrices the issue called out. Local
verification only — not CI.

```
cargo run -p rustlab-notebook -- render quantum_lab/lessons/02-quantum-gates --out-dir /tmp/qg
grep -c '\\\\' /tmp/qg/02-quantum-gates.html   # must be > 0 for Pauli-X/Y/Z
```

### Done criteria
- 4 new tests added and passing
- Existing `render::tests` tests still pass
  (`cargo test -p rustlab-notebook --lib render::`)
- Manual render of `02-quantum-gates/notebook.md` shows `\\` in HTML
- `dev/issues/notebook-math-backslash-escape.md` archived (annotate
  "RESOLVED in feb2fb0/6a4f618" or move to `dev/issues/closed/`)

---

## Phase 2 — Verify the TUI fix and add the missing regression test

### What the existing fix already gives us

- `default_new_output()` (`figure.rs:458–468`) returns
  `FigureOutput::Html(_)` under `PlotContext::Notebook`. The
  "figure() overwrites Html suppression" failure mode cannot occur.
- `figure_new()` (`figure.rs:471–491`) and `figure_switch()`
  (`figure.rs:512–550`) both consume `default_new_output()`, so calling
  `figure()` with no args **and** with a numeric arg both honor the
  context.
- `render_figure_terminal()` (`ascii.rs:313–320`),
  `render_heatmap_tui()` (`ascii.rs:416–431`),
  `render_image_tui()` (`ascii.rs:498–510`), and the surface renderer
  (`ascii.rs:613–619`) all early-return under `Notebook | Headless`
  before any ratatui/crossterm setup. `wait_for_key()` is unreachable.
- `imagesc_terminal` lives at `ascii.rs:498` (issue cites `:281` — moved).
- `builtin_figure` lives at
  `crates/rustlab-script/src/eval/builtins.rs:3023–3043` (issue cites
  `builtins.rs:1793` — moved). The no-arg branch at `:3041` calls
  `figure_new()`, which now honors the context.

### Tests to add

The `ascii.rs` tests at `:1197` and `:1230` already toggle
`PlotContext::Notebook` for `imagesc` / `render_heatmap_tui` smoke tests.
The exact regression that motivated the issue is **`figure()` mid-block
must not break suppression**.

Add to `crates/rustlab-notebook/src/execute.rs` `mod tests` (the existing
`notebook_*` cluster around `:379–486`):

1. `notebook_figure_call_does_not_override_suppression` — block source
   `"x = 0:10; figure(); plot(x, sin(x));"`. Assert:
   - `error` is `None`
   - `figures.len() == 1`
   - `rustlab_plot::plot_context() == PlotContext::Notebook` after exec
   - `current_figure_output()` is `Html(_)` not `Terminal`

2. `notebook_imagesc_does_not_block` — block source
   `"A = magic(5); imagesc(A);"`. Assert no error and one figure captured.
   If this regresses the test process hangs on `wait_for_key`, which is
   exactly the failure mode the issue describes.

3. `notebook_multiple_figures_in_block` — block source
   `"figure(); plot(1:5); figure(); plot(1:5, (1:5).^2);"`. Assert
   `figures.len() == 2` and `text_output` is empty (no terminal leak).

These are inexpensive (no I/O, no real terminal) and directly gate the
fix.

### Done criteria
- 3 new tests added and passing in `execute::tests`
- Full notebook test suite green (`cargo test -p rustlab-notebook`)
- Full plot test suite green (`cargo test -p rustlab-plot`)
- `dev/issues/notebook-tui-suppression.md` archived (annotate "RESOLVED in
  597b95d")

---

## Phase 3 — CLI naming decision (doc-only by default)

The issue proposes `rustlab run --batch`. The repo already exposes the
same capability as `rustlab run --plot none`, which sets
`PlotContext::Headless`.

**Option A — accept existing flag, close issue.** Document `--plot none`
as the batch-mode flag in CLI help text and the closeout note. No code.

**Option B — add `--batch` alias.** In
`crates/rustlab-cli/src/commands/run.rs`, add a boolean
`#[arg(long, conflicts_with = "plot")]` that maps to `PlotMode::None`.
~6 lines.

Recommendation: **Option A.** A second flag for the same behaviour is API
surface debt and `--plot none` is more explicit about its effect.

---

## Risks and open questions

1. **Thread-local vs Evaluator config.** The issue proposed
   `BATCH_MODE: Cell<bool>`. `PlotContext` is the same scope under a
   clearer name. If `Evaluator` ever runs on multiple threads (it does
   not today — `FIGURE` is also `thread_local!`), both rework together.
   Not a regression. Add a `// see also: FIGURE thread_local in figure.rs`
   comment near `PLOT_CONTEXT` if not already there.

2. **`PlotContext::Notebook` vs `PlotContext::Headless`.** Both
   early-return from terminal renderers identically. Difference:
   `Notebook` *also* causes `default_new_output()` to return `Html(_)` so
   figures are captured. The semantic split is correct; worth a one-line
   doc-comment contrast on the enum.

3. **`--batch` vs `--plot none`.** See Phase 3. Defer to user.

4. **Stale issue line numbers.** `notebook-tui-suppression.md` cites
   `figure.rs:251`, `ascii.rs:281`, `builtins.rs:1793`. None match
   current `main`. Update line numbers when archiving so future readers
   can navigate.

5. **No new dependencies.** Both fixes are pure Rust against existing
   crates — no trade-off note required.

6. **PUA placeholder choice.** `math_placeholder` uses `\u{E000}` and
   `\u{E001}`. Sound; no risk pulldown-cmark mangles these. Keep.

---

## Files touched

| File | Phase | Change |
|---|---|---|
| `crates/rustlab-notebook/src/render.rs` | 1 | 4 new tests in `mod tests`. No production change. |
| `crates/rustlab-notebook/src/execute.rs` | 2 | 3 new tests in `mod tests`. No production change. |
| `crates/rustlab-plot/src/figure.rs` | 2 (optional) | Doc comment on `PlotContext` contrasting `Notebook` vs `Headless`. |
| `crates/rustlab-cli/src/commands/run.rs` | 3 (Option B only) | `--batch` alias for `--plot none`. Skip if Option A. |
| `dev/issues/notebook-math-backslash-escape.md` | 1 | Archive / annotate "RESOLVED in feb2fb0/6a4f618". |
| `dev/issues/notebook-tui-suppression.md` | 2 | Archive / annotate "RESOLVED in 597b95d". |

Total estimated diff: ~80 lines of new test code, optional ~6 lines of
CLI alias, two file moves. No production logic changes — both bugs are
already closed by code on `main`.

---

## Sequencing

1. Phase 1 (math): self-contained, single file, low risk.
2. Phase 2 (TUI): tests touch `rustlab-plot` indirectly through
   `execute_notebook`; run full plot suite after.
3. Phase 3 if user wants the alias.
4. Single PR with both phases.

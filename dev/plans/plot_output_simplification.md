# Plan: Simplify Plot Output Routing

**Status:** core fix implemented (PlotContext)  
**Related:** `dev/issues/notebook-tui-suppression.md`

---

## Problem

Plot output routing is scattered across thread-locals, auto-detection
heuristics, and per-figure overrides. The current system has these issues:

1. **Notebook workaround is fragile.** The notebook sets
   `FigureOutput::Html("")` at startup, but any `figure()` call in a code
   block calls `default_new_output()` which returns `Terminal`, overwriting
   the suppression. Plots leak to the TUI and block on keypress.

2. **`render_figure_terminal()` does too much.** It's named "terminal" but
   also routes to the viewer via `sync_viewer()`. Every plot builtin calls
   it, and it dispatches on a three-way enum internally.

3. **Output mode is per-figure but context is per-process.** The notebook
   binary should *never* render to a terminal, regardless of what
   `figure()` calls the user makes. But the mode is stored per-figure in
   `FigureStore`, so every new figure needs to be individually overridden.

4. **`viewer on`/`viewer off` is a language statement** (`StmtKind::Viewer`)
   that directly manipulates plot internals from the evaluator. The
   `figure("file.html")` form is a builtin function. The `report` system is
   a REPL command. Three different mechanisms control where output goes.

5. **`sync_figure_outputs()` is called separately from rendering.** After
   every plot command, builtins call `render_figure_terminal()` (which may
   route to viewer) *and then* `sync_figure_outputs()` (which may *also*
   route to viewer). The viewer path runs twice.

---

## Current Architecture

```
User calls plot(x, y)
  → builtin_plot()
    → push_xy_line()                          [mutates FIGURE thread-local]
    → render_figure_terminal()
        match current_figure_output():
          Terminal → ratatui TUI + wait_for_key()
          Html(_)  → return (no-op)
          Viewer   → sync_viewer()
    → sync_figure_outputs()
        match current_figure_output():
          Terminal → no-op
          Html(_)  → sync_html_file()
          Viewer   → sync_viewer()             ← called again!

User calls figure()
  → default_new_output()
      viewer_active()? → Viewer
      else             → Terminal              ← ignores notebook context

User calls figure("out.html")
  → figure_new_html("out.html")               [separate code path]

User types "viewer on"
  → StmtKind::Viewer { on: true }
    → connect_viewer()
    → set_current_figure_output(Viewer(id))    [language-level statement]
```

**Thread-locals involved:** `FIGURE`, `STORE` (FigureStore), `HTML_FIGURE_PATH`,
`VIEWER_CONN`, `ACTIVE_REPORT` — five separate thread-locals that must stay
in sync.

---

## Proposal: Process-level output context + single render dispatch

### Core idea

Introduce a **process-level output context** that overrides per-figure
defaults. The context is set once at startup by each binary and cannot be
overridden by user code (like `figure()`).

```rust
/// Set once at process startup. Determines where new figures route by default
/// and what rendering is allowed.
pub enum PlotContext {
    /// Interactive terminal (REPL, `rustlab run`). TUI rendering allowed.
    Terminal,
    /// Notebook batch rendering. No TUI, no viewer. Figures are captured
    /// as FigureState and rendered by the notebook itself.
    Notebook,
    /// Report generation. Like Notebook but driven from the REPL.
    Report,
}
```

### Changes by binary

| Binary | Sets context to | Effect |
|--------|----------------|--------|
| `rustlab` (REPL/run) | `Terminal` | Current behavior. `viewer on` can upgrade to viewer. |
| `rustlab-notebook` | `Notebook` | `default_new_output()` always returns `Html("")`. `render_figure_terminal()` is always a no-op. `figure()` never touches TUI. |
| (future `--batch` flag) | `Notebook` | Same suppression for headless `rustlab run`. |

### What changes in the code

**1. Add `PlotContext` and a thread-local to `rustlab-plot/src/figure.rs`:**

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlotContext {
    Terminal,
    Notebook,
}

thread_local! {
    static PLOT_CONTEXT: Cell<PlotContext> = Cell::new(PlotContext::Terminal);
}

pub fn set_plot_context(ctx: PlotContext) { ... }
pub fn plot_context() -> PlotContext { ... }
```

**2. `default_new_output()` respects context:**

```rust
fn default_new_output() -> FigureOutput {
    if plot_context() == PlotContext::Notebook {
        return FigureOutput::Html(String::new());
    }
    #[cfg(feature = "viewer")]
    if viewer_active() {
        return FigureOutput::Viewer(allocate_viewer_fig_id());
    }
    FigureOutput::Terminal
}
```

This is the key fix — notebook context is *sticky*. No matter how many
times the user calls `figure()`, new figures always get `Html("")` in
notebook mode.

**3. `render_figure_terminal()` respects context:**

Add an early return at the top:

```rust
if plot_context() == PlotContext::Notebook {
    return Ok(());
}
```

This is belt-and-suspenders — `default_new_output()` already prevents
`Terminal` mode, but this catches edge cases where output was set manually.

**4. Notebook executor simplifies:**

```rust
pub fn execute_notebook(blocks: &[Block]) -> Vec<Rendered> {
    set_plot_context(PlotContext::Notebook);  // one line, sticky
    // ... rest unchanged, remove the set_current_figure_output() call
}
```

**5. `imagesc_terminal()` respects context:**

Same early return for `Notebook` context.

### What stays the same

- `viewer on` / `viewer off` keeps working as-is in `Terminal` context
- `figure("file.html")` keeps working (explicit HTML mode)
- `savefig()`, `savebar()`, etc. keep working (they bypass the output routing)
- Per-figure output modes still exist (for multi-figure HTML/viewer mixing)
- `FigureStore`, `FIGURE` thread-local, all series data — untouched

### What this does NOT do

This proposal is intentionally minimal. It fixes the notebook TUI leak
with a single new enum + thread-local and 3 guard checks. It does **not**:

- Merge `viewer on` with `figure()` (they serve different purposes)
- Refactor `render_figure_terminal()` into separate functions
- Remove `sync_figure_outputs()` duplication
- Change the report system
- Add a `--batch` flag to `rustlab run` (easy follow-up)

---

## Future simplifications (not in this PR)

These are ideas for further cleanup if the `PlotContext` approach proves
its value:

### A. Unify render dispatch

Replace the `render_figure_terminal()` + `sync_figure_outputs()` two-call
pattern with a single `render_current_figure()` that does the right thing:

```rust
pub fn render_current_figure() -> Result<(), PlotError> {
    match current_figure_output() {
        Terminal  => render_to_tui(),      // ratatui
        Html(p)   => sync_html_file(),     // plotly
        Viewer(v) => sync_viewer(),        // IPC
    }
}
```

Every builtin calls this once instead of calling `render_figure_terminal()`
and then `sync_figure_outputs()`. Eliminates the double viewer sync.

### B. `--batch` flag on `rustlab run`

```
rustlab run --batch script.rlab
```

Sets `PlotContext::Notebook` so scripts that `savefig()` to disk can run
without any TUI interaction. Useful for CI and automated pipelines.

---

## Files changed

| File | Change |
|------|--------|
| `crates/rustlab-plot/src/figure.rs` | Add `PlotContext` enum, thread-local, `set_plot_context()`, `plot_context()`. Update `default_new_output()`. |
| `crates/rustlab-plot/src/lib.rs` | Re-export `PlotContext`, `set_plot_context`, `plot_context` |
| `crates/rustlab-plot/src/ascii.rs` | Add `PlotContext::Notebook` guard in `render_figure_terminal()` and `imagesc_terminal()` |
| `crates/rustlab-notebook/src/execute.rs` | Replace `set_current_figure_output()` with `set_plot_context(Notebook)` |

4 files, ~25 lines of new code.

---

## Test strategy

1. Existing 933 tests continue to pass (default context is `Terminal`)
2. Add unit test: `set_plot_context(Notebook)` → `default_new_output()` returns `Html("")`
3. Add unit test: in `Notebook` context, `figure_new()` does not return `Terminal` mode
4. Notebook integration: render a notebook with `figure()` + `plot()` calls, verify no TUI rendering and plots appear in HTML output

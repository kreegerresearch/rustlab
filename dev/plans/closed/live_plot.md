# Development Plan: Live Plot & Spectrum Monitor

**Target example:** `examples/audio/spectrum_monitor.rlab`
**Current phase:** complete
**Status:** All phases complete
**Depends on:** `audio_streaming.md` Phases 1–4 (for the example script only; Phases 1–2 of this plan are independent)

---

## Overview

Add a persistent live-updating terminal plot (`LiveFigure`) that stays open
across multiple draw calls — suitable for real-time monitoring, oscilloscope
displays, and spectrum analyzers. The scripting API target:

```r
h = window(1024, "hann");
adc = audio_in(44100.0, 1024);
fig = figure_live(2, 1);              # 2 rows, 1 column

while true
    frame = audio_read(adc);
    X     = fft(frame .* h);
    freqs = fftfreq(1024, 44100.0);

    plot_update(fig, 1, frame);                    # time domain
    plot_update(fig, 2, freqs(1:512), mag2db(X(1:512)));  # spectrum in dB
    figure_draw(fig);
end
```

Work is split into three phases. Phases 1–2 (the live plot infrastructure)
are **independent of the audio streaming plan** and can ship first.

---

## Phase 1 — `LiveFigure` in `rustlab-plot`

**Status: complete**

All changes in this phase are confined to `crates/rustlab-plot/`.

### 1a. Extract a shared rendering helper

- **File:** `crates/rustlab-plot/src/ascii.rs`
- **Problem:** `render_figure_terminal()` contains the multi-subplot rendering
  closure inline. `LiveFigure::redraw()` needs to run the same layout + chart
  logic but on a pre-existing `Terminal` rather than a freshly-created one.
- **Change:** Extract the inner `terminal.draw(|f| { ... })` closure body into
  a standalone function:
  ```rust
  /// Render a slice of SubplotState panels into an existing ratatui frame.
  /// Called by both render_figure_terminal() and LiveFigure::redraw().
  pub(crate) fn draw_subplots(
      f: &mut ratatui::Frame,
      subplots: &[SubplotState],
      rows: usize,
      cols: usize,
  );
  ```
  `render_figure_terminal()` is then refactored to call `draw_subplots` inside
  its own `terminal.draw(|f| draw_subplots(f, ...))` call. Behavior is
  unchanged — this is a pure refactor with no API change.
- **Test:** All existing plot tests pass after the refactor.

### 1b. `LiveFigure` struct

- **File:** `crates/rustlab-plot/src/live.rs` (new file)

```rust
use crossterm::{execute, terminal::{disable_raw_mode, enable_raw_mode,
    EnterAlternateScreen, LeaveAlternateScreen}};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::Stdout;
use crate::{ascii::draw_subplots, error::PlotError, figure::SubplotState};

pub struct LiveFigure {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    panels:   Vec<SubplotState>,   // length = rows * cols
    rows:     usize,
    cols:     usize,
}
```

**`LiveFigure::new(rows: usize, cols: usize) -> Result<LiveFigure, PlotError>`**
1. Check `std::io::stdout().is_terminal()` — return `PlotError::NotATty` if false
   (caller should degrade gracefully, e.g. skip drawing).
2. `execute!(stdout(), EnterAlternateScreen)?`
3. `enable_raw_mode()?`
4. `Terminal::new(CrosstermBackend::new(stdout()))?`
5. Initialize `panels = vec![SubplotState::new(); rows * cols]`.
6. Return `Ok(LiveFigure { terminal, panels, rows, cols })`.

**`LiveFigure::update_panel(&mut self, idx: usize, x: Vec<f64>, y: Vec<f64>)`**
- `idx` is 0-based internally (callers pass 1-based, conversion at the
  builtin boundary).
- Replaces the series data for panel `idx`:
  ```rust
  let panel = &mut self.panels[idx];
  panel.series.clear();
  panel.series.push(Series {
      label: String::new(),
      x_data: x,
      y_data: y,
      color: SeriesColor::Cyan,
      style: LineStyle::Solid,
      kind: PlotKind::Line,
  });
  ```
- Does **not** redraw — caller calls `redraw()` explicitly after all panels
  are updated. This ensures a single atomic screen refresh per loop iteration
  rather than two partial flickers.

**`LiveFigure::set_panel_labels(&mut self, idx: usize, title: &str, xlabel: &str, ylabel: &str)`**
- Sets `panels[idx].title`, `.xlabel`, `.ylabel`. Optional; defaults to empty.

**`LiveFigure::redraw(&mut self) -> Result<(), PlotError>`**
```rust
pub fn redraw(&mut self) -> Result<(), PlotError> {
    let panels = &self.panels;
    let rows   = self.rows;
    let cols   = self.cols;
    self.terminal.draw(|f| draw_subplots(f, panels, rows, cols))?;
    Ok(())
}
```
No `wait_for_key()` — returns immediately after the ratatui draw call.

**`Drop for LiveFigure`**
```rust
impl Drop for LiveFigure {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
    }
}
```
This fires on `figure_close()`, at end of script, or on Ctrl-C (Rust drops
locals when unwinding). The terminal is always restored.

**`PlotError::NotATty`** — add this variant to `error.rs`:
```rust
#[error("figure_live requires a real terminal (stdout is not a tty)")]
NotATty,
```

### 1c. Re-export from `lib.rs`

- **File:** `crates/rustlab-plot/src/lib.rs`
- **Change:**
  ```rust
  pub mod live;
  pub use live::LiveFigure;
  ```

### 1d. Tests

No hardware required for these tests — they test data logic only:

```rust
#[test]
fn test_live_figure_panel_update() {
    // Can't open a terminal in CI, but we can test the data model
    let mut panels = vec![SubplotState::new(); 2];
    // update panel 0 with dummy data
    panels[0].series.push(Series { ... });
    assert_eq!(panels[0].series.len(), 1);
    assert!(panels[1].series.is_empty());
}
```

Mark any test that calls `LiveFigure::new()` with `#[ignore = "requires tty"]`.

---

## Phase 2 — Script Integration

**Status: complete**

All changes in this phase are confined to `crates/rustlab-script/`.

### 2a. `Value::LiveFigure`

- **File:** `crates/rustlab-script/src/eval/value.rs`
- **Change:** Add variant:
  ```rust
  /// Handle to a persistent live-updating terminal plot.
  /// Arc<Mutex<Option<...>>> allows cheap Value clone (ref-counted) while
  /// the Option lets figure_close drop the inner LiveFigure (firing Drop →
  /// terminal restore) without invalidating other clones of the Arc.
  LiveFigure(Arc<Mutex<Option<rustlab_plot::LiveFigure>>>),
  ```
- `type_name()` → `"live_figure"`.
- `Display` → `"<live_figure>"` (closed figures display as `"<live_figure closed>"`).
- `Clone` is derived (Arc clone — O(1), no deep copy).

### 2b. `figure_live(rows, cols)` builtin

```rust
fn builtin_figure_live(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("figure_live", &args, 2)?;
    let rows = args[0].to_usize()?;
    let cols = args[1].to_usize()?;
    let fig  = rustlab_plot::LiveFigure::new(rows, cols)
                   .map_err(|e| ScriptError::Runtime(e.to_string()))?;
    Ok(Value::LiveFigure(Arc::new(Mutex::new(Some(fig)))))
}
```

If stdout is not a tty (e.g. running in CI or piped), `LiveFigure::new`
returns `PlotError::NotATty` which becomes a `ScriptError::Runtime`. Scripts
that need graceful degradation can wrap the call in an `if` guard checking
`isatty()` — see Phase 3 for that builtin.

### 2c. `plot_update(fig, panel, y)` and `plot_update(fig, panel, x, y)` builtins

- **Signature:** 3 or 4 arguments.
- Panel index is **1-based** (consistent with the language).
- `y` must be a Vector; `x` if provided must be a Vector of the same length.
- If `x` is omitted, auto-generate `1, 2, ..., N` as the x-axis.

```rust
fn builtin_plot_update(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("plot_update", &args, 3, 4)?;
    let Value::LiveFigure(fig) = &args[0] else {
        return Err(ScriptError::type_error("plot_update", "live_figure", args[0].type_name()));
    };
    let panel = args[1].to_usize()?.saturating_sub(1);  // 1-based → 0-based
    let (x, y) = if args.len() == 4 {
        let x = args[2].to_cvector()?.iter().map(|c| c.re).collect::<Vec<_>>();
        let y = args[3].to_cvector()?.iter().map(|c| c.re).collect::<Vec<_>>();
        (x, y)
    } else {
        let y = args[2].to_cvector()?.iter().map(|c| c.re).collect::<Vec<_>>();
        let x = (1..=y.len()).map(|i| i as f64).collect::<Vec<_>>();
        (x, y)
    };
    fig.lock().unwrap()
        .as_mut()
        .ok_or_else(|| ScriptError::Runtime("plot_update: figure is closed".to_string()))?
        .update_panel(panel, x, y);
    Ok(Value::None)
}
```

### 2d. `figure_draw(fig)` builtin

```rust
fn builtin_figure_draw(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("figure_draw", &args, 1)?;
    let Value::LiveFigure(fig) = &args[0] else {
        return Err(ScriptError::type_error("figure_draw", "live_figure", args[0].type_name()));
    };
    fig.lock().unwrap()
        .as_mut()
        .ok_or_else(|| ScriptError::Runtime("figure_draw: figure is closed".to_string()))?
        .redraw()
        .map_err(|e| ScriptError::Runtime(e.to_string()))?;
    Ok(Value::None)
}
```

### 2e. `figure_close(fig)` builtin

```rust
fn builtin_figure_close(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("figure_close", &args, 1)?;
    let Value::LiveFigure(fig) = &args[0] else {
        return Err(ScriptError::type_error("figure_close", "live_figure", args[0].type_name()));
    };
    // .take() replaces the Option with None, dropping the LiveFigure value,
    // which fires Drop::drop → disable_raw_mode + LeaveAlternateScreen.
    // Other clones of the Arc see None on their next lock.
    fig.lock().unwrap().take();
    Ok(Value::None)
}
```

> **Note on `figure_close` semantics:** `LiveFigure` uses `Drop` for terminal
> cleanup, so the terminal is always restored even if the script panics or
> the process receives SIGINT. Explicit `figure_close` is provided as a
> convenience for scripts that want to return to the normal terminal mid-script.
> The `Option` wrapper inside the `Mutex` makes explicit close correct:
> `.take()` drops the `LiveFigure` (firing `Drop`) without destroying the `Arc`
> that other value clones may still hold.

### 2f. Register all builtins

```rust
r.register("figure_live",  builtin_figure_live);
r.register("plot_update",  builtin_plot_update);
r.register("figure_draw",  builtin_figure_draw);
r.register("figure_close", builtin_figure_close);
```

### 2g. REPL help entries

```
"figure_live"  → "figure_live(rows, cols) — open persistent live terminal plot"
"plot_update"  → "plot_update(fig, panel, y) or plot_update(fig, panel, x, y) — update panel data"
"figure_draw"  → "figure_draw(fig) — redraw all panels to terminal"
"figure_close" → "figure_close(fig) — close live figure and restore terminal"
```

---

## Phase 3 — `mag2db`, Example Script, and Docs

**Status: complete**

### 3a. `mag2db(X)` builtin

- **File:** `crates/rustlab-script/src/eval/builtins.rs`
- **Formula:** `20 * log10(max(|x|, 1e-10))` applied element-wise.
- The `1e-10` floor maps silence to −200 dB — well below any audible signal.
  This prevents `-inf` from appearing in the output and crashing the chart
  renderer's axis scaling.

```rust
fn builtin_mag2db(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("mag2db", &args, 1)?;
    match &args[0] {
        Value::Scalar(x)   => Ok(Value::Scalar(20.0 * (x.abs().max(1e-10)).log10())),
        Value::Complex(c)  => Ok(Value::Scalar(20.0 * (c.norm().max(1e-10)).log10())),
        Value::Vector(v)   => {
            let out: CVector = v.iter()
                .map(|c| C64::new(20.0 * c.norm().max(1e-10).log10(), 0.0))
                .collect();
            Ok(Value::Vector(out))
        },
        Value::Matrix(m)   => {
            let out = m.map(|c| C64::new(20.0 * c.norm().max(1e-10).log10(), 0.0));
            Ok(Value::Matrix(out))
        },
        other => Err(ScriptError::type_error("mag2db", "numeric", other.type_name())),
    }
}
```

- **Register:** `r.register("mag2db", builtin_mag2db);`
- **REPL help:** `"mag2db(X) — convert magnitude to dB: 20·log10(|X|), floor at −200 dB"`
- **Test:** `mag2db(1.0) == 0.0`, `mag2db(0.0) == -200.0`, `mag2db(10.0) ≈ 20.0`.

### 3b. Example script — spectrum monitor

- **File:** `examples/audio/spectrum_monitor.rlab`

```r
# Real-time audio spectrum monitor
# Top panel: time-domain waveform
# Bottom panel: magnitude spectrum in dB (0–Nyquist)

sr        = 44100.0;
fft_size  = 1024;
half      = fft_size / 2;

h   = window(fft_size, "hann");
adc = audio_in(sr, fft_size);
fig = figure_live(2, 1);

while true
    frame = audio_read(adc);

    # Frequency analysis
    X     = fft(frame .* h);
    freqs = fftfreq(fft_size, sr);

    # Update both panels, then draw once (one refresh per frame)
    plot_update(fig, 1, frame);
    plot_update(fig, 2, freqs(1:half), mag2db(X(1:half)));
    figure_draw(fig);
end
```

- **File:** `examples/audio/spectrum_monitor_annotated.rlab` — same script with
  comments explaining windowing, why only the first half of the FFT is shown
  (Nyquist), the dB floor, and frame-rate arithmetic.

### 3c. Documentation updates

- **`docs/functions.md`:** Add sections for `figure_live`, `plot_update`,
  `figure_draw`, `figure_close`, and `mag2db`. Include the spectrum monitor
  example and explain the update/draw separation (why two calls per loop).

- **`docs/quickref.md`:** Add `Live Plotting` row to the capability table.

- **`AGENTS.md`:**
  - Add `live.rs` to the `rustlab-plot` section under Repository Layout.
  - Add `Value::LiveFigure` to the Value enum table.
  - Add all five new builtins to the builtins table.
  - Note the `Drop`-based terminal cleanup contract.

---

## Architectural Notes

### Why `update` and `draw` are separate calls

If `plot_update` triggered an immediate redraw, a two-panel loop would draw
twice per iteration — once with the new time-domain data and stale spectrum,
then again with both updated. This produces a visible flicker between the two
partial states. Explicit `figure_draw` means exactly one atomic screen refresh
per loop iteration.

### Why `Drop` handles terminal cleanup

The ratatui alternate screen must be exited cleanly or the user's terminal
is left in a broken state. Implementing cleanup in `Drop` means it fires in
all exit paths: normal script completion, `figure_close`, a runtime error,
and Ctrl-C (SIGINT terminates the process and Rust runs destructors for
locals on the current thread before exit).

### Reuse of `SubplotState`

`LiveFigure` stores `Vec<SubplotState>` — the same type used by the existing
`FigureState`. The `draw_subplots` helper extracted in Phase 1a renders both.
No duplication of chart logic.

### What live plots cannot do (scope boundaries)

- **Color/style control per panel:** `plot_update` always uses the default
  color cycle. Per-series color control is a future extension.
- **Zoom / scroll / interactivity:** The live figure is display-only. No
  keyboard interaction while running (Ctrl-C exits the whole script).
- **File export:** `figure_draw` renders to terminal only. Saving a snapshot
  to PNG from a live figure is a future extension.

---

## Dependency on Audio Streaming Plan

Phases 1 and 2 of this plan have **zero dependency** on the audio streaming
plan. `LiveFigure` is general-purpose and works with any data source:

```r
# Animate a sine wave with no audio at all
fig = figure_live(1, 1);
t = linspace(0, 2*pi, 256);
for phase = linspace(0, 2*pi, 100)
    plot_update(fig, 1, sin(t + phase));
    figure_draw(fig);
end
figure_close(fig);
```

The spectrum monitor example in Phase 3 requires `audio_in` and
`audio_read` from the audio streaming plan, but that example can be deferred
until those builtins exist.

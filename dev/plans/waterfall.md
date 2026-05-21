# Development Plan: Frequency Waterfall plot

**Target use case:** classic SDR-style waterfall — a live frequency-vs-amplitude
line spectrum on top, sharing its x-axis with a downward-scrolling
spectrogram heatmap below (newest row at the top, oldest scrolling off the
bottom). Works in both the ratatui TUI and `rustlab-viewer`, and is
drivable from `audio_in` or any frame source via a streaming surface that
matches the existing `stft_stream` / `plot_update_heatmap` pattern.

**Status:** proposed, awaiting approval.

---

## Why a new plot type

`spectrogram` already exists and renders a `[n_freqs × n_frames]` heatmap
that scrolls leftward when driven through `stft_stream` + `plot_update_heatmap`.
A waterfall is the same data, transposed and oriented for a different
ergonomic goal:

| Aspect | `spectrogram_monitor.rlab` | `waterfall(...)` |
|---|---|---|
| Time axis | x (horizontal) | y (vertical, downward) |
| Scroll direction | left (oldest off-screen) | down (oldest off-screen) |
| Companion panel | none | live amplitude spectrum on top |
| Frequency axis shared with line plot | no | yes (column-aligned) |
| Convention | physics/`Xy` (low freq at bottom) | image/`Ij` (newest at top) |

The shared frequency axis between the live spectrum and the recent
history is the whole point — burst/peak correlation between "now" and
"a few seconds ago" becomes a vertical eye-trace down a single column.

## Confirmed design choices

(From the proposal conversation, 2026-05-21.)

1. **Top panel:** line plot (not stem) — `PlotKind::Line`, log-amplitude
   via dB (no separate log-y mode needed).
2. **History units:** seconds (`time_history = 5.0`), matching
   `examples/audio/spectrogram_monitor.rlab:32`.
3. **API shape:** combined call — `waterfall_stream(samples, fig, state)`
   updates both subplots and redraws in one call.

## API

Three new builtins, registered in `crates/rustlab-script/src/eval/builtins.rs`
adjacent to the existing `stft` / `spectrogram` registrations
(builtins.rs:75-82):

```matlab
% Offline (one-shot): renders 2-row figure or returns data
waterfall(x, fs)
waterfall(x, fs, window, noverlap, nfft)
[W, f, t] = waterfall(x, fs, ...)     % W is [n_time × n_freqs] in dB

% Streaming
state = waterfall_stream_init(fs, window, noverlap, nfft, time_history)
state = waterfall_stream_init(fs, window, noverlap, nfft, time_history, opts)
[fig, state] = waterfall_stream(samples, fig, state)
```

`opts` is an optional struct (later phase — Phase 1 lands with positional
args only):
- `vmin_db` (default −100), `vmax_db` (default 0)
- `colormap` (default `"viridis"`)
- `smooth_frames` (top-panel rolling average, default 1)
- `update_every` (redraw decimation, default 4)

The combined-call form keeps user code as short as the existing
spectrogram example:

```matlab
state = waterfall_stream_init(sr, window("hann", nfft), noverlap, nfft, 5.0);
fig   = figure_live(2, 1);
adc   = audio_in(sr, frame);
while true
    samples = audio_read(adc);
    [fig, state] = waterfall_stream(samples, fig, state);
end
```

## Data model

Internal display matrix `W` is `[n_time × n_freqs]`:
- rows = time (row 0 = newest), cols = frequency (col 0 = DC, col n = Nyquist)
- `n_time = ceil(time_history * fs / hop)` where `hop = nfft − noverlap`
- new STFT columns from `stft_stream` are transposed and pushed onto the
  top; the bottom is trimmed. Internally implemented as a `VecDeque<Vec<f64>>`
  of rows so push-front + truncate-back are O(1) amortised.

The top panel's spectrum is the most recent column of the underlying STFT
output (or, when `smooth_frames > 1`, the mean of the last m columns).

## Rendering

**Layout:** `figure_live(2, 1)`. Panel 0 = line plot (spectrum), panel 1 =
heatmap (waterfall). Both pinned to the same x-limits `(0, fs/2)` via
`set_panel_limits`.

**Downward scrolling.** Heatmap origin needs to be `Ij` (row 0 at top) for
this panel only — opposite of the `Xy` default that `update_panel_heatmap`
currently hardcodes (live.rs:201, viewer_live.rs analogue). Two options:

- **(A, chosen)** Extend `LivePlot::update_panel_heatmap` with an
  `origin: HeatmapOrigin` argument. Existing callers pass `Lower` (current
  behaviour); waterfall passes `Upper`. The y-axis tick labels are set
  via the existing `HeatmapData::y_labels` field to show seconds-ago
  (`0.0`, `−1.0`, `−2.0`, …).
- (B, rejected) Build `W` upside-down and keep `Xy`. Visually identical,
  but the data form `[W, f, t] = waterfall(...)` would have row 0 = oldest,
  which is confusing for downstream code.

**TUI** (`crates/rustlab-plot/src/ascii.rs::render_heatmap_tui`): existing
block-character renderer; the `origin` flag controls iteration order.

**Viewer** (`crates/rustlab-plot/src/viewer_live.rs`): the existing
`render_heatmap_cells_to_rgba` already places rows; flipping based on
`origin` is a single conditional. Viewer renders both panels in its native
egui_plot grid; no new viewer-side wire-protocol changes needed.

## Phasing

**Phase 1 — DSP + offline builtin + tests.** Land `waterfall(x, fs, …)`
returning `[W, f, t]` (offline only), no rendering yet. New unit tests in
`crates/rustlab-dsp/src/tests.rs` (or wherever `stft` tests live)
validating that `W` is the column-transpose of `stft(x, fs, …)` with newest
column on row 0, and a property test that `n_time` rows match the
`ceil(time_history * fs / hop)` formula.

**Phase 2 — Origin flag on live heatmaps.** Extend `LivePlot` trait + both
implementations (TUI and viewer). Add `HeatmapOrigin::{Lower, Upper}` to
`crates/rustlab-plot/src/figure.rs`. Default = `Lower` so existing
`spectrogram_monitor.rlab` keeps working unchanged. Unit test: render a
tiny matrix top-down vs bottom-up and compare cell ordering.

**Phase 3 — Streaming builtins.** `waterfall_stream_init` returning an
opaque state handle (wrapping `StftState` + `VecDeque` row buffer);
`waterfall_stream` advancing it, updating both subplots, and redrawing.
Implementation in `crates/rustlab-dsp/src/welch_stream.rs` alongside
`StftState`. Tests in `crates/rustlab-script/src/tests.rs` exercising the
streaming builtin with synthetic frames.

**Phase 4 — Example + acceptance.** `examples/audio/waterfall_monitor.rlab`
+ `.sh` wrapper mirroring `spectrogram_monitor.{rlab,sh}`. Acceptance test
in `crates/rustlab-cli/tests/examples.rs` running the example over a
short canned WAV file and checking it exits cleanly (matches the existing
spectrogram-monitor test).

## Files touched

| File | Phase | Change |
|---|---|---|
| `crates/rustlab-dsp/src/stft.rs` *(or new `waterfall.rs`)* | 1 | offline `waterfall()` helper |
| `crates/rustlab-dsp/src/tests.rs` | 1 | unit tests for offline waterfall |
| `crates/rustlab-plot/src/figure.rs` | 2 | `HeatmapOrigin` enum + `HeatmapData::origin` field |
| `crates/rustlab-plot/src/live.rs` | 2 | honour origin in `update_panel_heatmap` |
| `crates/rustlab-plot/src/viewer_live.rs` | 2 | honour origin in RGBA renderer |
| `crates/rustlab-plot/src/ascii.rs` | 2 | honour origin in `render_heatmap_tui` |
| `crates/rustlab-plot/src/lib.rs` (`LivePlot` trait) | 2 | extra trait-method arg |
| `crates/rustlab-dsp/src/welch_stream.rs` | 3 | `WaterfallState` wrapping `StftState` + row buffer |
| `crates/rustlab-script/src/eval/builtins.rs` | 1, 3 | three builtins + `r.register` calls |
| `crates/rustlab-script/src/tests.rs` | 3 | streaming-builtin tests |
| `crates/rustlab-cli/src/commands/repl.rs` | 1, 3 | `HelpEntry` × 3, add to spectral category row (~line 1121) |
| `examples/audio/waterfall_monitor.rlab` | 4 | example script |
| `examples/audio/waterfall_monitor.sh` | 4 | shell wrapper |
| `crates/rustlab-cli/tests/examples.rs` | 4 | acceptance test |
| `AGENTS.md` | each phase | function table + workflow notes |
| `docs/quickref.md` | each phase | spectral section additions |

## Trade-offs / open risks

- **`LivePlot` trait signature change** is a breaking change for any
  out-of-tree implementor. None known; both implementors live in
  `rustlab-plot`. Risk: low. Alternative: add a second trait method
  `update_panel_heatmap_oriented` and have the old one delegate; rejected
  as cruft.
- **VecDeque vs Vec<Vec<f64>>:** chose `VecDeque` for O(1) push-front;
  conversion to the `Vec<Vec<f64>>` that `HeatmapData::z` expects requires
  one allocation per redraw. At 11 Hz redraws and a few hundred rows this
  is negligible. If profiling later shows it, the rendering layer can be
  taught to accept `&[&[f64]]`.
- **Top-panel y-range:** auto-scaling per frame will jitter visually. Pin
  it to `[vmin_db, vmax_db]` (the same clip range as the heatmap) so the
  spectrum stays visually anchored to the colourmap. Trade-off:
  out-of-range peaks clip silently. Acceptable — matches every SDR tool.
- **No `freq_log` flag in Phase 1.** Audio waterfalls usually want log-x;
  defer to a follow-up since both panels would need coordinated log-x and
  the viewer + TUI log-axis code is currently per-line-plot only.

## Out of scope

- 3-D mesh waterfalls (`surf` over the W matrix). Different code path; can
  follow if requested.
- Per-column peak hold / decay on the top panel.
- Frequency cursors / markers shared between panels.
- HTML/Plotly export form of `waterfall(...)` — Phase 1 returns data;
  rendering via the existing `figure_live` path. Static HTML export can
  follow.

## Workflow compliance

Per `feedback_workflow.md` rules, each phase ships:
- tests in the same commit (Rule 2)
- `AGENTS.md` updates (Rule 4)
- `docs/quickref.md` updates (Rule 5)
- `HelpEntry` + category-row entry for every new builtin (Rule 6)
- commit only after explicit user approval (Rule 3)

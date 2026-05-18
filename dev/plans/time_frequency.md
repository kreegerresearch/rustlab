# Development Plan: Time-Frequency Analysis (pwelch + STFT + CWT + streaming)

**Target use case:** rounding out rustlab's signal-identification plotting
with the standard time-frequency toolkit — Welch power spectral density
estimation, Short-Time Fourier Transform with spectrogram visualization,
Continuous Wavelet Transform with scalogram visualization, and a
streaming surface (state-machine builtins + live heatmap panels in
`rustlab-viewer`) so all three can drive realtime displays from
`audio_in` or any other frame source.

**Current phase:** all four phases complete (Phases 1–4 shipped 2026-05-17)
**Status:** complete

---

## Overview

Four related capabilities that share most of their plumbing:

1. **`pwelch(x, fs, ...)`** — Welch's method for power spectral density
   estimation. The natural next step beyond the periodogram and
   Hann-windowed periodogram already demonstrated in
   `examples/notebooks/spectral_estimation.md`. Segments the signal into
   overlapping pieces, windows each, computes per-segment periodograms,
   averages them. Trades frequency resolution for variance reduction.

2. **`stft(x, fs, ...)` / `spectrogram(x, fs, ...)`** — Short-Time Fourier
   Transform. Same segment-window-FFT loop as pwelch but *keeps* every
   per-segment spectrum rather than averaging, producing a 2-D
   `[n_freqs × n_frames]` matrix. `spectrogram` is `stft` + a heatmap
   render (`imagesc` of the magnitude in dB).

3. **`cwt(x, fs, ...)` / `scalogram(x, fs, ...)`** — Continuous Wavelet
   Transform. Convolves the signal with scaled/shifted versions of a mother
   wavelet (Morlet by default) to give a `[n_scales × n_samples]`
   time-frequency matrix. `scalogram` is `cwt` + heatmap render of `|W|`
   in dB (matches spectrogram colour scale).

4. **Streaming variants** — `pwelch_stream(frame, state)`,
   `stft_stream(frame, state)`, `cwt_stream(frame, state)`. Mirror the
   `filter_stream` + `state_init` pattern: state is an `Arc<Mutex<...>>`
   handle held across frames, output is the incremental analysis for the
   samples just consumed. Pairs with a new `plot_update_heatmap` live
   plot builtin that drives `rustlab-viewer` (and the ratatui fallback)
   for realtime spectrograms / scalograms.

Why bundle: pwelch and stft share the segment-iteration infrastructure
(`hop = win_len − noverlap`, padding, windowing). cwt is mathematically
different but follows the same `2-D matrix → imagesc → heatmap` plotting
pattern that already works across every rustlab backend. And all three
streaming variants share one `SegmentBuffer` ring helper, one
`DspStreamState` value variant, one `*_stream_init` constructor pattern.
One coherent plan, four phases.

---

## Architecture: keeping rustlab small

A guiding constraint per [[feedback_rustlab_binary_size]] and the
broader "offload heavy plotting" preference: **no new rendering code
in the main `rustlab` CLI binary.** Concretely:

- **Math** (pwelch, stft, cwt, streaming variants) lands in `rustlab-dsp`
  (library crate). No CLI surface.
- **Script builtins** (`pwelch`, `stft`, `spectrogram`, `cwt`,
  `scalogram`, `plot_update_heatmap`, six streaming) live in
  `rustlab-script` (library; the main CLI links it). Each builtin is a
  thin wrapper — compute, then call the existing `imagesc` /
  `LivePlot` / `plot_labels` infrastructure. **Target: ~30 lines per
  builtin, no rendering logic.**
- **Heatmap rasterization** (matrix → RGBA / SVG / PNG / terminal cells)
  stays in `rustlab-plot`. We extend the existing `imagesc` rasterizer
  with a `vmin`/`vmax` clip path (DRY — once, used by spectrogram +
  scalogram + any future heatmap), not duplicate it inside the
  builtins.
- **Live display** (egui window, mouse interaction) is `rustlab-viewer`'s
  job. The `PanelHeatmap` wire message already exists end-to-end; the
  viewer binary needs **zero changes** for Phase 4.
- **Notebook static renders** (SVG/PNG/HTML embedded in `time_frequency.md`)
  flow through the existing `rustlab-notebook render` path. No new
  notebook subcommands.

Net new code in the main CLI binary: zero. New `rustlab-plot` surface:
one `db_clip` helper and one `LivePlot::update_panel_heatmap` trait
method (one method each in two impls). All actual math goes in
`rustlab-dsp`; all script glue goes in `rustlab-script`.

---

## Design decisions (locked in)

Decisions below are confirmed; recorded here as a permanent decision log
rather than open questions. MATLAB-compatible defaults except where a
deliberate divergence is noted.

1. **Wavelet families: Morlet only.** **Diverges from MATLAB default**
   (`'morse'` since R2018a, a 3-parameter family). Morlet matches
   MATLAB's classic `'amor'` and pywt's default. Sticking with one
   family keeps Phase 3 scoped; adding Mexican-hat / Gaussian-derivative
   later is additive.

2. **One-sided vs two-sided PSD: auto-detect.** One-sided for real
   input, two-sided for complex. Explicit `"onesided"` / `"twosided"`
   override is accepted as a trailing arg. **Matches MATLAB pwelch.**

3. **Default window: Hamming.** **Matches MATLAB pwelch.** Note: the
   existing `examples/notebooks/spectral_estimation.md` uses Hann in
   its periodogram demos; the pwelch section there will show Hamming
   as the default and call out the difference as a teaching point.

4. **Default overlap: 50%.** Matches MATLAB.

5. **Segment length default.** For pwelch: `L = floor(2·length(x) / 9)`
   — exact MATLAB formula for 8 segments at 50% overlap (the
   round-number `length(x)/8` would give 9 segments, off by ~12.5%).
   For stft: **128 samples** (MATLAB default), not 256. Users wanting
   higher freq resolution pass an explicit length / vector window.

6. **Detrending: none.** Matches MATLAB pwelch. Diverges from scipy
   (`detrend='constant'`). Users wanting detrending pass
   `pwelch(x - mean(x), ...)`. The "DC → bin 0" test locks the choice
   in.

7. **String-window length resolution.** When `window` is a string,
   segment length = the default from decision 5. When `window` is a
   `Vector`, length = its length. **Deliberate divergence from MATLAB**
   (MATLAB requires a precomputed vector); we add string names as a
   usability nicety.

8. **ISTFT out of scope.** Has its own correctness criterion (perfect
   reconstruction under COLA). Fits a future plan.

9. **Reassignment / multitaper / Wigner-Ville out of scope.**

10. **`pwelch_stream` averaging mode: cumulative default + optional
    EMA.** Cumulative converges to the batch `pwelch_psd` (gives the
    convergence regression test). EMA is opt-in via a 5th init arg:
    `pwelch_stream_init(fs, window, noverlap, nfft, ema_alpha)`.

11. **Streaming warmup: empty return.** Before enough samples for a
    complete segment arrive, the stream functions return empty
    matrices/vectors. Caller checks `length(Pxx) == 0` or
    `size(S, 2) == 0`. Honest signal; matches `filter_stream`'s
    no-output-on-short-frames behaviour.

12. **`stft_stream` empty shape: `n_freqs × 0`.** Lets the caller
    always read freq-bin count from `size(S, 1)` regardless of whether
    new columns arrived this frame.

13. **`cwt_stream` edge effects: not trimmed.** Returns the full
    sliding-window CWT over the last `N` samples. The rightmost
    `~half_max_scale_support` columns have edge effects (same as the
    *ends* of batch CWT output). Documented in the function help.
    KISS choice: trimming would add a latency contract and complicate
    the API for a visual artefact users can already see.

14. **`DspStreamState` dispatch.** Single `Value::DspStreamState`
    variant wrapping an internal `enum StreamKind { Pwelch(_), Stft(_),
    Cwt(_) }`. One Value variant; exhaustive dispatch in builtins.

15. **`plot_update_heatmap` axes: caller-set via existing
    `plot_limits`.** No auto-axis magic. Mirrors how `plot_update`
    doesn't set its own axes either.

---

## Data model decisions (apply to all four phases)

### Output shapes

| Feature | Single-return form | Multi-return form |
|---|---|---|
| `pwelch` | real `Vector` (Pxx) | `[Pxx, f] = pwelch(x, fs, ...)` |
| `stft` | complex `Matrix` (S) | `[S, f, t] = stft(x, fs, ...)` |
| `cwt` | complex `Matrix` (W) | `[W, freqs, t] = cwt(x, fs, ...)` |
| `pwelch_stream` | — | `[Pxx, state] = pwelch_stream(frame, state)` |
| `stft_stream` | — | `[S_cols, state] = stft_stream(frame, state)` |
| `cwt_stream` | — | `[W, state] = cwt_stream(frame, state)` |

Matrix layout for STFT/CWT: **rows = frequencies (low at row 1), cols =
time samples (early at col 1)**. This is what `imagesc` already plots
naturally — time on x-axis, frequency on y-axis.

**Deliberate departure from `spectrum`.** The existing `spectrum(x, fs)`
builtin returns a 2×n matrix (row 0 = Hz, row 1 = complex spectrum).
The new builtins return tuples instead, matching the
destructuring-assignment convention used by `bode`, `eig`,
`stabilitymu`. `spectrum`'s 2×n shape is the outlier and we don't
propagate it.

**Streaming state is an `Arc<Mutex<...>>` handle.** Returned `state` is
the same handle that was passed in — not a copy. The
`[Pxx, state] = pwelch_stream(...)` rebinding is script-language
sugar, identical to how `filter_stream` works today.

### Auto-plot when not assigned

Per the rustlab pattern used by `bode` / `nyquist` / `rfplot`: bare
calls auto-render. `spectrogram` / `scalogram` are dedicated plot-only
wrappers (no data return). Streaming variants (`*_stream`) always
require capture — they have no batch "plot the whole signal"
interpretation.

### Windowing convention

Same `WindowFunction` enum already used by `window()` and the FIR
designers. Accept either:

- A string name: `"hann"`, `"hamming"`, `"rectangular"`, `"blackman"`, `"kaiser"`.
- A precomputed coefficient `Vector` — its length is the segment size.

For string windows the segment length comes from the default (decision
5); `nfft` (when larger) implies zero-padding for finer frequency
resolution.

### Shared helpers (DRY)

| Helper | Crate / location | Used by |
|---|---|---|
| `segment_iter(n, win_len, noverlap)` | `rustlab-dsp::welch` (crate-private) | `pwelch_psd`, `stft` |
| `SegmentBuffer<T>` (ring buffer + hop-boundary callback) | `rustlab-dsp::welch_stream` (crate-private) | `pwelch_stream`, `stft_stream` |
| `db_clip(matrix, floor_db)` (returns `(matrix_db, vmin, vmax)`) | `rustlab-plot` (public) | `spectrogram`, `scalogram`, streaming display path, future heatmap users |
| `imagesc(..., vmin, vmax)` extension (optional limits) | `rustlab-plot` (existing call, new optional args) | All heatmap-bearing builtins |
| `LivePlot::update_panel_heatmap` | `rustlab-plot` trait (new method) | `plot_update_heatmap` |

---

## Phase 1 — pwelch (Welch's PSD estimator)

**Status:** complete (2026-05-17)
**Goal:** ship the simplest of the three batch features end-to-end so
the sliding-window infrastructure has a battle-tested home before STFT
extends it.

### 1a. `dsp::welch` math module

- **New file:** `crates/rustlab-dsp/src/welch.rs`
- **Crate-private helper** (used by Phase 2 too):
  ```rust
  pub(crate) fn segment_iter(
      n: usize,
      win_len: usize,
      noverlap: usize,
  ) -> impl Iterator<Item = (usize, usize)>;
  // Yields (start, end) sample indices, hop = win_len - noverlap.
  // Debug-asserts hop > 0 (caller validates noverlap < win_len).
  ```
- **Public signature:**
  ```rust
  pub fn pwelch_psd(
      x: &CVector,        // accepts real or complex; auto-detects per decision 2
      fs: f64,
      window: &RVector,
      noverlap: usize,
      nfft: usize,
      sided: Sided,       // OneSided / TwoSided / Auto
  ) -> Result<(RVector /* Pxx */, RVector /* f */), DspError>;
  ```
- **Algorithm:**
  1. `hop = window.len() - noverlap`; segments via `segment_iter`.
  2. No detrending (decision 6).
  3. For each segment, multiply by window, zero-pad to `nfft`, FFT.
  4. Two-sided per-segment periodogram: `|X|² / (fs · ∑w²)`. Average
     across segments.
  5. If `Auto`: one-sided when input is real, two-sided when complex.
  6. One-sided folding:
     - **Even `nfft`:** keep bins 0..nfft/2, double bins 1..nfft/2−1.
     - **Odd `nfft`:** keep bins 0..(nfft−1)/2, double bins 1..(nfft−1)/2.
- **Validation:** `window.len() <= len(x)`; `noverlap < window.len()`;
  `nfft >= window.len()`.
- **Tests** in `crates/rustlab-dsp/src/tests.rs`:
  - Unit-amplitude sine: integrated PSD = 0.5 (sine power).
  - White noise: approximately flat across bins.
  - Constant signal: all power in bin 0 (locks detrending=none).
  - Even and odd `nfft` total-power conservation under folding.
  - Real input auto-detects one-sided; complex input auto-detects
    two-sided.

### 1b. `pwelch` script builtin

- **File:** `crates/rustlab-script/src/eval/builtins.rs`
- **Signatures:**
  - `pwelch(x, fs)` — defaults: Hamming window of length
    `floor(2·length(x)/9)`, 50% overlap, `nfft = window length`,
    sided = auto.
  - `pwelch(x, fs, window)` — string name or `Vector`.
  - `pwelch(x, fs, window, noverlap)` — overlap as sample count.
  - `pwelch(x, fs, window, noverlap, nfft)` — `nfft` zero-pads.
  - `pwelch(x, fs, window, noverlap, nfft, sided)` — `"onesided"` /
    `"twosided"`.
  - `[Pxx, f] = pwelch(...)` — capture, no plot.
  - `pwelch(x, fs)` (no capture) — auto-plot dB PSD vs frequency.
- **Tests:**
  - Two-tone signal: pwelch identifies both tones with > 20 dB SNR.
  - Hamming-windowed pwelch has lower sidelobes than rectangular.
  - Multi-return destructuring works.
  - String-window vs Vector-window forms produce equal output.

### Phase-1 deliverables checklist

- [x] `dsp::welch::pwelch_psd` + `segment_iter` with the math tests.
- [x] `pub use welch::{pwelch_psd, default_segment_len, Sided};` re-export.
- [x] `pwelch` script builtin (6 calling forms).
- [x] REPL `HelpEntry` in the existing `"DSP"` category.
- [x] `docs/functions.md` / `docs/quickref.md` / `AGENTS.md` updates.
- [x] Example `examples/spectral/pwelch.rlab`.
- [x] Extended `examples/notebooks/spectral_estimation.md` with a
      Welch's Method section.
- [x] `cargo test --workspace` green (11 dsp tests + 5 script tests).

---

## Phase 2 — STFT + spectrogram

**Status:** complete (2026-05-17)
**Depends on:** Phase 1 (`segment_iter` was built there)

### 2a. `dsp::stft` math module

- **New file:** `crates/rustlab-dsp/src/stft.rs`
- **Signature:**
  ```rust
  pub fn stft(
      x: &CVector,
      fs: f64,
      window: &RVector,
      noverlap: usize,
      nfft: usize,
      sided: Sided,
  ) -> Result<(CMatrix /* S */, RVector /* f */, RVector /* t */), DspError>;
  ```
- **Algorithm:**
  1. Reuse `segment_iter`.
  2. For each segment: window, zero-pad to `nfft`, FFT.
  3. Stack column-by-column into `S`.
  4. `t[k] = (k·hop + win_len/2) / fs`.
  5. `f` from `fftfreq(nfft, fs)`, folded if one-sided.
- **Tests:**
  - Pure tone: magnitude concentrated in correct bin across frames.
  - Linear chirp: peak-bin per frame tracks instantaneous frequency.
  - Shape correctness for one-sided even, one-sided odd, two-sided.

### 2b. `stft` and `spectrogram` script builtins

- `stft(x, fs)` / `stft(x, fs, window, noverlap, nfft, sided)` —
  default window length **128** (decision 5); capture returns
  `[S, f, t]`; bare call auto-renders.
- `spectrogram(x, fs)` / `spectrogram(x, fs, window, noverlap, nfft)`
  — dedicated plot wrapper. Calls **shared `db_clip` helper** in
  `rustlab-plot` (DRY: same logic used by `scalogram` and any future
  heatmap user) with floor = 80 dB, then `imagesc` with `viridis`,
  then `axis("xy")` and `plot_labels`. **No dB-conversion code lives
  in the builtin itself** — it's a thin compose.
- **Tests:**
  - Calling-form acceptance.
  - Shape: heatmap dimensions match `[nfreqs, nframes]`.
  - dB-clipping behaviour pinned by `db_clip` unit tests in
    `rustlab-plot`, not duplicated here.

### 2c. Cross-backend rendering

Spectrogram is just `imagesc`, which already works across every
backend (SVG, PNG, HTML/Plotly, terminal, viewer, LaTeX/PDF). New
tests pin one heatmap output per **file** backend (SVG / HTML / PNG);
terminal and viewer paths are covered by existing `imagesc` and
`figure_live` tests.

### Phase-2 deliverables checklist

- [x] `dsp::stft::stft` with 6 math tests.
- [x] `pub use stft::stft;` re-export.
- [x] `db_clip` helper in `rustlab-plot` + 5 unit tests.
- [x] `stft` / `spectrogram` script builtins (thin composes via
      shared `resolve_stft_args` + `push_db_heatmap` helpers).
- [x] REPL `HelpEntry` in existing `"DSP"` category.
- [x] Docs / quickref / AGENTS updates.
- [x] `examples/spectral/spectrogram_chirp.rlab`.
- [x] New notebook `examples/notebooks/time_frequency.md`.
- [x] Gallery entry in `gallery/README.md`; cross-link from
      `spectral_estimation.md`.
- [x] Backend tests: SVG / HTML / PNG spectrogram outputs.
- [x] `cargo test --workspace` green (6 dsp + 5 plot + 8 script tests).

---

## Phase 3 — CWT + scalogram

**Status:** complete (2026-05-17)
**Depends on:** Phase 2 (shares `db_clip` and heatmap rendering)

### 3a. Morlet wavelet

- **New file:** `crates/rustlab-dsp/src/wavelet.rs`
- **Morlet (complex):**
  ```
  ψ(t) = π^(-1/4) · exp(j·ω₀·t) · exp(-t²/2)
  ```
  with ω₀ = 6 (canonical choice).

### 3b. CWT math

- **Signature:**
  ```rust
  pub fn cwt_morlet(
      x: &CVector,
      fs: f64,
      scales: &RVector,
  ) -> Result<(CMatrix /* W */, RVector /* freqs */, RVector /* t */), DspError>;
  ```
- **Algorithm (frequency-domain):**
  1. Pad `x` to `next_pow2(len(x) + max_scale · wavelet_support)`
     with zeros; trim after IFFT. Prevents circular wrap from
     creating phantom energy.
  2. FFT padded `x` once.
  3. For each scale `s`: build `Ψ_s*(ω)` analytically (Morlet's FT
     is a shifted Gaussian), multiply, IFFT, trim.
  4. Stack rows into `W`.
  5. `freqs[i] = ω₀ / (2π · scales[i] · dt)`.
- **Default scale grid:** log-spaced from 2 samples to `len(x)/4`,
  64 scales.

### 3c. `cwt` and `scalogram` script builtins

- `cwt(x, fs)` / `cwt(x, fs, "morlet")` /
  `cwt(x, fs, "morlet", n_scales)` /
  `cwt(x, fs, "morlet", scales_vector)`. Returns `[W, freqs, t]`;
  bare call auto-plots.
- `scalogram(x, fs)` — dedicated plot wrapper. Thin compose: call
  `db_clip` (shared helper), then `imagesc` with `viridis`, then
  `axis("xy")`, then set log-frequency y-axis via existing
  `semilogy`-style plumbing.
- **Tests:**
  - Gaussian-modulated impulse localised at `(t₀, f₀)`.
  - Two tones resolved as horizontal bands.
  - Shape: `W.nrows() == n_scales`, `W.ncols() == len(x)`.
  - Energy ratio: two tones ≈ 2× one tone (no admissibility-constant
    magic numbers).

### Phase-3 deliverables checklist

- [x] `dsp::wavelet::cwt_morlet` (with edge padding) and 7 math tests.
- [x] `pub use wavelet::{cwt_morlet, default_scales};` re-export.
- [x] `cwt` / `scalogram` script builtins (thin composes, reuse
      `db_clip` + `push_db_heatmap` from Phase 2).
- [x] REPL `HelpEntry` in existing `"DSP"` category.
- [x] Docs / quickref / AGENTS updates.
- [x] `examples/spectral/cwt_chirp.rlab`.
- [x] Extended `time_frequency.md` with CWT + scalogram section.
- [x] `cargo test --workspace` green (7 dsp + 6 script tests).

---

## Phase 4 — Streaming (state-machine + live viewer)

**Status:** complete (2026-05-17)
**Depends on:** Phases 1–3 (math primitives) and the existing
`rustlab-viewer` IPC surface (`PanelHeatmap` wire message already
exists; we extend the `LivePlot` trait to drive it from script).

### 4a. LivePlot trait extension — heatmap streaming

Existing trait (`crates/rustlab-plot/src/lib.rs:74`) is line-plot only.
Add one method:

```rust
fn update_panel_heatmap(
    &mut self,
    idx: usize,
    matrix: &rustlab_core::RMatrix,
    colormap: &str,
    vmin: Option<f64>,
    vmax: Option<f64>,
);
```

Two implementations:

- **`LiveFigure` (ratatui, `crates/rustlab-plot/src/live.rs`)**: render
  matrix → coloured-cell terminal output via existing `colormap_rgb`.
- **`ViewerFigure` (`crates/rustlab-plot/src/viewer_live.rs`)**:
  rasterize matrix → RGBA via the existing `imagesc` rasterizer; ship
  as `ViewerMsg::PanelHeatmap`. **Wire message already exists; viewer
  binary unchanged.**

### 4b. `plot_update_heatmap` script builtin

```text
plot_update_heatmap(fig, panel, matrix)
plot_update_heatmap(fig, panel, matrix, colormap)
plot_update_heatmap(fig, panel, matrix, colormap, vmin, vmax)
```

Same shape as `plot_update`. 1-based panel index, error if the
`LiveFigure` handle is closed, no-op if matrix is empty. Axes are set
by the caller via the existing `plot_limits` (decision 15).

### 4c. Streaming DSP math primitives

New file `crates/rustlab-dsp/src/welch_stream.rs`:

```rust
// Single internal state enum; one Value::DspStreamState variant
// wraps Arc<Mutex<StreamKind>>.
pub(crate) enum StreamKind {
    Pwelch(PwelchState),
    Stft(StftState),
    Cwt(CwtState),
}

// Shared ring buffer + hop-boundary helper (DRY: pwelch + stft).
pub(crate) struct SegmentBuffer { /* window, ring, write_pos */ }

pub fn pwelch_stream(frame: &CVector, kind: &mut StreamKind) -> RVector;
pub fn stft_stream  (frame: &CVector, kind: &mut StreamKind) -> CMatrix;
pub fn cwt_stream   (frame: &CVector, kind: &mut StreamKind) -> CMatrix;
```

Design notes:

- **`pwelch_stream`** holds a `SegmentBuffer` plus a running PSD
  accumulator. **Default: cumulative average** (true PSD, converges
  to batch). **EMA optional** via `ema_alpha ∈ (0, 1]` passed to
  `pwelch_stream_init`; when set, the running average is
  `Pxx ← α·new + (1−α)·Pxx`. Decision 10.
- **`stft_stream`** holds a `SegmentBuffer`; emits zero or more new
  spectrogram columns per frame. Empty return is `n_freqs × 0`.
- **`cwt_stream`** holds a fixed-length signal ring buffer (default
  `2 · max_scale_support`, capped to a sensible upper limit) and
  recomputes the CWT over the window each frame. Edge effects on the
  rightmost columns are not trimmed (decision 13).
- All three: empty return during warmup (decision 11).
- Init constructors:
  - `pwelch_stream_init(fs, window, noverlap, nfft)` — cumulative.
  - `pwelch_stream_init(fs, window, noverlap, nfft, ema_alpha)` — EMA.
  - `stft_stream_init(fs, window, noverlap, nfft)`.
  - `cwt_stream_init(fs, n_samples, scales)`.

### 4d. Script builtins for streaming

- `pwelch_stream(frame, state)` → `[Pxx, state]`
- `stft_stream(frame, state)` → `[S_cols, state]`
- `cwt_stream(frame, state)` → `[W, state]`
- `pwelch_stream_init`, `stft_stream_init`, `cwt_stream_init`

All wrap math primitives behind a single new `Value::DspStreamState`
variant (decision 14).

### 4e. Tests

- **Math** (in `rustlab-dsp/src/tests.rs`):
  - `pwelch_stream` (cumulative) converges to batch `pwelch_psd`
    within 1 dB per bin after enough averaging.
  - `pwelch_stream` (EMA) tracks a stepwise frequency change with
    expected time constant.
  - `stft_stream` concatenated matches batch `stft` shape and
    per-column peak frequency.
  - `cwt_stream` central column matches `cwt_morlet` for a
    stationary tone (edge columns explicitly *not* checked).
- **Script** (in `rustlab-script/src/tests.rs`):
  - Each calling form for each builtin.
  - `plot_update_heatmap` headless no-op; ratatui round-trip.
  - Empty-warmup contract: `size(S_cols, 2) == 0` until first
    segment lands.
- **Viewer wire smoke test:** mocked `ViewerFigure` sink receives
  one `ViewerMsg::PanelHeatmap` per `plot_update_heatmap` call.

### Phase-4 deliverables checklist

- [x] `LivePlot::update_panel_heatmap` trait method + ratatui +
      viewer impls (the viewer impl rasterizes through the existing
      `render_panel_to_rgba` and sends `PanelHeatmap` — no proto bump).
- [x] `plot_update_heatmap` script builtin (3 calling forms) in
      existing `"Live Plotting"` REPL category.
- [x] `dsp::welch_stream` module: `SegmentBuffer` helper + two stream
      kinds (pwelch/stft); `CwtState` + `cwt_stream` co-located in
      `wavelet.rs` next to `cwt_morlet`.
- [x] `Value::DspStreamState` variant + `DspStreamKind` internal
      enum; `type_name`, Display, `whos_type` / `whos_preview` arms.
- [x] Six streaming script builtins (`pwelch_stream_init` /
      `pwelch_stream` / `stft_stream_init` / `stft_stream` /
      `cwt_stream_init` / `cwt_stream`) in existing `"Streaming DSP"`
      REPL category.
- [x] Docs / quickref / AGENTS updates.
- [x] `examples/audio/spectrogram_monitor.{sh,rlab}` — header
      updated to reflect Phase 4 having landed; uses the real API.
- [x] Extended `time_frequency.md` with a Streaming section
      (synthetic chirp → frame loop → assembled column count) plus
      pseudo-code for the live `audio_in` driver.
- [x] `cargo build --workspace --features viewer` clean.
- [x] `cargo test --workspace` green (9 dsp streaming-math tests +
      7 script streaming tests).

---

## Risk register

| Risk | Mitigation |
|---|---|
| **Window-overlap-FFT math subtle** | Unit-sine power integral = 0.5 and known-frequency tone tests catch the family of normalization mistakes at once. |
| **Detrending divergence from scipy** | Documented in decision 6, function docs, and help. DC-bin-0 test locks the choice in. |
| **One-sided folding wrong for odd `nfft`** | Explicit even/odd branches; one test per branch. |
| **One-sided/two-sided auto-detect surprises** | Decision 2 is MATLAB-faithful; explicit `"onesided"` / `"twosided"` overrides documented. |
| **Spectrogram y-axis convention** | `axis("xy")` inside `spectrogram` and `scalogram`; existing plumbing at `builtins.rs:3360`. |
| **CWT edge wrap from circular convolution** | Zero-pad before forward FFT; trim after IFFT. |
| **CWT streaming edge effects** | Not trimmed (decision 13). Documented in help text; users who care can trim themselves. |
| **Morlet ω₀ choice** | Hard-coded ω₀ = 6. Tunable parameter is additive if asked. |
| **`LivePlot` trait change ripples** | Two impls, both land in the same commit. Wire protocol already supports `PanelHeatmap` — no proto bump. |
| **Streaming PSD numerical drift over long sessions** | Cumulative form accumulates over millions of frames eventually. Kahan-style compensation if regression test flags drift; EMA mode sidesteps the issue entirely. |
| **rustlab CLI bloat** | Architecture section pins the rule: no rendering code in CLI. Builtins are ~30-line thin composes; all rasterization in `rustlab-plot`. |
| **Notebook discoverability gap** | Notebooks are per-phase deliverables, not deferred. |

---

## File / crate impact summary

| Crate | New files | Modified files |
|---|---|---|
| `rustlab-dsp` | `src/welch.rs`, `src/stft.rs`, `src/wavelet.rs`, `src/welch_stream.rs` | `src/lib.rs` (mod + re-exports), `src/tests.rs` |
| `rustlab-plot` | — | `src/lib.rs` (`LivePlot::update_panel_heatmap`; `db_clip` helper), `src/live.rs` (ratatui impl), `src/viewer_live.rs` (viewer impl) |
| `rustlab-proto` | — | **None — `PanelHeatmap` / `WireHeatmap` already exist.** |
| `rustlab-viewer` | — | **None — wire path already handled.** |
| `rustlab-script` | — | `eval/builtins.rs` (11 thin wrapper builtins), `eval/value.rs` (one new `DspStreamState` variant), `tests.rs` |
| `rustlab-cli` | — | `commands/repl.rs` — HelpEntries split across **existing** categories: `pwelch`/`stft`/`spectrogram`/`cwt`/`scalogram` → `"DSP"`; `plot_update_heatmap` → `"Live Plotting"`; six streaming builtins → `"Streaming DSP"`. No new categories. |
| Docs / examples | `examples/spectral/{pwelch,spectrogram_chirp,cwt_chirp}.rlab` (the `spectrogram_monitor.{sh,rlab}` audio demo already exists alongside this plan) | `docs/functions.md`, `docs/quickref.md`, `AGENTS.md`, `gallery/index.html`, `examples/notebooks/spectral_estimation.md`, `examples/notebooks/time_frequency.md` |

No new workspace dependencies. All math is pure Rust per workflow rule
10. Re-uses the existing `fft` / `ifft` / `WindowFunction` /
`imagesc` / `LivePlot` / `PanelHeatmap` infrastructure.

---

## Test strategy summary

- **Math anchors** (`rustlab-dsp/src/tests.rs`):
  - pwelch: unit-sine integral = 0.5; white-noise flatness;
    DC-bin-0; even/odd `nfft` folding; auto-sided dispatch.
  - STFT: tone peak per frame; chirp tracking; shape.
  - CWT: localisation; band resolution; shape; energy-ratio.
  - Streaming: cumulative convergence; EMA tracking; concatenation
    matches batch; warmup empties; central-column match.
- **Script integration** (`rustlab-script/src/tests.rs`):
  - Each calling form per builtin.
  - Multi-return destructuring.
  - String vs Vector window equivalence.
  - `plot_update_heatmap` headless no-op + ratatui round-trip.
- **Cross-backend** (workflow rule 9): SVG/HTML/PNG per heatmap-bearing
  builtin; terminal/viewer covered by existing `imagesc` tests.
- **Viewer wire smoke**: mocked-sink check for `PanelHeatmap` send.
- **Notebook round-trip**: `time_frequency.md` renders cleanly under
  the existing `notebook_render` test.

---

## Gallery / notebook integration — first-class

- **Phase 1**: extends `spectral_estimation.md` with pwelch section.
- **Phase 2**: creates `time_frequency.md` with STFT/spectrogram;
  cross-linked from `spectral_estimation.md`.
- **Phase 3**: extends `time_frequency.md` with CWT (side-by-side
  spectrogram vs scalogram).
- **Phase 4**: extends `time_frequency.md` with a Streaming section
  using a synthetic chirp split into frames (notebooks render offline;
  live `audio_in` lives in the `.rlab` example).

---

## Next step

On approval, start Phase 1 (pwelch). Decisions 1–15 are locked in;
8–9 are explicit deferrals for future plans.

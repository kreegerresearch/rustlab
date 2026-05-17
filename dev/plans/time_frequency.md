# Development Plan: Time-Frequency Analysis (pwelch + STFT + CWT)

**Target use case:** rounding out rustlab's signal-identification plotting
with the standard time-frequency toolkit — Welch power spectral density
estimation, Short-Time Fourier Transform with spectrogram visualization, and
Continuous Wavelet Transform with scalogram visualization. Three features,
one common sliding-window infrastructure, one shared plot path (`imagesc`).

**Current phase:** not started
**Status:** draft — awaiting user approval

---

## Overview

Three related capabilities that share most of their plumbing:

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
   time-frequency matrix. `scalogram` is `cwt` + heatmap render of `|cwt|²`.

Why bundle: pwelch and stft share the segment-iteration infrastructure
(`hop = win_len − noverlap`, padding, windowing). cwt is mathematically
different but follows the same `2-D matrix → imagesc → heatmap` plotting
pattern that already works across every rustlab backend. One coherent
plan, three phases.

---

## Open questions for the user before we start

These are reasonable defaults — call them out if you want different.

1. **Wavelet families in CWT scope.** Start with **Morlet** only (the
   workhorse for time-frequency analysis); add **Mexican-hat** (Ricker)
   and **Gaussian-derivative** as named options later if you ask. Three
   families is enough — `cwt` taking a wavelet-family arg as a string
   covers it without needing user-defined wavelets.

2. **PSD output one-sided vs two-sided.** Default to **one-sided**
   (positive frequencies only, with 2× scaling on internal bins to
   preserve total power) — what almost every RF/audio engineer wants.
   Two-sided available via a `"twosided"` flag.

3. **Default window for pwelch and stft.** **Hann** (matches MATLAB's
   default and matches the existing `spectral_estimation` notebook).

4. **Default overlap.** **50%** of the segment length — the standard
   choice with Hann (gives near-unit COLA reconstruction).

5. **Segment length default.** **`length(x) / 8`** for pwelch (MATLAB's
   default — eight averaging segments). For STFT default to `256` so the
   spectrogram has a useful time axis.

6. **What about ISTFT?** The Inverse Short-Time Fourier Transform is
   useful for spectral filtering / time-stretching / phase-vocoder work,
   but it's a separate feature with its own correctness criterion
   (perfect reconstruction under COLA). **Out of scope** here; can be a
   later plan if you want it.

7. **What about reassignment / multitaper / Wigner-Ville?** All are RF
   adjacent and could fit a "time-frequency advanced" plan, but each
   significantly increases the math surface. **Out of scope** here.

**Please confirm 1–5 before we start Phase 1.** Items 6–7 are noted as
explicit deferrals so we don't accidentally creep.

---

## Data model decisions (apply to all three features)

### Output shapes

| Feature | Single-return form | Multi-return form |
|---|---|---|
| `pwelch` | real `Vector` (Pxx) | `[Pxx, f] = pwelch(x, fs, ...)` |
| `stft` | complex `Matrix` (S) | `[S, f, t] = stft(x, fs, ...)` |
| `cwt` | complex `Matrix` (W) | `[W, freqs, t] = cwt(x, fs, ...)` |

Matrix layout for STFT/CWT: **rows = frequencies (low at row 1), cols =
time samples (early at col 1)**. This is what `imagesc` already plots
naturally — time on x-axis, frequency on y-axis. Note this is the
opposite convention from `parameters[k, i, j]` in the sparameters toolbox
(which uses `[freq, port, port]`) — but for 2-D time-frequency images
the heatmap convention dominates and matches MATLAB.

### Auto-plot when not assigned

Per the rustlab pattern already used by `bode` / `nyquist` / `rfplot`:
when the function is called without capturing its return values, it
auto-renders. For `spectrogram` / `scalogram` these are dedicated
plot-only wrappers — they don't return data. For `pwelch` / `stft` /
`cwt`, calling without `[..] = ...` assignment auto-plots.

### Windowing convention

Same `WindowFunction` enum already used by `window()` and the FIR
designers (`crates/rustlab-dsp/src/window/mod.rs`). Accept either:

- A string name: `"hann"`, `"hamming"`, `"rectangular"`, `"blackman"`, `"kaiser"`.
- A precomputed coefficient `Vector` — for users who want a custom window.

The window's length determines the segment size; `nfft` (when larger)
implies zero-padding for finer frequency resolution.

---

## Phase 1 — pwelch (Welch's PSD estimator)

**Status:** not started
**Goal:** ship the simplest of the three features end-to-end so the
sliding-window infrastructure has a battle-tested home before STFT extends
it.

### 1a. `dsp::welch` math module

- **New file:** `crates/rustlab-dsp/src/welch.rs`
- **Signature:**
  ```rust
  pub fn pwelch_psd(
      x: &RVector,
      fs: f64,
      window: &RVector,
      noverlap: usize,
      nfft: usize,
      onesided: bool,
  ) -> Result<(RVector /* Pxx */, RVector /* f */), DspError>;
  ```
- **Algorithm:**
  1. `hop = window.len() - noverlap`
  2. `n_segments = floor((len(x) - window.len()) / hop) + 1`
  3. For each segment, multiply by window, zero-pad to `nfft`, FFT.
  4. Per-segment periodogram: `|X|² / (fs · ∑w²)`.
  5. Average across segments.
  6. If `onesided`, fold k > nfft/2 onto k < nfft/2 with 2× weighting
     for interior bins (excluding DC and Nyquist).
- **Validation:** `window.len() <= len(x)`; `noverlap < window.len()`;
  `nfft >= window.len()`.
- **Tests** in `crates/rustlab-dsp/src/tests.rs`:
  - PSD of a unit-amplitude sine at frequency `f0`, integrated, equals
    `0.5` (sine power).
  - PSD of white noise is approximately flat (variance check across bins).
  - DC component: `pwelch` of a DC signal (constant) puts all power in
    bin 0.
  - One-sided integral equals two-sided integral (sanity on the folding).

### 1b. `pwelch` script builtin

- **File:** `crates/rustlab-script/src/eval/builtins.rs`
- **Signatures:**
  - `pwelch(x, fs)` — defaults: Hann window of length `len(x)/8`,
    50% overlap, `nfft = window length`, one-sided.
  - `pwelch(x, fs, window)` — window is a string name or a `Vector`.
  - `pwelch(x, fs, window, noverlap)` — overlap as integer sample count.
  - `pwelch(x, fs, window, noverlap, nfft)` — `nfft` zero-pads for finer
    frequency resolution.
  - `[Pxx, f] = pwelch(...)` — capture data without plotting.
  - `pwelch(x, fs)` (no capture) — auto-plot dB-scale PSD vs frequency.
- **Tests** in `crates/rustlab-script/src/tests.rs`:
  - Two-tone signal: pwelch identifies both tones with > 20 dB SNR over
    the noise floor.
  - Hann-windowed pwelch has lower sidelobes than rectangular pwelch on
    the same signal (the existing `spectral_estimation` notebook's
    teaching point, but now quantified).
  - Multi-return tuple destructuring: `[Pxx, f] = pwelch(x, fs)`.

### Phase-1 deliverables checklist

- [ ] `dsp::welch::pwelch_psd` with the 4 math tests above.
- [ ] `pwelch` script builtin (5 calling forms).
- [ ] REPL `HelpEntry` for `pwelch`; add to a new
      `"Spectral Estimation"` category alongside `spectrum`, `fft`,
      `fftshift`.
- [ ] `docs/functions.md` § "Spectral Estimation" entry with the
      five forms and a note on the variance/resolution trade-off.
- [ ] `docs/quickref.md` § "Spectral Estimation" sub-table.
- [ ] `AGENTS.md` builtin entry.
- [ ] Example `examples/spectral/pwelch.rlab` showing periodogram →
      windowed periodogram → pwelch progression with the same noisy
      two-tone signal as the existing notebook.
- [ ] Notebook `examples/notebooks/spectral_estimation_extended.md` (or
      extension of the existing `spectral_estimation.md`) — gallery
      entry that demonstrates pwelch with embedded plots. **This is
      mandatory** per the discoverability lesson from the sparameters
      work; don't repeat that gap.
- [ ] `cargo test --workspace` green.

---

## Phase 2 — STFT + spectrogram

**Status:** not started
**Depends on:** Phase 1 (shares the segment-iterator helper)

### 2a. Shared segment iterator

Extract a `crate-private` helper `dsp::welch::segment_iter(x, win_len,
noverlap)` that returns `(start_index, end_index)` tuples. Phase 1 uses
it inline; Phase 2 imports it for the STFT loop. Keeps the segment math
in one place so a future bug fix only happens once.

### 2b. `dsp::stft` math module

- **New file:** `crates/rustlab-dsp/src/stft.rs`
- **Signature:**
  ```rust
  pub fn stft(
      x: &RVector,
      fs: f64,
      window: &RVector,
      noverlap: usize,
      nfft: usize,
      onesided: bool,
  ) -> Result<(CMatrix /* S */, RVector /* f */, RVector /* t */), DspError>;
  ```
- **Algorithm:**
  1. Same segment iterator as pwelch.
  2. For each segment, multiply by window, zero-pad to `nfft`, FFT.
  3. Stack column-by-column into `S` (rows = freq bins, cols = time
     frames; the layout `imagesc` plots naturally).
  4. `t[k] = (k·hop + win_len/2) / fs` — segment-centre times.
  5. `f` from `fftfreq(nfft, fs)`, folded to one-sided if requested.
- **Tests:**
  - Pure-tone constant signal: STFT magnitude is concentrated in the
    bin closest to the tone frequency across every time frame.
  - Linear chirp: peak-magnitude bin per frame tracks the instantaneous
    frequency to within one bin.
  - Shape: `S.nrows()` equals expected freq-bin count
    (`nfft/2 + 1` one-sided, `nfft` two-sided);
    `S.ncols()` equals the segment count.

### 2c. `stft` and `spectrogram` script builtins

- `stft(x, fs)` / `stft(x, fs, window, noverlap, nfft)` —
  capture returns `[S, f, t]`. Bare call auto-renders the magnitude
  spectrogram.
- `spectrogram(x, fs)` / `spectrogram(x, fs, window, noverlap, nfft)`
  — dedicated plot wrapper. Renders `20·log10(|S|)` via `imagesc` with
  a `viridis` colormap; sets axis labels (time vs frequency); applies
  `axis("xy")` so frequency increases upward (physics convention) —
  the convention every audio/RF tool uses.
- **Tests:**
  - Calling form acceptance for both `stft` and `spectrogram`.
  - Shape: `spectrogram` populates the current subplot with a heatmap
    whose dimensions match the expected `[nfreqs, nframes]`.

### 2d. Cross-backend rendering

The spectrogram is just `imagesc`, which already works across every
backend (SVG, PNG, HTML/Plotly, terminal, viewer, LaTeX/PDF) — same
"no per-backend dispatch needed" pattern that worked for Smith charts
and rfplot. Pin one test per backend that the file output is non-empty
and well-formed (matching the Phase 3 sparameters pattern).

### Phase-2 deliverables checklist

- [ ] Segment iterator extracted from Phase 1.
- [ ] `dsp::stft::stft` with 3 math tests.
- [ ] `stft` and `spectrogram` script builtins.
- [ ] REPL `HelpEntry` for both.
- [ ] `docs/functions.md`, `docs/quickref.md`, `AGENTS.md` updates.
- [ ] Example `examples/spectral/spectrogram_chirp.rlab` — linear
      chirp from 100 Hz to 5 kHz over 2 seconds, render the
      spectrogram to see the time-frequency ramp.
- [ ] Notebook `examples/notebooks/time_frequency.md` extending the
      pwelch notebook with the spectrogram view.
- [ ] Backend tests: SVG / HTML / PNG of a small spectrogram.
- [ ] `cargo test --workspace` green.

---

## Phase 3 — CWT + scalogram

**Status:** not started
**Depends on:** Phase 2 (shares the 2-D heatmap rendering convention)

### 3a. Wavelet families

Start with **Morlet** only; the API leaves room for additions.

- **New file:** `crates/rustlab-dsp/src/wavelet.rs`
- **Morlet (complex)**:
  ```
  ψ(t) = π^(-1/4) · exp(j·ω₀·t) · exp(-t²/2)
  ```
  with `ω₀ = 6` (the canonical choice — gives ~6 oscillations under the
  Gaussian envelope, the standard time-frequency-resolution
  compromise).

### 3b. CWT math

- **Signature:**
  ```rust
  pub fn cwt_morlet(
      x: &RVector,
      fs: f64,
      scales: &RVector,
  ) -> Result<(CMatrix /* W */, RVector /* freqs */, RVector /* t */), DspError>;
  ```
- **Algorithm (frequency-domain, the only realistic path):**
  1. FFT the signal once.
  2. For each scale `s`, build the FFT of the scaled wavelet `ψ((t)/s)`
     analytically (Morlet's FT has a closed form: shifted Gaussian).
  3. Multiply `X(ω) · Ψ_s*(ω)`, IFFT to get the wavelet coefficients
     at that scale.
  4. Stack rows into `W`.
  5. `freqs[i] = ω₀ / (2π · scales[i] · dt)` (Morlet's centre-frequency
     formula).
- **Scale grid default:** logarithmic from 2 samples to `len(x)/4`,
  `n_scales = 64` (gives a smooth scalogram without being wasteful).

### 3c. `cwt` and `scalogram` script builtins

- `cwt(x, fs)` / `cwt(x, fs, "morlet")` / `cwt(x, fs, "morlet",
  n_scales)` / `cwt(x, fs, "morlet", scales_vector)`. Returns
  `[W, freqs, t]` when captured; auto-plots the scalogram when bare.
- `scalogram(x, fs)` — dedicated plot wrapper, renders `|W|²` via
  `imagesc` with a `viridis` colormap, log-frequency axis.
- **Tests:**
  - Single Gaussian-modulated impulse at `t₀, f₀`: scalogram peak is at
    `(t₀, f₀)` within one scale-band.
  - Two well-separated tones at different frequencies: scalogram
    resolves both as horizontal bands.
  - Shape: `W.nrows() == n_scales`, `W.ncols() == len(x)`.
  - Parseval-like sanity: `∑|W|²` over the time-frequency plane is
    proportional to signal energy (admissibility constant for Morlet
    is well-known, pin numerically).

### Phase-3 deliverables checklist

- [ ] `dsp::wavelet::cwt_morlet` with 4 math tests.
- [ ] `cwt` and `scalogram` script builtins.
- [ ] REPL `HelpEntry`, docs, quickref, AGENTS updates.
- [ ] Example `examples/spectral/cwt_chirp.rlab` — same chirp as the
      spectrogram example, rendered through CWT for contrast (better
      time resolution at high frequencies, better frequency resolution
      at low frequencies — the canonical demonstration of why wavelets
      exist).
- [ ] Extend the `time_frequency.md` notebook with the CWT section.
- [ ] `cargo test --workspace` green.

---

## Risk register

| Risk | Mitigation |
|---|---|
| **Window-overlap-FFT math subtle** (normalization factors trip everyone up the first time) | Anchor pwelch against the integral-equals-power identity for a unit sine; anchor STFT magnitude against a known-frequency tone. Both are numerical, both catch the entire family of `1/N` vs `1/(fs·∑w²)` mistakes at once. |
| **Spectrogram y-axis convention** (frequency increases upward; rustlab's `imagesc` defaults to image convention — row 0 at top) | Explicitly set `axis("xy")` inside the `spectrogram` builtin. There's prior art for this exact decision in the imagesc 2026-05-16 fix. |
| **CWT-default-scales choice** | Start with log-spaced 64 scales; document the formula in the help text. Users can pass their own scale vector via the 4-arg form. |
| **Morlet ω₀ choice** | Hard-code ω₀ = 6 (the canonical choice). If users ask for a tunable parameter later, add it as a name-value `("omega0", 8.0)` option without changing the default. |
| **Performance** | All three features are O(N log N) per segment via the existing FFT (pwelch, STFT) or O(N log N) per scale (CWT via frequency-domain multiply). For a typical N = 100k signal this is fine. Add a perf bench under `perf/` if the gallery rebuild slows down noticeably. |
| **Notebook discoverability gap repeating** | Notebooks are in the Phase-1 and Phase-2 checklists, not deferred. The sparameters work had the gallery gap because notebook entries were always "next phase." Don't repeat that. |

---

## File / crate impact summary

| Crate | New files | Modified files |
|---|---|---|
| `rustlab-dsp` | `src/welch.rs`, `src/stft.rs`, `src/wavelet.rs` | `src/lib.rs` (mod declarations + re-exports), `src/tests.rs` |
| `rustlab-script` | — | `eval/builtins.rs` (5 new builtins), `tests.rs` |
| `rustlab-cli` | — | `commands/repl.rs` (5 HelpEntries + new "Spectral Estimation" category) |
| Docs / examples | `examples/spectral/{pwelch,spectrogram_chirp,cwt_chirp}.rlab`, `examples/notebooks/time_frequency.md` | `docs/functions.md`, `docs/quickref.md`, `AGENTS.md`, `gallery/index.html` (add notebook link), existing `spectral_estimation.md` (cross-reference link to the new one) |

No new workspace dependencies. All math is pure Rust per workflow rule
10. Re-uses the existing `fft` / `ifft` / `WindowFunction` /
`imagesc` infrastructure.

---

## Test strategy summary

- **Math anchors** (in `rustlab-dsp/src/tests.rs`):
  - pwelch: unit-sine power integral = 0.5; white-noise flatness;
    DC concentrate in bin 0.
  - STFT: tone peak per frame; chirp tracking; shape correctness.
  - CWT: impulse Gaussian localisation; tone band resolution; Parseval.
- **Script integration tests** (in `rustlab-script/src/tests.rs`):
  - Each calling form for each builtin.
  - Multi-return tuple destructuring (`[Pxx, f] = pwelch(...)`,
    `[S, f, t] = stft(...)`, `[W, freqs, t] = cwt(...)`).
  - String-name vs precomputed-vector window forms.
- **Cross-backend rendering** (workflow rule 9): one test each for
  spectrogram SVG / HTML / PNG / terminal rendering being non-empty
  and well-formed; the existing `imagesc` cross-backend pinning carries
  most of it.
- **Notebook round-trip**: the new `time_frequency.md` notebook must
  render through `rustlab-notebook render` cleanly under the
  `notebook_render` test that already covers the gallery.

---

## Gallery / notebook integration — explicitly first-class

After the sparameters work shipped without gallery coverage and you
flagged that gap, this plan treats notebooks as a first-class
deliverable per phase, not as deferred polish:

- **Phase 1**: `time_frequency.md` notebook starts here with the pwelch
  section; gallery entry added to `gallery/index.html` in the same
  commit.
- **Phase 2**: notebook extended with the spectrogram section
  (embedded heatmap of the chirp); gallery link still works (existing
  entry just gains a section).
- **Phase 3**: notebook extended with the CWT section (side-by-side
  spectrogram vs scalogram of the chirp).

The notebook is the single discoverable artefact; the standalone
`.rlab` examples are runnable reference but not the primary
documentation path.

---

## Next step

Plan ready for review. On approval, start Phase 1 (pwelch). Open
questions 1–5 in the "Open questions" section above are the only items
that need explicit confirmation before code lands; 6–7 are deliberate
deferrals.

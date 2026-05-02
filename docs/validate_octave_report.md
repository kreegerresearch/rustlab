# Octave Validation Report

**Date:** 2026-04-04
**Rustlab commit:** 6268ed2
**Octave version:** GNU Octave 11.1.0 (aarch64-apple-darwin25.2.0)
**Octave packages:** signal 4.x, control 4.x

## Summary

19 cross-validation tests were run comparing rustlab DSP output against
equivalent GNU Octave computations. All 19 passed.

| Result | Count |
|--------|-------|
| PASS   | 19    |
| FAIL   | 0     |

## Test Procedure

Test scripts live in `tests/octave/`:

| File | Purpose |
|------|---------|
| `reference.m` | Octave script; computes reference values and writes `ref_*.csv` |
| `rustlab_outputs.rlab` | Rustlab script; computes the same values and writes `out_*.csv` |
| `compare.m` | Octave script; reads both CSV sets, reports max absolute error and PASS/FAIL per test |

To reproduce:

```sh
cd tests/octave
octave --no-gui reference.m
cargo run -q -- run rustlab_outputs.rlab
octave --no-gui compare.m
```

Each test compares vectors element-wise and reports the maximum absolute error
against a per-category tolerance. Tolerances were chosen based on the nature of
each algorithm:

| Tolerance | Category |
|-----------|----------|
| 1e-9 | Exact arithmetic (FFT, convolution, fftshift) |
| 1e-6 | Filter design (windowed-sinc, Kaiser) — same closed-form formula |
| 1e-4 | firpm — iterative Remez algorithm; different implementations may converge to slightly different equiripple solutions |

## Results

```
Result Test                           max_err        tol
-----------------------------------------------------------------
PASS  FFT real (re)                   2.22e-15       1.00e-09
PASS  FFT real (im)                   1.33e-15       1.00e-09
PASS  IFFT round-trip                 4.44e-16       1.00e-09
PASS  FFT complex (re)                0.00e+00       1.00e-09
PASS  FFT complex (im)                4.44e-16       1.00e-09
PASS  fftshift N=8                    0.00e+00       1.00e-09
PASS  fftshift N=7                    0.00e+00       1.00e-09
PASS  convolve                        0.00e+00       1.00e-09
PASS  fir_lowpass Hann                2.78e-17       1.00e-06
PASS  fir_highpass Hann               2.78e-17       1.00e-06
PASS  fir_bandpass Hann               5.55e-17       1.00e-06
PASS  fir_lowpass Hamming             5.55e-17       1.00e-06
PASS  freqz Hz axis                   0.00e+00       1.00e-06
PASS  freqz magnitude                 8.86e-15       1.00e-06
PASS  firpm LP 63-tap                 1.92e-06       1.00e-04
PASS  firpm BP 79-tap                 5.67e-06       1.00e-04
PASS  Kaiser LP                       7.77e-16       1.00e-06
PASS  Kaiser HP                       5.52e-16       1.00e-06
PASS  SNR formula                     3.55e-15       1.00e-06
-----------------------------------------------------------------
Total: 19 passed, 0 failed
```

## Functions Validated

| Function | Octave Reference | Notes |
|----------|-----------------|-------|
| `fft` | `fft()` | Real and complex input; max error at floating-point floor (~1 ULP) |
| `ifft` | `ifft()` | Round-trip fft→ifft; error < 5e-16 |
| `fftshift` | `fftshift()` | Even (N=8) and odd (N=7) lengths; exact match |
| `convolve` | `conv()` | Exact match |
| `fir_lowpass` (Hann) | Windowed-sinc formula + DC normalization | Error < 3e-17 |
| `fir_lowpass` (Hamming) | Windowed-sinc formula + DC normalization | Error < 6e-17 |
| `fir_highpass` (Hann) | Spectral inversion of LP | Error < 3e-17 |
| `fir_bandpass` (Hann) | Difference of two normalized LPs | Error < 6e-17 |
| `freqz` | `freqz()` | Hz axis and magnitude; frequency range 0 to Nyquist |
| `firpm` (LP 63-tap) | `firpm()` | Max tap error 1.92e-6; same equiripple result |
| `firpm` (BP 79-tap) | `firpm()` | Max tap error 5.67e-6; same equiripple result |
| `fir_lowpass_kaiser` | Windowed-sinc + Kaiser β formula | Error < 8e-16 |
| `fir_highpass_kaiser` | Spectral inversion of Kaiser LP | Error < 6e-16 |
| `snr` | `10*log10(Σs²/Σ(s-n)²)` | Error < 4e-15 |

## Issues Found and Fixed During This Session

Two correctness bugs were identified by reviewing example output plots and
confirmed by cross-validation. Both were fixed before this report was run.

### 1. `freqz` frequency axis covered 0 → fs instead of 0 → Nyquist

**File:** `crates/rustlab-dsp/src/fir/kaiser.rs`

The FFT size was `next_power_of_two(n_points.max(h.len()))`. For a short
filter and n_points=512 this made fft_size=512, so the first 512 DFT bins
spanned the full 0 to fs range instead of 0 to fs/2. All frequency response
plots (`savedb`, `plotdb`) were labelled with a frequency axis twice as wide
as Nyquist.

**Fix:** `fft_size = next_power_of_two((2 * n_points).max(h.len()))` ensures
the n_points bins stay within the positive-frequency half of the spectrum.

### 2. `fir_lowpass` did not normalize for unity DC gain

**File:** `crates/rustlab-dsp/src/fir/design.rs`

The windowed-sinc formula `h[n] = 2·fc·sinc(2·fc·(n−m))·w[n]` was not
normalized after windowing. For narrow-band filters (e.g. fc=1 kHz at
sr=44.1 kHz with a 32-tap Hann window) the passband gain was approximately
−4 dB instead of 0 dB. Octave's `fir1` normalizes after windowing.

**Fix:** After computing the windowed coefficients, divide by their sum
(`dc_gain = Σ h[n]`). Derived filters (highpass via spectral inversion,
bandpass via LP difference) are correct by construction once the LP is
normalized: highpass DC = −1 + 1 = 0, bandpass DC = 1 − 1 = 0.

//! Continuous Wavelet Transform with the analytic Morlet mother wavelet.
//!
//! Returns the canonical `(W, freqs, t)` triple expected by the script-side
//! `cwt` and `scalogram` builtins. Frequency-domain implementation:
//! pad signal with zeros to a power-of-two length, FFT once, multiply by the
//! analytic Morlet's Gaussian transfer function at each scale, IFFT, trim.
//!
//! The analytic Morlet wavelet in the time domain:
//!
//! ```text
//! ψ(t) = π^(-1/4) · exp(j·ω₀·t) · exp(-t²/2)
//! ```
//!
//! with `ω₀ = 6` (canonical choice — ~6 oscillations under the Gaussian
//! envelope; the standard time-frequency-resolution trade-off). Its
//! Fourier transform is a Gaussian centred at `ω = ω₀`, so the
//! scaled-wavelet FT in radians-per-sample is the simple Gaussian
//! `Ψ_s(ω) = exp(-(s·ω − ω₀)² / 2)` (unnormalised — the absolute
//! amplitude is irrelevant to all CWT applications in this crate;
//! callers care about *relative* magnitudes within the W matrix).
//!
//! Scale-to-frequency relation (Morlet): `f = ω₀ · fs / (2π · s)`.

use crate::convolution::next_power_of_two;
use crate::error::DspError;
use crate::fft::{fft_raw, ifft_raw};
use ndarray::{Array1, Array2};
use num_complex::Complex;
use rustlab_core::{CMatrix, CVector, RVector};
use std::f64::consts::PI;

/// Canonical Morlet central frequency parameter.
const OMEGA0: f64 = 6.0;

/// Continuous Wavelet Transform with the analytic Morlet wavelet.
///
/// Returns `(W, freqs, t)` where:
/// - `W` is a complex matrix with `scales.len()` rows and `x.len()` columns.
///   Row `i` is the CWT at scale `scales[i]`.
/// - `freqs[i] = ω₀ · fs / (2π · scales[i])` — the centre frequency of
///   the wavelet at scale `i`, in Hz.
/// - `t[k] = k / fs` — the time of sample `k`, in seconds.
///
/// The signal is zero-padded to `next_pow2(len + 8·max_scale)` before the
/// forward FFT to suppress circular-convolution wrap from the long
/// scales. After the IFFT the padding is trimmed; only the original
/// `len(x)` samples are returned.
///
/// Scales are interpreted in samples and must be strictly positive.
/// Edge effects on the leftmost / rightmost columns are not masked —
/// users who need a strict cone-of-influence envelope can post-process
/// the matrix themselves.
pub fn cwt_morlet(
    x: &CVector,
    fs: f64,
    scales: &RVector,
) -> Result<(CMatrix, RVector, RVector), DspError> {
    let n = x.len();
    if n == 0 {
        return Err(DspError::InvalidParameter(
            "cwt_morlet: signal is empty".into(),
        ));
    }
    if scales.is_empty() {
        return Err(DspError::InvalidParameter(
            "cwt_morlet: scales vector is empty".into(),
        ));
    }
    if !(fs > 0.0) {
        return Err(DspError::InvalidParameter(format!(
            "cwt_morlet: fs {fs} must be > 0"
        )));
    }
    let max_scale = scales.iter().copied().fold(0.0f64, f64::max);
    if !(max_scale > 0.0) || !max_scale.is_finite() {
        return Err(DspError::InvalidParameter(format!(
            "cwt_morlet: scales must be strictly positive (max={max_scale})"
        )));
    }
    for &s in scales.iter() {
        if !(s > 0.0) {
            return Err(DspError::InvalidParameter(format!(
                "cwt_morlet: scale {s} must be > 0"
            )));
        }
    }

    // Zero-pad symmetrically by 4σ at the maximum scale. The Morlet's
    // Gaussian envelope has σ = scale (in samples), so 4σ catches > 99.99%
    // of the wavelet's time-domain energy.
    let pad = (4.0 * max_scale).ceil() as usize;
    let n_padded = next_power_of_two(n + 2 * pad);
    let pad_left = pad;

    let mut x_padded = vec![Complex::new(0.0, 0.0); n_padded];
    for (k, &c) in x.iter().enumerate() {
        x_padded[pad_left + k] = c;
    }
    let x_fft = fft_raw(&x_padded);

    let n_scales = scales.len();
    let mut w = Array2::<Complex<f64>>::zeros((n_scales, n));
    let mut filtered = vec![Complex::new(0.0, 0.0); n_padded];

    for (row, &s) in scales.iter().enumerate() {
        for k in 0..n_padded {
            // Angular frequency in rad/sample for FFT bin k.
            let omega = if k <= n_padded / 2 {
                2.0 * PI * k as f64 / n_padded as f64
            } else {
                2.0 * PI * (k as f64 - n_padded as f64) / n_padded as f64
            };
            let arg = s * omega - OMEGA0;
            let psi_hat = (-0.5 * arg * arg).exp();
            filtered[k] = x_fft[k] * psi_hat;
        }
        let conv = ifft_raw(&filtered);
        for col in 0..n {
            w[(row, col)] = conv[pad_left + col];
        }
    }

    let freqs: RVector =
        Array1::from_iter(scales.iter().map(|&s| OMEGA0 * fs / (2.0 * PI * s)));
    let t: RVector = Array1::from_iter((0..n).map(|k| k as f64 / fs));
    Ok((w, freqs, t))
}

// ─── cwt_stream ──────────────────────────────────────────────────────────────

/// Per-stream state for `cwt_stream`. Holds the sliding window of
/// recent samples plus the scale grid; CWT is recomputed each call
/// over the current window contents.
#[derive(Debug)]
pub struct CwtState {
    fs: f64,
    scales: RVector,
    n_samples: usize,
    history: Vec<num_complex::Complex<f64>>,
}

/// Construct a streaming-CWT state with a fixed sliding-window length.
/// On each `cwt_stream` call the latest `n_samples` are CWT'd; older
/// samples are dropped.
pub fn cwt_stream_init(
    fs: f64,
    n_samples: usize,
    scales: &RVector,
) -> Result<CwtState, DspError> {
    if n_samples == 0 {
        return Err(DspError::InvalidParameter(
            "cwt_stream_init: n_samples must be > 0".into(),
        ));
    }
    if scales.is_empty() {
        return Err(DspError::InvalidParameter(
            "cwt_stream_init: scales vector is empty".into(),
        ));
    }
    if !(fs > 0.0) {
        return Err(DspError::InvalidParameter(format!(
            "cwt_stream_init: fs {fs} must be > 0"
        )));
    }
    for &s in scales.iter() {
        if !(s > 0.0) {
            return Err(DspError::InvalidParameter(format!(
                "cwt_stream_init: scale {s} must be > 0"
            )));
        }
    }
    Ok(CwtState {
        fs,
        scales: scales.clone(),
        n_samples,
        history: Vec::with_capacity(n_samples * 2),
    })
}

/// Push a frame of new samples into the streaming CWT state and return
/// the CWT over the current sliding window of length `n_samples`.
/// Returns an `n_scales × 0` matrix until the buffer first fills.
///
/// Edge effects on the rightmost columns are *not* trimmed (decision
/// 13 in the time-frequency plan) — long-scale wavelets reach beyond
/// the most recent samples, so the rightmost columns are visually
/// "ahead" of complete information. For a live display this is fine.
pub fn cwt_stream(frame: &CVector, state: &mut CwtState) -> CMatrix {
    state.history.extend(frame.iter().copied());
    if state.history.len() > state.n_samples {
        let drop = state.history.len() - state.n_samples;
        state.history.drain(..drop);
    }
    if state.history.len() < state.n_samples {
        return Array2::zeros((state.scales.len(), 0));
    }
    let x: CVector = Array1::from_vec(state.history.clone());
    match cwt_morlet(&x, state.fs, &state.scales) {
        Ok((w, _, _)) => w,
        Err(_) => Array2::zeros((state.scales.len(), 0)),
    }
}

/// Number of scale rows the state emits per call once the buffer has filled.
pub fn cwt_state_n_scales(state: &CwtState) -> usize {
    state.scales.len()
}

/// Default scale grid: `n_scales` log-spaced scales from 2 samples
/// (highest frequency) to `signal_len / 4` samples (lowest frequency).
///
/// For typical signals this gives a smooth scalogram without oversampling
/// either extreme. Scales below ~5 samples have degraded wavelet shape
/// due to undersampling; that's a known trade-off, not a bug.
pub fn default_scales(signal_len: usize, n_scales: usize) -> RVector {
    if n_scales == 0 {
        return Array1::zeros(0);
    }
    if n_scales == 1 {
        return Array1::from_vec(vec![2.0]);
    }
    let s_min = 2.0;
    let s_max = ((signal_len as f64) / 4.0).max(s_min + 1.0);
    let log_min = s_min.ln();
    let log_max = s_max.ln();
    let denom = (n_scales - 1) as f64;
    Array1::from_iter((0..n_scales).map(|i| {
        let t = i as f64 / denom;
        (log_min + t * (log_max - log_min)).exp()
    }))
}

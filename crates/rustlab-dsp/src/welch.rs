//! Welch's power spectral density estimator and shared segment iterator.
//!
//! `pwelch_psd` segments the signal into overlapping windows, computes a
//! periodogram for each, and averages them. Trades frequency resolution
//! for variance reduction. Matches MATLAB pwelch conventions: no
//! detrending, one-sided default for real input, two-sided default for
//! complex.
//!
//! `segment_iter` is the crate-private helper shared by `pwelch_psd` and
//! `stft` (Phase 2).

use crate::convolution::next_power_of_two;
use crate::error::DspError;
use crate::fft::fft_raw;
use ndarray::Array1;
use num_complex::Complex;
use rustlab_core::{CVector, RVector};

/// One-sided / two-sided dispatch for PSD output.
#[derive(Debug, Clone, Copy)]
pub enum Sided {
    /// Positive frequencies only; interior bins doubled.
    OneSided,
    /// All FFT bins in raw order.
    TwoSided,
    /// One-sided for real input, two-sided for complex.
    Auto,
}

/// Iterate `(start, end)` sample indices for sliding segments of length
/// `win_len` with hop `win_len − noverlap` over a signal of length `n`.
///
/// Yields nothing if `win_len == 0` or `win_len > n`. Debug-asserts that
/// `hop > 0`; callers validate `noverlap < win_len`.
pub(crate) fn segment_iter(
    n: usize,
    win_len: usize,
    noverlap: usize,
) -> impl Iterator<Item = (usize, usize)> {
    let hop = win_len.saturating_sub(noverlap);
    debug_assert!(
        win_len == 0 || hop > 0,
        "segment_iter: hop must be > 0 (got win_len={win_len}, noverlap={noverlap})"
    );
    let mut start = 0usize;
    std::iter::from_fn(move || {
        if win_len == 0 || hop == 0 || start + win_len > n {
            None
        } else {
            let r = (start, start + win_len);
            start += hop;
            Some(r)
        }
    })
}

/// Welch's power spectral density estimator.
///
/// Returns `(Pxx, f)`. No detrending is applied (matches MATLAB pwelch);
/// callers wanting detrending should pass `x - mean(x)`.
///
/// `nfft` is the requested FFT size; if not a power of two it is
/// rounded up internally. The output length follows the effective FFT
/// size: `n_eff/2 + 1` bins for `Sided::OneSided`, `n_eff` bins for
/// `Sided::TwoSided`. `Sided::Auto` resolves to one-sided when `x` has
/// no imaginary component, two-sided otherwise.
pub fn pwelch_psd(
    x: &CVector,
    fs: f64,
    window: &RVector,
    noverlap: usize,
    nfft: usize,
    sided: Sided,
) -> Result<(RVector, RVector), DspError> {
    let n = x.len();
    let m = window.len();
    if m == 0 {
        return Err(DspError::InvalidParameter("pwelch: window is empty".into()));
    }
    if m > n {
        return Err(DspError::InvalidParameter(format!(
            "pwelch: window length {m} exceeds signal length {n}"
        )));
    }
    if noverlap >= m {
        return Err(DspError::InvalidParameter(format!(
            "pwelch: noverlap {noverlap} must be < window length {m}"
        )));
    }
    if nfft < m {
        return Err(DspError::InvalidParameter(format!(
            "pwelch: nfft {nfft} must be >= window length {m}"
        )));
    }
    if !(fs > 0.0) {
        return Err(DspError::InvalidParameter(format!(
            "pwelch: fs {fs} must be > 0"
        )));
    }

    let n_eff = next_power_of_two(nfft);

    let win_pow: f64 = window.iter().map(|w| w * w).sum();
    if !(win_pow > 0.0) {
        return Err(DspError::InvalidParameter(
            "pwelch: window has zero energy".into(),
        ));
    }
    let scale = 1.0 / (fs * win_pow);

    let onesided = match sided {
        Sided::OneSided => true,
        Sided::TwoSided => false,
        Sided::Auto => x.iter().all(|c| c.im == 0.0),
    };

    let mut pxx_acc = vec![0.0f64; n_eff];
    let mut n_segments = 0usize;
    let mut buf = vec![Complex::new(0.0, 0.0); n_eff];
    for (start, _end) in segment_iter(n, m, noverlap) {
        for k in 0..m {
            buf[k] = x[start + k] * window[k];
        }
        for k in m..n_eff {
            buf[k] = Complex::new(0.0, 0.0);
        }
        let spectrum = fft_raw(&buf);
        for (acc, c) in pxx_acc.iter_mut().zip(spectrum.iter()) {
            *acc += c.norm_sqr() * scale;
        }
        n_segments += 1;
    }
    if n_segments == 0 {
        return Err(DspError::InvalidParameter(
            "pwelch: signal too short for any complete segment".into(),
        ));
    }
    let inv_n = 1.0 / n_segments as f64;
    for p in pxx_acc.iter_mut() {
        *p *= inv_n;
    }

    if !onesided {
        // Two-sided: raw FFT order (DC at 0, positives 1..n_eff/2, negatives n_eff/2..n_eff-1).
        let f: Vec<f64> = (0..n_eff)
            .map(|k| {
                if k <= n_eff / 2 {
                    k as f64 * fs / n_eff as f64
                } else {
                    (k as f64 - n_eff as f64) * fs / n_eff as f64
                }
            })
            .collect();
        return Ok((Array1::from_vec(pxx_acc), Array1::from_vec(f)));
    }

    // One-sided fold. n_eff is always a power of two (≥ 2), so it is even.
    let half = n_eff / 2;
    let mut out = vec![0.0; half + 1];
    out[0] = pxx_acc[0];
    for k in 1..half {
        out[k] = 2.0 * pxx_acc[k];
    }
    out[half] = pxx_acc[half];
    let f1: Vec<f64> = (0..=half).map(|k| k as f64 * fs / n_eff as f64).collect();
    Ok((Array1::from_vec(out), Array1::from_vec(f1)))
}

/// MATLAB-compatible default segment length: 8 segments at 50% overlap.
/// Algebra: `N = L + 7·L/2 = 9L/2` ⇒ `L = 2N/9`.
pub fn default_segment_len(n: usize) -> usize {
    let l = (2 * n) / 9;
    l.max(1)
}


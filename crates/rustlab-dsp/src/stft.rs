//! Short-Time Fourier Transform.
//!
//! Same segment-window-FFT loop as `pwelch_psd` but keeps every
//! per-segment spectrum rather than averaging, producing a 2-D
//! `[n_freqs × n_frames]` complex matrix. Pairs with the `spectrogram`
//! script builtin (heatmap of `|S|` in dB) for time-frequency
//! visualisation.
//!
//! Layout: rows = frequency bins (low at row 0), cols = time frames
//! (early at col 0). This is the layout `imagesc` plots naturally with
//! `axis("xy")`.

use crate::convolution::next_power_of_two;
use crate::error::DspError;
use crate::fft::fft_raw;
use crate::welch::{segment_iter, Sided};
use ndarray::{Array1, Array2};
use num_complex::Complex;
use rustlab_core::{CMatrix, CVector, RVector};

/// Short-Time Fourier Transform.
///
/// Returns `(S, f, t)` where `S` is a complex matrix with rows indexed
/// by frequency bin and columns indexed by time frame, `f` is the
/// frequency axis in Hz, and `t` is the segment-centre time axis in
/// seconds.
///
/// `nfft` is the requested FFT size and is rounded up internally to the
/// next power of two. `Sided::Auto` resolves to one-sided when `x` has
/// no imaginary component, two-sided otherwise (matches `pwelch_psd`).
pub fn stft(
    x: &CVector,
    fs: f64,
    window: &RVector,
    noverlap: usize,
    nfft: usize,
    sided: Sided,
) -> Result<(CMatrix, RVector, RVector), DspError> {
    let n = x.len();
    let m = window.len();
    if m == 0 {
        return Err(DspError::InvalidParameter("stft: window is empty".into()));
    }
    if m > n {
        return Err(DspError::InvalidParameter(format!(
            "stft: window length {m} exceeds signal length {n}"
        )));
    }
    if noverlap >= m {
        return Err(DspError::InvalidParameter(format!(
            "stft: noverlap {noverlap} must be < window length {m}"
        )));
    }
    if nfft < m {
        return Err(DspError::InvalidParameter(format!(
            "stft: nfft {nfft} must be >= window length {m}"
        )));
    }
    if !(fs > 0.0) {
        return Err(DspError::InvalidParameter(format!(
            "stft: fs {fs} must be > 0"
        )));
    }

    let n_eff = next_power_of_two(nfft);
    let hop = m - noverlap;

    let onesided = match sided {
        Sided::OneSided => true,
        Sided::TwoSided => false,
        Sided::Auto => x.iter().all(|c| c.im == 0.0),
    };
    let n_freqs = if onesided { n_eff / 2 + 1 } else { n_eff };

    // Collect segment starts so we know how many columns to allocate.
    let segments: Vec<(usize, usize)> = segment_iter(n, m, noverlap).collect();
    let n_frames = segments.len();
    if n_frames == 0 {
        return Err(DspError::InvalidParameter(
            "stft: signal too short for any complete segment".into(),
        ));
    }

    let mut s = Array2::<Complex<f64>>::zeros((n_freqs, n_frames));
    let mut buf = vec![Complex::new(0.0, 0.0); n_eff];
    for (col, &(start, _end)) in segments.iter().enumerate() {
        for k in 0..m {
            buf[k] = x[start + k] * window[k];
        }
        for k in m..n_eff {
            buf[k] = Complex::new(0.0, 0.0);
        }
        let spectrum = fft_raw(&buf);
        for row in 0..n_freqs {
            s[(row, col)] = spectrum[row];
        }
    }

    // Frequency axis.
    let f = if onesided {
        Array1::from_iter((0..n_freqs).map(|k| k as f64 * fs / n_eff as f64))
    } else {
        Array1::from_iter((0..n_eff).map(|k| {
            if k <= n_eff / 2 {
                k as f64 * fs / n_eff as f64
            } else {
                (k as f64 - n_eff as f64) * fs / n_eff as f64
            }
        }))
    };

    // Time axis: segment-centre times.
    let t = Array1::from_iter(
        (0..n_frames).map(|k| (k * hop) as f64 / fs + (m as f64) / (2.0 * fs)),
    );

    Ok((s, f, t))
}

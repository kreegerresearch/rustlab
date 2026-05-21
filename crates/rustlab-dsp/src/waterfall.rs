//! Frequency waterfall: spectrogram data oriented for a downward-scrolling
//! display.
//!
//! Same segment-window-FFT loop as [`crate::stft::stft`], but the output
//! matrix is transposed and time-reversed so that **row 0 is the newest
//! segment** and rows below it look further back in time. The natural
//! display is then a heatmap with the image-convention y-axis (row 0 at
//! the top), giving the classic SDR waterfall look. Magnitudes are
//! returned in dB (`20·log10(|S|)`) so the data is plot-ready.
//!
//! Layout: `[n_time × n_freqs]` real matrix, rows indexed by time
//! (row 0 = newest segment, row `n_time-1` = first segment), columns
//! indexed by frequency bin (col 0 = DC). Pairs with the
//! `waterfall_stream` builtin in Phase 3 for live use.
//!
//! For the column-oriented spectrogram layout (rows = freq, cols = time,
//! col 0 = first segment), use [`crate::stft::stft`] directly.
//!
//! Numerical guard: `20·log10(0)` is `-∞`. A small additive epsilon
//! (`1e-12`) is added before the log to keep silent bins finite, matching
//! `examples/audio/spectrogram_monitor.rlab:79`.

use crate::error::DspError;
use crate::stft::stft;
use crate::welch::Sided;
use ndarray::Array1;
use rustlab_core::{CVector, RMatrix, RVector};

/// Frequency waterfall: magnitude spectrogram oriented for downward scroll.
///
/// Returns `(W, f, t)` where:
/// - `W` is an `[n_time × n_freqs]` real matrix of magnitudes in dB.
///   **Row 0 is the newest segment**, row `n_time-1` is the first segment.
///   Columns are frequency bins (col 0 = DC, col `n_freqs-1` = Nyquist for
///   one-sided output).
/// - `f` is the frequency axis in Hz (same as [`stft`]).
/// - `t` is the segment-centre time of each row, in seconds from the start
///   of `x`. Aligned with `W` rows, so `t` is **monotonically decreasing**
///   (`t[0]` is the latest segment, `t[n_time-1]` is the first).
///
/// All keyword-style arguments mirror [`stft`]: `nfft` is rounded up to
/// the next power of two; `Sided::Auto` resolves to one-sided for real
/// input and two-sided for complex.
pub fn waterfall(
    x: &CVector,
    fs: f64,
    window: &RVector,
    noverlap: usize,
    nfft: usize,
    sided: Sided,
) -> Result<(RMatrix, RVector, RVector), DspError> {
    let (s, f, t) = stft(x, fs, window, noverlap, nfft, sided)?;
    let n_freqs = s.nrows();
    let n_time = s.ncols();

    // Build W[row, col] = 20·log10(|S[col_freq=col, src_col=(n_time-1-row)]| + ε).
    // i.e. transpose S and reverse the time axis so row 0 holds the
    // newest segment's magnitudes.
    let mut w = RMatrix::zeros((n_time, n_freqs));
    let eps = 1e-12_f64;
    for row in 0..n_time {
        let src_col = n_time - 1 - row;
        for col in 0..n_freqs {
            w[(row, col)] = 20.0 * (s[(col, src_col)].norm() + eps).log10();
        }
    }

    // Reverse the time axis so t[0] = latest segment, aligned with W rows.
    let t_rev: RVector = Array1::from_iter(t.iter().rev().copied());

    Ok((w, f, t_rev))
}

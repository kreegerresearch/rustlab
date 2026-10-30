use crate::error::DspError;
use ndarray::Array1;
use num_complex::Complex;
use rustlab_core::{checked_elems, variable_too_large, CVector, ELEM_C64};

/// Upsample by `p`, apply FIR filter `h`, then downsample by `q`.
///
/// Uses a **polyphase decomposition**: the length-`N` filter is split into `p`
/// subfilters, each of length `⌈N/p⌉`.  For each output sample only one
/// subfilter is applied to a short window of the original (un-upsampled) input,
/// so the work per output sample is `⌈N/p⌉` multiply-adds rather than `N`.
/// Total complexity is `O(n_out · ⌈N/p⌉)`.
///
/// # Output length
///
/// ```text
/// n_out = ((n_in − 1) · p + N − 1) / q  +  1
/// ```
///
/// This matches the scipy / numpy convention: the upsampled signal has length
/// `(n_in − 1) · p + 1` (zeros inserted *between* samples, none appended after
/// the last sample), producing a full-length linear convolution with `h` before
/// the final downsampling step.
///
/// # Parameters
/// - `x` — complex input signal
/// - `h` — real FIR filter coefficients
/// - `p` — upsample factor (≥ 1)
/// - `q` — downsample factor (≥ 1)
///
/// # Special cases
/// - `p = 1, q = 1` — ordinary FIR filtering (equivalent to `convolve`)
/// - `p = 1, q > 1` — decimate: filter then keep every `q`-th sample
/// - `p > 1, q = 1` — interpolate: insert `p−1` zeros and filter
///
/// # Errors
/// Returns [`DspError::InvalidParameter`] if `p == 0`, `q == 0`, or `h` is empty.
pub fn upfirdn(x: &CVector, h: &[f64], p: usize, q: usize) -> Result<CVector, DspError> {
    if p == 0 {
        return Err(DspError::InvalidParameter("upfirdn: p must be >= 1".into()));
    }
    if q == 0 {
        return Err(DspError::InvalidParameter("upfirdn: q must be >= 1".into()));
    }
    if h.is_empty() {
        return Err(DspError::InvalidParameter(
            "upfirdn: filter h must be non-empty".into(),
        ));
    }
    if x.is_empty() {
        return Ok(Array1::zeros(0));
    }

    let n_x = x.len();
    let n_h = h.len();

    // Length of each polyphase subfilter: h[r], h[r+p], h[r+2p], ...
    let n_poly = (n_h + p - 1) / p;

    // Output length (full linear-convolution result, then decimated)
    // n_out = floor(((n_x - 1)*p + n_h - 1) / q) + 1
    let n_out = (n_x - 1)
        .checked_mul(p)
        .and_then(|t| t.checked_add(n_h - 1))
        .map(|t| t / q + 1)
        .ok_or_else(|| {
            DspError::InvalidParameter(variable_too_large("upfirdn", &[n_x, p, n_h, q]))
        })?;
    checked_elems(&[n_out], ELEM_C64)
        .map_err(|_| DspError::InvalidParameter(variable_too_large("upfirdn", &[n_out])))?;

    let mut y: CVector = Array1::zeros(n_out);

    for m in 0..n_out {
        let t = m * q; // virtual time in the upsampled coordinate
        let r = t % p; // selects subfilter: h[r], h[r+p], h[r+2p], ...
        let x_pos = t / p; // most-recent contributing sample index in x

        let mut acc = Complex::new(0.0f64, 0.0f64);
        for k in 0..n_poly {
            let h_idx = r + k * p;
            if h_idx >= n_h {
                break; // remainder of subfilter is zero-padded — nothing to add
            }
            let x_idx = x_pos as isize - k as isize;
            if x_idx >= 0 && (x_idx as usize) < n_x {
                acc += h[h_idx] * x[x_idx as usize];
            }
        }
        y[m] = acc;
    }

    Ok(y)
}

use super::design::{fir_bandpass, fir_highpass, fir_lowpass, FirFilter};
use crate::error::DspError;
use crate::fft::fft;
use crate::window::WindowFunction;
use ndarray::Array1;
use num_complex::Complex;
use rustlab_core::{CVector, RVector, C64};

/// Compute the Kaiser window shape parameter β from the desired stopband
/// attenuation (Harris 1978).
///
/// - `A >= 50` dB → β = 0.1102 * (A − 8.7)
/// - `21 <= A < 50` dB → β = 0.5842*(A−21)^0.4 + 0.07886*(A−21)
/// - `A < 21` dB → β = 0.0
pub fn kaiser_beta(stopband_attn_db: f64) -> f64 {
    let a = stopband_attn_db;
    if a >= 50.0 {
        0.1102 * (a - 8.7)
    } else if a >= 21.0 {
        let diff = a - 21.0;
        0.5842 * diff.powf(0.4) + 0.07886 * diff
    } else {
        0.0
    }
}

/// Compute the minimum number of taps needed for a Kaiser-windowed FIR filter
/// (Harris 1978). The result is always odd.
///
/// - `trans_bw_hz`: one-sided transition bandwidth in Hz
/// - `stopband_attn_db`: desired stopband attenuation in dB (positive value)
/// - `sample_rate`: sample rate in Hz
pub fn kaiser_num_taps(trans_bw_hz: f64, stopband_attn_db: f64, sample_rate: f64) -> usize {
    let a = stopband_attn_db;
    let d = if a > 21.0 { (a - 7.95) / 14.36 } else { 0.9222 };
    let delta_f = trans_bw_hz / sample_rate;
    let mut n = (d / delta_f).ceil() as usize + 1;
    if n % 2 == 0 {
        n += 1;
    }
    n
}

/// Design a Kaiser-windowed FIR lowpass filter.
///
/// Computes the required β and tap count from `trans_bw_hz` and
/// `stopband_attn_db`, then delegates to the standard windowed-sinc design.
///
/// # Errors
/// Returns [`DspError::InvalidKaiserSpec`] if `trans_bw_hz <= 0`, or
/// forwarded errors from the underlying FIR design if the cutoff is invalid.
pub fn fir_lowpass_kaiser(
    cutoff_hz: f64,
    trans_bw_hz: f64,
    stopband_attn_db: f64,
    sample_rate: f64,
) -> Result<FirFilter, DspError> {
    let (num_taps, window) = kaiser_design_params(trans_bw_hz, stopband_attn_db, sample_rate)?;
    fir_lowpass(num_taps, cutoff_hz, sample_rate, window)
}

/// Design a Kaiser-windowed FIR highpass filter.
pub fn fir_highpass_kaiser(
    cutoff_hz: f64,
    trans_bw_hz: f64,
    stopband_attn_db: f64,
    sample_rate: f64,
) -> Result<FirFilter, DspError> {
    let (num_taps, window) = kaiser_design_params(trans_bw_hz, stopband_attn_db, sample_rate)?;
    fir_highpass(num_taps, cutoff_hz, sample_rate, window)
}

/// Design a Kaiser-windowed FIR bandpass filter.
pub fn fir_bandpass_kaiser(
    low_hz: f64,
    high_hz: f64,
    trans_bw_hz: f64,
    stopband_attn_db: f64,
    sample_rate: f64,
) -> Result<FirFilter, DspError> {
    let (num_taps, window) = kaiser_design_params(trans_bw_hz, stopband_attn_db, sample_rate)?;
    fir_bandpass(num_taps, low_hz, high_hz, sample_rate, window)
}

/// Shared prologue for the Kaiser design functions: validate the spec, then
/// derive the tap count and Kaiser window (β) from the transition bandwidth
/// and stopband attenuation.
fn kaiser_design_params(
    trans_bw_hz: f64,
    stopband_attn_db: f64,
    sample_rate: f64,
) -> Result<(usize, WindowFunction), DspError> {
    validate_kaiser(trans_bw_hz, stopband_attn_db)?;
    let beta = kaiser_beta(stopband_attn_db);
    let num_taps = kaiser_num_taps(trans_bw_hz, stopband_attn_db, sample_rate);
    Ok((num_taps, WindowFunction::Kaiser { beta }))
}

/// Design a FIR notch filter via spectral inversion of a bandpass.
///
/// Designs a bandpass at `[center_hz - bandwidth_hz/2, center_hz + bandwidth_hz/2]`
/// then inverts the spectrum: `h_notch[n] = −h_bp[n]`,
/// `h_notch[center_tap] += 1.0`.
///
/// # Parameters
/// - `center_hz`: center frequency of the notch in Hz
/// - `bandwidth_hz`: total bandwidth to reject in Hz
/// - `sample_rate`: sample rate in Hz
/// - `num_taps`: number of filter taps (odd recommended)
/// - `window`: window function for the prototype bandpass
pub fn fir_notch(
    center_hz: f64,
    bandwidth_hz: f64,
    sample_rate: f64,
    num_taps: usize,
    window: WindowFunction,
) -> Result<FirFilter, DspError> {
    let low_hz = center_hz - bandwidth_hz / 2.0;
    let high_hz = center_hz + bandwidth_hz / 2.0;
    let bp = fir_bandpass(num_taps, low_hz, high_hz, sample_rate, window)?;

    let center_tap = (num_taps - 1) / 2;
    let mut h_notch = -bp.coefficients;
    h_notch[center_tap] = h_notch[center_tap] + Complex::new(1.0, 0.0);

    Ok(FirFilter {
        coefficients: h_notch,
    })
}

/// Complex frequency response of a filter.
///
/// For `n_points > 1` the frequencies are `f_k = k · fs / (2(n-1))`,
/// `k = 0 .. n-1`, so the last sample is Nyquist. That is the first `n`
/// bins of a length-`2(n-1)` DFT. `n_points == 1` is Nyquist alone.
/// A filter longer than the DFT is summed directly on the same grid.
///
/// # Errors
/// Returns [`DspError::InvalidParameter`] if `n_points == 0`.
pub fn freqz(
    h: &CVector,
    n_points: usize,
    sample_rate: f64,
) -> Result<(RVector, CVector), DspError> {
    if n_points == 0 {
        return Err(DspError::InvalidParameter(
            "freqz: n_points must be > 0".to_string(),
        ));
    }
    if n_points == 1 {
        let mut acc = C64::new(0.0, 0.0);
        for (m, &hm) in h.iter().enumerate() {
            let sign = if m % 2 == 0 { 1.0 } else { -1.0 };
            acc += hm * Complex::new(sign, 0.0);
        }
        return Ok((
            Array1::from_elem(1, sample_rate / 2.0),
            Array1::from_vec(vec![acc]),
        ));
    }
    // Length 2(n-1). Bin k = n-1 is exactly fs/2. When that length is
    // not a power of two the FFT is Bluestein, which is slower than the
    // old length-2n radix-2 transform (n = 4096: 8190 vs 8192).
    let nfft = 2 * (n_points - 1);
    let freqs: RVector =
        Array1::from_iter((0..n_points).map(|k| k as f64 * sample_rate / nfft as f64));
    let h_out: CVector = if h.len() <= nfft {
        let mut padded = vec![Complex::new(0.0, 0.0); nfft];
        for (i, &c) in h.iter().enumerate() {
            padded[i] = c;
        }
        let spectrum = fft(&Array1::from_vec(padded))?;
        Array1::from_iter(spectrum.iter().take(n_points).copied())
    } else {
        use std::f64::consts::PI;
        Array1::from_iter((0..n_points).map(|k| {
            let mut acc = C64::new(0.0, 0.0);
            for (m, &hm) in h.iter().enumerate() {
                let ang = -2.0 * PI * (k as f64) * (m as f64) / nfft as f64;
                acc += hm * Complex::new(ang.cos(), ang.sin());
            }
            acc
        }))
    };

    Ok((freqs, h_out))
}

fn validate_kaiser(trans_bw_hz: f64, stopband_attn_db: f64) -> Result<(), DspError> {
    if trans_bw_hz <= 0.0 {
        return Err(DspError::InvalidKaiserSpec(format!(
            "trans_bw_hz must be > 0, got {trans_bw_hz}"
        )));
    }
    if stopband_attn_db <= 0.0 {
        return Err(DspError::InvalidKaiserSpec(format!(
            "stopband_attn_db must be > 0, got {stopband_attn_db}"
        )));
    }
    Ok(())
}

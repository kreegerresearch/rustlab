use crate::convolution::convolve;
use crate::error::DspError;
use crate::window::WindowFunction;
use ndarray::Array1;
use num_complex::Complex;
use rustlab_core::{CVector, CoreError, Filter, C64};
use std::f64::consts::PI;

/// A linear-phase FIR filter represented by its complex tap coefficients.
///
/// The coefficients are stored as a [`CVector`] (complex-valued 1-D ndarray).
/// For filters designed by the windowed-sinc functions in this module the
/// imaginary parts are zero, but the complex representation lets `FirFilter`
/// process analytic (complex baseband) signals without modification.
///
/// Filtering is performed via direct-form linear convolution (see
/// [`convolve`]) through the [`Filter`] impl.
pub struct FirFilter {
    pub coefficients: CVector,
}

impl Filter for FirFilter {
    /// Apply the FIR filter via direct-form convolution.
    fn apply(&self, input: &CVector) -> Result<CVector, CoreError> {
        convolve(input, &self.coefficients)
    }

    /// Evaluate H(e^{j*omega}) at `n_points` normalized frequencies in [0, 0.5).
    fn frequency_response(&self, n_points: usize) -> Result<CVector, CoreError> {
        if n_points == 0 {
            return Ok(Array1::zeros(0));
        }
        let h = &self.coefficients;
        let response = (0..n_points)
            .map(|i| {
                // omega in [0, 0.5) normalized frequency -> [0, pi) radians/sample
                let omega = PI * i as f64 / n_points as f64;
                h.iter()
                    .enumerate()
                    .map(|(k, &hk)| {
                        let angle = -omega * k as f64;
                        hk * Complex::new(angle.cos(), angle.sin())
                    })
                    .sum::<C64>()
            })
            .collect::<Vec<_>>();

        Ok(Array1::from_vec(response))
    }
}

/// Normalized sinc: sinc(x) = sin(pi*x)/(pi*x), sinc(0) = 1.
fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-15 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// Validate FIR design parameters.
fn validate_fir(num_taps: usize, cutoff_hz: f64, sample_rate: f64) -> Result<(), DspError> {
    if num_taps == 0 {
        return Err(DspError::InvalidOrder(num_taps));
    }
    let nyquist = sample_rate / 2.0;
    if cutoff_hz <= 0.0 || cutoff_hz >= nyquist {
        return Err(DspError::InvalidCutoff {
            cutoff: cutoff_hz,
            nyquist,
        });
    }
    Ok(())
}

/// Validate FIR design parameters for a two-cutoff (band) design: both edges
/// must individually satisfy [`validate_fir`], and `low_hz < high_hz`.
fn validate_fir_range(
    num_taps: usize,
    low_hz: f64,
    high_hz: f64,
    sample_rate: f64,
) -> Result<(), DspError> {
    validate_fir(num_taps, low_hz, sample_rate)?;
    validate_fir(num_taps, high_hz, sample_rate)?;
    if low_hz >= high_hz {
        return Err(DspError::Core(rustlab_core::CoreError::InvalidParameter(
            format!("low_hz ({low_hz}) must be less than high_hz ({high_hz})"),
        )));
    }
    Ok(())
}

/// Design a windowed-sinc FIR lowpass filter.
///
/// Uses the *windowed-sinc* method: the ideal (infinite-length) lowpass impulse
/// response `h[n] = 2·fc·sinc(2·fc·(n − M/2))` is truncated to `num_taps`
/// samples and multiplied element-wise by the chosen `window` to suppress the
/// ringing that would result from abrupt truncation.
///
/// # Parameters
/// - `num_taps`: total number of filter taps. **An odd value is strongly
///   recommended** — it places the center tap exactly at the mid-point and
///   guarantees a Type-I linear-phase FIR with a symmetric impulse response and
///   zero group-delay distortion.
/// - `cutoff_hz`: the −3 dB cutoff frequency in Hz. Must be strictly between 0
///   and the Nyquist frequency (`sample_rate / 2`).
/// - `sample_rate`: the sample rate of the signal in Hz.
/// - `window`: window function applied to the sinc kernel to control
///   sidelobe leakage and transition-band width.
///
/// # Errors
/// Returns [`DspError::InvalidOrder`] if `num_taps == 0`, or
/// [`DspError::InvalidCutoff`] if `cutoff_hz` is out of range.
pub fn fir_lowpass(
    num_taps: usize,
    cutoff_hz: f64,
    sample_rate: f64,
    window: WindowFunction,
) -> Result<FirFilter, DspError> {
    validate_fir(num_taps, cutoff_hz, sample_rate)?;

    let fc = cutoff_hz / sample_rate; // normalized cutoff in [0, 0.5)
    let m = (num_taps - 1) as f64 / 2.0; // center tap (may be fractional for even num_taps)
    let win = window.generate(num_taps);

    let coeffs: CVector = Array1::from_iter((0..num_taps).map(|n| {
        let h = 2.0 * fc * sinc(2.0 * fc * (n as f64 - m));
        Complex::new(h * win[n], 0.0)
    }));

    // Normalize for unity DC gain (H(0) = sum of taps = 1.0).
    // Windowing can reduce the passband gain,
    // especially for narrow-band filters where many sinc tail samples are
    // tapered away by the window.
    let dc_gain: f64 = coeffs.iter().map(|c| c.re).sum();
    let coeffs = if dc_gain.abs() > 1e-12 {
        coeffs.mapv(|c| Complex::new(c.re / dc_gain, c.im / dc_gain))
    } else {
        coeffs
    };

    Ok(FirFilter {
        coefficients: coeffs,
    })
}

/// Design a windowed-sinc FIR highpass filter via spectral inversion.
///
/// First designs a prototype lowpass filter with the same parameters, then
/// applies *spectral inversion*: negates all coefficients and adds 1 to the
/// center tap (`h_hp[n] = δ[n − M] − h_lp[n]`). Subtracting the lowpass
/// response from a unit-delay (all-pass) flips the passband and stopband,
/// producing a highpass with the same transition width and sidelobe behaviour
/// as the prototype.
///
/// # Parameters
/// Same as [`fir_lowpass`]: `num_taps` (odd recommended), `cutoff_hz`,
/// `sample_rate`, and `window`.
///
/// # Errors
/// Returns [`DspError::InvalidOrder`] or [`DspError::InvalidCutoff`] on invalid
/// input, forwarded from the internal lowpass design.
pub fn fir_highpass(
    num_taps: usize,
    cutoff_hz: f64,
    sample_rate: f64,
    window: WindowFunction,
) -> Result<FirFilter, DspError> {
    validate_fir(num_taps, cutoff_hz, sample_rate)?;

    // Design prototype lowpass
    let lp = fir_lowpass(num_taps, cutoff_hz, sample_rate, window)?;
    let m = (num_taps - 1) / 2;

    // Spectral inversion: h_hp[n] = delta[n - M] - h_lp[n]
    let mut hp_coeffs = -lp.coefficients;
    hp_coeffs[m] = hp_coeffs[m] + Complex::new(1.0, 0.0);

    Ok(FirFilter {
        coefficients: hp_coeffs,
    })
}

/// Design a windowed-sinc FIR bandpass filter.
///
/// Uses a *difference-of-lowpass* approach: two lowpass filters are designed
/// with cutoff frequencies `high_hz` and `low_hz` respectively, and their
/// impulse responses are subtracted element-wise:
///
/// ```text
/// h_bp[n] = h_lp(high_hz)[n] − h_lp(low_hz)[n]
/// ```
///
/// This works because both lowpass filters pass the band `[0, low_hz)`;
/// subtracting cancels that overlap and leaves only the band `[low_hz, high_hz)`.
///
/// # Parameters
/// - `num_taps`: number of filter taps (odd recommended for linear phase).
/// - `low_hz`: lower −3 dB edge of the passband in Hz. Must be > 0 and < `high_hz`.
/// - `high_hz`: upper −3 dB edge of the passband in Hz. Must be < Nyquist.
/// - `sample_rate`: sample rate in Hz.
/// - `window`: window function applied to both prototype lowpass kernels.
///
/// # Errors
/// Returns [`DspError::InvalidOrder`], [`DspError::InvalidCutoff`], or
/// [`DspError::Core`] if any parameter is invalid or if `low_hz >= high_hz`.
pub fn fir_bandpass(
    num_taps: usize,
    low_hz: f64,
    high_hz: f64,
    sample_rate: f64,
    window: WindowFunction,
) -> Result<FirFilter, DspError> {
    validate_fir_range(num_taps, low_hz, high_hz, sample_rate)?;

    let lp_high = fir_lowpass(num_taps, high_hz, sample_rate, window.clone())?;
    let lp_low = fir_lowpass(num_taps, low_hz, sample_rate, window)?;

    let bp_coeffs = lp_high.coefficients - lp_low.coefficients;
    Ok(FirFilter {
        coefficients: bp_coeffs,
    })
}

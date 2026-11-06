use crate::error::DspError;
use ndarray::Array1;
use num_complex::Complex;
use rustlab_core::{CVector, CoreError, Filter, C64};
use std::f64::consts::PI;

/// An IIR filter defined by its numerator (`b`) and denominator (`a`)
/// polynomial coefficients in direct-form II transposed representation.
///
/// The z-domain transfer function is:
///
/// ```text
/// H(z) = (b[0] + b[1]·z⁻¹ + … + b[M]·z⁻ᴹ)
///       / (1   + a[1]·z⁻¹ + … + a[N]·z⁻ᴺ)
/// ```
///
/// - `b`: numerator (feedforward) coefficients, length M+1.
/// - `a`: denominator (feedback) coefficients, length N+1. `a[0]` is always
///   **1.0** (monic denominator); non-unity values are not supported.
///
/// When built by [`butterworth_lowpass`] or [`butterworth_highpass`] the
/// coefficients are the product of cascaded biquad (2nd-order) sections, each
/// derived via the bilinear transform with frequency pre-warping.
pub struct IirFilter {
    pub b: Vec<f64>, // numerator coefficients
    pub a: Vec<f64>, // denominator coefficients (a[0] = 1)
}

impl IirFilter {
    /// Create a new IirFilter from b and a coefficient vectors.
    pub fn new(b: Vec<f64>, a: Vec<f64>) -> Self {
        IirFilter { b, a }
    }

    /// Apply this filter to a real-valued signal (as complex with zero imaginary part).
    /// Uses direct-form II transposed implementation.
    pub(crate) fn apply_real(&self, input: &[f64]) -> Vec<f64> {
        let nb = self.b.len();
        let na = self.a.len();
        let state_len = nb.max(na) - 1;
        let mut w = vec![0.0f64; state_len];
        let mut output = vec![0.0f64; input.len()];

        for (n, &x) in input.iter().enumerate() {
            // Direct-form II transposed
            let y = self.b[0] * x + if state_len > 0 { w[0] } else { 0.0 };
            output[n] = y;
            for i in 0..state_len {
                let b_term = if i + 1 < nb { self.b[i + 1] * x } else { 0.0 };
                let a_term = if i + 1 < na { self.a[i + 1] * y } else { 0.0 };
                w[i] = b_term - a_term + if i + 1 < state_len { w[i + 1] } else { 0.0 };
            }
        }
        output
    }

    /// Zero-phase forward-backward filter matching Octave's filtfilt behavior.
    ///
    /// Algorithm (Likhterov & Kopeika 2003 / Octave signal package):
    ///   1. Compute steady-state initial conditions si from b, a coefficients.
    ///   2. Extend signal with odd reflection at both ends (lrefl = 3*(n-1) samples).
    ///   3. Forward filter with IC = si * v[0].
    ///   4. Reverse filter with IC = si * v[end].
    ///   5. Extract center portion corresponding to original signal.
    pub fn filtfilt(&self, x: &[f64]) -> Result<Vec<f64>, CoreError> {
        let nx = x.len();
        if nx == 0 {
            return Ok(vec![]);
        }
        let nb = self.b.len();
        let na = self.a.len();
        let n = nb.max(na);
        if n == 0 {
            return Ok(vec![]);
        }
        // `3 * (n - 1)` and `nx + 2 * lrefl` overflow for a huge coefficient
        // vector, and `Vec::with_capacity` then panics.
        let lrefl = (n - 1).checked_mul(3).ok_or_else(|| {
            CoreError::InvalidParameter(rustlab_core::variable_too_large("filtfilt", &[n]))
        })?;
        let ext = lrefl
            .checked_mul(2)
            .and_then(|p| p.checked_add(nx))
            .ok_or_else(|| {
                CoreError::InvalidParameter(rustlab_core::variable_too_large(
                    "filtfilt",
                    &[nx, lrefl],
                ))
            })?;
        rustlab_core::checked_elems(&[ext], rustlab_core::ELEM_F64).map_err(|_| {
            CoreError::InvalidParameter(rustlab_core::variable_too_large("filtfilt", &[ext]))
        })?;

        // Pad b and a to length n
        let b: Vec<f64> = {
            let mut v = self.b.clone();
            v.resize(n, 0.0);
            v
        };
        let a: Vec<f64> = {
            let mut v = self.a.clone();
            v.resize(n, 0.0);
            v
        };

        // Steady-state DC gain
        let sum_b: f64 = b.iter().sum();
        let sum_a: f64 = a.iter().sum();
        let kdc = if sum_a.abs() > 1e-300 {
            sum_b / sum_a
        } else {
            0.0
        };

        // si = fliplr(cumsum(fliplr(b - kdc*a))); si(1) = []
        // i.e., b - kdc*a reversed, cumulative sum, reversed, drop first element
        let diff: Vec<f64> = b
            .iter()
            .zip(a.iter())
            .map(|(&bi, &ai)| bi - kdc * ai)
            .collect();
        // fliplr then cumsum
        let mut rev_diff: Vec<f64> = diff.iter().rev().cloned().collect();
        // cumsum in-place
        for i in 1..rev_diff.len() {
            rev_diff[i] += rev_diff[i - 1];
        }
        // fliplr again
        let si_full: Vec<f64> = rev_diff.into_iter().rev().collect();
        // Drop first element (si(1) = [] in Octave)
        let si: Vec<f64> = si_full[1..].to_vec();
        let state_len = si.len(); // = n - 1

        // Extend signal with odd reflection:
        //   v = [2*x[0] - x[lrefl:-1:1], x, 2*x[n-1] - x[n-2:-1:n-1-lrefl]]
        let mut v: Vec<f64> = Vec::with_capacity(ext);
        for i in (1..=lrefl).rev() {
            let idx = i.min(nx - 1);
            v.push(2.0 * x[0] - x[idx]);
        }
        v.extend_from_slice(x);
        for i in 1..=lrefl {
            let idx = nx.saturating_sub(1 + i);
            v.push(2.0 * x[nx - 1] - x[idx]);
        }

        // Forward filter with IC = si * v[0]
        let v0 = v[0];
        let ic_fwd: Vec<f64> = si.iter().map(|&s| s * v0).collect();
        let y_fwd = self.apply_real_with_ic(&v, &ic_fwd, state_len);

        // Reverse
        let mut v_rev: Vec<f64> = y_fwd.into_iter().rev().collect();
        let v_end = v_rev[0]; // after reversal, first element = last of y_fwd
        let ic_bwd: Vec<f64> = si.iter().map(|&s| s * v_end).collect();
        let y_bwd = self.apply_real_with_ic(&v_rev.clone(), &ic_bwd, state_len);

        // Reverse back and extract center
        v_rev = y_bwd.into_iter().rev().collect();
        Ok(v_rev[lrefl..lrefl + nx].to_vec())
    }

    /// Apply filter with given initial state (direct-form II transposed).
    fn apply_real_with_ic(&self, input: &[f64], ic: &[f64], state_len: usize) -> Vec<f64> {
        let nb = self.b.len();
        let na = self.a.len();
        let mut w = vec![0.0f64; state_len];
        for (i, &v) in ic.iter().enumerate() {
            if i < state_len {
                w[i] = v;
            }
        }
        let mut output = vec![0.0f64; input.len()];
        for (idx, &x) in input.iter().enumerate() {
            let y = self.b[0] * x + if state_len > 0 { w[0] } else { 0.0 };
            output[idx] = y;
            for i in 0..state_len {
                let b_term = if i + 1 < nb { self.b[i + 1] * x } else { 0.0 };
                let a_term = if i + 1 < na { self.a[i + 1] * y } else { 0.0 };
                w[i] = b_term - a_term + if i + 1 < state_len { w[i + 1] } else { 0.0 };
            }
        }
        output
    }
}

impl Filter for IirFilter {
    fn apply(&self, input: &CVector) -> Result<CVector, CoreError> {
        // Apply filter to real and imaginary parts separately
        let re: Vec<f64> = input.iter().map(|c| c.re).collect();
        let im: Vec<f64> = input.iter().map(|c| c.im).collect();

        let re_out = self.apply_real(&re);
        let im_out = self.apply_real(&im);

        let result = Array1::from_iter(
            re_out
                .iter()
                .zip(im_out.iter())
                .map(|(&r, &i)| Complex::new(r, i)),
        );
        Ok(result)
    }

    fn frequency_response(&self, n_points: usize) -> Result<CVector, CoreError> {
        if n_points == 0 {
            return Ok(Array1::zeros(0));
        }

        let response = (0..n_points)
            .map(|i| {
                // Normalized frequency omega in [0, pi)
                let omega = PI * i as f64 / n_points as f64;

                // Evaluate B(z) and A(z) at z = e^{j*omega}
                let bz: C64 = self
                    .b
                    .iter()
                    .enumerate()
                    .map(|(k, &bk)| {
                        let angle = -(omega * k as f64);
                        Complex::new(bk, 0.0) * Complex::new(angle.cos(), angle.sin())
                    })
                    .sum();

                let az: C64 = self
                    .a
                    .iter()
                    .enumerate()
                    .map(|(k, &ak)| {
                        let angle = -(omega * k as f64);
                        Complex::new(ak, 0.0) * Complex::new(angle.cos(), angle.sin())
                    })
                    .sum();

                if az.norm() < 1e-15 {
                    Complex::new(0.0, 0.0)
                } else {
                    bz / az
                }
            })
            .collect::<Vec<_>>();

        Ok(Array1::from_vec(response))
    }
}

/// Polynomial multiplication: c = a * b (convolution of coefficient arrays).
fn poly_multiply(a: &[f64], b: &[f64]) -> Vec<f64> {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }
    let mut c = vec![0.0f64; a.len() + b.len() - 1];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            c[i + j] += ai * bj;
        }
    }
    c
}

// ---------------------------------------------------------------------------
// Butterworth design helpers
// ---------------------------------------------------------------------------

/// Design an Nth-order Butterworth lowpass IIR filter.
///
/// The design procedure follows three steps:
///
/// 1. **Analog prototype**: the maximally-flat Butterworth poles are placed on
///    the unit circle in the left-half s-plane. The prototype cutoff is always 1
///    rad/s; the real cutoff is introduced in the next step.
///
/// 2. **Frequency pre-warping**: the digital cutoff `cutoff_hz` is mapped to an
///    analog cutoff via `K = tan(π · cutoff_hz / sample_rate)`. This corrects
///    for the non-linear frequency compression introduced by the bilinear
///    transform so that the −3 dB point lands exactly at `cutoff_hz`.
///
/// 3. **Bilinear transform**: each analog pole pair is converted to a discrete
///    biquad section using `s = (1 − z⁻¹)/(1 + z⁻¹)` (scaled by K). An odd
///    order includes one first-order section for the real pole. All sections are
///    then cascaded (polynomial-multiplied) into a single `b`/`a` representation.
///
/// # Parameters
/// - `order`: filter order N ≥ 1. Higher order → steeper roll-off (20·N dB/decade).
/// - `cutoff_hz`: the −3 dB cutoff frequency in Hz.
/// - `sample_rate`: the sample rate in Hz.
///
/// # Errors
/// Returns [`DspError::InvalidOrder`] if `order == 0`, or
/// [`DspError::InvalidCutoff`] if `cutoff_hz` is out of range.
pub fn butterworth_lowpass(
    order: usize,
    cutoff_hz: f64,
    sample_rate: f64,
) -> Result<IirFilter, DspError> {
    if order == 0 {
        return Err(DspError::InvalidOrder(order));
    }
    let nyquist = sample_rate / 2.0;
    if cutoff_hz <= 0.0 || cutoff_hz >= nyquist {
        return Err(DspError::InvalidCutoff {
            cutoff: cutoff_hz,
            nyquist,
        });
    }

    let sections = butterworth_lp_sections(order, cutoff_hz, sample_rate);
    // Combine all sections into a single b/a representation
    let mut b_total = vec![1.0f64];
    let mut a_total = vec![1.0f64];
    for section in &sections {
        b_total = poly_multiply(&b_total, &section.b);
        a_total = poly_multiply(&a_total, &section.a);
    }
    Ok(IirFilter {
        b: b_total,
        a: a_total,
    })
}

/// Design an Nth-order Butterworth highpass IIR filter.
///
/// Follows the same bilinear-transform procedure as [`butterworth_lowpass`] but
/// applies a lowpass-to-highpass frequency transformation in the analog domain
/// before discretising: the s-plane prototype poles are reflected across the
/// imaginary axis, and the biquad numerator coefficients become `[Q, −2Q, Q]`
/// instead of `[K²Q, 2K²Q, K²Q]`, producing a highpass response.
///
/// Frequency pre-warping ensures the −3 dB point falls exactly at `cutoff_hz`.
/// Cascaded biquad sections (plus a first-order section for odd orders) are
/// polynomial-multiplied into a single `b`/`a` representation.
///
/// # Parameters
/// - `order`: filter order N ≥ 1.
/// - `cutoff_hz`: the −3 dB cutoff frequency in Hz.
/// - `sample_rate`: the sample rate in Hz.
///
/// # Errors
/// Returns [`DspError::InvalidOrder`] if `order == 0`, or
/// [`DspError::InvalidCutoff`] if `cutoff_hz` is out of range.
pub fn butterworth_highpass(
    order: usize,
    cutoff_hz: f64,
    sample_rate: f64,
) -> Result<IirFilter, DspError> {
    if order == 0 {
        return Err(DspError::InvalidOrder(order));
    }
    let nyquist = sample_rate / 2.0;
    if cutoff_hz <= 0.0 || cutoff_hz >= nyquist {
        return Err(DspError::InvalidCutoff {
            cutoff: cutoff_hz,
            nyquist,
        });
    }

    let sections = butterworth_hp_sections(order, cutoff_hz, sample_rate);
    let mut b_total = vec![1.0f64];
    let mut a_total = vec![1.0f64];
    for section in &sections {
        b_total = poly_multiply(&b_total, &section.b);
        a_total = poly_multiply(&a_total, &section.a);
    }
    Ok(IirFilter {
        b: b_total,
        a: a_total,
    })
}

/// Build Butterworth lowpass as a list of 1st/2nd order IirFilter sections.
fn butterworth_lp_sections(order: usize, cutoff_hz: f64, sample_rate: f64) -> Vec<IirFilter> {
    // Pre-warped analog cutoff
    let k = (PI * cutoff_hz / sample_rate).tan();
    let mut sections = Vec::new();

    // If order is odd, add a 1st-order section first
    if order % 2 == 1 {
        sections.push(first_order_lp(k));
    }

    // Add 2nd-order sections
    let n_biquads = order / 2;
    for i in 0..n_biquads {
        // Butterworth left-half-plane poles lie at angles in (π/2, π).
        // For the i-th conjugate pair (0-indexed): θ = π*(order + 2i + 1) / (2*order)
        // This places poles correctly so cos(θ) < 0 and Q = -1/(2*cos(θ)) > 0.
        let theta = PI * (order + 2 * i + 1) as f64 / (2 * order) as f64;
        let q = -0.5 / theta.cos(); // Q > 0 since cos(θ) < 0 for θ ∈ (π/2, π)
        sections.push(second_order_lp(k, q));
    }

    sections
}

/// Build Butterworth highpass as a list of 1st/2nd order IirFilter sections.
fn butterworth_hp_sections(order: usize, cutoff_hz: f64, sample_rate: f64) -> Vec<IirFilter> {
    let k = (PI * cutoff_hz / sample_rate).tan();
    let mut sections = Vec::new();

    if order % 2 == 1 {
        sections.push(first_order_hp(k));
    }

    let n_biquads = order / 2;
    for i in 0..n_biquads {
        let theta = PI * (order + 2 * i + 1) as f64 / (2 * order) as f64;
        let q = -0.5 / theta.cos();
        sections.push(second_order_hp(k, q));
    }

    sections
}

/// 1st-order Butterworth lowpass section via bilinear transform.
/// H(s) = 1 / (s/omega_c + 1)  →  bilinear with K = tan(pi*fc/fs)
/// b = [K, K] / (1 + K),  a = [1, (K-1)/(K+1)]
fn first_order_lp(k: f64) -> IirFilter {
    let norm = 1.0 + k;
    let b0 = k / norm;
    let b1 = k / norm;
    let a1 = (k - 1.0) / norm;
    IirFilter {
        b: vec![b0, b1],
        a: vec![1.0, a1],
    }
}

/// 1st-order Butterworth highpass section via bilinear transform.
/// H(s) = (s/omega_c) / (s/omega_c + 1)
/// b = [1, -1] / (1 + K),  a = [1, (K-1)/(K+1)]
fn first_order_hp(k: f64) -> IirFilter {
    let norm = 1.0 + k;
    let b0 = 1.0 / norm;
    let b1 = -1.0 / norm;
    let a1 = (k - 1.0) / norm;
    IirFilter {
        b: vec![b0, b1],
        a: vec![1.0, a1],
    }
}

/// 2nd-order Butterworth lowpass biquad section.
/// Derived from the analog prototype H(s) = 1/(s^2 + s/Q + 1) via bilinear transform
/// with pre-warping K = tan(pi*fc/fs).
/// norm = K^2*Q + K + Q  (= Q*(K^2+1) + K)
fn second_order_lp(k: f64, q: f64) -> IirFilter {
    let k2 = k * k;
    // norm = K^2*Q + K + Q
    let norm = k2 * q + k + q;
    let b0 = k2 * q / norm;
    let b1 = 2.0 * k2 * q / norm;
    let b2 = k2 * q / norm;
    let a1 = 2.0 * q * (k2 - 1.0) / norm;
    let a2 = (k2 * q - k + q) / norm;
    IirFilter {
        b: vec![b0, b1, b2],
        a: vec![1.0, a1, a2],
    }
}

/// 2nd-order Butterworth highpass biquad section.
/// H_hp(s) = s^2 / (s^2 + (1/Q)*s + 1) (analog prototype)
fn second_order_hp(k: f64, q: f64) -> IirFilter {
    let k2 = k * k;
    let norm = k2 * q + k + q;
    // HP: swap roles: b comes from s^2 numerator
    // b = [Q, -2Q, Q] / norm
    // a same as LP
    let b0 = q / norm;
    let b1 = -2.0 * q / norm;
    let b2 = q / norm;
    let a1 = 2.0 * q * (k2 - 1.0) / norm;
    let a2 = (k2 * q - k + q) / norm;
    IirFilter {
        b: vec![b0, b1, b2],
        a: vec![1.0, a1, a2],
    }
}

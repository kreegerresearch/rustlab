use crate::error::DspError;
use ndarray::Array1;
use num_complex::Complex;
use rustlab_core::{CVector, CoreError, RVector, Transform, C64};
use std::f64::consts::PI;

mod bluestein;

/// FFT transform implementing the [`Transform`] trait.
///
/// `inverse(forward(x))` is an exact-length round trip for every input
/// length (both directions are length-preserving).
pub struct FftTransform;

impl Transform for FftTransform {
    fn forward(&self, input: &CVector) -> Result<CVector, CoreError> {
        fft(input).map_err(|e| CoreError::InvalidParameter(e.to_string()))
    }
    fn inverse(&self, input: &CVector) -> Result<CVector, CoreError> {
        ifft(input).map_err(|e| CoreError::InvalidParameter(e.to_string()))
    }
}

/// n-point DFT of `buf` in place (`n == buf.len()`). Powers of two take
/// the radix-2 fast path; every other length uses the Bluestein chirp-z
/// transform. A 0- or 1-point DFT is the identity.
pub(crate) fn dft_any_len(buf: &mut Vec<C64>, inverse: bool) {
    let n = buf.len();
    if n <= 1 {
        return;
    }
    if n.is_power_of_two() {
        fft_inplace(buf, inverse);
    } else {
        *buf = bluestein::bluestein(buf, inverse);
    }
}

/// Zero-pad or truncate `x` to exactly `n`, then run the n-point DFT.
fn transform(x: &CVector, n: usize, inverse: bool) -> CVector {
    let mut buf: Vec<C64> = x
        .iter()
        .copied()
        .chain(std::iter::repeat(Complex::new(0.0, 0.0)))
        .take(n)
        .collect();
    dft_any_len(&mut buf, inverse);
    Array1::from_vec(buf)
}

/// Compute the DFT of a complex vector.
///
/// Length-preserving: returns exactly `x.len()` bins, so a frequency axis
/// built from the input length (`fftfreq(x.len(), fs)`) is always correct.
/// Powers of two use the radix-2 fast path; any other length uses the
/// Bluestein chirp-z transform.
pub fn fft(x: &CVector) -> Result<CVector, DspError> {
    Ok(transform(x, x.len(), false))
}

/// n-point DFT: zero-pad or truncate `x` to exactly `n`, then transform.
///
/// Errors if `n == 0`.
pub fn fft_n(x: &CVector, n: usize) -> Result<CVector, DspError> {
    if n == 0 {
        return Err(DspError::InvalidParameter(
            "fft: n must be a positive integer".to_string(),
        ));
    }
    Ok(transform(x, n, false))
}

/// Compute the inverse DFT of a complex vector (1/n scaling).
///
/// Accepts any input length and returns exactly that many samples.
pub fn ifft(x: &CVector) -> Result<CVector, DspError> {
    Ok(transform(x, x.len(), true))
}

/// n-point inverse DFT: zero-pad or truncate `x` to exactly `n`, then
/// invert.
///
/// Errors if `n == 0`.
pub fn ifft_n(x: &CVector, n: usize) -> Result<CVector, DspError> {
    if n == 0 {
        return Err(DspError::InvalidParameter(
            "ifft: n must be a positive integer".to_string(),
        ));
    }
    Ok(transform(x, n, true))
}

/// Shift the zero-frequency component to the center of the spectrum.
///
/// For a vector of length `n`, rotates the data so that the element at index
/// `ceil(n/2)` moves to the front.
pub fn fftshift(x: &CVector) -> CVector {
    let n = x.len();
    if n == 0 {
        return Array1::zeros(0);
    }
    let half = (n + 1) / 2; // ceil(n/2)
    let mut out: Vec<C64> = Vec::with_capacity(n);
    for i in half..n {
        out.push(x[i]);
    }
    for i in 0..half {
        out.push(x[i]);
    }
    Array1::from_vec(out)
}

/// Return the FFT sample frequencies for a given length and sample rate.
///
/// Bins `0..ceil(n/2)` are positive frequencies, the rest are negative.
pub fn fftfreq(n: usize, sample_rate: f64) -> RVector {
    let half = (n + 1) / 2; // number of non-negative frequency bins
    Array1::from_iter((0..n).map(|k| {
        if k < half {
            k as f64 * sample_rate / n as f64
        } else {
            (k as f64 - n as f64) * sample_rate / n as f64
        }
    }))
}

/// In-place FFT, no padding, no error checking. Input length must be a power of two.
pub(crate) fn fft_raw(x: &[C64]) -> Vec<C64> {
    let mut buf = x.to_vec();
    fft_inplace(&mut buf, false);
    buf
}

/// In-place IFFT + scale 1/N. Input length must be a power of two.
pub(crate) fn ifft_raw(x: &[C64]) -> Vec<C64> {
    let mut buf = x.to_vec();
    fft_inplace(&mut buf, true);
    buf
}

/// Cooley-Tukey radix-2 DIT FFT in-place.
///
/// `inverse = false` → forward FFT (twiddle factor e^{-j 2π k/N})
/// `inverse = true`  → inverse FFT (twiddle factor e^{+j 2π k/N}) with 1/N scaling
fn fft_inplace(buf: &mut [C64], inverse: bool) {
    let n = buf.len();
    if n <= 1 {
        return;
    }

    bit_reverse_permute(buf);

    let sign: f64 = if inverse { 1.0 } else { -1.0 };

    let mut len = 2usize;
    while len <= n {
        let angle = sign * 2.0 * PI / len as f64;
        let w_base = Complex::new(angle.cos(), angle.sin());
        let mut i = 0;
        while i < n {
            let mut w = Complex::new(1.0, 0.0);
            for k in 0..len / 2 {
                let t = w * buf[i + k + len / 2];
                buf[i + k + len / 2] = buf[i + k] - t;
                buf[i + k] = buf[i + k] + t;
                w *= w_base;
            }
            i += len;
        }
        len <<= 1;
    }

    if inverse {
        let scale = 1.0 / n as f64;
        for x in buf.iter_mut() {
            *x = Complex::new(x.re * scale, x.im * scale);
        }
    }
}

fn bit_reverse_permute(buf: &mut [C64]) {
    let n = buf.len();
    let bits = n.trailing_zeros() as usize;
    for i in 0..n {
        let j = reverse_bits(i, bits);
        if i < j {
            buf.swap(i, j);
        }
    }
}

fn reverse_bits(mut x: usize, bits: usize) -> usize {
    let mut r = 0usize;
    for _ in 0..bits {
        r = (r << 1) | (x & 1);
        x >>= 1;
    }
    r
}

#[cfg(test)]
mod proptests;

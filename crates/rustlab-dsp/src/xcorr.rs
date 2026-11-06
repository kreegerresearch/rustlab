//! Cross-correlation and the raw sums behind `xcov`.
//!
//! Positive lag `m` is `sum_n x[n+m] conj(y[n])`. The result runs from
//! `-maxlag` to `+maxlag`. A full-lag call with a long input uses the
//! in-tree FFT (`fft(x) * conj(fft(y))`, then inverse). `y` stays in
//! natural order; the negative lags are the wrapped end of that product,
//! not a reversed copy of `y`.

use crate::fft::dft_any_len;
use num_complex::Complex;
use rustlab_core::{checked_elems, variable_too_large, CoreError, C64, ELEM_C64, ELEM_F64};

/// How the raw sums are scaled. `N` is the longer input length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcorrScale {
    /// Raw sums.
    None,
    /// Divide by `N`.
    Biased,
    /// Divide by `N - |m|`. A non-positive denominator is NaN.
    Unbiased,
    /// Divide by `sqrt(r_xx(0) r_yy(0))`. A zero energy is NaN.
    Coeff,
}

/// Which algorithm computes the raw sums.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcorrMethod {
    /// Direct when `maxlag <= 32` or `N < 256`. FFT otherwise.
    Auto,
    Direct,
    Fft,
}

/// Number of lags from `-maxlag` to `+maxlag`, or an error when that
/// length cannot be stored. `2 * maxlag + 1` is the expression that
/// overflowed `Vec::with_capacity` (`capacity overflow`).
pub fn lag_count(name: &str, maxlag: usize, elem: usize) -> Result<usize, CoreError> {
    let n = maxlag
        .checked_mul(2)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| CoreError::InvalidParameter(variable_too_large(name, &[maxlag])))?;
    checked_elems(&[n], elem)
        .map_err(|_| CoreError::InvalidParameter(variable_too_large(name, &[n])))?;
    Ok(n)
}

/// Cross-correlation of two complex sequences.
pub fn xcorr_c64(
    x: &[C64],
    y: &[C64],
    maxlag: usize,
    scale: XcorrScale,
    method: XcorrMethod,
) -> Result<Vec<C64>, CoreError> {
    let nout = lag_count("xcorr", maxlag, ELEM_C64)?;
    let mut raw = raw_c64(x, y, maxlag, nout, method)?;
    scale_c64(&mut raw, x, y, maxlag, scale);
    Ok(raw)
}

/// Cross-correlation of two real sequences. `conj` is a no-op, and the
/// result is real. The FFT path uses a complex product internally and
/// keeps the real part.
pub fn xcorr_f64(
    x: &[f64],
    y: &[f64],
    maxlag: usize,
    scale: XcorrScale,
    method: XcorrMethod,
) -> Result<Vec<f64>, CoreError> {
    let nout = lag_count("xcorr", maxlag, ELEM_F64)?;
    let mut raw = raw_f64(x, y, maxlag, nout, method)?;
    scale_f64(&mut raw, x, y, maxlag, scale);
    Ok(raw)
}

fn use_fft(n: usize, maxlag: usize, method: XcorrMethod) -> bool {
    match method {
        XcorrMethod::Direct => false,
        XcorrMethod::Fft => true,
        XcorrMethod::Auto => maxlag > 32 && n >= 256,
    }
}

fn raw_c64(
    x: &[C64],
    y: &[C64],
    maxlag: usize,
    nout: usize,
    method: XcorrMethod,
) -> Result<Vec<C64>, CoreError> {
    let n = x.len().max(y.len());
    if use_fft(n, maxlag, method) {
        fft_c64(x, y, maxlag, nout)
    } else {
        Ok(direct_c64(x, y, maxlag, nout))
    }
}

fn raw_f64(
    x: &[f64],
    y: &[f64],
    maxlag: usize,
    nout: usize,
    method: XcorrMethod,
) -> Result<Vec<f64>, CoreError> {
    let n = x.len().max(y.len());
    if use_fft(n, maxlag, method) {
        let xc: Vec<C64> = x.iter().map(|&v| Complex::new(v, 0.0)).collect();
        let yc: Vec<C64> = y.iter().map(|&v| Complex::new(v, 0.0)).collect();
        Ok(fft_c64(&xc, &yc, maxlag, nout)?
            .into_iter()
            .map(|c| c.re)
            .collect())
    } else {
        Ok(direct_f64(x, y, maxlag, nout))
    }
}

fn direct_c64(x: &[C64], y: &[C64], maxlag: usize, nout: usize) -> Vec<C64> {
    let nx = x.len() as isize;
    let ny = y.len() as isize;
    let mut out = Vec::with_capacity(nout);
    for k in 0..nout {
        let m = k as isize - maxlag as isize;
        let n0 = 0.max(-m);
        let n1 = ny.min(nx - m);
        let mut s = Complex::new(0.0, 0.0);
        if n1 > n0 {
            for n in n0..n1 {
                s += x[(n + m) as usize] * y[n as usize].conj();
            }
        }
        out.push(s);
    }
    out
}

fn direct_f64(x: &[f64], y: &[f64], maxlag: usize, nout: usize) -> Vec<f64> {
    let nx = x.len() as isize;
    let ny = y.len() as isize;
    let mut out = Vec::with_capacity(nout);
    for k in 0..nout {
        let m = k as isize - maxlag as isize;
        let n0 = 0.max(-m);
        let n1 = ny.min(nx - m);
        let mut s = 0.0;
        if n1 > n0 {
            for n in n0..n1 {
                s += x[(n + m) as usize] * y[n as usize];
            }
        }
        out.push(s);
    }
    out
}

/// Linear correlation via `ifft(fft(x) * conj(fft(y)))`.
///
/// The FFT length is at least `2*maxlag+1` and at least `nx+ny-1`, rounded
/// up to a power of two, so a requested lag does not wrap into another.
fn fft_c64(x: &[C64], y: &[C64], maxlag: usize, nout: usize) -> Result<Vec<C64>, CoreError> {
    let linear = x.len().saturating_add(y.len()).saturating_sub(1).max(1);
    let wanted = nout.max(linear);
    let Some(len) = wanted.checked_next_power_of_two() else {
        return Ok(direct_c64(x, y, maxlag, nout));
    };
    if checked_elems(&[len], ELEM_C64).is_err() {
        return Ok(direct_c64(x, y, maxlag, nout));
    }
    let mut xbuf = vec![Complex::new(0.0, 0.0); len];
    let mut ybuf = vec![Complex::new(0.0, 0.0); len];
    xbuf[..x.len()].copy_from_slice(x);
    ybuf[..y.len()].copy_from_slice(y);
    // The length is a power of two bounded by the inputs already in hand.
    // A refusal here is a size error, not a wrong correlation.
    dft_any_len(&mut xbuf, false)?;
    dft_any_len(&mut ybuf, false)?;
    for i in 0..len {
        xbuf[i] *= ybuf[i].conj();
    }
    dft_any_len(&mut xbuf, true)?;
    let mut out = Vec::with_capacity(nout);
    for k in 0..nout {
        let m = k as isize - maxlag as isize;
        let idx = if m >= 0 {
            m as usize
        } else {
            len - (-m as usize)
        };
        out.push(xbuf[idx]);
    }
    Ok(out)
}

fn scale_c64(raw: &mut [C64], x: &[C64], y: &[C64], maxlag: usize, scale: XcorrScale) {
    let n = x.len().max(y.len());
    match scale {
        XcorrScale::None => {}
        XcorrScale::Biased => {
            if n == 0 {
                raw.fill(Complex::new(f64::NAN, f64::NAN));
                return;
            }
            let d = n as f64;
            for v in raw.iter_mut() {
                *v /= d;
            }
        }
        XcorrScale::Unbiased => {
            for (k, v) in raw.iter_mut().enumerate() {
                let m = k as isize - maxlag as isize;
                let d = n as isize - m.abs();
                if d <= 0 {
                    *v = Complex::new(f64::NAN, f64::NAN);
                } else {
                    *v /= d as f64;
                }
            }
        }
        XcorrScale::Coeff => {
            let ex: f64 = x.iter().map(|c| c.norm_sqr()).sum();
            let ey: f64 = y.iter().map(|c| c.norm_sqr()).sum();
            let d = (ex * ey).sqrt();
            if d == 0.0 {
                raw.fill(Complex::new(f64::NAN, f64::NAN));
            } else {
                for v in raw.iter_mut() {
                    *v /= d;
                }
            }
        }
    }
}

fn scale_f64(raw: &mut [f64], x: &[f64], y: &[f64], maxlag: usize, scale: XcorrScale) {
    let n = x.len().max(y.len());
    match scale {
        XcorrScale::None => {}
        XcorrScale::Biased => {
            if n == 0 {
                raw.fill(f64::NAN);
                return;
            }
            let d = n as f64;
            for v in raw.iter_mut() {
                *v /= d;
            }
        }
        XcorrScale::Unbiased => {
            for (k, v) in raw.iter_mut().enumerate() {
                let m = k as isize - maxlag as isize;
                let d = n as isize - m.abs();
                if d <= 0 {
                    *v = f64::NAN;
                } else {
                    *v /= d as f64;
                }
            }
        }
        XcorrScale::Coeff => {
            let ex: f64 = x.iter().map(|v| v * v).sum();
            let ey: f64 = y.iter().map(|v| v * v).sum();
            let d = (ex * ey).sqrt();
            if d == 0.0 {
                raw.fill(f64::NAN);
            } else {
                for v in raw.iter_mut() {
                    *v /= d;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(re: f64, im: f64) -> C64 {
        Complex::new(re, im)
    }

    #[test]
    fn four_point_hand_sum_with_conj() {
        let x = [c(1.0, 0.0), c(2.0, 0.0), c(3.0, 0.0), c(4.0, 0.0)];
        let y = [c(1.0, 1.0), c(2.0, 0.0)];
        // lags -3..3. Positive lag m is sum_n x[n+m] conj(y[n]).
        let want = [
            c(0.0, 0.0),
            c(0.0, 0.0),
            c(2.0, 0.0),
            c(5.0, -1.0),
            c(8.0, -2.0),
            c(11.0, -3.0),
            c(4.0, -4.0),
        ];
        for method in [XcorrMethod::Direct, XcorrMethod::Fft] {
            let got = xcorr_c64(&x, &y, 3, XcorrScale::None, method).unwrap();
            assert_eq!(got.len(), 7, "{method:?}");
            for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
                assert!(
                    (g - w).norm() < 1e-9,
                    "{method:?} lag index {i}: {g} vs {w}"
                );
            }
        }
    }

    #[test]
    fn real_autocorrelation_is_even_and_coeff_is_one_at_zero() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let got = xcorr_f64(&x, &x, 3, XcorrScale::None, XcorrMethod::Direct).unwrap();
        assert_eq!(got, vec![4.0, 11.0, 20.0, 30.0, 20.0, 11.0, 4.0]);
        let coeff = xcorr_f64(&x, &x, 3, XcorrScale::Coeff, XcorrMethod::Direct).unwrap();
        assert!((coeff[3] - 1.0).abs() < 1e-12);
        let biased = xcorr_f64(&x, &x, 3, XcorrScale::Biased, XcorrMethod::Direct).unwrap();
        assert!((biased[3] - 30.0 / 4.0).abs() < 1e-12);
        let unbiased = xcorr_f64(&x, &x, 3, XcorrScale::Unbiased, XcorrMethod::Fft).unwrap();
        assert!((unbiased[0] - 4.0).abs() < 1e-9);
        assert!((unbiased[3] - 30.0 / 4.0).abs() < 1e-9);
    }

    #[test]
    fn unequal_lengths_and_a_lag_past_the_end() {
        // Hand sum. x = [1, 2, 3], y = [4, 5], lags −2..2.
        let x = [1.0, 2.0, 3.0];
        let y = [4.0, 5.0];
        let want = [0.0, 5.0, 14.0, 23.0, 12.0];
        let direct = xcorr_f64(&x, &y, 2, XcorrScale::None, XcorrMethod::Direct).unwrap();
        assert_eq!(direct, want);
        let fft = xcorr_f64(&x, &y, 2, XcorrScale::None, XcorrMethod::Fft).unwrap();
        for (i, (g, w)) in fft.iter().zip(want).enumerate() {
            assert!((g - w).abs() < 1e-9, "fft lag index {i}: {g} vs {w}");
        }
        // maxlag 4 pads past N−1 = 2. |m| = N makes the unbiased divisor 0.
        let wide = xcorr_f64(&x, &x, 4, XcorrScale::None, XcorrMethod::Fft).unwrap();
        let direct = xcorr_f64(&x, &x, 4, XcorrScale::None, XcorrMethod::Direct).unwrap();
        assert_eq!(direct, vec![0.0, 0.0, 3.0, 8.0, 14.0, 8.0, 3.0, 0.0, 0.0]);
        for (i, (a, b)) in wide.iter().zip(direct.iter()).enumerate() {
            assert!((a - b).abs() < 1e-9, "lag index {i}: {a} vs {b}");
        }
        let u = xcorr_f64(&x, &x, 3, XcorrScale::Unbiased, XcorrMethod::Fft).unwrap();
        assert!(u[0].is_nan() && u[6].is_nan());
        assert!((u[1] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn fft_matches_direct_at_length_256_including_unbiased_ends() {
        let x: Vec<f64> = (0..256).map(|k| ((k * 17) % 11) as f64 - 5.0).collect();
        let y: Vec<f64> = (0..256).map(|k| ((k * 3) % 7) as f64 - 2.0).collect();
        let direct = xcorr_f64(&x, &y, 255, XcorrScale::Unbiased, XcorrMethod::Direct).unwrap();
        let fft = xcorr_f64(&x, &y, 255, XcorrScale::Unbiased, XcorrMethod::Fft).unwrap();
        assert_eq!(direct.len(), 511);
        let scale = direct
            .iter()
            .map(|v| v.abs())
            .fold(0.0_f64, f64::max)
            .max(1.0);
        for (i, (a, b)) in direct.iter().zip(fft.iter()).enumerate() {
            assert!((a - b).abs() / scale < 1e-9, "lag index {i}: {a} vs {b}");
        }
        // Auto on this length is the FFT path. It must match too.
        let auto = xcorr_f64(&x, &y, 255, XcorrScale::Unbiased, XcorrMethod::Auto).unwrap();
        assert!((auto[0] - direct[0]).abs() / scale < 1e-9);
        assert!((auto[510] - direct[510]).abs() / scale < 1e-9);
    }

    #[test]
    fn huge_maxlag_is_an_error_not_a_panic() {
        // `2 * maxlag + 1` used to overflow into `Vec::with_capacity`,
        // which panics with "capacity overflow" (release) or an
        // arithmetic overflow (debug). Both shapes must be a Result.
        let too_big = usize::MAX / 2 + 8;
        let err = xcorr_f64(
            &[1.0, 2.0, 3.0],
            &[1.0],
            too_big,
            XcorrScale::None,
            XcorrMethod::Direct,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("xcorr"), "{msg}");
        assert!(xcorr_c64(
            &[Complex::new(1.0, 0.0)],
            &[Complex::new(1.0, -1.0)],
            too_big,
            XcorrScale::Coeff,
            XcorrMethod::Fft,
        )
        .is_err());
        // A lag count that fits in `usize` but not in the element cap.
        let wide = (1usize << 47) + 3;
        assert!(xcorr_f64(&[1.0], &[1.0], wide, XcorrScale::None, XcorrMethod::Auto).is_err());
    }
}

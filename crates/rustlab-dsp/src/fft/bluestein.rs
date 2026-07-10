//! Arbitrary-length DFT via Bluestein's chirp-z transform.
//!
//! Uses the identity `jk = (j² + k² − (k−j)²) / 2` to rewrite the n-point
//! DFT as a linear convolution of the chirp-premultiplied input with the
//! conjugate chirp, evaluated as a circular convolution of length
//! `m = next_power_of_two(2n − 1)` through the existing radix-2 kernel:
//!
//! ```text
//! X[k] = w[k] · Σ_j (x[j]·w[j]) · conj(w[k−j]),   w[t] = e^{∓iπt²/n}
//! ```
//!
//! Cost: two forward + one inverse radix-2 FFT of `m ∈ [2n, 4n)` plus
//! O(n) chirp work — a constant factor (~6–12×) over a same-length
//! power-of-two FFT. The chirp table `fb` could be cached per length as
//! a future optimisation; v1 recomputes it per call.

use num_complex::Complex;
use rustlab_core::C64;
use std::f64::consts::PI;

/// n-point DFT (or inverse DFT with 1/n scaling) of `x` for any length.
///
/// The chirp phase uses `k² mod 2n` computed in `u128`: `e^{∓iπk²/n}` is
/// 2n-periodic in `k²`, and the reduced value is < 2n ≤ 2⁵³, so the phase
/// argument stays exact in f64 for any realistic n (naive `(k*k) as f64`
/// loses precision once k² exceeds 2⁵³).
pub(super) fn bluestein(x: &[C64], inverse: bool) -> Vec<C64> {
    let n = x.len();
    if n <= 1 {
        return x.to_vec();
    }
    let m = (2 * n - 1).next_power_of_two();
    let sign: f64 = if inverse { 1.0 } else { -1.0 };

    // w[t] = e^{sign·iπt²/n} for t in 0..n.
    let chirp: Vec<C64> = (0..n)
        .map(|k| {
            let q = ((k as u128 * k as u128) % (2 * n as u128)) as f64;
            let angle = sign * PI * q / n as f64;
            Complex::new(angle.cos(), angle.sin())
        })
        .collect();

    // a[j] = x[j]·w[j], zero-padded to m.
    let mut a = vec![Complex::new(0.0, 0.0); m];
    for j in 0..n {
        a[j] = x[j] * chirp[j];
    }

    // b[t] = conj(w[t]) for t in −(n−1)..=(n−1), wrapped circularly
    // (b is even in t, so index −j lands at m − j).
    let mut b = vec![Complex::new(0.0, 0.0); m];
    b[0] = Complex::new(1.0, 0.0);
    for j in 1..n {
        let c = chirp[j].conj();
        b[j] = c;
        b[m - j] = c;
    }

    // Circular convolution via the radix-2 kernel (m is a power of two).
    let fa = super::fft_raw(&a);
    let fb = super::fft_raw(&b);
    let prod: Vec<C64> = fa.iter().zip(fb.iter()).map(|(p, q)| p * q).collect();
    let conv = super::ifft_raw(&prod);

    // X[k] = w[k]·conv[k]; inverse additionally scales by 1/n.
    let mut out: Vec<C64> = (0..n).map(|k| conv[k] * chirp[k]).collect();
    if inverse {
        let scale = 1.0 / n as f64;
        for v in out.iter_mut() {
            *v = Complex::new(v.re * scale, v.im * scale);
        }
    }
    out
}

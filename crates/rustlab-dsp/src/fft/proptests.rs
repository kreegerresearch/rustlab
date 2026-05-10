//! Property-based tests for the FFT.
//!
//! Generates random complex vectors of arbitrary length and checks
//! invariants that should hold for *every* input:
//!
//! - **Round-trip:** `ifft(fft(x))[0..len(x)] ≈ x` for any `x`. The FFT
//!   zero-pads to the next power of two, so the inverse must be sliced
//!   back to the original length.
//! - **Linearity:** `fft(αx + βy) = α·fft(x) + β·fft(y)`.
//! - **DC coefficient:** `fft(x)[0]` equals the sum of `x` (after
//!   zero-padding). For a constant signal of length n, the DC bin is
//!   `n·c` and all other bins are zero.
//! - **Parseval's theorem:** `Σ|x_i|² = (1/N)·Σ|X_k|²` for the FFT
//!   of length-N (with our normalization, ifft includes the 1/N factor;
//!   so Σ|x_i|² = (1/N)·Σ|X_k|²).

use crate::fft::{fft, ifft};
use ndarray::Array1;
use num_complex::Complex;
use proptest::prelude::*;
use rustlab_core::{CVector, C64};

const ROUND_TRIP_TOL: f64 = 1e-10;

fn arb_complex_vec(min_len: usize, max_len: usize) -> impl Strategy<Value = CVector> {
    (min_len..=max_len).prop_flat_map(|n| {
        proptest::collection::vec((-10.0_f64..10.0_f64, -10.0_f64..10.0_f64), n).prop_map(
            |entries| {
                let v: Vec<C64> = entries
                    .into_iter()
                    .map(|(re, im)| Complex::new(re, im))
                    .collect();
                Array1::from_vec(v)
            },
        )
    })
}

fn next_pow2(n: usize) -> usize {
    if n <= 1 {
        return 1;
    }
    let mut p = 1usize;
    while p < n {
        p <<= 1;
    }
    p
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    })]

    /// `ifft(fft(x))` reconstructs the zero-padded `x` to within tol.
    /// Slice back to original length — fft zero-pads internally.
    #[test]
    fn fft_round_trip(x in arb_complex_vec(1, 64)) {
        let len = x.len();
        let xf = fft(&x).expect("fft");
        let xr = ifft(&xf).expect("ifft");
        // xr has padded length; first `len` entries should match x to tol.
        for i in 0..len {
            let diff = (xr[i] - x[i]).norm();
            prop_assert!(
                diff < ROUND_TRIP_TOL,
                "round-trip failed at idx {i}: x={} got={} (diff {diff})",
                x[i],
                xr[i]
            );
        }
        // Padded tail should be near-zero.
        for i in len..xr.len() {
            prop_assert!(
                xr[i].norm() < ROUND_TRIP_TOL,
                "padding leaked into idx {i}: {}",
                xr[i]
            );
        }
    }

    /// FFT is linear: `fft(α·x + β·y) = α·fft(x) + β·fft(y)`.
    #[test]
    fn fft_linearity(
        x in arb_complex_vec(2, 32),
        alpha_re in -3.0_f64..3.0_f64,
        alpha_im in -3.0_f64..3.0_f64,
        beta_re in -3.0_f64..3.0_f64,
        beta_im in -3.0_f64..3.0_f64,
    ) {
        // Build y as a deterministic shuffle of x so dimensions match.
        let n = x.len();
        let mut y = x.clone();
        // Cycle by one to make y != x while keeping the same length.
        if n >= 2 {
            let last = y[n - 1];
            for i in (1..n).rev() {
                y[i] = y[i - 1];
            }
            y[0] = last;
        }
        let alpha = Complex::new(alpha_re, alpha_im);
        let beta = Complex::new(beta_re, beta_im);
        let mut combined = Array1::<C64>::zeros(n);
        for i in 0..n {
            combined[i] = alpha * x[i] + beta * y[i];
        }
        let lhs = fft(&combined).unwrap();
        let xf = fft(&x).unwrap();
        let yf = fft(&y).unwrap();
        let m = lhs.len();
        for i in 0..m {
            let rhs_i = alpha * xf[i] + beta * yf[i];
            let diff = (lhs[i] - rhs_i).norm();
            prop_assert!(
                diff < 1e-9,
                "linearity violation at bin {i}: lhs={} rhs={} (diff {diff})",
                lhs[i],
                rhs_i
            );
        }
    }

    /// DC bin equals the sum of the (zero-padded) input.
    #[test]
    fn fft_dc_bin_is_sum(x in arb_complex_vec(1, 32)) {
        let xf = fft(&x).unwrap();
        let sum: C64 = x.iter().copied().sum();
        let diff = (xf[0] - sum).norm();
        prop_assert!(
            diff < 1e-10,
            "DC bin {} != sum {} (diff {diff})",
            xf[0],
            sum
        );
    }

    /// Constant input → DC bin only. fft of `[c, c, ..., c]` (length n,
    /// internally zero-padded to next power of two) has bin 0 equal
    /// to n·c, all other bins zero.
    #[test]
    fn fft_constant_signal_one_nonzero_bin(
        c_re in -5.0_f64..5.0_f64,
        c_im in -5.0_f64..5.0_f64,
        n in 1usize..=16,
    ) {
        let c = Complex::new(c_re, c_im);
        let x = Array1::from_elem(n, c);
        let xf = fft(&x).unwrap();
        let dc_expected = c * (n as f64);
        let dc_diff = (xf[0] - dc_expected).norm();
        prop_assert!(
            dc_diff < 1e-10,
            "DC bin: got {} expected {} (diff {dc_diff})",
            xf[0],
            dc_expected
        );
        // Non-DC bins should be exactly zero only if the full padded
        // signal is constant; with zero-padding they aren't. Restrict
        // the assertion to the case n is already a power of two so the
        // padded signal IS constant.
        if n == next_pow2(n) {
            for k in 1..xf.len() {
                prop_assert!(
                    xf[k].norm() < 1e-10,
                    "bin {k} for constant power-of-two signal: {} != 0",
                    xf[k]
                );
            }
        }
    }

    /// Parseval: Σ|x_i|² · N = Σ|X_k|² where N is the padded length.
    /// (Our FFT is unscaled forward; ifft applies 1/N. So the standard
    /// Parseval is Σ|x|² = (1/N)·Σ|X|² ⇔ N·Σ|x|² = Σ|X|² when x is
    /// already the padded vector.)
    #[test]
    fn fft_parseval(x in arb_complex_vec(2, 32)) {
        let xf = fft(&x).unwrap();
        let n_padded = xf.len();
        // Construct the padded x to compute its norm correctly.
        let mut x_padded: Vec<C64> = x.iter().copied().collect();
        x_padded.resize(n_padded, Complex::new(0.0, 0.0));
        let lhs: f64 = x_padded
            .iter()
            .map(|c| c.norm_sqr())
            .sum::<f64>()
            * (n_padded as f64);
        let rhs: f64 = xf.iter().map(|c| c.norm_sqr()).sum::<f64>();
        let diff = (lhs - rhs).abs();
        let scale = lhs.max(rhs).max(1e-12);
        prop_assert!(
            diff / scale < 1e-9,
            "Parseval violation: N·Σ|x|² = {lhs} vs Σ|X|² = {rhs} (rel diff {})",
            diff / scale
        );
    }
}

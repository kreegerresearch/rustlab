//! Property-based tests for the FFT.
//!
//! Generates random complex vectors of arbitrary length and checks
//! invariants that should hold for *every* input. `fft` is
//! length-preserving (powers of two on the radix-2 path, everything else
//! through Bluestein), so the invariants hold with no padding caveats:
//!
//! - **Round-trip:** `ifft(fft(x)) ≈ x`, same length, for any `x`.
//! - **Linearity:** `fft(αx + βy) = α·fft(x) + β·fft(y)`.
//! - **DC coefficient:** `fft(x)[0]` equals the sum of `x`; a constant
//!   signal of *any* length has energy only in the DC bin.
//! - **Parseval's theorem:** `N·Σ|x_i|² = Σ|X_k|²` (unscaled forward
//!   transform; `ifft` carries the 1/N).
//! - **Explicit size:** `fft_n(x, n)` equals `fft` of the hand-padded or
//!   hand-truncated input.
//! - **Oracle:** `fft` matches the naive O(n²) DFT for every length.

use crate::fft::{fft, fft_n, ifft};
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

/// Textbook O(n²) DFT used as the correctness oracle.
fn dft_naive(x: &CVector) -> Vec<C64> {
    let n = x.len();
    let mut out = vec![Complex::new(0.0, 0.0); n];
    for (k, o) in out.iter_mut().enumerate() {
        for (j, &v) in x.iter().enumerate() {
            let angle = -2.0 * std::f64::consts::PI * (j * k) as f64 / n as f64;
            *o += v * Complex::new(angle.cos(), angle.sin());
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    })]

    /// `ifft(fft(x)) == x` — exact length, elementwise, any length.
    #[test]
    fn fft_round_trip(x in arb_complex_vec(1, 512)) {
        let len = x.len();
        let xf = fft(&x).expect("fft");
        prop_assert_eq!(xf.len(), len, "fft must be length-preserving");
        let xr = ifft(&xf).expect("ifft");
        prop_assert_eq!(xr.len(), len, "ifft must be length-preserving");
        for i in 0..len {
            let diff = (xr[i] - x[i]).norm();
            prop_assert!(
                diff < ROUND_TRIP_TOL,
                "round-trip failed at idx {}: x={} got={} (diff {})",
                i, x[i], xr[i], diff
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
        for i in 0..n {
            let rhs_i = alpha * xf[i] + beta * yf[i];
            let diff = (lhs[i] - rhs_i).norm();
            prop_assert!(
                diff < 1e-9,
                "linearity violation at bin {}: lhs={} rhs={} (diff {})",
                i, lhs[i], rhs_i, diff
            );
        }
    }

    /// DC bin equals the sum of the input.
    #[test]
    fn fft_dc_bin_is_sum(x in arb_complex_vec(1, 32)) {
        let xf = fft(&x).unwrap();
        let sum: C64 = x.iter().copied().sum();
        let diff = (xf[0] - sum).norm();
        prop_assert!(
            diff < 1e-10,
            "DC bin {} != sum {} (diff {})",
            xf[0], sum, diff
        );
    }

    /// Constant input → DC bin only, for EVERY length (length-preserving
    /// fft means the transformed signal really is constant — no padded
    /// tail to smear energy across bins).
    #[test]
    fn fft_constant_signal_one_nonzero_bin(
        c_re in -5.0_f64..5.0_f64,
        c_im in -5.0_f64..5.0_f64,
        n in 1usize..=48,
    ) {
        let c = Complex::new(c_re, c_im);
        let x = Array1::from_elem(n, c);
        let xf = fft(&x).unwrap();
        let dc_expected = c * (n as f64);
        let dc_diff = (xf[0] - dc_expected).norm();
        prop_assert!(
            dc_diff < 1e-9,
            "DC bin: got {} expected {} (diff {})",
            xf[0], dc_expected, dc_diff
        );
        // Scale the near-zero tolerance with the signal energy.
        let tol = 1e-10 * (1.0 + dc_expected.norm());
        for k in 1..n {
            prop_assert!(
                xf[k].norm() < tol,
                "bin {} for constant length-{} signal: {} != 0",
                k, n, xf[k]
            );
        }
    }

    /// Parseval: `N·Σ|x_i|² = Σ|X_k|²` — exact-length, no padded copy.
    #[test]
    fn fft_parseval(x in arb_complex_vec(2, 64)) {
        let n = x.len();
        let xf = fft(&x).unwrap();
        let lhs: f64 = x.iter().map(|c| c.norm_sqr()).sum::<f64>() * (n as f64);
        let rhs: f64 = xf.iter().map(|c| c.norm_sqr()).sum::<f64>();
        let diff = (lhs - rhs).abs();
        let scale = lhs.max(rhs).max(1e-12);
        prop_assert!(
            diff / scale < 1e-9,
            "Parseval violation: N·Σ|x|² = {} vs Σ|X|² = {} (rel diff {})",
            lhs, rhs, diff / scale
        );
    }

    /// `fft_n(x, n)` is exactly `fft` of the hand-padded / hand-truncated
    /// input, for arbitrary n.
    #[test]
    fn fft_n_matches_manual_pad_or_truncate(
        x in arb_complex_vec(1, 64),
        n in 1usize..=128,
    ) {
        let sized = fft_n(&x, n).unwrap();
        prop_assert_eq!(sized.len(), n);
        let mut manual: Vec<C64> = x.iter().copied().take(n).collect();
        manual.resize(n, Complex::new(0.0, 0.0));
        let expect = fft(&Array1::from_vec(manual)).unwrap();
        for k in 0..n {
            let diff = (sized[k] - expect[k]).norm();
            prop_assert!(
                diff < 1e-12,
                "fft_n mismatch at bin {}: {} vs {}",
                k, sized[k], expect[k]
            );
        }
    }

    /// `fft` matches the naive O(n²) DFT for every length 1..=64.
    #[test]
    fn fft_matches_naive_dft(x in arb_complex_vec(1, 64)) {
        let expect = dft_naive(&x);
        let got = fft(&x).unwrap();
        // Naive-DFT rounding grows with n and signal magnitude; scale
        // the tolerance accordingly.
        let energy: f64 = x.iter().map(|c| c.norm()).sum();
        let tol = 1e-11 * (1.0 + energy) * (x.len() as f64);
        for k in 0..x.len() {
            let diff = (got[k] - expect[k]).norm();
            prop_assert!(
                diff < tol,
                "naive-DFT mismatch at bin {} (n={}): {} vs {} (diff {}, tol {})",
                k, x.len(), got[k], expect[k], diff, tol
            );
        }
    }
}

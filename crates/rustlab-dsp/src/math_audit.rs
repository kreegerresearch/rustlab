//! Reference-value checks for DSP primitives.
//!
//! Expected numbers are closed forms or values published with the algorithm
//! (window samples, a length-3 DFT, a one-pole step). Nothing here calls
//! Octave or NumPy.

use crate::convolution::{convolve, overlap_add};
use crate::fft::{fft, fft_n, ifft};
use crate::fir::design::{fir_bandpass, fir_highpass, fir_lowpass};
use crate::fir::kaiser::freqz;
use crate::iir::butterworth::{butterworth_highpass, butterworth_lowpass};
use crate::window::WindowFunction;
use ndarray::Array1;
use rustlab_core::{Filter, C64};
use std::f64::consts::PI;

fn c(re: f64, im: f64) -> C64 {
    C64::new(re, im)
}

fn real(xs: &[f64]) -> Array1<C64> {
    Array1::from_iter(xs.iter().copied().map(|x| c(x, 0.0)))
}

fn assert_close(got: C64, want: C64, tol: f64) {
    let d = (got - want).norm();
    assert!(d <= tol, "got {got}, want {want}, |diff| {d:e}");
}

#[test]
fn fft_length_four_matches_dft() {
    // DFT of [1, 2, 3, 4].
    let y = fft(&real(&[1.0, 2.0, 3.0, 4.0])).unwrap();
    let want = [c(10.0, 0.0), c(-2.0, 2.0), c(-2.0, 0.0), c(-2.0, -2.0)];
    for (g, w) in y.iter().zip(want) {
        assert_close(*g, w, 1e-12);
    }
}

#[test]
fn fft_length_three_matches_closed_form() {
    // X1 = 1 + 2 ω + 3 ω², ω = exp(-2πi/3) = -1/2 - i√3/2.
    let y = fft(&real(&[1.0, 2.0, 3.0])).unwrap();
    let s = 3.0_f64.sqrt();
    assert_close(y[0], c(6.0, 0.0), 1e-12);
    assert_close(y[1], c(-1.5, 0.5 * s), 1e-12);
    assert_close(y[2], c(-1.5, -0.5 * s), 1e-12);
}

#[test]
fn fft_of_a_constant_is_a_spike_at_dc() {
    for n in [1_usize, 2, 5, 6, 7, 8, 9, 16] {
        let y = fft(&real(&vec![1.0; n])).unwrap();
        assert_close(y[0], c(n as f64, 0.0), 1e-10);
        for k in 1..n {
            assert!(y[k].norm() < 1e-9, "n={n} bin {k} = {} should be 0", y[k]);
        }
    }
}

#[test]
fn ifft_fft_round_trip_including_non_powers_of_two() {
    let x = real(&[0.3, -1.2, 4.0, 0.0, 2.5, -0.7, 8.0, 1.1, -3.0]);
    for n in [1_usize, 2, 3, 5, 6, 7, 8, 9] {
        let slice = real(&x.iter().take(n).map(|z| z.re).collect::<Vec<_>>());
        let y = ifft(&fft(&slice).unwrap()).unwrap();
        for (a, b) in slice.iter().zip(y.iter()) {
            assert_close(*a, *b, 1e-10);
        }
    }
}

#[test]
fn fft_n_pads_a_prime_length() {
    // fft([1, 2, 3], 4) is the length-4 DFT of [1, 2, 3, 0].
    let y = fft_n(&real(&[1.0, 2.0, 3.0]), 4).unwrap();
    assert_eq!(y.len(), 4);
    assert_close(y[0], c(6.0, 0.0), 1e-12);
    assert_close(y[1], c(-2.0, -2.0), 1e-12);
    assert_close(y[2], c(2.0, 0.0), 1e-12);
    assert_close(y[3], c(-2.0, 2.0), 1e-12);
}

#[test]
fn parseval_holds_for_a_mixed_length() {
    let x = real(&[1.0, -2.0, 0.5, 4.0, -1.0, 3.0]);
    let y = fft(&x).unwrap();
    let time: f64 = x.iter().map(|z| z.norm_sqr()).sum();
    let freq: f64 = y.iter().map(|z| z.norm_sqr()).sum::<f64>() / y.len() as f64;
    assert!((time - freq).abs() < 1e-10, "{time} vs {freq}");
}

#[test]
fn convolve_matches_the_definition() {
    // [1, 2, 3] * [4, 5] = [4, 13, 22, 15]
    let y = convolve(&real(&[1.0, 2.0, 3.0]), &real(&[4.0, 5.0])).unwrap();
    let want = [4.0, 13.0, 22.0, 15.0];
    assert_eq!(y.len(), 4);
    for (g, w) in y.iter().zip(want) {
        assert_close(*g, c(w, 0.0), 1e-12);
    }
}

#[test]
fn convolve_complex_and_empty() {
    let y = convolve(
        &real(&[1.0, 0.0]),
        &Array1::from_vec(vec![c(0.0, 1.0), c(1.0, 0.0)]),
    )
    .unwrap();
    assert_close(y[0], c(0.0, 1.0), 1e-12);
    assert_close(y[1], c(1.0, 0.0), 1e-12);
    assert!(convolve(&real(&[]), &real(&[1.0])).unwrap().is_empty());
}

#[test]
fn overlap_add_matches_direct_convolution() {
    let x = real(&[1.0, -1.0, 2.0, 0.5, 3.0, -2.0, 0.25, 4.0, 1.5]);
    let h = real(&[0.5, 1.0, -0.25, 0.1]);
    let direct = convolve(&x, &h).unwrap();
    for block in [1_usize, 2, 3, 4, 8] {
        let y = overlap_add(&x, &h, block).unwrap();
        assert_eq!(y.len(), direct.len());
        for (a, b) in y.iter().zip(direct.iter()) {
            assert_close(*a, *b, 1e-9);
        }
    }
}

#[test]
fn symmetric_windows_match_published_samples() {
    // Symmetric (divide by N-1) samples, endpoints included.
    let hann = WindowFunction::Hann.generate(4);
    for (g, w) in hann.iter().zip([0.0, 0.75, 0.75, 0.0]) {
        assert!((g - w).abs() < 1e-12, "hann {g} vs {w}");
    }
    let ham = WindowFunction::Hamming.generate(4);
    // 0.54 - 0.46 cos(2π i / 3)
    for (g, w) in ham.iter().zip([0.08, 0.77, 0.77, 0.08]) {
        assert!((g - w).abs() < 1e-12, "hamming {g} vs {w}");
    }
    let blk = WindowFunction::Blackman.generate(5);
    assert!((blk[0] - 0.0).abs() < 1e-12);
    assert!((blk[4] - 0.0).abs() < 1e-12);
    assert!((blk[2] - 1.0).abs() < 1e-12, "blackman center {}", blk[2]);
    let rect = WindowFunction::Rectangular.generate(3);
    assert!(rect.iter().all(|&x| x == 1.0));
    assert_eq!(WindowFunction::Hann.generate(0).len(), 0);
    assert!((WindowFunction::Kaiser { beta: 0.0 }.generate(5)[2] - 1.0).abs() < 1e-12);
}

#[test]
fn kaiser_beta_matches_the_bessel_ratio() {
    // I0 series evaluated independently of `bessel_i0`, β = 5, length 5.
    fn i0(x: f64) -> f64 {
        let mut sum = 1.0;
        let mut term = 1.0;
        let half = x / 2.0;
        for k in 1..=200 {
            term *= half / k as f64;
            let add = term * term;
            sum += add;
            if add < 1e-18 * sum {
                break;
            }
        }
        sum
    }
    let beta = 5.0;
    let w = WindowFunction::Kaiser { beta }.generate(5);
    let i0b = i0(beta);
    for i in 0..5 {
        let ratio = (i as f64 - 2.0) / 2.0;
        let want = i0(beta * (1.0 - ratio * ratio).max(0.0).sqrt()) / i0b;
        assert!(
            (w[i] - want).abs() < 1e-12,
            "kaiser[{i}] {} vs {want}",
            w[i]
        );
    }
}

#[test]
fn fir_lowpass_has_unit_dc_and_highpass_rejects_dc() {
    let lp = fir_lowpass(31, 1000.0, 8000.0, WindowFunction::Hann).unwrap();
    let dc: f64 = lp.coefficients.iter().map(|z| z.re).sum();
    assert!((dc - 1.0).abs() < 1e-12, "lowpass DC {dc}");

    let hp = fir_highpass(31, 1000.0, 8000.0, WindowFunction::Hann).unwrap();
    let hp_dc: f64 = hp.coefficients.iter().map(|z| z.re).sum();
    assert!(hp_dc.abs() < 1e-9, "highpass DC {hp_dc}");
    // H(-1) = Σ (-1)^k h[k]. For an odd length the linear-phase delay
    // (N-1)/2 makes this ±1, not a complex value. Magnitude is the gain.
    let nyq: f64 = hp
        .coefficients
        .iter()
        .enumerate()
        .map(|(k, z)| if k % 2 == 0 { z.re } else { -z.re })
        .sum();
    // Windowed, so the stopband of the prototype is not exactly zero and
    // |H(π)| is 1 only to about 1e-4 for 31 taps.
    assert!(
        (nyq.abs() - 1.0).abs() < 1e-4,
        "highpass |H(Nyquist)| {}",
        nyq.abs()
    );
}

#[test]
fn fir_bandpass_is_quiet_at_dc_and_loud_in_band() {
    let bp = fir_bandpass(63, 1000.0, 2000.0, 8000.0, WindowFunction::Hamming).unwrap();
    let dc: f64 = bp.coefficients.iter().map(|z| z.re).sum();
    assert!(dc.abs() < 1e-6, "bandpass DC {dc}");
    let h = bp.frequency_response(32).unwrap();
    // Bin i is at f = i/32 * 4000 Hz. 1500 Hz is bin 12.
    assert!(
        h[12].norm() > 0.5,
        "midband |H| {} should pass",
        h[12].norm()
    );
    assert!(h[0].norm() < 0.05, "DC |H| {}", h[0].norm());
}

#[test]
fn butterworth_cutoff_is_minus_three_db() {
    let target = 1.0 / 2.0_f64.sqrt();
    for order in [1_usize, 2, 3, 4, 5] {
        let lp = butterworth_lowpass(order, 1000.0, 8000.0).unwrap();
        let h = lp.frequency_response(8).unwrap();
        // ω = π i / 8; i = 2 → ω = π/4 → f = fs/8 = 1000 Hz.
        let mag = h[2].norm();
        assert!(
            (mag - target).abs() < 1e-6,
            "LP order {order} |H(fc)| = {mag}, want {target}"
        );
        assert!((h[0].norm() - 1.0).abs() < 1e-8, "LP DC {}", h[0].norm());

        let hp = butterworth_highpass(order, 1000.0, 8000.0).unwrap();
        let hh = hp.frequency_response(8).unwrap();
        let hmag = hh[2].norm();
        assert!(
            (hmag - target).abs() < 1e-6,
            "HP order {order} |H(fc)| = {hmag}"
        );
        assert!(hh[0].norm() < 1e-8, "HP DC {}", hh[0].norm());
    }
}

#[test]
fn filtfilt_of_a_constant_through_a_lowpass_stays_put() {
    let f = butterworth_lowpass(2, 1000.0, 8000.0).unwrap();
    let x = vec![1.0; 64];
    let y = f.filtfilt(&x);
    assert_eq!(y.len(), 64);
    for (i, &v) in y.iter().enumerate() {
        assert!((v - 1.0).abs() < 1e-8, "sample {i} = {v}");
    }
}

#[test]
fn one_pole_filter_matches_the_recurrence() {
    // y[n] = x[n] + 0.5 y[n-1], x = δ. y = [1, 0.5, 0.25, 0.125].
    use crate::iir::butterworth::IirFilter;
    let f = IirFilter::new(vec![1.0], vec![1.0, -0.5]);
    let y = f.apply(&real(&[1.0, 0.0, 0.0, 0.0])).unwrap();
    for (g, w) in y.iter().zip([1.0, 0.5, 0.25, 0.125]) {
        assert_close(*g, c(w, 0.0), 1e-12);
    }
}

#[test]
fn freqz_grid_reaches_toward_nyquist_for_any_length() {
    // A one-sample delay has H(f) = exp(-j 2π f/fs).
    let h = real(&[0.0, 1.0]);
    let (freqs, resp) = freqz(&h, 5, 1000.0).unwrap();
    assert_eq!(freqs.len(), 5);
    // f_k = k * 1000 / (2*4) = k * 125. The last bin is Nyquist.
    for k in 0..5 {
        assert!((freqs[k] - k as f64 * 125.0).abs() < 1e-12);
        let ang = -2.0 * PI * freqs[k] / 1000.0;
        assert_close(resp[k], c(ang.cos(), ang.sin()), 1e-10);
    }
    assert!((freqs[4] - 500.0).abs() < 1e-12);
    let (one, hone) = freqz(&h, 1, 1000.0).unwrap();
    assert!((one[0] - 500.0).abs() < 1e-12);
    assert_close(hone[0], c(-1.0, 0.0), 1e-12);
}

#[test]
fn freqz_of_a_long_filter_is_the_dtft() {
    // Longer than 2 * n_points, so an FFT of length 2n would time-alias it.
    let h = real(&(0..20).map(|k| 0.05 * (k as f64)).collect::<Vec<_>>());
    let (freqs, resp) = freqz(&h, 4, 800.0).unwrap();
    assert!((freqs[3] - 400.0).abs() < 1e-12);
    for (k, &hk) in resp.iter().enumerate() {
        let mut acc = c(0.0, 0.0);
        for (m, hm) in h.iter().enumerate() {
            let ang = -2.0 * PI * freqs[k] / 800.0 * m as f64;
            acc += hm * c(ang.cos(), ang.sin());
        }
        assert_close(hk, acc, 1e-9);
    }
}

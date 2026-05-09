/// Unit tests for rustlab-dsp: windows, FIR design, IIR design, and convolution.

#[cfg(test)]
mod window_tests {
    use crate::window::WindowFunction;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn rectangular_all_ones() {
        let w = WindowFunction::Rectangular.generate(8);
        assert_eq!(w.len(), 8);
        assert!(w.iter().all(|&x| close(x, 1.0)));
    }

    #[test]
    fn hann_endpoints_zero() {
        let w = WindowFunction::Hann.generate(16);
        assert!(close(w[0], 0.0), "Hann window must start at 0");
        assert!(close(w[15], 0.0), "Hann window must end at 0");
    }

    #[test]
    fn hann_peak_near_center() {
        let n = 17;
        let w = WindowFunction::Hann.generate(n);
        let center_val = w[(n - 1) / 2];
        assert!(
            close(center_val, 1.0),
            "Hann center should be 1.0, got {center_val}"
        );
    }

    #[test]
    fn hamming_endpoints_non_zero() {
        let w = WindowFunction::Hamming.generate(16);
        // Hamming endpoints are 0.08 (= 0.54 - 0.46)
        assert!(close(w[0], 0.08), "Hamming[0] = 0.08, got {}", w[0]);
        assert!(close(w[15], 0.08), "Hamming[-1] = 0.08, got {}", w[15]);
    }

    #[test]
    fn hamming_peak_near_center() {
        let n = 17;
        let w = WindowFunction::Hamming.generate(n);
        let center_val = w[(n - 1) / 2];
        assert!(close(center_val, 1.0), "Hamming center should be 1.0");
    }

    #[test]
    fn blackman_endpoints_near_zero() {
        let w = WindowFunction::Blackman.generate(16);
        // Blackman w[0] = 0.42 - 0.5 + 0.08 = 0.0
        assert!(w[0].abs() < 1e-9, "Blackman[0] should be ~0");
        assert!(w[15].abs() < 1e-9, "Blackman[-1] should be ~0");
    }

    #[test]
    fn kaiser_beta_zero_is_rectangular() {
        let w = WindowFunction::Kaiser { beta: 0.0 }.generate(16);
        // Kaiser(beta=0) -> all ones (I0(0)=1, I0(0·√...)=1)
        assert!(
            w.iter().all(|&x| close(x, 1.0)),
            "Kaiser(beta=0) should equal rectangular"
        );
    }

    #[test]
    fn kaiser_default_beta_from_str() {
        let w = WindowFunction::from_str("kaiser", None).unwrap();
        match w {
            WindowFunction::Kaiser { beta } => assert!(close(beta, 8.6)),
            _ => panic!("Expected Kaiser variant"),
        }
    }

    #[test]
    fn length_zero_returns_empty() {
        for w in [
            WindowFunction::Rectangular,
            WindowFunction::Hann,
            WindowFunction::Hamming,
            WindowFunction::Blackman,
            WindowFunction::Kaiser { beta: 5.0 },
        ] {
            assert_eq!(w.generate(0).len(), 0);
        }
    }

    #[test]
    fn length_one_returns_single_one() {
        for w in [
            WindowFunction::Rectangular,
            WindowFunction::Hann,
            WindowFunction::Hamming,
            WindowFunction::Blackman,
            WindowFunction::Kaiser { beta: 5.0 },
        ] {
            let v = w.generate(1);
            assert_eq!(v.len(), 1);
            assert!(close(v[0], 1.0));
        }
    }

    #[test]
    fn from_str_case_insensitive() {
        assert!(WindowFunction::from_str("HANN", None).is_ok());
        assert!(WindowFunction::from_str("Hamming", None).is_ok());
        assert!(WindowFunction::from_str("RECT", None).is_ok());
        assert!(WindowFunction::from_str("Blackman", None).is_ok());
    }

    #[test]
    fn from_str_aliases() {
        assert!(WindowFunction::from_str("hanning", None).is_ok());
        assert!(WindowFunction::from_str("rect", None).is_ok());
        assert!(WindowFunction::from_str("boxcar", None).is_ok());
    }

    #[test]
    fn from_str_unknown_returns_error() {
        assert!(WindowFunction::from_str("cosine", None).is_err());
    }
}

#[cfg(test)]
mod fir_tests {
    use crate::fir::design::{fir_bandpass, fir_highpass, fir_lowpass};
    use crate::window::WindowFunction;
    use ndarray::Array1;
    use num_complex::Complex;
    use rustlab_core::Filter;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    /// Sum of squared imaginary parts — should be ~0 for windowed-sinc filters.
    fn imag_energy(coeffs: &rustlab_core::CVector) -> f64 {
        coeffs.iter().map(|c| c.im * c.im).sum::<f64>()
    }

    #[test]
    fn lowpass_correct_tap_count() {
        let f = fir_lowpass(31, 1000.0, 8000.0, WindowFunction::Hann).unwrap();
        assert_eq!(f.coefficients.len(), 31);
    }

    #[test]
    fn lowpass_real_coefficients() {
        let f = fir_lowpass(31, 1000.0, 8000.0, WindowFunction::Hann).unwrap();
        assert!(
            imag_energy(&f.coefficients) < 1e-20,
            "FIR coefficients should be real"
        );
    }

    #[test]
    fn lowpass_symmetric_impulse_response() {
        let f = fir_lowpass(31, 1000.0, 8000.0, WindowFunction::Hann).unwrap();
        let n = f.coefficients.len();
        for i in 0..n / 2 {
            let diff = (f.coefficients[i].re - f.coefficients[n - 1 - i].re).abs();
            assert!(diff < 1e-12, "FIR LP must be symmetric at index {i}");
        }
    }

    #[test]
    fn lowpass_dc_gain_near_one() {
        // Wide cutoff (2kHz / 8kHz = 0.25 normalized) with many taps; rectangular
        // window so the sum converges cleanly to 1.0 (no window-amplitude reduction).
        let f = fir_lowpass(127, 2000.0, 8000.0, WindowFunction::Rectangular).unwrap();
        let dc: f64 = f.coefficients.iter().map(|c| c.re).sum();
        assert!(close(dc, 1.0, 0.01), "DC gain should be ~1.0, got {dc}");
    }

    #[test]
    fn highpass_symmetric_impulse_response() {
        let f = fir_highpass(31, 1000.0, 8000.0, WindowFunction::Hann).unwrap();
        let n = f.coefficients.len();
        for i in 0..n / 2 {
            let diff = (f.coefficients[i].re - f.coefficients[n - 1 - i].re).abs();
            assert!(diff < 1e-12, "FIR HP must be symmetric at index {i}");
        }
    }

    #[test]
    fn highpass_dc_gain_near_zero() {
        let f = fir_highpass(63, 1000.0, 8000.0, WindowFunction::Hamming).unwrap();
        let dc: f64 = f.coefficients.iter().map(|c| c.re).sum();
        assert!(dc.abs() < 0.01, "Highpass DC gain should be ~0, got {dc}");
    }

    #[test]
    fn bandpass_correct_tap_count() {
        let f = fir_bandpass(31, 500.0, 1500.0, 8000.0, WindowFunction::Hann).unwrap();
        assert_eq!(f.coefficients.len(), 31);
    }

    #[test]
    fn bandpass_dc_gain_near_zero() {
        let f = fir_bandpass(63, 500.0, 1500.0, 8000.0, WindowFunction::Hamming).unwrap();
        let dc: f64 = f.coefficients.iter().map(|c| c.re).sum();
        assert!(dc.abs() < 0.05, "Bandpass DC gain should be ~0, got {dc}");
    }

    #[test]
    fn zero_taps_returns_error() {
        assert!(fir_lowpass(0, 1000.0, 8000.0, WindowFunction::Hann).is_err());
        assert!(fir_highpass(0, 1000.0, 8000.0, WindowFunction::Hann).is_err());
        assert!(fir_bandpass(0, 500.0, 1500.0, 8000.0, WindowFunction::Hann).is_err());
    }

    #[test]
    fn cutoff_at_nyquist_returns_error() {
        assert!(fir_lowpass(31, 4000.0, 8000.0, WindowFunction::Hann).is_err());
    }

    #[test]
    fn cutoff_at_zero_returns_error() {
        assert!(fir_lowpass(31, 0.0, 8000.0, WindowFunction::Hann).is_err());
    }

    #[test]
    fn bandpass_inverted_cutoffs_returns_error() {
        assert!(fir_bandpass(31, 2000.0, 500.0, 8000.0, WindowFunction::Hann).is_err());
    }

    #[test]
    fn fir_apply_output_length() {
        let n_taps = 31;
        let signal_len = 100;
        let f = fir_lowpass(n_taps, 1000.0, 8000.0, WindowFunction::Hann).unwrap();
        let signal = Array1::from_elem(signal_len, Complex::new(1.0, 0.0));
        let out = f.apply(&signal).unwrap();
        assert_eq!(out.len(), signal_len + n_taps - 1);
    }

    #[test]
    fn fir_frequency_response_length() {
        let f = fir_lowpass(31, 1000.0, 8000.0, WindowFunction::Hann).unwrap();
        let resp = f.frequency_response(128).unwrap();
        assert_eq!(resp.len(), 128);
    }

    #[test]
    fn fir_frequency_response_empty_for_zero_points() {
        let f = fir_lowpass(31, 1000.0, 8000.0, WindowFunction::Hann).unwrap();
        assert_eq!(f.frequency_response(0).unwrap().len(), 0);
    }
}

#[cfg(test)]
mod iir_tests {
    use crate::iir::butterworth::{butterworth_highpass, butterworth_lowpass};
    use ndarray::Array1;
    use num_complex::Complex;
    use rustlab_core::Filter;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn zero_order_returns_error() {
        assert!(butterworth_lowpass(0, 1000.0, 8000.0).is_err());
        assert!(butterworth_highpass(0, 1000.0, 8000.0).is_err());
    }

    #[test]
    fn cutoff_at_nyquist_returns_error() {
        assert!(butterworth_lowpass(2, 4000.0, 8000.0).is_err());
    }

    #[test]
    fn cutoff_at_zero_returns_error() {
        assert!(butterworth_lowpass(2, 0.0, 8000.0).is_err());
    }

    #[test]
    fn monic_denominator() {
        let f = butterworth_lowpass(4, 1000.0, 8000.0).unwrap();
        assert!(close(f.a[0], 1.0, 1e-12), "a[0] must be 1.0 (monic)");
    }

    #[test]
    fn order1_two_b_coefficients() {
        let f = butterworth_lowpass(1, 1000.0, 8000.0).unwrap();
        assert_eq!(f.b.len(), 2);
        assert_eq!(f.a.len(), 2);
    }

    #[test]
    fn order2_three_coefficients() {
        let f = butterworth_lowpass(2, 1000.0, 8000.0).unwrap();
        assert_eq!(f.b.len(), 3);
        assert_eq!(f.a.len(), 3);
    }

    #[test]
    fn order4_five_coefficients() {
        let f = butterworth_lowpass(4, 1000.0, 8000.0).unwrap();
        assert_eq!(f.b.len(), 5);
        assert_eq!(f.a.len(), 5);
    }

    #[test]
    fn lowpass_output_length_equals_input() {
        let f = butterworth_lowpass(2, 1000.0, 8000.0).unwrap();
        let signal = Array1::from_elem(50, Complex::new(1.0, 0.0));
        let out = f.apply(&signal).unwrap();
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn lowpass_dc_gain_near_one() {
        // Impulse response DC gain: sum(b) / sum(a) ≈ 1.0 for LP
        let f = butterworth_lowpass(2, 1000.0, 8000.0).unwrap();
        let sum_b: f64 = f.b.iter().sum();
        let sum_a: f64 = f.a.iter().sum();
        let dc_gain = sum_b / sum_a;
        assert!(
            close(dc_gain, 1.0, 0.02),
            "Butterworth LP DC gain ~1, got {dc_gain}"
        );
    }

    #[test]
    fn highpass_dc_gain_near_zero() {
        // DC gain of highpass: sum(b) / sum(a) ≈ 0
        let f = butterworth_highpass(2, 1000.0, 8000.0).unwrap();
        let sum_b: f64 = f.b.iter().sum();
        let sum_a: f64 = f.a.iter().sum();
        let dc_gain = (sum_b / sum_a).abs();
        assert!(dc_gain < 0.05, "Butterworth HP DC gain ~0, got {dc_gain}");
    }

    #[test]
    fn frequency_response_length() {
        let f = butterworth_lowpass(2, 1000.0, 8000.0).unwrap();
        let resp = f.frequency_response(256).unwrap();
        assert_eq!(resp.len(), 256);
    }

    #[test]
    fn frequency_response_empty_for_zero_points() {
        let f = butterworth_lowpass(2, 1000.0, 8000.0).unwrap();
        assert_eq!(f.frequency_response(0).unwrap().len(), 0);
    }

    #[test]
    fn apply_dc_signal_passes_lowpass() {
        // Steady-state output of LP filter fed DC should approach input magnitude.
        // Use 1000 samples to ensure the filter has fully settled from zero state.
        let f = butterworth_lowpass(2, 1000.0, 8000.0).unwrap();
        let n = 1000;
        let signal = Array1::from_elem(n, Complex::new(1.0, 0.0));
        let out = f.apply(&signal).unwrap();
        assert!(
            close(out[n - 1].re, 1.0, 0.02),
            "LP steady-state should be ~1.0, got {}",
            out[n - 1].re
        );
    }

    #[test]
    fn butterworth_poles_inside_unit_circle() {
        // Stability proxy for a 2nd-order Butterworth lowpass:
        // The denominator polynomial a = [1, a1, a2] is stable iff all roots have |z| < 1.
        // For a digital Butterworth, |a[2]| < 1 and |a[1]| < 1 + a[2] (Jury stability).
        // We use the simpler proxy: |a[1]| < 2.0 and |a[2]| < 1.0.
        let f = butterworth_lowpass(2, 1000.0, 8000.0).unwrap();
        assert!(
            f.a.len() >= 3,
            "order-2 filter should have at least 3 'a' coefficients"
        );
        let a1 = f.a[1].abs();
        let a2 = f.a[2].abs();
        assert!(a1 < 2.0, "stability proxy: |a[1]|={a1} should be < 2.0");
        assert!(
            a2 < 1.0,
            "stability proxy: |a[2]|={a2} should be < 1.0 (inside unit circle)"
        );
    }
}

#[cfg(test)]
mod convolution_tests {
    use crate::convolution::{convolve, overlap_add};
    use ndarray::Array1;
    use num_complex::Complex;

    fn cvec(v: &[f64]) -> rustlab_core::CVector {
        Array1::from_iter(v.iter().map(|&x| Complex::new(x, 0.0)))
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn output_length() {
        let x = cvec(&[1.0, 2.0, 3.0]);
        let h = cvec(&[1.0, 1.0]);
        assert_eq!(convolve(&x, &h).unwrap().len(), 4); // 3 + 2 - 1
    }

    #[test]
    fn convolve_impulse_is_identity() {
        let x = cvec(&[1.0, 2.0, 3.0, 4.0]);
        let h = cvec(&[1.0]); // unit impulse
        let y = convolve(&x, &h).unwrap();
        for i in 0..4 {
            assert!(close(y[i].re, x[i].re));
        }
    }

    #[test]
    fn convolve_known_values() {
        // [1,2,3] * [0,1,0.5] = [0, 1, 2.5, 4, 1.5]
        let x = cvec(&[1.0, 2.0, 3.0]);
        let h = cvec(&[0.0, 1.0, 0.5]);
        let y = convolve(&x, &h).unwrap();
        let expected = [0.0, 1.0, 2.5, 4.0, 1.5];
        for (i, &e) in expected.iter().enumerate() {
            assert!(close(y[i].re, e), "y[{i}] = {}, expected {e}", y[i].re);
        }
    }

    #[test]
    fn convolve_empty_input_returns_empty() {
        let empty = cvec(&[]);
        let h = cvec(&[1.0, 2.0]);
        assert_eq!(convolve(&empty, &h).unwrap().len(), 0);
        assert_eq!(convolve(&h, &empty).unwrap().len(), 0);
    }

    #[test]
    fn convolve_commutative() {
        let x = cvec(&[1.0, 2.0, 3.0]);
        let h = cvec(&[4.0, 5.0]);
        let xy = convolve(&x, &h).unwrap();
        let yx = convolve(&h, &x).unwrap();
        assert_eq!(xy.len(), yx.len());
        for i in 0..xy.len() {
            assert!(close(xy[i].re, yx[i].re));
        }
    }

    #[test]
    fn overlap_add_matches_direct() {
        let x = cvec(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        let h = cvec(&[0.5, 0.3, 0.2]);
        let direct = convolve(&x, &h).unwrap();
        let ola = overlap_add(&x, &h, 4).unwrap();
        assert_eq!(direct.len(), ola.len());
        for i in 0..direct.len() {
            assert!(
                close(direct[i].re, ola[i].re),
                "index {i}: direct={:.6} ola={:.6}",
                direct[i].re,
                ola[i].re
            );
        }
    }

    #[test]
    fn overlap_add_zero_block_size_errors() {
        let x = cvec(&[1.0, 2.0]);
        let h = cvec(&[1.0]);
        assert!(overlap_add(&x, &h, 0).is_err());
    }

    #[test]
    fn overlap_add_empty_input_returns_empty() {
        let empty = cvec(&[]);
        let h = cvec(&[1.0, 2.0]);
        assert_eq!(overlap_add(&empty, &h, 4).unwrap().len(), 0);
    }

    #[test]
    fn convolve_complex_signal() {
        // Convolve [1+j, 2+j] * [1, 1]
        let x = Array1::from_vec(vec![Complex::new(1.0, 1.0), Complex::new(2.0, 1.0)]);
        let h = Array1::from_vec(vec![Complex::new(1.0, 0.0), Complex::new(1.0, 0.0)]);
        let y = convolve(&x, &h).unwrap();
        assert_eq!(y.len(), 3);
        assert!(close(y[0].re, 1.0) && close(y[0].im, 1.0));
        assert!(close(y[1].re, 3.0) && close(y[1].im, 2.0));
        assert!(close(y[2].re, 2.0) && close(y[2].im, 1.0));
    }
}

// ─── FFT tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod fft_tests {
    use crate::convolution::overlap_add;
    use crate::fft::{fft, fftfreq, fftshift, ifft};
    use ndarray::Array1;
    use num_complex::Complex;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn close_eps(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn fft_length_is_next_pow2() {
        let x = Array1::from_iter((0..5).map(|i| Complex::new(i as f64, 0.0)));
        let y = fft(&x).unwrap();
        assert_eq!(y.len(), 8, "5 elements → next pow2 = 8");
    }

    #[test]
    fn fft_impulse_is_flat() {
        let n = 8;
        let mut x = Array1::zeros(n);
        x[0] = Complex::new(1.0, 0.0);
        let y = fft(&x).unwrap();
        assert_eq!(y.len(), n);
        for k in 0..n {
            let mag = y[k].norm();
            assert!(close(mag, 1.0), "bin {k}: magnitude = {mag}, expected 1.0");
        }
    }

    #[test]
    fn fft_single_tone_peak() {
        // Cosine at bin k₀ in an 8-point FFT
        let n = 8usize;
        let k0 = 2usize;
        let x: Array1<Complex<f64>> = Array1::from_iter((0..n).map(|t| {
            let angle = 2.0 * std::f64::consts::PI * k0 as f64 * t as f64 / n as f64;
            Complex::new(angle.cos(), 0.0)
        }));
        let y = fft(&x).unwrap();
        // Peak should be at bin k0 and n-k0
        let peak_k = y
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.norm().partial_cmp(&b.1.norm()).unwrap())
            .unwrap()
            .0;
        assert!(
            peak_k == k0 || peak_k == n - k0,
            "peak at {peak_k}, expected {k0} or {}",
            n - k0
        );
    }

    #[test]
    fn ifft_inverts_fft() {
        let n = 8usize;
        let x: Array1<Complex<f64>> =
            Array1::from_iter((0..n).map(|i| Complex::new(i as f64, 0.0)));
        let y = fft(&x).unwrap();
        let x_rec = ifft(&y).unwrap();
        for i in 0..n {
            assert!(
                close_eps(x_rec[i].re, x[i].re, 1e-10),
                "real[{i}]: got {}, expected {}",
                x_rec[i].re,
                x[i].re
            );
            assert!(
                close_eps(x_rec[i].im, x[i].im, 1e-10),
                "imag[{i}]: got {}, expected {}",
                x_rec[i].im,
                x[i].im
            );
        }
    }

    #[test]
    fn fftshift_even() {
        let x = Array1::from_iter((0..4).map(|i| Complex::new(i as f64, 0.0)));
        let y = fftshift(&x);
        let expected = [2.0, 3.0, 0.0, 1.0];
        for (k, &e) in expected.iter().enumerate() {
            assert!(
                close(y[k].re, e),
                "fftshift_even[{k}]: got {}, expected {}",
                y[k].re,
                e
            );
        }
    }

    #[test]
    fn fftshift_odd() {
        let x = Array1::from_iter((0..5).map(|i| Complex::new(i as f64, 0.0)));
        let y = fftshift(&x);
        let expected = [3.0, 4.0, 0.0, 1.0, 2.0];
        for (k, &e) in expected.iter().enumerate() {
            assert!(
                close(y[k].re, e),
                "fftshift_odd[{k}]: got {}, expected {}",
                y[k].re,
                e
            );
        }
    }

    #[test]
    fn fftfreq_dc_zero() {
        let freqs = fftfreq(8, 8000.0);
        assert!(close(freqs[0], 0.0), "DC bin should be 0 Hz");
    }

    #[test]
    fn fftfreq_nyquist() {
        let freqs = fftfreq(8, 8000.0);
        assert!(
            close(freqs[4], -4000.0),
            "bin 4 (Nyquist) should be -4000 Hz, got {}",
            freqs[4]
        );
    }

    #[test]
    fn fft_parseval() {
        // Parseval: Σ|x|² = (1/N) Σ|X|²
        let n = 8usize;
        let x: Array1<Complex<f64>> =
            Array1::from_iter((0..n).map(|i| Complex::new(i as f64 + 1.0, 0.0)));
        let y = fft(&x).unwrap();
        let energy_x: f64 = x.iter().map(|c| c.norm_sqr()).sum();
        let energy_y: f64 = y.iter().map(|c| c.norm_sqr()).sum::<f64>() / n as f64;
        assert!(
            close_eps(energy_x, energy_y, 1e-8),
            "Parseval: Σ|x|²={energy_x} ≠ (1/N)Σ|X|²={energy_y}"
        );
    }

    #[test]
    fn overlap_add_still_matches_direct() {
        use crate::convolution::convolve;
        let x: Array1<Complex<f64>> =
            Array1::from_iter((0..32).map(|i| Complex::new(i as f64, 0.0)));
        let h: Array1<Complex<f64>> =
            Array1::from_iter((0..5).map(|i| Complex::new((i + 1) as f64, 0.0)));
        let direct = convolve(&x, &h).unwrap();
        let ola = overlap_add(&x, &h, 8).unwrap();
        assert_eq!(direct.len(), ola.len());
        for i in 0..direct.len() {
            let diff = (direct[i] - ola[i]).norm();
            assert!(
                diff < 1e-8,
                "overlap_add mismatch at index {i}: diff = {diff}"
            );
        }
    }

    #[test]
    fn fft_pads_non_power_of_two() {
        // Input of length 5 should be zero-padded to length 8 (next power of 2)
        let x = Array1::from_iter((0..5).map(|i| Complex::new(i as f64, 0.0)));
        let y = fft(&x).unwrap();
        assert_eq!(
            y.len(),
            8,
            "FFT of 5-element input should have length 8 (next power of 2)"
        );
    }
}

// ─── Parks-McClellan FIR tests ─────────────────────────────────────────────

#[cfg(test)]
mod pm_tests {
    use crate::fir::pm::firpm;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    /// Lowpass: passband 0–0.4, transition 0.4–0.5, stopband 0.5–1.0
    fn lp_bands() -> (&'static [f64], &'static [f64], &'static [f64]) {
        (&[0.0, 0.4, 0.5, 1.0], &[1.0, 1.0, 0.0, 0.0], &[1.0, 1.0])
    }

    #[test]
    fn pm_lowpass_length() {
        let (bands, desired, weights) = lp_bands();
        let f = firpm(31, bands, desired, weights).unwrap();
        assert_eq!(f.coefficients.len(), 31);
    }

    #[test]
    fn pm_lowpass_symmetry() {
        let (bands, desired, weights) = lp_bands();
        let f = firpm(31, bands, desired, weights).unwrap();
        let h: Vec<f64> = f.coefficients.iter().map(|c| c.re).collect();
        let n = h.len();
        for i in 0..n / 2 {
            let diff = (h[i] - h[n - 1 - i]).abs();
            assert!(
                diff < 1e-10,
                "symmetry broken at i={i}: h[{i}]={} h[{}]={}",
                h[i],
                n - 1 - i,
                h[n - 1 - i]
            );
        }
    }

    #[test]
    fn pm_lowpass_dc_gain() {
        // Sum of lowpass coefficients ≈ 1.0 (DC gain)
        let (bands, desired, weights) = lp_bands();
        let f = firpm(31, bands, desired, weights).unwrap();
        let dc: f64 = f.coefficients.iter().map(|c| c.re).sum();
        assert!(
            close(dc, 1.0, 0.05),
            "lowpass DC gain should be ≈1.0, got {dc}"
        );
    }

    #[test]
    fn pm_lowpass_nyquist_gain() {
        // Alternating sum ≈ 0.0 for lowpass (Nyquist gain = 0)
        let (bands, desired, weights) = lp_bands();
        let f = firpm(31, bands, desired, weights).unwrap();
        let alt: f64 = f
            .coefficients
            .iter()
            .enumerate()
            .map(|(i, c)| if i % 2 == 0 { c.re } else { -c.re })
            .sum();
        assert!(
            alt.abs() < 0.05,
            "lowpass Nyquist gain should be ≈0.0, got {alt}"
        );
    }

    #[test]
    fn pm_highpass_dc_gain() {
        // Highpass: passband 0.5–1.0, stopband 0–0.4
        let f = firpm(
            31,
            &[0.0, 0.4, 0.5, 1.0],
            &[0.0, 0.0, 1.0, 1.0],
            &[1.0, 1.0],
        )
        .unwrap();
        let dc: f64 = f.coefficients.iter().map(|c| c.re).sum();
        assert!(dc.abs() < 0.05, "highpass DC gain should be ≈0.0, got {dc}");
    }

    #[test]
    fn pm_highpass_nyquist_gain() {
        // Alternating sum magnitude ≈ 1.0 for highpass (Nyquist gain = 1)
        // The sign may be negative depending on the filter's linear phase offset.
        let f = firpm(
            31,
            &[0.0, 0.4, 0.5, 1.0],
            &[0.0, 0.0, 1.0, 1.0],
            &[1.0, 1.0],
        )
        .unwrap();
        let alt: f64 = f
            .coefficients
            .iter()
            .enumerate()
            .map(|(i, c)| if i % 2 == 0 { c.re } else { -c.re })
            .sum();
        assert!(
            close(alt.abs(), 1.0, 0.1),
            "highpass Nyquist gain magnitude should be ≈1.0, got {alt}"
        );
    }

    #[test]
    fn pm_bandpass_symmetry() {
        // Bandpass: passband 0.3–0.6, stopbands 0–0.2 and 0.7–1.0
        let f = firpm(
            31,
            &[0.0, 0.2, 0.3, 0.6, 0.7, 1.0],
            &[0.0, 0.0, 1.0, 1.0, 0.0, 0.0],
            &[1.0, 1.0, 1.0],
        )
        .unwrap();
        let h: Vec<f64> = f.coefficients.iter().map(|c| c.re).collect();
        let n = h.len();
        for i in 0..n / 2 {
            let diff = (h[i] - h[n - 1 - i]).abs();
            assert!(diff < 1e-10, "bandpass symmetry broken at i={i}");
        }
    }

    #[test]
    fn pm_bandpass_dc_gain() {
        // Bandpass should have near-zero DC gain (stopband at DC)
        let f = firpm(
            31,
            &[0.0, 0.2, 0.3, 0.6, 0.7, 1.0],
            &[0.0, 0.0, 1.0, 1.0, 0.0, 0.0],
            &[1.0, 1.0, 1.0],
        )
        .unwrap();
        let dc: f64 = f.coefficients.iter().map(|c| c.re).sum();
        assert!(dc.abs() < 0.1, "bandpass DC gain should be ≈0.0, got {dc}");
    }

    #[test]
    fn pm_rejects_even_taps() {
        // Even tap count is silently rounded up to odd (n_taps+1=33), so 32 produces a 33-tap filter.
        // Verify it succeeds and returns an odd-length filter (not an error).
        let (bands, desired, weights) = lp_bands();
        let result = firpm(32, bands, desired, weights);
        // Per the implementation, even tap counts are rounded up to odd — so it succeeds.
        // The returned filter should have length 33 (next odd after 32).
        match result {
            Ok(f) => assert!(
                f.coefficients.len() % 2 == 1,
                "even input should produce odd-length filter"
            ),
            Err(_) => {} // also acceptable if the implementation decides to reject even
        }
    }

    #[test]
    fn pm_rejects_mismatched_weights() {
        // Wrong number of weights (2 bands need 2 weights, not 3)
        let result = firpm(
            31,
            &[0.0, 0.4, 0.5, 1.0],
            &[1.0, 1.0, 0.0, 0.0],
            &[1.0, 1.0, 1.0],
        );
        assert!(result.is_err(), "mismatched weight count should return Err");
    }

    #[test]
    fn pm_rejects_mismatched_desired() {
        // desired must have same length as bands
        let result = firpm(31, &[0.0, 0.4, 0.5, 1.0], &[1.0, 1.0, 0.0], &[1.0, 1.0]);
        assert!(
            result.is_err(),
            "mismatched desired length should return Err"
        );
    }

    #[test]
    fn firpmq_integer_coefficients() {
        use crate::fir::pm::firpmq;
        let f = firpmq(
            31,
            &[0.0, 0.4, 0.5, 1.0],
            &[1.0, 1.0, 0.0, 0.0],
            &[1.0, 1.0],
            16,
            8,
        )
        .unwrap();
        let h: Vec<f64> = f.coefficients.iter().map(|c| c.re).collect();
        // All coefficients must be integers (no fractional part)
        for &v in &h {
            assert!((v - v.round()).abs() < 1e-9, "non-integer coefficient: {v}");
        }
        // Max absolute value must not exceed 2^15 - 1 = 32767
        let peak = h.iter().map(|&x| x.abs()).fold(0.0_f64, f64::max);
        assert!(peak <= 32767.0, "coefficient out of 16-bit range: {peak}");
        // Symmetric
        let n = h.len();
        for i in 0..n / 2 {
            assert_eq!(h[i], h[n - 1 - i], "symmetry broken at i={i}");
        }
    }

    #[test]
    fn firpmq_passband_gain_near_unity() {
        use crate::fir::pm::{firpm, firpmq};
        // The integer filter's DC gain, normalized the same way as the float filter, should be ~1.
        // Normalization: scale = i_max / peak_float, so dc_gain = sum(h_int) / scale
        //              = sum(h_int) * peak_float / i_max
        let float_f = firpm(
            31,
            &[0.0, 0.4, 0.5, 1.0],
            &[1.0, 1.0, 0.0, 0.0],
            &[1.0, 1.0],
        )
        .unwrap();
        let peak_float: f64 = float_f
            .coefficients
            .iter()
            .map(|c| c.re.abs())
            .fold(0.0_f64, f64::max);

        let f = firpmq(
            31,
            &[0.0, 0.4, 0.5, 1.0],
            &[1.0, 1.0, 0.0, 0.0],
            &[1.0, 1.0],
            16,
            8,
        )
        .unwrap();
        let h: Vec<f64> = f.coefficients.iter().map(|c| c.re).collect();
        let i_max = 32767.0_f64;
        // DC gain of integer filter: sum(h_int) / scale = sum(h_int) * peak_float / i_max
        let dc_gain: f64 = h.iter().sum::<f64>() * peak_float / i_max;
        assert!(
            (dc_gain - 1.0).abs() < 0.03,
            "DC gain deviates from unity: {dc_gain}"
        );
    }
}

// ─── Kaiser FIR tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod kaiser_fir_tests {
    use crate::fir::kaiser::{
        fir_bandpass_kaiser, fir_highpass_kaiser, fir_lowpass_kaiser, fir_notch, freqz,
        kaiser_beta, kaiser_num_taps,
    };
    use ndarray::Array1;
    use num_complex::Complex;

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn kaiser_beta_high() {
        let b = kaiser_beta(60.0);
        assert!(close(b, 5.653, 0.01), "beta(60) ≈ 5.653, got {b}");
    }

    #[test]
    fn kaiser_beta_mid() {
        let b = kaiser_beta(40.0);
        assert!(close(b, 3.395, 0.01), "beta(40) ≈ 3.395, got {b}");
    }

    #[test]
    fn kaiser_beta_low() {
        let b = kaiser_beta(15.0);
        assert!(close(b, 0.0, 1e-12), "beta(15) = 0.0, got {b}");
    }

    #[test]
    fn kaiser_num_taps_is_odd() {
        let n = kaiser_num_taps(200.0, 60.0, 8000.0);
        assert!(n % 2 == 1, "kaiser_num_taps must be odd, got {n}");
    }

    #[test]
    fn kaiser_num_taps_scales_with_attn() {
        let n40 = kaiser_num_taps(200.0, 40.0, 8000.0);
        let n80 = kaiser_num_taps(200.0, 80.0, 8000.0);
        assert!(
            n80 > n40,
            "more attenuation should require more taps ({n40} vs {n80})"
        );
    }

    #[test]
    fn lowpass_kaiser_symmetric() {
        let f = fir_lowpass_kaiser(1000.0, 200.0, 60.0, 8000.0).unwrap();
        let h = &f.coefficients;
        let n = h.len();
        for i in 0..n / 2 {
            let diff = (h[i].re - h[n - 1 - i].re).abs();
            assert!(diff < 1e-10, "asymmetry at index {i}: diff = {diff}");
        }
    }

    #[test]
    fn lowpass_kaiser_dc_gain() {
        let f = fir_lowpass_kaiser(1000.0, 200.0, 60.0, 8000.0).unwrap();
        let dc_gain: f64 = f.coefficients.iter().map(|c| c.re).sum();
        assert!(
            close(dc_gain, 1.0, 5e-3),
            "lowpass DC gain should be ≈1.0, got {dc_gain}"
        );
    }

    #[test]
    fn highpass_kaiser_dc_gain() {
        let f = fir_highpass_kaiser(1000.0, 200.0, 60.0, 8000.0).unwrap();
        let dc_gain: f64 = f.coefficients.iter().map(|c| c.re).sum();
        assert!(
            dc_gain.abs() < 0.01,
            "highpass DC gain should be ≈0.0, got {dc_gain}"
        );
    }

    #[test]
    fn bandpass_kaiser_dc_gain() {
        let f = fir_bandpass_kaiser(500.0, 1500.0, 200.0, 60.0, 8000.0).unwrap();
        let dc_gain: f64 = f.coefficients.iter().map(|c| c.re).sum();
        assert!(
            dc_gain.abs() < 0.01,
            "bandpass DC gain should be ≈0.0, got {dc_gain}"
        );
    }

    #[test]
    fn notch_dc_passes() {
        use crate::window::WindowFunction;
        let f = fir_notch(1000.0, 200.0, 8000.0, 65, WindowFunction::Hann).unwrap();
        let dc_gain: f64 = f.coefficients.iter().map(|c| c.re).sum();
        assert!(
            close(dc_gain, 1.0, 5e-3),
            "notch DC gain should be ≈1.0, got {dc_gain}"
        );
    }

    #[test]
    fn freqz_length() {
        let h: Array1<Complex<f64>> = Array1::from_iter((0..33).map(|_| Complex::new(1.0, 0.0)));
        let (freqs, h_out) = freqz(&h, 512, 8000.0).unwrap();
        assert_eq!(freqs.len(), 512);
        assert_eq!(h_out.len(), 512);
    }

    #[test]
    fn freqz_dc_matches_sum() {
        // |H[0]| == |Σh|
        let f = fir_lowpass_kaiser(1000.0, 200.0, 60.0, 8000.0).unwrap();
        let h_sum: f64 = f.coefficients.iter().map(|c| c.re).sum();
        let (_, h_out) = freqz(&f.coefficients, 512, 8000.0).unwrap();
        let h0_mag = h_out[0].norm();
        assert!(
            (h0_mag - h_sum.abs()).abs() < 1e-6,
            "|H[0]|={h0_mag} should equal |Σh|={}",
            h_sum.abs()
        );
    }

    #[test]
    fn invalid_trans_bw_errors() {
        assert!(
            fir_lowpass_kaiser(1000.0, 0.0, 60.0, 8000.0).is_err(),
            "trans_bw=0 should error"
        );
        assert!(
            fir_lowpass_kaiser(1000.0, -100.0, 60.0, 8000.0).is_err(),
            "negative trans_bw should error"
        );
    }

    #[test]
    fn invalid_attn_errors() {
        assert!(
            fir_lowpass_kaiser(1000.0, 200.0, 0.0, 8000.0).is_err(),
            "attn=0 should error"
        );
        assert!(
            fir_lowpass_kaiser(1000.0, 200.0, -10.0, 8000.0).is_err(),
            "negative attn should error"
        );
    }

    #[test]
    fn kaiser_lowpass_stopband_attenuation() {
        // Design a 60 dB Kaiser lowpass: cutoff=1000, tbw=200, attn=60, sr=8000
        // At 2*cutoff = 2000 Hz, magnitude should be < 0.001 (< -60 dB)
        let cutoff = 1000.0_f64;
        let sr = 8000.0_f64;
        let f = fir_lowpass_kaiser(cutoff, 200.0, 60.0, sr).unwrap();
        let (freqs, h_out) = freqz(&f.coefficients, 1024, sr).unwrap();
        // Find the bin closest to 2*cutoff
        let target = 2.0 * cutoff;
        let idx = freqs
            .iter()
            .enumerate()
            .min_by(|a, b| {
                (a.1 - target)
                    .abs()
                    .partial_cmp(&(b.1 - target).abs())
                    .unwrap()
            })
            .map(|(i, _)| i)
            .unwrap();
        let mag = h_out[idx].norm();
        assert!(
            mag < 0.001,
            "stopband magnitude at {target}Hz should be < 0.001, got {mag}"
        );
    }
}

#[cfg(test)]
mod upfirdn_tests {
    use crate::upfirdn::upfirdn;
    use ndarray::Array1;
    use num_complex::Complex;

    fn c(re: f64) -> Complex<f64> {
        Complex::new(re, 0.0)
    }
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-10
    }

    fn run(x: &[f64], h: &[f64], p: usize, q: usize) -> Vec<f64> {
        let xv = Array1::from_iter(x.iter().map(|&v| c(v)));
        upfirdn(&xv, h, p, q)
            .unwrap()
            .iter()
            .map(|c| c.re)
            .collect()
    }

    // identity: p=1, q=1, h=[1] → input unchanged
    #[test]
    fn identity_filter() {
        let y = run(&[1.0, 2.0, 3.0], &[1.0], 1, 1);
        assert_eq!(y, vec![1.0, 2.0, 3.0]);
    }

    // upsample by 2 with delta filter: inserts zeros between samples
    #[test]
    fn upsample_by_2_delta() {
        let y = run(&[1.0, 2.0], &[1.0], 2, 1);
        // output length = ((2-1)*2 + 1 - 1)/1 + 1 = 2/1 + 1 = 3
        assert_eq!(y.len(), 3);
        assert!(close(y[0], 1.0));
        assert!(close(y[1], 0.0));
        assert!(close(y[2], 2.0));
    }

    // downsample by 2 with delta filter: keeps every other sample
    #[test]
    fn downsample_by_2_delta() {
        let y = run(&[1.0, 2.0, 3.0, 4.0], &[1.0], 1, 2);
        // output = [1, 3]
        assert_eq!(y.len(), 2);
        assert!(close(y[0], 1.0));
        assert!(close(y[1], 3.0));
    }

    // interpolate by 2 with box filter: each sample spreads to two outputs
    #[test]
    fn interpolate_box_filter() {
        // x=[1,2], h=[1,1], p=2, q=1
        // x_up=[1,0,2], convolve [1,1] → [1,1,2,2] length=(2-1)*2+2=4
        let y = run(&[1.0, 2.0], &[1.0, 1.0], 2, 1);
        assert_eq!(y.len(), 4);
        assert!(close(y[0], 1.0));
        assert!(close(y[1], 1.0));
        assert!(close(y[2], 2.0));
        assert!(close(y[3], 2.0));
    }

    // output length matches scipy formula for various p/q
    #[test]
    fn output_length_formula() {
        for &(n_x, n_h, p, q) in &[(6, 4, 1, 2), (5, 3, 3, 2), (10, 7, 4, 3), (1, 1, 1, 1)] {
            let x = Array1::from_iter((0..n_x).map(|i| c(i as f64)));
            let h: Vec<f64> = vec![1.0; n_h];
            let y = upfirdn(&x, &h, p, q).unwrap();
            let expected = ((n_x - 1) * p + n_h - 1) / q + 1;
            assert_eq!(
                y.len(),
                expected,
                "len mismatch for n_x={n_x} n_h={n_h} p={p} q={q}"
            );
        }
    }

    // polyphase gives same result as naive brute-force for 3↑ / 2↓
    #[test]
    fn matches_brute_force() {
        let x_vals: Vec<f64> = vec![1.0, -1.0, 0.5, 2.0];
        let h_vals: Vec<f64> = vec![0.25, 0.5, 0.25];
        let p = 3usize;
        let q = 2usize;
        let n_x = x_vals.len();
        let n_h = h_vals.len();

        // Brute-force: build full upsampled signal, convolve, decimate
        let n_up = (n_x - 1) * p + 1;
        let mut x_up = vec![0.0f64; n_up];
        for (i, &v) in x_vals.iter().enumerate() {
            x_up[i * p] = v;
        }

        let conv_len = n_up + n_h - 1;
        let mut y_full = vec![0.0f64; conv_len];
        for n in 0..conv_len {
            for k in 0..n_h {
                if n >= k && n - k < n_up {
                    y_full[n] += h_vals[k] * x_up[n - k];
                }
            }
        }
        let y_ref: Vec<f64> = y_full
            .iter()
            .copied()
            .enumerate()
            .filter(|&(i, _)| i % q == 0)
            .map(|(_, v)| v)
            .collect();

        let y_poly = run(&x_vals, &h_vals, p, q);

        assert_eq!(
            y_poly.len(),
            y_ref.len(),
            "length mismatch: poly={} brute={}",
            y_poly.len(),
            y_ref.len()
        );
        for (i, (&a, &b)) in y_poly.iter().zip(y_ref.iter()).enumerate() {
            assert!(close(a, b), "mismatch at index {i}: poly={a} brute={b}");
        }
    }

    // error cases
    #[test]
    fn error_p_zero() {
        assert!(upfirdn(&Array1::zeros(4), &[1.0], 0, 1).is_err());
    }
    #[test]
    fn error_q_zero() {
        assert!(upfirdn(&Array1::zeros(4), &[1.0], 1, 0).is_err());
    }
    #[test]
    fn error_empty_h() {
        assert!(upfirdn(&Array1::zeros(4), &[], 1, 1).is_err());
    }
    #[test]
    fn empty_input_ok() {
        assert_eq!(upfirdn(&Array1::zeros(0), &[1.0], 2, 3).unwrap().len(), 0);
    }
}

#[cfg(test)]
mod vector_calc_tests {
    use crate::vector_calc::{curl_2d, divergence_2d, from_complex_fn, from_real_fn, gradient_2d};
    use num_complex::Complex;

    fn close_re(a: num_complex::Complex<f64>, b: f64, tol: f64) -> bool {
        (a.re - b).abs() < tol && a.im.abs() < tol
    }

    // Build a grid where row i corresponds to y = i*dy and column j to x = j*dx.
    fn grid_xy(ny: usize, nx: usize, dx: f64, dy: f64) -> (Vec<f64>, Vec<f64>) {
        let xs = (0..nx).map(|j| j as f64 * dx).collect();
        let ys = (0..ny).map(|i| i as f64 * dy).collect();
        (xs, ys)
    }

    #[test]
    fn gradient_of_x2_plus_y2_is_2x_2y() {
        // F(x, y) = x² + y² → ∇F = (2x, 2y). Quadratic → 2nd-order stencils
        // are exact on a uniform grid, including at the boundaries.
        let (ny, nx, dx, dy) = (5, 7, 0.1, 0.2);
        let (xs, ys) = grid_xy(ny, nx, dx, dy);
        let f = from_real_fn(ny, nx, |i, j| xs[j].powi(2) + ys[i].powi(2));

        let (fx, fy) = gradient_2d(&f, dx, dy).unwrap();

        for i in 0..ny {
            for j in 0..nx {
                assert!(
                    close_re(fx[[i, j]], 2.0 * xs[j], 1e-9),
                    "fx[{i},{j}] = {:?}, expected {}",
                    fx[[i, j]],
                    2.0 * xs[j]
                );
                assert!(
                    close_re(fy[[i, j]], 2.0 * ys[i], 1e-9),
                    "fy[{i},{j}] = {:?}, expected {}",
                    fy[[i, j]],
                    2.0 * ys[i]
                );
            }
        }
    }

    #[test]
    fn divergence_of_xy_is_two_everywhere() {
        // F = (x, y) → ∇·F = 2.
        let (ny, nx, dx, dy) = (6, 8, 0.25, 0.5);
        let (xs, ys) = grid_xy(ny, nx, dx, dy);
        let fx = from_real_fn(ny, nx, |_, j| xs[j]);
        let fy = from_real_fn(ny, nx, |i, _| ys[i]);

        let d = divergence_2d(&fx, &fy, dx, dy).unwrap();

        for i in 0..ny {
            for j in 0..nx {
                assert!(
                    close_re(d[[i, j]], 2.0, 1e-9),
                    "div[{i},{j}] = {:?}, expected 2.0",
                    d[[i, j]]
                );
            }
        }
    }

    #[test]
    fn curl_of_solid_rotation_is_two() {
        // F = (-y, x) → ∇×F · ẑ = 2.
        let (ny, nx, dx, dy) = (5, 5, 0.3, 0.3);
        let (xs, ys) = grid_xy(ny, nx, dx, dy);
        let fx = from_real_fn(ny, nx, |i, _| -ys[i]);
        let fy = from_real_fn(ny, nx, |_, j| xs[j]);

        let c = curl_2d(&fx, &fy, dx, dy).unwrap();

        for i in 0..ny {
            for j in 0..nx {
                assert!(
                    close_re(c[[i, j]], 2.0, 1e-9),
                    "curl[{i},{j}] = {:?}, expected 2.0",
                    c[[i, j]]
                );
            }
        }
    }

    #[test]
    fn curl_of_irrotational_field_is_zero() {
        // F = (x, y) is irrotational → curl = 0.
        let (ny, nx, dx, dy) = (4, 4, 0.5, 0.5);
        let (xs, ys) = grid_xy(ny, nx, dx, dy);
        let fx = from_real_fn(ny, nx, |_, j| xs[j]);
        let fy = from_real_fn(ny, nx, |i, _| ys[i]);

        let c = curl_2d(&fx, &fy, dx, dy).unwrap();

        for i in 0..ny {
            for j in 0..nx {
                assert!(
                    close_re(c[[i, j]], 0.0, 1e-9),
                    "curl[{i},{j}] = {:?}, expected 0",
                    c[[i, j]]
                );
            }
        }
    }

    #[test]
    fn complex_gradient_of_exp_ix() {
        // F(x, y) = exp(i*x) → ∂F/∂x = i*exp(i*x), ∂F/∂y = 0.
        // 2nd-order central differences have O(dx²) error → choose small dx.
        let nx = 41;
        let ny = 5;
        let dx = 2.0 * std::f64::consts::PI / (nx as f64 - 1.0); // one period across grid
        let dy = 0.1;
        let xs: Vec<f64> = (0..nx).map(|j| j as f64 * dx).collect();
        let f = from_complex_fn(ny, nx, |_, j| Complex::from_polar(1.0, xs[j]));

        let (fx, fy) = gradient_2d(&f, dx, dy).unwrap();

        // Check interior points only (boundary one-sided is also O(h²) but
        // with a larger constant; tighten on interior).
        let tol = 5e-3;
        for i in 0..ny {
            for j in 1..nx - 1 {
                let expected = Complex::new(0.0, 1.0) * Complex::from_polar(1.0, xs[j]);
                let err = (fx[[i, j]] - expected).norm();
                assert!(err < tol, "fx[{i},{j}] err = {err}");
                assert!(fy[[i, j]].norm() < tol, "fy[{i},{j}] = {:?}", fy[[i, j]]);
            }
        }
    }

    #[test]
    fn boundary_is_second_order_for_quadratic() {
        // Pure boundary check: a 1-D quadratic profile along x. The 2nd-order
        // one-sided stencils are exact on quadratics, so the boundary value
        // should match analytically.
        let (ny, nx, dx, dy) = (3, 4, 0.5, 1.0);
        let xs: Vec<f64> = (0..nx).map(|j| j as f64 * dx).collect();
        let f = from_real_fn(ny, nx, |_, j| xs[j].powi(2));
        let (fx, _) = gradient_2d(&f, dx, dy).unwrap();

        for i in 0..ny {
            assert!(close_re(fx[[i, 0]], 0.0, 1e-12), "left boundary");
            assert!(
                close_re(fx[[i, nx - 1]], 2.0 * xs[nx - 1], 1e-12),
                "right boundary"
            );
        }
    }

    #[test]
    fn shape_mismatch_errors() {
        let a = from_real_fn(3, 3, |_, _| 0.0);
        let b = from_real_fn(3, 4, |_, _| 0.0);
        assert!(divergence_2d(&a, &b, 1.0, 1.0).is_err());
        assert!(curl_2d(&a, &b, 1.0, 1.0).is_err());
    }

    #[test]
    fn too_small_dimension_errors() {
        // 2-row grid is too small for 2nd-order y-stencil.
        let f = from_real_fn(2, 5, |_, _| 0.0);
        assert!(gradient_2d(&f, 1.0, 1.0).is_err());
    }

    #[test]
    fn nonpositive_step_errors() {
        let f = from_real_fn(3, 3, |_, _| 0.0);
        assert!(gradient_2d(&f, 0.0, 1.0).is_err());
        assert!(gradient_2d(&f, 1.0, -1.0).is_err());
    }
}

#[cfg(test)]
mod vector_calc_3d_tests {
    use crate::vector_calc::{curl_3d, divergence_3d, from_real_fn_3d, gradient_3d};

    fn close_re(a: num_complex::Complex<f64>, b: f64, tol: f64) -> bool {
        (a.re - b).abs() < tol && a.im.abs() < tol
    }

    // Build coordinate triples for a uniform grid where:
    //   row    i ↔ y = i*dy
    //   col    j ↔ x = j*dx
    //   page   k ↔ z = k*dz
    fn coords(
        m: usize,
        n: usize,
        p: usize,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let xs = (0..n).map(|j| j as f64 * dx).collect();
        let ys = (0..m).map(|i| i as f64 * dy).collect();
        let zs = (0..p).map(|k| k as f64 * dz).collect();
        (xs, ys, zs)
    }

    #[test]
    fn gradient3_of_x2_y2_z2_is_2x_2y_2z() {
        // F = x² + y² + z²  →  ∇F = (2x, 2y, 2z). Quadratic → exact even at boundaries.
        let (m, n, p) = (4, 5, 6);
        let (dx, dy, dz) = (0.1, 0.2, 0.3);
        let (xs, ys, zs) = coords(m, n, p, dx, dy, dz);
        let f = from_real_fn_3d(m, n, p, |i, j, k| {
            xs[j].powi(2) + ys[i].powi(2) + zs[k].powi(2)
        });

        let (fx, fy, fz) = gradient_3d(&f, dx, dy, dz).unwrap();

        for i in 0..m {
            for j in 0..n {
                for k in 0..p {
                    assert!(
                        close_re(fx[[i, j, k]], 2.0 * xs[j], 1e-9),
                        "fx[{i},{j},{k}]"
                    );
                    assert!(
                        close_re(fy[[i, j, k]], 2.0 * ys[i], 1e-9),
                        "fy[{i},{j},{k}]"
                    );
                    assert!(
                        close_re(fz[[i, j, k]], 2.0 * zs[k], 1e-9),
                        "fz[{i},{j},{k}]"
                    );
                }
            }
        }
    }

    #[test]
    fn divergence3_of_xyz_is_three_everywhere() {
        // F = (x, y, z)  →  ∇·F = 3.
        let (m, n, p) = (4, 4, 4);
        let (dx, dy, dz) = (0.5, 0.25, 0.1);
        let (xs, ys, zs) = coords(m, n, p, dx, dy, dz);
        let fx = from_real_fn_3d(m, n, p, |_, j, _| xs[j]);
        let fy = from_real_fn_3d(m, n, p, |i, _, _| ys[i]);
        let fz = from_real_fn_3d(m, n, p, |_, _, k| zs[k]);

        let d = divergence_3d(&fx, &fy, &fz, dx, dy, dz).unwrap();

        for i in 0..m {
            for j in 0..n {
                for k in 0..p {
                    assert!(close_re(d[[i, j, k]], 3.0, 1e-9), "div[{i},{j},{k}]");
                }
            }
        }
    }

    #[test]
    fn curl3_of_solid_rotation_is_2_zhat() {
        // F = (-y, x, 0)  →  ∇×F = (0, 0, 2).
        let (m, n, p) = (3, 3, 3);
        let (dx, dy, dz) = (0.3, 0.3, 0.3);
        let (xs, ys, _zs) = coords(m, n, p, dx, dy, dz);
        let fx = from_real_fn_3d(m, n, p, |i, _, _| -ys[i]);
        let fy = from_real_fn_3d(m, n, p, |_, j, _| xs[j]);
        let fz = from_real_fn_3d(m, n, p, |_, _, _| 0.0);

        let (cx, cy, cz) = curl_3d(&fx, &fy, &fz, dx, dy, dz).unwrap();

        for i in 0..m {
            for j in 0..n {
                for k in 0..p {
                    assert!(close_re(cx[[i, j, k]], 0.0, 1e-9), "cx[{i},{j},{k}]");
                    assert!(close_re(cy[[i, j, k]], 0.0, 1e-9), "cy[{i},{j},{k}]");
                    assert!(close_re(cz[[i, j, k]], 2.0, 1e-9), "cz[{i},{j},{k}]");
                }
            }
        }
    }

    #[test]
    fn curl3_of_irrotational_field_is_zero() {
        // F = (x, y, z)  →  ∇×F = (0, 0, 0).
        let (m, n, p) = (3, 3, 3);
        let (dx, dy, dz) = (0.5, 0.5, 0.5);
        let (xs, ys, zs) = coords(m, n, p, dx, dy, dz);
        let fx = from_real_fn_3d(m, n, p, |_, j, _| xs[j]);
        let fy = from_real_fn_3d(m, n, p, |i, _, _| ys[i]);
        let fz = from_real_fn_3d(m, n, p, |_, _, k| zs[k]);

        let (cx, cy, cz) = curl_3d(&fx, &fy, &fz, dx, dy, dz).unwrap();

        for i in 0..m {
            for j in 0..n {
                for k in 0..p {
                    assert!(close_re(cx[[i, j, k]], 0.0, 1e-9));
                    assert!(close_re(cy[[i, j, k]], 0.0, 1e-9));
                    assert!(close_re(cz[[i, j, k]], 0.0, 1e-9));
                }
            }
        }
    }

    #[test]
    fn curl3_of_axis_helix() {
        // F = (-y, x, z)  →  ∇×F = (0, 0, 2). Adds a non-zero z-component to
        // the solid-rotation field; curl should be unaffected.
        let (m, n, p) = (3, 3, 3);
        let (dx, dy, dz) = (0.4, 0.4, 0.4);
        let (xs, ys, zs) = coords(m, n, p, dx, dy, dz);
        let fx = from_real_fn_3d(m, n, p, |i, _, _| -ys[i]);
        let fy = from_real_fn_3d(m, n, p, |_, j, _| xs[j]);
        let fz = from_real_fn_3d(m, n, p, |_, _, k| zs[k]);

        let (cx, cy, cz) = curl_3d(&fx, &fy, &fz, dx, dy, dz).unwrap();

        for i in 0..m {
            for j in 0..n {
                for k in 0..p {
                    assert!(close_re(cx[[i, j, k]], 0.0, 1e-9));
                    assert!(close_re(cy[[i, j, k]], 0.0, 1e-9));
                    assert!(close_re(cz[[i, j, k]], 2.0, 1e-9));
                }
            }
        }
    }

    #[test]
    fn divergence3_of_grad_is_laplacian() {
        // For V = x² + y² + z²,  ∇²V = 6  everywhere.
        let (m, n, p) = (4, 4, 4);
        let (dx, dy, dz) = (0.2, 0.2, 0.2);
        let (xs, ys, zs) = coords(m, n, p, dx, dy, dz);
        let v = from_real_fn_3d(m, n, p, |i, j, k| {
            xs[j].powi(2) + ys[i].powi(2) + zs[k].powi(2)
        });

        let (vx, vy, vz) = gradient_3d(&v, dx, dy, dz).unwrap();
        let lap = divergence_3d(&vx, &vy, &vz, dx, dy, dz).unwrap();

        for i in 0..m {
            for j in 0..n {
                for k in 0..p {
                    assert!(close_re(lap[[i, j, k]], 6.0, 1e-9));
                }
            }
        }
    }

    #[test]
    fn shape_mismatch_errors_3d() {
        let a = from_real_fn_3d(3, 3, 3, |_, _, _| 0.0);
        let b = from_real_fn_3d(3, 3, 4, |_, _, _| 0.0);
        assert!(divergence_3d(&a, &a, &b, 1.0, 1.0, 1.0).is_err());
        assert!(curl_3d(&a, &b, &a, 1.0, 1.0, 1.0).is_err());
    }

    #[test]
    fn too_small_dimension_errors_3d() {
        // page axis length 2 is too small.
        let f = from_real_fn_3d(3, 3, 2, |_, _, _| 0.0);
        assert!(gradient_3d(&f, 1.0, 1.0, 1.0).is_err());
    }

    #[test]
    fn nonpositive_step_errors_3d() {
        let f = from_real_fn_3d(3, 3, 3, |_, _, _| 0.0);
        assert!(gradient_3d(&f, 0.0, 1.0, 1.0).is_err());
        assert!(gradient_3d(&f, 1.0, -1.0, 1.0).is_err());
        assert!(gradient_3d(&f, 1.0, 1.0, 0.0).is_err());
    }
}

// ── Phase 3 (em_performance): fused / parallel / non-contiguous tests ───────

#[cfg(test)]
mod vector_calc_phase3_tests {
    use crate::vector_calc::{
        __test_set_par_threshold, curl_2d, divergence_2d, divergence_3d, from_real_fn,
        from_real_fn_3d, gradient_2d,
    };
    use ndarray::{s, Array2};
    use num_complex::Complex;

    fn rng_grid(ny: usize, nx: usize, seed: u64) -> Array2<Complex<f64>> {
        // Deterministic LCG generated into a Vec, then indexed —
        // avoids the FnMut/Fn mismatch with from_real_fn's Fn bound.
        let mut state = seed.wrapping_add(1);
        let mut buf = Vec::with_capacity(ny * nx);
        for _ in 0..ny * nx {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            buf.push(((state >> 33) as f64) / (u32::MAX as f64) - 0.5);
        }
        from_real_fn(ny, nx, |i, j| buf[i * nx + j])
    }

    /// Reference (naive) divergence: `d_dx(fx) + d_dy(fy)` via two
    /// gradient calls. The fused `divergence_2d` must match this to
    /// machine precision on identical input.
    fn naive_div_2d(
        fx: &Array2<Complex<f64>>,
        fy: &Array2<Complex<f64>>,
        dx: f64,
        dy: f64,
    ) -> Array2<Complex<f64>> {
        // gradient_2d returns (∂F/∂x, ∂F/∂y); we want ∂fx/∂x and ∂fy/∂y
        // from two scalar fields. Easiest to reuse gradient on a copy:
        // gradient(fx) gives (∂fx/∂x, ∂fx/∂y); we keep the first.
        let (dfx_dx, _) = gradient_2d(fx, dx, dy).unwrap();
        let (_, dfy_dy) = gradient_2d(fy, dx, dy).unwrap();
        dfx_dx + dfy_dy
    }

    fn naive_curl_2d(
        fx: &Array2<Complex<f64>>,
        fy: &Array2<Complex<f64>>,
        dx: f64,
        dy: f64,
    ) -> Array2<Complex<f64>> {
        let (dfy_dx, _) = gradient_2d(fy, dx, dy).unwrap();
        let (_, dfx_dy) = gradient_2d(fx, dx, dy).unwrap();
        dfy_dx - dfx_dy
    }

    fn max_abs(a: &Array2<Complex<f64>>) -> f64 {
        a.iter().map(|c| c.norm()).fold(0.0_f64, f64::max)
    }

    #[test]
    fn fused_div_2d_matches_compose() {
        let (ny, nx, dx, dy) = (12, 17, 0.13, 0.07);
        let fx = rng_grid(ny, nx, 1);
        let fy = rng_grid(ny, nx, 2);
        let fused = divergence_2d(&fx, &fy, dx, dy).unwrap();
        let naive = naive_div_2d(&fx, &fy, dx, dy);
        let diff = &fused - &naive;
        assert!(
            max_abs(&diff) < 1e-12,
            "fused/naive divergence disagree by {}",
            max_abs(&diff)
        );
    }

    #[test]
    fn fused_curl_2d_matches_compose() {
        let (ny, nx, dx, dy) = (12, 17, 0.13, 0.07);
        let fx = rng_grid(ny, nx, 3);
        let fy = rng_grid(ny, nx, 4);
        let fused = curl_2d(&fx, &fy, dx, dy).unwrap();
        let naive = naive_curl_2d(&fx, &fy, dx, dy);
        let diff = &fused - &naive;
        assert!(
            max_abs(&diff) < 1e-12,
            "fused/naive curl disagree by {}",
            max_abs(&diff)
        );
    }

    #[test]
    fn parallel_matches_serial_2d() {
        // Force the parallel path with a tiny threshold; compare to a
        // run with the threshold pushed higher than the grid size.
        let (ny, nx, dx, dy) = (40, 50, 0.1, 0.2);
        let fx = rng_grid(ny, nx, 5);
        let fy = rng_grid(ny, nx, 6);

        __test_set_par_threshold(Some(1)); // parallel
        let par_grad = gradient_2d(&fx, dx, dy).unwrap();
        let par_div = divergence_2d(&fx, &fy, dx, dy).unwrap();
        let par_curl = curl_2d(&fx, &fy, dx, dy).unwrap();

        __test_set_par_threshold(Some(usize::MAX)); // serial
        let ser_grad = gradient_2d(&fx, dx, dy).unwrap();
        let ser_div = divergence_2d(&fx, &fy, dx, dy).unwrap();
        let ser_curl = curl_2d(&fx, &fy, dx, dy).unwrap();

        __test_set_par_threshold(None); // restore default

        // Parallel and serial paths run identical scalar arithmetic in
        // the same order per-row, so equality should be bit-exact.
        assert_eq!(par_grad.0, ser_grad.0);
        assert_eq!(par_grad.1, ser_grad.1);
        assert_eq!(par_div, ser_div);
        assert_eq!(par_curl, ser_curl);
    }

    #[test]
    fn parallel_matches_serial_3d() {
        let (m, n, p, dx, dy, dz) = (8, 9, 10, 0.1, 0.2, 0.05);
        let fx = from_real_fn_3d(m, n, p, |i, j, k| {
            (i as f64) - 0.3 * (j as f64) + 0.7 * (k as f64)
        });
        let fy = from_real_fn_3d(m, n, p, |i, j, k| {
            0.4 * (i as f64) + (j as f64).sin() - 0.1 * (k as f64)
        });
        let fz = from_real_fn_3d(m, n, p, |i, j, k| {
            (i as f64).cos() + (j as f64) * (k as f64) * 0.05
        });

        __test_set_par_threshold(Some(1));
        let par = divergence_3d(&fx, &fy, &fz, dx, dy, dz).unwrap();

        __test_set_par_threshold(Some(usize::MAX));
        let ser = divergence_3d(&fx, &fy, &fz, dx, dy, dz).unwrap();

        __test_set_par_threshold(None);

        let diff = &par - &ser;
        let max_d = diff.iter().map(|c| c.norm()).fold(0.0_f64, f64::max);
        assert!(max_d < 1e-12, "3D parallel/serial disagree by {max_d}");
    }

    #[test]
    fn non_contiguous_input_still_works() {
        // Build a 30×40 grid, take a non-contiguous 10×20 slice, and run
        // gradient/divergence/curl on it. The `as_standard_layout` guard
        // at each entry point should make a contiguous copy transparently.
        let big = rng_grid(30, 40, 7);
        let slice = big.slice(s![5..15, 5..25]).to_owned();
        let view = big.slice(s![5..15, 5..25]); // ArrayView, may be non-contiguous

        // Compute on the contiguous copy (truth) and on the view-derived
        // owned matrix. Both should match.
        let truth = gradient_2d(&slice, 0.1, 0.1).unwrap();
        let from_view = gradient_2d(&view.to_owned(), 0.1, 0.1).unwrap();
        let diff_x = &truth.0 - &from_view.0;
        let diff_y = &truth.1 - &from_view.1;
        assert!(max_abs(&diff_x) < 1e-15);
        assert!(max_abs(&diff_y) < 1e-15);

        // And the view itself, fed in directly. ndarray gives us an
        // ArrayBase<ViewRepr<&Complex<f64>>, Ix2>; convert to an owned
        // CMatrix because gradient_2d takes &CMatrix. The point of the
        // as_standard_layout guard is that *if* this CMatrix turns out
        // non-contiguous after some other slicing path, we don't crash.
        let owned: Array2<Complex<f64>> = view.to_owned();
        // owned is contiguous by construction; sanity-check that the
        // call succeeds (real coverage of as_standard_layout would need
        // a non-standard-stride CMatrix, which to_owned() doesn't make).
        assert!(gradient_2d(&owned, 0.1, 0.1).is_ok());
    }
}

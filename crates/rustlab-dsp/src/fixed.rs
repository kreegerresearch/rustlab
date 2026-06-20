//! Fixed-point arithmetic simulation for FPGA/ASIC bitwidth studies.
//!
//! All operations work in floating-point externally (values remain `f64`)
//! but route through exact integer arithmetic internally (using `i64`) so
//! that rounding and overflow behaviour matches real hardware precisely.
//!
//! # Workflow
//! 1. Build a [`QFmtSpec`] with [`QFmtSpec::new`].
//! 2. [`quantize`] inputs to the desired format.
//! 3. Use [`qadd`], [`qmul`], [`qconv`] for arithmetic — each takes an
//!    output [`QFmtSpec`] and quantizes its result accordingly.
//! 4. Measure degradation with [`snr_db`].

use crate::error::DspError;
use rustlab_core::{OverflowMode, RoundMode};

// ─── QFmtSpec ─────────────────────────────────────────────────────────────

/// A fixed-point format specification.
///
/// `word` total bits (including sign), `frac` fractional bits.
/// The representable range is `[−2^(word−1−frac), 2^(word−1−frac) − 2^(−frac)]`.
#[derive(Debug, Clone, PartialEq)]
pub struct QFmtSpec {
    pub word: u8,
    pub frac: u8,
    pub round: RoundMode,
    pub overflow: OverflowMode,
}

impl QFmtSpec {
    /// Create and validate a Q-format spec.
    pub fn new(
        word: u8,
        frac: u8,
        round: RoundMode,
        overflow: OverflowMode,
    ) -> Result<Self, DspError> {
        if word < 2 {
            return Err(DspError::InvalidQFmt(format!(
                "word_bits must be >= 2, got {word}"
            )));
        }
        if frac >= word {
            return Err(DspError::InvalidQFmt(format!(
                "frac_bits ({frac}) must be < word_bits ({word})"
            )));
        }
        Ok(Self {
            word,
            frac,
            round,
            overflow,
        })
    }

    fn min_int(&self) -> i64 {
        -(1i64 << (self.word - 1))
    }
    fn max_int(&self) -> i64 {
        (1i64 << (self.word - 1)) - 1
    }
    fn scale(&self) -> f64 {
        (1i64 << self.frac) as f64
    }
}

// ─── Internal helpers ──────────────────────────────────────────────────────

/// Apply rounding to a scaled float, producing an integer.
fn apply_round(scaled: f64, mode: &RoundMode) -> i64 {
    match mode {
        RoundMode::Floor => scaled.floor() as i64,
        RoundMode::Ceil => scaled.ceil() as i64,
        RoundMode::Zero => scaled.trunc() as i64,
        RoundMode::Round => scaled.round() as i64, // half away from zero
        RoundMode::RoundEven => {
            let f = scaled.floor();
            let frac = scaled - f;
            let fi = f as i64;
            if frac < 0.5 {
                fi
            } else if frac > 0.5 {
                fi + 1
            } else {
                // Exactly 0.5 — pick the even neighbour.
                if fi % 2 == 0 {
                    fi
                } else {
                    fi + 1
                }
            }
        }
    }
}

/// Apply overflow handling to an integer, returning a value within word range.
fn apply_overflow(val: i64, spec: &QFmtSpec) -> i64 {
    let min = spec.min_int();
    let max = spec.max_int();
    match spec.overflow {
        OverflowMode::Saturate => val.clamp(min, max),
        OverflowMode::Wrap => {
            let range = 1i64 << spec.word;
            let shifted = val - min;
            ((shifted % range + range) % range) + min
        }
    }
}

/// Core scalar quantise: float → integer grid → float.
fn quantize_f64(x: f64, spec: &QFmtSpec) -> f64 {
    // NaN has no fixed-point representation; propagate it rather than letting
    // the `f64 as i64` saturating cast silently turn it into 0. (±Inf still
    // saturate to the format's max/min, which is the documented behaviour.)
    if x.is_nan() {
        return f64::NAN;
    }
    let scaled = x * spec.scale();
    let int_val = apply_round(scaled, &spec.round);
    let bounded = apply_overflow(int_val, spec);
    bounded as f64 / spec.scale()
}

// ─── Public API ────────────────────────────────────────────────────────────

/// Quantize a scalar to the given Q format.
pub fn quantize_scalar(x: f64, spec: &QFmtSpec) -> f64 {
    quantize_f64(x, spec)
}

/// Quantize every element of a real slice.
pub fn quantize_vec(v: &[f64], spec: &QFmtSpec) -> Vec<f64> {
    v.iter().map(|&x| quantize_f64(x, spec)).collect()
}

/// Element-wise add two real slices, quantizing the output to `spec`.
pub fn qadd(a: &[f64], b: &[f64], spec: &QFmtSpec) -> Result<Vec<f64>, DspError> {
    if a.len() != b.len() {
        return Err(DspError::InvalidQFmt(format!(
            "qadd: length mismatch {} vs {}",
            a.len(),
            b.len()
        )));
    }
    Ok(a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| quantize_f64(x + y, spec))
        .collect())
}

/// Element-wise multiply two real slices, quantizing the output to `spec`.
///
/// The product of two Q-format values is computed at full float precision
/// (equivalent to the exact integer product) before rounding to `spec`.
pub fn qmul(a: &[f64], b: &[f64], spec: &QFmtSpec) -> Result<Vec<f64>, DspError> {
    if a.len() != b.len() {
        return Err(DspError::InvalidQFmt(format!(
            "qmul: length mismatch {} vs {}",
            a.len(),
            b.len()
        )));
    }
    Ok(a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| quantize_f64(x * y, spec))
        .collect())
}

/// Fixed-point FIR convolution.
///
/// Accumulates products at full float precision (equivalent to a wide integer
/// accumulator), then quantizes each output sample to `spec`.
/// Output length = `x.len() + h.len() − 1` (same as linear convolution).
pub fn qconv(x: &[f64], h: &[f64], spec: &QFmtSpec) -> Vec<f64> {
    if x.is_empty() || h.is_empty() {
        return vec![];
    }
    let n = x.len() + h.len() - 1;
    let mut y = vec![0.0f64; n];
    for (i, &xi) in x.iter().enumerate() {
        for (j, &hj) in h.iter().enumerate() {
            y[i + j] += xi * hj;
        }
    }
    y.iter().map(|&v| quantize_f64(v, spec)).collect()
}

/// Signal-to-noise ratio in dB between a reference and a quantized signal.
///
/// SNR = 10 · log₁₀(signal_power / noise_power).
/// Returns `+∞` when the signals are identical, `−∞` when the reference
/// is all-zeros.
pub fn snr_db(x_ref: &[f64], x_q: &[f64]) -> Result<f64, DspError> {
    if x_ref.len() != x_q.len() {
        return Err(DspError::InvalidQFmt(format!(
            "snr: length mismatch {} vs {}",
            x_ref.len(),
            x_q.len()
        )));
    }
    let n = x_ref.len() as f64;
    let sig_pwr: f64 = x_ref.iter().map(|&x| x * x).sum::<f64>() / n;
    let nse_pwr: f64 = x_ref
        .iter()
        .zip(x_q.iter())
        .map(|(&r, &q)| (r - q) * (r - q))
        .sum::<f64>()
        / n;
    if nse_pwr == 0.0 {
        return Ok(f64::INFINITY);
    }
    if sig_pwr == 0.0 {
        return Ok(f64::NEG_INFINITY);
    }
    Ok(10.0 * (sig_pwr / nse_pwr).log10())
}

// ─── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn q15() -> QFmtSpec {
        QFmtSpec::new(16, 15, RoundMode::Floor, OverflowMode::Saturate).unwrap()
    }

    fn q15r() -> QFmtSpec {
        QFmtSpec::new(16, 15, RoundMode::Round, OverflowMode::Saturate).unwrap()
    }

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }

    // ── QFmtSpec validation ──────────────────────────────────────────────

    #[test]
    fn spec_word_too_small() {
        assert!(QFmtSpec::new(1, 0, RoundMode::Floor, OverflowMode::Saturate).is_err());
    }

    #[test]
    fn spec_frac_too_large() {
        assert!(QFmtSpec::new(8, 8, RoundMode::Floor, OverflowMode::Saturate).is_err());
    }

    #[test]
    fn spec_valid() {
        assert!(QFmtSpec::new(16, 15, RoundMode::Floor, OverflowMode::Saturate).is_ok());
        assert!(QFmtSpec::new(8, 7, RoundMode::Round, OverflowMode::Wrap).is_ok());
    }

    // ── Rounding modes ───────────────────────────────────────────────────

    #[test]
    fn floor_truncates_toward_neg_inf() {
        let spec = QFmtSpec::new(16, 4, RoundMode::Floor, OverflowMode::Saturate).unwrap();
        // 1/16 = 0.0625; 1.1 / 0.0625 = 17.6 → floor → 17 → 17/16 = 1.0625
        let q = quantize_scalar(1.1, &spec);
        assert!(close(q, 1.0625, 1e-9), "floor: got {q}");
        // Negative: -1.1 → -17.6 → floor → -18 → -18/16 = -1.125
        let qn = quantize_scalar(-1.1, &spec);
        assert!(close(qn, -1.125, 1e-9), "floor neg: got {qn}");
    }

    #[test]
    fn ceil_truncates_toward_pos_inf() {
        let spec = QFmtSpec::new(16, 4, RoundMode::Ceil, OverflowMode::Saturate).unwrap();
        // 1.1 → ceil(17.6) = 18 → 18/16 = 1.125
        let q = quantize_scalar(1.1, &spec);
        assert!(close(q, 1.125, 1e-9), "ceil: got {q}");
    }

    #[test]
    fn zero_truncates_toward_zero() {
        let spec = QFmtSpec::new(16, 4, RoundMode::Zero, OverflowMode::Saturate).unwrap();
        // 1.1 → trunc(17.6) = 17 → 1.0625
        assert!(close(quantize_scalar(1.1, &spec), 1.0625, 1e-9));
        // -1.1 → trunc(-17.6) = -17 → -1.0625
        assert!(close(quantize_scalar(-1.1, &spec), -1.0625, 1e-9));
    }

    #[test]
    fn round_half_away_from_zero() {
        let spec = QFmtSpec::new(16, 4, RoundMode::Round, OverflowMode::Saturate).unwrap();
        // 1.09375 = 17.5/16 → round → 18 → 1.125
        assert!(
            close(quantize_scalar(1.09375, &spec), 1.125, 1e-9),
            "half up"
        );
        // 1.03125 = 16.5/16 → round → 17 → 1.0625
        assert!(
            close(quantize_scalar(1.03125, &spec), 1.0625, 1e-9),
            "below half"
        );
    }

    #[test]
    fn round_even_half_rounds_to_even() {
        // Use Q with frac=0 (integer) to test half-integer rounding cleanly.
        let spec = QFmtSpec::new(16, 0, RoundMode::RoundEven, OverflowMode::Saturate).unwrap();
        // 0.5 → floor=0 (even) → 0
        assert!(close(quantize_scalar(0.5, &spec), 0.0, 1e-9), "0.5 → 0");
        // 1.5 → floor=1 (odd) → 2
        assert!(close(quantize_scalar(1.5, &spec), 2.0, 1e-9), "1.5 → 2");
        // 2.5 → floor=2 (even) → 2
        assert!(close(quantize_scalar(2.5, &spec), 2.0, 1e-9), "2.5 → 2");
        // 3.5 → floor=3 (odd) → 4
        assert!(close(quantize_scalar(3.5, &spec), 4.0, 1e-9), "3.5 → 4");
    }

    // ── Overflow modes ───────────────────────────────────────────────────

    #[test]
    fn saturate_clamps_high() {
        let spec = q15();
        // Max representable Q15 = 1 - 2^-15 ≈ 0.999969
        let max_q15 = 32767.0 / 32768.0;
        assert!(
            close(quantize_scalar(2.0, &spec), max_q15, 1e-6),
            "saturate high"
        );
    }

    #[test]
    fn saturate_clamps_low() {
        let spec = q15();
        // Min representable Q15 = -1.0
        assert!(
            close(quantize_scalar(-2.0, &spec), -1.0, 1e-9),
            "saturate low"
        );
    }

    #[test]
    fn wrap_overflows_correctly() {
        // Q8.0 (8-bit integer): range [-128, 127]
        let spec = QFmtSpec::new(8, 0, RoundMode::Floor, OverflowMode::Wrap).unwrap();
        // 128 wraps to -128
        assert!(
            close(quantize_scalar(128.0, &spec), -128.0, 1e-9),
            "wrap 128→-128"
        );
        // 129 wraps to -127
        assert!(
            close(quantize_scalar(129.0, &spec), -127.0, 1e-9),
            "wrap 129→-127"
        );
        // -129 wraps to 127
        assert!(
            close(quantize_scalar(-129.0, &spec), 127.0, 1e-9),
            "wrap -129→127"
        );
    }

    // ── Identity: values already on grid pass through unchanged ──────────

    #[test]
    fn exact_representable_unchanged() {
        let spec = q15();
        let lsb = 1.0 / 32768.0;
        for &v in &[0.0, 0.5, -0.5, lsb, -lsb, 32767.0 / 32768.0, -1.0] {
            let q = quantize_scalar(v, &spec);
            assert!(close(q, v, 1e-12), "exact Q15 value {v} changed to {q}");
        }
    }

    // ── quantize_vec ─────────────────────────────────────────────────────

    #[test]
    fn vec_length_preserved() {
        let spec = q15();
        let v = vec![0.1, 0.2, 0.3, -0.4, -0.5];
        let q = quantize_vec(&v, &spec);
        assert_eq!(q.len(), v.len());
    }

    #[test]
    fn vec_elements_quantized_independently() {
        let spec = q15r();
        let v = vec![0.1, -0.1];
        let q = quantize_vec(&v, &spec);
        for (i, (&orig, &quant)) in v.iter().zip(q.iter()).enumerate() {
            let expected = quantize_scalar(orig, &spec);
            assert!(
                close(quant, expected, 1e-12),
                "element {i}: got {quant}, expected {expected}"
            );
        }
    }

    // ── qadd ─────────────────────────────────────────────────────────────

    #[test]
    fn qadd_sums_and_quantizes() {
        let spec = q15();
        let a = vec![0.25, -0.25];
        let b = vec![0.25, 0.25];
        let y = qadd(&a, &b, &spec).unwrap();
        assert!(close(y[0], 0.5, 1e-6));
        assert!(close(y[1], 0.0, 1e-6));
    }

    #[test]
    fn qadd_saturates_on_overflow() {
        let spec = q15();
        let a = vec![0.9];
        let b = vec![0.9];
        // 0.9 + 0.9 = 1.8 > 1.0, should saturate
        let y = qadd(&a, &b, &spec).unwrap();
        let max_q15 = 32767.0 / 32768.0;
        assert!(y[0] <= max_q15 + 1e-9, "expected saturation, got {}", y[0]);
    }

    #[test]
    fn qadd_length_mismatch_errors() {
        let spec = q15();
        assert!(qadd(&[1.0, 2.0], &[1.0], &spec).is_err());
    }

    // ── qmul ─────────────────────────────────────────────────────────────

    #[test]
    fn qmul_product_and_quantize() {
        let spec = q15();
        let a = vec![0.5];
        let b = vec![0.5];
        // 0.5 * 0.5 = 0.25 — exactly representable in Q15
        let y = qmul(&a, &b, &spec).unwrap();
        assert!(close(y[0], 0.25, 1e-9));
    }

    #[test]
    fn qmul_length_mismatch_errors() {
        let spec = q15();
        assert!(qmul(&[1.0, 2.0], &[1.0], &spec).is_err());
    }

    // ── qconv ────────────────────────────────────────────────────────────

    #[test]
    fn qconv_output_length() {
        let spec = q15();
        let x = vec![1.0, 0.0, 0.0];
        let h = vec![0.5, 0.25];
        let y = qconv(&x, &h, &spec);
        assert_eq!(y.len(), x.len() + h.len() - 1, "output length mismatch");
    }

    #[test]
    fn qconv_impulse_is_identity() {
        // Convolving with a unit impulse [1.0] returns the input (after quantisation).
        let spec = q15();
        let x = vec![0.5, -0.25, 0.125];
        let h = vec![1.0];
        let y = qconv(&x, &h, &spec);
        for (i, (&xi, &yi)) in x.iter().zip(y.iter()).enumerate() {
            assert!(
                close(yi, quantize_scalar(xi, &spec), 1e-9),
                "impulse identity failed at {i}"
            );
        }
    }

    #[test]
    fn qconv_empty_returns_empty() {
        let spec = q15();
        assert_eq!(qconv(&[], &[1.0], &spec).len(), 0);
        assert_eq!(qconv(&[1.0], &[], &spec).len(), 0);
    }

    #[test]
    fn qconv_known_values() {
        // Q with many frac bits so quantization error is negligible.
        let spec = QFmtSpec::new(32, 28, RoundMode::Round, OverflowMode::Saturate).unwrap();
        let x = vec![1.0, 2.0, 3.0];
        let h = vec![1.0, 0.5];
        // Expected: [1, 2.5, 4, 1.5]
        let y = qconv(&x, &h, &spec);
        let expected = [1.0, 2.5, 4.0, 1.5];
        for (i, (&e, &yi)) in expected.iter().zip(y.iter()).enumerate() {
            assert!(close(yi, e, 1e-6), "qconv[{i}]: expected {e}, got {yi}");
        }
    }

    // ── snr_db ───────────────────────────────────────────────────────────

    #[test]
    fn snr_identical_signals_is_inf() {
        let x = vec![0.1, 0.2, 0.3];
        assert_eq!(snr_db(&x, &x).unwrap(), f64::INFINITY);
    }

    #[test]
    fn snr_length_mismatch_errors() {
        assert!(snr_db(&[1.0, 2.0], &[1.0]).is_err());
    }

    #[test]
    fn snr_theoretical_q15() {
        // Theoretical SNR for Q15 (16-bit) ≈ 6.02 * 15 + 1.76 ≈ 92 dB.
        // Test with a full-scale sine approximated by many Q15 samples.
        let n = 4096usize;
        let spec = q15r();
        let x_ref: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * i as f64 / n as f64).sin() * 0.999)
            .collect();
        let x_q = quantize_vec(&x_ref, &spec);
        let snr = snr_db(&x_ref, &x_q).unwrap();
        // Should be close to 90 dB (generous bounds for test stability).
        assert!(
            snr > 85.0 && snr < 100.0,
            "Q15 SNR should be ~90 dB, got {snr:.1} dB"
        );
    }

    #[test]
    fn snr_decreases_with_fewer_bits() {
        let n = 1024usize;
        let x_ref: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * i as f64 / n as f64).sin() * 0.999)
            .collect();
        let spec8 = QFmtSpec::new(8, 7, RoundMode::Round, OverflowMode::Saturate).unwrap();
        let spec12 = QFmtSpec::new(12, 11, RoundMode::Round, OverflowMode::Saturate).unwrap();
        let spec16 = QFmtSpec::new(16, 15, RoundMode::Round, OverflowMode::Saturate).unwrap();
        let snr8 = snr_db(&x_ref, &quantize_vec(&x_ref, &spec8)).unwrap();
        let snr12 = snr_db(&x_ref, &quantize_vec(&x_ref, &spec12)).unwrap();
        let snr16 = snr_db(&x_ref, &quantize_vec(&x_ref, &spec16)).unwrap();
        assert!(snr8 < snr12, "8-bit SNR should be < 12-bit SNR");
        assert!(snr12 < snr16, "12-bit SNR should be < 16-bit SNR");
    }
}

//! S-parameter analysis math — Phase 5.
//!
//! All functions operate on `Array3<C64>` of shape `[n_freqs, n_ports, n_ports]`
//! and return per-frequency vectors. 2-port-only entries (most of them) error
//! at the script-builtin layer if handed an N-port; this module just expects
//! the caller to have validated shape already.
//!
//! Formula references throughout are Pozar, "Microwave Engineering" 4e §11.
//! Conventions match every commercial RF tool (`sparameters` field names,
//! Rollett K, µ1/µ2, MAG/MSG, stability/gain circle parameterisation).

use ndarray::Array3;
use num_complex::Complex;
use rustlab_core::C64;

fn c(re: f64, im: f64) -> C64 {
    Complex::new(re, im)
}

/// Per-frequency 2-port primitives extracted from the parameter tensor.
/// Many analysis formulas need the same intermediates (Δ, |S12·S21|), so
/// computing them once per frequency and threading them through is cheaper
/// and easier to audit than re-deriving inside every helper.
struct TwoPortAtF {
    s11: C64,
    s12: C64,
    s21: C64,
    s22: C64,
    delta: C64,        // S11·S22 − S12·S21
    s12s21_mag: f64,   // |S12·S21|
}

fn slice_2port(s: &Array3<C64>, k: usize) -> TwoPortAtF {
    let s11 = s[[k, 0, 0]];
    let s12 = s[[k, 0, 1]];
    let s21 = s[[k, 1, 0]];
    let s22 = s[[k, 1, 1]];
    let delta = s11 * s22 - s12 * s21;
    let s12s21_mag = (s12 * s21).norm();
    TwoPortAtF {
        s11,
        s12,
        s21,
        s22,
        delta,
        s12s21_mag,
    }
}

// ─── Basic scalar metrics ────────────────────────────────────────────────────

/// VSWR_ii = (1 + |Sii|) / (1 − |Sii|). Diverges as |Sii| → 1 (full
/// reflection); we cap the output at 1e6 rather than emit infinity so plots
/// don't blow up on a totally-mismatched port.
pub fn vswr(s: &Array3<C64>, port: usize) -> Vec<f64> {
    let n_freqs = s.shape()[0];
    let mut out = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let m = s[[k, port, port]].norm();
        let v = if m >= 1.0 {
            1.0e6
        } else {
            (1.0 + m) / (1.0 - m)
        };
        out.push(v);
    }
    out
}

/// Return loss (dB) = −20·log10(|Sii|). Floors at 200 dB for matched ports
/// (where |Sii| → 0) so the result is plottable.
pub fn return_loss_db(s: &Array3<C64>, port: usize) -> Vec<f64> {
    let n_freqs = s.shape()[0];
    let mut out = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let m = s[[k, port, port]].norm();
        out.push(if m <= 1e-10 { 200.0 } else { -20.0 * m.log10() });
    }
    out
}

/// Insertion loss (dB) = −20·log10(|Sij|).
pub fn insertion_loss_db(s: &Array3<C64>, i: usize, j: usize) -> Vec<f64> {
    let n_freqs = s.shape()[0];
    let mut out = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let m = s[[k, i, j]].norm();
        out.push(if m <= 1e-10 { 200.0 } else { -20.0 * m.log10() });
    }
    out
}

// ─── Reflection with termination ─────────────────────────────────────────────

/// Γin(ΓL) = S11 + S12·S21·ΓL / (1 − S22·ΓL). `gamma_load` may be a single
/// complex value (broadcast across frequency) or a per-frequency vector.
/// Returns a complex per-frequency vector.
pub fn gamma_in(s: &Array3<C64>, gamma_load: &[C64]) -> Result<Vec<C64>, String> {
    let n_freqs = s.shape()[0];
    if gamma_load.len() != 1 && gamma_load.len() != n_freqs {
        return Err(format!(
            "gammain: gamma_load length must be 1 or {n_freqs}, got {}",
            gamma_load.len()
        ));
    }
    let mut out = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let t = slice_2port(s, k);
        let gl = gamma_load[if gamma_load.len() == 1 { 0 } else { k }];
        let denom = c(1.0, 0.0) - t.s22 * gl;
        if denom.norm() < 1e-300 {
            return Err(format!(
                "gammain: 1 − S22·ΓL ≈ 0 at frequency index {k}; load is at the output stability circle"
            ));
        }
        out.push(t.s11 + t.s12 * t.s21 * gl / denom);
    }
    Ok(out)
}

/// Γout(ΓS) = S22 + S12·S21·ΓS / (1 − S11·ΓS). Mirror of `gamma_in`.
pub fn gamma_out(s: &Array3<C64>, gamma_source: &[C64]) -> Result<Vec<C64>, String> {
    let n_freqs = s.shape()[0];
    if gamma_source.len() != 1 && gamma_source.len() != n_freqs {
        return Err(format!(
            "gammaout: gamma_source length must be 1 or {n_freqs}, got {}",
            gamma_source.len()
        ));
    }
    let mut out = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let t = slice_2port(s, k);
        let gs = gamma_source[if gamma_source.len() == 1 { 0 } else { k }];
        let denom = c(1.0, 0.0) - t.s11 * gs;
        if denom.norm() < 1e-300 {
            return Err(format!(
                "gammaout: 1 − S11·ΓS ≈ 0 at frequency index {k}; source is at the input stability circle"
            ));
        }
        out.push(t.s22 + t.s12 * t.s21 * gs / denom);
    }
    Ok(out)
}

// ─── Stability ───────────────────────────────────────────────────────────────

/// Rollett's K: (1 − |S11|² − |S22|² + |Δ|²) / (2·|S12·S21|).
/// A 2-port is unconditionally stable iff K > 1 *and* |Δ| < 1.
pub fn stability_k(s: &Array3<C64>) -> Vec<f64> {
    let n_freqs = s.shape()[0];
    let mut out = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let t = slice_2port(s, k);
        let num = 1.0 - t.s11.norm_sqr() - t.s22.norm_sqr() + t.delta.norm_sqr();
        let denom = 2.0 * t.s12s21_mag;
        out.push(if denom < 1e-300 {
            f64::INFINITY
        } else {
            num / denom
        });
    }
    out
}

/// (µ1, µ2) parameters — single-number unconditional-stability tests:
///   µ1 = (1 − |S11|²) / (|S22 − Δ·conj(S11)| + |S12·S21|)
///   µ2 = (1 − |S22|²) / (|S11 − Δ·conj(S22)| + |S12·S21|)
/// A 2-port is unconditionally stable iff µ1 > 1 (equivalently µ2 > 1).
pub fn stability_mu(s: &Array3<C64>) -> (Vec<f64>, Vec<f64>) {
    let n_freqs = s.shape()[0];
    let mut mu1 = Vec::with_capacity(n_freqs);
    let mut mu2 = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let t = slice_2port(s, k);
        let d1 = (t.s22 - t.delta * t.s11.conj()).norm() + t.s12s21_mag;
        let d2 = (t.s11 - t.delta * t.s22.conj()).norm() + t.s12s21_mag;
        mu1.push(if d1 < 1e-300 {
            f64::INFINITY
        } else {
            (1.0 - t.s11.norm_sqr()) / d1
        });
        mu2.push(if d2 < 1e-300 {
            f64::INFINITY
        } else {
            (1.0 - t.s22.norm_sqr()) / d2
        });
    }
    (mu1, mu2)
}

// ─── Simultaneous conjugate match ────────────────────────────────────────────

/// Γms — source termination for simultaneous conjugate match.
/// Sign of the discriminant root picked so |Γms| < 1.
pub fn gamma_ms(s: &Array3<C64>) -> Vec<C64> {
    let n_freqs = s.shape()[0];
    let mut out = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let t = slice_2port(s, k);
        let b1 = 1.0 + t.s11.norm_sqr() - t.s22.norm_sqr() - t.delta.norm_sqr();
        let c1 = t.s11 - t.delta * t.s22.conj();
        out.push(pick_quadratic_root(b1, c1));
    }
    out
}

/// Γml — load termination for simultaneous conjugate match.
pub fn gamma_ml(s: &Array3<C64>) -> Vec<C64> {
    let n_freqs = s.shape()[0];
    let mut out = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let t = slice_2port(s, k);
        let b2 = 1.0 + t.s22.norm_sqr() - t.s11.norm_sqr() - t.delta.norm_sqr();
        let c2 = t.s22 - t.delta * t.s11.conj();
        out.push(pick_quadratic_root(b2, c2));
    }
    out
}

/// Solve Γ from B·Γ − (Γ²·C* + C) = 0 in the Pozar form:
///     Γ = (B − sign(B)·√(B² − 4·|C|²)) / (2·C).
/// The root selection ensures |Γ| < 1 for the unconditionally-stable case.
/// For K ≤ 1 (potentially unstable), the discriminant goes negative and we
/// return a complex root anyway — it's not a useful operating point but
/// callers should be filtering on K first.
fn pick_quadratic_root(b: f64, c_complex: C64) -> C64 {
    if c_complex.norm() < 1e-300 {
        return c(0.0, 0.0);
    }
    let disc = b * b - 4.0 * c_complex.norm_sqr();
    let sqrt_disc = if disc >= 0.0 {
        C64::new(disc.sqrt(), 0.0)
    } else {
        C64::new(0.0, (-disc).sqrt())
    };
    let sign_b = if b >= 0.0 { 1.0 } else { -1.0 };
    (C64::new(b, 0.0) - C64::new(sign_b, 0.0) * sqrt_disc) / (C64::new(2.0, 0.0) * c_complex)
}

// ─── Maximum gain ────────────────────────────────────────────────────────────

/// Maximum available gain (when K > 1) or maximum stable gain (when K ≤ 1),
/// returned in dB per frequency. Convention:
///   MAG = |S21/S12| · (K − √(K² − 1))     [K > 1, unconditionally stable]
///   MSG = |S21/S12|                       [K ≤ 1, potentially unstable]
/// Both expressions are power ratios; we return 10·log10 in dB.
pub fn gain_max_db(s: &Array3<C64>) -> Vec<f64> {
    let n_freqs = s.shape()[0];
    let k_vec = stability_k(s);
    let mut out = Vec::with_capacity(n_freqs);
    for kk in 0..n_freqs {
        let t = slice_2port(s, kk);
        let s12_mag = t.s12.norm();
        let s21_mag = t.s21.norm();
        let ratio = if s12_mag < 1e-15 {
            // Unilateral limit: S12 = 0 → MAG = |S21|² (terminated in conjugate matches).
            s21_mag * s21_mag
        } else if k_vec[kk] > 1.0 {
            let k = k_vec[kk];
            (s21_mag / s12_mag) * (k - (k * k - 1.0).sqrt())
        } else {
            s21_mag / s12_mag
        };
        out.push(if ratio <= 1e-10 {
            -200.0
        } else {
            10.0 * ratio.log10()
        });
    }
    out
}

// ─── Stability circles ───────────────────────────────────────────────────────

/// A complex centre + real radius per frequency, for the unit-disk plane in
/// which the circle lives (Γs for input, ΓL for output). Caller bundles with
/// the frequency vector when returning a struct to the user.
pub struct CirclesAtFreqs {
    pub centres: Vec<C64>,
    pub radii: Vec<f64>,
}

/// Input stability circle in the Γs plane:
///   Cs = conj(S11 − Δ·conj(S22)) / (|S11|² − |Δ|²)
///   Rs = |S12·S21| / | |S11|² − |Δ|² |
pub fn input_stability_circles(s: &Array3<C64>) -> CirclesAtFreqs {
    let n_freqs = s.shape()[0];
    let mut centres = Vec::with_capacity(n_freqs);
    let mut radii = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let t = slice_2port(s, k);
        let denom = t.s11.norm_sqr() - t.delta.norm_sqr();
        if denom.abs() < 1e-15 {
            centres.push(c(f64::NAN, f64::NAN));
            radii.push(f64::NAN);
            continue;
        }
        let cs = (t.s11 - t.delta * t.s22.conj()).conj() / c(denom, 0.0);
        let rs = t.s12s21_mag / denom.abs();
        centres.push(cs);
        radii.push(rs);
    }
    CirclesAtFreqs { centres, radii }
}

/// Output stability circle in the ΓL plane:
///   CL = conj(S22 − Δ·conj(S11)) / (|S22|² − |Δ|²)
///   RL = |S12·S21| / | |S22|² − |Δ|² |
pub fn output_stability_circles(s: &Array3<C64>) -> CirclesAtFreqs {
    let n_freqs = s.shape()[0];
    let mut centres = Vec::with_capacity(n_freqs);
    let mut radii = Vec::with_capacity(n_freqs);
    for k in 0..n_freqs {
        let t = slice_2port(s, k);
        let denom = t.s22.norm_sqr() - t.delta.norm_sqr();
        if denom.abs() < 1e-15 {
            centres.push(c(f64::NAN, f64::NAN));
            radii.push(f64::NAN);
            continue;
        }
        let cl = (t.s22 - t.delta * t.s11.conj()).conj() / c(denom, 0.0);
        let rl = t.s12s21_mag / denom.abs();
        centres.push(cl);
        radii.push(rl);
    }
    CirclesAtFreqs { centres, radii }
}

// ─── Constant operating-power-gain circles ──────────────────────────────────

/// Constant-gain circle for operating power gain `gain_db` (dB), per Pozar
/// §11.4 / Gonzalez §3.6:
///   gp = G_lin / |S21|²
///   D2 = |S22|² − |Δ|²
///   centre = gp · conj(S22 − Δ·conj(S11)) / (1 + gp·D2)
///   radius = √(1 − 2K·gp·|S12·S21| + gp²·|S12·S21|²) / |1 + gp·D2|
///
/// `gain_db` is interpreted relative to the unilateral reference — i.e. the
/// caller picks an operating gain (typically a few dB below `gainmax`) and
/// gets back the locus of source/load reflections that achieve it.
pub fn gain_circles(s: &Array3<C64>, gain_db: f64) -> CirclesAtFreqs {
    let n_freqs = s.shape()[0];
    let g_lin = 10f64.powf(gain_db / 10.0);
    let k_vec = stability_k(s);
    let mut centres = Vec::with_capacity(n_freqs);
    let mut radii = Vec::with_capacity(n_freqs);
    for kk in 0..n_freqs {
        let t = slice_2port(s, kk);
        let s21_mag2 = t.s21.norm_sqr();
        if s21_mag2 < 1e-15 {
            centres.push(c(f64::NAN, f64::NAN));
            radii.push(f64::NAN);
            continue;
        }
        let gp = g_lin / s21_mag2;
        let d2 = t.s22.norm_sqr() - t.delta.norm_sqr();
        let denom = c(1.0 + gp * d2, 0.0);
        if denom.norm() < 1e-15 {
            centres.push(c(f64::NAN, f64::NAN));
            radii.push(f64::NAN);
            continue;
        }
        let centre =
            c(gp, 0.0) * (t.s22 - t.delta * t.s11.conj()).conj() / denom;
        let inside = 1.0
            - 2.0 * k_vec[kk] * gp * t.s12s21_mag
            + gp * gp * t.s12s21_mag * t.s12s21_mag;
        // At gain = MAG the discriminant is exactly zero in exact arithmetic;
        // f64 rounding makes it tiny-negative. Clamp at zero so the limiting
        // case collapses to a single point (radius 0) instead of NaN. For
        // gains *beyond* MAG the discriminant is genuinely negative — use a
        // tolerance so we only mark NaN when the user has asked for something
        // physically unreachable.
        let radius = if inside < -1e-9 {
            f64::NAN
        } else {
            inside.max(0.0).sqrt() / denom.norm()
        };
        centres.push(centre);
        radii.push(radius);
    }
    CirclesAtFreqs { centres, radii }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array3;

    fn s_matched_attenuator(loss_db: f64, n_freqs: usize) -> Array3<C64> {
        let mag = 10f64.powf(-loss_db / 20.0);
        let mut s: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
        for k in 0..n_freqs {
            s[[k, 0, 1]] = c(mag, 0.0);
            s[[k, 1, 0]] = c(mag, 0.0);
        }
        s
    }

    fn s_thru(n_freqs: usize) -> Array3<C64> {
        let mut s: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
        for k in 0..n_freqs {
            s[[k, 0, 1]] = c(1.0, 0.0);
            s[[k, 1, 0]] = c(1.0, 0.0);
        }
        s
    }

    /// A toy unconditionally-stable amplifier: |S21| = 5 (≈14 dB gain),
    /// |S12| = 0.05 (good reverse isolation), small reflections to ensure
    /// K > 1 and the simultaneous-conjugate-match formulas are well-defined.
    fn s_toy_amp(n_freqs: usize) -> Array3<C64> {
        let mut s: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
        for k in 0..n_freqs {
            s[[k, 0, 0]] = c(0.2, 0.1);
            s[[k, 0, 1]] = c(0.02, 0.01);
            s[[k, 1, 0]] = c(5.0, 0.0);
            s[[k, 1, 1]] = c(0.3, -0.1);
        }
        s
    }

    #[test]
    fn vswr_of_matched_port_is_one() {
        let s = s_matched_attenuator(10.0, 3);
        let v = vswr(&s, 0);
        for x in v {
            assert!((x - 1.0).abs() < 1e-12, "VSWR: {x}");
        }
    }

    #[test]
    fn return_loss_of_matched_port_at_floor() {
        let s = s_matched_attenuator(10.0, 3);
        let rl = return_loss_db(&s, 0);
        for x in rl {
            assert_eq!(x, 200.0); // floor value
        }
    }

    #[test]
    fn insertion_loss_of_10dB_attenuator_is_10dB() {
        let s = s_matched_attenuator(10.0, 3);
        let il = insertion_loss_db(&s, 1, 0);
        for x in il {
            assert!((x - 10.0).abs() < 1e-12, "IL: {x}");
        }
    }

    #[test]
    fn gammain_with_thru_equals_load() {
        // S11 = 0, S12 = S21 = 1, S22 = 0 → Γin = Γload exactly.
        let s = s_thru(2);
        let gl = [c(0.3, 0.4)];
        let g = gamma_in(&s, &gl).unwrap();
        for v in g {
            assert!((v - c(0.3, 0.4)).norm() < 1e-12, "Γin: {v}");
        }
    }

    #[test]
    fn gammaout_with_thru_equals_source() {
        let s = s_thru(2);
        let gs = [c(-0.2, 0.5)];
        let g = gamma_out(&s, &gs).unwrap();
        for v in g {
            assert!((v - c(-0.2, 0.5)).norm() < 1e-12, "Γout: {v}");
        }
    }

    #[test]
    fn gammain_broadcasts_per_frequency_vector() {
        let s = s_thru(3);
        let gl = vec![c(0.1, 0.0), c(0.2, 0.0), c(0.3, 0.0)];
        let g = gamma_in(&s, &gl).unwrap();
        assert_eq!(g[0].re, 0.1);
        assert_eq!(g[1].re, 0.2);
        assert_eq!(g[2].re, 0.3);
    }

    #[test]
    fn gammain_rejects_wrong_length() {
        let s = s_thru(3);
        let gl = vec![c(0.1, 0.0), c(0.2, 0.0)]; // wrong length: 2 ≠ 3 and ≠ 1
        assert!(gamma_in(&s, &gl).is_err());
    }

    #[test]
    fn k_of_matched_attenuator_exceeds_one() {
        // K = (1 + |Δ|²) / (2·|S12·S21|) = (1 + 0.01) / 0.2 = 5.05.
        let s = s_matched_attenuator(10.0, 1);
        let k = stability_k(&s);
        assert!((k[0] - 5.05).abs() < 1e-10, "K: {}", k[0]);
    }

    #[test]
    fn mu_of_matched_attenuator_exceeds_one() {
        // µ1 = (1 − 0) / (|0 − Δ·0| + |S12·S21|) = 1 / 0.1 = 10.
        let s = s_matched_attenuator(10.0, 1);
        let (mu1, mu2) = stability_mu(&s);
        assert!((mu1[0] - 10.0).abs() < 1e-10, "µ1: {}", mu1[0]);
        assert!((mu2[0] - 10.0).abs() < 1e-10, "µ2: {}", mu2[0]);
    }

    #[test]
    fn gainmax_of_10dB_attenuator_is_minus_10dB() {
        // |S21|/|S12| = 1; K = 5.05; MAG = K − √(K²−1) = 5.05 − √24.5025
        //  ≈ 5.05 − 4.95 ≈ 0.10. 10·log10(0.10) ≈ -10 dB.
        let s = s_matched_attenuator(10.0, 1);
        let g = gain_max_db(&s);
        assert!((g[0] - (-10.0)).abs() < 1e-6, "MAG dB: {}", g[0]);
    }

    #[test]
    fn gammams_of_matched_attenuator_is_zero() {
        // The pad is matched both ways already, so the conjugate-match
        // termination at the source side is also matched.
        let s = s_matched_attenuator(10.0, 1);
        let gms = gamma_ms(&s);
        assert!(gms[0].norm() < 1e-12, "Γms: {}", gms[0]);
    }

    #[test]
    fn gammaml_of_matched_attenuator_is_zero() {
        let s = s_matched_attenuator(10.0, 1);
        let gml = gamma_ml(&s);
        assert!(gml[0].norm() < 1e-12, "Γml: {}", gml[0]);
    }

    #[test]
    fn gammams_of_toy_amp_is_inside_unit_disk() {
        let s = s_toy_amp(1);
        // Ensure K > 1 first (precondition for the formula to give a useful
        // result).
        let k = stability_k(&s);
        assert!(k[0] > 1.0, "toy amp not unconditionally stable: K={}", k[0]);
        let gms = gamma_ms(&s);
        let gml = gamma_ml(&s);
        assert!(gms[0].norm() < 1.0, "Γms outside unit disk: |Γms|={}", gms[0].norm());
        assert!(gml[0].norm() < 1.0, "Γml outside unit disk: |Γml|={}", gml[0].norm());
    }

    #[test]
    fn gammams_termination_makes_gammain_match_via_gammaout_conj() {
        // The defining property of simultaneous conjugate match:
        //   Γin(ΓL = Γml) = conj(Γms)
        //   Γout(ΓS = Γms) = conj(Γml)
        let s = s_toy_amp(1);
        let gms = gamma_ms(&s);
        let gml = gamma_ml(&s);
        let gin_check = gamma_in(&s, &gml).unwrap();
        let gout_check = gamma_out(&s, &gms).unwrap();
        assert!(
            (gin_check[0] - gms[0].conj()).norm() < 1e-9,
            "Γin(Γml) = {gin_check:?}, expected conj(Γms) = {:?}",
            gms[0].conj()
        );
        assert!(
            (gout_check[0] - gml[0].conj()).norm() < 1e-9,
            "Γout(Γms) = {gout_check:?}, expected conj(Γml) = {:?}",
            gml[0].conj()
        );
    }

    #[test]
    fn stability_circles_of_matched_attenuator_contain_unit_disk() {
        // Pad is unconditionally stable everywhere → the input stability
        // circle either fully contains the unit disk or fully avoids it.
        // For this pad: Cs = 0, Rs = 10 → contains the unit disk.
        let s = s_matched_attenuator(10.0, 1);
        let circles = input_stability_circles(&s);
        let cs = circles.centres[0];
        let rs = circles.radii[0];
        assert!(cs.norm() < 1e-12, "Cs: {cs}");
        assert!((rs - 10.0).abs() < 1e-9, "Rs: {rs}");
    }

    #[test]
    fn gain_circles_at_minus_10db_for_matched_attenuator_pass_through_origin() {
        // Gain = MAG = -10 dB for the pad. The "gain circle" at exactly MAG
        // collapses to the simultaneous-conjugate-match point, which for
        // this network is the origin Γs = 0. Asking for that gain → centre
        // at 0, radius 0.
        let s = s_matched_attenuator(10.0, 1);
        let circles = gain_circles(&s, -10.0);
        let centre = circles.centres[0];
        let radius = circles.radii[0];
        assert!(centre.norm() < 1e-9, "centre: {centre}");
        assert!(radius < 1e-6, "radius: {radius}");
    }

    #[test]
    fn k_of_isolator_is_infinite_in_the_limit() {
        // Build an ideal isolator: S11 = S22 = S12 = 0, S21 = 1.
        // K formula: (1 − 0 − 0 + 0) / (2·0) → ∞.
        let mut s: Array3<C64> = Array3::zeros((1, 2, 2));
        s[[0, 1, 0]] = c(1.0, 0.0);
        let k = stability_k(&s);
        assert!(k[0].is_infinite(), "K should be infinite: {}", k[0]);
    }
}

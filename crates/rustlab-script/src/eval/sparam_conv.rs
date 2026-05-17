//! Network-parameter conversions for the RF S-parameter toolbox (Phase 2).
//!
//! All conversions operate on `Array3<C64>` of shape `[n_freqs, n_ports,
//! n_ports]` paired with a scalar real reference impedance `Z0` (Ohms).
//! Per-frequency NxN inversion goes through `matrix_inv` (Gauss-Jordan,
//! `eval::builtins::matrix_inv`) so all linear algebra stays in pure Rust
//! per workflow rule 10.
//!
//! Formulas (Pozar, Microwave Engineering, 4e §4.5 — normalized for scalar Z0):
//!
//!   S → Z :  Z_k = Z0 · (I + S_k) · (I − S_k)⁻¹
//!   Z → S :  S_k = (Z_k − Z0·I) · (Z_k + Z0·I)⁻¹
//!   S → Y :  Y_k = (1/Z0) · (I − S_k) · (I + S_k)⁻¹
//!   Y → S :  S_k = (I − Z0·Y_k) · (I + Z0·Y_k)⁻¹
//!
//! Closed-form 2-port mappings for T and ABCD (see Pozar §4.4 Table 4.2 and
//! §4.5 Eqs. 4.45) appear below — those are 2-port only and error otherwise.

use crate::eval::builtins::matrix_inv;
use ndarray::{Array2, Array3};
use num_complex::Complex;
use rustlab_core::C64;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn c(re: f64, im: f64) -> C64 {
    Complex::new(re, im)
}

fn identity(n: usize) -> Array2<C64> {
    let mut m: Array2<C64> = Array2::zeros((n, n));
    for i in 0..n {
        m[[i, i]] = c(1.0, 0.0);
    }
    m
}

fn slab(a3: &Array3<C64>, k: usize) -> Array2<C64> {
    let n = a3.shape()[1];
    let mut out: Array2<C64> = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            out[[i, j]] = a3[[k, i, j]];
        }
    }
    out
}

fn write_slab(a3: &mut Array3<C64>, k: usize, m: &Array2<C64>) {
    let n = m.nrows();
    for i in 0..n {
        for j in 0..n {
            a3[[k, i, j]] = m[[i, j]];
        }
    }
}

fn mat_add(a: &Array2<C64>, b: &Array2<C64>) -> Array2<C64> {
    a + b
}
fn mat_sub(a: &Array2<C64>, b: &Array2<C64>) -> Array2<C64> {
    a - b
}
fn mat_scale(m: &Array2<C64>, k: f64) -> Array2<C64> {
    m.mapv(|x| x * k)
}
fn mat_mul(a: &Array2<C64>, b: &Array2<C64>) -> Array2<C64> {
    a.dot(b)
}

fn require_2port(name: &str, params: &Array3<C64>) -> Result<(), String> {
    if params.shape()[1] != 2 || params.shape()[2] != 2 {
        return Err(format!(
            "{name}: requires a 2-port network (got {}-port)",
            params.shape()[1]
        ));
    }
    Ok(())
}

// ─── Public per-frequency conversion shells ──────────────────────────────────

pub fn s_to_z(s: &Array3<C64>, z0: f64) -> Result<Array3<C64>, String> {
    let (n_freqs, n_ports, _) = s.dim();
    let i_mat = identity(n_ports);
    let mut z: Array3<C64> = Array3::zeros((n_freqs, n_ports, n_ports));
    for k in 0..n_freqs {
        let sk = slab(s, k);
        let i_plus_s = mat_add(&i_mat, &sk);
        let i_minus_s = mat_sub(&i_mat, &sk);
        let inv = matrix_inv(&i_minus_s).map_err(|e| format!("s2z: {e}"))?;
        let zk = mat_scale(&mat_mul(&i_plus_s, &inv), z0);
        write_slab(&mut z, k, &zk);
    }
    Ok(z)
}

pub fn z_to_s(z: &Array3<C64>, z0: f64) -> Result<Array3<C64>, String> {
    let (n_freqs, n_ports, _) = z.dim();
    let i_mat = identity(n_ports);
    let z0_mat = mat_scale(&i_mat, z0);
    let mut s: Array3<C64> = Array3::zeros((n_freqs, n_ports, n_ports));
    for k in 0..n_freqs {
        let zk = slab(z, k);
        let lhs = mat_sub(&zk, &z0_mat);
        let rhs = mat_add(&zk, &z0_mat);
        let inv = matrix_inv(&rhs).map_err(|e| format!("z2s: {e}"))?;
        let sk = mat_mul(&lhs, &inv);
        write_slab(&mut s, k, &sk);
    }
    Ok(s)
}

pub fn s_to_y(s: &Array3<C64>, z0: f64) -> Result<Array3<C64>, String> {
    let (n_freqs, n_ports, _) = s.dim();
    let i_mat = identity(n_ports);
    let y0 = 1.0 / z0;
    let mut y: Array3<C64> = Array3::zeros((n_freqs, n_ports, n_ports));
    for k in 0..n_freqs {
        let sk = slab(s, k);
        let i_plus_s = mat_add(&i_mat, &sk);
        let i_minus_s = mat_sub(&i_mat, &sk);
        let inv = matrix_inv(&i_plus_s).map_err(|e| format!("s2y: {e}"))?;
        let yk = mat_scale(&mat_mul(&i_minus_s, &inv), y0);
        write_slab(&mut y, k, &yk);
    }
    Ok(y)
}

pub fn y_to_s(y: &Array3<C64>, z0: f64) -> Result<Array3<C64>, String> {
    let (n_freqs, n_ports, _) = y.dim();
    let i_mat = identity(n_ports);
    let mut s: Array3<C64> = Array3::zeros((n_freqs, n_ports, n_ports));
    for k in 0..n_freqs {
        let yk = slab(y, k);
        let z0y = mat_scale(&yk, z0);
        let lhs = mat_sub(&i_mat, &z0y);
        let rhs = mat_add(&i_mat, &z0y);
        let inv = matrix_inv(&rhs).map_err(|e| format!("y2s: {e}"))?;
        let sk = mat_mul(&lhs, &inv);
        write_slab(&mut s, k, &sk);
    }
    Ok(s)
}

// ─── 2-port T-parameters (cascade form) ──────────────────────────────────────
//
// Convention (Pozar §4.4): [a1; b1] = T · [b2; a2]. With this,
//   T = (1/S21) * [ -det(S)   S11 ]
//                 [  -S22       1 ]
// Inverse:
//   S11 = T12/T22,  S12 = det(T)/T22,  S21 = 1/T22,  S22 = -T21/T22.
//
// Cascade of two 2-ports (port 2 of A → port 1 of B): T_AB = T_A · T_B.

pub fn s_to_t(s: &Array3<C64>) -> Result<Array3<C64>, String> {
    require_2port("s2t", s)?;
    let n_freqs = s.shape()[0];
    let mut t: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
    for k in 0..n_freqs {
        let s11 = s[[k, 0, 0]];
        let s12 = s[[k, 0, 1]];
        let s21 = s[[k, 1, 0]];
        let s22 = s[[k, 1, 1]];
        if s21.norm() < 1e-300 {
            return Err(format!(
                "s2t: S21 ≈ 0 at frequency index {k} — T-parameters undefined for non-transmitting networks"
            ));
        }
        let det_s = s11 * s22 - s12 * s21;
        t[[k, 0, 0]] = -det_s / s21;
        t[[k, 0, 1]] = s11 / s21;
        t[[k, 1, 0]] = -s22 / s21;
        t[[k, 1, 1]] = c(1.0, 0.0) / s21;
    }
    Ok(t)
}

pub fn t_to_s(t: &Array3<C64>) -> Result<Array3<C64>, String> {
    require_2port("t2s", t)?;
    let n_freqs = t.shape()[0];
    let mut s: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
    for k in 0..n_freqs {
        let t11 = t[[k, 0, 0]];
        let t12 = t[[k, 0, 1]];
        let t21 = t[[k, 1, 0]];
        let t22 = t[[k, 1, 1]];
        if t22.norm() < 1e-300 {
            return Err(format!(
                "t2s: T22 ≈ 0 at frequency index {k} — S-parameters undefined"
            ));
        }
        let det_t = t11 * t22 - t12 * t21;
        s[[k, 0, 0]] = t12 / t22;
        s[[k, 0, 1]] = det_t / t22;
        s[[k, 1, 0]] = c(1.0, 0.0) / t22;
        s[[k, 1, 1]] = -t21 / t22;
    }
    Ok(s)
}

// ─── 2-port ABCD-parameters (voltage/current chain) ──────────────────────────
//
// Convention: [V1; I1] = ABCD · [V2; -I2]. Useful because lumped elements have
// trivial ABCD matrices (series Z: [[1, Z],[0, 1]]; shunt Y: [[1, 0],[Y, 1]]).
// Conversion formulas from Pozar §4.4 Table 4.2.

pub fn s_to_abcd(s: &Array3<C64>, z0: f64) -> Result<Array3<C64>, String> {
    require_2port("s2abcd", s)?;
    let n_freqs = s.shape()[0];
    let mut a3: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
    let z0c = c(z0, 0.0);
    let two = c(2.0, 0.0);
    for k in 0..n_freqs {
        let s11 = s[[k, 0, 0]];
        let s12 = s[[k, 0, 1]];
        let s21 = s[[k, 1, 0]];
        let s22 = s[[k, 1, 1]];
        if s21.norm() < 1e-300 {
            return Err(format!(
                "s2abcd: S21 ≈ 0 at frequency index {k} — ABCD parameters undefined"
            ));
        }
        let one = c(1.0, 0.0);
        let denom = two * s21;
        a3[[k, 0, 0]] = ((one + s11) * (one - s22) + s12 * s21) / denom;
        a3[[k, 0, 1]] = z0c * ((one + s11) * (one + s22) - s12 * s21) / denom;
        a3[[k, 1, 0]] = ((one - s11) * (one - s22) - s12 * s21) / (denom * z0c);
        a3[[k, 1, 1]] = ((one - s11) * (one + s22) + s12 * s21) / denom;
    }
    Ok(a3)
}

pub fn abcd_to_s(a: &Array3<C64>, z0: f64) -> Result<Array3<C64>, String> {
    require_2port("abcd2s", a)?;
    let n_freqs = a.shape()[0];
    let z0c = c(z0, 0.0);
    let two = c(2.0, 0.0);
    let mut s: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
    for k in 0..n_freqs {
        let aa = a[[k, 0, 0]];
        let bb = a[[k, 0, 1]];
        let cc = a[[k, 1, 0]];
        let dd = a[[k, 1, 1]];
        let denom = aa + bb / z0c + cc * z0c + dd;
        if denom.norm() < 1e-300 {
            return Err(format!(
                "abcd2s: denominator ≈ 0 at frequency index {k}"
            ));
        }
        s[[k, 0, 0]] = (aa + bb / z0c - cc * z0c - dd) / denom;
        s[[k, 0, 1]] = two * (aa * dd - bb * cc) / denom;
        s[[k, 1, 0]] = two / denom;
        s[[k, 1, 1]] = (-aa + bb / z0c - cc * z0c + dd) / denom;
    }
    Ok(s)
}

// ─── Cascade / De-embedding (2-port) ─────────────────────────────────────────

pub fn cascade_s_pair(a: &Array3<C64>, b: &Array3<C64>) -> Result<Array3<C64>, String> {
    require_2port("cascade", a)?;
    require_2port("cascade", b)?;
    if a.shape()[0] != b.shape()[0] {
        return Err(format!(
            "cascade: frequency counts differ ({} vs {})",
            a.shape()[0],
            b.shape()[0]
        ));
    }
    let ta = s_to_t(a)?;
    let tb = s_to_t(b)?;
    let n_freqs = a.shape()[0];
    let mut tc: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
    for k in 0..n_freqs {
        let ma = slab(&ta, k);
        let mb = slab(&tb, k);
        let mc = mat_mul(&ma, &mb);
        write_slab(&mut tc, k, &mc);
    }
    t_to_s(&tc)
}

pub fn cascade_s_chain(networks: &[Array3<C64>]) -> Result<Array3<C64>, String> {
    if networks.is_empty() {
        return Err("cascade: requires at least one network".to_string());
    }
    let mut acc = networks[0].clone();
    for n in &networks[1..] {
        acc = cascade_s_pair(&acc, n)?;
    }
    Ok(acc)
}

/// De-embed fixtures from a measured cascade:  T_dut = T_left⁻¹ · T_meas · T_right⁻¹.
pub fn deembed_s(
    meas: &Array3<C64>,
    left: &Array3<C64>,
    right: &Array3<C64>,
) -> Result<Array3<C64>, String> {
    require_2port("deembed", meas)?;
    require_2port("deembed", left)?;
    require_2port("deembed", right)?;
    let n_freqs = meas.shape()[0];
    if left.shape()[0] != n_freqs || right.shape()[0] != n_freqs {
        return Err("deembed: frequency counts must match across meas/left/right".to_string());
    }
    let tm = s_to_t(meas)?;
    let tl = s_to_t(left)?;
    let tr = s_to_t(right)?;
    let mut td: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
    for k in 0..n_freqs {
        let l_inv = matrix_inv(&slab(&tl, k)).map_err(|e| format!("deembed left: {e}"))?;
        let r_inv = matrix_inv(&slab(&tr, k)).map_err(|e| format!("deembed right: {e}"))?;
        let dut_t = mat_mul(&mat_mul(&l_inv, &slab(&tm, k)), &r_inv);
        write_slab(&mut td, k, &dut_t);
    }
    t_to_s(&td)
}

// ─── Re-normalisation ────────────────────────────────────────────────────────

pub fn renormalise_s(s: &Array3<C64>, z_old: f64, z_new: f64) -> Result<Array3<C64>, String> {
    // Detour through Z-domain: Z = Z_old·(I+S)(I-S)⁻¹  →  S' from Z and Z_new.
    let z = s_to_z(s, z_old)?;
    z_to_s(&z, z_new)
}

// ─── Mixed-mode (differential / common-mode) conversion — Phase 6 ────────────
//
// Standard 4-port pairing: single-ended ports (1,3) form differential pair
// 1 with port 1 positive, port 3 negative; ports (2,4) form differential
// pair 2. The mixed-mode network has ports ordered [Sdd | Sdc; Scd | Scc],
// i.e. differential mode 1, differential mode 2, common mode 1, common mode 2.
//
// Transformation matrix (Bockelman/Eisenstadt 1995; standard convention used
// by every commercial mixed-mode-capable VNA):
//   M = (1/√2) · [[ 1, 0, -1,  0],     // d1 = (a1 - a3) / √2
//                 [ 0, 1,  0, -1],     // d2 = (a2 - a4) / √2
//                 [ 1, 0,  1,  0],     // c1 = (a1 + a3) / √2
//                 [ 0, 1,  0,  1]]     // c2 = (a2 + a4) / √2
//
// Forward: Smm = M · Sse · M⁻¹ = M · Sse · Mᵀ (since M is orthogonal).
// Reverse: Sse = Mᵀ · Smm · M.

fn mixed_mode_transform() -> Array2<C64> {
    let inv_sqrt2 = 1.0_f64 / 2.0_f64.sqrt();
    let mut m: Array2<C64> = Array2::zeros((4, 4));
    let one = c(inv_sqrt2, 0.0);
    let neg = c(-inv_sqrt2, 0.0);
    // d1 row
    m[[0, 0]] = one;
    m[[0, 2]] = neg;
    // d2 row
    m[[1, 1]] = one;
    m[[1, 3]] = neg;
    // c1 row
    m[[2, 0]] = one;
    m[[2, 2]] = one;
    // c2 row
    m[[3, 1]] = one;
    m[[3, 3]] = one;
    m
}

/// Single-ended S-parameters → mixed-mode S-parameters (4-port only).
/// Resulting port order is [d1, d2, c1, c2].
pub fn s_to_smm(s: &Array3<C64>) -> Result<Array3<C64>, String> {
    if s.shape()[1] != 4 || s.shape()[2] != 4 {
        return Err(format!(
            "s2smm: requires a 4-port network, got {}-port",
            s.shape()[1]
        ));
    }
    let n_freqs = s.shape()[0];
    let m = mixed_mode_transform();
    let mt = m.t().to_owned();
    let mut smm: Array3<C64> = Array3::zeros((n_freqs, 4, 4));
    for k in 0..n_freqs {
        let sse_k = slab(s, k);
        let mid = mat_mul(&m, &sse_k);
        let smm_k = mat_mul(&mid, &mt);
        write_slab(&mut smm, k, &smm_k);
    }
    Ok(smm)
}

/// Mixed-mode S-parameters → single-ended (inverse of `s_to_smm`).
pub fn smm_to_s(smm: &Array3<C64>) -> Result<Array3<C64>, String> {
    if smm.shape()[1] != 4 || smm.shape()[2] != 4 {
        return Err(format!(
            "smm2s: requires a 4-port mixed-mode network, got {}-port",
            smm.shape()[1]
        ));
    }
    let n_freqs = smm.shape()[0];
    let m = mixed_mode_transform();
    let mt = m.t().to_owned();
    let mut sse: Array3<C64> = Array3::zeros((n_freqs, 4, 4));
    for k in 0..n_freqs {
        let smm_k = slab(smm, k);
        let mid = mat_mul(&mt, &smm_k);
        let sse_k = mat_mul(&mid, &m);
        write_slab(&mut sse, k, &sse_k);
    }
    Ok(sse)
}

// ─── Frequency-grid interpolation (Phase 6) ──────────────────────────────────

/// Linear interpolation of complex S-parameter values across frequency.
/// `freqs_old` and the slab-major axis of `s` must agree in length and
/// both must be strictly monotonically increasing (the latter is already
/// enforced by `build_sparameters_struct`).
///
/// For each `f_new`, finds the bracketing pair in `freqs_old`, computes the
/// linear factor `α = (f_new − f_lo) / (f_hi − f_lo)`, and returns
/// `S_lo + α · (S_hi − S_lo)` element-wise. Exact matches at boundary
/// frequencies return the boundary slab unchanged.
///
/// Hard error on extrapolation (`f_new < freqs_old[0]` or `> freqs_old[n−1]`)
/// — RF measurements are bandlimited, and extrapolating S-parameters past
/// the measured range gives garbage answers that are worse than failing.
pub fn interp_freq(
    s: &Array3<C64>,
    freqs_old: &[f64],
    freqs_new: &[f64],
) -> Result<Array3<C64>, String> {
    let (n_old, n_ports, _) = s.dim();
    if freqs_old.len() != n_old {
        return Err(format!(
            "interp_freq: freqs_old length ({}) must equal n_freqs in S ({n_old})",
            freqs_old.len()
        ));
    }
    if freqs_new.is_empty() {
        return Err("interp_freq: freqs_new must be non-empty".to_string());
    }
    for w in freqs_new.windows(2) {
        if !(w[1] > w[0]) {
            return Err(
                "interp_freq: freqs_new must be strictly monotonically increasing".to_string(),
            );
        }
    }
    let f_lo_bound = freqs_old[0];
    let f_hi_bound = freqs_old[n_old - 1];
    for &f in freqs_new {
        if f < f_lo_bound - 1e-9 || f > f_hi_bound + 1e-9 {
            return Err(format!(
                "interp_freq: target frequency {f} Hz outside source range [{f_lo_bound}, {f_hi_bound}] — extrapolation not supported"
            ));
        }
    }

    let n_new = freqs_new.len();
    let mut out: Array3<C64> = Array3::zeros((n_new, n_ports, n_ports));
    // Two-finger walk: search index advances as f_new grows. O(n_old + n_new)
    // total rather than O(n_old · n_new).
    let mut k_lo = 0usize;
    for (k_new, &f_new) in freqs_new.iter().enumerate() {
        while k_lo + 1 < n_old && freqs_old[k_lo + 1] < f_new {
            k_lo += 1;
        }
        // Clamp pathological floating-point edge: if f_new equals the last
        // sample exactly, k_lo might land at n_old-1, in which case we just
        // copy that slab.
        if k_lo + 1 >= n_old {
            for i in 0..n_ports {
                for j in 0..n_ports {
                    out[[k_new, i, j]] = s[[n_old - 1, i, j]];
                }
            }
            continue;
        }
        let f_lo = freqs_old[k_lo];
        let f_hi = freqs_old[k_lo + 1];
        let span = f_hi - f_lo;
        let alpha = if span > 0.0 {
            ((f_new - f_lo) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let one_minus_a = 1.0 - alpha;
        for i in 0..n_ports {
            for j in 0..n_ports {
                let s_lo = s[[k_lo, i, j]];
                let s_hi = s[[k_lo + 1, i, j]];
                out[[k_new, i, j]] = Complex::new(
                    one_minus_a * s_lo.re + alpha * s_hi.re,
                    one_minus_a * s_lo.im + alpha * s_hi.im,
                );
            }
        }
    }
    Ok(out)
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn arr_close(a: &Array3<C64>, b: &Array3<C64>, tol: f64) {
        assert_eq!(a.shape(), b.shape());
        for (x, y) in a.iter().zip(b.iter()) {
            assert!(
                (x.re - y.re).abs() < tol && (x.im - y.im).abs() < tol,
                "values differ: {x} vs {y}"
            );
        }
    }

    fn s_series_resistor(r: f64, z0: f64, n_freqs: usize) -> Array3<C64> {
        // Series resistor between port 1 and port 2: passive, reciprocal,
        // symmetric. S11 = r/(r+2), S21 = 2/(r+2) where r' = r/Z0.
        let r_norm = r / z0;
        let s11 = r_norm / (r_norm + 2.0);
        let s21 = 2.0 / (r_norm + 2.0);
        let mut s: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
        for k in 0..n_freqs {
            s[[k, 0, 0]] = c(s11, 0.0);
            s[[k, 1, 1]] = c(s11, 0.0);
            s[[k, 0, 1]] = c(s21, 0.0);
            s[[k, 1, 0]] = c(s21, 0.0);
        }
        s
    }

    fn s_shunt_resistor(r: f64, z0: f64, n_freqs: usize) -> Array3<C64> {
        // Shunt resistor to ground. S11 = -Z0/(2R+Z0), S21 = 2R/(2R+Z0).
        let s11 = -z0 / (2.0 * r + z0);
        let s21 = 2.0 * r / (2.0 * r + z0);
        let mut s: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
        for k in 0..n_freqs {
            s[[k, 0, 0]] = c(s11, 0.0);
            s[[k, 1, 1]] = c(s11, 0.0);
            s[[k, 0, 1]] = c(s21, 0.0);
            s[[k, 1, 0]] = c(s21, 0.0);
        }
        s
    }

    fn s_thru(n_freqs: usize) -> Array3<C64> {
        // Ideal thru: S11 = S22 = 0, S21 = S12 = 1.
        let mut s: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
        for k in 0..n_freqs {
            s[[k, 0, 1]] = c(1.0, 0.0);
            s[[k, 1, 0]] = c(1.0, 0.0);
        }
        s
    }

    /// A symmetric matched attenuator with `loss_db` of insertion loss.
    /// S11 = S22 = 0, |S21| = |S12| = 10^(-loss/20). This is a useful round-trip
    /// anchor because its (I±S) matrices are well-conditioned for every domain
    /// (S↔Z, S↔Y, S↔T, S↔ABCD) — unlike a pure series/shunt resistor whose
    /// Z- or Y-parameters are themselves singular.
    fn s_matched_attenuator(loss_db: f64, n_freqs: usize) -> Array3<C64> {
        let mag = 10f64.powf(-loss_db / 20.0);
        let mut s: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
        for k in 0..n_freqs {
            s[[k, 0, 1]] = c(mag, 0.0);
            s[[k, 1, 0]] = c(mag, 0.0);
        }
        s
    }

    /// Arbitrary non-degenerate 2-port S matrix for round-trip tests where the
    /// physics doesn't matter — just that every conversion's inverse exists.
    fn s_generic(n_freqs: usize) -> Array3<C64> {
        let mut s: Array3<C64> = Array3::zeros((n_freqs, 2, 2));
        for k in 0..n_freqs {
            let drift = k as f64 * 0.01;
            s[[k, 0, 0]] = c(0.1 + drift, 0.2);
            s[[k, 0, 1]] = c(0.3, -0.1);
            s[[k, 1, 0]] = c(0.4, 0.05);
            s[[k, 1, 1]] = c(-0.1 + drift, 0.1);
        }
        s
    }

    #[test]
    fn round_trip_s_z_s_2port() {
        let s = s_generic(3);
        let z = s_to_z(&s, 50.0).unwrap();
        let s2 = z_to_s(&z, 50.0).unwrap();
        arr_close(&s, &s2, 1e-10);
    }

    #[test]
    fn round_trip_s_y_s_2port() {
        let s = s_generic(3);
        let y = s_to_y(&s, 50.0).unwrap();
        let s2 = y_to_s(&y, 50.0).unwrap();
        arr_close(&s, &s2, 1e-10);
    }

    #[test]
    fn round_trip_s_z_s_3port() {
        // N-port: use a contrived but well-conditioned 3x3 S matrix.
        let mut s: Array3<C64> = Array3::zeros((1, 3, 3));
        s[[0, 0, 0]] = c(0.10, 0.05);
        s[[0, 0, 1]] = c(0.20, 0.00);
        s[[0, 0, 2]] = c(0.15, 0.05);
        s[[0, 1, 0]] = c(0.20, 0.00);
        s[[0, 1, 1]] = c(0.05, 0.10);
        s[[0, 1, 2]] = c(0.25, 0.10);
        s[[0, 2, 0]] = c(0.15, 0.05);
        s[[0, 2, 1]] = c(0.25, 0.10);
        s[[0, 2, 2]] = c(-0.05, 0.15);
        let z = s_to_z(&s, 50.0).unwrap();
        let s2 = z_to_s(&z, 50.0).unwrap();
        arr_close(&s, &s2, 1e-10);
    }

    #[test]
    fn round_trip_s_t_s_2port() {
        let s = s_series_resistor(25.0, 50.0, 4);
        let t = s_to_t(&s).unwrap();
        let s2 = t_to_s(&t).unwrap();
        arr_close(&s, &s2, 1e-12);
    }

    #[test]
    fn round_trip_s_abcd_s_2port() {
        let s = s_series_resistor(33.0, 50.0, 5);
        let a = s_to_abcd(&s, 50.0).unwrap();
        let s2 = abcd_to_s(&a, 50.0).unwrap();
        arr_close(&s, &s2, 1e-10);
    }

    #[test]
    fn matched_attenuator_round_trips_through_z_and_y() {
        // 10 dB matched pad: S11 = S22 = 0, |S21| = 0.31623. (I−S) and (I+S)
        // are both well-conditioned, so Z and Y exist and the round-trip is
        // numerically clean.
        let s = s_matched_attenuator(10.0, 2);
        let z = s_to_z(&s, 50.0).unwrap();
        let s_from_z = z_to_s(&z, 50.0).unwrap();
        arr_close(&s, &s_from_z, 1e-12);
        let y = s_to_y(&s, 50.0).unwrap();
        let s_from_y = y_to_s(&y, 50.0).unwrap();
        arr_close(&s, &s_from_y, 1e-12);
    }

    #[test]
    fn series_resistor_abcd_matches_lumped_model() {
        // Series Z lumped element: A=1, B=Z, C=0, D=1.
        let s = s_series_resistor(25.0, 50.0, 1);
        let abcd = s_to_abcd(&s, 50.0).unwrap();
        let aa = abcd[[0, 0, 0]];
        let bb = abcd[[0, 0, 1]];
        let cc = abcd[[0, 1, 0]];
        let dd = abcd[[0, 1, 1]];
        assert!((aa - c(1.0, 0.0)).norm() < 1e-10, "A: {aa}");
        assert!((bb - c(25.0, 0.0)).norm() < 1e-10, "B: {bb}");
        assert!(cc.norm() < 1e-10, "C: {cc}");
        assert!((dd - c(1.0, 0.0)).norm() < 1e-10, "D: {dd}");
    }

    #[test]
    fn shunt_resistor_abcd_matches_lumped_model() {
        // Shunt admittance Y = 1/R: A=1, B=0, C=Y, D=1.
        let r = 100.0;
        let s = s_shunt_resistor(r, 50.0, 1);
        let abcd = s_to_abcd(&s, 50.0).unwrap();
        let aa = abcd[[0, 0, 0]];
        let bb = abcd[[0, 0, 1]];
        let cc = abcd[[0, 1, 0]];
        let dd = abcd[[0, 1, 1]];
        assert!((aa - c(1.0, 0.0)).norm() < 1e-10, "A: {aa}");
        assert!(bb.norm() < 1e-10, "B: {bb}");
        assert!((cc - c(1.0 / r, 0.0)).norm() < 1e-10, "C: {cc}");
        assert!((dd - c(1.0, 0.0)).norm() < 1e-10, "D: {dd}");
    }

    #[test]
    fn cascade_thru_thru_is_thru() {
        let t1 = s_thru(3);
        let t2 = s_thru(3);
        let c12 = cascade_s_pair(&t1, &t2).unwrap();
        arr_close(&c12, &s_thru(3), 1e-12);
    }

    #[test]
    fn cascade_series_resistors_adds_resistance() {
        // R1 in series cascaded with R2 in series → equivalent to R1+R2 in
        // series. Compare S21 to the analytic series-(R1+R2) network.
        let r1 = 25.0;
        let r2 = 75.0;
        let z0 = 50.0;
        let s1 = s_series_resistor(r1, z0, 1);
        let s2 = s_series_resistor(r2, z0, 1);
        let combined = cascade_s_pair(&s1, &s2).unwrap();
        let expected = s_series_resistor(r1 + r2, z0, 1);
        arr_close(&combined, &expected, 1e-10);
    }

    #[test]
    fn deembed_recovers_dut() {
        // Synthetic experiment: build meas = cascade(L, DUT, R), then
        // deembed(meas, L, R) should recover DUT to high precision.
        let z0 = 50.0;
        let n = 4;
        let left = s_series_resistor(10.0, z0, n);
        let right = s_shunt_resistor(200.0, z0, n);
        let dut = s_series_resistor(33.0, z0, n);
        let meas = cascade_s_pair(&cascade_s_pair(&left, &dut).unwrap(), &right).unwrap();
        let recovered = deembed_s(&meas, &left, &right).unwrap();
        arr_close(&recovered, &dut, 1e-9);
    }

    #[test]
    fn renormalise_50_to_75_to_50_is_identity() {
        // Use the matched attenuator: well-conditioned Z domain.
        let s = s_matched_attenuator(6.0, 3);
        let s_75 = renormalise_s(&s, 50.0, 75.0).unwrap();
        let s_back = renormalise_s(&s_75, 75.0, 50.0).unwrap();
        arr_close(&s, &s_back, 1e-10);
    }

    #[test]
    fn renormalise_50_to_50_is_identity() {
        // Trivial sanity check: renormalising to the same Z0 must not change
        // the matrix at all (and must not throw).
        let s = s_generic(2);
        let s2 = renormalise_s(&s, 50.0, 50.0).unwrap();
        arr_close(&s, &s2, 1e-10);
    }

    #[test]
    fn cascade_freq_mismatch_errors() {
        let a = s_series_resistor(10.0, 50.0, 3);
        let b = s_series_resistor(20.0, 50.0, 5);
        let err = cascade_s_pair(&a, &b).unwrap_err();
        assert!(err.contains("frequency"), "{err}");
    }

    #[test]
    fn ten_db_attenuator_t_param_cascade_is_twenty_db() {
        // Cross-check: cascade two 10 dB matched attenuators via T-multiplication
        // and verify the resulting S21 matches the analytic 20 dB / S11 = 0
        // expectation.
        let att = s_matched_attenuator(10.0, 1);
        let combo = cascade_s_pair(&att, &att).unwrap();
        let s11 = combo[[0, 0, 0]].norm();
        let s21 = combo[[0, 1, 0]].norm();
        assert!(s11 < 1e-12, "S11 should be 0, got {s11}");
        assert!((s21 - 0.1).abs() < 1e-10, "S21 should be 0.1, got {s21}");
    }

    #[test]
    fn ten_db_attenuator_cascade_is_twenty_db() {
        // Symmetric 10 dB pi-attenuator at 50 Ω:
        // |S21| = 10^(-10/20) ≈ 0.31623; matched (S11 = 0). Cascading two of
        // them ideally yields |S21| ≈ 10^(-20/20) = 0.1 with S11 ≈ 0.
        let mag = 10f64.powf(-10.0 / 20.0);
        let mut att: Array3<C64> = Array3::zeros((1, 2, 2));
        att[[0, 0, 1]] = c(mag, 0.0);
        att[[0, 1, 0]] = c(mag, 0.0);
        // Matched attenuator: S11 = S22 = 0.
        let result = cascade_s_pair(&att, &att).unwrap();
        let s11 = result[[0, 0, 0]].norm();
        let s21 = result[[0, 1, 0]].norm();
        assert!(s11 < 1e-12, "S11 should be ~0, got {s11}");
        assert!((s21 - 0.1).abs() < 1e-10, "S21 should be 0.1, got {s21}");
    }

    // ── Phase 6: interp_freq math ──────────────────────────────────────────

    #[test]
    fn interp_freq_at_existing_sample_returns_exact_value() {
        let s = s_generic(5);
        let freqs_old: Vec<f64> = (0..5).map(|k| 1e9 + k as f64 * 1e8).collect();
        let r = interp_freq(&s, &freqs_old, &[1.3e9]).unwrap();
        for i in 0..2 {
            for j in 0..2 {
                let expected = s[[3, i, j]];
                let got = r[[0, i, j]];
                assert!(
                    (got - expected).norm() < 1e-12,
                    "({i},{j}): got {got}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn interp_freq_midpoint_is_arithmetic_mean() {
        let s = s_generic(2);
        let freqs_old = vec![1.0e9, 2.0e9];
        let r = interp_freq(&s, &freqs_old, &[1.5e9]).unwrap();
        for i in 0..2 {
            for j in 0..2 {
                let mean = (s[[0, i, j]] + s[[1, i, j]]) * c(0.5, 0.0);
                let got = r[[0, i, j]];
                assert!((got - mean).norm() < 1e-12);
            }
        }
    }

    #[test]
    fn interp_freq_rejects_extrapolation() {
        let s = s_generic(3);
        let f_old: Vec<f64> = (0..3).map(|k| 1e9 + k as f64 * 1e8).collect();
        let err = interp_freq(&s, &f_old, &[0.5e9]).unwrap_err();
        assert!(err.contains("outside source range"));
        let err = interp_freq(&s, &f_old, &[2e9]).unwrap_err();
        assert!(err.contains("outside source range"));
    }

    #[test]
    fn interp_freq_rejects_non_monotonic_new_grid() {
        let s = s_generic(3);
        let f_old: Vec<f64> = (0..3).map(|k| 1e9 + k as f64 * 1e8).collect();
        let err = interp_freq(&s, &f_old, &[1.1e9, 1.0e9]).unwrap_err();
        assert!(err.contains("increasing"));
    }

    // ── Phase 6: mixed-mode round-trip ─────────────────────────────────────

    fn s_generic_4port(n_freqs: usize) -> Array3<C64> {
        let mut s: Array3<C64> = Array3::zeros((n_freqs, 4, 4));
        for k in 0..n_freqs {
            let drift = k as f64 * 0.01;
            for i in 0..4 {
                for j in 0..4 {
                    let re = 0.05 * (i + j) as f64 + drift;
                    let im = -0.03 * (i as f64 - j as f64);
                    s[[k, i, j]] = c(re, im);
                }
            }
        }
        s
    }

    #[test]
    fn s_to_smm_round_trip() {
        let s = s_generic_4port(2);
        let smm = s_to_smm(&s).unwrap();
        let back = smm_to_s(&smm).unwrap();
        for (a, b) in s.iter().zip(back.iter()) {
            assert!((a - b).norm() < 1e-12, "{a} vs {b}");
        }
    }

    #[test]
    fn s_to_smm_of_diagonal_thru_pair_has_no_mode_conversion() {
        // Two ideal thrus: 1↔3 and 2↔4. Mixed-mode result must have zero
        // Sdc and Scd blocks (no differential↔common conversion).
        let mut s: Array3<C64> = Array3::zeros((1, 4, 4));
        s[[0, 0, 2]] = c(1.0, 0.0);
        s[[0, 2, 0]] = c(1.0, 0.0);
        s[[0, 1, 3]] = c(1.0, 0.0);
        s[[0, 3, 1]] = c(1.0, 0.0);
        let smm = s_to_smm(&s).unwrap();
        for i in 0..2 {
            for j in 2..4 {
                assert!(smm[[0, i, j]].norm() < 1e-12, "Sdc[{i},{j}]: {}", smm[[0, i, j]]);
                assert!(smm[[0, j, i]].norm() < 1e-12, "Scd[{j},{i}]: {}", smm[[0, j, i]]);
            }
        }
    }

    #[test]
    fn s_to_smm_rejects_non_4port() {
        let s = s_generic(2);
        let err = s_to_smm(&s).unwrap_err();
        assert!(err.contains("4-port"));
    }
}

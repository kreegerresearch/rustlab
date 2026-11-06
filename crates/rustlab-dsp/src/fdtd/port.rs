//! One lumped port: a series voltage source and a gap resistance, plus S11.
//!
//! The gap resistance is split across the stack and baked as extra edge
//! conductivity (`media::bake`). The source is **not** the impressed-field
//! update written in the plan (`Ez := Ez − Cb·Vg/(N·Δ)`). That line is the
//! existing soft source. It does not put `Vg` in series with `Z0`: the
//! steady Ampère current would be `−V/R + Vg·Area/Δ`, not `(Vg − V)/Z0`.
//!
//! The series source the same paragraph describes adds the current `Vg/Z0`:
//!
//! ```text
//! J  = Vg / (Z0 · Area)          (the same on every edge; N cancels)
//! E := E − (dt/ε0) · Cb_rel · J
//! ```
//!
//! `Cb_rel` is the baked relative coefficient (1 in vacuum, `(1/ε)/(1+s)`
//! once `σ` is included). Positive `Vg` makes `E` more negative, so
//! `V = −Σ E·Δ` rises, the same sign as a soft source. In the quasi-static
//! limit `(Vg − V) / I → Z0`. `V/I → Z0` is the impedance of the grid the
//! port drives, which equals `Z0` only when that grid is matched.
//!
//! Ampère segment lengths follow the Yee curl in `yee.rs`, not the plan's
//! `dx`/`dy` swap (they agree on a square cell). The sign is the plan's:
//! positive `V` and positive `I` send power into the grid.

use super::EPS0;
use crate::error::DspError;
use crate::fft::fft;
use crate::window::WindowFunction;
use ndarray::Array1;
use num_complex::Complex;
use std::f64::consts::PI;

/// Window applied to `V` and `I` before the DFT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortWindow {
    /// Symmetric Hann, endpoints about 0. The default.
    Hann,
    /// Rectangular. Used when a long tail must not be tapered.
    Rect,
}

impl PortWindow {
    /// `"hann"` / `"hanning"` or `"rect"` / `"rectangular"`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "hann" | "hanning" => Some(Self::Hann),
            "rect" | "rectangular" => Some(Self::Rect),
            _ => None,
        }
    }
}

/// Positive-frequency S11 of one port. DC is omitted. Bin `m` (`1..=n/2`)
/// sits at `m / (n·dt)`, so an even length keeps the Nyquist bin.
#[derive(Clone, Debug)]
pub struct S11 {
    pub frequencies: Vec<f64>,
    pub s11: Vec<Complex<f64>>,
}

/// `σ` added to one edge of a stack whose total resistance is `z0`.
///
/// `R_edge = z0 / n`, `σ = Δ_along / (R_edge · Area)`.
pub(super) fn edge_sigma(z0: f64, n: usize, along: f64, area: f64) -> f64 {
    let r_edge = z0 / n as f64;
    along / (r_edge * area)
}

/// Amount subtracted from each port edge for open-circuit voltage `vg`.
///
/// `rel_cb` is the baked relative `Cb` of that edge (1 when the bake stored
/// nothing). See the module comment for why this is `Vg/(Z0·Area)` and not
/// `Vg/(N·Δ)`.
pub(super) fn series_delta(rel_cb: f64, dt: f64, vg: f64, z0: f64, area: f64) -> f64 {
    rel_cb * (dt / EPS0) * vg / (z0 * area)
}

/// Ampère current through an Ez edge, right-hand rule about +z.
///
/// `hy` / `hy_m` are `Hy(i, j, k)` and `Hy(i, j−1, k)`. `hx` / `hx_m` are
/// `Hx(i, j, k)` and `Hx(i−1, j, k)`. Segment lengths follow
/// `curl Ez ~ ∂Hy/∂x − ∂Hx/∂y`, so `I = ΔHy·dy − ΔHx·dx`.
pub(super) fn ampere_ez(hy: f64, hy_m: f64, hx: f64, hx_m: f64, dx: f64, dy: f64) -> f64 {
    (hy - hy_m) * dy - (hx - hx_m) * dx
}

/// Ampère current through an Ex edge, right-hand rule about +x.
///
/// `hz` / `hz_m` are `Hz(i, j, k)` and `Hz(i−1, j, k)`. `hy` / `hy_m` are
/// `Hy(i, j, k)` and `Hy(i, j, k−1)`.
pub(super) fn ampere_ex(hz: f64, hz_m: f64, hy: f64, hy_m: f64, dy: f64, dz: f64) -> f64 {
    (hz - hz_m) * dz - (hy - hy_m) * dy
}

/// Ampère current through an Ey edge, right-hand rule about +y.
///
/// `hx` / `hx_m` are `Hx(i, j, k)` and `Hx(i, j, k−1)`. `hz` / `hz_m` are
/// `Hz(i, j, k)` and `Hz(i, j−1, k)`.
pub(super) fn ampere_ey(hx: f64, hx_m: f64, hz: f64, hz_m: f64, dx: f64, dz: f64) -> f64 {
    (hx - hx_m) * dx - (hz - hz_m) * dz
}

/// One-port S11 from stored gap voltage and Ampère current.
///
/// `I` is the half-step sample (`H^{n+1/2}` next to `V` at integer time).
/// Its DFT is multiplied by `exp(+j ω dt / 2)` before
///
/// ```text
/// a   = (V + Z0 I) / (2 √Z0)
/// b   = (V − Z0 I) / (2 √Z0)
/// S11 = b / a
/// ```
///
/// Bins whose `|a|` is below `1e-12` times the peak `|a|` (DC included in
/// the peak, then dropped) are `NaN`. `v` and `i` must be the same length,
/// at least 2, and finite. `dt` and `z0` must be finite and positive.
pub fn s11(v: &[f64], i: &[f64], dt: f64, z0: f64, window: PortWindow) -> Result<S11, DspError> {
    if v.len() != i.len() {
        return Err(DspError::InvalidParameter(format!(
            "V and I must have the same length, got {} and {}",
            v.len(),
            i.len()
        )));
    }
    let n = v.len();
    if n < 2 {
        return Err(DspError::InvalidParameter(format!(
            "need at least 2 samples to form a positive-frequency bin, got {n}"
        )));
    }
    if !(dt.is_finite() && dt > 0.0) {
        return Err(DspError::InvalidParameter(format!(
            "dt must be finite and positive, got {dt}"
        )));
    }
    if !(z0.is_finite() && z0 > 0.0) {
        return Err(DspError::InvalidParameter(format!(
            "Z0 must be finite and positive, got {z0}"
        )));
    }
    if v.iter().any(|x| !x.is_finite()) || i.iter().any(|x| !x.is_finite()) {
        return Err(DspError::InvalidParameter("V and I must be finite".into()));
    }

    let w: Vec<f64> = match window {
        PortWindow::Rect => vec![1.0; n],
        PortWindow::Hann => WindowFunction::Hann.generate(n).to_vec(),
    };
    let vc = Array1::from_iter(v.iter().zip(&w).map(|(&x, &ww)| Complex::new(x * ww, 0.0)));
    let ic = Array1::from_iter(i.iter().zip(&w).map(|(&x, &ww)| Complex::new(x * ww, 0.0)));
    let vw = fft(&vc)?;
    let iw = fft(&ic)?;

    let nd = n as f64 * dt;
    let sqrt_z = z0.sqrt();
    let mut a_bins = Vec::with_capacity(n / 2 + 1);
    let mut b_bins = Vec::with_capacity(n / 2 + 1);
    let mut freqs = Vec::with_capacity(n / 2 + 1);
    for m in 0..=n / 2 {
        let freq = m as f64 / nd;
        // exp(+j ω dt / 2) = exp(+j π m / n), including +j at Nyquist.
        let phase = Complex::new(0.0, PI * freq * dt).exp();
        let iw_c = iw[m] * phase;
        let a = (vw[m] + z0 * iw_c) / (2.0 * sqrt_z);
        let b = (vw[m] - z0 * iw_c) / (2.0 * sqrt_z);
        a_bins.push(a);
        b_bins.push(b);
        freqs.push(freq);
    }
    let peak = a_bins.iter().map(|a| a.norm()).fold(0.0_f64, f64::max);
    let floor = 1e-12 * peak;
    let mut frequencies = Vec::with_capacity(n / 2);
    let mut s11 = Vec::with_capacity(n / 2);
    for m in 1..=n / 2 {
        let a = a_bins[m];
        let mag = a.norm();
        let s = if !a.is_finite() || mag == 0.0 || mag < floor {
            Complex::new(f64::NAN, f64::NAN)
        } else {
            b_bins[m] / a
        };
        frequencies.push(freqs[m]);
        s11.push(s);
    }
    Ok(S11 { frequencies, s11 })
}

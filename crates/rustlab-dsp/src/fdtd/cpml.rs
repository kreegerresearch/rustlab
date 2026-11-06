//! Convolutional PML (Roden–Gedney) profiles and auxiliaries.
//!
//! One auxiliary per Cartesian derivative in the curl, inside an `n`-cell
//! slab on each enabled face. The outer boundary stays PEC. Coefficients are
//! baked only while `steps == 0`.
//!
//! The Yee update itself is the Phase 2 kernel (no `κ`, no `ψ`). After each
//! half-step a correction pass, only over cells inside an enabled slab, updates
//! `q` and adds the stretch. `q` stored on [`Absorber`] is `−Δψ`, not the
//! Roden–Gedney `ψ`: the correction is `c·((kinv − 1)·du − q)` with
//! `q := b·q − a·du`. That term is exactly 0 when `kinv = 1` and `q = +0`
//! (signed zero included), and the pass does not write a 0 back into the
//! field. Scratch rows (CPML off) are not read by the kernel. Full `q` arrays
//! are allocated when any face is on.

use super::{Axis, C0, EPS0};
use crate::error::DspError;
use rustlab_core::size::{checked_elems, ELEM_F64};

/// H-update auxiliaries, in the order stored by [`Absorber::q_h`].
pub(super) const HX_Y: usize = 0;
pub(super) const HX_Z: usize = 1;
pub(super) const HY_Z: usize = 2;
pub(super) const HY_X: usize = 3;
pub(super) const HZ_X: usize = 4;
pub(super) const HZ_Y: usize = 5;

/// E-update auxiliaries, in the order stored by [`Absorber::q_e`].
pub(super) const EX_Y: usize = 0;
pub(super) const EX_Z: usize = 1;
pub(super) const EY_Z: usize = 2;
pub(super) const EY_X: usize = 3;
pub(super) const EZ_X: usize = 4;
pub(super) const EZ_Y: usize = 5;

/// The six faces, in the order used by `faces` arrays and `fdtd_info.cpml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    XMin,
    XMax,
    YMin,
    YMax,
    ZMin,
    ZMax,
}

impl Face {
    pub const ALL: [Face; 6] = [
        Face::XMin,
        Face::XMax,
        Face::YMin,
        Face::YMax,
        Face::ZMin,
        Face::ZMax,
    ];

    /// Case-insensitive `"xmin"` … `"zmax"`.
    pub fn parse(s: &str) -> Option<Face> {
        match s.to_ascii_lowercase().as_str() {
            "xmin" => Some(Face::XMin),
            "xmax" => Some(Face::XMax),
            "ymin" => Some(Face::YMin),
            "ymax" => Some(Face::YMax),
            "zmin" => Some(Face::ZMin),
            "zmax" => Some(Face::ZMax),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Face::XMin => "xmin",
            Face::XMax => "xmax",
            Face::YMin => "ymin",
            Face::YMax => "ymax",
            Face::ZMin => "zmin",
            Face::ZMax => "zmax",
        }
    }

    /// Index into [`Face::ALL`].
    pub fn index(self) -> usize {
        match self {
            Face::XMin => 0,
            Face::XMax => 1,
            Face::YMin => 2,
            Face::YMax => 3,
            Face::ZMin => 4,
            Face::ZMax => 5,
        }
    }

    pub fn axis(self) -> Axis {
        match self {
            Face::XMin | Face::XMax => Axis::X,
            Face::YMin | Face::YMax => Axis::Y,
            Face::ZMin | Face::ZMax => Axis::Z,
        }
    }

    pub fn is_max(self) -> bool {
        matches!(self, Face::XMax | Face::YMax | Face::ZMax)
    }
}

/// User knobs. [`Default`] is the plan's defaults (`sigma_factor` 1,
/// `kappa_max` 1, `alpha_max` 0, `order` 3, `alpha_order` 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CpmlParams {
    pub sigma_factor: f64,
    pub kappa_max: f64,
    pub alpha_max: f64,
    pub order: f64,
    pub alpha_order: f64,
}

impl Default for CpmlParams {
    fn default() -> Self {
        Self {
            sigma_factor: 1.0,
            kappa_max: 1.0,
            alpha_max: 0.0,
            order: 3.0,
            alpha_order: 1.0,
        }
    }
}

impl CpmlParams {
    pub fn validate(&self) -> Result<(), DspError> {
        check("sigma_factor", self.sigma_factor, 0.0)?;
        check("kappa_max", self.kappa_max, 1.0)?;
        check("alpha_max", self.alpha_max, 0.0)?;
        check("order", self.order, 0.0)?;
        check("alpha_order", self.alpha_order, 0.0)?;
        Ok(())
    }
}

fn check(name: &str, v: f64, min: f64) -> Result<(), DspError> {
    if v.is_finite() && v >= min {
        Ok(())
    } else {
        // `min` is 0 or 1; print it as an integer so the message matches the
        // script-facing text ("finite and >= 0" / "finite and >= 1").
        let bound = if min == 0.0 { "0" } else { "1" };
        Err(DspError::InvalidParameter(format!(
            "{name} must be finite and >= {bound}, got {v}"
        )))
    }
}

/// What the user asked for. `faces[f]` is 0 (off) or `n`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CpmlSettings {
    pub n: usize,
    pub faces: [usize; 6],
    pub params: CpmlParams,
}

/// `m` is inside the lo or hi slab. `half` is the H (half-node) sampling.
/// `n_hi = 0` must not match: `len - 0 = len`, and no sample has `m > len`
/// (E) or `m >= len` (H).
#[inline]
pub(super) fn in_slab(m: usize, len: usize, n_lo: usize, n_hi: usize, half: bool) -> bool {
    if m < n_lo {
        return true;
    }
    n_hi > 0
        && n_hi <= len
        && if half {
            m >= len - n_hi
        } else {
            m > len - n_hi
        }
}

/// One axis of CPML coefficients, one entry per sample along that axis.
/// Identity entries are exactly `kinv = 1.0`, `b = 1.0`, `a = 0.0`.
#[derive(Clone, Debug)]
pub(super) struct Profile {
    pub kinv: Vec<f64>,
    pub b: Vec<f64>,
    pub a: Vec<f64>,
}

impl Profile {
    pub(super) fn identity(len: usize) -> Profile {
        Profile {
            kinv: vec![1.0; len],
            b: vec![1.0; len],
            a: vec![0.0; len],
        }
    }

    /// `len` samples along an axis of `len` cells. `half` selects H sampling
    /// (half-node positions). Samples outside the slab are identity literals,
    /// not the profile formula (so `α(ρ = 0)` never leaks into the interior).
    pub(super) fn bake(
        len: usize,
        n_lo: usize,
        n_hi: usize,
        delta: f64,
        dt: f64,
        p: &CpmlParams,
        half: bool,
    ) -> Profile {
        let mut kinv = Vec::with_capacity(len);
        let mut b = Vec::with_capacity(len);
        let mut a = Vec::with_capacity(len);
        let sigma_max = p.sigma_factor * (p.order + 1.0) * EPS0 * C0 / delta;
        for m in 0..len {
            if !in_slab(m, len, n_lo, n_hi, half) {
                kinv.push(1.0);
                b.push(1.0);
                a.push(0.0);
                continue;
            }
            let on_lo = m < n_lo;
            let (depth_cells, into) = if on_lo {
                let pos = if half {
                    (m as f64 + 0.5) * delta
                } else {
                    m as f64 * delta
                };
                (n_lo as f64 * delta, n_lo as f64 * delta - pos)
            } else {
                let pos = if half {
                    (m as f64 + 0.5) * delta
                } else {
                    m as f64 * delta
                };
                let inner = (len - n_hi) as f64 * delta;
                (n_hi as f64 * delta, pos - inner)
            };
            let rho = into / depth_cells;
            let rho_m = rho.powf(p.order);
            let sigma = sigma_max * rho_m;
            let kappa = 1.0 + (p.kappa_max - 1.0) * rho_m;
            let alpha = p.alpha_max * (1.0 - rho).powf(p.alpha_order);
            let bb = (-(sigma / kappa + alpha) * dt / EPS0).exp();
            // `σ = 0` makes the formula `0/0` when `α = 0` too. A literal
            // `+0.0` keeps the sigma-factor-0 identity exact; `σ = 0, α > 0`
            // is the same value the formula would give (`0 · (b − 1)`).
            let aa = if sigma == 0.0 {
                0.0
            } else {
                sigma / (kappa * (sigma + kappa * alpha)) * (bb - 1.0)
            };
            kinv.push(1.0 / kappa);
            b.push(bb);
            a.push(aa);
        }
        Profile { kinv, b, a }
    }
}

/// Baked coefficients and auxiliaries. Always present on a grid.
#[derive(Clone, Debug)]
pub(super) struct Absorber {
    /// Node (E) samples, axes x, y, z. Lengths `nx`, `ny`, `nz`.
    pub e: [Profile; 3],
    /// Half-node (H) samples, axes x, y, z.
    pub h: [Profile; 3],
    /// `EX_Y` .. `EZ_Y`. Full grid, or one scratch row of `nx` when CPML is off.
    pub q_e: [Vec<f64>; 6],
    /// `HX_Y` .. `HZ_Y`. Same shape rule as `q_e`.
    pub q_h: [Vec<f64>; 6],
}

impl Absorber {
    /// Identity profiles and one scratch row per auxiliary. `q` stays `+0.0`.
    pub(super) fn off(nx: usize, ny: usize, nz: usize) -> Absorber {
        let rows = || std::array::from_fn(|_| vec![0.0; nx]);
        Absorber {
            e: [
                Profile::identity(nx),
                Profile::identity(ny),
                Profile::identity(nz),
            ],
            h: [
                Profile::identity(nx),
                Profile::identity(ny),
                Profile::identity(nz),
            ],
            q_e: rows(),
            q_h: rows(),
        }
    }

    pub(super) fn bake(
        nx: usize,
        ny: usize,
        nz: usize,
        (dx, dy, dz): (f64, f64, f64),
        dt: f64,
        s: &CpmlSettings,
    ) -> Result<Absorber, DspError> {
        let n_lo = |axis: usize| s.faces[axis * 2];
        let n_hi = |axis: usize| s.faces[axis * 2 + 1];
        let e = [
            Profile::bake(nx, n_lo(0), n_hi(0), dx, dt, &s.params, false),
            Profile::bake(ny, n_lo(1), n_hi(1), dy, dt, &s.params, false),
            Profile::bake(nz, n_lo(2), n_hi(2), dz, dt, &s.params, false),
        ];
        let h = [
            Profile::bake(nx, n_lo(0), n_hi(0), dx, dt, &s.params, true),
            Profile::bake(ny, n_lo(1), n_hi(1), dy, dt, &s.params, true),
            Profile::bake(nz, n_lo(2), n_hi(2), dz, dt, &s.params, true),
        ];
        for p in e.iter().chain(h.iter()) {
            for ((&k, &b), &a) in p.kinv.iter().zip(p.b.iter()).zip(p.a.iter()) {
                if !(k > 0.0 && k <= 1.0 && b > 0.0 && b <= 1.0 && a.is_finite() && a <= 0.0) {
                    return Err(DspError::InvalidParameter(format!(
                        "CPML coefficients left the range kinv in (0, 1], b in (0, 1], a finite and <= 0 \
                         (kinv = {k}, b = {b}, a = {a})"
                    )));
                }
            }
        }
        let active = s.faces.iter().any(|&n| n > 0);
        let qlen = if active {
            checked_elems(&[nx, ny, nz, 12], ELEM_F64)? / 12
        } else {
            nx
        };
        let rows = || std::array::from_fn(|_| vec![0.0; qlen]);
        Ok(Absorber {
            e,
            h,
            q_e: rows(),
            q_h: rows(),
        })
    }

    /// Bytes of the full auxiliaries and the profiles. 0 when off (scratch
    /// rows are a fixed overhead like `zero_row`, which `bytes()` does not
    /// count either).
    ///
    /// Full arrays: `12 · ncell · 8`. Profiles: `2 · 3 · (nx + ny + nz) · 8`
    /// — two sample sets (E, H), three coefficients (`kinv`, `b`, `a`), and
    /// the three axis lengths added together.
    pub(super) fn bytes(&self, active: bool) -> usize {
        if !active {
            return 0;
        }
        let ncell = self.q_h[0].len();
        let axes = self.e[0].kinv.len() + self.e[1].kinv.len() + self.e[2].kinv.len();
        12 * ncell * ELEM_F64 + 2 * 3 * axes * ELEM_F64
    }

    pub(super) fn is_full(&self) -> bool {
        let (nx, ny, nz) = (
            self.e[0].kinv.len(),
            self.e[1].kinv.len(),
            self.e[2].kinv.len(),
        );
        ny.saturating_mul(nz) > 1 && self.q_h[0].len() == nx * ny * nz
    }
}

//! Yee curl updates on x-fastest slices.
//!
//! Every array is indexed `(k * ny + i) * nx + j`, so one `(k, i)` row is a
//! contiguous run of `nx` values and the inner loop over `j` is unit stride.
//! A neighbour past the last index along an axis reads as 0, which closes the
//! max walls (tangential E on `x = nx·dx`, `y = ny·dy`, `z = nz·dz` is 0).
//! Tangential E on the min walls (`Ex`, `Ez` at `i = 0`; `Ey`, `Ez` at
//! `j = 0`; `Ex`, `Ey` at `k = 0`) is never updated and stays 0.
//!
//! Per-edge coefficients are relative to the vacuum curl coefficients in
//! [`Coeffs`]: an absent array means 1 (vacuum), so a grid without media runs
//! the same arithmetic as Phase 1, bit for bit. The variant is chosen once per
//! row, outside the inner loop.
//!
//! Every derivative goes through `kinv·du − q` (CPML). `q` is `−Δψ`
//! (Roden–Gedney `ψ`) so that `kinv·du − q` is exactly `du` when `kinv = 1`
//! and `q = +0`, signed zero included. A grid without CPML runs that same
//! arithmetic with identity coefficients (`kinv = 1`, `b = 1`, `a = 0`) and
//! one scratch row shared by every `(i, k)`. The closure is called once per
//! `j`; a second call would advance `q` twice.

use super::cpml::Profile;
use super::cpml::{EX_Y, EX_Z, EY_X, EY_Z, EZ_X, EZ_Y, HX_Y, HX_Z, HY_X, HY_Z, HZ_X, HZ_Y};

/// The twelve auxiliary slots are `0..6` in this order. Referenced so a
/// reordering of the constants fails here rather than silently in the curl.
const _: () = {
    assert!(HX_Y == 0 && HX_Z == 1 && HY_Z == 2 && HY_X == 3 && HZ_X == 4 && HZ_Y == 5);
    assert!(EX_Y == 0 && EX_Z == 1 && EY_Z == 2 && EY_X == 3 && EZ_X == 4 && EZ_Y == 5);
};

/// Per-axis curl coefficients: `dt / (μ0 Δ)` for H, `dt / (ε0 Δ)` for E.
#[derive(Clone, Copy, Debug)]
pub(super) struct Coeffs {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Dims {
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
}

impl Dims {
    #[inline]
    fn row(&self, i: usize, k: usize) -> usize {
        (k * self.ny + i) * self.nx
    }
}

/// Relative E coefficients per component (`[Ex, Ey, Ez]`). `cb` multiplies
/// the vacuum curl; `ca` multiplies the old E and is present only on a lossy
/// grid (then `cb` is present too).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ECoefs<'a> {
    pub cb: Option<[&'a [f64]; 3]>,
    pub ca: Option<[&'a [f64]; 3]>,
}

/// Relative H coefficients per component (`1 / mu_r` on the face).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct HCoefs<'a> {
    pub db: Option<[&'a [f64]; 3]>,
}

#[derive(Clone, Copy)]
enum ERow<'a> {
    Vacuum,
    Lossless(&'a [f64]),
    Lossy(&'a [f64], &'a [f64]),
}

#[derive(Clone, Copy)]
enum HRow<'a> {
    Vacuum,
    Scaled(&'a [f64]),
}

impl<'a> ECoefs<'a> {
    #[inline]
    fn row(&self, c: usize, r: usize, nx: usize) -> ERow<'a> {
        match (self.ca, self.cb) {
            (Some(ca), Some(cb)) => ERow::Lossy(&ca[c][r..r + nx], &cb[c][r..r + nx]),
            (None, Some(cb)) => ERow::Lossless(&cb[c][r..r + nx]),
            _ => ERow::Vacuum,
        }
    }
}

impl<'a> HCoefs<'a> {
    #[inline]
    fn row(&self, c: usize, r: usize, nx: usize) -> HRow<'a> {
        match self.db {
            Some(db) => HRow::Scaled(&db[c][r..r + nx]),
            None => HRow::Vacuum,
        }
    }
}

/// E profiles or H profiles, one [`Profile`] per axis.
#[derive(Clone, Copy)]
pub(super) struct Axes<'a> {
    pub x: &'a Profile,
    pub y: &'a Profile,
    pub z: &'a Profile,
}

/// `q := b q − a du`, returning the new `q`. `q` is `−Δψ` (Roden–Gedney ψ).
#[inline(always)]
fn aux(q: &mut f64, b: f64, a: f64, du: f64) -> f64 {
    let s = b * *q - a * du;
    *q = s;
    s
}

/// The material arms of one E row. A `FnMut` closure here compiled to a
/// call per cell (the curl body is too large to inline into `run`), which
/// dropped the vacuum kernel from ~350 Mcell/s to ~60. Pasting the body
/// into the loop keeps one kernel and one evaluation of the curl per `j`.
macro_rules! step_e {
    ($e:expr, $lo:expr, $coef:expr, |$j:ident| $body:expr) => {{
        let e = &mut $e[..];
        let n = e.len();
        let lo = $lo;
        match $coef {
            ERow::Vacuum => {
                for (x, $j) in e[lo..].iter_mut().zip(lo..) {
                    *x += $body;
                }
            }
            ERow::Lossless(cb) => {
                let cb = &cb[..n];
                for $j in lo..n {
                    e[$j] += cb[$j] * $body;
                }
            }
            ERow::Lossy(ca, cb) => {
                let (ca, cb) = (&ca[..n], &cb[..n]);
                for $j in lo..n {
                    e[$j] = ca[$j] * e[$j] + cb[$j] * $body;
                }
            }
        }
    }};
}

/// The material arms of one H row. Same reason as [`step_e`].
macro_rules! step_h {
    ($h:expr, $lo:expr, $hi:expr, $coef:expr, |$j:ident| $body:expr) => {{
        let hi = $hi;
        let h = &mut $h[..hi];
        let lo = $lo;
        match $coef {
            HRow::Vacuum => {
                for (x, $j) in h[lo..].iter_mut().zip(lo..) {
                    *x -= $body;
                }
            }
            HRow::Scaled(db) => {
                let db = &db[..hi];
                for $j in lo..hi {
                    h[$j] -= db[$j] * $body;
                }
            }
        }
    }};
}

/// `H^{n+1/2} = H^{n-1/2} - (dt/μ) curl E^n`, forward differences of E.
/// Each difference is the stretched form `kinv·du − q`.
///
/// `inline(never)`: the curl closures have to land inside this function's
/// row loops. Inlining the whole update into `FdtdSim::run` makes that
/// function large enough that LLVM leaves a call per cell (about 6× slower
/// than the Phase 2 vacuum kernel).
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn update_h(
    d: Dims,
    c: Coeffs,
    m: HCoefs,
    p: Axes,
    q: &mut [Vec<f64>; 6],
    zero: &[f64],
    ex: &[f64],
    ey: &[f64],
    ez: &[f64],
    hx: &mut [f64],
    hy: &mut [f64],
    hz: &mut [f64],
) {
    let nx = d.nx;
    let zero = &zero[..nx];
    // `false` → one scratch row, shared by every `(i, k)`.
    let full = q[0].len() == nx * d.ny * d.nz;
    let (kx, bx, ax) = (&p.x.kinv[..nx], &p.x.b[..nx], &p.x.a[..nx]);
    let [q_hx_y, q_hx_z, q_hy_z, q_hy_x, q_hz_x, q_hz_y] = q;
    for k in 0..d.nz {
        let (kz, bz, az) = (p.z.kinv[k], p.z.b[k], p.z.a[k]);
        for i in 0..d.ny {
            let (ky, by, ay) = (p.y.kinv[i], p.y.b[i], p.y.a[i]);
            let r = d.row(i, k);
            let pr = if full { r } else { 0 };
            let ex_r = &ex[r..r + nx];
            let ey_r = &ey[r..r + nx];
            let ez_r = &ez[r..r + nx];
            let (ex_ip, ez_ip) = if i + 1 < d.ny {
                let r2 = d.row(i + 1, k);
                (&ex[r2..r2 + nx], &ez[r2..r2 + nx])
            } else {
                (zero, zero)
            };
            let (ex_kp, ey_kp) = if k + 1 < d.nz {
                let r2 = d.row(i, k + 1);
                (&ex[r2..r2 + nx], &ey[r2..r2 + nx])
            } else {
                (zero, zero)
            };
            let hx_r = &mut hx[r..r + nx];
            let hy_r = &mut hy[r..r + nx];
            let hz_r = &mut hz[r..r + nx];
            let (mx, my, mz) = (m.row(0, r, nx), m.row(1, r, nx), m.row(2, r, nx));

            let (q1, q2) = (&mut q_hx_y[pr..pr + nx], &mut q_hx_z[pr..pr + nx]);
            step_h!(hx_r, 0, nx, mx, |j| {
                let du_y = ez_ip[j] - ez_r[j];
                let du_z = ey_kp[j] - ey_r[j];
                let sy = aux(&mut q1[j], by, ay, du_y);
                let sz = aux(&mut q2[j], bz, az, du_z);
                c.y * (ky * du_y - sy) - c.z * (kz * du_z - sz)
            });

            let (q3, q4) = (&mut q_hy_z[pr..pr + nx], &mut q_hy_x[pr..pr + nx]);
            step_h!(hy_r, 0, nx - 1, my, |j| {
                let du_z = ex_kp[j] - ex_r[j];
                let du_x = ez_r[j + 1] - ez_r[j];
                let sz = aux(&mut q3[j], bz, az, du_z);
                let sx = aux(&mut q4[j], bx[j], ax[j], du_x);
                c.z * (kz * du_z - sz) - c.x * (kx[j] * du_x - sx)
            });
            let (q5, q6) = (&mut q_hz_x[pr..pr + nx], &mut q_hz_y[pr..pr + nx]);
            step_h!(hz_r, 0, nx - 1, mz, |j| {
                let du_x = ey_r[j + 1] - ey_r[j];
                let du_y = ex_ip[j] - ex_r[j];
                let sx = aux(&mut q5[j], bx[j], ax[j], du_x);
                let sy = aux(&mut q6[j], by, ay, du_y);
                c.x * (kx[j] * du_x - sx) - c.y * (ky * du_y - sy)
            });
            let last = nx - 1;
            // The `j = nx − 1` column is inside the xmax slab. These closures
            // are what updates its x auxiliary (the neighbour past the array is 0).
            step_h!(hy_r, last, nx, my, |j| {
                let du_z = ex_kp[j] - ex_r[j];
                let du_x = 0.0 - ez_r[j];
                let sz = aux(&mut q3[j], bz, az, du_z);
                let sx = aux(&mut q4[j], bx[j], ax[j], du_x);
                c.z * (kz * du_z - sz) - c.x * (kx[j] * du_x - sx)
            });
            step_h!(hz_r, last, nx, mz, |j| {
                let du_x = 0.0 - ey_r[j];
                let du_y = ex_ip[j] - ex_r[j];
                let sx = aux(&mut q5[j], bx[j], ax[j], du_x);
                let sy = aux(&mut q6[j], by, ay, du_y);
                c.x * (kx[j] * du_x - sx) - c.y * (ky * du_y - sy)
            });
        }
    }
}

/// `E^{n+1} = Ca E^n + Cb curl H^{n+1/2}`, backward differences of H. On a
/// vacuum grid `Ca = 1` and `Cb = dt/ε0`. Each difference is `kinv·du − q`
/// with the E (node) profiles. `inline(never)` for the same reason as
/// [`update_h`]: the curl body must stay inside these loops.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn update_e(
    d: Dims,
    c: Coeffs,
    m: ECoefs,
    p: Axes,
    q: &mut [Vec<f64>; 6],
    hx: &[f64],
    hy: &[f64],
    hz: &[f64],
    ex: &mut [f64],
    ey: &mut [f64],
    ez: &mut [f64],
) {
    let nx = d.nx;
    let full = q[0].len() == nx * d.ny * d.nz;
    let (kx, bx, ax) = (&p.x.kinv[..nx], &p.x.b[..nx], &p.x.a[..nx]);
    let [q_ex_y, q_ex_z, q_ey_z, q_ey_x, q_ez_x, q_ez_y] = q;
    for k in 0..d.nz {
        let (kz, bz, az) = (p.z.kinv[k], p.z.b[k], p.z.a[k]);
        for i in 0..d.ny {
            let (ky, by, ay) = (p.y.kinv[i], p.y.b[i], p.y.a[i]);
            let r = d.row(i, k);
            let pr = if full { r } else { 0 };
            let hx_r = &hx[r..r + nx];
            let hy_r = &hy[r..r + nx];
            let hz_r = &hz[r..r + nx];

            if i >= 1 && k >= 1 {
                let rim = d.row(i - 1, k);
                let rkm = d.row(i, k - 1);
                let hz_im = &hz[rim..rim + nx];
                let hy_km = &hy[rkm..rkm + nx];
                let (q7, q8) = (&mut q_ex_y[pr..pr + nx], &mut q_ex_z[pr..pr + nx]);
                step_e!(&mut ex[r..r + nx], 0, m.row(0, r, nx), |j| {
                    let du_y = hz_r[j] - hz_im[j];
                    let du_z = hy_r[j] - hy_km[j];
                    let sy = aux(&mut q7[j], by, ay, du_y);
                    let sz = aux(&mut q8[j], bz, az, du_z);
                    c.y * (ky * du_y - sy) - c.z * (kz * du_z - sz)
                });
            }
            if k >= 1 {
                let rkm = d.row(i, k - 1);
                let hx_km = &hx[rkm..rkm + nx];
                let (q9, q10) = (&mut q_ey_z[pr..pr + nx], &mut q_ey_x[pr..pr + nx]);
                step_e!(&mut ey[r..r + nx], 1, m.row(1, r, nx), |j| {
                    let du_z = hx_r[j] - hx_km[j];
                    let du_x = hz_r[j] - hz_r[j - 1];
                    let sz = aux(&mut q9[j], bz, az, du_z);
                    let sx = aux(&mut q10[j], bx[j], ax[j], du_x);
                    c.z * (kz * du_z - sz) - c.x * (kx[j] * du_x - sx)
                });
            }
            if i >= 1 {
                let rim = d.row(i - 1, k);
                let hx_im = &hx[rim..rim + nx];
                let (q11, q12) = (&mut q_ez_x[pr..pr + nx], &mut q_ez_y[pr..pr + nx]);
                step_e!(&mut ez[r..r + nx], 1, m.row(2, r, nx), |j| {
                    let du_x = hy_r[j] - hy_r[j - 1];
                    let du_y = hx_r[j] - hx_im[j];
                    let sx = aux(&mut q11[j], bx[j], ax[j], du_x);
                    let sy = aux(&mut q12[j], by, ay, du_y);
                    c.x * (kx[j] * du_x - sx) - c.y * (ky * du_y - sy)
                });
            }
        }
    }
}

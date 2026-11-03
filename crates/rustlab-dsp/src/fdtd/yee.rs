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

#[inline(always)]
fn step_e(e: &mut [f64], lo: usize, coef: ERow, curl: impl Fn(usize) -> f64) {
    let n = e.len();
    match coef {
        ERow::Vacuum => {
            for (x, j) in e[lo..].iter_mut().zip(lo..) {
                *x += curl(j);
            }
        }
        ERow::Lossless(cb) => {
            let cb = &cb[..n];
            for j in lo..n {
                e[j] += cb[j] * curl(j);
            }
        }
        ERow::Lossy(ca, cb) => {
            let (ca, cb) = (&ca[..n], &cb[..n]);
            for j in lo..n {
                e[j] = ca[j] * e[j] + cb[j] * curl(j);
            }
        }
    }
}

#[inline(always)]
fn step_h(h: &mut [f64], lo: usize, hi: usize, coef: HRow, curl: impl Fn(usize) -> f64) {
    let h = &mut h[..hi];
    match coef {
        HRow::Vacuum => {
            for (x, j) in h[lo..].iter_mut().zip(lo..) {
                *x -= curl(j);
            }
        }
        HRow::Scaled(db) => {
            let db = &db[..hi];
            for j in lo..hi {
                h[j] -= db[j] * curl(j);
            }
        }
    }
}

/// `H^{n+1/2} = H^{n-1/2} - (dt/μ) curl E^n`, forward differences of E.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_h(
    d: Dims,
    c: Coeffs,
    m: HCoefs,
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
    for k in 0..d.nz {
        for i in 0..d.ny {
            let r = d.row(i, k);
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

            step_h(hx_r, 0, nx, mx, |j| {
                c.y * (ez_ip[j] - ez_r[j]) - c.z * (ey_kp[j] - ey_r[j])
            });
            step_h(hy_r, 0, nx - 1, my, |j| {
                c.z * (ex_kp[j] - ex_r[j]) - c.x * (ez_r[j + 1] - ez_r[j])
            });
            step_h(hz_r, 0, nx - 1, mz, |j| {
                c.x * (ey_r[j + 1] - ey_r[j]) - c.y * (ex_ip[j] - ex_r[j])
            });
            let last = nx - 1;
            step_h(hy_r, last, nx, my, |j| {
                c.z * (ex_kp[j] - ex_r[j]) - c.x * (0.0 - ez_r[j])
            });
            step_h(hz_r, last, nx, mz, |j| {
                c.x * (0.0 - ey_r[j]) - c.y * (ex_ip[j] - ex_r[j])
            });
        }
    }
}

/// `E^{n+1} = Ca E^n + Cb curl H^{n+1/2}`, backward differences of H. On a
/// vacuum grid `Ca = 1` and `Cb = dt/ε0`.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_e(
    d: Dims,
    c: Coeffs,
    m: ECoefs,
    hx: &[f64],
    hy: &[f64],
    hz: &[f64],
    ex: &mut [f64],
    ey: &mut [f64],
    ez: &mut [f64],
) {
    let nx = d.nx;
    for k in 0..d.nz {
        for i in 0..d.ny {
            let r = d.row(i, k);
            let hx_r = &hx[r..r + nx];
            let hy_r = &hy[r..r + nx];
            let hz_r = &hz[r..r + nx];

            if i >= 1 && k >= 1 {
                let rim = d.row(i - 1, k);
                let rkm = d.row(i, k - 1);
                let hz_im = &hz[rim..rim + nx];
                let hy_km = &hy[rkm..rkm + nx];
                step_e(&mut ex[r..r + nx], 0, m.row(0, r, nx), |j| {
                    c.y * (hz_r[j] - hz_im[j]) - c.z * (hy_r[j] - hy_km[j])
                });
            }
            if k >= 1 {
                let rkm = d.row(i, k - 1);
                let hx_km = &hx[rkm..rkm + nx];
                step_e(&mut ey[r..r + nx], 1, m.row(1, r, nx), |j| {
                    c.z * (hx_r[j] - hx_km[j]) - c.x * (hz_r[j] - hz_r[j - 1])
                });
            }
            if i >= 1 {
                let rim = d.row(i - 1, k);
                let hx_im = &hx[rim..rim + nx];
                step_e(&mut ez[r..r + nx], 1, m.row(2, r, nx), |j| {
                    c.x * (hy_r[j] - hy_r[j - 1]) - c.y * (hx_r[j] - hx_im[j])
                });
            }
        }
    }
}

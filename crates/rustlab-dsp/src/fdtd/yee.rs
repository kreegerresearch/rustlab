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
//! CPML is a second pass ([`correct_h`], [`correct_e`]), not part of this
//! curl. The interior update is the Phase 2 kernel over the whole grid. The
//! correction runs only on cells inside an enabled slab, after that update,
//! and only when a face is on. If every axis is `kinv = 1` and `a = 0`
//! (`sigma_factor = 0`, including `alpha_max > 0`), the pass returns without
//! writing, so those fields stay the Phase 2 bits, signed zero included.

use super::cpml::{
    in_slab, Profile, EX_Y, EX_Z, EY_X, EY_Z, EZ_X, EZ_Y, HX_Y, HX_Z, HY_X, HY_Z, HZ_X, HZ_Y,
};
use rayon::prelude::*;
use std::cell::Cell;

/// Cells at or above this count use the shared rayon pool. Below it the
/// step stays the serial loop, including the call overhead. Thread-local
/// so a test can force the parallel driver without racing other tests.
const DEFAULT_PAR_CELLS: usize = 32_768;

thread_local! {
    static PAR_CELLS: Cell<usize> = const { Cell::new(DEFAULT_PAR_CELLS) };
    static PAR_HITS: Cell<u64> = const { Cell::new(0) };
    /// `0` uses [`rows_per_task`]'s formula. A test sets 2 so a step is many
    /// chunks and a wrong offset on a later chunk fails the bit compare.
    static ROWS_PER_TASK: Cell<usize> = const { Cell::new(0) };
    static PAR_CHUNKS: Cell<u64> = const { Cell::new(0) };
    /// Times the CPML correction took the parallel row split. Stays 0 while
    /// `correct_h` / `correct_e` are the serial loops.
    static PAR_CPML: Cell<u64> = const { Cell::new(0) };
}

pub(super) fn par_cells_threshold() -> usize {
    PAR_CELLS.with(|c| c.get())
}

#[cfg(test)]
pub(super) fn default_par_cells() -> usize {
    DEFAULT_PAR_CELLS
}

/// Test override on this thread. `0` forces the parallel driver on every grid.
#[cfg(test)]
pub(super) fn set_par_cells_threshold(n: usize) {
    PAR_CELLS.with(|c| c.set(n));
}

#[cfg(test)]
pub(super) fn reset_par_hits() {
    PAR_HITS.set(0);
}

#[cfg(test)]
pub(super) fn par_hits() -> u64 {
    PAR_HITS.get()
}

#[cfg(test)]
pub(super) fn set_rows_per_task(n: usize) {
    ROWS_PER_TASK.with(|c| c.set(n));
}

#[cfg(test)]
pub(super) fn reset_par_chunks() {
    PAR_CHUNKS.set(0);
}

#[cfg(test)]
pub(super) fn par_chunks() -> u64 {
    PAR_CHUNKS.get()
}

#[cfg(test)]
pub(super) fn reset_cpml_hits() {
    PAR_CPML.set(0);
}

#[cfg(test)]
pub(super) fn cpml_hits() -> u64 {
    PAR_CPML.get()
}

fn note_parallel() {
    PAR_HITS.with(|c| c.set(c.get().saturating_add(1)));
}

fn note_chunks(n: usize) {
    PAR_CHUNKS.with(|c| c.set(c.get().saturating_add(n as u64)));
}

fn note_cpml() {
    PAR_CPML.with(|c| c.set(c.get().saturating_add(1)));
}

/// Whole rows per task, at least one.
///
/// The cap is 64k cells: 16k left the 4-thread reference step at 1.82× and
/// 64k reached 1.89×. The floor is one row. Between those, the task count is
/// about four per thread so a short grid (the lesson mesh is 5 tasks at the
/// cap, and one thread then does two) still fills the pool. A test override
/// of `n > 0` replaces the formula on this thread. The chunk size does not
/// change the arithmetic.
fn rows_per_task(nx: usize, n_rows: usize) -> usize {
    let forced = ROWS_PER_TASK.with(|c| c.get());
    if forced > 0 {
        return forced;
    }
    let cap = (65_536 / nx).max(1);
    let threads = rayon::current_num_threads().max(1);
    (n_rows / (4 * threads)).max(1).min(cap)
}

fn chunk_plan(nx: usize, n_rows: usize) -> (usize, usize) {
    let rows_per = rows_per_task(nx, n_rows);
    let n_chunks = n_rows.div_ceil(rows_per);
    (rows_per, n_chunks)
}

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

/// One `(k, i)` row of the H update. The serial loop and the parallel
/// driver both call this, so a row is the same arithmetic either way.
#[allow(clippy::too_many_arguments)]
fn update_h_row(
    d: Dims,
    c: Coeffs,
    m: HCoefs,
    zero: &[f64],
    ex: &[f64],
    ey: &[f64],
    ez: &[f64],
    i: usize,
    k: usize,
    hx_r: &mut [f64],
    hy_r: &mut [f64],
    hz_r: &mut [f64],
) {
    let nx = d.nx;
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
    parallel: bool,
) {
    let nx = d.nx;
    let zero = &zero[..nx];
    let row = |i: usize, k: usize, hx_r: &mut [f64], hy_r: &mut [f64], hz_r: &mut [f64]| {
        update_h_row(d, c, m, zero, ex, ey, ez, i, k, hx_r, hy_r, hz_r);
    };
    if parallel {
        note_parallel();
        let (rows_per, n_chunks) = chunk_plan(nx, d.ny * d.nz);
        note_chunks(n_chunks);
        let chunk = nx * rows_per;
        hx.par_chunks_mut(chunk)
            .zip(hy.par_chunks_mut(chunk))
            .zip(hz.par_chunks_mut(chunk))
            .enumerate()
            .for_each(|(cidx, ((hxc, hyc), hzc))| {
                let n = hxc.len() / nx;
                let row0 = cidx * rows_per;
                for t in 0..n {
                    let row_i = row0 + t;
                    let k = row_i / d.ny;
                    let i = row_i % d.ny;
                    let o = t * nx;
                    row(
                        i,
                        k,
                        &mut hxc[o..o + nx],
                        &mut hyc[o..o + nx],
                        &mut hzc[o..o + nx],
                    );
                }
            });
    } else {
        for k in 0..d.nz {
            for i in 0..d.ny {
                let r = d.row(i, k);
                row(
                    i,
                    k,
                    &mut hx[r..r + nx],
                    &mut hy[r..r + nx],
                    &mut hz[r..r + nx],
                );
            }
        }
    }
}

/// One `(k, i)` row of the E update. Same body for the serial and parallel
/// drivers.
#[allow(clippy::too_many_arguments)]
fn update_e_row(
    d: Dims,
    c: Coeffs,
    m: ECoefs,
    hx: &[f64],
    hy: &[f64],
    hz: &[f64],
    i: usize,
    k: usize,
    ex_r: &mut [f64],
    ey_r: &mut [f64],
    ez_r: &mut [f64],
) {
    let nx = d.nx;
    let r = d.row(i, k);
    let hx_r = &hx[r..r + nx];
    let hy_r = &hy[r..r + nx];
    let hz_r = &hz[r..r + nx];

    if i >= 1 && k >= 1 {
        let rim = d.row(i - 1, k);
        let rkm = d.row(i, k - 1);
        let hz_im = &hz[rim..rim + nx];
        let hy_km = &hy[rkm..rkm + nx];
        step_e(ex_r, 0, m.row(0, r, nx), |j| {
            c.y * (hz_r[j] - hz_im[j]) - c.z * (hy_r[j] - hy_km[j])
        });
    }
    if k >= 1 {
        let rkm = d.row(i, k - 1);
        let hx_km = &hx[rkm..rkm + nx];
        step_e(ey_r, 1, m.row(1, r, nx), |j| {
            c.z * (hx_r[j] - hx_km[j]) - c.x * (hz_r[j] - hz_r[j - 1])
        });
    }
    if i >= 1 {
        let rim = d.row(i - 1, k);
        let hx_im = &hx[rim..rim + nx];
        step_e(ez_r, 1, m.row(2, r, nx), |j| {
            c.x * (hy_r[j] - hy_r[j - 1]) - c.y * (hx_r[j] - hx_im[j])
        });
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
    parallel: bool,
) {
    let nx = d.nx;
    let row = |i: usize, k: usize, ex_r: &mut [f64], ey_r: &mut [f64], ez_r: &mut [f64]| {
        update_e_row(d, c, m, hx, hy, hz, i, k, ex_r, ey_r, ez_r);
    };
    if parallel {
        note_parallel();
        let (rows_per, n_chunks) = chunk_plan(nx, d.ny * d.nz);
        note_chunks(n_chunks);
        let chunk = nx * rows_per;
        ex.par_chunks_mut(chunk)
            .zip(ey.par_chunks_mut(chunk))
            .zip(ez.par_chunks_mut(chunk))
            .enumerate()
            .for_each(|(cidx, ((exc, eyc), ezc))| {
                let n = exc.len() / nx;
                let row0 = cidx * rows_per;
                for t in 0..n {
                    let row_i = row0 + t;
                    let k = row_i / d.ny;
                    let i = row_i % d.ny;
                    let o = t * nx;
                    row(
                        i,
                        k,
                        &mut exc[o..o + nx],
                        &mut eyc[o..o + nx],
                        &mut ezc[o..o + nx],
                    );
                }
            });
    } else {
        for k in 0..d.nz {
            for i in 0..d.ny {
                let r = d.row(i, k);
                row(
                    i,
                    k,
                    &mut ex[r..r + nx],
                    &mut ey[r..r + nx],
                    &mut ez[r..r + nx],
                );
            }
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

/// `true` when the stretch term is identically zero: `kinv = 1` and `a = 0`
/// at every sample, so `q` stays `+0` and the field correction is 0.
fn axis_is_plain(p: &Profile) -> bool {
    p.kinv.iter().all(|&k| k == 1.0) && p.a.iter().all(|&a| a == 0.0)
}

/// `q := b q − a du`, and the extra curl term `scale · ((kinv − 1) du − q)`.
#[inline(always)]
fn aux_stretch(q: &mut f64, b: f64, a: f64, kinv: f64, scale: f64, du: f64) -> f64 {
    let s = b * *q - a * du;
    *q = s;
    scale * ((kinv - 1.0) * du - s)
}

/// Hx correction over one row. `Y` / `Z` are the slab flags for this `(i, k)`,
/// folded into the loop so the inner body has no per-cell branch.
#[inline(always)]
fn hx_cells<const Y: bool, const Z: bool>(
    h: &mut [f64],
    qy: &mut [f64],
    qz: &mut [f64],
    ez_ip: &[f64],
    ez_r: &[f64],
    ey_kp: &[f64],
    ey_r: &[f64],
    row: HRow,
    c: Coeffs,
    ky: f64,
    by: f64,
    ay: f64,
    kz: f64,
    bz: f64,
    az: f64,
) {
    let n = h.len();
    match row {
        HRow::Vacuum => {
            for j in 0..n {
                let ty = if Y {
                    aux_stretch(&mut qy[j], by, ay, ky, c.y, ez_ip[j] - ez_r[j])
                } else {
                    0.0
                };
                let tz = if Z {
                    aux_stretch(&mut qz[j], bz, az, kz, c.z, ey_kp[j] - ey_r[j])
                } else {
                    0.0
                };
                h[j] -= ty - tz;
            }
        }
        HRow::Scaled(db) => {
            for j in 0..n {
                let ty = if Y {
                    aux_stretch(&mut qy[j], by, ay, ky, c.y, ez_ip[j] - ez_r[j])
                } else {
                    0.0
                };
                let tz = if Z {
                    aux_stretch(&mut qz[j], bz, az, kz, c.z, ey_kp[j] - ey_r[j])
                } else {
                    0.0
                };
                h[j] -= db[j] * (ty - tz);
            }
        }
    }
}

/// Hy correction for `j` in `j0..j1`. `LAST` uses the max-wall neighbour 0.
#[inline(always)]
fn hy_cells<const X: bool, const Z: bool, const LAST: bool>(
    h: &mut [f64],
    qz: &mut [f64],
    qx: &mut [f64],
    ex_r: &[f64],
    ex_kp: &[f64],
    ez_r: &[f64],
    row: HRow,
    c: Coeffs,
    j0: usize,
    j1: usize,
    kz: f64,
    bz: f64,
    az: f64,
    kx: &[f64],
    bx: &[f64],
    ax: &[f64],
) {
    match row {
        HRow::Vacuum => {
            for j in j0..j1 {
                let tz = if Z {
                    aux_stretch(&mut qz[j], bz, az, kz, c.z, ex_kp[j] - ex_r[j])
                } else {
                    0.0
                };
                let tx = if X {
                    let du_x = if LAST {
                        0.0 - ez_r[j]
                    } else {
                        ez_r[j + 1] - ez_r[j]
                    };
                    aux_stretch(&mut qx[j], bx[j], ax[j], kx[j], c.x, du_x)
                } else {
                    0.0
                };
                h[j] -= tz - tx;
            }
        }
        HRow::Scaled(db) => {
            for j in j0..j1 {
                let tz = if Z {
                    aux_stretch(&mut qz[j], bz, az, kz, c.z, ex_kp[j] - ex_r[j])
                } else {
                    0.0
                };
                let tx = if X {
                    let du_x = if LAST {
                        0.0 - ez_r[j]
                    } else {
                        ez_r[j + 1] - ez_r[j]
                    };
                    aux_stretch(&mut qx[j], bx[j], ax[j], kx[j], c.x, du_x)
                } else {
                    0.0
                };
                h[j] -= db[j] * (tz - tx);
            }
        }
    }
}

/// Hz correction for `j` in `j0..j1`. `LAST` uses the max-wall neighbour 0.
#[inline(always)]
fn hz_cells<const X: bool, const Y: bool, const LAST: bool>(
    h: &mut [f64],
    qx: &mut [f64],
    qy: &mut [f64],
    ey_r: &[f64],
    ex_ip: &[f64],
    ex_r: &[f64],
    row: HRow,
    c: Coeffs,
    j0: usize,
    j1: usize,
    ky: f64,
    by: f64,
    ay: f64,
    kx: &[f64],
    bx: &[f64],
    ax: &[f64],
) {
    match row {
        HRow::Vacuum => {
            for j in j0..j1 {
                let tx = if X {
                    let du_x = if LAST {
                        0.0 - ey_r[j]
                    } else {
                        ey_r[j + 1] - ey_r[j]
                    };
                    aux_stretch(&mut qx[j], bx[j], ax[j], kx[j], c.x, du_x)
                } else {
                    0.0
                };
                let ty = if Y {
                    aux_stretch(&mut qy[j], by, ay, ky, c.y, ex_ip[j] - ex_r[j])
                } else {
                    0.0
                };
                h[j] -= tx - ty;
            }
        }
        HRow::Scaled(db) => {
            for j in j0..j1 {
                let tx = if X {
                    let du_x = if LAST {
                        0.0 - ey_r[j]
                    } else {
                        ey_r[j + 1] - ey_r[j]
                    };
                    aux_stretch(&mut qx[j], bx[j], ax[j], kx[j], c.x, du_x)
                } else {
                    0.0
                };
                let ty = if Y {
                    aux_stretch(&mut qy[j], by, ay, ky, c.y, ex_ip[j] - ex_r[j])
                } else {
                    0.0
                };
                h[j] -= db[j] * (tx - ty);
            }
        }
    }
}

#[inline(always)]
fn ex_cells<const Y: bool, const Z: bool>(
    e: &mut [f64],
    qy: &mut [f64],
    qz: &mut [f64],
    hz_r: &[f64],
    hz_im: &[f64],
    hy_r: &[f64],
    hy_km: &[f64],
    row: ERow,
    c: Coeffs,
    ky: f64,
    by: f64,
    ay: f64,
    kz: f64,
    bz: f64,
    az: f64,
) {
    let n = e.len();
    match row {
        ERow::Vacuum => {
            for j in 0..n {
                let ty = if Y {
                    aux_stretch(&mut qy[j], by, ay, ky, c.y, hz_r[j] - hz_im[j])
                } else {
                    0.0
                };
                let tz = if Z {
                    aux_stretch(&mut qz[j], bz, az, kz, c.z, hy_r[j] - hy_km[j])
                } else {
                    0.0
                };
                e[j] += ty - tz;
            }
        }
        ERow::Lossless(cb) | ERow::Lossy(_, cb) => {
            for j in 0..n {
                let ty = if Y {
                    aux_stretch(&mut qy[j], by, ay, ky, c.y, hz_r[j] - hz_im[j])
                } else {
                    0.0
                };
                let tz = if Z {
                    aux_stretch(&mut qz[j], bz, az, kz, c.z, hy_r[j] - hy_km[j])
                } else {
                    0.0
                };
                e[j] += cb[j] * (ty - tz);
            }
        }
    }
}

/// Ey correction for `j` in `j0..j1` (`j >= 1`, so `j - 1` is in range).
#[inline(always)]
fn ey_cells<const X: bool, const Z: bool>(
    e: &mut [f64],
    qz: &mut [f64],
    qx: &mut [f64],
    hx_r: &[f64],
    hx_km: &[f64],
    hz_r: &[f64],
    row: ERow,
    c: Coeffs,
    j0: usize,
    j1: usize,
    kz: f64,
    bz: f64,
    az: f64,
    kx: &[f64],
    bx: &[f64],
    ax: &[f64],
) {
    match row {
        ERow::Vacuum => {
            for j in j0..j1 {
                let tz = if Z {
                    aux_stretch(&mut qz[j], bz, az, kz, c.z, hx_r[j] - hx_km[j])
                } else {
                    0.0
                };
                let tx = if X {
                    aux_stretch(&mut qx[j], bx[j], ax[j], kx[j], c.x, hz_r[j] - hz_r[j - 1])
                } else {
                    0.0
                };
                e[j] += tz - tx;
            }
        }
        ERow::Lossless(cb) | ERow::Lossy(_, cb) => {
            for j in j0..j1 {
                let tz = if Z {
                    aux_stretch(&mut qz[j], bz, az, kz, c.z, hx_r[j] - hx_km[j])
                } else {
                    0.0
                };
                let tx = if X {
                    aux_stretch(&mut qx[j], bx[j], ax[j], kx[j], c.x, hz_r[j] - hz_r[j - 1])
                } else {
                    0.0
                };
                e[j] += cb[j] * (tz - tx);
            }
        }
    }
}

/// Ez correction for `j` in `j0..j1` (`j >= 1`).
#[inline(always)]
fn ez_cells<const X: bool, const Y: bool>(
    e: &mut [f64],
    qx: &mut [f64],
    qy: &mut [f64],
    hy_r: &[f64],
    hx_r: &[f64],
    hx_im: &[f64],
    row: ERow,
    c: Coeffs,
    j0: usize,
    j1: usize,
    ky: f64,
    by: f64,
    ay: f64,
    kx: &[f64],
    bx: &[f64],
    ax: &[f64],
) {
    match row {
        ERow::Vacuum => {
            for j in j0..j1 {
                let tx = if X {
                    aux_stretch(&mut qx[j], bx[j], ax[j], kx[j], c.x, hy_r[j] - hy_r[j - 1])
                } else {
                    0.0
                };
                let ty = if Y {
                    aux_stretch(&mut qy[j], by, ay, ky, c.y, hx_r[j] - hx_im[j])
                } else {
                    0.0
                };
                e[j] += tx - ty;
            }
        }
        ERow::Lossless(cb) | ERow::Lossy(_, cb) => {
            for j in j0..j1 {
                let tx = if X {
                    aux_stretch(&mut qx[j], bx[j], ax[j], kx[j], c.x, hy_r[j] - hy_r[j - 1])
                } else {
                    0.0
                };
                let ty = if Y {
                    aux_stretch(&mut qy[j], by, ay, ky, c.y, hx_r[j] - hx_im[j])
                } else {
                    0.0
                };
                e[j] += cb[j] * (tx - ty);
            }
        }
    }
}

/// H half-node x ranges, excluding the last column (its neighbour is 0).
struct XRanges {
    lo_end: usize,
    hi_start: usize,
    last: usize,
    hi_on: bool,
}

fn h_x_ranges(nx: usize, n_lo: usize, n_hi: usize) -> XRanges {
    let last = nx - 1;
    let hi = if n_hi > 0 && n_hi <= nx {
        nx - n_hi
    } else {
        nx
    };
    XRanges {
        lo_end: n_lo.min(last),
        hi_start: hi.min(last),
        last,
        hi_on: n_hi > 0,
    }
}

/// E node x ranges. Ey and Ez start at `j = 1`.
struct ERanges {
    lo0: usize,
    lo1: usize,
    mid0: usize,
    mid1: usize,
    hi0: usize,
    hi1: usize,
}

fn e_x_ranges(nx: usize, n_lo: usize, n_hi: usize) -> ERanges {
    let hi0 = if n_hi > 0 && n_hi <= nx {
        nx - n_hi + 1
    } else {
        nx
    };
    let lo1 = n_lo.min(nx);
    ERanges {
        lo0: 1,
        lo1: lo1.max(1),
        mid0: lo1.max(1),
        mid1: hi0,
        hi0,
        hi1: nx,
    }
}

/// One task owns `rows_per` whole rows of every slice. `body` sees one
/// `(i, k)` row: nine slices of length `nx`, in the caller's order. The
/// serial loop and this split call the same row function.
#[allow(clippy::too_many_arguments)]
fn drive_rows(
    nx: usize,
    ny: usize,
    rows_per: usize,
    s0: &mut [f64],
    s1: &mut [f64],
    s2: &mut [f64],
    s3: &mut [f64],
    s4: &mut [f64],
    s5: &mut [f64],
    s6: &mut [f64],
    s7: &mut [f64],
    s8: &mut [f64],
    body: &(impl Fn(usize, usize, [&mut [f64]; 9]) + Sync),
) {
    let chunk = nx * rows_per;
    s0.par_chunks_mut(chunk)
        .zip(s1.par_chunks_mut(chunk))
        .zip(s2.par_chunks_mut(chunk))
        .zip(s3.par_chunks_mut(chunk))
        .zip(s4.par_chunks_mut(chunk))
        .zip(s5.par_chunks_mut(chunk))
        .zip(s6.par_chunks_mut(chunk))
        .zip(s7.par_chunks_mut(chunk))
        .zip(s8.par_chunks_mut(chunk))
        .enumerate()
        .for_each(|(cidx, ((((((((a, b), c), d), e), f), g), h), i_chunk))| {
            let n = a.len() / nx;
            let row0 = cidx * rows_per;
            for t in 0..n {
                let row_i = row0 + t;
                let k = row_i / ny;
                let ii = row_i % ny;
                let o = t * nx;
                let stop = o + nx;
                body(
                    ii,
                    k,
                    [
                        &mut a[o..stop],
                        &mut b[o..stop],
                        &mut c[o..stop],
                        &mut d[o..stop],
                        &mut e[o..stop],
                        &mut f[o..stop],
                        &mut g[o..stop],
                        &mut h[o..stop],
                        &mut i_chunk[o..stop],
                    ],
                );
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn correct_h_row(
    d: Dims,
    c: Coeffs,
    m: HCoefs,
    p: Axes,
    faces: [usize; 6],
    zero: &[f64],
    ex: &[f64],
    ey: &[f64],
    ez: &[f64],
    i: usize,
    k: usize,
    rows: [&mut [f64]; 9],
) {
    let nx = d.nx;
    let ny = d.ny;
    let nz = d.nz;
    let [hx_r, hy_r, hz_r, q_hx_y, q_hx_z, q_hy_z, q_hy_x, q_hz_x, q_hz_y] = rows;
    let zero = &zero[..nx];
    let (n_lo_x, n_hi_x) = (faces[0], faces[1]);
    let (n_lo_y, n_hi_y) = (faces[2], faces[3]);
    let (n_lo_z, n_hi_z) = (faces[4], faces[5]);
    let any_x = n_lo_x > 0 || n_hi_x > 0;
    let xr = h_x_ranges(nx, n_lo_x, n_hi_x);
    let (kx, bx, ax) = (&p.x.kinv[..nx], &p.x.b[..nx], &p.x.a[..nx]);
    let z_on = in_slab(k, nz, n_lo_z, n_hi_z, true);
    let (kz, bz, az) = (p.z.kinv[k], p.z.b[k], p.z.a[k]);
    if !z_on && !any_x && n_lo_y == 0 && n_hi_y == 0 {
        return;
    }
    let y_on = in_slab(i, ny, n_lo_y, n_hi_y, true);
    if !y_on && !z_on && !any_x {
        return;
    }
    let (ky, by, ay) = (p.y.kinv[i], p.y.b[i], p.y.a[i]);
    let r = d.row(i, k);
    let ex_r = &ex[r..r + nx];
    let ey_r = &ey[r..r + nx];
    let ez_r = &ez[r..r + nx];
    let (ex_ip, ez_ip) = if i + 1 < ny {
        let r2 = d.row(i + 1, k);
        (&ex[r2..r2 + nx], &ez[r2..r2 + nx])
    } else {
        (zero, zero)
    };
    let (ex_kp, ey_kp) = if k + 1 < nz {
        let r2 = d.row(i, k + 1);
        (&ex[r2..r2 + nx], &ey[r2..r2 + nx])
    } else {
        (zero, zero)
    };
    let (mx, my, mz) = (m.row(0, r, nx), m.row(1, r, nx), m.row(2, r, nx));

    if y_on || z_on {
        let qy = q_hx_y;
        let qz = q_hx_z;
        let hrow = hx_r;
        if y_on && z_on {
            hx_cells::<true, true>(
                hrow, qy, qz, ez_ip, ez_r, ey_kp, ey_r, mx, c, ky, by, ay, kz, bz, az,
            );
        } else if y_on {
            hx_cells::<true, false>(
                hrow, qy, qz, ez_ip, ez_r, ey_kp, ey_r, mx, c, ky, by, ay, kz, bz, az,
            );
        } else {
            hx_cells::<false, true>(
                hrow, qy, qz, ez_ip, ez_r, ey_kp, ey_r, mx, c, ky, by, ay, kz, bz, az,
            );
        }
    }

    if z_on || any_x {
        let qz = q_hy_z;
        let qx = q_hy_x;
        let hrow = hy_r;
        if z_on {
            hy_cells::<true, true, false>(
                hrow, qz, qx, ex_r, ex_kp, ez_r, my, c, 0, xr.lo_end, kz, bz, az, kx, bx, ax,
            );
            hy_cells::<false, true, false>(
                hrow,
                qz,
                qx,
                ex_r,
                ex_kp,
                ez_r,
                my,
                c,
                xr.lo_end,
                xr.hi_start,
                kz,
                bz,
                az,
                kx,
                bx,
                ax,
            );
            hy_cells::<true, true, false>(
                hrow,
                qz,
                qx,
                ex_r,
                ex_kp,
                ez_r,
                my,
                c,
                xr.hi_start,
                xr.last,
                kz,
                bz,
                az,
                kx,
                bx,
                ax,
            );
            if xr.hi_on {
                hy_cells::<true, true, true>(
                    hrow, qz, qx, ex_r, ex_kp, ez_r, my, c, xr.last, nx, kz, bz, az, kx, bx, ax,
                );
            } else {
                hy_cells::<false, true, true>(
                    hrow, qz, qx, ex_r, ex_kp, ez_r, my, c, xr.last, nx, kz, bz, az, kx, bx, ax,
                );
            }
        } else {
            hy_cells::<true, false, false>(
                hrow, qz, qx, ex_r, ex_kp, ez_r, my, c, 0, xr.lo_end, kz, bz, az, kx, bx, ax,
            );
            hy_cells::<true, false, false>(
                hrow,
                qz,
                qx,
                ex_r,
                ex_kp,
                ez_r,
                my,
                c,
                xr.hi_start,
                xr.last,
                kz,
                bz,
                az,
                kx,
                bx,
                ax,
            );
            if xr.hi_on {
                hy_cells::<true, false, true>(
                    hrow, qz, qx, ex_r, ex_kp, ez_r, my, c, xr.last, nx, kz, bz, az, kx, bx, ax,
                );
            }
        }
    }

    if y_on || any_x {
        let qx = q_hz_x;
        let qy = q_hz_y;
        let hrow = hz_r;
        if y_on && any_x {
            hz_cells::<true, true, false>(
                hrow, qx, qy, ey_r, ex_ip, ex_r, mz, c, 0, xr.lo_end, ky, by, ay, kx, bx, ax,
            );
            hz_cells::<false, true, false>(
                hrow,
                qx,
                qy,
                ey_r,
                ex_ip,
                ex_r,
                mz,
                c,
                xr.lo_end,
                xr.hi_start,
                ky,
                by,
                ay,
                kx,
                bx,
                ax,
            );
            hz_cells::<true, true, false>(
                hrow,
                qx,
                qy,
                ey_r,
                ex_ip,
                ex_r,
                mz,
                c,
                xr.hi_start,
                xr.last,
                ky,
                by,
                ay,
                kx,
                bx,
                ax,
            );
            if xr.hi_on {
                hz_cells::<true, true, true>(
                    hrow, qx, qy, ey_r, ex_ip, ex_r, mz, c, xr.last, nx, ky, by, ay, kx, bx, ax,
                );
            } else {
                hz_cells::<false, true, true>(
                    hrow, qx, qy, ey_r, ex_ip, ex_r, mz, c, xr.last, nx, ky, by, ay, kx, bx, ax,
                );
            }
        } else if y_on {
            hz_cells::<false, true, false>(
                hrow, qx, qy, ey_r, ex_ip, ex_r, mz, c, 0, xr.last, ky, by, ay, kx, bx, ax,
            );
            hz_cells::<false, true, true>(
                hrow, qx, qy, ey_r, ex_ip, ex_r, mz, c, xr.last, nx, ky, by, ay, kx, bx, ax,
            );
        } else {
            hz_cells::<true, false, false>(
                hrow, qx, qy, ey_r, ex_ip, ex_r, mz, c, 0, xr.lo_end, ky, by, ay, kx, bx, ax,
            );
            hz_cells::<true, false, false>(
                hrow,
                qx,
                qy,
                ey_r,
                ex_ip,
                ex_r,
                mz,
                c,
                xr.hi_start,
                xr.last,
                ky,
                by,
                ay,
                kx,
                bx,
                ax,
            );
            if xr.hi_on {
                hz_cells::<true, false, true>(
                    hrow, qx, qy, ey_r, ex_ip, ex_r, mz, c, xr.last, nx, ky, by, ay, kx, bx, ax,
                );
            }
        }
    }
}

/// CPML correction after [`update_h`]. `faces` is `[xmin, xmax, ymin, ymax, zmin, zmax]`.
/// Full `q` arrays, one per derivative (`12 · ncell · 8` bytes when a face is
/// on). `inline(never)` so this body cannot land inside [`update_h`].
/// The parallel path runs the same row as the serial loop.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn correct_h(
    d: Dims,
    c: Coeffs,
    m: HCoefs,
    p: Axes,
    faces: [usize; 6],
    q: &mut [Vec<f64>; 6],
    zero: &[f64],
    ex: &[f64],
    ey: &[f64],
    ez: &[f64],
    hx: &mut [f64],
    hy: &mut [f64],
    hz: &mut [f64],
    parallel: bool,
) {
    if axis_is_plain(p.x) && axis_is_plain(p.y) && axis_is_plain(p.z) {
        return;
    }
    let nx = d.nx;
    let ny = d.ny;
    let nz = d.nz;
    debug_assert_eq!(q[0].len(), nx * ny * nz);
    let [q_hx_y, q_hx_z, q_hy_z, q_hy_x, q_hz_x, q_hz_y] = q;
    let go = |i: usize, k: usize, rows: [&mut [f64]; 9]| {
        correct_h_row(d, c, m, p, faces, zero, ex, ey, ez, i, k, rows);
    };
    if parallel {
        note_cpml();
        let (rows_per, n_chunks) = chunk_plan(nx, ny * nz);
        note_chunks(n_chunks);
        drive_rows(
            nx, ny, rows_per, hx, hy, hz, q_hx_y, q_hx_z, q_hy_z, q_hy_x, q_hz_x, q_hz_y, &go,
        );
    } else {
        for k in 0..nz {
            for i in 0..ny {
                let r = d.row(i, k);
                go(
                    i,
                    k,
                    [
                        &mut hx[r..r + nx],
                        &mut hy[r..r + nx],
                        &mut hz[r..r + nx],
                        &mut q_hx_y[r..r + nx],
                        &mut q_hx_z[r..r + nx],
                        &mut q_hy_z[r..r + nx],
                        &mut q_hy_x[r..r + nx],
                        &mut q_hz_x[r..r + nx],
                        &mut q_hz_y[r..r + nx],
                    ],
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn correct_e_row(
    d: Dims,
    c: Coeffs,
    m: ECoefs,
    p: Axes,
    faces: [usize; 6],
    hx: &[f64],
    hy: &[f64],
    hz: &[f64],
    i: usize,
    k: usize,
    rows: [&mut [f64]; 9],
) {
    let nx = d.nx;
    let ny = d.ny;
    let nz = d.nz;
    let [ex_r, ey_r, ez_r, q_ex_y, q_ex_z, q_ey_z, q_ey_x, q_ez_x, q_ez_y] = rows;
    let (n_lo_x, n_hi_x) = (faces[0], faces[1]);
    let (n_lo_y, n_hi_y) = (faces[2], faces[3]);
    let (n_lo_z, n_hi_z) = (faces[4], faces[5]);
    let any_x = n_lo_x > 0 || n_hi_x > 0;
    let xr = e_x_ranges(nx, n_lo_x, n_hi_x);
    let (kx, bx, ax) = (&p.x.kinv[..nx], &p.x.b[..nx], &p.x.a[..nx]);
    let z_on = in_slab(k, nz, n_lo_z, n_hi_z, false);
    let (kz, bz, az) = (p.z.kinv[k], p.z.b[k], p.z.a[k]);
    let y_on = in_slab(i, ny, n_lo_y, n_hi_y, false);
    if !y_on && !z_on && !any_x {
        return;
    }
    let (ky, by, ay) = (p.y.kinv[i], p.y.b[i], p.y.a[i]);
    let r = d.row(i, k);
    let hx_r = &hx[r..r + nx];
    let hy_r = &hy[r..r + nx];
    let hz_r = &hz[r..r + nx];

    if i >= 1 && k >= 1 && (y_on || z_on) {
        let rim = d.row(i - 1, k);
        let rkm = d.row(i, k - 1);
        let hz_im = &hz[rim..rim + nx];
        let hy_km = &hy[rkm..rkm + nx];
        let qy = q_ex_y;
        let qz = q_ex_z;
        let erow = ex_r;
        let coef = m.row(0, r, nx);
        if y_on && z_on {
            ex_cells::<true, true>(
                erow, qy, qz, hz_r, hz_im, hy_r, hy_km, coef, c, ky, by, ay, kz, bz, az,
            );
        } else if y_on {
            ex_cells::<true, false>(
                erow, qy, qz, hz_r, hz_im, hy_r, hy_km, coef, c, ky, by, ay, kz, bz, az,
            );
        } else {
            ex_cells::<false, true>(
                erow, qy, qz, hz_r, hz_im, hy_r, hy_km, coef, c, ky, by, ay, kz, bz, az,
            );
        }
    }

    if k >= 1 && (z_on || any_x) {
        let rkm = d.row(i, k - 1);
        let hx_km = &hx[rkm..rkm + nx];
        let qz = q_ey_z;
        let qx = q_ey_x;
        let erow = ey_r;
        let coef = m.row(1, r, nx);
        if z_on && any_x {
            ey_cells::<true, true>(
                erow, qz, qx, hx_r, hx_km, hz_r, coef, c, xr.lo0, xr.lo1, kz, bz, az, kx, bx, ax,
            );
            ey_cells::<false, true>(
                erow, qz, qx, hx_r, hx_km, hz_r, coef, c, xr.mid0, xr.mid1, kz, bz, az, kx, bx, ax,
            );
            ey_cells::<true, true>(
                erow, qz, qx, hx_r, hx_km, hz_r, coef, c, xr.hi0, xr.hi1, kz, bz, az, kx, bx, ax,
            );
        } else if z_on {
            ey_cells::<false, true>(
                erow, qz, qx, hx_r, hx_km, hz_r, coef, c, 1, nx, kz, bz, az, kx, bx, ax,
            );
        } else {
            ey_cells::<true, false>(
                erow, qz, qx, hx_r, hx_km, hz_r, coef, c, xr.lo0, xr.lo1, kz, bz, az, kx, bx, ax,
            );
            ey_cells::<true, false>(
                erow, qz, qx, hx_r, hx_km, hz_r, coef, c, xr.hi0, xr.hi1, kz, bz, az, kx, bx, ax,
            );
        }
    }

    if i >= 1 && (y_on || any_x) {
        let rim = d.row(i - 1, k);
        let hx_im = &hx[rim..rim + nx];
        let qx = q_ez_x;
        let qy = q_ez_y;
        let erow = ez_r;
        let coef = m.row(2, r, nx);
        if y_on && any_x {
            ez_cells::<true, true>(
                erow, qx, qy, hy_r, hx_r, hx_im, coef, c, xr.lo0, xr.lo1, ky, by, ay, kx, bx, ax,
            );
            ez_cells::<false, true>(
                erow, qx, qy, hy_r, hx_r, hx_im, coef, c, xr.mid0, xr.mid1, ky, by, ay, kx, bx, ax,
            );
            ez_cells::<true, true>(
                erow, qx, qy, hy_r, hx_r, hx_im, coef, c, xr.hi0, xr.hi1, ky, by, ay, kx, bx, ax,
            );
        } else if y_on {
            ez_cells::<false, true>(
                erow, qx, qy, hy_r, hx_r, hx_im, coef, c, 1, nx, ky, by, ay, kx, bx, ax,
            );
        } else {
            ez_cells::<true, false>(
                erow, qx, qy, hy_r, hx_r, hx_im, coef, c, xr.lo0, xr.lo1, ky, by, ay, kx, bx, ax,
            );
            ez_cells::<true, false>(
                erow, qx, qy, hy_r, hx_r, hx_im, coef, c, xr.hi0, xr.hi1, ky, by, ay, kx, bx, ax,
            );
        }
    }
}

/// CPML correction after [`update_e`]. Same slab rule as [`correct_h`].
/// Node sampling (`half = false`). Tangential E on the min walls is not
/// corrected: the Phase 2 update never wrote it. The parallel path runs the
/// same row as the serial loop.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn correct_e(
    d: Dims,
    c: Coeffs,
    m: ECoefs,
    p: Axes,
    faces: [usize; 6],
    q: &mut [Vec<f64>; 6],
    hx: &[f64],
    hy: &[f64],
    hz: &[f64],
    ex: &mut [f64],
    ey: &mut [f64],
    ez: &mut [f64],
    parallel: bool,
) {
    if axis_is_plain(p.x) && axis_is_plain(p.y) && axis_is_plain(p.z) {
        return;
    }
    let nx = d.nx;
    let ny = d.ny;
    let nz = d.nz;
    debug_assert_eq!(q[0].len(), nx * ny * nz);
    let [q_ex_y, q_ex_z, q_ey_z, q_ey_x, q_ez_x, q_ez_y] = q;
    let go = |i: usize, k: usize, rows: [&mut [f64]; 9]| {
        correct_e_row(d, c, m, p, faces, hx, hy, hz, i, k, rows);
    };
    if parallel {
        note_cpml();
        let (rows_per, n_chunks) = chunk_plan(nx, ny * nz);
        note_chunks(n_chunks);
        drive_rows(
            nx, ny, rows_per, ex, ey, ez, q_ex_y, q_ex_z, q_ey_z, q_ey_x, q_ez_x, q_ez_y, &go,
        );
    } else {
        for k in 0..nz {
            for i in 0..ny {
                let r = d.row(i, k);
                go(
                    i,
                    k,
                    [
                        &mut ex[r..r + nx],
                        &mut ey[r..r + nx],
                        &mut ez[r..r + nx],
                        &mut q_ex_y[r..r + nx],
                        &mut q_ex_z[r..r + nx],
                        &mut q_ey_z[r..r + nx],
                        &mut q_ey_x[r..r + nx],
                        &mut q_ez_x[r..r + nx],
                        &mut q_ez_y[r..r + nx],
                    ],
                );
            }
        }
    }
}

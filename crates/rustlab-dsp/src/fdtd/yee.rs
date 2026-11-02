//! Vacuum Yee curl updates on x-fastest slices.
//!
//! Every array is indexed `(k * ny + i) * nx + j`, so one `(k, i)` row is a
//! contiguous run of `nx` values and the inner loop over `j` is unit stride.
//! A neighbour past the last index along an axis reads as 0, which closes the
//! max walls (tangential E on `x = nx·dx`, `y = ny·dy`, `z = nz·dz` is 0).
//! Tangential E on the min walls (`Ex`, `Ez` at `i = 0`; `Ey`, `Ez` at
//! `j = 0`; `Ex`, `Ey` at `k = 0`) is never updated and stays 0.

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

/// `H^{n+1/2} = H^{n-1/2} - (dt/μ0) curl E^n`, forward differences of E.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_h(
    d: Dims,
    c: Coeffs,
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

            for j in 0..nx {
                hx_r[j] -= c.y * (ez_ip[j] - ez_r[j]) - c.z * (ey_kp[j] - ey_r[j]);
            }
            for j in 0..nx - 1 {
                hy_r[j] -= c.z * (ex_kp[j] - ex_r[j]) - c.x * (ez_r[j + 1] - ez_r[j]);
                hz_r[j] -= c.x * (ey_r[j + 1] - ey_r[j]) - c.y * (ex_ip[j] - ex_r[j]);
            }
            let j = nx - 1;
            hy_r[j] -= c.z * (ex_kp[j] - ex_r[j]) - c.x * (0.0 - ez_r[j]);
            hz_r[j] -= c.x * (0.0 - ey_r[j]) - c.y * (ex_ip[j] - ex_r[j]);
        }
    }
}

/// `E^{n+1} = E^n + (dt/ε0) curl H^{n+1/2}`, backward differences of H.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_e(
    d: Dims,
    c: Coeffs,
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
                let ex_r = &mut ex[r..r + nx];
                for j in 0..nx {
                    ex_r[j] += c.y * (hz_r[j] - hz_im[j]) - c.z * (hy_r[j] - hy_km[j]);
                }
            }
            if k >= 1 {
                let rkm = d.row(i, k - 1);
                let hx_km = &hx[rkm..rkm + nx];
                let ey_r = &mut ey[r..r + nx];
                for j in 1..nx {
                    ey_r[j] += c.z * (hx_r[j] - hx_km[j]) - c.x * (hz_r[j] - hz_r[j - 1]);
                }
            }
            if i >= 1 {
                let rim = d.row(i - 1, k);
                let hx_im = &hx[rim..rim + nx];
                let ez_r = &mut ez[r..r + nx];
                for j in 1..nx {
                    ez_r[j] += c.x * (hy_r[j] - hy_r[j - 1]) - c.y * (hx_r[j] - hx_im[j]);
                }
            }
        }
    }
}

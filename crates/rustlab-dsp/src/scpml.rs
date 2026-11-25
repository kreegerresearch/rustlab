//! Frequency-domain stretched-coordinate PML (SC-PML) operator in 2-D.
//!
//! [`laplacian_scpml_2d`] assembles
//!
//! ```text
//! (1/s_x^e) ∂x (1/s_x^h) ∂x  +  (1/s_y^e) ∂y (1/s_y^h) ∂y
//! ```
//!
//! on the cell grid of `laplacian_2d` (rows index y, columns index x,
//! `k = j*ny + i`). `s^e` is the stretch at each cell (where the unknown
//! lives) and `s^h` the stretch at the face between cell `j` and `j + 1`
//! (where the derivative lives), as in Rumpf, *Electromagnetic and Photonic
//! Simulation for the Beginner* ch. 5, and Taflove & Hagness ch. 7. Adding
//! `k0² ε` on the diagonal gives the TMz Helmholtz operator with a PML on
//! every face whose stretch is not 1.
//!
//! This is the time-harmonic operator. The time-domain absorber is
//! `fdtd_cpml`, which does not use it.

use crate::error::DspError;
use crate::laplacian::{check_min, check_pos, flush_row, BoundaryCondition};
use num_complex::Complex;
use rustlab_core::{SparseMat, C64};

fn check_stretch(name: &str, label: &str, s: &[C64]) -> Result<(), DspError> {
    for (k, v) in s.iter().enumerate() {
        if !v.re.is_finite() || !v.im.is_finite() {
            return Err(DspError::InvalidParameter(format!(
                "{name}: {label}({}) must be finite, got {v}",
                k + 1
            )));
        }
        if v.re == 0.0 && v.im == 0.0 {
            return Err(DspError::InvalidParameter(format!(
                "{name}: {label}({}) must be nonzero",
                k + 1
            )));
        }
    }
    Ok(())
}

fn check_pair(name: &str, axis: char, e: &[C64], h: &[C64]) -> Result<(), DspError> {
    if h.len() != e.len() {
        return Err(DspError::InvalidParameter(format!(
            "{name}: s{axis}_h has {} entries but s{axis}_e has {} (one face per cell)",
            h.len(),
            e.len()
        )));
    }
    check_min(name, &format!("n{axis} (length of s{axis}_e)"), e.len(), 2)?;
    check_stretch(name, &format!("s{axis}_e"), e)?;
    check_stretch(name, &format!("s{axis}_h"), h)
}

/// One axis of the stencil at cell `c` of `n`: the coefficients toward the
/// lower and upper neighbours, each with the neighbour's offset in cells
/// (`None` when the face has no neighbour, i.e. a Dirichlet wall).
struct AxisRow {
    lower: Option<(isize, C64)>,
    upper: Option<(isize, C64)>,
    diag: C64,
}

fn axis_row(c: usize, s_e: &[C64], s_h: &[C64], inv_h2: f64, bc: BoundaryCondition) -> AxisRow {
    let n = s_e.len();
    let num = Complex::new(inv_h2, 0.0);
    let coef = |face: C64| num / (s_e[c] * face);
    let zero = Complex::new(0.0, 0.0);
    let (mut lower, mut upper) = (None, None);
    let (mut d_lo, mut d_up) = (zero, zero);

    if c > 0 {
        let a = coef(s_h[c - 1]);
        lower = Some((-1, a));
        d_lo = a;
    } else {
        match bc {
            BoundaryCondition::Dirichlet => d_lo = coef(s_e[c]),
            BoundaryCondition::Neumann => {}
            BoundaryCondition::Periodic => {
                let a = coef(s_h[n - 1]);
                lower = Some((n as isize - 1, a));
                d_lo = a;
            }
        }
    }
    if c + 1 < n {
        let a = coef(s_h[c]);
        upper = Some((1, a));
        d_up = a;
    } else {
        match bc {
            BoundaryCondition::Dirichlet => d_up = coef(s_e[c]),
            BoundaryCondition::Neumann => {}
            BoundaryCondition::Periodic => {
                let a = coef(s_h[c]);
                upper = Some((-(n as isize - 1), a));
                d_up = a;
            }
        }
    }
    AxisRow {
        lower,
        upper,
        diag: d_lo + d_up,
    }
}

/// 2-D SC-PML operator `(1/s_x^e)∂x(1/s_x^h)∂x + (1/s_y^e)∂y(1/s_y^h)∂y`.
///
/// `sx_e` / `sx_h` have one entry per column (`nx`), `sy_e` / `sy_h` one
/// per row (`ny`). `s_h(j)` is the stretch on the face between cells `j`
/// and `j + 1`. Row `k = j*ny + i` is
///
/// ```text
/// A(k, k ± ny) = 1 / (sx_e(j) · sx_h(face) · dx²)
/// A(k, k ± 1)  = 1 / (sy_e(i) · sy_h(face) · dy²)
/// A(k, k)      = −(sum of the four face coefficients)
/// ```
///
/// Boundary faces:
/// - `Neumann` drops the outer face from the off-diagonal and the diagonal
///   (zero flux). This is the assembly of `rustlab_em`'s
///   `fdfd_tmz_pml_2d`.
/// - `Dirichlet` (ghost cell 0) keeps the outer face on the diagonal with
///   the boundary cell's own `s_e` as its stretch, the way
///   `laplacian_eps_2d` uses the cell's own ε there. `sx_h(nx)` and
///   `sy_h(ny)` are not read.
/// - `Periodic` wraps; `s_h(n)` is the face between the last and first
///   cell.
///
/// With every stretch equal to 1 the result is `laplacian_2d_bc` with the
/// same boundary condition (bit for bit at unit spacing). The operator is
/// not symmetric, but `diag(sx_e(j)·sy_e(i)) · A` is complex symmetric.
pub fn laplacian_scpml_2d(
    sx_e: &[C64],
    sx_h: &[C64],
    sy_e: &[C64],
    sy_h: &[C64],
    dx: f64,
    dy: f64,
    bc: BoundaryCondition,
) -> Result<SparseMat, DspError> {
    const NAME: &str = "laplacian_scpml_2d";
    check_pair(NAME, 'x', sx_e, sx_h)?;
    check_pair(NAME, 'y', sy_e, sy_h)?;
    check_pos(NAME, "dx", dx)?;
    check_pos(NAME, "dy", dy)?;
    let (nx, ny) = (sx_e.len(), sy_e.len());
    let n = rustlab_core::checked_elems(&[nx, ny], rustlab_core::ELEM_F64).map_err(|_| {
        DspError::InvalidParameter(rustlab_core::variable_too_large(NAME, &[nx, ny]))
    })?;
    let inv_dx2 = 1.0 / (dx * dx);
    let inv_dy2 = 1.0 / (dy * dy);

    let rows_y: Vec<AxisRow> = (0..ny)
        .map(|i| axis_row(i, sy_e, sy_h, inv_dy2, bc))
        .collect();
    let mut entries: Vec<(usize, usize, C64)> = Vec::with_capacity(5 * n);
    let mut buf: [(usize, C64); 5] = [(0, Complex::new(0.0, 0.0)); 5];

    for j in 0..nx {
        let rx = axis_row(j, sx_e, sx_h, inv_dx2, bc);
        for (i, ry) in rows_y.iter().enumerate() {
            let k = j * ny + i;
            let mut len = 0usize;
            for (step, off) in [ry.lower, ry.upper]
                .into_iter()
                .map(|e| (1isize, e))
                .chain([rx.lower, rx.upper].into_iter().map(|e| (ny as isize, e)))
            {
                if let Some((cells, a)) = off {
                    buf[len] = ((k as isize + cells * step) as usize, a);
                    len += 1;
                }
            }
            buf[len] = (k, -(ry.diag + rx.diag));
            len += 1;
            flush_row::<5>(&mut entries, k, &mut buf, len);
        }
    }
    Ok(SparseMat::from_sorted_entries(n, n, entries))
}

/// Stretch factors `s = 1 + jσ/(ω ε0)` for a polynomial PML of `npml` cells
/// at both ends of an axis of `n` cells: `(s_e, s_h)`, the cell values and
/// the face values for [`laplacian_scpml_2d`].
///
/// With 1-based cell `i`, `σ_e(i) = σmax·(max(xl, 0)^m + max(xr, 0)^m)`,
/// `xl = (npml − i + 1)/npml`, `xr = (i − (n − npml))/npml`, so the
/// outermost cells carry `σmax` and the interior is exactly 1. The face
/// profile is the same polynomial half a cell further on (`+0.5` in `xl`
/// and `xr`); `s_h(n)` is the outer wall face. This is `rustlab_em`'s
/// `pml_sigma_profile` / `pml_sigma_profile_h` / `pml_stretching_factor`
/// with `m` = `order`. `ε0` is the CODATA 2018 value used by the FDTD
/// code (`1/(μ0 c0²)`).
pub fn scpml_stretch(
    n: usize,
    npml: usize,
    sigma_max: f64,
    omega: f64,
    order: f64,
) -> Result<(Vec<C64>, Vec<C64>), DspError> {
    const NAME: &str = "scpml_stretch";
    check_min(NAME, "n", n, 2)?;
    if 2 * npml > n {
        return Err(DspError::InvalidParameter(format!(
            "{NAME}: npml = {npml} leaves no interior on an axis of {n} cells (need 2·npml <= n)"
        )));
    }
    if !sigma_max.is_finite() || sigma_max < 0.0 {
        return Err(DspError::InvalidParameter(format!(
            "{NAME}: sigma_max must be finite and >= 0, got {sigma_max}"
        )));
    }
    check_pos(NAME, "omega", omega)?;
    check_pos(NAME, "order", order)?;
    if npml == 0 {
        let one = vec![Complex::new(1.0, 0.0); n];
        return Ok((one.clone(), one));
    }

    let (nf, pf) = (n as f64, npml as f64);
    let scale = 1.0 / (omega * crate::fdtd::EPS0);
    let profile = |shift: f64| -> Vec<C64> {
        (1..=n)
            .map(|ii| {
                let ii = ii as f64;
                let xl = (pf - ii + 1.0 - shift) / pf;
                let xr = (ii - (nf - pf) + shift) / pf;
                let sigma = sigma_max * (xl.max(0.0).powf(order) + xr.max(0.0).powf(order));
                Complex::new(1.0, sigma * scale)
            })
            .collect()
    };
    Ok((profile(0.0), profile(0.5)))
}

#[cfg(test)]
mod tests;

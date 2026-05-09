//! Vector-calculus operators on uniform 2-D and 3-D grids.
//!
//! 2-D grid convention: `F(i, j)` corresponds to position `(x = j*dx, y = i*dy)` —
//! rows index `y`, columns index `x`. Same as Octave / NumPy.
//!
//! 3-D grid convention extends the 2-D one with the page axis as `z`:
//! `F(i, j, k)` ↔ `(x = j*dx, y = i*dy, z = k*dz)`. Axis 0 = y (rows),
//! axis 1 = x (cols), axis 2 = z (pages).
//!
//! All kernels accept complex inputs — EM fields are routinely complex in the
//! frequency domain.
//!
//! Stencils:
//! - Interior: 2nd-order central differences.
//! - Boundary: 2nd-order one-sided (forward at i=0, backward at i=n-1).
//!
//! Each differentiation axis must have length ≥ 3.
//!
//! Phase 3 of `dev/plans/em_performance.md` rewrote the inner loops to use
//! row/column slice iteration (no per-element bounds checks), fused the
//! `divergence` and `curl` paths so they don't allocate intermediate
//! per-axis derivative tensors, and added rayon-parallel outer-axis
//! sweeps when `n*m >= PAR_THRESHOLD`.

use crate::error::DspError;
use num_complex::Complex;
use rayon::prelude::*;
#[cfg(test)]
use rustlab_core::C64;
use rustlab_core::{CMatrix, CTensor3};

/// Element count above which the public 2-D / 3-D kernels switch to
/// rayon-parallel outer-axis sweeps. Below the threshold, serial
/// loops win — rayon's per-task overhead dominates on small grids.
/// Tuned empirically against a quiet laptop; adjust if profiling shows
/// a different sweet spot. Test code can override via the
/// `__test_par_threshold` knob below.
const PAR_THRESHOLD: usize = 4096;

/// Test-only override of `PAR_THRESHOLD`. When set to `Some(n)`, both
/// 2-D and 3-D kernels treat `n*m >= n_override` as parallel-eligible.
/// `None` means "use the production threshold". The flag lives behind
/// a `Mutex` for thread safety even though tests run on one thread by
/// default — this is a debug aid, not a hot path.
#[cfg(test)]
static TEST_PAR_THRESHOLD: std::sync::Mutex<Option<usize>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn __test_set_par_threshold(threshold: Option<usize>) {
    *TEST_PAR_THRESHOLD.lock().unwrap() = threshold;
}

#[inline]
fn par_threshold() -> usize {
    #[cfg(test)]
    {
        if let Some(t) = *TEST_PAR_THRESHOLD.lock().unwrap() {
            return t;
        }
    }
    PAR_THRESHOLD
}

#[inline]
fn use_parallel(n: usize, m: usize) -> bool {
    n.saturating_mul(m) >= par_threshold()
}

fn check_axis_len(name: &str, axis: &str, len: usize) -> Result<(), DspError> {
    if len < 3 {
        return Err(DspError::InvalidParameter(format!(
            "{name}: axis {axis} length {len} < 3 (need at least 3 samples for 2nd-order stencils)"
        )));
    }
    Ok(())
}

fn check_step(name: &str, label: &str, h: f64) -> Result<(), DspError> {
    if !h.is_finite() || h <= 0.0 {
        return Err(DspError::InvalidParameter(format!(
            "{name}: {label} must be a positive finite number, got {h}"
        )));
    }
    Ok(())
}

fn check_same_shape(
    name: &str,
    a: &CMatrix,
    b: &CMatrix,
    a_lbl: &str,
    b_lbl: &str,
) -> Result<(), DspError> {
    if a.dim() != b.dim() {
        return Err(DspError::InvalidParameter(format!(
            "{name}: {a_lbl} shape {:?} ≠ {b_lbl} shape {:?}",
            a.dim(),
            b.dim()
        )));
    }
    Ok(())
}

// ─── 2-D kernels — slice-iterating, optionally parallel ──────────────────────
//
// Inputs are guaranteed standard-layout (row-major contiguous) by the public
// entry points via `as_standard_layout`. That gives us `as_slice()` access for
// stride-1 row reads and `as_slice_mut()` writes — the inner loops become
// straight f64/Complex64 arithmetic with no bounds checks, which LLVM is happy
// to vectorize on AVX2 / NEON.

/// Fill `out_row[..nx]` with `∂f/∂x` along a single row. `f_row` must
/// have length `nx >= 3`.
#[inline]
fn d_dx_row(f_row: &[Complex<f64>], out_row: &mut [Complex<f64>], dx: f64) {
    let nx = f_row.len();
    debug_assert_eq!(out_row.len(), nx);
    debug_assert!(nx >= 3);
    let inv_2dx = 0.5 / dx;
    // Left boundary: 2nd-order forward.
    out_row[0] = (f_row[1] * 4.0 - f_row[0] * 3.0 - f_row[2]) * inv_2dx;
    // Interior: central.
    for j in 1..nx - 1 {
        out_row[j] = (f_row[j + 1] - f_row[j - 1]) * inv_2dx;
    }
    // Right boundary: 2nd-order backward.
    out_row[nx - 1] = (f_row[nx - 1] * 3.0 - f_row[nx - 2] * 4.0 + f_row[nx - 3]) * inv_2dx;
}

/// `∂F/∂x` along columns (axis 1). Step `dx` between adjacent columns.
fn d_dx(f: &CMatrix, dx: f64) -> CMatrix {
    let f = f.as_standard_layout();
    let (ny, nx) = f.dim();
    let mut out = CMatrix::zeros((ny, nx));

    if use_parallel(ny, nx) {
        let f_slice = f.as_slice().expect("standard layout has slice");
        let out_slice = out.as_slice_mut().expect("zeros() is standard layout");
        out_slice
            .par_chunks_mut(nx)
            .zip(f_slice.par_chunks(nx))
            .for_each(|(out_row, f_row)| d_dx_row(f_row, out_row, dx));
    } else {
        let f_slice = f.as_slice().expect("standard layout has slice");
        let out_slice = out.as_slice_mut().expect("zeros() is standard layout");
        for (out_row, f_row) in out_slice.chunks_mut(nx).zip(f_slice.chunks(nx)) {
            d_dx_row(f_row, out_row, dx);
        }
    }
    out
}

/// `∂F/∂y` along rows (axis 0). Step `dy` between adjacent rows.
///
/// Row-parallel sweep using `par_chunks_mut(nx)`. Each row's output
/// cell at (i, j) depends on f rows i-1, i, i+1 (or boundary-adjusted),
/// so the *write* is row-local; only reads cross rows. That's safe under
/// rayon — multiple tasks can read the same `f_slice` simultaneously.
fn d_dy(f: &CMatrix, dy: f64) -> CMatrix {
    let f = f.as_standard_layout();
    let (ny, nx) = f.dim();
    let mut out = CMatrix::zeros((ny, nx));
    let inv_2dy = 0.5 / dy;

    let f_slice: &[Complex<f64>] = f.as_slice().expect("standard layout has slice");
    let out_slice = out.as_slice_mut().expect("zeros() is standard layout");

    let row_kernel = |i: usize, out_row: &mut [Complex<f64>]| {
        if i == 0 {
            let r0 = &f_slice[0..nx];
            let r1 = &f_slice[nx..2 * nx];
            let r2 = &f_slice[2 * nx..3 * nx];
            for j in 0..nx {
                out_row[j] = (r1[j] * 4.0 - r0[j] * 3.0 - r2[j]) * inv_2dy;
            }
        } else if i == ny - 1 {
            let last = ny - 1;
            let r0 = &f_slice[(last - 2) * nx..(last - 1) * nx];
            let r1 = &f_slice[(last - 1) * nx..last * nx];
            let r2 = &f_slice[last * nx..(last + 1) * nx];
            for j in 0..nx {
                out_row[j] = (r2[j] * 3.0 - r1[j] * 4.0 + r0[j]) * inv_2dy;
            }
        } else {
            let r_prev = &f_slice[(i - 1) * nx..i * nx];
            let r_next = &f_slice[(i + 1) * nx..(i + 2) * nx];
            for j in 0..nx {
                out_row[j] = (r_next[j] - r_prev[j]) * inv_2dy;
            }
        }
    };

    if use_parallel(ny, nx) {
        out_slice
            .par_chunks_mut(nx)
            .enumerate()
            .for_each(|(i, out_row)| row_kernel(i, out_row));
    } else {
        for (i, out_row) in out_slice.chunks_mut(nx).enumerate() {
            row_kernel(i, out_row);
        }
    }
    out
}

/// 2-D gradient of scalar field `F` on a uniform grid.
///
/// Returns `(Fx, Fy)` with the same shape as `F`. `Fx` is `∂F/∂x` (along
/// columns, step `dx`); `Fy` is `∂F/∂y` (along rows, step `dy`).
pub fn gradient_2d(f: &CMatrix, dx: f64, dy: f64) -> Result<(CMatrix, CMatrix), DspError> {
    check_step("gradient", "dx", dx)?;
    check_step("gradient", "dy", dy)?;
    let (ny, nx) = f.dim();
    check_axis_len("gradient", "x (columns)", nx)?;
    check_axis_len("gradient", "y (rows)", ny)?;
    Ok((d_dx(f, dx), d_dy(f, dy)))
}

/// 2-D divergence `∂Fx/∂x + ∂Fy/∂y`. Output has the same shape as the inputs.
///
/// Fused single-sweep implementation: writes directly to the output
/// without allocating intermediate per-axis derivative matrices.
/// Compared to `d_dx(fx) + d_dy(fy)` this saves two full-grid
/// allocations and one full-grid summation pass.
pub fn divergence_2d(fx: &CMatrix, fy: &CMatrix, dx: f64, dy: f64) -> Result<CMatrix, DspError> {
    check_step("divergence", "dx", dx)?;
    check_step("divergence", "dy", dy)?;
    check_same_shape("divergence", fx, fy, "Fx", "Fy")?;
    let (ny, nx) = fx.dim();
    check_axis_len("divergence", "x (columns)", nx)?;
    check_axis_len("divergence", "y (rows)", ny)?;

    let fx = fx.as_standard_layout();
    let fy = fy.as_standard_layout();
    let mut out = CMatrix::zeros((ny, nx));
    let inv_2dx = 0.5 / dx;
    let inv_2dy = 0.5 / dy;

    let fx_s = fx.as_slice().expect("standard layout has slice");
    let fy_s = fy.as_slice().expect("standard layout has slice");
    let out_s = out.as_slice_mut().expect("zeros() is standard layout");

    let row_kernel = |i: usize, out_row: &mut [Complex<f64>]| {
        let off = i * nx;
        let fx_row = &fx_s[off..off + nx];
        // Combine d_dx contribution from the fx slice, in place, with the
        // d_dy contribution from fy at this row. d_dy depends on i, so we
        // must read three fy rows: i-1, i, i+1 (or boundary-adjusted).
        // x-derivative — same recipe as d_dx_row, but we write the partial
        // result; the y part adds in below.
        let dxc = inv_2dx;
        out_row[0] = (fx_row[1] * 4.0 - fx_row[0] * 3.0 - fx_row[2]) * dxc;
        for j in 1..nx - 1 {
            out_row[j] = (fx_row[j + 1] - fx_row[j - 1]) * dxc;
        }
        out_row[nx - 1] =
            (fx_row[nx - 1] * 3.0 - fx_row[nx - 2] * 4.0 + fx_row[nx - 3]) * dxc;

        // y-derivative contribution depending on row position.
        let dyc = inv_2dy;
        if i == 0 {
            let r0 = &fy_s[0..nx];
            let r1 = &fy_s[nx..2 * nx];
            let r2 = &fy_s[2 * nx..3 * nx];
            for j in 0..nx {
                out_row[j] += (r1[j] * 4.0 - r0[j] * 3.0 - r2[j]) * dyc;
            }
        } else if i == ny - 1 {
            let last = ny - 1;
            let r0 = &fy_s[(last - 2) * nx..(last - 1) * nx];
            let r1 = &fy_s[(last - 1) * nx..last * nx];
            let r2 = &fy_s[last * nx..(last + 1) * nx];
            for j in 0..nx {
                out_row[j] += (r2[j] * 3.0 - r1[j] * 4.0 + r0[j]) * dyc;
            }
        } else {
            let r_prev = &fy_s[(i - 1) * nx..i * nx];
            let r_next = &fy_s[(i + 1) * nx..(i + 2) * nx];
            for j in 0..nx {
                out_row[j] += (r_next[j] - r_prev[j]) * dyc;
            }
        }
    };

    if use_parallel(ny, nx) {
        out_s
            .par_chunks_mut(nx)
            .enumerate()
            .for_each(|(i, out_row)| row_kernel(i, out_row));
    } else {
        for (i, out_row) in out_s.chunks_mut(nx).enumerate() {
            row_kernel(i, out_row);
        }
    }
    Ok(out)
}

/// 2-D scalar curl `∂Fy/∂x − ∂Fx/∂y` (the z-component of `∇×F` in 3-space).
///
/// Fused single-sweep, mirror image of `divergence_2d`.
pub fn curl_2d(fx: &CMatrix, fy: &CMatrix, dx: f64, dy: f64) -> Result<CMatrix, DspError> {
    check_step("curl", "dx", dx)?;
    check_step("curl", "dy", dy)?;
    check_same_shape("curl", fx, fy, "Fx", "Fy")?;
    let (ny, nx) = fx.dim();
    check_axis_len("curl", "x (columns)", nx)?;
    check_axis_len("curl", "y (rows)", ny)?;

    let fx = fx.as_standard_layout();
    let fy = fy.as_standard_layout();
    let mut out = CMatrix::zeros((ny, nx));
    let inv_2dx = 0.5 / dx;
    let inv_2dy = 0.5 / dy;

    let fx_s = fx.as_slice().expect("standard layout has slice");
    let fy_s = fy.as_slice().expect("standard layout has slice");
    let out_s = out.as_slice_mut().expect("zeros() is standard layout");

    let row_kernel = |i: usize, out_row: &mut [Complex<f64>]| {
        let off = i * nx;
        let fy_row = &fy_s[off..off + nx];
        let dxc = inv_2dx;
        // d/dx of fy → out
        out_row[0] = (fy_row[1] * 4.0 - fy_row[0] * 3.0 - fy_row[2]) * dxc;
        for j in 1..nx - 1 {
            out_row[j] = (fy_row[j + 1] - fy_row[j - 1]) * dxc;
        }
        out_row[nx - 1] =
            (fy_row[nx - 1] * 3.0 - fy_row[nx - 2] * 4.0 + fy_row[nx - 3]) * dxc;

        // − d/dy of fx → subtract
        let dyc = inv_2dy;
        if i == 0 {
            let r0 = &fx_s[0..nx];
            let r1 = &fx_s[nx..2 * nx];
            let r2 = &fx_s[2 * nx..3 * nx];
            for j in 0..nx {
                out_row[j] -= (r1[j] * 4.0 - r0[j] * 3.0 - r2[j]) * dyc;
            }
        } else if i == ny - 1 {
            let last = ny - 1;
            let r0 = &fx_s[(last - 2) * nx..(last - 1) * nx];
            let r1 = &fx_s[(last - 1) * nx..last * nx];
            let r2 = &fx_s[last * nx..(last + 1) * nx];
            for j in 0..nx {
                out_row[j] -= (r2[j] * 3.0 - r1[j] * 4.0 + r0[j]) * dyc;
            }
        } else {
            let r_prev = &fx_s[(i - 1) * nx..i * nx];
            let r_next = &fx_s[(i + 1) * nx..(i + 2) * nx];
            for j in 0..nx {
                out_row[j] -= (r_next[j] - r_prev[j]) * dyc;
            }
        }
    };

    if use_parallel(ny, nx) {
        out_s
            .par_chunks_mut(nx)
            .enumerate()
            .for_each(|(i, out_row)| row_kernel(i, out_row));
    } else {
        for (i, out_row) in out_s.chunks_mut(nx).enumerate() {
            row_kernel(i, out_row);
        }
    }
    Ok(out)
}

// ─── 3-D operators on Tensor3 ────────────────────────────────────────────────
//
// Tensor3 layout per ndarray's column-major-of-pages convention:
// flat index `((k * n) + j) * m + i` for shape (m=rows, n=cols, p=pages).
// We don't lean on slice access for the 3-D variants because the axes
// have different strides (axis 0 = stride 1 — contiguous row, axis 1 =
// stride m, axis 2 = stride m*n). Indexed access via [[i, j, k]] is fine
// here; the wins come from fusion (one sweep per output, no
// intermediates) and parallelism across the outermost index.

fn check_same_shape_3d(
    name: &str,
    a: &CTensor3,
    b: &CTensor3,
    a_lbl: &str,
    b_lbl: &str,
) -> Result<(), DspError> {
    if a.dim() != b.dim() {
        return Err(DspError::InvalidParameter(format!(
            "{name}: {a_lbl} shape {:?} ≠ {b_lbl} shape {:?}",
            a.dim(),
            b.dim()
        )));
    }
    Ok(())
}

/// Differentiate `f` along `axis` using a 2nd-order stencil (central interior,
/// one-sided boundaries). `axis`: 0 = y (rows), 1 = x (cols), 2 = z (pages).
fn d_along_axis_3d(f: &CTensor3, axis: usize, h: f64) -> CTensor3 {
    let s = f.shape();
    let (m, n, p) = (s[0], s[1], s[2]);
    let mut out = CTensor3::zeros((m, n, p));
    let inv_2h = 0.5 / h;
    match axis {
        0 => {
            for j in 0..n {
                for k in 0..p {
                    out[[0, j, k]] = (f[[1, j, k]] * 4.0 - f[[0, j, k]] * 3.0 - f[[2, j, k]])
                        * inv_2h;
                    for i in 1..m - 1 {
                        out[[i, j, k]] = (f[[i + 1, j, k]] - f[[i - 1, j, k]]) * inv_2h;
                    }
                    out[[m - 1, j, k]] = (f[[m - 1, j, k]] * 3.0 - f[[m - 2, j, k]] * 4.0
                        + f[[m - 3, j, k]])
                        * inv_2h;
                }
            }
        }
        1 => {
            for i in 0..m {
                for k in 0..p {
                    out[[i, 0, k]] = (f[[i, 1, k]] * 4.0 - f[[i, 0, k]] * 3.0 - f[[i, 2, k]])
                        * inv_2h;
                    for j in 1..n - 1 {
                        out[[i, j, k]] = (f[[i, j + 1, k]] - f[[i, j - 1, k]]) * inv_2h;
                    }
                    out[[i, n - 1, k]] = (f[[i, n - 1, k]] * 3.0 - f[[i, n - 2, k]] * 4.0
                        + f[[i, n - 3, k]])
                        * inv_2h;
                }
            }
        }
        2 => {
            for i in 0..m {
                for j in 0..n {
                    out[[i, j, 0]] = (f[[i, j, 1]] * 4.0 - f[[i, j, 0]] * 3.0 - f[[i, j, 2]])
                        * inv_2h;
                    for k in 1..p - 1 {
                        out[[i, j, k]] = (f[[i, j, k + 1]] - f[[i, j, k - 1]]) * inv_2h;
                    }
                    out[[i, j, p - 1]] = (f[[i, j, p - 1]] * 3.0 - f[[i, j, p - 2]] * 4.0
                        + f[[i, j, p - 3]])
                        * inv_2h;
                }
            }
        }
        _ => unreachable!("axis must be 0, 1, or 2"),
    }
    out
}

fn check_3d_axes(name: &str, t: &CTensor3) -> Result<(), DspError> {
    let s = t.shape();
    check_axis_len(name, "y (rows)", s[0])?;
    check_axis_len(name, "x (cols)", s[1])?;
    check_axis_len(name, "z (pages)", s[2])?;
    Ok(())
}

/// 3-D gradient of scalar field `F` on a uniform grid.
///
/// Returns `(Fx, Fy, Fz)` with the same shape as `F`. `Fx` is `∂F/∂x` (along
/// columns / axis 1, step `dx`); `Fy` is `∂F/∂y` (along rows / axis 0, step
/// `dy`); `Fz` is `∂F/∂z` (along pages / axis 2, step `dz`).
pub fn gradient_3d(
    f: &CTensor3,
    dx: f64,
    dy: f64,
    dz: f64,
) -> Result<(CTensor3, CTensor3, CTensor3), DspError> {
    check_step("gradient3", "dx", dx)?;
    check_step("gradient3", "dy", dy)?;
    check_step("gradient3", "dz", dz)?;
    check_3d_axes("gradient3", f)?;
    let fx = d_along_axis_3d(f, 1, dx);
    let fy = d_along_axis_3d(f, 0, dy);
    let fz = d_along_axis_3d(f, 2, dz);
    Ok((fx, fy, fz))
}

/// 3-D divergence `∂Fx/∂x + ∂Fy/∂y + ∂Fz/∂z`. Output has the same shape as the inputs.
///
/// Fused: one sweep over the output, three boundary-aware reads per
/// element. Allocates a single output tensor instead of three
/// intermediate per-axis derivatives plus two summation temporaries.
pub fn divergence_3d(
    fx: &CTensor3,
    fy: &CTensor3,
    fz: &CTensor3,
    dx: f64,
    dy: f64,
    dz: f64,
) -> Result<CTensor3, DspError> {
    check_step("divergence3", "dx", dx)?;
    check_step("divergence3", "dy", dy)?;
    check_step("divergence3", "dz", dz)?;
    check_same_shape_3d("divergence3", fx, fy, "Fx", "Fy")?;
    check_same_shape_3d("divergence3", fx, fz, "Fx", "Fz")?;
    check_3d_axes("divergence3", fx)?;

    let s = fx.shape();
    let (m, n, p) = (s[0], s[1], s[2]);
    let mut out = CTensor3::zeros((m, n, p));
    let inv_2dx = 0.5 / dx;
    let inv_2dy = 0.5 / dy;
    let inv_2dz = 0.5 / dz;

    // Compute one (i, j) cell of the divergence, given the page index `k`.
    // Pulled out so both the serial and parallel paths share the body.
    let cell = |i: usize, j: usize, k: usize| -> Complex<f64> {
        let dxv = if j == 0 {
            (fx[[i, 1, k]] * 4.0 - fx[[i, 0, k]] * 3.0 - fx[[i, 2, k]]) * inv_2dx
        } else if j == n - 1 {
            (fx[[i, n - 1, k]] * 3.0 - fx[[i, n - 2, k]] * 4.0 + fx[[i, n - 3, k]]) * inv_2dx
        } else {
            (fx[[i, j + 1, k]] - fx[[i, j - 1, k]]) * inv_2dx
        };
        let dyv = if i == 0 {
            (fy[[1, j, k]] * 4.0 - fy[[0, j, k]] * 3.0 - fy[[2, j, k]]) * inv_2dy
        } else if i == m - 1 {
            (fy[[m - 1, j, k]] * 3.0 - fy[[m - 2, j, k]] * 4.0 + fy[[m - 3, j, k]]) * inv_2dy
        } else {
            (fy[[i + 1, j, k]] - fy[[i - 1, j, k]]) * inv_2dy
        };
        let dzv = if k == 0 {
            (fz[[i, j, 1]] * 4.0 - fz[[i, j, 0]] * 3.0 - fz[[i, j, 2]]) * inv_2dz
        } else if k == p - 1 {
            (fz[[i, j, p - 1]] * 3.0 - fz[[i, j, p - 2]] * 4.0 + fz[[i, j, p - 3]]) * inv_2dz
        } else {
            (fz[[i, j, k + 1]] - fz[[i, j, k - 1]]) * inv_2dz
        };
        dxv + dyv + dzv
    };

    if use_parallel(m * n, p) {
        // Page-parallel: split `out` along axis 2 so each task owns one
        // page (or chunk of pages) exclusively. `axis_chunks_iter_mut`
        // returns a parallel iterator (via rayon's `IntoParallelIterator`
        // impl in ndarray-rayon? — not available here, so use the serial
        // chunks iterator and bridge with rayon manually).
        use ndarray::Axis;
        let pages: Vec<usize> = (0..p).collect();
        // Build per-page output slabs in parallel and assemble. The slabs
        // own contiguous-by-page memory in ndarray's default layout.
        let chunks: Vec<(usize, Vec<Complex<f64>>)> = pages
            .par_iter()
            .map(|&k| {
                let mut slab = vec![Complex::new(0.0, 0.0); m * n];
                for j in 0..n {
                    for i in 0..m {
                        slab[j * m + i] = cell(i, j, k);
                    }
                }
                (k, slab)
            })
            .collect();
        for (k, slab) in chunks {
            let mut page = out.index_axis_mut(Axis(2), k);
            for j in 0..n {
                for i in 0..m {
                    page[[i, j]] = slab[j * m + i];
                }
            }
        }
    } else {
        for k in 0..p {
            for j in 0..n {
                for i in 0..m {
                    out[[i, j, k]] = cell(i, j, k);
                }
            }
        }
    }
    Ok(out)
}

/// 3-D curl `∇×F`. Returns `(Cx, Cy, Cz)` with each component having the same
/// shape as the inputs.
///
/// - `Cx = ∂Fz/∂y − ∂Fy/∂z`
/// - `Cy = ∂Fx/∂z − ∂Fz/∂x`
/// - `Cz = ∂Fy/∂x − ∂Fx/∂y`
pub fn curl_3d(
    fx: &CTensor3,
    fy: &CTensor3,
    fz: &CTensor3,
    dx: f64,
    dy: f64,
    dz: f64,
) -> Result<(CTensor3, CTensor3, CTensor3), DspError> {
    check_step("curl3", "dx", dx)?;
    check_step("curl3", "dy", dy)?;
    check_step("curl3", "dz", dz)?;
    check_same_shape_3d("curl3", fx, fy, "Fx", "Fy")?;
    check_same_shape_3d("curl3", fx, fz, "Fx", "Fz")?;
    check_3d_axes("curl3", fx)?;
    let cx = d_along_axis_3d(fz, 0, dy) - d_along_axis_3d(fy, 2, dz);
    let cy = d_along_axis_3d(fx, 2, dz) - d_along_axis_3d(fz, 1, dx);
    let cz = d_along_axis_3d(fy, 1, dx) - d_along_axis_3d(fx, 0, dy);
    Ok((cx, cy, cz))
}

// ─── Test helpers ────────────────────────────────────────────────────────────

/// Convenience constructor for filling a CMatrix from a real-valued closure.
#[cfg(test)]
pub(crate) fn from_real_fn<F: Fn(usize, usize) -> f64>(ny: usize, nx: usize, f: F) -> CMatrix {
    CMatrix::from_shape_fn((ny, nx), |(i, j)| Complex::new(f(i, j), 0.0))
}

/// Convenience constructor for filling a CMatrix from a complex-valued closure.
#[cfg(test)]
pub(crate) fn from_complex_fn<F: Fn(usize, usize) -> C64>(ny: usize, nx: usize, f: F) -> CMatrix {
    CMatrix::from_shape_fn((ny, nx), |(i, j)| f(i, j))
}

/// Convenience constructor for filling a CTensor3 from a real-valued closure.
#[cfg(test)]
pub(crate) fn from_real_fn_3d<F: Fn(usize, usize, usize) -> f64>(
    m: usize,
    n: usize,
    p: usize,
    f: F,
) -> CTensor3 {
    CTensor3::from_shape_fn((m, n, p), |(i, j, k)| Complex::new(f(i, j, k), 0.0))
}

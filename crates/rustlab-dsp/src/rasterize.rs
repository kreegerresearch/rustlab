//! Shape rasterization primitives — `rect_mask`, `disk_mask`, `polygon_mask`.
//!
//! Each builder takes meshgrid coordinate matrices `X` and `Y` (shape `(ny, nx)`,
//! same as `meshgrid` output) plus shape parameters and returns a real-valued
//! mask matrix of the same shape. Entries are `1.0` inside the shape and `0.0`
//! outside. Masks compose with element-wise math: `M1 .* M2` (intersection),
//! `1 - M` (complement), `max(M1, M2)` (union), `M1 .* (1 - M2)` (set difference).
//!
//! Coordinate matrices `X` and `Y` are interpreted as real-valued; only the
//! real part of each entry is used.

use crate::error::DspError;
use num_complex::Complex;
use rustlab_core::CMatrix;

const ONE: Complex<f64> = Complex::new(1.0, 0.0);

fn check_xy_shape(name: &str, x: &CMatrix, y: &CMatrix) -> Result<(), DspError> {
    if x.dim() != y.dim() {
        return Err(DspError::InvalidParameter(format!(
            "{name}: X shape {:?} ≠ Y shape {:?}",
            x.dim(),
            y.dim()
        )));
    }
    Ok(())
}

fn check_finite_nonneg(name: &str, label: &str, v: f64) -> Result<(), DspError> {
    if !v.is_finite() || v < 0.0 {
        return Err(DspError::InvalidParameter(format!(
            "{name}: {label} must be a finite non-negative number, got {v}"
        )));
    }
    Ok(())
}

/// Axis-aligned rectangle mask. Inclusive on all four sides:
/// `M(i, j) = 1.0` iff `x0 ≤ X(i, j) ≤ x0+w` and `y0 ≤ Y(i, j) ≤ y0+h`.
///
/// `w` and `h` must be finite and non-negative; a zero-extent rectangle is
/// allowed and matches only points lying on that line / point.
pub fn rect_mask(
    x: &CMatrix,
    y: &CMatrix,
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
) -> Result<CMatrix, DspError> {
    check_xy_shape("rect_mask", x, y)?;
    check_finite_nonneg("rect_mask", "w", w)?;
    check_finite_nonneg("rect_mask", "h", h)?;
    if !x0.is_finite() || !y0.is_finite() {
        return Err(DspError::InvalidParameter(format!(
            "rect_mask: x0 and y0 must be finite, got x0={x0}, y0={y0}"
        )));
    }

    let (ny, nx) = x.dim();
    let x_hi = x0 + w;
    let y_hi = y0 + h;
    let mut out = CMatrix::zeros((ny, nx));
    for i in 0..ny {
        for j in 0..nx {
            let xv = x[[i, j]].re;
            let yv = y[[i, j]].re;
            if xv >= x0 && xv <= x_hi && yv >= y0 && yv <= y_hi {
                out[[i, j]] = ONE;
            }
        }
    }
    Ok(out)
}

/// Closed-disk mask. `M(i, j) = 1.0` iff `(X-xc)² + (Y-yc)² ≤ r²`.
///
/// `r` must be finite and non-negative; `r = 0` matches only the centre point.
pub fn disk_mask(
    x: &CMatrix,
    y: &CMatrix,
    xc: f64,
    yc: f64,
    r: f64,
) -> Result<CMatrix, DspError> {
    check_xy_shape("disk_mask", x, y)?;
    check_finite_nonneg("disk_mask", "r", r)?;
    if !xc.is_finite() || !yc.is_finite() {
        return Err(DspError::InvalidParameter(format!(
            "disk_mask: xc and yc must be finite, got xc={xc}, yc={yc}"
        )));
    }

    let (ny, nx) = x.dim();
    let r_sq = r * r;
    let mut out = CMatrix::zeros((ny, nx));
    for i in 0..ny {
        for j in 0..nx {
            let dx = x[[i, j]].re - xc;
            let dy = y[[i, j]].re - yc;
            if dx * dx + dy * dy <= r_sq {
                out[[i, j]] = ONE;
            }
        }
    }
    Ok(out)
}

/// Polygon mask via even-odd ray casting (PNPOLY).
///
/// `verts` is an `N × 2` matrix where each row is `[x, y]`. Polygon is treated
/// as closed (an implicit edge connects vertex `N-1` back to vertex `0`).
///
/// Degenerate inputs return an all-zero mask:
/// - Fewer than 3 vertices.
/// - All vertices collinear (zero interior area).
///
/// Behaviour at points exactly on an edge is implementation-defined — the
/// PNPOLY half-open inequality returns a deterministic 0/1 but it is not
/// guaranteed to match any particular convention on the boundary. Callers
/// who need exact-edge semantics should perturb the polygon or use a
/// dedicated computational-geometry routine.
pub fn polygon_mask(x: &CMatrix, y: &CMatrix, verts: &CMatrix) -> Result<CMatrix, DspError> {
    check_xy_shape("polygon_mask", x, y)?;
    let (n_verts, vcols) = verts.dim();
    if vcols != 2 {
        return Err(DspError::InvalidParameter(format!(
            "polygon_mask: verts must be N×2 (each row is [x, y]); got shape {:?}",
            verts.dim()
        )));
    }

    let (ny, nx) = x.dim();
    let mut out = CMatrix::zeros((ny, nx));

    // Degenerate: fewer than 3 vertices → empty interior.
    if n_verts < 3 {
        return Ok(out);
    }

    let vx: Vec<f64> = (0..n_verts).map(|k| verts[[k, 0]].re).collect();
    let vy: Vec<f64> = (0..n_verts).map(|k| verts[[k, 1]].re).collect();

    for i in 0..ny {
        for j in 0..nx {
            let tx = x[[i, j]].re;
            let ty = y[[i, j]].re;
            let mut inside = false;
            let mut k = 0usize;
            let mut prev = n_verts - 1;
            while k < n_verts {
                let yi = vy[k];
                let yj = vy[prev];
                if (yi > ty) != (yj > ty) {
                    let xi = vx[k];
                    let xj = vx[prev];
                    let x_cross = (xj - xi) * (ty - yi) / (yj - yi) + xi;
                    if tx < x_cross {
                        inside = !inside;
                    }
                }
                prev = k;
                k += 1;
            }
            if inside {
                out[[i, j]] = ONE;
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;
    use rustlab_core::C64;

    fn meshgrid(xv: &[f64], yv: &[f64]) -> (CMatrix, CMatrix) {
        let m = xv.len();
        let n = yv.len();
        let x = Array2::from_shape_fn((n, m), |(_, j)| C64::new(xv[j], 0.0));
        let y = Array2::from_shape_fn((n, m), |(i, _)| C64::new(yv[i], 0.0));
        (x, y)
    }

    fn count(m: &CMatrix) -> usize {
        m.iter().filter(|c| c.re > 0.5).count()
    }

    #[test]
    fn rect_mask_matches_unit_square() {
        let xv: Vec<f64> = (0..=10).map(|k| k as f64 / 10.0).collect();
        let (x, y) = meshgrid(&xv, &xv);
        let m = rect_mask(&x, &y, 0.0, 0.0, 1.0, 1.0).unwrap();
        // Inclusive on all four sides → entire 11×11 grid is inside.
        assert_eq!(count(&m), 11 * 11);
    }

    #[test]
    fn rect_mask_zero_height_is_a_line() {
        let xv: Vec<f64> = (0..=10).map(|k| k as f64 / 10.0).collect();
        let (x, y) = meshgrid(&xv, &xv);
        let m = rect_mask(&x, &y, 0.0, 0.5, 1.0, 0.0).unwrap();
        // Only the row where y == 0.5 matches.
        assert_eq!(count(&m), 11);
    }

    #[test]
    fn rect_mask_rejects_negative_dims() {
        let (x, y) = meshgrid(&[0.0, 1.0], &[0.0, 1.0]);
        assert!(rect_mask(&x, &y, 0.0, 0.0, -1.0, 1.0).is_err());
        assert!(rect_mask(&x, &y, 0.0, 0.0, 1.0, -1.0).is_err());
    }

    #[test]
    fn disk_mask_approximates_pi() {
        // Disk of radius 1 inside [-1.5, 1.5]^2 with 200×200 cells.
        let n = 200usize;
        let lo = -1.5;
        let hi = 1.5;
        let step = (hi - lo) / (n - 1) as f64;
        let coords: Vec<f64> = (0..n).map(|k| lo + step * k as f64).collect();
        let (x, y) = meshgrid(&coords, &coords);
        let m = disk_mask(&x, &y, 0.0, 0.0, 1.0).unwrap();
        let area = count(&m) as f64 * step * step;
        // π ≈ 3.14159; allow 1.5% slack for finite-grid sampling.
        assert!((area - std::f64::consts::PI).abs() < 0.05, "got area {area}");
    }

    #[test]
    fn disk_mask_zero_radius_matches_only_centre() {
        let xv: Vec<f64> = (-2..=2).map(|k| k as f64).collect();
        let (x, y) = meshgrid(&xv, &xv);
        let m = disk_mask(&x, &y, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(count(&m), 1);
    }

    #[test]
    fn polygon_mask_equals_rect_for_unit_square() {
        // Use a fine grid that does not coincide with the polygon edges, so
        // both functions agree on every interior cell without boundary
        // ambiguity.
        let n = 50usize;
        let lo = -0.25;
        let hi = 1.25;
        let step = (hi - lo) / (n - 1) as f64;
        let coords: Vec<f64> = (0..n)
            .map(|k| lo + step * k as f64 + 0.5 * step * 0.123) // off-grid offset
            .collect();
        let (x, y) = meshgrid(&coords, &coords);

        let mut verts = CMatrix::zeros((4, 2));
        verts[[0, 0]] = C64::new(0.0, 0.0);
        verts[[0, 1]] = C64::new(0.0, 0.0);
        verts[[1, 0]] = C64::new(1.0, 0.0);
        verts[[1, 1]] = C64::new(0.0, 0.0);
        verts[[2, 0]] = C64::new(1.0, 0.0);
        verts[[2, 1]] = C64::new(1.0, 0.0);
        verts[[3, 0]] = C64::new(0.0, 0.0);
        verts[[3, 1]] = C64::new(1.0, 0.0);

        let poly = polygon_mask(&x, &y, &verts).unwrap();
        let rect = rect_mask(&x, &y, 0.0, 0.0, 1.0, 1.0).unwrap();
        for i in 0..n {
            for j in 0..n {
                assert_eq!(
                    poly[[i, j]].re,
                    rect[[i, j]].re,
                    "mismatch at ({i},{j}): x={}, y={}",
                    x[[i, j]].re,
                    y[[i, j]].re
                );
            }
        }
    }

    #[test]
    fn polygon_mask_degenerate_returns_zero() {
        let (x, y) = meshgrid(&[0.0, 0.5, 1.0], &[0.0, 0.5, 1.0]);

        // Empty
        let empty = CMatrix::zeros((0, 2));
        assert_eq!(count(&polygon_mask(&x, &y, &empty).unwrap()), 0);

        // Single vertex
        let single = CMatrix::from_shape_fn((1, 2), |_| C64::new(0.5, 0.0));
        assert_eq!(count(&polygon_mask(&x, &y, &single).unwrap()), 0);

        // Two vertices (a line segment, no interior)
        let mut two = CMatrix::zeros((2, 2));
        two[[0, 0]] = C64::new(0.0, 0.0);
        two[[1, 0]] = C64::new(1.0, 0.0);
        assert_eq!(count(&polygon_mask(&x, &y, &two).unwrap()), 0);

        // Collinear triangle (all three on y=0); zero area.
        let mut collinear = CMatrix::zeros((3, 2));
        collinear[[0, 0]] = C64::new(0.0, 0.0);
        collinear[[1, 0]] = C64::new(0.5, 0.0);
        collinear[[2, 0]] = C64::new(1.0, 0.0);
        assert_eq!(count(&polygon_mask(&x, &y, &collinear).unwrap()), 0);
    }

    #[test]
    fn polygon_mask_rejects_wrong_verts_shape() {
        let (x, y) = meshgrid(&[0.0, 1.0], &[0.0, 1.0]);
        let bad = CMatrix::zeros((3, 3));
        assert!(polygon_mask(&x, &y, &bad).is_err());
    }

    #[test]
    fn shape_mismatch_is_err() {
        let (x, _) = meshgrid(&[0.0, 1.0, 2.0], &[0.0, 1.0]);
        let (_, y) = meshgrid(&[0.0, 1.0], &[0.0, 1.0, 2.0]);
        assert!(rect_mask(&x, &y, 0.0, 0.0, 1.0, 1.0).is_err());
        assert!(disk_mask(&x, &y, 0.0, 0.0, 1.0).is_err());
        let mut verts = CMatrix::zeros((3, 2));
        verts[[1, 0]] = C64::new(1.0, 0.0);
        verts[[2, 1]] = C64::new(1.0, 0.0);
        assert!(polygon_mask(&x, &y, &verts).is_err());
    }
}

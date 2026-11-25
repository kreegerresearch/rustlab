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
pub fn disk_mask(x: &CMatrix, y: &CMatrix, xc: f64, yc: f64, r: f64) -> Result<CMatrix, DspError> {
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
    use rustlab_core::{RMatrix, C64};

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
        assert!(
            (area - std::f64::consts::PI).abs() < 0.05,
            "got area {area}"
        );
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

    // ─── Fractional coverage ────────────────────────────────────────────

    /// `n` nodes spaced `h` apart, centred on 0 and exactly antisymmetric.
    fn centred(n: usize, h: f64) -> Vec<f64> {
        (0..n).map(|k| (2.0 * k as f64 - (n - 1) as f64) * 0.5 * h).collect()
    }

    fn triangle() -> CMatrix {
        let pts = [(-0.9, -0.7), (1.1, -0.4), (0.2, 1.05)];
        CMatrix::from_shape_fn((3, 2), |(r, c)| {
            C64::new(if c == 0 { pts[r].0 } else { pts[r].1 }, 0.0)
        })
    }

    fn total(m: &RMatrix) -> f64 {
        m.iter().sum()
    }

    fn assert_bitwise_binary(cov: &RMatrix, bin: &CMatrix, what: &str) {
        assert_eq!(cov.dim(), bin.dim());
        for ((idx, a), b) in cov.indexed_iter().zip(bin.iter()) {
            assert_eq!(a.to_bits(), b.re.to_bits(), "{what}: mismatch at {idx:?}");
        }
    }

    /// Brute-force K×K reference: every sub-sample through the binary
    /// predicate, counted, divided by K². The fast paths must match it bit
    /// for bit.
    fn brute(x: &CMatrix, y: &CMatrix, k: usize, inside: impl Fn(f64, f64) -> bool) -> RMatrix {
        let (xs, ys) = meshgrid_axes("brute", x, y).unwrap();
        let sx = cell_samples(&xs, k);
        let sy = cell_samples(&ys, k);
        let (ny, nx) = x.dim();
        RMatrix::from_shape_fn((ny, nx), |(i, j)| {
            let mut c = 0u64;
            for &py in &sy[i * k..(i + 1) * k] {
                for &px in &sx[j * k..(j + 1) * k] {
                    if inside(px, py) {
                        c += 1;
                    }
                }
            }
            c as f64 / (k * k) as f64
        })
    }

    #[test]
    fn coverage_full_inside_is_one_and_outside_is_zero() {
        let c = centred(41, 0.1);
        let (x, y) = meshgrid(&c, &c);
        for k in [1, 2, 5, 16] {
            let d = disk_coverage(&x, &y, 0.0, 0.0, 1.0, k).unwrap();
            assert_eq!(d[[20, 20]], 1.0, "disk centre, K={k}");
            assert_eq!(d[[0, 0]], 0.0, "disk corner, K={k}");
            assert!(d.iter().all(|&a| (0.0..=1.0).contains(&a)));

            let r = rect_coverage(&x, &y, -1.0, -0.5, 1.5, 1.0, k).unwrap();
            assert_eq!(r[[20, 15]], 1.0, "rect interior, K={k}");
            assert_eq!(r[[40, 40]], 0.0, "rect outside, K={k}");
            assert!(r.iter().all(|&a| (0.0..=1.0).contains(&a)));

            let p = polygon_coverage(&x, &y, &triangle(), k).unwrap();
            assert_eq!(p[[20, 20]], 1.0, "triangle interior, K={k}");
            assert_eq!(p[[40, 0]], 0.0, "triangle outside, K={k}");
            assert!(p.iter().all(|&a| (0.0..=1.0).contains(&a)));
        }
    }

    #[test]
    fn coverage_boundary_cells_are_fractional() {
        let c = centred(41, 0.1);
        let (x, y) = meshgrid(&c, &c);
        // A rectangle edge through the middle of a column of cells covers
        // half of each cell in that column (even K puts K/2 samples inside).
        let r = rect_coverage(&x, &y, 1e-9, -5.0, 10.0, 10.0, 8).unwrap();
        assert_eq!(r[[20, 20]], 0.5);
        assert_eq!(r[[20, 21]], 1.0);
        assert_eq!(r[[20, 19]], 0.0);
        let d = disk_coverage(&x, &y, 0.0, 0.0, 1.0, 8).unwrap();
        assert!(d.iter().any(|&a| a > 0.0 && a < 1.0));
    }

    #[test]
    fn coverage_area_converges_within_h_over_k() {
        // |sum(α)·h² − A| ≤ P·h/K, with P the perimeter: the sub-sample error
        // in a boundary cell is at most one sub-row of that cell.
        for (n, h) in [(31, 0.1), (61, 0.05), (121, 0.025)] {
            let c = centred(n, h);
            let (x, y) = meshgrid(&c, &c);
            let r0 = 1.0;
            for k in [1, 2, 4, 8, 16, 32] {
                let bound = |perimeter: f64| perimeter * h / k as f64;

                let d = disk_coverage(&x, &y, 0.03, -0.02, r0, k).unwrap();
                let err = (total(&d) * h * h - std::f64::consts::PI * r0 * r0).abs();
                let b = bound(2.0 * std::f64::consts::PI * r0);
                assert!(err <= b, "disk h={h} K={k}: err {err} > {b}");

                let rc = rect_coverage(&x, &y, -0.83, -0.41, 1.37, 0.92, k).unwrap();
                let err = (total(&rc) * h * h - 1.37 * 0.92).abs();
                let b = bound(2.0 * (1.37 + 0.92));
                assert!(err <= b, "rect h={h} K={k}: err {err} > {b}");

                let t = triangle();
                let p = polygon_coverage(&x, &y, &t, k).unwrap();
                let (ax, ay, bx, by, cx, cy) = (-0.9, -0.7, 1.1, -0.4, 0.2, 1.05);
                let area = 0.5 * ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)).abs();
                let per = f64::hypot(bx - ax, by - ay)
                    + f64::hypot(cx - bx, cy - by)
                    + f64::hypot(ax - cx, ay - cy);
                let err = (total(&p) * h * h - area).abs();
                let b = bound(per);
                assert!(err <= b, "triangle h={h} K={k}: err {err} > {b}");
            }
        }
    }

    #[test]
    fn coverage_refines_the_binary_area() {
        // On a coarse grid, K = 16 is much closer to π than the binary mask.
        let h = 0.2;
        let c = centred(15, h);
        let (x, y) = meshgrid(&c, &c);
        let e1 = (total(&disk_coverage(&x, &y, 0.0, 0.0, 1.0, 1).unwrap()) * h * h
            - std::f64::consts::PI)
            .abs();
        let e16 = (total(&disk_coverage(&x, &y, 0.0, 0.0, 1.0, 16).unwrap()) * h * h
            - std::f64::consts::PI)
            .abs();
        assert!(e16 < 0.25 * e1, "K=1 err {e1}, K=16 err {e16}");
        assert!(e16 < 0.01, "K=16 err {e16}");
    }

    #[test]
    fn coverage_is_symmetric() {
        let c = centred(40, 0.07);
        let (x, y) = meshgrid(&c, &c);
        for k in [1, 3, 4, 9] {
            let d = disk_coverage(&x, &y, 0.0, 0.0, 0.9, k).unwrap();
            let r = rect_coverage(&x, &y, -0.6, -0.6, 1.2, 1.2, k).unwrap();
            let sq = CMatrix::from_shape_fn((4, 2), |(i, j)| {
                let p = [(-0.6, -0.6), (0.6, -0.6), (0.6, 0.6), (-0.6, 0.6)][i];
                C64::new(if j == 0 { p.0 } else { p.1 }, 0.0)
            });
            let p = polygon_coverage(&x, &y, &sq, k).unwrap();
            let n = c.len();
            for m in [&d, &r] {
                for i in 0..n {
                    for j in 0..n {
                        let v = m[[i, j]];
                        assert_eq!(v, m[[j, i]], "transpose K={k} ({i},{j})");
                        assert_eq!(v, m[[n - 1 - i, j]], "flipud K={k} ({i},{j})");
                        assert_eq!(v, m[[i, n - 1 - j]], "fliplr K={k} ({i},{j})");
                    }
                }
            }
            // The polygon square is symmetric under transpose (vertices in
            // the grid's sub-sample set never sit exactly on an edge here).
            for i in 0..n {
                for j in 0..n {
                    assert_eq!(p[[i, j]], p[[j, i]], "polygon transpose K={k}");
                    assert_eq!(p[[i, j]], r[[i, j]], "polygon vs rect K={k}");
                }
            }
        }
    }

    #[test]
    fn coverage_k1_equals_binary_mask() {
        // Uniform, off-grid, non-uniform, and decreasing coordinates.
        let uniform: Vec<f64> = (0..37).map(|k| -1.3 + k as f64 * 0.071).collect();
        let nonuni: Vec<f64> = (0..29).map(|k| -1.2 + 0.0031 * (k * k) as f64).collect();
        let decreasing: Vec<f64> = uniform.iter().rev().copied().collect();
        for (xv, yv) in [(&uniform, &uniform), (&nonuni, &uniform), (&decreasing, &nonuni)] {
            let (x, y) = meshgrid(xv, yv);
            assert_bitwise_binary(
                &disk_coverage(&x, &y, 0.1, -0.05, 0.77, 1).unwrap(),
                &disk_mask(&x, &y, 0.1, -0.05, 0.77).unwrap(),
                "disk",
            );
            assert_bitwise_binary(
                &rect_coverage(&x, &y, -0.5, -0.3, 0.9, 0.7, 1).unwrap(),
                &rect_mask(&x, &y, -0.5, -0.3, 0.9, 0.7).unwrap(),
                "rect",
            );
            assert_bitwise_binary(
                &polygon_coverage(&x, &y, &triangle(), 1).unwrap(),
                &polygon_mask(&x, &y, &triangle()).unwrap(),
                "polygon",
            );
        }
        // Nodes exactly on the boundary: the inclusive rule is kept.
        let xv: Vec<f64> = (0..=10).map(|k| k as f64 / 10.0).collect();
        let (x, y) = meshgrid(&xv, &xv);
        assert_bitwise_binary(
            &rect_coverage(&x, &y, 0.0, 0.0, 1.0, 1.0, 1).unwrap(),
            &rect_mask(&x, &y, 0.0, 0.0, 1.0, 1.0).unwrap(),
            "rect inclusive",
        );
        assert_bitwise_binary(
            &disk_coverage(&x, &y, 0.5, 0.5, 0.0, 1).unwrap(),
            &disk_mask(&x, &y, 0.5, 0.5, 0.0).unwrap(),
            "disk r=0",
        );
    }

    #[test]
    fn coverage_fast_paths_match_brute_force() {
        let xv: Vec<f64> = (0..23).map(|k| -1.1 + 0.0042 * (k * k) as f64 + 0.03 * k as f64).collect();
        let yv: Vec<f64> = (0..19).map(|k| 1.0 - 0.111 * k as f64).collect();
        let (x, y) = meshgrid(&xv, &yv);
        let t = triangle();
        let tv: Vec<(f64, f64)> = (0..3).map(|r| (t[[r, 0]].re, t[[r, 1]].re)).collect();
        for k in [1, 2, 3, 7, 10] {
            let (xc, yc, r) = (0.13, -0.08, 0.66);
            let d = disk_coverage(&x, &y, xc, yc, r, k).unwrap();
            let db = brute(&x, &y, k, |px, py| {
                let (dx, dy) = (px - xc, py - yc);
                dx * dx + dy * dy <= r * r
            });
            assert_eq!(d, db, "disk K={k}");

            let (x0, y0, w, h) = (-0.7, -0.45, 1.1, 0.8);
            let rc = rect_coverage(&x, &y, x0, y0, w, h, k).unwrap();
            let rb = brute(&x, &y, k, |px, py| {
                px >= x0 && px <= x0 + w && py >= y0 && py <= y0 + h
            });
            assert_eq!(rc, rb, "rect K={k}");

            let p = polygon_coverage(&x, &y, &t, k).unwrap();
            let pb = brute(&x, &y, k, |px, py| {
                let mut inside = false;
                let mut prev = tv.len() - 1;
                for (kk, &(xi, yi)) in tv.iter().enumerate() {
                    let (xj, yj) = tv[prev];
                    if (yi > py) != (yj > py) {
                        let x_cross = (xj - xi) * (py - yi) / (yj - yi) + xi;
                        if px < x_cross {
                            inside = !inside;
                        }
                    }
                    prev = kk;
                }
                inside
            });
            assert_eq!(p, pb, "polygon K={k}");
        }
    }

    #[test]
    fn coverage_rejects_invalid_k() {
        let c = centred(5, 0.5);
        let (x, y) = meshgrid(&c, &c);
        for k in [0, MAX_COVERAGE_K + 1] {
            assert!(disk_coverage(&x, &y, 0.0, 0.0, 1.0, k).is_err(), "K={k}");
            assert!(rect_coverage(&x, &y, 0.0, 0.0, 1.0, 1.0, k).is_err(), "K={k}");
            assert!(polygon_coverage(&x, &y, &triangle(), k).is_err(), "K={k}");
        }
        assert!(disk_coverage(&x, &y, 0.0, 0.0, 1.0, MAX_COVERAGE_K).is_ok());
    }

    #[test]
    fn coverage_needs_a_meshgrid() {
        let c = centred(5, 0.5);
        let (x, y) = meshgrid(&c, &c);
        // X varying down a column is not a meshgrid X.
        let mut bad = x.clone();
        bad[[3, 1]] = C64::new(9.0, 0.0);
        assert!(disk_coverage(&bad, &y, 0.0, 0.0, 1.0, 2).is_err());
        assert!(disk_coverage(&x, &bad.t().to_owned(), 0.0, 0.0, 1.0, 2).is_err());
        // One node along an axis gives no cell width.
        let (x1, y1) = meshgrid(&[0.0], &c);
        assert!(disk_coverage(&x1, &y1, 0.0, 0.0, 1.0, 2).is_err());
        // Repeated coordinates give a zero-width cell.
        let (x2, y2) = meshgrid(&[0.0, 0.0, 1.0], &c);
        assert!(rect_coverage(&x2, &y2, 0.0, 0.0, 1.0, 1.0, 2).is_err());
        // Shape and parameter checks still apply.
        assert!(disk_coverage(&x, &y, 0.0, 0.0, -1.0, 2).is_err());
        assert!(rect_coverage(&x, &y, 0.0, 0.0, 1.0, -1.0, 2).is_err());
    }

    #[test]
    fn coverage_degenerate_polygon_is_zero() {
        let c = centred(7, 0.3);
        let (x, y) = meshgrid(&c, &c);
        let two = CMatrix::zeros((2, 2));
        assert_eq!(total(&polygon_coverage(&x, &y, &two, 4).unwrap()), 0.0);
        assert!(polygon_coverage(&x, &y, &CMatrix::zeros((3, 3)), 4).is_err());
    }
}

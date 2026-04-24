//! Arrow glyph geometry and auto-scaling for `quiver` plots.
//!
//! Pure-functional helpers that turn a sampled 2-D vector field into arrow
//! polylines (shaft + triangular head) in world coordinates. Backends consume
//! these polylines directly — SVG/PNG via plotters draws each segment, HTML
//! via Plotly concatenates them into a single scatter trace with `None`
//! break separators between arrows.
//!
//! Grid convention: `u[row][col]`, `v[row][col]`, with `row` indexing `y` and
//! `col` indexing `x`. NaN entries in either component skip that cell.

/// A single arrow in world coordinates. `shaft.0` is the tail, `shaft.1` the
/// tip. `head` is the triangular arrowhead (three world points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrow {
    pub shaft: ((f64, f64), (f64, f64)),
    pub head: [(f64, f64); 3],
}

/// Nearest-neighbour cell distance. Used as the auto-scale reference length:
/// after scaling, no arrow is longer than this value.
pub fn cell_distance(x: &[f64], y: &[f64]) -> f64 {
    let dx = if x.len() >= 2 { (x[x.len() - 1] - x[0]).abs() / (x.len() - 1) as f64 } else { 1.0 };
    let dy = if y.len() >= 2 { (y[y.len() - 1] - y[0]).abs() / (y.len() - 1) as f64 } else { 1.0 };
    dx.min(dy)
}

/// Auto-scale factor such that the longest finite `(u, v)` pair is rescaled
/// to `cell_distance(x, y)`. Returns `0.0` (causing zero-length arrows) when
/// the field is identically zero or all-NaN.
pub fn auto_scale(u: &[Vec<f64>], v: &[Vec<f64>], x: &[f64], y: &[f64]) -> f64 {
    let mut max_mag = 0.0f64;
    for r in 0..u.len().min(v.len()) {
        let row_u = &u[r];
        let row_v = &v[r];
        for c in 0..row_u.len().min(row_v.len()) {
            let a = row_u[c];
            let b = row_v[c];
            if !a.is_finite() || !b.is_finite() { continue; }
            let m = (a * a + b * b).sqrt();
            if m > max_mag { max_mag = m; }
        }
    }
    let ref_len = cell_distance(x, y);
    if max_mag <= 0.0 || ref_len <= 0.0 {
        0.0
    } else {
        ref_len / max_mag
    }
}

/// Build the arrow glyph at base `(bx, by)` with world-space displacement
/// `(dx, dy)`. Returns `None` if the displacement is zero or non-finite.
pub fn arrow_at(bx: f64, by: f64, dx: f64, dy: f64) -> Option<Arrow> {
    if !dx.is_finite() || !dy.is_finite() { return None; }
    let len = (dx * dx + dy * dy).sqrt();
    if len <= 0.0 { return None; }

    let tx = bx + dx;
    let ty = by + dy;

    // Arrowhead: 30% of the arrow length, 20° half-angle.
    let head_len = 0.30 * len;
    let half_angle = 20.0_f64.to_radians();
    let ux = dx / len;
    let uy = dy / len;

    // Rotate the reverse unit vector by ±half_angle to get the two barbs.
    let cos_a = half_angle.cos();
    let sin_a = half_angle.sin();
    let lx = -ux * cos_a - -uy * sin_a;
    let ly = -ux * sin_a + -uy * cos_a;
    let rx = -ux * cos_a + -uy * sin_a;
    let ry = ux * sin_a + -uy * cos_a;

    let left = (tx + head_len * lx, ty + head_len * ly);
    let right = (tx + head_len * rx, ty + head_len * ry);
    Some(Arrow {
        shaft: ((bx, by), (tx, ty)),
        head: [left, (tx, ty), right],
    })
}

/// Build arrows for the full `(u, v)` grid. Applies `auto_scale(u, v, x, y)`
/// uniformly and then multiplies by the user-supplied `scale`. NaN entries
/// and zero-magnitude cells are skipped.
pub fn build_arrows(u: &[Vec<f64>], v: &[Vec<f64>], x: &[f64], y: &[f64], scale: f64)
    -> Vec<Arrow>
{
    let nrows = u.len().min(v.len()).min(y.len());
    if nrows == 0 { return Vec::new(); }
    let ncols = u.iter().chain(v.iter()).map(|row| row.len()).min().unwrap_or(0).min(x.len());
    if ncols == 0 { return Vec::new(); }

    let base = auto_scale(u, v, x, y);
    let k = base * scale;
    if k == 0.0 { return Vec::new(); }

    let mut arrows = Vec::with_capacity(nrows * ncols);
    for r in 0..nrows {
        for c in 0..ncols {
            let a = u[r][c];
            let b = v[r][c];
            if !a.is_finite() || !b.is_finite() { continue; }
            if let Some(arr) = arrow_at(x[c], y[r], k * a, k * b) {
                arrows.push(arr);
            }
        }
    }
    arrows
}

/// Arrow glyph placed at the midpoint of a polyline, pointing along its
/// local direction. Returns `None` for polylines shorter than 3 points or
/// when the local tangent is degenerate. `length` is the arrow shaft length
/// in world units — streamline callers typically pass `cell_distance` × 0.5.
pub fn midpoint_arrow(path: &[(f64, f64)], length: f64) -> Option<Arrow> {
    if path.len() < 3 || length <= 0.0 { return None; }
    let mid = path.len() / 2;
    let lo = mid.saturating_sub(1);
    let hi = (mid + 1).min(path.len() - 1);
    let dx = path[hi].0 - path[lo].0;
    let dy = path[hi].1 - path[lo].1;
    let m = (dx * dx + dy * dy).sqrt();
    if m <= 0.0 { return None; }
    let ux = dx / m;
    let uy = dy / m;
    let (tx, ty) = path[mid];
    // Tip sits at the midpoint; tail is one `length` behind.
    arrow_at(tx - length * ux, ty - length * uy, length * ux, length * uy)
}

/// Build default `X, Y` coordinate vectors for the `quiver(U, V)` shortcut.
/// Uses 1-based indexing (columns → `1..=ncols`, rows → `1..=nrows`) to
/// match the convention used elsewhere in the script layer.
pub fn default_xy(nrows: usize, ncols: usize) -> (Vec<f64>, Vec<f64>) {
    let x: Vec<f64> = (1..=ncols).map(|i| i as f64).collect();
    let y: Vec<f64> = (1..=nrows).map(|i| i as f64).collect();
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_distance_unit_grid() {
        let x: Vec<f64> = (0..5).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..5).map(|i| i as f64).collect();
        assert!((cell_distance(&x, &y) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn auto_scale_normalizes_longest_arrow() {
        let x: Vec<f64> = (0..4).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..4).map(|i| i as f64).collect();
        let u = vec![vec![3.0; 4]; 4];
        let v = vec![vec![4.0; 4]; 4]; // magnitude = 5
        let k = auto_scale(&u, &v, &x, &y);
        assert!((k * 5.0 - 1.0).abs() < 1e-12);
    }

    #[test]
    fn arrow_at_zero_displacement_is_none() {
        assert!(arrow_at(0.0, 0.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn arrow_head_is_behind_tip_along_shaft() {
        let arr = arrow_at(0.0, 0.0, 1.0, 0.0).unwrap();
        // Tip is at (1, 0). Left and right barbs should be at x < 1.
        assert!(arr.head[0].0 < arr.shaft.1.0);
        assert!(arr.head[2].0 < arr.shaft.1.0);
        // Head is vertically symmetric about y = 0.
        assert!((arr.head[0].1 + arr.head[2].1).abs() < 1e-12);
    }

    #[test]
    fn build_arrows_skips_nan() {
        let x = vec![0.0, 1.0, 2.0];
        let y = vec![0.0, 1.0];
        let u = vec![vec![1.0, f64::NAN, 1.0], vec![1.0, 1.0, 1.0]];
        let v = vec![vec![0.0, 0.0, 0.0], vec![0.0, 0.0, 0.0]];
        let arrows = build_arrows(&u, &v, &x, &y, 1.0);
        assert_eq!(arrows.len(), 5); // 6 cells minus 1 NaN
    }

    #[test]
    fn build_arrows_empty_when_field_is_zero() {
        let x = vec![0.0, 1.0];
        let y = vec![0.0, 1.0];
        let u = vec![vec![0.0; 2]; 2];
        let v = vec![vec![0.0; 2]; 2];
        assert!(build_arrows(&u, &v, &x, &y, 1.0).is_empty());
    }
}

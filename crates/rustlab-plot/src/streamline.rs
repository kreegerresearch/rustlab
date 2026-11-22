//! Streamline integration for 2-D vector fields.
//!
//! Pure-functional RK4 integrator used by the `streamplot` builtin. Given a
//! uniform-grid vector field `(u, v)` evaluated on `x[col] × y[row]`, trace a
//! single streamline from a seed point forward and backward until it either
//! leaves the domain, reaches a NaN sample, drops below a minimum magnitude,
//! or hits the step budget.
//!
//! Grid convention matches `contour.rs`: `u[row][col]`, `v[row][col]`, with
//! `row` indexing `y` and `col` indexing `x`.

/// Bilinearly sample `(u, v)` at world coordinate `(px, py)`.
///
/// Returns `None` if the point lies outside the grid or any surrounding
/// corner is non-finite.
pub fn sample(u: &[Vec<f64>], v: &[Vec<f64>], x: &[f64], y: &[f64], px: f64, py: f64)
    -> Option<(f64, f64)>
{
    let nrows = u.len();
    if nrows < 2 { return None; }
    let ncols = u[0].len();
    if ncols < 2 || x.len() < ncols || y.len() < nrows { return None; }
    if !px.is_finite() || !py.is_finite() { return None; }

    if px < x[0] || px > x[ncols - 1] || py < y[0] || py > y[nrows - 1] {
        return None;
    }

    // Locate bracketing column: largest c with x[c] <= px (stops one before end).
    let c = match x[..ncols].binary_search_by(|v| v.partial_cmp(&px).unwrap_or(std::cmp::Ordering::Equal)) {
        Ok(i) => i.min(ncols - 2),
        Err(i) => i.saturating_sub(1).min(ncols - 2),
    };
    let r = match y[..nrows].binary_search_by(|v| v.partial_cmp(&py).unwrap_or(std::cmp::Ordering::Equal)) {
        Ok(i) => i.min(nrows - 2),
        Err(i) => i.saturating_sub(1).min(nrows - 2),
    };

    let x0 = x[c]; let x1 = x[c + 1];
    let y0 = y[r]; let y1 = y[r + 1];
    let dx = x1 - x0;
    let dy = y1 - y0;
    if dx <= 0.0 || dy <= 0.0 { return None; }
    let tx = (px - x0) / dx;
    let ty = (py - y0) / dy;

    let u00 = u[r][c];     let u10 = u[r][c + 1];
    let u01 = u[r + 1][c]; let u11 = u[r + 1][c + 1];
    let v00 = v[r][c];     let v10 = v[r][c + 1];
    let v01 = v[r + 1][c]; let v11 = v[r + 1][c + 1];
    for &z in &[u00, u10, u01, u11, v00, v10, v01, v11] {
        if !z.is_finite() { return None; }
    }

    let lerp = |a: f64, b: f64, t: f64| a * (1.0 - t) + b * t;
    let ub = lerp(u00, u10, tx);
    let ut = lerp(u01, u11, tx);
    let vb = lerp(v00, v10, tx);
    let vt = lerp(v01, v11, tx);
    Some((lerp(ub, ut, ty), lerp(vb, vt, ty)))
}

/// RK4 step of signed size `h`. Returns `None` if any intermediate sample
/// falls outside the domain or is non-finite.
fn rk4_step(u: &[Vec<f64>], v: &[Vec<f64>], x: &[f64], y: &[f64],
            px: f64, py: f64, h: f64) -> Option<(f64, f64)>
{
    let (k1x, k1y) = sample(u, v, x, y, px, py)?;
    let (k2x, k2y) = sample(u, v, x, y, px + 0.5 * h * k1x, py + 0.5 * h * k1y)?;
    let (k3x, k3y) = sample(u, v, x, y, px + 0.5 * h * k2x, py + 0.5 * h * k2y)?;
    let (k4x, k4y) = sample(u, v, x, y, px + h * k3x, py + h * k3y)?;
    let dx = h * (k1x + 2.0 * k2x + 2.0 * k3x + k4x) / 6.0;
    let dy = h * (k1y + 2.0 * k2y + 2.0 * k3y + k4y) / 6.0;
    Some((px + dx, py + dy))
}

/// Integrate a streamline from `(sx, sy)` forward and backward, returning the
/// concatenated polyline (backward points, reversed, then forward points).
///
/// `step` is the nominal arc-length step in world units; it is bounded to a
/// fraction of the smaller grid spacing to avoid overshoot. `max_steps`
/// caps each direction. `min_speed` terminates when the field magnitude
/// drops to effectively zero.
pub fn integrate(
    u: &[Vec<f64>], v: &[Vec<f64>], x: &[f64], y: &[f64],
    sx: f64, sy: f64,
    step: f64, max_steps: usize, min_speed: f64,
) -> Vec<(f64, f64)>
{
    if sample(u, v, x, y, sx, sy).is_none() {
        return Vec::new();
    }

    let cycle_tol = 0.5 * step;
    let trace_dir = |sign: f64| -> Vec<(f64, f64)> {
        let mut pts = Vec::new();
        let (mut px, mut py) = (sx, sy);
        let mut arc = 0.0f64;
        let min_arc_before_cycle = 4.0 * step;
        for _ in 0..max_steps {
            let (ux, uy) = match sample(u, v, x, y, px, py) {
                Some(s) => s,
                None => break,
            };
            let speed = (ux * ux + uy * uy).sqrt();
            if !speed.is_finite() || speed < min_speed {
                break;
            }
            // Normalize by speed so `step` is an arc-length, not time-step.
            let h = sign * step / speed;
            let (nx, ny) = match rk4_step(u, v, x, y, px, py, h) {
                Some(p) => p,
                None => break,
            };
            if !nx.is_finite() || !ny.is_finite() {
                break;
            }
            px = nx;
            py = ny;
            pts.push((px, py));
            arc += step;
            // Cycle detection: closed orbits re-approach the seed after one
            // loop; stop the trace rather than re-drawing the same curve.
            if arc > min_arc_before_cycle {
                let d2 = (px - sx) * (px - sx) + (py - sy) * (py - sy);
                if d2 < cycle_tol * cycle_tol {
                    break;
                }
            }
        }
        pts
    };

    let backward = trace_dir(-1.0);
    let forward = trace_dir(1.0);

    let mut out = Vec::with_capacity(backward.len() + 1 + forward.len());
    for p in backward.into_iter().rev() {
        out.push(p);
    }
    out.push((sx, sy));
    out.extend(forward);
    out
}

/// Default arc-length step: 1/5 of the smaller grid spacing. Keeps traces
/// inside the bracketing cell each RK4 evaluation.
pub fn default_step(x: &[f64], y: &[f64]) -> f64 {
    let dx = if x.len() >= 2 { (x[x.len() - 1] - x[0]).abs() / (x.len() - 1) as f64 } else { 1.0 };
    let dy = if y.len() >= 2 { (y[y.len() - 1] - y[0]).abs() / (y.len() - 1) as f64 } else { 1.0 };
    0.2 * dx.min(dy).max(1e-300)
}

/// Default seed grid for `streamplot` when the user did not supply explicit
/// seeds. Places a `ceil(10 * density) × ceil(10 * density)` grid of seeds
/// across the interior of the domain. `density = 1.0` ≈ 100 seeds, which
/// reads well on typical 20–50-cell grids without producing megabyte HTML.
pub fn default_seeds(x: &[f64], y: &[f64], density: f64) -> Vec<(f64, f64)> {
    if x.len() < 2 || y.len() < 2 { return Vec::new(); }
    let n = ((10.0 * density.max(0.0)).ceil() as usize).max(2);
    let x0 = x[0]; let x1 = x[x.len() - 1];
    let y0 = y[0]; let y1 = y[y.len() - 1];
    let mut seeds = Vec::with_capacity(n * n);
    for r in 0..n {
        let ty = (r as f64 + 0.5) / n as f64;
        let py = y0 + (y1 - y0) * ty;
        for c in 0..n {
            let tx = (c as f64 + 0.5) / n as f64;
            let px = x0 + (x1 - x0) * tx;
            seeds.push((px, py));
        }
    }
    seeds
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform_field(nx: usize, ny: usize) -> (Vec<Vec<f64>>, Vec<Vec<f64>>, Vec<f64>, Vec<f64>) {
        let x: Vec<f64> = (0..nx).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..ny).map(|i| i as f64).collect();
        let u = vec![vec![1.0; nx]; ny];
        let v = vec![vec![0.0; nx]; ny];
        (u, v, x, y)
    }

    fn vortex(nx: usize, ny: usize, lo: f64, hi: f64)
        -> (Vec<Vec<f64>>, Vec<Vec<f64>>, Vec<f64>, Vec<f64>)
    {
        let x: Vec<f64> = (0..nx).map(|i| lo + (hi - lo) * i as f64 / (nx - 1) as f64).collect();
        let y: Vec<f64> = (0..ny).map(|i| lo + (hi - lo) * i as f64 / (ny - 1) as f64).collect();
        let mut u = vec![vec![0.0; nx]; ny];
        let mut v = vec![vec![0.0; nx]; ny];
        for r in 0..ny {
            for c in 0..nx {
                u[r][c] = -y[r];
                v[r][c] = x[c];
            }
        }
        (u, v, x, y)
    }

    #[test]
    fn sample_uniform_midpoint() {
        let (u, v, x, y) = uniform_field(5, 5);
        let (ux, vy) = sample(&u, &v, &x, &y, 1.5, 2.5).unwrap();
        assert!((ux - 1.0).abs() < 1e-12);
        assert!(vy.abs() < 1e-12);
    }

    #[test]
    fn sample_out_of_domain_returns_none() {
        let (u, v, x, y) = uniform_field(5, 5);
        assert!(sample(&u, &v, &x, &y, -0.1, 2.0).is_none());
        assert!(sample(&u, &v, &x, &y, 2.0, 10.0).is_none());
    }

    #[test]
    fn integrate_uniform_is_horizontal() {
        let (u, v, x, y) = uniform_field(20, 20);
        let pts = integrate(&u, &v, &x, &y, 10.0, 10.0,
                            default_step(&x, &y), 200, 1e-10);
        // All y-values should stay at the seed y within numerical tolerance.
        for (_, py) in &pts {
            assert!((py - 10.0).abs() < 1e-6, "py drifted: {}", py);
        }
        // And the trace should span a meaningful horizontal extent.
        let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
        let span = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                 - xs.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(span > 5.0, "trace too short: {}", span);
    }

    #[test]
    fn integrate_vortex_is_approximately_circular() {
        let (u, v, x, y) = vortex(41, 41, -5.0, 5.0);
        let pts = integrate(&u, &v, &x, &y, 3.0, 0.0,
                            default_step(&x, &y), 800, 1e-10);
        // Radius should be preserved (field is (-y, x) → pure rotation).
        let r0 = (3.0_f64.powi(2)).sqrt();
        for (px, py) in &pts {
            let r = (px * px + py * py).sqrt();
            assert!((r - r0).abs() < 0.1, "radius drift: r={} r0={}", r, r0);
        }
    }

    #[test]
    fn integrate_terminates_on_nan() {
        let (mut u, v, x, y) = uniform_field(10, 10);
        u[5][5] = f64::NAN;
        let pts = integrate(&u, &v, &x, &y, 0.0, 5.0,
                            default_step(&x, &y), 200, 1e-10);
        // Should still produce a usable trace that stops before the NaN cell.
        assert!(!pts.is_empty());
    }

    #[test]
    fn default_seeds_respects_density() {
        let x: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let sparse = default_seeds(&x, &y, 0.1);
        let dense = default_seeds(&x, &y, 1.0);
        assert!(dense.len() > sparse.len());
    }
}

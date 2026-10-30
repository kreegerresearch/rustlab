//! Largest singular value of a dense matrix (the matrix 2-norm).
//!
//! The value is `sqrt(λ_max)` of the smaller Gram matrix, `AᴴA` when
//! `A` has at least as many rows as columns and `AAᴴ` otherwise. Lanczos
//! applies that product without forming it and without singular vectors.
//! A full Hermitian Jacobi of the Gram is the fallback when the Krylov
//! residual is still large. Entries are scaled by the largest magnitude
//! so a matrix of `1e200` does not overflow the product.

use ndarray::Array2;

use crate::sparse_eig::sym_eig_jacobi;
use crate::types::C64;

/// Stop when the Ritz residual is this fraction of the Rayleigh quotient.
const RESIDUAL_TOL: f64 = 1e-14;
/// Lanczos steps. Past this, the Gram is diagonalized directly.
const MAX_STEPS: usize = 64;

/// Largest singular value of a real matrix. Empty is 0. A `NaN` entry
/// returns `NaN`. An `Inf` entry returns `Inf` when no entry is `NaN`.
pub fn largest_singular_value(a: &Array2<f64>) -> f64 {
    let (m, n) = a.dim();
    if let Some(slice) = a.as_slice() {
        largest_singular_value_slice(slice, m, n)
    } else {
        let buf: Vec<f64> = a.iter().copied().collect();
        largest_singular_value_slice(&buf, m, n)
    }
}

/// Largest singular value of a complex matrix. A matrix whose imaginary
/// part is all `+0` uses the real routine, so the bits match real storage.
pub fn largest_singular_value_complex(a: &Array2<C64>) -> f64 {
    if a.iter().all(|z| z.im == 0.0) {
        let buf: Vec<f64> = a.iter().map(|z| z.re).collect();
        return largest_singular_value_slice(&buf, a.nrows(), a.ncols());
    }
    let (m, n) = a.dim();
    if m == 0 || n == 0 {
        return 0.0;
    }
    let buf: Vec<C64> = a.iter().copied().collect();
    let mut any_nan = false;
    let mut any_inf = false;
    let mut scale = 0.0_f64;
    for z in &buf {
        if z.re.is_nan() || z.im.is_nan() {
            any_nan = true;
        } else if z.re.is_infinite() || z.im.is_infinite() {
            any_inf = true;
        } else {
            scale = scale.max(z.norm());
        }
    }
    if any_nan {
        return f64::NAN;
    }
    if any_inf {
        return f64::INFINITY;
    }
    if scale == 0.0 {
        return 0.0;
    }
    let left = m < n;
    let dim = if left { m } else { n };
    if dim == 1 {
        return scale * frobenius_scaled_complex(&buf, scale);
    }
    let (start, lower) = axis_start_complex(&buf, m, n, left, scale);
    let theta_lower = (lower / scale) * (lower / scale);
    let mut z = vec![C64::new(0.0, 0.0); dim];
    let mut hz = vec![C64::new(0.0, 0.0); dim];
    let scratch_len = if left { n } else { m };
    let mut scratch = vec![C64::new(0.0, 0.0); scratch_len];
    let mut embedded = vec![0.0; 2 * dim];
    embedded[..dim].copy_from_slice(&start);
    let (theta, converged) = lanczos_largest(embedded, |v, out| {
        for i in 0..dim {
            z[i] = C64::new(v[i], v[dim + i]);
        }
        apply_gram_complex(&buf, m, n, left, scale, &z, &mut scratch, &mut hz);
        for i in 0..dim {
            out[i] = hz[i].re;
            out[dim + i] = hz[i].im;
        }
    });
    let theta = if lanczos_ok(theta, theta_lower, converged) {
        theta
    } else {
        gram_largest_complex(&buf, m, n, left, scale)
    };
    scale * theta.max(0.0).sqrt()
}

fn largest_singular_value_slice(data: &[f64], m: usize, n: usize) -> f64 {
    if m == 0 || n == 0 {
        return 0.0;
    }
    debug_assert_eq!(data.len(), m * n);
    let mut any_nan = false;
    let mut any_inf = false;
    let mut scale = 0.0_f64;
    for &x in data {
        if x.is_nan() {
            any_nan = true;
        } else if x.is_infinite() {
            any_inf = true;
        } else {
            scale = scale.max(x.abs());
        }
    }
    if any_nan {
        return f64::NAN;
    }
    if any_inf {
        return f64::INFINITY;
    }
    if scale == 0.0 {
        return 0.0;
    }
    let left = m < n;
    let dim = if left { m } else { n };
    if dim == 1 {
        let mut sum_sq = 0.0;
        for &x in data {
            let y = x / scale;
            sum_sq += y * y;
        }
        return scale * sum_sq.sqrt();
    }
    let (start, lower) = axis_start(data, m, n, left, scale);
    let theta_lower = (lower / scale) * (lower / scale);
    let scratch_len = if left { n } else { m };
    let mut scratch = vec![0.0; scratch_len];
    let (theta, converged) = lanczos_largest(start, |v, out| {
        apply_gram(data, m, n, left, scale, v, &mut scratch, out);
    });
    let theta = if lanczos_ok(theta, theta_lower, converged) {
        theta
    } else {
        gram_largest(data, m, n, left, scale)
    };
    scale * theta.max(0.0).sqrt()
}

fn lanczos_ok(theta: f64, theta_lower: f64, converged: bool) -> bool {
    converged && theta.is_finite() && theta + 1e-8 * theta.max(1.0) >= theta_lower
}

fn frobenius_scaled_complex(data: &[C64], scale: f64) -> f64 {
    let mut sum_sq = 0.0;
    for z in data {
        let re = z.re / scale;
        let im = z.im / scale;
        sum_sq += re * re + im * im;
    }
    sum_sq.sqrt()
}

/// Column (or row) 2-norms, normalized. The second value is the largest
/// of those norms and is a lower bound on the singular value.
fn axis_start(data: &[f64], m: usize, n: usize, left: bool, scale: f64) -> (Vec<f64>, f64) {
    let dim = if left { m } else { n };
    let mut v = vec![0.0; dim];
    if left {
        for i in 0..m {
            let mut sum_sq = 0.0;
            let row = &data[i * n..(i + 1) * n];
            for &x in row {
                let y = x / scale;
                sum_sq += y * y;
            }
            v[i] = sum_sq.sqrt();
        }
    } else {
        for j in 0..n {
            let mut sum_sq = 0.0;
            for i in 0..m {
                let y = data[i * n + j] / scale;
                sum_sq += y * y;
            }
            v[j] = sum_sq.sqrt();
        }
    }
    finish_start(v, scale)
}

fn axis_start_complex(data: &[C64], m: usize, n: usize, left: bool, scale: f64) -> (Vec<f64>, f64) {
    let dim = if left { m } else { n };
    let mut v = vec![0.0; dim];
    if left {
        for i in 0..m {
            let mut sum_sq = 0.0;
            for k in 0..n {
                let z = data[i * n + k];
                let re = z.re / scale;
                let im = z.im / scale;
                sum_sq += re * re + im * im;
            }
            v[i] = sum_sq.sqrt();
        }
    } else {
        for j in 0..n {
            let mut sum_sq = 0.0;
            for i in 0..m {
                let z = data[i * n + j];
                let re = z.re / scale;
                let im = z.im / scale;
                sum_sq += re * re + im * im;
            }
            v[j] = sum_sq.sqrt();
        }
    }
    finish_start(v, scale)
}

fn finish_start(mut v: Vec<f64>, scale: f64) -> (Vec<f64>, f64) {
    let mut lower_scaled = 0.0_f64;
    for &x in &v {
        lower_scaled = lower_scaled.max(x);
    }
    let nrm = dot(&v, &v).sqrt();
    if nrm > 0.0 {
        let inv = 1.0 / nrm;
        for x in &mut v {
            *x *= inv;
        }
    }
    (v, lower_scaled * scale)
}

/// `out = Aᵀ A v` or `A Aᵀ v`, with `A` divided by `scale`.
fn apply_gram(
    data: &[f64],
    m: usize,
    n: usize,
    left: bool,
    scale: f64,
    v: &[f64],
    scratch: &mut [f64],
    out: &mut [f64],
) {
    if left {
        scratch.fill(0.0);
        for i in 0..m {
            let row = &data[i * n..(i + 1) * n];
            let s = v[i] / scale;
            for j in 0..n {
                scratch[j] += row[j] * s;
            }
        }
        for i in 0..m {
            let row = &data[i * n..(i + 1) * n];
            let mut sum = 0.0;
            for j in 0..n {
                sum += row[j] * scratch[j];
            }
            out[i] = sum / scale;
        }
    } else {
        for i in 0..m {
            let row = &data[i * n..(i + 1) * n];
            let mut sum = 0.0;
            for j in 0..n {
                sum += row[j] * v[j];
            }
            scratch[i] = sum / scale;
        }
        out.fill(0.0);
        for i in 0..m {
            let row = &data[i * n..(i + 1) * n];
            let s = scratch[i] / scale;
            for j in 0..n {
                out[j] += row[j] * s;
            }
        }
    }
}

fn apply_gram_complex(
    data: &[C64],
    m: usize,
    n: usize,
    left: bool,
    scale: f64,
    v: &[C64],
    scratch: &mut [C64],
    out: &mut [C64],
) {
    if left {
        scratch.fill(C64::new(0.0, 0.0));
        for i in 0..m {
            let s = v[i] / scale;
            for j in 0..n {
                scratch[j] += data[i * n + j].conj() * s;
            }
        }
        for i in 0..m {
            let mut sum = C64::new(0.0, 0.0);
            for j in 0..n {
                sum += data[i * n + j] * scratch[j];
            }
            out[i] = sum / scale;
        }
    } else {
        for i in 0..m {
            let mut sum = C64::new(0.0, 0.0);
            for j in 0..n {
                sum += data[i * n + j] * v[j];
            }
            scratch[i] = sum / scale;
        }
        out.fill(C64::new(0.0, 0.0));
        for i in 0..m {
            let s = scratch[i] / scale;
            for j in 0..n {
                out[j] += data[i * n + j].conj() * s;
            }
        }
    }
}

/// Largest eigenvalue of a symmetric operator, and whether the residual
/// test passed. `start` is a unit vector.
fn lanczos_largest(mut v: Vec<f64>, mut apply: impl FnMut(&[f64], &mut [f64])) -> (f64, bool) {
    let n = v.len();
    if n == 0 {
        return (0.0, true);
    }
    let steps = n.min(MAX_STEPS);
    let mut basis: Vec<Vec<f64>> = Vec::with_capacity(steps);
    let mut alpha: Vec<f64> = Vec::with_capacity(steps);
    let mut beta: Vec<f64> = Vec::with_capacity(steps);
    let mut w = vec![0.0; n];
    let mut theta = 0.0;
    for step in 0..steps {
        apply(&v, &mut w);
        let a = dot(&v, &w);
        alpha.push(a);
        axpy(&mut w, -a, &v);
        if step > 0 {
            axpy(&mut w, -beta[step - 1], &basis[step - 1]);
        }
        for _ in 0..2 {
            let c = dot(&v, &w);
            axpy(&mut w, -c, &v);
            for u in &basis {
                let c = dot(u, &w);
                axpy(&mut w, -c, u);
            }
        }
        let bnorm = dot(&w, &w).sqrt();
        basis.push(std::mem::replace(&mut v, Vec::new()));
        let (th, y_last) = ritz_largest(&alpha, &beta);
        theta = th;
        let rho = bnorm * y_last;
        let scale = th.abs().max(1e-30);
        if !bnorm.is_finite() || !th.is_finite() {
            return (theta, false);
        }
        if bnorm <= RESIDUAL_TOL * (1.0 + scale.sqrt()) || rho <= RESIDUAL_TOL * scale {
            return (th.max(0.0), true);
        }
        // A full basis is the whole space. The Ritz value is an eigenvalue.
        if step + 1 == n {
            return (th.max(0.0), true);
        }
        if step + 1 == steps {
            break;
        }
        beta.push(bnorm);
        let inv = 1.0 / bnorm;
        for x in &mut w {
            *x *= inv;
        }
        v = std::mem::take(&mut w);
        w = vec![0.0; n];
    }
    (theta.max(0.0), false)
}

/// Largest Ritz pair of the Lanczos tridiagonal. Returns `(θ, |y_last|)`.
///
/// Small blocks use Jacobi. Larger ones use a Sturm bisection and one
/// inverse iteration, which is O(m) rather than a dense eigen-decomposition
/// on every Lanczos step.
fn ritz_largest(alpha: &[f64], beta: &[f64]) -> (f64, f64) {
    let m = alpha.len();
    if m == 0 {
        return (0.0, 0.0);
    }
    if m == 1 {
        return (alpha[0], 1.0);
    }
    if m <= 8 {
        return ritz_jacobi(alpha, beta);
    }
    let theta = tridiag_largest(alpha, beta);
    let y_last = tridiag_last_component(alpha, beta, theta);
    (theta, y_last)
}

fn ritz_jacobi(alpha: &[f64], beta: &[f64]) -> (f64, f64) {
    let m = alpha.len();
    let mut t = Array2::<f64>::zeros((m, m));
    for i in 0..m {
        t[[i, i]] = alpha[i];
        if i + 1 < m {
            let b = beta[i];
            t[[i, i + 1]] = b;
            t[[i + 1, i]] = b;
        }
    }
    let (vals, vecs) = sym_eig_jacobi(&t);
    let mut k = 0;
    for i in 1..vals.len() {
        if vals[i] > vals[k] {
            k = i;
        }
    }
    (vals[k], vecs[[m - 1, k]].abs())
}

/// Largest eigenvalue of a symmetric tridiagonal, by bisection.
/// `sturm_lt` counts eigenvalues strictly below the shift.
fn tridiag_largest(alpha: &[f64], beta: &[f64]) -> f64 {
    let m = alpha.len();
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for i in 0..m {
        let mut radius = 0.0;
        if i > 0 {
            radius += beta[i - 1].abs();
        }
        if i + 1 < m {
            radius += beta[i].abs();
        }
        lo = lo.min(alpha[i] - radius);
        hi = hi.max(alpha[i] + radius);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return 0.0;
    }
    for _ in 0..80 {
        if hi - lo <= 1e-15 * hi.abs().max(1.0) {
            break;
        }
        let mid = 0.5 * (lo + hi);
        if sturm_lt(alpha, beta, mid) >= m {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    0.5 * (lo + hi)
}

fn sturm_lt(alpha: &[f64], beta: &[f64], sigma: f64) -> usize {
    let mut count = 0usize;
    let mut q = alpha[0] - sigma;
    if q < 0.0 {
        count += 1;
    }
    if q.abs() < 1e-30 {
        q = 1e-30;
    }
    for i in 1..alpha.len() {
        let b2 = beta[i - 1] * beta[i - 1];
        q = (alpha[i] - sigma) - b2 / q;
        if q < 0.0 {
            count += 1;
        }
        if q.abs() < 1e-30 {
            q = if q.is_sign_negative() { -1e-30 } else { 1e-30 };
        }
    }
    count
}

/// Absolute value of the last component of the top eigenvector.
/// A singular shift means the right-hand side is already that direction.
fn tridiag_last_component(alpha: &[f64], beta: &[f64], theta: f64) -> f64 {
    let m = alpha.len();
    let mut y = vec![0.0; m];
    y[m - 1] = 1.0;
    for _ in 0..8 {
        let Some(z) = tridiag_solve(alpha, beta, theta, &y) else {
            break;
        };
        let nrm = dot(&z, &z).sqrt();
        if !nrm.is_finite() || nrm == 0.0 {
            break;
        }
        let inv = 1.0 / nrm;
        for (yi, zi) in y.iter_mut().zip(&z) {
            *yi = zi * inv;
        }
    }
    y[m - 1].abs()
}

/// Solve `(T - θI) x = rhs`. `None` when a pivot vanishes.
fn tridiag_solve(alpha: &[f64], beta: &[f64], theta: f64, rhs: &[f64]) -> Option<Vec<f64>> {
    let m = alpha.len();
    let mut c = vec![0.0; m];
    let mut d = vec![0.0; m];
    let b0 = alpha[0] - theta;
    if b0.abs() < 1e-30 {
        return None;
    }
    c[0] = if m > 1 { beta[0] / b0 } else { 0.0 };
    d[0] = rhs[0] / b0;
    for i in 1..m {
        let sub = beta[i - 1];
        let diag = alpha[i] - theta;
        let denom = diag - sub * c[i - 1];
        if denom.abs() < 1e-30 {
            return None;
        }
        c[i] = if i + 1 < m { beta[i] / denom } else { 0.0 };
        d[i] = (rhs[i] - sub * d[i - 1]) / denom;
    }
    let mut x = vec![0.0; m];
    x[m - 1] = d[m - 1];
    for i in (0..m - 1).rev() {
        x[i] = d[i] - c[i] * x[i + 1];
    }
    Some(x)
}

fn gram_largest(data: &[f64], m: usize, n: usize, left: bool, scale: f64) -> f64 {
    let p = if left { m } else { n };
    let mut g = Array2::<f64>::zeros((p, p));
    if left {
        for i in 0..m {
            for j in 0..=i {
                let mut sum = 0.0;
                for k in 0..n {
                    sum += (data[i * n + k] / scale) * (data[j * n + k] / scale);
                }
                g[[i, j]] = sum;
                g[[j, i]] = sum;
            }
        }
    } else {
        for i in 0..n {
            for j in 0..=i {
                let mut sum = 0.0;
                for r in 0..m {
                    sum += (data[r * n + i] / scale) * (data[r * n + j] / scale);
                }
                g[[i, j]] = sum;
                g[[j, i]] = sum;
            }
        }
    }
    jacobi_largest(&g)
}

fn gram_largest_complex(data: &[C64], m: usize, n: usize, left: bool, scale: f64) -> f64 {
    let p = if left { m } else { n };
    let mut g = Array2::<C64>::zeros((p, p));
    if left {
        for i in 0..m {
            for j in 0..=i {
                let mut sum = C64::new(0.0, 0.0);
                for k in 0..n {
                    sum += (data[i * n + k] / scale) * (data[j * n + k] / scale).conj();
                }
                g[[i, j]] = sum;
                g[[j, i]] = sum.conj();
            }
        }
    } else {
        for i in 0..n {
            for j in 0..=i {
                let mut sum = C64::new(0.0, 0.0);
                for r in 0..m {
                    sum += (data[r * n + i] / scale).conj() * (data[r * n + j] / scale);
                }
                g[[i, j]] = sum;
                g[[j, i]] = sum.conj();
            }
        }
    }
    // Real embedding of a Hermitian matrix. Eigenvalues are unchanged.
    let n2 = 2 * p;
    let mut emb = Array2::<f64>::zeros((n2, n2));
    for i in 0..p {
        for j in 0..p {
            let re = g[[i, j]].re;
            let im = g[[i, j]].im;
            emb[[i, j]] = re;
            emb[[i + p, j + p]] = re;
            emb[[i, j + p]] = -im;
            emb[[i + p, j]] = im;
        }
    }
    jacobi_largest(&emb)
}

/// Largest eigenvalue of a symmetric matrix. Eigenvectors are not stored.
fn jacobi_largest(a: &Array2<f64>) -> f64 {
    let n = a.nrows();
    if n == 0 {
        return 0.0;
    }
    let mut t = a.to_owned();
    if n == 1 {
        return t[[0, 0]].max(0.0);
    }
    for _ in 0..50 {
        let mut off = 0.0;
        for i in 0..n {
            for j in (i + 1)..n {
                off += t[[i, j]] * t[[i, j]];
            }
        }
        let diag: f64 = (0..n).map(|i| t[[i, i]] * t[[i, i]]).sum();
        if off <= 1e-28 * (diag + off) {
            break;
        }
        for p in 0..n - 1 {
            for r in (p + 1)..n {
                let apq = t[[p, r]];
                if apq.abs() < 1e-300 {
                    continue;
                }
                let theta = (t[[r, r]] - t[[p, p]]) / (2.0 * apq);
                let tau = if theta >= 0.0 {
                    1.0 / (theta + (1.0 + theta * theta).sqrt())
                } else {
                    1.0 / (theta - (1.0 + theta * theta).sqrt())
                };
                let c = 1.0 / (1.0 + tau * tau).sqrt();
                let s = tau * c;
                let app = t[[p, p]];
                let arr = t[[r, r]];
                t[[p, p]] = app - tau * apq;
                t[[r, r]] = arr + tau * apq;
                t[[p, r]] = 0.0;
                t[[r, p]] = 0.0;
                for k in 0..n {
                    if k == p || k == r {
                        continue;
                    }
                    let tkp = t[[k, p]];
                    let tkr = t[[k, r]];
                    t[[k, p]] = c * tkp - s * tkr;
                    t[[k, r]] = s * tkp + c * tkr;
                    t[[p, k]] = t[[k, p]];
                    t[[r, k]] = t[[k, r]];
                }
            }
        }
    }
    let mut best = t[[0, 0]];
    for i in 1..n {
        best = best.max(t[[i, i]]);
    }
    best.max(0.0)
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn axpy(y: &mut [f64], alpha: f64, x: &[f64]) {
    for (yi, xi) in y.iter_mut().zip(x) {
        *yi += alpha * xi;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    fn close(got: f64, want: f64) {
        let tol = 1e-10 * (1.0 + want.abs());
        assert!(
            (got - want).abs() <= tol,
            "got {got}, want {want}, |diff| {}",
            (got - want).abs()
        );
    }

    #[test]
    fn two_by_two_closed_form() {
        // λ_max(AᵀA) = 15 + sqrt(221), so σ = sqrt of that.
        let a = Array2::from_shape_vec((2, 2), vec![1.0, -2.0, -3.0, 4.0]).unwrap();
        let want = (15.0 + 221.0_f64.sqrt()).sqrt();
        close(largest_singular_value(&a), want);
        assert!((want - 5.464985704219043).abs() < 1e-12);
    }

    #[test]
    fn diagonal_is_the_largest_magnitude() {
        let a = Array2::from_shape_vec((2, 2), vec![3.0, 0.0, 0.0, -4.0]).unwrap();
        close(largest_singular_value(&a), 4.0);
    }

    #[test]
    fn rank_one_matches_frobenius() {
        // Columns are parallel, so the only nonzero singular value is the
        // Frobenius norm.
        let a = Array2::from_shape_vec((2, 2), vec![1.0, 2.0, 2.0, 4.0]).unwrap();
        close(largest_singular_value(&a), 5.0);
        let wide = Array2::from_shape_vec((2, 3), vec![1.0, 2.0, 3.0, 2.0, 4.0, 6.0]).unwrap();
        close(largest_singular_value(&wide), 70.0_f64.sqrt());
        let tall = Array2::from_shape_vec((3, 2), vec![1.0, 2.0, 2.0, 4.0, 3.0, 6.0]).unwrap();
        close(largest_singular_value(&tall), 70.0_f64.sqrt());
    }

    #[test]
    fn rotated_diagonal_keeps_the_largest_entry() {
        // Q diag(3, -4) Qᵀ with a plane rotation. σ_max stays 4.
        let (c, s) = (0.6, 0.8);
        let a00 = 3.0 * c * c - 4.0 * s * s;
        let a11 = 3.0 * s * s - 4.0 * c * c;
        let a01 = 7.0 * c * s;
        let a = Array2::from_shape_vec((2, 2), vec![a00, a01, a01, a11]).unwrap();
        close(largest_singular_value(&a), 4.0);
    }

    #[test]
    fn empty_zero_and_identity() {
        let empty = Array2::<f64>::zeros((0, 3));
        assert_eq!(largest_singular_value(&empty), 0.0);
        assert_eq!(largest_singular_value(&Array2::<f64>::zeros((4, 0))), 0.0);
        assert_eq!(largest_singular_value(&Array2::<f64>::zeros((0, 0))), 0.0);
        assert_eq!(largest_singular_value(&Array2::<f64>::zeros((3, 5))), 0.0);
        let eye = Array2::<f64>::eye(4);
        close(largest_singular_value(&eye), 1.0);
        let one = Array2::from_elem((1, 1), -5.0);
        close(largest_singular_value(&one), 5.0);
    }

    #[test]
    fn nan_and_inf() {
        let mut nan = Array2::from_elem((2, 2), 1.0);
        nan[[1, 0]] = f64::NAN;
        assert!(largest_singular_value(&nan).is_nan());
        let mut inf = Array2::from_elem((2, 2), 1.0);
        inf[[0, 1]] = f64::INFINITY;
        assert!(largest_singular_value(&inf).is_infinite());
        let mut both = Array2::from_elem((2, 2), 0.0);
        both[[0, 0]] = f64::INFINITY;
        both[[1, 1]] = f64::NAN;
        assert!(largest_singular_value(&both).is_nan());
    }

    #[test]
    fn scale_does_not_overflow() {
        let a = Array2::from_shape_vec((2, 2), vec![3.0e200, 0.0, 0.0, -4.0e200]).unwrap();
        close(largest_singular_value(&a), 4.0e200);
        let tiny = Array2::from_shape_vec((2, 2), vec![3.0e-200, 0.0, 0.0, -4.0e-200]).unwrap();
        close(largest_singular_value(&tiny), 4.0e-200);
    }

    #[test]
    fn complex_closed_form_and_real_bits() {
        // A = [[1, i], [0, 2]], λ_max(AᴴA) = 3 + sqrt(5).
        let a = Array2::from_shape_vec(
            (2, 2),
            vec![
                C64::new(1.0, 0.0),
                C64::new(0.0, 1.0),
                C64::new(0.0, 0.0),
                C64::new(2.0, 0.0),
            ],
        )
        .unwrap();
        close(
            largest_singular_value_complex(&a),
            (3.0 + 5.0_f64.sqrt()).sqrt(),
        );
        let real = Array2::from_shape_vec((2, 2), vec![1.0, -2.0, -3.0, 4.0]).unwrap();
        let as_complex = Array2::from_shape_fn(real.dim(), |(i, j)| C64::new(real[[i, j]], 0.0));
        assert_eq!(
            largest_singular_value(&real).to_bits(),
            largest_singular_value_complex(&as_complex).to_bits()
        );
    }

    #[test]
    fn complex_nan_inf_and_rank_deficient() {
        let mut nan = Array2::from_elem((2, 2), C64::new(1.0, 0.0));
        nan[[0, 1]] = C64::new(f64::NAN, 0.0);
        assert!(largest_singular_value_complex(&nan).is_nan());
        let mut inf = Array2::from_elem((1, 2), C64::new(0.0, 0.0));
        inf[[0, 0]] = C64::new(0.0, f64::INFINITY);
        assert!(largest_singular_value_complex(&inf).is_infinite());
        // Rank 1: outer product [1, i]ᵀ [1, 2]. σ = sqrt(2) * sqrt(5).
        let u = [C64::new(1.0, 0.0), C64::new(0.0, 1.0)];
        let v = [C64::new(1.0, 0.0), C64::new(2.0, 0.0)];
        let mut a = Array2::<C64>::zeros((2, 2));
        for i in 0..2 {
            for j in 0..2 {
                a[[i, j]] = u[i] * v[j];
            }
        }
        close(largest_singular_value_complex(&a), (10.0_f64).sqrt());
    }

    #[test]
    fn agrees_with_the_gram_on_a_random_matrix() {
        // Fixed xorshift so the case does not depend on the script RNG.
        let mut state = 0x1234_5678_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        let m = 12;
        let n = 9;
        let mut data = Vec::with_capacity(m * n);
        for _ in 0..(m * n) {
            data.push(next());
        }
        let got = largest_singular_value_slice(&data, m, n);
        let want_theta = gram_largest(&data, m, n, false, 1.0);
        // `gram_largest` divides by the scale it is given. Pass the same
        // scale the public path would, by comparing σ² against an
        // unscaled Gram built at scale 1. The matrix entries are O(1).
        close(got, want_theta.sqrt());
    }
}

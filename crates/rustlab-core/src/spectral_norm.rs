//! Largest singular value of a dense matrix (the matrix 2-norm).
//!
//! The value is `sqrt(λ_max)` of the smaller Gram matrix, `AᴴA` when
//! `A` has at least as many rows as columns and `AAᴴ` otherwise. Lanczos
//! applies that product without forming it and without singular vectors.
//! The iteration count is capped. A Hermitian Jacobi of the Gram is the
//! fallback only while the smaller side is tiny; on a 512×512 matrix that
//! Jacobi ran for minutes after 64 Lanczos steps missed a 1e-14 residual.
//! Entries are scaled by the largest magnitude so a matrix of `1e200` does
//! not overflow the product. [`NormCancelled`] is how a caller stops early.

use ndarray::Array2;

use crate::sparse_eig::sym_eig_jacobi;
use crate::types::C64;

/// Stop when the Ritz residual is this fraction of the Rayleigh quotient.
const RESIDUAL_TOL: f64 = 1e-14;
/// Hard cap on Lanczos steps. Past this the Ritz value is the answer,
/// unless the smaller side is small enough for an exact Gram Jacobi.
const MAX_STEPS: usize = 256;
/// Gram Jacobi is O(p³) per sweep. Above this side length it is not a fallback.
const JACOBI_MAX_DIM: usize = 96;
/// Two passes over the entries per Lanczos step. The step count stays
/// under this budget so a multi-gigabyte matrix cannot run for minutes.
const ENTRY_BUDGET: usize = 800_000_000;

/// The caller asked to stop (Ctrl+C, or a test hook) before the 2-norm finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormCancelled;

fn step_limit(space: usize, entries: usize) -> usize {
    let by_budget = (ENTRY_BUDGET / entries.max(1)).max(32);
    by_budget.min(MAX_STEPS).min(space.max(1))
}

fn halted(stop: &mut dyn FnMut() -> bool) -> Result<(), NormCancelled> {
    if stop() {
        Err(NormCancelled)
    } else {
        Ok(())
    }
}

/// Largest singular value of a real matrix. Empty is 0. A `NaN` entry
/// returns `NaN`. An `Inf` entry returns `Inf` when no entry is `NaN`.
pub fn largest_singular_value(a: &Array2<f64>) -> f64 {
    largest_singular_value_cancellable(a, &mut || false).expect("stop callback never fires")
}

/// [`largest_singular_value`], returning [`NormCancelled`] when `stop` is true.
/// `stop` is polled between Lanczos steps and while the matrix is scanned.
pub fn largest_singular_value_cancellable(
    a: &Array2<f64>,
    stop: &mut dyn FnMut() -> bool,
) -> Result<f64, NormCancelled> {
    let (m, n) = a.dim();
    if let Some(slice) = a.as_slice() {
        largest_singular_value_slice(slice, m, n, stop)
    } else {
        let buf: Vec<f64> = a.iter().copied().collect();
        largest_singular_value_slice(&buf, m, n, stop)
    }
}

/// Largest singular value of a complex matrix. A matrix whose imaginary
/// part is all `+0` uses the real routine, so the bits match real storage.
pub fn largest_singular_value_complex(a: &Array2<C64>) -> f64 {
    largest_singular_value_complex_cancellable(a, &mut || false).expect("stop callback never fires")
}

/// [`largest_singular_value_complex`], returning [`NormCancelled`] when `stop` is true.
pub fn largest_singular_value_complex_cancellable(
    a: &Array2<C64>,
    stop: &mut dyn FnMut() -> bool,
) -> Result<f64, NormCancelled> {
    if a.iter().all(|z| z.im == 0.0) {
        let buf: Vec<f64> = a.iter().map(|z| z.re).collect();
        return largest_singular_value_slice(&buf, a.nrows(), a.ncols(), stop);
    }
    halted(stop)?;
    let (m, n) = a.dim();
    if m == 0 || n == 0 {
        return Ok(0.0);
    }
    let buf: Vec<C64> = a.iter().copied().collect();
    let mut any_nan = false;
    let mut any_inf = false;
    let mut scale = 0.0_f64;
    for (i, z) in buf.iter().enumerate() {
        if i % 8192 == 0 {
            halted(stop)?;
        }
        if z.re.is_nan() || z.im.is_nan() {
            any_nan = true;
        } else if z.re.is_infinite() || z.im.is_infinite() {
            any_inf = true;
        } else {
            scale = scale.max(z.norm());
        }
    }
    if any_nan {
        return Ok(f64::NAN);
    }
    if any_inf {
        return Ok(f64::INFINITY);
    }
    if scale == 0.0 {
        return Ok(0.0);
    }
    let left = m < n;
    let dim = if left { m } else { n };
    if dim == 1 {
        return Ok(scale * frobenius_scaled_complex(&buf, scale));
    }
    let (start, lower) = axis_start_complex(&buf, m, n, left, scale);
    let theta_lower = (lower / scale) * (lower / scale);
    let mut z = vec![C64::new(0.0, 0.0); dim];
    let mut hz = vec![C64::new(0.0, 0.0); dim];
    let scratch_len = if left { n } else { m };
    let mut scratch = vec![C64::new(0.0, 0.0); scratch_len];
    let mut embedded = vec![0.0; 2 * dim];
    embedded[..dim].copy_from_slice(&start);
    let steps = step_limit(embedded.len(), m.saturating_mul(n));
    let (theta, converged) = lanczos_largest(embedded, steps, stop, |v, out| {
        for i in 0..dim {
            z[i] = C64::new(v[i], v[dim + i]);
        }
        apply_gram_complex(&buf, m, n, left, scale, &z, &mut scratch, &mut hz);
        for i in 0..dim {
            out[i] = hz[i].re;
            out[dim + i] = hz[i].im;
        }
    })?;
    let theta = if lanczos_ok(theta, theta_lower, converged) {
        theta
    } else if dim <= JACOBI_MAX_DIM {
        halted(stop)?;
        gram_largest_complex(&buf, m, n, left, scale)
    } else {
        theta.max(theta_lower)
    };
    Ok((scale * theta.max(0.0).sqrt()).max(lower))
}

fn largest_singular_value_slice(
    data: &[f64],
    m: usize,
    n: usize,
    stop: &mut dyn FnMut() -> bool,
) -> Result<f64, NormCancelled> {
    if m == 0 || n == 0 {
        return Ok(0.0);
    }
    debug_assert_eq!(data.len(), m * n);
    halted(stop)?;
    let mut any_nan = false;
    let mut any_inf = false;
    let mut scale = 0.0_f64;
    for (i, &x) in data.iter().enumerate() {
        if i % 8192 == 0 {
            halted(stop)?;
        }
        if x.is_nan() {
            any_nan = true;
        } else if x.is_infinite() {
            any_inf = true;
        } else {
            scale = scale.max(x.abs());
        }
    }
    if any_nan {
        return Ok(f64::NAN);
    }
    if any_inf {
        return Ok(f64::INFINITY);
    }
    if scale == 0.0 {
        return Ok(0.0);
    }
    let left = m < n;
    let dim = if left { m } else { n };
    if dim == 1 {
        let mut sum_sq = 0.0;
        for &x in data {
            let y = x / scale;
            sum_sq += y * y;
        }
        return Ok(scale * sum_sq.sqrt());
    }
    let (start, lower) = axis_start(data, m, n, left, scale);
    let theta_lower = (lower / scale) * (lower / scale);
    let scratch_len = if left { n } else { m };
    let mut scratch = vec![0.0; scratch_len];
    let steps = step_limit(dim, m.saturating_mul(n));
    let (theta, converged) = lanczos_largest(start, steps, stop, |v, out| {
        apply_gram(data, m, n, left, scale, v, &mut scratch, out);
    })?;
    let theta = if lanczos_ok(theta, theta_lower, converged) {
        theta
    } else if dim <= JACOBI_MAX_DIM {
        halted(stop)?;
        gram_largest(data, m, n, left, scale)
    } else {
        theta.max(theta_lower)
    };
    // `lower` is already in the original scaling. `theta` is an eigenvalue
    // of the scaled Gram, so the singular value is `scale * sqrt(theta)`.
    Ok((scale * theta.max(0.0).sqrt()).max(lower))
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
/// test passed. `start` is a unit vector. `steps` is the hard iteration cap.
fn lanczos_largest(
    mut v: Vec<f64>,
    steps: usize,
    stop: &mut dyn FnMut() -> bool,
    mut apply: impl FnMut(&[f64], &mut [f64]),
) -> Result<(f64, bool), NormCancelled> {
    let n = v.len();
    if n == 0 {
        return Ok((0.0, true));
    }
    let steps = steps.min(n).max(1);
    let mut basis: Vec<Vec<f64>> = Vec::with_capacity(steps);
    let mut alpha: Vec<f64> = Vec::with_capacity(steps);
    let mut beta: Vec<f64> = Vec::with_capacity(steps);
    let mut w = vec![0.0; n];
    let mut theta = 0.0;
    for step in 0..steps {
        halted(stop)?;
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
            return Ok((theta, false));
        }
        if bnorm <= RESIDUAL_TOL * (1.0 + scale.sqrt()) || rho <= RESIDUAL_TOL * scale {
            return Ok((th.max(0.0), true));
        }
        // A full basis is the whole space. The Ritz value is an eigenvalue.
        if step + 1 == n {
            return Ok((th.max(0.0), true));
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
    Ok((theta.max(0.0), false))
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
        let got = largest_singular_value_slice(&data, m, n, &mut || false).unwrap();
        let want_theta = gram_largest(&data, m, n, false, 1.0);
        // `gram_largest` divides by the scale it is given. Pass the same
        // scale the public path would, by comparing σ² against an
        // unscaled Gram built at scale 1. The matrix entries are O(1).
        close(got, want_theta.sqrt());
    }

    #[test]
    fn diagonal_400_is_the_largest_entry_and_finishes() {
        let n = 400usize;
        let mut data = vec![0.0; n * n];
        for i in 0..n {
            data[i * n + i] = (i + 1) as f64;
        }
        let a = Array2::from_shape_vec((n, n), data).unwrap();
        let t = std::time::Instant::now();
        let s = largest_singular_value(&a);
        assert!(t.elapsed().as_secs() < 2, "{:?}", t.elapsed());
        close(s, n as f64);
    }

    #[test]
    fn random_512_stays_between_column_and_frobenius_and_finishes() {
        // A 512×512 random matrix used to miss the 1e-14 Lanczos test after
        // 64 steps and then run a Gram Jacobi for minutes. The value has to
        // come back, and it has to sit between the two norms that bound σ_max.
        let mut state = 0xC0FFEE_u64;
        let n = 512usize;
        let mut data = Vec::with_capacity(n * n);
        for _ in 0..(n * n) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            data.push((state >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0);
        }
        let mut fro = 0.0_f64;
        let mut col_max = 0.0_f64;
        for j in 0..n {
            let mut col = 0.0;
            for i in 0..n {
                let x = data[i * n + j];
                fro += x * x;
                col += x * x;
            }
            col_max = col_max.max(col.sqrt());
        }
        let a = Array2::from_shape_vec((n, n), data).unwrap();
        let t = std::time::Instant::now();
        let s = largest_singular_value(&a);
        let elapsed = t.elapsed();
        assert!(
            elapsed.as_secs() < 5,
            "512×512 2-norm took {elapsed:?}; the Gram Jacobi fallback used to take minutes"
        );
        assert!(s.is_finite(), "{s}");
        assert!(s + 1e-8 >= col_max, "σ {s} below column norm {col_max}");
        assert!(s <= fro.sqrt() * (1.0 + 1e-8), "σ {s} above Frobenius");
    }

    /// Reference 2-norm: largest eigenvalue of the smaller Gram, via the
    /// Hermitian eigensolver (the same value a full SVD reports as σ_max).
    fn reference_sigma(data: &[f64], m: usize, n: usize) -> f64 {
        let left = m < n;
        let p = if left { m } else { n };
        let mut g = Array2::<C64>::zeros((p, p));
        if left {
            for i in 0..m {
                for j in 0..=i {
                    let mut sum = 0.0;
                    for k in 0..n {
                        sum += data[i * n + k] * data[j * n + k];
                    }
                    let z = C64::new(sum, 0.0);
                    g[[i, j]] = z;
                    g[[j, i]] = z;
                }
            }
        } else {
            for i in 0..n {
                for j in 0..=i {
                    let mut sum = 0.0;
                    for r in 0..m {
                        sum += data[r * n + i] * data[r * n + j];
                    }
                    let z = C64::new(sum, 0.0);
                    g[[i, j]] = z;
                    g[[j, i]] = z;
                }
            }
        }
        let (vals, _) = crate::dense_eig::eig(&g, false).unwrap();
        let lam = vals.iter().map(|z| z.re).fold(f64::NEG_INFINITY, f64::max);
        lam.max(0.0).sqrt()
    }

    fn reference_sigma_complex(data: &[C64], m: usize, n: usize) -> f64 {
        let left = m <= n;
        let p = if left { m } else { n };
        let mut g = Array2::<C64>::zeros((p, p));
        if left {
            for i in 0..m {
                for j in 0..=i {
                    let mut sum = C64::new(0.0, 0.0);
                    for k in 0..n {
                        sum += data[i * n + k] * data[j * n + k].conj();
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
                        sum += data[r * n + i].conj() * data[r * n + j];
                    }
                    g[[i, j]] = sum;
                    g[[j, i]] = sum.conj();
                }
            }
        }
        let (vals, _) = crate::dense_eig::eig(&g, false).unwrap();
        let lam = vals.iter().map(|z| z.re).fold(f64::NEG_INFINITY, f64::max);
        lam.max(0.0).sqrt()
    }

    fn xorshift(state: &mut u64) -> f64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        (*state >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }

    fn rel_err(got: f64, want: f64) -> f64 {
        if want == 0.0 {
            got.abs()
        } else {
            (got - want).abs() / want.abs()
        }
    }

    fn report(name: &str, got: f64, want: f64, elapsed: std::time::Duration) -> f64 {
        let rel = rel_err(got, want);
        eprintln!("{name}: got {got:.12e}  svd {want:.12e}  rel {rel:.3e}  {elapsed:?}");
        rel
    }

    /// Accuracy against the Hermitian eig of the Gram (the SVD's σ_max).
    /// Not part of `cargo test`.
    ///
    /// ```text
    /// cargo test -p rustlab-core --release --lib accuracy_against_svd -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore]
    fn accuracy_against_svd() {
        let mut worst = 0.0_f64;
        let mut state = 0xC0FFEE_u64;
        for n in [97usize, 128, 256, 512] {
            let mut data = Vec::with_capacity(n * n);
            for _ in 0..(n * n) {
                data.push(xorshift(&mut state));
            }
            let a = Array2::from_shape_vec((n, n), data.clone()).unwrap();
            let t = std::time::Instant::now();
            let got = largest_singular_value(&a);
            let elapsed = t.elapsed();
            let want = reference_sigma(&data, n, n);
            worst = worst.max(report(&format!("random {n}"), got, want, elapsed));
        }

        // Hilbert: huge condition number, σ_max still well separated.
        for n in [64usize, 128] {
            let mut data = vec![0.0; n * n];
            for i in 0..n {
                for j in 0..n {
                    data[i * n + j] = 1.0 / (i + j + 1) as f64;
                }
            }
            let a = Array2::from_shape_vec((n, n), data.clone()).unwrap();
            let t = std::time::Instant::now();
            let got = largest_singular_value(&a);
            worst = worst.max(report(
                &format!("hilbert {n}"),
                got,
                reference_sigma(&data, n, n),
                t.elapsed(),
            ));
        }

        // Badly scaled diagonal, then a Householder similarity so it is dense.
        // Singular values are the absolute diagonal entries.
        {
            let n = 128usize;
            let mut u = vec![0.0; n];
            for x in &mut u {
                *x = xorshift(&mut state);
            }
            let unorm = u.iter().map(|x| x * x).sum::<f64>().sqrt();
            for x in &mut u {
                *x /= unorm;
            }
            let mut s = vec![0.0; n];
            s[0] = 1e8;
            s[1] = 1.0;
            s[2] = 1e-8;
            for i in 3..n {
                s[i] = 0.1;
            }
            // A = (I - 2uuᵀ) diag(s) (I - 2uuᵀ). Eigenvalues are s.
            let mut data = vec![0.0; n * n];
            for i in 0..n {
                for j in 0..n {
                    // (H D H)_ij = δ_ij s_j - 2 u_i u_j s_j - 2 u_i u_j s_i
                    //   + 4 u_i u_j (u·(s⊙u))
                    let mut dot_su = 0.0;
                    for k in 0..n {
                        dot_su += u[k] * s[k] * u[k];
                    }
                    let hij = if i == j { s[j] } else { 0.0 }
                        - 2.0 * u[i] * u[j] * s[j]
                        - 2.0 * u[i] * u[j] * s[i]
                        + 4.0 * u[i] * u[j] * dot_su;
                    data[i * n + j] = hij;
                }
            }
            let a = Array2::from_shape_vec((n, n), data.clone()).unwrap();
            let t = std::time::Instant::now();
            let got = largest_singular_value(&a);
            worst = worst.max(report(
                "scaled-householder 128",
                got,
                reference_sigma(&data, n, n),
                t.elapsed(),
            ));
        }

        // Top two singular values nearly equal. A = U diag(s) Vᵀ with
        // U, V from modified Gram-Schmidt of Gaussian matrices.
        for (gap, n) in [(1e-4, 96usize), (1e-6, 128usize), (1e-3, 256usize)] {
            let mut cols_u = vec![vec![0.0; n]; n];
            let mut cols_v = vec![vec![0.0; n]; n];
            for set in [&mut cols_u, &mut cols_v] {
                for col in set.iter_mut() {
                    for x in col.iter_mut() {
                        *x = xorshift(&mut state);
                    }
                }
                for i in 0..n {
                    for j in 0..i {
                        let dot: f64 = cols_u_dot(&set[j], &set[i]);
                        for k in 0..n {
                            set[i][k] -= dot * set[j][k];
                        }
                    }
                    let nrm = set[i].iter().map(|x| x * x).sum::<f64>().sqrt();
                    if nrm > 0.0 {
                        for x in &mut set[i] {
                            *x /= nrm;
                        }
                    }
                }
            }
            fn cols_u_dot(a: &[f64], b: &[f64]) -> f64 {
                a.iter().zip(b).map(|(x, y)| x * y).sum()
            }
            let mut s = vec![0.5; n];
            s[0] = 1.0;
            s[1] = 1.0 - gap;
            let mut data = vec![0.0; n * n];
            for i in 0..n {
                for j in 0..n {
                    let mut acc = 0.0;
                    for k in 0..n {
                        acc += cols_u[k][i] * s[k] * cols_v[k][j];
                    }
                    data[i * n + j] = acc;
                }
            }
            let a = Array2::from_shape_vec((n, n), data.clone()).unwrap();
            let t = std::time::Instant::now();
            let got = largest_singular_value(&a);
            worst = worst.max(report(
                &format!("cluster gap {gap} n {n}"),
                got,
                reference_sigma(&data, n, n),
                t.elapsed(),
            ));
        }

        // Rank deficient: random 128×128 with the last 40 columns zero.
        {
            let (m, n) = (128usize, 128usize);
            let mut data = vec![0.0; m * n];
            for i in 0..m {
                for j in 0..(n - 40) {
                    data[i * n + j] = xorshift(&mut state);
                }
            }
            let a = Array2::from_shape_vec((m, n), data.clone()).unwrap();
            let t = std::time::Instant::now();
            let got = largest_singular_value(&a);
            worst = worst.max(report(
                "rank-deficient 128",
                got,
                reference_sigma(&data, m, n),
                t.elapsed(),
            ));
        }

        for (m, n, label) in [
            (300usize, 40usize, "tall"),
            (40, 300, "wide"),
            (200, 80, "tall2"),
            (80, 200, "wide2"),
        ] {
            let mut data = Vec::with_capacity(m * n);
            for _ in 0..(m * n) {
                data.push(xorshift(&mut state));
            }
            let a = Array2::from_shape_vec((m, n), data.clone()).unwrap();
            let t = std::time::Instant::now();
            let got = largest_singular_value(&a);
            worst = worst.max(report(
                &format!("{label} {m}x{n}"),
                got,
                reference_sigma(&data, m, n),
                t.elapsed(),
            ));
        }

        // Tighter than the cases above, and a spectrum with no isolated gap
        // (singular values 1, 1+Δ, …). Both sit past the Jacobi cutoff.
        for (gap, n) in [(1e-8, 160usize)] {
            let mut cols_u = vec![vec![0.0; n]; n];
            let mut cols_v = vec![vec![0.0; n]; n];
            for set in [&mut cols_u, &mut cols_v] {
                for col in set.iter_mut() {
                    for x in col.iter_mut() {
                        *x = xorshift(&mut state);
                    }
                }
                for i in 0..n {
                    for j in 0..i {
                        let dot: f64 = set[j].iter().zip(set[i].iter()).map(|(a, b)| a * b).sum();
                        for k in 0..n {
                            set[i][k] -= dot * set[j][k];
                        }
                    }
                    let nrm = set[i].iter().map(|x| x * x).sum::<f64>().sqrt();
                    if nrm > 0.0 {
                        for x in &mut set[i] {
                            *x /= nrm;
                        }
                    }
                }
            }
            let mut s = vec![0.5; n];
            s[0] = 1.0;
            s[1] = 1.0 - gap;
            let mut data = vec![0.0; n * n];
            for i in 0..n {
                for j in 0..n {
                    let mut acc = 0.0;
                    for k in 0..n {
                        acc += cols_u[k][i] * s[k] * cols_v[k][j];
                    }
                    data[i * n + j] = acc;
                }
            }
            let a = Array2::from_shape_vec((n, n), data.clone()).unwrap();
            let t = std::time::Instant::now();
            let got = largest_singular_value(&a);
            worst = worst.max(report(
                &format!("tight-gap {gap} n {n}"),
                got,
                reference_sigma(&data, n, n),
                t.elapsed(),
            ));
        }

        // Complex random.
        for n in [64usize, 96] {
            let mut data = Vec::with_capacity(n * n);
            for _ in 0..(n * n) {
                data.push(C64::new(xorshift(&mut state), xorshift(&mut state)));
            }
            let a = Array2::from_shape_vec((n, n), data.clone()).unwrap();
            let t = std::time::Instant::now();
            let got = largest_singular_value_complex(&a);
            worst = worst.max(report(
                &format!("complex {n}"),
                got,
                reference_sigma_complex(&data, n, n),
                t.elapsed(),
            ));
        }

        eprintln!("worst relative error {worst:.3e}");
        assert!(
            worst < 1e-10,
            "worst rel {worst:.3e} (MATLAB-sized 2-norm agreement)"
        );
    }

    /// The sizes just past the Jacobi cutoff, a tight gap, Hilbert,
    /// rank deficiency, a short rectangle, and a complex matrix. σ_max
    /// must match the Hermitian eig of the Gram to 1e-10.
    #[test]
    fn hard_cases_match_svd_to_1e_10() {
        let mut worst = 0.0_f64;
        let mut state = 0xA11CE_u64;
        let n = 97usize;
        let mut data = Vec::with_capacity(n * n);
        for _ in 0..(n * n) {
            data.push(xorshift(&mut state));
        }
        let a = Array2::from_shape_vec((n, n), data.clone()).unwrap();
        let got = largest_singular_value(&a);
        worst = worst.max(rel_err(got, reference_sigma(&data, n, n)));

        // σ₁ = 1, σ₂ = 1−10⁻⁶, the rest 0.5. Past the Jacobi cutoff.
        let gap = 1e-6;
        let mut cols_u = vec![vec![0.0; n]; n];
        let mut cols_v = vec![vec![0.0; n]; n];
        for set in [&mut cols_u, &mut cols_v] {
            for col in set.iter_mut() {
                for x in col.iter_mut() {
                    *x = xorshift(&mut state);
                }
            }
            for i in 0..n {
                for j in 0..i {
                    let dot: f64 = set[j].iter().zip(set[i].iter()).map(|(p, q)| p * q).sum();
                    for k in 0..n {
                        set[i][k] -= dot * set[j][k];
                    }
                }
                let nrm = set[i].iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-30);
                for x in &mut set[i] {
                    *x /= nrm;
                }
            }
        }
        let mut s = vec![0.5; n];
        s[0] = 1.0;
        s[1] = 1.0 - gap;
        let mut clustered = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                let mut acc = 0.0;
                for k in 0..n {
                    acc += cols_u[k][i] * s[k] * cols_v[k][j];
                }
                clustered[i * n + j] = acc;
            }
        }
        let a = Array2::from_shape_vec((n, n), clustered.clone()).unwrap();
        worst = worst.max(rel_err(
            largest_singular_value(&a),
            reference_sigma(&clustered, n, n),
        ));

        let h = 32usize;
        let mut hilbert = vec![0.0; h * h];
        for i in 0..h {
            for j in 0..h {
                hilbert[i * h + j] = 1.0 / (i + j + 1) as f64;
            }
        }
        let a = Array2::from_shape_vec((h, h), hilbert.clone()).unwrap();
        worst = worst.max(rel_err(
            largest_singular_value(&a),
            reference_sigma(&hilbert, h, h),
        ));

        let (m, nc) = (48usize, 48usize);
        let mut rank = vec![0.0; m * nc];
        for i in 0..m {
            for j in 0..(nc - 12) {
                rank[i * nc + j] = xorshift(&mut state);
            }
        }
        let a = Array2::from_shape_vec((m, nc), rank.clone()).unwrap();
        worst = worst.max(rel_err(
            largest_singular_value(&a),
            reference_sigma(&rank, m, nc),
        ));

        for (rm, rn) in [(40usize, 12usize), (12, 40)] {
            let mut rect = Vec::with_capacity(rm * rn);
            for _ in 0..(rm * rn) {
                rect.push(xorshift(&mut state));
            }
            let a = Array2::from_shape_vec((rm, rn), rect.clone()).unwrap();
            worst = worst.max(rel_err(
                largest_singular_value(&a),
                reference_sigma(&rect, rm, rn),
            ));
        }

        let cn = 32usize;
        let mut cdata = Vec::with_capacity(cn * cn);
        for _ in 0..(cn * cn) {
            cdata.push(C64::new(xorshift(&mut state), xorshift(&mut state)));
        }
        let a = Array2::from_shape_vec((cn, cn), cdata.clone()).unwrap();
        worst = worst.max(rel_err(
            largest_singular_value_complex(&a),
            reference_sigma_complex(&cdata, cn, cn),
        ));

        assert!(worst < 1e-10, "2-norm drifted from the SVD by {worst:.3e}");
    }

    #[test]
    fn cancel_stops_during_lanczos() {
        let n = 256usize;
        let a = Array2::from_elem((n, n), 0.1);
        let mut hits = 0u32;
        let t = std::time::Instant::now();
        let err = largest_singular_value_cancellable(&a, &mut || {
            hits += 1;
            hits > 4
        })
        .unwrap_err();
        assert_eq!(err, NormCancelled);
        assert!(hits < 30, "polled {hits} times");
        assert!(t.elapsed().as_millis() < 2000, "{:?}", t.elapsed());
    }
}

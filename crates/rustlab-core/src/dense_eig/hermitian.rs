//! Hermitian (and real symmetric) eigenproblem.
//!
//! 1. Householder reflectors reduce `A` to Hermitian tridiagonal form,
//!    `Qᴴ A Q = T`.
//! 2. A diagonal unitary `D` rotates each subdiagonal entry of `T` onto the
//!    positive real axis, leaving a real symmetric tridiagonal matrix (the
//!    diagonal of a Hermitian matrix is already real).
//! 3. The implicit QL iteration with Wilkinson shifts (`tql2`) diagonalises
//!    it, accumulating the rotations in `Z`.
//!
//! Eigenvalues are real and returned in ascending order; the eigenvectors
//! `Q D Z` are orthonormal. Real input stays real throughout: every
//! reflector phase is `x / |x|`, which is exactly `±1` for real `x`.

use super::MAX_ITER_PER_EIGENVALUE;
use crate::{CMatrix, CoreError, C64};
use ndarray::Array2;

pub(super) fn eig(a: &CMatrix, want_vectors: bool) -> Result<(Vec<f64>, Option<CMatrix>), CoreError> {
    let n = a.nrows();
    let (mut d, mut e, q) = tridiagonalize(a, want_vectors);
    let mut z: Array2<f64> = Array2::eye(n);
    tql2(&mut d, &mut e, want_vectors.then_some(&mut z))?;

    // Ascending order (tql2 leaves them nearly sorted; a stable sort keeps
    // equal eigenvalues in place).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| d[i].total_cmp(&d[j]));
    let vals: Vec<f64> = order.iter().map(|&i| d[i]).collect();
    let vecs = q.map(|q| {
        // V = (Q D) Z, columns permuted into ascending order.
        Array2::from_shape_fn((n, n), |(i, k)| {
            let col = order[k];
            (0..n).map(|m| q[[i, m]] * z[[m, col]]).sum::<C64>()
        })
    });
    Ok((vals, vecs))
}

/// `(diagonal, subdiagonal, Q·D)` with `(Q D)ᴴ A (Q D)` real symmetric
/// tridiagonal. `e[k]` is entry `(k+1, k)`; `e[n-1] = 0`.
fn tridiagonalize(a: &CMatrix, want_q: bool) -> (Vec<f64>, Vec<f64>, Option<CMatrix>) {
    let n = a.nrows();
    let mut t = a.clone();
    let mut q: Option<CMatrix> = want_q.then(|| Array2::eye(n));
    let zero = C64::new(0.0, 0.0);
    for k in 0..n.saturating_sub(2) {
        // Reflector H = I - 2 v vᴴ (‖v‖ = 1) mapping t[k+1.., k] onto e1.
        let norm = (k + 1..n).map(|i| t[[i, k]].norm_sqr()).sum::<f64>().sqrt();
        if norm == 0.0 {
            continue;
        }
        let x0 = t[[k + 1, k]];
        let phase = if x0 == zero { C64::new(1.0, 0.0) } else { x0 / x0.norm() };
        let alpha = -phase * norm;
        let mut v = vec![zero; n];
        for i in k + 1..n {
            v[i] = t[[i, k]];
        }
        v[k + 1] -= alpha;
        let vnorm = (k + 1..n).map(|i| v[i].norm_sqr()).sum::<f64>().sqrt();
        if vnorm == 0.0 {
            continue;
        }
        for x in v.iter_mut() {
            *x /= vnorm;
        }
        // T ← H T H = T - 2 v wᴴ - 2 w vᴴ, with p = T v, γ = vᴴ p, w = p - γ v.
        let p: Vec<C64> = (0..n)
            .map(|i| (k + 1..n).map(|j| t[[i, j]] * v[j]).sum())
            .collect();
        let gamma: C64 = (k + 1..n).map(|i| v[i].conj() * p[i]).sum();
        let w: Vec<C64> = (0..n).map(|i| p[i] - gamma * v[i]).collect();
        for i in 0..n {
            for j in 0..n {
                t[[i, j]] -= 2.0 * (v[i] * w[j].conj() + w[i] * v[j].conj());
            }
        }
        // Exact zeros below the subdiagonal, and the subdiagonal itself.
        for i in k + 2..n {
            t[[i, k]] = zero;
            t[[k, i]] = zero;
        }
        t[[k + 1, k]] = alpha;
        t[[k, k + 1]] = alpha.conj();
        if let Some(q) = q.as_mut() {
            // Q ← Q H = Q - 2 (Q v) vᴴ.
            let qv: Vec<C64> = (0..n)
                .map(|i| (k + 1..n).map(|j| q[[i, j]] * v[j]).sum())
                .collect();
            for i in 0..n {
                for j in k + 1..n {
                    q[[i, j]] -= 2.0 * qv[i] * v[j].conj();
                }
            }
        }
    }
    // Rotate each subdiagonal entry onto the positive real axis: with
    // T' = Dᴴ T D, T'[k+1, k] = conj(d[k+1]) T[k+1, k] d[k].
    let mut dphase = vec![C64::new(1.0, 0.0); n];
    let mut sub = vec![0.0; n];
    for k in 0..n.saturating_sub(1) {
        let s = t[[k + 1, k]];
        sub[k] = s.norm();
        dphase[k + 1] = if s == zero { dphase[k] } else { dphase[k] * s / s.norm() };
    }
    if let Some(q) = q.as_mut() {
        for i in 0..n {
            for j in 0..n {
                q[[i, j]] *= dphase[j];
            }
        }
    }
    let diag = (0..n).map(|i| t[[i, i]].re).collect();
    (diag, sub, q)
}

/// Implicit QL with Wilkinson shifts on the symmetric tridiagonal matrix
/// `(d, e)`, `e[k]` coupling `k` and `k+1`. On return `d` holds the
/// eigenvalues and `z` (if given) has been multiplied by the rotations.
fn tql2(d: &mut [f64], e: &mut [f64], mut z: Option<&mut Array2<f64>>) -> Result<(), CoreError> {
    let n = d.len();
    let eps = f64::EPSILON;
    let mut f = 0.0;
    let mut tst1: f64 = 0.0;
    for l in 0..n {
        tst1 = tst1.max(d[l].abs() + e[l].abs());
        let mut m = l;
        while m < n - 1 && e[m].abs() > eps * tst1 {
            m += 1;
        }
        if m > l {
            let mut iter = 0;
            loop {
                iter += 1;
                if iter > MAX_ITER_PER_EIGENVALUE {
                    return Err(CoreError::InvalidParameter(
                        "eig: symmetric QL iteration did not converge".to_string(),
                    ));
                }
                let g = d[l];
                let mut p = (d[l + 1] - g) / (2.0 * e[l]);
                let mut r = p.hypot(1.0);
                if p < 0.0 {
                    r = -r;
                }
                d[l] = e[l] / (p + r);
                d[l + 1] = e[l] * (p + r);
                let dl1 = d[l + 1];
                let mut h = g - d[l];
                for di in d.iter_mut().skip(l + 2) {
                    *di -= h;
                }
                f += h;
                p = d[m];
                let (mut c, mut c2, mut c3) = (1.0, 1.0, 1.0);
                let el1 = e[l + 1];
                let (mut s, mut s2) = (0.0, 0.0);
                for i in (l..m).rev() {
                    c3 = c2;
                    c2 = c;
                    s2 = s;
                    let g = c * e[i];
                    h = c * p;
                    r = p.hypot(e[i]);
                    e[i + 1] = s * r;
                    s = e[i] / r;
                    c = p / r;
                    p = c * d[i] - s * g;
                    d[i + 1] = h + s * (c * g + s * d[i]);
                    if let Some(z) = z.as_deref_mut() {
                        for k in 0..n {
                            let h = z[[k, i + 1]];
                            z[[k, i + 1]] = s * z[[k, i]] + c * h;
                            z[[k, i]] = c * z[[k, i]] - s * h;
                        }
                    }
                }
                p = -s * s2 * c3 * el1 * e[l] / dl1;
                e[l] = s * p;
                d[l] = c * p;
                if e[l].abs() <= eps * tst1 {
                    break;
                }
            }
        }
        d[l] += f;
        e[l] = 0.0;
    }
    Ok(())
}

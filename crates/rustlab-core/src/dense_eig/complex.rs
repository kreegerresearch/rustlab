//! Complex non-Hermitian eigenproblem: Householder reduction to Hessenberg
//! form, the single-shift complex QR iteration to Schur form `A = Q T Qᴴ`
//! (`T` upper triangular), and back substitution on `T` for the
//! eigenvectors.
//!
//! The QR sweep runs on the active unreduced block only — rows and columns
//! `lo..=hi`, where `hi` is the lowest unconverged row and `lo` follows the
//! nearest negligible subdiagonal entry above it — while the rotations are
//! still applied to all of `T` and `Q`, so `T` ends as a full Schur form.

use super::{abs1, MAX_ITER_PER_EIGENVALUE};
use crate::{CMatrix, CoreError, C64};
use ndarray::Array2;

pub(super) fn eig(a: &CMatrix, want_vectors: bool) -> Result<(Vec<C64>, Option<CMatrix>), CoreError> {
    let n = a.nrows();
    let (mut t, mut q) = hessenberg(a);
    schur(&mut t, &mut q)?;
    let vals: Vec<C64> = (0..n).map(|i| t[[i, i]]).collect();
    if !want_vectors {
        return Ok((vals, None));
    }
    let y = triangular_eigenvectors(&t);
    Ok((vals, Some(q.dot(&y))))
}

/// `(H, Q)` with `A = Q H Qᴴ`, `H` upper Hessenberg.
fn hessenberg(a: &CMatrix) -> (CMatrix, CMatrix) {
    let n = a.nrows();
    let zero = C64::new(0.0, 0.0);
    let mut h = a.clone();
    let mut q: CMatrix = Array2::eye(n);
    for k in 0..n.saturating_sub(2) {
        let norm = (k + 1..n).map(|i| h[[i, k]].norm_sqr()).sum::<f64>().sqrt();
        if norm == 0.0 {
            continue;
        }
        let x0 = h[[k + 1, k]];
        let phase = if x0 == zero { C64::new(1.0, 0.0) } else { x0 / x0.norm() };
        let alpha = -phase * norm;
        let mut v = vec![zero; n];
        for i in k + 1..n {
            v[i] = h[[i, k]];
        }
        v[k + 1] -= alpha;
        let vnorm = (k + 1..n).map(|i| v[i].norm_sqr()).sum::<f64>().sqrt();
        if vnorm == 0.0 {
            continue;
        }
        for x in v.iter_mut() {
            *x /= vnorm;
        }
        // H ← (I - 2vvᴴ) H: rows k+1.., columns k..
        for j in k..n {
            let s: C64 = (k + 1..n).map(|i| v[i].conj() * h[[i, j]]).sum();
            for i in k + 1..n {
                h[[i, j]] -= 2.0 * v[i] * s;
            }
        }
        // H ← H (I - 2vvᴴ) and Q ← Q (I - 2vvᴴ): columns k+1..
        for m in [&mut h, &mut q] {
            for i in 0..n {
                let s: C64 = (k + 1..n).map(|j| m[[i, j]] * v[j]).sum();
                for j in k + 1..n {
                    m[[i, j]] -= 2.0 * s * v[j].conj();
                }
            }
        }
        h[[k + 1, k]] = alpha;
        for i in k + 2..n {
            h[[i, k]] = zero;
        }
    }
    (h, q)
}

/// Single-shift QR iteration on the Hessenberg matrix `t` until it is upper
/// triangular; rotations are accumulated into `q`.
fn schur(t: &mut CMatrix, q: &mut CMatrix) -> Result<(), CoreError> {
    let n = t.nrows();
    let eps = f64::EPSILON;
    let zero = C64::new(0.0, 0.0);
    let norm: f64 = t.iter().map(|z| abs1(*z)).sum();
    if n < 2 || norm == 0.0 {
        return Ok(());
    }
    let mut hi = n - 1;
    let mut iter = 0;
    while hi > 0 {
        // lo: start of the unreduced block that ends at hi.
        let mut lo = hi;
        while lo > 0 {
            let mut s = abs1(t[[lo - 1, lo - 1]]) + abs1(t[[lo, lo]]);
            if s == 0.0 {
                s = norm;
            }
            if abs1(t[[lo, lo - 1]]) <= eps * s {
                t[[lo, lo - 1]] = zero;
                break;
            }
            lo -= 1;
        }
        if lo == hi {
            // t[hi, hi] has converged.
            hi -= 1;
            iter = 0;
            continue;
        }
        iter += 1;
        if iter > MAX_ITER_PER_EIGENVALUE * 2 {
            return Err(CoreError::InvalidParameter(
                "eig: complex QR iteration did not converge".to_string(),
            ));
        }
        let shift = if iter % 10 == 0 {
            // Exceptional shift, to break a cycle the Wilkinson shift cannot
            // (e.g. the roots-of-unity companion matrix, whose shift is 0).
            t[[hi, hi]] + C64::new(0.75 * abs1(t[[hi, hi - 1]]), 0.0)
        } else {
            wilkinson(t[[hi - 1, hi - 1]], t[[hi - 1, hi]], t[[hi, hi - 1]], t[[hi, hi]])
        };
        // Implicit single-shift sweep on rows / columns lo..=hi.
        let mut x = t[[lo, lo]] - shift;
        let mut y = t[[lo + 1, lo]];
        for k in lo..hi {
            let r = x.norm().hypot(y.norm());
            if r == 0.0 {
                break;
            }
            let (c, s) = (x / r, y / r);
            // Left: rows k, k+1 ← G [rows], G = [c̄ s̄; -s c], all columns from k-1 on.
            for j in (if k > lo { k - 1 } else { k })..n {
                let (u, w) = (t[[k, j]], t[[k + 1, j]]);
                t[[k, j]] = c.conj() * u + s.conj() * w;
                t[[k + 1, j]] = -s * u + c * w;
            }
            if k > lo {
                // The bulge this rotation chased up is exactly zero.
                t[[k + 1, k - 1]] = zero;
            }
            // Right: columns k, k+1 ← [cols] Gᴴ, all rows down to the bulge.
            for i in 0..=(k + 2).min(hi) {
                let (u, w) = (t[[i, k]], t[[i, k + 1]]);
                t[[i, k]] = c * u + s * w;
                t[[i, k + 1]] = -s.conj() * u + c.conj() * w;
            }
            for i in 0..n {
                let (u, w) = (q[[i, k]], q[[i, k + 1]]);
                q[[i, k]] = c * u + s * w;
                q[[i, k + 1]] = -s.conj() * u + c.conj() * w;
            }
            if k + 1 < hi {
                x = t[[k + 1, k]];
                y = t[[k + 2, k]];
            }
        }
        if lo > 0 {
            t[[lo, lo - 1]] = zero;
        }
    }
    // Clear the rounding left below the diagonal.
    for i in 1..n {
        for j in 0..i {
            t[[i, j]] = zero;
        }
    }
    Ok(())
}

/// The eigenvalue of `[a b; c d]` closer to `d`, without forming `(a+d)²`.
fn wilkinson(a: C64, b: C64, c: C64, d: C64) -> C64 {
    let half = (a - d) / 2.0;
    let disc = (half * half + b * c).sqrt();
    // d + (half ∓ disc): pick the sign that keeps the root nearer d, written
    // as d - bc / (half ± disc) so there is no cancellation.
    let den = if (half + disc).norm() >= (half - disc).norm() { half + disc } else { half - disc };
    if den.norm() == 0.0 {
        d
    } else {
        d - b * c / den
    }
}

/// Eigenvectors of the upper triangular `t`, one per column: column `k`
/// solves `(T - t_kk I) y = 0` with `y_k = 1` by back substitution. A
/// denominator below `eps·‖T‖` (a repeated eigenvalue) is raised to that
/// size, as LAPACK's `ztrevc` does, so the solve stays finite.
fn triangular_eigenvectors(t: &CMatrix) -> CMatrix {
    let n = t.nrows();
    let eps = f64::EPSILON;
    let norm: f64 = t.iter().map(|z| abs1(*z)).fold(0.0, f64::max);
    let small = (eps * norm).max(f64::MIN_POSITIVE);
    let mut y: CMatrix = Array2::zeros((n, n));
    for k in 0..n {
        let lambda = t[[k, k]];
        y[[k, k]] = C64::new(1.0, 0.0);
        for i in (0..k).rev() {
            let s: C64 = (i + 1..=k).map(|j| t[[i, j]] * y[[j, k]]).sum();
            let mut den = t[[i, i]] - lambda;
            if abs1(den) < small {
                den = C64::new(small, 0.0);
            }
            y[[i, k]] = -s / den;
            // Overflow control: rescale the partial column.
            let big = abs1(y[[i, k]]);
            if big > 1e100 {
                for j in i..=k {
                    y[[j, k]] /= big;
                }
            }
        }
    }
    y
}

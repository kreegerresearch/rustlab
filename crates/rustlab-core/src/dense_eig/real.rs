//! Real non-symmetric eigenproblem: `orthes` (Householder reduction to upper
//! Hessenberg form, accumulating the transform) and `hqr2` (Francis
//! double-shift QR in real arithmetic, then back substitution on the real
//! Schur form for the eigenvectors). After Wilkinson & Reinsch's
//! *Handbook* procedures, in the form popularised by JAMA.
//!
//! A complex conjugate pair comes out of one 2×2 block of the Schur form,
//! so the pair is exactly conjugate and real eigenvalues are exactly real.

use super::MAX_ITER_PER_EIGENVALUE;
use crate::{CMatrix, CoreError, C64};
use ndarray::Array2;

pub(super) fn eig(a: &Array2<f64>, want_vectors: bool) -> Result<(Vec<C64>, Option<CMatrix>), CoreError> {
    let n = a.nrows();
    let mut h = a.clone();
    // Without eigenvectors the orthogonal transform is not needed: an empty
    // `v` turns every update of it into a no-op.
    let mut v: Array2<f64> = if want_vectors { Array2::eye(n) } else { Array2::zeros((0, 0)) };
    orthes(&mut h, &mut v);
    let mut d = vec![0.0; n];
    let mut e = vec![0.0; n];
    hqr2(&mut h, &mut v, &mut d, &mut e, want_vectors)?;
    let vals = (0..n).map(|i| C64::new(d[i], e[i])).collect();
    if !want_vectors {
        return Ok((vals, None));
    }
    // A pair λ = d + ie (e > 0) at columns j, j+1 has eigenvector
    // v_j + i v_{j+1}; its conjugate has v_j - i v_{j+1}.
    let mut out: CMatrix = Array2::zeros((n, n));
    let mut j = 0;
    while j < n {
        if e[j] == 0.0 {
            for i in 0..n {
                out[[i, j]] = C64::new(v[[i, j]], 0.0);
            }
            j += 1;
        } else {
            for i in 0..n {
                out[[i, j]] = C64::new(v[[i, j]], v[[i, j + 1]]);
                out[[i, j + 1]] = C64::new(v[[i, j]], -v[[i, j + 1]]);
            }
            j += 2;
        }
    }
    Ok((vals, Some(out)))
}

/// Householder reduction to upper Hessenberg form; `v` receives the
/// orthogonal transform (`a = v h vᵀ`).
fn orthes(h: &mut Array2<f64>, v: &mut Array2<f64>) {
    let n = h.nrows();
    if n < 3 {
        return;
    }
    let high = n - 1;
    let mut ort = vec![0.0; n];
    for m in 1..high {
        let scale: f64 = (m..=high).map(|i| h[[i, m - 1]].abs()).sum();
        if scale == 0.0 {
            continue;
        }
        let mut hh = 0.0;
        for i in (m..=high).rev() {
            ort[i] = h[[i, m - 1]] / scale;
            hh += ort[i] * ort[i];
        }
        let mut g = hh.sqrt();
        if ort[m] > 0.0 {
            g = -g;
        }
        hh -= ort[m] * g;
        ort[m] -= g;
        for j in m..n {
            let f = (m..=high).rev().map(|i| ort[i] * h[[i, j]]).sum::<f64>() / hh;
            for i in m..=high {
                h[[i, j]] -= f * ort[i];
            }
        }
        for i in 0..=high {
            let f = (m..=high).rev().map(|j| ort[j] * h[[i, j]]).sum::<f64>() / hh;
            for j in m..=high {
                h[[i, j]] -= f * ort[j];
            }
        }
        ort[m] *= scale;
        h[[m, m - 1]] = scale * g;
    }
    // Accumulate the transformations (Golub & Van Loan, Algorithm 5.1.1).
    if v.is_empty() {
        return;
    }
    for m in (1..high).rev() {
        if h[[m, m - 1]] != 0.0 {
            for i in m + 1..=high {
                ort[i] = h[[i, m - 1]];
            }
            for j in m..=high {
                let g = (m..=high).map(|i| ort[i] * v[[i, j]]).sum::<f64>();
                // Double division avoids possible underflow.
                let g = (g / ort[m]) / h[[m, m - 1]];
                for i in m..=high {
                    v[[i, j]] += g * ort[i];
                }
            }
        }
    }
}

/// Complex division (xr + i xi) / (yr + i yi), Smith's algorithm.
fn cdiv(xr: f64, xi: f64, yr: f64, yi: f64) -> (f64, f64) {
    if yr.abs() > yi.abs() {
        let r = yi / yr;
        let d = yr + r * yi;
        ((xr + r * xi) / d, (xi - r * xr) / d)
    } else {
        let r = yr / yi;
        let d = yi + r * yr;
        ((r * xr + xi) / d, (r * xi - xr) / d)
    }
}

// The work variables p, q, r, s, t, w, x, y, z carry values from one branch
// of the iteration into the next, as in the Handbook procedure; some initial
// values are never read, which the lint cannot see through.
#[allow(unused_assignments)]
/// Francis double-shift QR on the Hessenberg matrix `h`, accumulating into
/// `v`; eigenvalues to `(d, e)` (real, imaginary). With `want_vectors`,
/// back substitution leaves the eigenvectors in `v` (a complex pair as two
/// real columns).
fn hqr2(
    h: &mut Array2<f64>,
    v: &mut Array2<f64>,
    d: &mut [f64],
    e: &mut [f64],
    want_vectors: bool,
) -> Result<(), CoreError> {
    let nn = h.nrows();
    let low = 0usize;
    let high = nn - 1;
    let eps = f64::EPSILON;
    let mut exshift = 0.0;
    let (mut p, mut q, mut r, mut s, mut z) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut t, mut w, mut x, mut y) = (0.0, 0.0, 0.0, 0.0);

    let mut norm = 0.0;
    for i in 0..nn {
        for j in i.saturating_sub(1)..nn {
            norm += h[[i, j]].abs();
        }
    }

    // `n` is the bottom of the active block; it walks up as roots deflate.
    let mut n = nn as isize - 1;
    let mut iter = 0;
    while n >= low as isize {
        let nu = n as usize;
        // Look for a single small subdiagonal element.
        let mut l = nu;
        while l > low {
            s = h[[l - 1, l - 1]].abs() + h[[l, l]].abs();
            if s == 0.0 {
                s = norm;
            }
            if h[[l, l - 1]].abs() < eps * s {
                break;
            }
            l -= 1;
        }

        if l == nu {
            // One root found.
            h[[nu, nu]] += exshift;
            d[nu] = h[[nu, nu]];
            e[nu] = 0.0;
            n -= 1;
            iter = 0;
        } else if l == nu - 1 {
            // Two roots found.
            w = h[[nu, nu - 1]] * h[[nu - 1, nu]];
            p = (h[[nu - 1, nu - 1]] - h[[nu, nu]]) / 2.0;
            q = p * p + w;
            z = q.abs().sqrt();
            h[[nu, nu]] += exshift;
            h[[nu - 1, nu - 1]] += exshift;
            x = h[[nu, nu]];
            if q >= 0.0 {
                // Real pair.
                z = if p >= 0.0 { p + z } else { p - z };
                d[nu - 1] = x + z;
                d[nu] = d[nu - 1];
                if z != 0.0 {
                    d[nu] = x - w / z;
                }
                e[nu - 1] = 0.0;
                e[nu] = 0.0;
                x = h[[nu, nu - 1]];
                s = x.abs() + z.abs();
                p = x / s;
                q = z / s;
                r = (p * p + q * q).sqrt();
                p /= r;
                q /= r;
                for j in nu - 1..nn {
                    z = h[[nu - 1, j]];
                    h[[nu - 1, j]] = q * z + p * h[[nu, j]];
                    h[[nu, j]] = q * h[[nu, j]] - p * z;
                }
                for i in 0..=nu {
                    z = h[[i, nu - 1]];
                    h[[i, nu - 1]] = q * z + p * h[[i, nu]];
                    h[[i, nu]] = q * h[[i, nu]] - p * z;
                }
                for i in (low..=high).filter(|_| want_vectors) {
                    z = v[[i, nu - 1]];
                    v[[i, nu - 1]] = q * z + p * v[[i, nu]];
                    v[[i, nu]] = q * v[[i, nu]] - p * z;
                }
            } else {
                // Complex pair.
                d[nu - 1] = x + p;
                d[nu] = x + p;
                e[nu - 1] = z;
                e[nu] = -z;
            }
            n -= 2;
            iter = 0;
        } else {
            // No convergence yet: form the shift.
            x = h[[nu, nu]];
            y = 0.0;
            w = 0.0;
            if l < nu {
                y = h[[nu - 1, nu - 1]];
                w = h[[nu, nu - 1]] * h[[nu - 1, nu]];
            }
            // Wilkinson's original ad hoc shift.
            if iter == 10 {
                exshift += x;
                for i in low..=nu {
                    h[[i, i]] -= x;
                }
                s = h[[nu, nu - 1]].abs() + h[[nu - 1, nu - 2]].abs();
                x = 0.75 * s;
                y = x;
                w = -0.4375 * s * s;
            }
            // A second ad hoc shift for stubborn cases.
            if iter == 30 {
                s = (y - x) / 2.0;
                s = s * s + w;
                if s > 0.0 {
                    s = s.sqrt();
                    if y < x {
                        s = -s;
                    }
                    s = x - w / ((y - x) / 2.0 + s);
                    for i in low..=nu {
                        h[[i, i]] -= s;
                    }
                    exshift += s;
                    x = 0.964;
                    y = x;
                    w = x;
                }
            }
            iter += 1;
            if iter > MAX_ITER_PER_EIGENVALUE * 2 {
                return Err(CoreError::InvalidParameter(
                    "eig: QR iteration did not converge".to_string(),
                ));
            }

            // Look for two consecutive small subdiagonal elements.
            let mut m = nu - 2;
            loop {
                z = h[[m, m]];
                r = x - z;
                s = y - z;
                p = (r * s - w) / h[[m + 1, m]] + h[[m, m + 1]];
                q = h[[m + 1, m + 1]] - z - r - s;
                r = h[[m + 2, m + 1]];
                s = p.abs() + q.abs() + r.abs();
                p /= s;
                q /= s;
                r /= s;
                if m == l {
                    break;
                }
                if h[[m, m - 1]].abs() * (q.abs() + r.abs())
                    < eps * (p.abs() * (h[[m - 1, m - 1]].abs() + z.abs() + h[[m + 1, m + 1]].abs()))
                {
                    break;
                }
                m -= 1;
            }
            for i in m + 2..=nu {
                h[[i, i - 2]] = 0.0;
                if i > m + 2 {
                    h[[i, i - 3]] = 0.0;
                }
            }

            // Double QR step on rows l..=n and columns m..=n.
            for k in m..nu {
                let notlast = k != nu - 1;
                if k != m {
                    p = h[[k, k - 1]];
                    q = h[[k + 1, k - 1]];
                    r = if notlast { h[[k + 2, k - 1]] } else { 0.0 };
                    x = p.abs() + q.abs() + r.abs();
                    if x == 0.0 {
                        continue;
                    }
                    p /= x;
                    q /= x;
                    r /= x;
                }
                s = (p * p + q * q + r * r).sqrt();
                if p < 0.0 {
                    s = -s;
                }
                if s != 0.0 {
                    if k != m {
                        h[[k, k - 1]] = -s * x;
                    } else if l != m {
                        h[[k, k - 1]] = -h[[k, k - 1]];
                    }
                    p += s;
                    x = p / s;
                    y = q / s;
                    z = r / s;
                    q /= p;
                    r /= p;
                    for j in k..nn {
                        p = h[[k, j]] + q * h[[k + 1, j]];
                        if notlast {
                            p += r * h[[k + 2, j]];
                            h[[k + 2, j]] -= p * z;
                        }
                        h[[k, j]] -= p * x;
                        h[[k + 1, j]] -= p * y;
                    }
                    for i in 0..=nu.min(k + 3) {
                        p = x * h[[i, k]] + y * h[[i, k + 1]];
                        if notlast {
                            p += z * h[[i, k + 2]];
                            h[[i, k + 2]] -= p * r;
                        }
                        h[[i, k]] -= p;
                        h[[i, k + 1]] -= p * q;
                    }
                    for i in (low..=high).filter(|_| want_vectors) {
                        p = x * v[[i, k]] + y * v[[i, k + 1]];
                        if notlast {
                            p += z * v[[i, k + 2]];
                            v[[i, k + 2]] -= p * r;
                        }
                        v[[i, k]] -= p;
                        v[[i, k + 1]] -= p * q;
                    }
                }
            }
        }
    }

    if !want_vectors || norm == 0.0 {
        return Ok(());
    }

    // Back substitution: eigenvectors of the quasi-triangular Schur form.
    for nu in (0..nn).rev() {
        p = d[nu];
        q = e[nu];
        if q == 0.0 {
            // Real vector.
            let mut l = nu;
            h[[nu, nu]] = 1.0;
            for i in (0..nu).rev() {
                w = h[[i, i]] - p;
                r = (l..=nu).map(|j| h[[i, j]] * h[[j, nu]]).sum();
                if e[i] < 0.0 {
                    z = w;
                    s = r;
                } else {
                    l = i;
                    if e[i] == 0.0 {
                        h[[i, nu]] = if w != 0.0 { -r / w } else { -r / (eps * norm) };
                    } else {
                        // Solve the real 2×2 system.
                        x = h[[i, i + 1]];
                        y = h[[i + 1, i]];
                        q = (d[i] - p) * (d[i] - p) + e[i] * e[i];
                        t = (x * s - z * r) / q;
                        h[[i, nu]] = t;
                        h[[i + 1, nu]] = if x.abs() > z.abs() { (-r - w * t) / x } else { (-s - y * t) / z };
                    }
                    // Overflow control.
                    t = h[[i, nu]].abs();
                    if (eps * t) * t > 1.0 {
                        for j in i..=nu {
                            h[[j, nu]] /= t;
                        }
                    }
                }
            }
        } else if q < 0.0 {
            // Complex vector, stored in columns nu-1 (real) and nu (imaginary).
            let mut l = nu - 1;
            // The last vector component is imaginary, so the matrix is triangular.
            if h[[nu, nu - 1]].abs() > h[[nu - 1, nu]].abs() {
                h[[nu - 1, nu - 1]] = q / h[[nu, nu - 1]];
                h[[nu - 1, nu]] = -(h[[nu, nu]] - p) / h[[nu, nu - 1]];
            } else {
                let (cr, ci) = cdiv(0.0, -h[[nu - 1, nu]], h[[nu - 1, nu - 1]] - p, q);
                h[[nu - 1, nu - 1]] = cr;
                h[[nu - 1, nu]] = ci;
            }
            h[[nu, nu - 1]] = 0.0;
            h[[nu, nu]] = 1.0;
            for i in (0..nu.saturating_sub(1)).rev() {
                let ra: f64 = (l..=nu).map(|j| h[[i, j]] * h[[j, nu - 1]]).sum();
                let sa: f64 = (l..=nu).map(|j| h[[i, j]] * h[[j, nu]]).sum();
                w = h[[i, i]] - p;
                if e[i] < 0.0 {
                    z = w;
                    r = ra;
                    s = sa;
                } else {
                    l = i;
                    if e[i] == 0.0 {
                        let (cr, ci) = cdiv(-ra, -sa, w, q);
                        h[[i, nu - 1]] = cr;
                        h[[i, nu]] = ci;
                    } else {
                        // Solve the complex 2×2 system.
                        x = h[[i, i + 1]];
                        y = h[[i + 1, i]];
                        let mut vr = (d[i] - p) * (d[i] - p) + e[i] * e[i] - q * q;
                        let vi = (d[i] - p) * 2.0 * q;
                        if vr == 0.0 && vi == 0.0 {
                            vr = eps * norm * (w.abs() + q.abs() + x.abs() + y.abs() + z.abs());
                        }
                        let (cr, ci) = cdiv(x * r - z * ra + q * sa, x * s - z * sa - q * ra, vr, vi);
                        h[[i, nu - 1]] = cr;
                        h[[i, nu]] = ci;
                        if x.abs() > z.abs() + q.abs() {
                            h[[i + 1, nu - 1]] = (-ra - w * h[[i, nu - 1]] + q * h[[i, nu]]) / x;
                            h[[i + 1, nu]] = (-sa - w * h[[i, nu]] - q * h[[i, nu - 1]]) / x;
                        } else {
                            let (cr, ci) = cdiv(-r - y * h[[i, nu - 1]], -s - y * h[[i, nu]], z, q);
                            h[[i + 1, nu - 1]] = cr;
                            h[[i + 1, nu]] = ci;
                        }
                    }
                    // Overflow control.
                    t = h[[i, nu - 1]].abs().max(h[[i, nu]].abs());
                    if (eps * t) * t > 1.0 {
                        for j in i..=nu {
                            h[[j, nu - 1]] /= t;
                            h[[j, nu]] /= t;
                        }
                    }
                }
            }
        }
    }

    // Back-transform: eigenvectors of the original matrix are V · (Schur vectors).
    for j in (low..nn).rev() {
        for i in low..=high {
            z = (low..=j.min(high)).map(|k| v[[i, k]] * h[[k, j]]).sum();
            v[[i, j]] = z;
        }
    }
    Ok(())
}

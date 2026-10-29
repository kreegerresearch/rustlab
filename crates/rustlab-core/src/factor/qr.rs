//! Householder QR.
//!
//! Unpivoted `A = Q R` is what `qr` returns. Least squares pivots columns
//! so a rank-deficient matrix still has a basic solution: `A[:, piv] = Q R`,
//! and free variables are 0. The diagonal of the returned `R` is real and
//! non-negative.

use ndarray::Array2;

use crate::C64;

#[derive(Debug, Clone)]
pub struct Qr {
    fac: Array2<C64>,
    taus: Vec<C64>,
    /// `piv[j]` is the original column moved to position `j`.
    pub piv: Vec<usize>,
    reflectors: usize,
    pub scale: f64,
}

/// `pivoted` selects columns by remaining norm. The public `qr` builtin
/// passes `false`.
pub fn qr(a: &Array2<C64>, pivoted: bool) -> Qr {
    let (m, n) = a.dim();
    let mut fac = a.clone();
    let mut piv: Vec<usize> = (0..n).collect();
    let k_max = m.min(n);
    let mut taus = vec![C64::new(0.0, 0.0); k_max];
    let scale = a.iter().map(|z| z.norm()).fold(0.0_f64, f64::max);

    for k in 0..k_max {
        if pivoted {
            let mut best_c = k;
            let mut best = -1.0_f64;
            for c in k..n {
                let mut s = 0.0_f64;
                for r in k..m {
                    s += fac[[r, c]].norm_sqr();
                }
                if s > best {
                    best = s;
                    best_c = c;
                }
            }
            if best_c != k {
                for r in 0..m {
                    fac.swap((r, k), (r, best_c));
                }
                piv.swap(k, best_c);
            }
        }
        let mut col = vec![C64::new(0.0, 0.0); m - k];
        for r in k..m {
            col[r - k] = fac[[r, k]];
        }
        let (tau, beta) = larfg(&mut col);
        taus[k] = tau;
        fac[[k, k]] = beta;
        for r in (k + 1)..m {
            fac[[r, k]] = col[r - k];
        }
        // The stored diagonal is β. The reflector's leading entry is 1.
        let mut v: Vec<C64> = (k..m).map(|r| fac[[r, k]]).collect();
        if !v.is_empty() {
            v[0] = C64::new(1.0, 0.0);
        }
        apply_left(&v, tau, &mut fac, k + 1, n);
    }

    Qr {
        fac,
        taus,
        piv,
        reflectors: k_max,
        scale,
    }
}

/// Economy `R` only: `k × n` with `k = min(m, n)`, diagonal real and
/// non-negative. Does not form `Q`. Pivoted input: `A[:, piv] = Q R`
/// for the `Q` `explicit` would build.
pub fn r_factor(qrf: &Qr) -> Array2<C64> {
    let (_m, n) = qrf.fac.dim();
    let k = qrf.reflectors;
    let mut r = Array2::<C64>::zeros((k, n));
    for i in 0..k {
        for j in i..n {
            r[[i, j]] = qrf.fac[[i, j]];
        }
    }
    for col in 0..k.min(n) {
        let d = r[[col, col]];
        let mag = d.norm();
        if mag == 0.0 || !mag.is_finite() {
            continue;
        }
        let conj = (d / mag).conj();
        for j in col..n {
            r[[col, j]] *= conj;
        }
        r[[col, col]] = C64::new(mag, 0.0);
    }
    r
}

/// `(Q, R)`. `economy` gives `Q` as `m × k` and `R` as `k × n`
/// (`k = min(m, n)`); otherwise `Q` is `m × m` and `R` is `m × n`.
/// Unpivoted: `A = Q R`. Pivoted: `A[:, piv] = Q R`.
pub fn explicit(qrf: &Qr, economy: bool) -> (Array2<C64>, Array2<C64>) {
    let (m, n) = qrf.fac.dim();
    let k = qrf.reflectors;
    // Qᴴ = H_{k-1} … H_0, because those are the reflectors that
    // triangularize A. Apply them to I, then conjugate-transpose.
    let mut qh = Array2::<C64>::zeros((m, m));
    for i in 0..m {
        qh[[i, i]] = C64::new(1.0, 0.0);
    }
    for col in 0..k {
        if m > col {
            let mut reflector: Vec<C64> = (col..m).map(|r| qrf.fac[[r, col]]).collect();
            // The stored diagonal is β. The reflector's leading entry is 1.
            reflector[0] = C64::new(1.0, 0.0);
            apply_left(&reflector, qrf.taus[col], &mut qh, 0, m);
        }
    }
    let mut q = Array2::<C64>::zeros((m, m));
    for r in 0..m {
        for c in 0..m {
            q[[r, c]] = qh[[c, r]].conj();
        }
    }
    let r_rows = if economy { k } else { m };
    let mut r = Array2::<C64>::zeros((r_rows, n));
    for i in 0..k.min(r_rows) {
        for j in i..n {
            r[[i, j]] = qrf.fac[[i, j]];
        }
    }
    let q_ret = if economy {
        if k == 0 {
            Array2::<C64>::zeros((m, 0))
        } else {
            q.slice(ndarray::s![.., ..k]).to_owned()
        }
    } else {
        q
    };
    phase_pair(q_ret, r)
}

/// Make `R`'s diagonal real and non-negative. `Q`'s matching column takes
/// the phase so `Q R` is unchanged.
fn phase_pair(mut q: Array2<C64>, mut r: Array2<C64>) -> (Array2<C64>, Array2<C64>) {
    let k = q.ncols().min(r.nrows()).min(r.ncols());
    for col in 0..k {
        let d = r[[col, col]];
        let mag = d.norm();
        if mag == 0.0 || !mag.is_finite() {
            continue;
        }
        let phase = d / mag;
        for i in 0..q.nrows() {
            q[[i, col]] *= phase;
        }
        let conj = phase.conj();
        for j in col..r.ncols() {
            r[[col, j]] *= conj;
        }
        r[[col, col]] = C64::new(mag, 0.0);
    }
    (q, r)
}

/// Least-squares `A X ≈ B`. Returns `(X, rank, rcond)`.
///
/// `rcond` is `min |R_ii| / (max(m, n) · scale)` over the accepted pivots.
/// `rank < min(m, n)` means the factor is rank-deficient.
pub fn least_squares(qrf: &Qr, b: &Array2<C64>) -> (Array2<C64>, usize, f64) {
    let (m, n) = qrf.fac.dim();
    let p = b.ncols();
    let k = qrf.reflectors;
    let mut rhs = b.clone();
    for col in 0..k {
        let mut v: Vec<C64> = (col..m).map(|r| qrf.fac[[r, col]]).collect();
        if !v.is_empty() {
            v[0] = C64::new(1.0, 0.0);
        }
        apply_left(&v, qrf.taus[col], &mut rhs, 0, p);
    }
    let tol = f64::EPSILON * (m.max(n) as f64).max(1.0) * qrf.scale;
    let mut rank = 0usize;
    let mut min_diag = f64::INFINITY;
    for i in 0..k {
        let d = qrf.fac[[i, i]].norm();
        if d <= tol || !d.is_finite() {
            break;
        }
        min_diag = min_diag.min(d);
        rank += 1;
    }
    let mut y = Array2::<C64>::zeros((n, p));
    for col in 0..p {
        for i in (0..rank).rev() {
            let diag = qrf.fac[[i, i]];
            let mut s = rhs[[i, col]];
            for j in (i + 1)..rank {
                s -= qrf.fac[[i, j]] * y[[j, col]];
            }
            y[[i, col]] = s / diag;
        }
    }
    let mut x = Array2::<C64>::zeros((n, p));
    for i in 0..n {
        for col in 0..p {
            x[[qrf.piv[i], col]] = y[[i, col]];
        }
    }
    let rcond = if rank == 0 || qrf.scale == 0.0 || !min_diag.is_finite() {
        0.0
    } else {
        min_diag / (qrf.scale * (m.max(n) as f64).max(1.0))
    };
    (x, rank, rcond)
}

/// `||x||` Householder reflector. On return `x[0]` is the real diagonal
/// entry and `x[1..]` is `v[1..]` with `v[0] = 1`. `tau` is the coefficient
/// in `H = I − tau v vᴴ`.
fn larfg(x: &mut [C64]) -> (C64, C64) {
    if x.is_empty() {
        return (C64::new(0.0, 0.0), C64::new(0.0, 0.0));
    }
    let mut tail = 0.0_f64;
    for z in x.iter().skip(1) {
        tail += z.norm_sqr();
    }
    let alpha = x[0];
    if tail == 0.0 && alpha.im == 0.0 {
        return (C64::new(0.0, 0.0), alpha);
    }
    let beta_mag = alpha.norm().hypot(tail.sqrt());
    let sign_re = if alpha.re < 0.0 { -1.0 } else { 1.0 };
    let beta_re = -sign_re * beta_mag;
    let beta = C64::new(beta_re, 0.0);
    // v = (x − β e₁) / (α − β), so v₀ = 1. tau = (α − β) / (vᴴ x)
    // makes (I − tau v vᴴ) x = β e₁.
    let gamma = alpha - beta;
    let inv_gamma = C64::new(1.0, 0.0) / gamma;
    for z in x.iter_mut().skip(1) {
        *z *= inv_gamma;
    }
    let mut vnorm_sq = 1.0_f64;
    for z in x.iter().skip(1) {
        vnorm_sq += z.norm_sqr();
    }
    let vhx = beta + gamma * vnorm_sq;
    let tau = gamma / vhx;
    (tau, beta)
}

/// `H = I − tau v vᴴ` on columns `[col_from, col_to)` of `target`.
/// `v[0]` must already be 1.
fn apply_left(v: &[C64], tau: C64, target: &mut Array2<C64>, col_from: usize, col_to: usize) {
    if tau == C64::new(0.0, 0.0) || col_from >= col_to || v.is_empty() {
        return;
    }
    let row0 = target.nrows() - v.len();
    for j in col_from..col_to {
        let mut dot = C64::new(0.0, 0.0);
        for (i, vk) in v.iter().enumerate() {
            dot += vk.conj() * target[[row0 + i, j]];
        }
        let step = tau * dot;
        for (i, vk) in v.iter().enumerate() {
            target[[row0 + i, j]] -= step * *vk;
        }
    }
}

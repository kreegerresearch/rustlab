//! LU with partial pivoting: `P A = L U`.
//!
//! `L` is unit lower trapezoidal (`m × k`), `U` is upper trapezoidal
//! (`k × n`), `k = min(m, n)`. `perm[i]` is the original row now at
//! position `i`, so the permutation matrix has a one at `(i, perm[i])`.

use ndarray::Array2;

use crate::C64;

#[derive(Debug, Clone)]
pub struct Lu {
    pub l: Array2<C64>,
    pub u: Array2<C64>,
    /// Original row index now sitting at each factored row.
    pub perm: Vec<usize>,
    /// A pivot was numerically zero.
    pub singular: bool,
    /// `min |U_ii| / (n · scale)`. `Inf` when the matrix is empty.
    pub rcond: f64,
}

/// Factor `A`. Rectangular inputs are allowed: elimination stops at
/// `min(m, n)`.
pub fn lu(a: &Array2<C64>) -> Lu {
    let (m, n) = a.dim();
    let k = m.min(n);
    let mut u_full = a.clone();
    let mut perm: Vec<usize> = (0..m).collect();
    let mut multipliers = vec![C64::new(0.0, 0.0); m * k];
    let scale = a.iter().map(|z| z.norm()).fold(0.0_f64, f64::max);
    let tol = f64::EPSILON * (m.max(n) as f64).max(1.0) * scale;
    let mut min_pivot = f64::INFINITY;
    let mut singular = false;

    for col in 0..k {
        let mut piv = col;
        let mut best = u_full[[col, col]].norm();
        for i in (col + 1)..m {
            let v = u_full[[i, col]].norm();
            if v > best {
                best = v;
                piv = i;
            }
        }
        if piv != col {
            for j in 0..n {
                u_full.swap((col, j), (piv, j));
            }
            for j in 0..col {
                multipliers.swap(col * k + j, piv * k + j);
            }
            perm.swap(col, piv);
        }
        let pivot = u_full[[col, col]];
        let pnorm = pivot.norm();
        min_pivot = min_pivot.min(pnorm);
        if !pnorm.is_finite() || pnorm <= tol {
            singular = true;
            continue;
        }
        for i in (col + 1)..m {
            let lik = u_full[[i, col]] / pivot;
            multipliers[i * k + col] = lik;
            for j in (col + 1)..n {
                let prod = lik * u_full[[col, j]];
                u_full[[i, j]] -= prod;
            }
            u_full[[i, col]] = C64::new(0.0, 0.0);
        }
    }

    let mut l = Array2::<C64>::zeros((m, k));
    for i in 0..m {
        for j in 0..k.min(i) {
            l[[i, j]] = multipliers[i * k + j];
        }
        if i < k {
            l[[i, i]] = C64::new(1.0, 0.0);
        }
    }
    let mut u = Array2::<C64>::zeros((k, n));
    for i in 0..k {
        for j in i..n {
            u[[i, j]] = u_full[[i, j]];
        }
    }
    let rcond = if k == 0 {
        f64::INFINITY
    } else if scale == 0.0 || !min_pivot.is_finite() {
        0.0
    } else {
        min_pivot / (scale * (m.max(n) as f64).max(1.0))
    };
    Lu {
        l,
        u,
        perm,
        singular,
        rcond,
    }
}

/// Solve `A X = B` with a factorization of `A`. `B` has `m` rows.
/// A singular factor produces a basic solution (zero for a zero pivot)
/// rather than `Inf`.
pub fn solve(f: &Lu, b: &Array2<C64>) -> Array2<C64> {
    let (m, k) = f.l.dim();
    let n = f.u.ncols();
    let p = b.ncols();
    let mut y = Array2::<C64>::zeros((m, p));
    for col in 0..p {
        for i in 0..m {
            y[[i, col]] = b[[f.perm[i], col]];
        }
        for j in 0..k {
            let yj = y[[j, col]];
            for i in (j + 1)..m {
                let prod = f.l[[i, j]] * yj;
                y[[i, col]] -= prod;
            }
        }
    }
    let mut x = Array2::<C64>::zeros((n, p));
    for col in 0..p {
        for i in (0..k).rev() {
            let diag = f.u[[i, i]];
            if diag.norm() == 0.0 || !diag.re.is_finite() || !diag.im.is_finite() {
                x[[i, col]] = C64::new(0.0, 0.0);
                continue;
            }
            let mut s = y[[i, col]];
            for j in (i + 1)..n {
                s -= f.u[[i, j]] * x[[j, col]];
            }
            x[[i, col]] = s / diag;
        }
    }
    x
}

/// Permutation matrix with a one at `(i, perm[i])`.
pub fn permutation_matrix(perm: &[usize]) -> Array2<C64> {
    let m = perm.len();
    let mut p = Array2::<C64>::zeros((m, m));
    for (i, &src) in perm.iter().enumerate() {
        p[[i, src]] = C64::new(1.0, 0.0);
    }
    p
}

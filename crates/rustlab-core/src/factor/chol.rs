//! Dense Cholesky for a real symmetric or complex Hermitian matrix.
//!
//! The factor reads one triangle. `upper` produces `R` with `A = R' R`
//! from the upper triangle. `lower` produces `L` with `A = L L'` from the
//! lower triangle. A non-positive pivot stops the factorization: `p` is the
//! 1-based index of that pivot and the returned factor is the leading
//! `(p-1) × (p-1)` block.

use ndarray::Array2;

use crate::C64;

#[derive(Debug, Clone)]
pub struct Chol {
    pub factor: Array2<C64>,
    /// `0` when the whole matrix factored. Otherwise the 1-based pivot
    /// that was not positive.
    pub p: usize,
}

/// Upper Cholesky. `p == 0` means `factor' * factor` reconstructs the
/// upper triangle of `a` (and the whole matrix when `a` is Hermitian).
pub fn upper(a: &Array2<C64>) -> Result<Chol, String> {
    factor(a, true)
}

/// Lower Cholesky. `p == 0` means `factor * factor'` reconstructs the
/// lower triangle of `a`.
pub fn lower(a: &Array2<C64>) -> Result<Chol, String> {
    factor(a, false)
}

fn factor(a: &Array2<C64>, upper: bool) -> Result<Chol, String> {
    let (m, n) = a.dim();
    if m != n {
        return Err(format!("chol: matrix must be square, got {m}×{n}"));
    }
    if n == 0 {
        return Ok(Chol { factor: Array2::zeros((0, 0)), p: 0 });
    }
    let mut f = Array2::<C64>::zeros((n, n));
    for j in 0..n {
        let mut diag = a[[j, j]].re;
        for k in 0..j {
            let z = if upper { f[[k, j]] } else { f[[j, k]] };
            diag -= z.norm_sqr();
        }
        if !(diag > 0.0) || !diag.is_finite() {
            let q = j;
            let mut small = Array2::<C64>::zeros((q, q));
            for r in 0..q {
                for c in 0..q {
                    small[[r, c]] = if upper { f[[r, c]] } else { f[[r, c]] };
                }
            }
            return Ok(Chol { factor: small, p: j + 1 });
        }
        let root = C64::new(diag.sqrt(), 0.0);
        if upper {
            f[[j, j]] = root;
            for i in (j + 1)..n {
                let mut sum = a[[j, i]];
                for k in 0..j {
                    sum -= f[[k, j]].conj() * f[[k, i]];
                }
                f[[j, i]] = sum / root;
            }
        } else {
            f[[j, j]] = root;
            for i in (j + 1)..n {
                let mut sum = a[[i, j]];
                for k in 0..j {
                    sum -= f[[i, k]] * f[[j, k]].conj();
                }
                f[[i, j]] = sum / root;
            }
        }
    }
    Ok(Chol { factor: f, p: 0 })
}

/// Solve `R' R X = B` with the upper factor from a successful `upper`.
pub fn solve_upper(r: &Array2<C64>, b: &Array2<C64>) -> Array2<C64> {
    let n = r.nrows();
    let p = b.ncols();
    let mut x = Array2::<C64>::zeros((n, p));
    for col in 0..p {
        let mut y = vec![C64::new(0.0, 0.0); n];
        for i in 0..n {
            let mut s = b[[i, col]];
            for j in 0..i {
                s -= r[[j, i]].conj() * y[j];
            }
            y[i] = s / r[[i, i]];
        }
        for i in (0..n).rev() {
            let mut s = y[i];
            for j in (i + 1)..n {
                s -= r[[i, j]] * x[[j, col]];
            }
            x[[i, col]] = s / r[[i, i]];
        }
    }
    x
}

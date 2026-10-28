//! Balancing: a diagonal similarity `B = D⁻¹ A D` that makes each row and
//! its column have similar norms (`balanc`, Parlett & Reinsch). The entries
//! of `D` are powers of two, so `B` carries no rounding error, and its
//! eigenvalues are those of `A`. Badly scaled matrices — entries spanning
//! many orders of magnitude — get much more accurate eigenvalues.

use crate::CMatrix;

/// `(D⁻¹ A D, diag(D))`.
pub(super) fn balance(a: &CMatrix) -> (CMatrix, Vec<f64>) {
    const RADIX: f64 = 2.0;
    let n = a.nrows();
    let mut b = a.clone();
    let mut d = vec![1.0; n];
    loop {
        let mut converged = true;
        for i in 0..n {
            let mut c = 0.0;
            let mut r = 0.0;
            for j in 0..n {
                if j != i {
                    c += b[[j, i]].norm();
                    r += b[[i, j]].norm();
                }
            }
            if c == 0.0 || r == 0.0 {
                continue;
            }
            let s = c + r;
            let mut f = 1.0;
            let mut g = r / RADIX;
            while c < g {
                f *= RADIX;
                c *= RADIX * RADIX;
            }
            g = r * RADIX;
            while c >= g {
                f /= RADIX;
                c /= RADIX * RADIX;
            }
            if (c + r) / f < 0.95 * s {
                converged = false;
                d[i] *= f;
                for j in 0..n {
                    b[[i, j]] /= f;
                    b[[j, i]] *= f;
                }
            }
        }
        if converged {
            return (b, d);
        }
    }
}

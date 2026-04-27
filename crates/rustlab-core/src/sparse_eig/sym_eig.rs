//! Dense symmetric eigensolver via cyclic Jacobi rotations.
//!
//! This is the small-matrix workhorse at the centre of Lanczos: after
//! the iteration produces a symmetric tridiagonal `T_m` of size `m × m`
//! (typically m ≤ 50), we extract the Ritz pairs by feeding `T_m` —
//! densified into a symmetric `m × m` matrix — through Jacobi
//! diagonalization.
//!
//! Jacobi rotations are O(m^3) per sweep and converge in roughly
//! O(log m) sweeps for well-conditioned matrices. Asymptotically slower
//! than Wilkinson-shifted symmetric QR (O(m^2) per pass) but with a
//! tiny constant factor and bulletproof correctness — exactly what we
//! want for a small dense subproblem.

use ndarray::Array2;

/// Diagonalize a symmetric matrix `A` via cyclic Jacobi rotations.
/// Returns `(eigenvalues, eigenvectors)`. The eigenvalues are in
/// arbitrary order; the eigenvector at column `k` corresponds to
/// eigenvalue `k`.
///
/// Assumes `A` is exactly symmetric (the algorithm is undefined
/// otherwise). For matrices arising from Lanczos's tridiagonal `T_m`
/// or from squaring an Arnoldi Hessenberg this is automatic.
pub fn sym_eig_jacobi(a: &Array2<f64>) -> (Vec<f64>, Array2<f64>) {
    let n = a.nrows();
    debug_assert_eq!(n, a.ncols(), "sym_eig requires a square matrix");

    let mut t = a.to_owned();
    let mut q = Array2::<f64>::eye(n);

    if n <= 1 {
        return ((0..n).map(|i| t[[i, i]]).collect(), q);
    }

    let max_sweeps = 50;
    let tol_factor: f64 = 1e-14;

    for _sweep in 0..max_sweeps {
        // Off-diagonal Frobenius norm — converged when this is small.
        let mut off_norm_sq = 0.0;
        for i in 0..n {
            for j in (i + 1)..n {
                off_norm_sq += 2.0 * t[[i, j]].powi(2);
            }
        }
        let total_sq: f64 = (0..n).map(|i| t[[i, i]].powi(2)).sum::<f64>() + off_norm_sq;
        if off_norm_sq <= tol_factor.powi(2) * total_sq {
            break;
        }

        // Cyclic sweep: rotate every (p, q) with p < q.
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

                // Apply rotation: T = G^T T G, Q = Q G.
                let app = t[[p, p]];
                let arr = t[[r, r]];
                t[[p, p]] = app - tau * apq;
                t[[r, r]] = arr + tau * apq;
                t[[p, r]] = 0.0;
                t[[r, p]] = 0.0;

                for k in 0..n {
                    if k != p && k != r {
                        let tkp = t[[k, p]];
                        let tkr = t[[k, r]];
                        t[[k, p]] = c * tkp - s * tkr;
                        t[[k, r]] = s * tkp + c * tkr;
                        t[[p, k]] = t[[k, p]];
                        t[[r, k]] = t[[k, r]];
                    }
                    let qkp = q[[k, p]];
                    let qkr = q[[k, r]];
                    q[[k, p]] = c * qkp - s * qkr;
                    q[[k, r]] = s * qkp + c * qkr;
                }
            }
        }
    }

    let eigenvalues: Vec<f64> = (0..n).map(|i| t[[i, i]]).collect();
    (eigenvalues, q)
}

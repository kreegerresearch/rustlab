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

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + a.abs() + b.abs())
    }

    /// Sort an `(eigenvalue, column)` pair list ascending by eigenvalue.
    /// Returns the sorted eigenvalues and the column-permuted eigenvector
    /// matrix. Useful since Jacobi returns eigenpairs in arbitrary order.
    fn sort_by_eigenvalue(vals: Vec<f64>, vecs: Array2<f64>) -> (Vec<f64>, Array2<f64>) {
        let n = vals.len();
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by(|&i, &j| vals[i].partial_cmp(&vals[j]).unwrap());
        let sorted_vals: Vec<f64> = idx.iter().map(|&i| vals[i]).collect();
        let mut sorted_vecs = Array2::<f64>::zeros((n, n));
        for (new_col, &old_col) in idx.iter().enumerate() {
            for r in 0..n {
                sorted_vecs[[r, new_col]] = vecs[[r, old_col]];
            }
        }
        (sorted_vals, sorted_vecs)
    }

    /// Verify each (lambda, v) pair satisfies A v = lambda v.
    fn assert_eigenpairs_valid(a: &Array2<f64>, vals: &[f64], vecs: &Array2<f64>, tol: f64) {
        let n = a.nrows();
        for k in 0..n {
            let v = vecs.column(k);
            let av: Vec<f64> = (0..n)
                .map(|i| (0..n).map(|j| a[[i, j]] * v[j]).sum::<f64>())
                .collect();
            for i in 0..n {
                let expected = vals[k] * v[i];
                assert!(
                    (av[i] - expected).abs() <= tol * (1.0 + av[i].abs() + expected.abs()),
                    "Av != λv at row {i} for k={k}: got {} expected {}",
                    av[i],
                    expected
                );
            }
        }
    }

    /// Verify V is orthonormal: V^T V = I within tolerance.
    fn assert_orthonormal(v: &Array2<f64>, tol: f64) {
        let n = v.ncols();
        for i in 0..n {
            for j in 0..n {
                let dot: f64 = (0..v.nrows()).map(|r| v[[r, i]] * v[[r, j]]).sum();
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (dot - expected).abs() < tol,
                    "V^T V[{i},{j}] = {dot}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn diag_2x2_returns_diagonal_entries() {
        let a = ndarray::arr2(&[[3.0, 0.0], [0.0, 7.0]]);
        let (vals, vecs) = sym_eig_jacobi(&a);
        let (sorted, _) = sort_by_eigenvalue(vals, vecs);
        assert!(close(sorted[0], 3.0, 1e-12));
        assert!(close(sorted[1], 7.0, 1e-12));
    }

    #[test]
    fn identity_3x3_eigenvalues_all_one() {
        let a = Array2::<f64>::eye(3);
        let (vals, vecs) = sym_eig_jacobi(&a);
        for v in &vals {
            assert!(close(*v, 1.0, 1e-12), "expected 1.0, got {v}");
        }
        assert_orthonormal(&vecs, 1e-12);
    }

    #[test]
    fn symmetric_tridiagonal_3x3_known_eigenvalues() {
        // Symmetric tridiagonal:
        //   [4 1 0]
        //   [1 4 1]
        //   [0 1 4]
        // Eigenvalues: 4 - sqrt(2), 4, 4 + sqrt(2).
        let a = ndarray::arr2(&[
            [4.0, 1.0, 0.0],
            [1.0, 4.0, 1.0],
            [0.0, 1.0, 4.0],
        ]);
        let (vals, vecs) = sym_eig_jacobi(&a);
        let (sorted, sorted_vecs) = sort_by_eigenvalue(vals, vecs);
        let s2 = 2f64.sqrt();
        assert!(close(sorted[0], 4.0 - s2, 1e-12));
        assert!(close(sorted[1], 4.0, 1e-12));
        assert!(close(sorted[2], 4.0 + s2, 1e-12));
        assert_eigenpairs_valid(&a, &sorted, &sorted_vecs, 1e-12);
        assert_orthonormal(&sorted_vecs, 1e-12);
    }

    #[test]
    fn dense_5x5_random_symmetric_eigenpairs_consistent() {
        // Build A = M + M^T where M is a fixed deterministic matrix.
        // The result is automatically symmetric.
        let mut m = Array2::<f64>::zeros((5, 5));
        let raw: [[f64; 5]; 5] = [
            [0.7, -1.2, 0.4, 1.1, -0.3],
            [0.5, 2.1, -0.8, 0.2, 1.4],
            [-1.0, 0.3, -0.6, 1.7, 0.9],
            [0.6, -0.4, 1.3, 2.5, -1.1],
            [-0.7, 1.0, 0.2, -0.5, 0.8],
        ];
        for i in 0..5 {
            for j in 0..5 {
                m[[i, j]] = raw[i][j];
            }
        }
        let mut a = Array2::<f64>::zeros((5, 5));
        for i in 0..5 {
            for j in 0..5 {
                a[[i, j]] = m[[i, j]] + m[[j, i]];
            }
        }
        let (vals, vecs) = sym_eig_jacobi(&a);
        assert_eigenpairs_valid(&a, &vals, &vecs, 1e-10);
        assert_orthonormal(&vecs, 1e-10);
    }

    #[test]
    fn nearly_defective_does_not_crash() {
        // Two equal eigenvalues — common Jacobi corner case.
        // diag(2, 2, 5) is symmetric with a doubled eigenvalue at 2.
        let a = ndarray::arr2(&[
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 5.0],
        ]);
        let (vals, vecs) = sym_eig_jacobi(&a);
        let (sorted, _) = sort_by_eigenvalue(vals, vecs);
        assert!(close(sorted[0], 2.0, 1e-12));
        assert!(close(sorted[1], 2.0, 1e-12));
        assert!(close(sorted[2], 5.0, 1e-12));
    }

    #[test]
    fn eigenvalues_invariant_under_orthogonal_similarity() {
        // For symmetric A, the eigenvalues of Q^T A Q (Q orthogonal) are
        // identical to those of A. We don't have a Q construction handy,
        // so use a synthetic case: rotate diag(1, 4, 9) by a 45° rotation
        // in the (0, 1) plane.
        let theta = std::f64::consts::FRAC_PI_4;
        let (c, s) = (theta.cos(), theta.sin());
        let q = ndarray::arr2(&[
            [c, -s, 0.0],
            [s, c, 0.0],
            [0.0, 0.0, 1.0],
        ]);
        let d = ndarray::arr2(&[
            [1.0, 0.0, 0.0],
            [0.0, 4.0, 0.0],
            [0.0, 0.0, 9.0],
        ]);
        // A = Q D Q^T
        let mut qd = Array2::<f64>::zeros((3, 3));
        for i in 0..3 {
            for j in 0..3 {
                qd[[i, j]] = (0..3).map(|k| q[[i, k]] * d[[k, j]]).sum::<f64>();
            }
        }
        let mut a = Array2::<f64>::zeros((3, 3));
        for i in 0..3 {
            for j in 0..3 {
                a[[i, j]] = (0..3).map(|k| qd[[i, k]] * q[[j, k]]).sum::<f64>();
            }
        }
        let (vals, _) = sym_eig_jacobi(&a);
        let mut sorted = vals;
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(close(sorted[0], 1.0, 1e-10));
        assert!(close(sorted[1], 4.0, 1e-10));
        assert!(close(sorted[2], 9.0, 1e-10));
    }
}

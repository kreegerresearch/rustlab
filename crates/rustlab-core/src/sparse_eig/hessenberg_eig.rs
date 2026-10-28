//! Eigenvalues + eigenvectors of a small dense complex Hessenberg matrix.
//!
//! Used by the Arnoldi path: after `m` Arnoldi iterations we have a
//! complex upper-Hessenberg `H` of size `m × m`. Its eigenpairs are the
//! Ritz pairs of the underlying sparse `A`.
//!
//! The work is done by the dense solver, [`crate::dense_eig::eig`]: its
//! QR iteration deflates on the active block (an Arnoldi `H` often splits
//! when the Krylov space captures an invariant subspace) and it takes the
//! eigenvectors from the Schur form, so repeated Ritz values get
//! independent vectors.

use crate::sparse_eig::SparseEigError;
use crate::types::C64;
use ndarray::Array2;
#[cfg(test)]
use num_complex::Complex;

/// Eigenvalues + eigenvectors of a complex upper-Hessenberg matrix.
/// Returns (`eigenvalues`, `eigenvectors`) where eigenvectors are
/// columns of an `m × m` dense matrix in the same order.
pub fn hessenberg_eig(h: &Array2<C64>) -> Result<(Vec<C64>, Array2<C64>), SparseEigError> {
    if h.nrows() != h.ncols() {
        return Err(SparseEigError::Internal(format!(
            "hessenberg_eig: expected square, got {}x{}",
            h.nrows(),
            h.ncols()
        )));
    }
    let (vals, vecs) =
        crate::dense_eig::eig(h, true).map_err(|e| SparseEigError::Internal(e.to_string()))?;
    Ok((vals, vecs.expect("eig returns vectors when asked")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(re: f64, im: f64) -> C64 {
        Complex::new(re, im)
    }

    fn close_c(a: C64, b: C64, tol: f64) -> bool {
        (a - b).norm() <= tol * (1.0 + a.norm() + b.norm())
    }

    /// Verify each eigenpair satisfies `H v = λ v` within `tol`.
    fn assert_eigenpairs_valid(h: &Array2<C64>, vals: &[C64], vecs: &Array2<C64>, tol: f64) {
        let n = h.nrows();
        for k in 0..n {
            let v = vecs.column(k);
            let hv: Vec<C64> = (0..n)
                .map(|i| (0..n).map(|j| h[[i, j]] * v[j]).sum::<C64>())
                .collect();
            for i in 0..n {
                let expected = vals[k] * v[i];
                let diff = (hv[i] - expected).norm();
                assert!(
                    diff <= tol * (1.0 + hv[i].norm() + expected.norm()),
                    "Hv != λv at row {i} for eigenpair {k}: got {} expected {} (diff {diff})",
                    hv[i],
                    expected
                );
            }
        }
    }

    fn find_eigenvalue(vals: &[C64], target: C64, tol: f64) -> bool {
        vals.iter().any(|&v| close_c(v, target, tol))
    }

    #[test]
    fn diag_2x2_returns_diagonal_entries() {
        // Diagonal IS Hessenberg. Eigenvalues = diagonal entries.
        let h = ndarray::arr2(&[[c(3.0, 1.0), c(0.0, 0.0)], [c(0.0, 0.0), c(7.0, -2.0)]]);
        let (vals, vecs) = hessenberg_eig(&h).unwrap();
        assert_eq!(vals.len(), 2);
        assert!(find_eigenvalue(&vals, c(3.0, 1.0), 1e-10));
        assert!(find_eigenvalue(&vals, c(7.0, -2.0), 1e-10));
        assert_eigenpairs_valid(&h, &vals, &vecs, 1e-10);
    }

    #[test]
    fn upper_triangular_3x3_eigenvalues_are_diagonal() {
        // Upper triangular is also Hessenberg. Eigenvalues = diagonal.
        let h = ndarray::arr2(&[
            [c(2.0, 0.0), c(1.0, 0.5), c(-0.3, 0.2)],
            [c(0.0, 0.0), c(5.0, 0.0), c(0.7, -0.1)],
            [c(0.0, 0.0), c(0.0, 0.0), c(11.0, 0.0)],
        ]);
        let (vals, vecs) = hessenberg_eig(&h).unwrap();
        assert!(find_eigenvalue(&vals, c(2.0, 0.0), 1e-10));
        assert!(find_eigenvalue(&vals, c(5.0, 0.0), 1e-10));
        assert!(find_eigenvalue(&vals, c(11.0, 0.0), 1e-10));
        assert_eigenpairs_valid(&h, &vals, &vecs, 1e-9);
    }

    #[test]
    fn hessenberg_2x2_known_eigenvalues() {
        // Real Hessenberg [[2, 1], [3, 4]]: eigenvalues are
        // (2+4 ± sqrt((2-4)² + 4*1*3))/2 = (6 ± sqrt(16))/2 = 5 and 1.
        let h = ndarray::arr2(&[[c(2.0, 0.0), c(1.0, 0.0)], [c(3.0, 0.0), c(4.0, 0.0)]]);
        let (vals, vecs) = hessenberg_eig(&h).unwrap();
        assert_eq!(vals.len(), 2);
        assert!(find_eigenvalue(&vals, c(5.0, 0.0), 1e-10));
        assert!(find_eigenvalue(&vals, c(1.0, 0.0), 1e-10));
        assert_eigenpairs_valid(&h, &vals, &vecs, 1e-9);
    }

    #[test]
    fn complex_hessenberg_2x2_complex_eigenvalues() {
        // Real coefficients with complex-conjugate eigenvalues:
        //   [[0, 1], [-1, 0]] has eigenvalues +i and -i.
        let h = ndarray::arr2(&[[c(0.0, 0.0), c(1.0, 0.0)], [c(-1.0, 0.0), c(0.0, 0.0)]]);
        let (vals, vecs) = hessenberg_eig(&h).unwrap();
        assert_eq!(vals.len(), 2);
        // Eigenvalues are ±i (in either order).
        assert!(
            (find_eigenvalue(&vals, c(0.0, 1.0), 1e-10)
                || find_eigenvalue(&vals, c(0.0, -1.0), 1e-10)),
            "expected ±i, got {:?}",
            vals
        );
        assert_eigenpairs_valid(&h, &vals, &vecs, 1e-9);
    }

    #[test]
    fn hessenberg_4x4_consistent_eigenpairs() {
        // 4×4 real upper-Hessenberg with deterministic entries.
        // Verify: the returned eigenvalues + eigenvectors satisfy Hv = λv,
        // and that the trace matches the sum of eigenvalues.
        let h = ndarray::arr2(&[
            [c(4.0, 0.0), c(1.0, 0.0), c(2.0, 0.0), c(3.0, 0.0)],
            [c(1.0, 0.0), c(3.0, 0.0), c(0.5, 0.0), c(2.0, 0.0)],
            [c(0.0, 0.0), c(0.5, 0.0), c(2.0, 0.0), c(1.0, 0.0)],
            [c(0.0, 0.0), c(0.0, 0.0), c(0.5, 0.0), c(5.0, 0.0)],
        ]);
        let (vals, vecs) = hessenberg_eig(&h).unwrap();
        assert_eq!(vals.len(), 4);
        // Trace identity.
        let trace: C64 = (0..4).map(|i| h[[i, i]]).sum();
        let sum: C64 = vals.iter().copied().sum();
        assert!(
            (trace - sum).norm() < 1e-9,
            "trace mismatch: {trace} vs sum {sum}"
        );
        assert_eigenpairs_valid(&h, &vals, &vecs, 1e-7);
    }

    #[test]
    fn complex_diagonal_3x3_with_imaginary_parts() {
        // Eigenvalues should be the diagonals exactly, including imag.
        let h = ndarray::arr2(&[
            [c(1.0, 2.0), c(0.0, 0.0), c(0.0, 0.0)],
            [c(0.0, 0.0), c(-3.0, 0.5), c(0.0, 0.0)],
            [c(0.0, 0.0), c(0.0, 0.0), c(4.0, -1.5)],
        ]);
        let (vals, vecs) = hessenberg_eig(&h).unwrap();
        assert_eq!(vals.len(), 3);
        assert!(find_eigenvalue(&vals, c(1.0, 2.0), 1e-10));
        assert!(find_eigenvalue(&vals, c(-3.0, 0.5), 1e-10));
        assert!(find_eigenvalue(&vals, c(4.0, -1.5), 1e-10));
        assert_eigenpairs_valid(&h, &vals, &vecs, 1e-9);
    }

    #[test]
    fn n_equals_1_returns_single_eigenvalue() {
        let h = ndarray::arr2(&[[c(7.5, -0.25)]]);
        let (vals, vecs) = hessenberg_eig(&h).unwrap();
        assert_eq!(vals.len(), 1);
        assert!(close_c(vals[0], c(7.5, -0.25), 1e-12));
        // Eigenvector for a 1×1 case is non-zero (typically [1]).
        assert!(vecs[[0, 0]].norm() > 0.0);
    }
}

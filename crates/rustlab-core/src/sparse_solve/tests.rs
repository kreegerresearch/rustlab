//! Cross-module integration tests for `sparse_solve`.

use crate::sparse_solve::{ColCountOrdering, SparseChol, SparseSolveError};
use crate::types::{SparseMat, C64};
use num_complex::Complex;

#[test]
fn coo_to_csc_real_path_rejects_complex() {
    // Build a SparseMat with one entry having a non-trivial imaginary part.
    let entries = vec![(0, 0, Complex::new(1.0, 0.1))];
    let m = SparseMat::new(1, 1, entries);
    let res: Result<crate::sparse_solve::SparseCsc<f64>, _> = m.to_csc();
    assert!(matches!(res, Err(SparseSolveError::ComplexInRealPath { .. })));
}

#[test]
fn coo_to_csc_real_path_accepts_real() {
    let entries = vec![
        (0, 0, Complex::new(2.0, 0.0)),
        (1, 1, Complex::new(3.0, 0.0)),
    ];
    let m = SparseMat::new(2, 2, entries);
    let csc: crate::sparse_solve::SparseCsc<f64> = m.to_csc().unwrap();
    assert_eq!(csc.nrows(), 2);
    assert_eq!(csc.ncols(), 2);
    assert_eq!(csc.nnz(), 2);
    assert_eq!(csc.get(0, 0), 2.0);
    assert_eq!(csc.get(1, 1), 3.0);
}

#[test]
fn coo_to_csc_complex_path() {
    let entries = vec![(0, 1, Complex::new(1.0, 2.0))];
    let m = SparseMat::new(2, 2, entries);
    let csc: crate::sparse_solve::SparseCsc<C64> = m.to_csc().unwrap();
    assert_eq!(csc.get(0, 1), Complex::new(1.0, 2.0));
}

#[test]
fn end_to_end_sparse_mat_to_solve() {
    // Build A = diag(2, 3, 4) via SparseMat, factor, solve.
    let entries = vec![
        (0, 0, Complex::new(2.0, 0.0)),
        (1, 1, Complex::new(3.0, 0.0)),
        (2, 2, Complex::new(4.0, 0.0)),
    ];
    let m = SparseMat::new(3, 3, entries);
    let csc: crate::sparse_solve::SparseCsc<f64> = m.to_csc().unwrap();
    let chol = SparseChol::factor(&csc, &ColCountOrdering).unwrap();
    let b = vec![2.0, 6.0, 12.0];
    let x = chol.solve(&b).unwrap();
    assert!((x[0] - 1.0).abs() < 1e-12);
    assert!((x[1] - 2.0).abs() < 1e-12);
    assert!((x[2] - 3.0).abs() < 1e-12);
}

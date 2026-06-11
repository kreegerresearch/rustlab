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

// ── SparseCsc numerics: round-trip, transpose, spmv, edge shapes ────────

/// Dense reference for the 3x4 fixture used by the CSC tests:
///   [ 1  0  2  0 ]
///   [ 0  3  0  0 ]
///   [ 4  0  0  5 ]
fn csc_fixture() -> crate::sparse_solve::SparseCsc<f64> {
    // COO triples sorted row-major, as SparseMat::new produces.
    let coo = vec![
        (0usize, 0usize, 1.0f64),
        (0, 2, 2.0),
        (1, 1, 3.0),
        (2, 0, 4.0),
        (2, 3, 5.0),
    ];
    crate::sparse_solve::SparseCsc::from_coo_sorted(3, 4, &coo)
}

fn csc_fixture_dense() -> [[f64; 4]; 3] {
    [
        [1.0, 0.0, 2.0, 0.0],
        [0.0, 3.0, 0.0, 0.0],
        [4.0, 0.0, 0.0, 5.0],
    ]
}

#[test]
fn csc_from_coo_sorted_round_trip_every_entry() {
    let a = csc_fixture();
    let dense = csc_fixture_dense();
    assert_eq!(a.nrows(), 3);
    assert_eq!(a.ncols(), 4);
    assert_eq!(a.nnz(), 5);
    // get() must reproduce every entry, including the structural zeros.
    for (i, row) in dense.iter().enumerate() {
        for (j, &expected) in row.iter().enumerate() {
            assert_eq!(a.get(i, j), expected, "A({i},{j})");
        }
    }
    // Column pointers: col 0 holds rows {0, 2}, col 1 {1}, col 2 {0}, col 3 {2}.
    assert_eq!(a.col_ptr, vec![0, 2, 3, 4, 5]);
}

#[test]
fn csc_transpose_pattern_correct() {
    let a = csc_fixture();
    let t = a.transpose();
    let dense = csc_fixture_dense();
    assert_eq!(t.nrows(), 4);
    assert_eq!(t.ncols(), 3);
    assert_eq!(t.nnz(), a.nnz());
    // T(j, i) == A(i, j) for every position.
    for (i, row) in dense.iter().enumerate() {
        for (j, &expected) in row.iter().enumerate() {
            assert_eq!(t.get(j, i), expected, "T({j},{i})");
        }
    }
    // Within each column of T, row indices stay sorted ascending.
    for c in 0..t.ncols() {
        let rows: Vec<usize> = t.col_iter(c).map(|(r, _)| r).collect();
        let mut sorted = rows.clone();
        sorted.sort_unstable();
        assert_eq!(rows, sorted, "column {c} rows not ascending");
    }
    // Double transpose returns the original.
    let tt = t.transpose();
    for (i, row) in dense.iter().enumerate() {
        for (j, &expected) in row.iter().enumerate() {
            assert_eq!(tt.get(i, j), expected);
        }
    }
}

#[test]
fn csc_spmv_matches_hand_computation() {
    // A = [[1, 2, 0], [0, 3, 4], [5, 0, 6]], x = [1, 2, 3].
    // y = [1·1 + 2·2, 3·2 + 4·3, 5·1 + 6·3] = [5, 18, 23].
    let coo = vec![
        (0usize, 0usize, 1.0f64),
        (0, 1, 2.0),
        (1, 1, 3.0),
        (1, 2, 4.0),
        (2, 0, 5.0),
        (2, 2, 6.0),
    ];
    let a = crate::sparse_solve::SparseCsc::from_coo_sorted(3, 3, &coo);
    let y = a.spmv(&[1.0, 2.0, 3.0]);
    assert_eq!(y, vec![5.0, 18.0, 23.0]);
}

#[test]
fn csc_empty_matrix() {
    let a = crate::sparse_solve::SparseCsc::<f64>::from_coo_sorted(3, 3, &[]);
    assert_eq!(a.nnz(), 0);
    assert_eq!(a.col_ptr, vec![0, 0, 0, 0]);
    for i in 0..3 {
        for j in 0..3 {
            assert_eq!(a.get(i, j), 0.0);
        }
    }
    assert_eq!(a.spmv(&[1.0, 2.0, 3.0]), vec![0.0, 0.0, 0.0]);
    let t = a.transpose();
    assert_eq!(t.nrows(), 3);
    assert_eq!(t.ncols(), 3);
    assert_eq!(t.nnz(), 0);
}

#[test]
fn csc_single_column() {
    // 4x1 column vector [2, 0, 0, -1]^T as a matrix.
    let coo = vec![(0usize, 0usize, 2.0f64), (3, 0, -1.0)];
    let a = crate::sparse_solve::SparseCsc::from_coo_sorted(4, 1, &coo);
    assert_eq!(a.nrows(), 4);
    assert_eq!(a.ncols(), 1);
    assert_eq!(a.col_ptr, vec![0, 2]);
    let col: Vec<(usize, f64)> = a.col_iter(0).collect();
    assert_eq!(col, vec![(0, 2.0), (3, -1.0)]);
    // spmv with scalar x: y = x[0] · column.
    assert_eq!(a.spmv(&[2.0]), vec![4.0, 0.0, 0.0, -2.0]);
    // Transpose is the 1x4 row vector.
    let t = a.transpose();
    assert_eq!(t.nrows(), 1);
    assert_eq!(t.ncols(), 4);
    assert_eq!(t.get(0, 0), 2.0);
    assert_eq!(t.get(0, 3), -1.0);
}

// ── Error paths ────────────────────────────────────────────────────────

#[test]
fn factor_rejects_non_square() {
    use crate::sparse_solve::{IdentityOrdering, SparseLU};
    // 2x3 matrix: structurally valid CSC but not factorable.
    let coo = vec![(0usize, 0usize, 1.0f64), (1, 1, 1.0)];
    let a = crate::sparse_solve::SparseCsc::from_coo_sorted(2, 3, &coo);
    let lu = SparseLU::factor(&a, &IdentityOrdering, 0.1);
    assert!(matches!(
        lu,
        Err(SparseSolveError::NotSquare { rows: 2, cols: 3 })
    ));
    let chol = SparseChol::factor(&a, &ColCountOrdering);
    assert!(matches!(
        chol,
        Err(SparseSolveError::NotSquare { rows: 2, cols: 3 })
    ));
}

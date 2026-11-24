//! Cholesky that reports the failed pivot instead of erroring, for
//! MATLAB's `[R, p] = chol(S)` and `[R, p, P] = chol(S)`
//! (dev/plans/sparse_spd_solve.md, Phase 4).

use num_complex::Complex;
use rustlab_core::sparse_solve::{
    IdentityOrdering, NestedDissectionOrdering, SparseChol, SparseCsc, SparseScalar,
    SpdAutoOrdering,
};
use rustlab_core::types::C64;

fn csc<T: SparseScalar>(n: usize, mut coo: Vec<(usize, usize, T)>) -> SparseCsc<T> {
    coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    SparseCsc::from_coo_sorted(n, n, &coo)
}

fn lap2_coo(n: usize) -> Vec<(usize, usize, f64)> {
    let id = |i: usize, j: usize| j * n + i;
    let mut coo = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let p = id(i, j);
            coo.push((p, p, 4.0));
            if i > 0 {
                coo.push((p, id(i - 1, j), -1.0));
            }
            if i + 1 < n {
                coo.push((p, id(i + 1, j), -1.0));
            }
            if j > 0 {
                coo.push((p, id(i, j - 1), -1.0));
            }
            if j + 1 < n {
                coo.push((p, id(i, j + 1), -1.0));
            }
        }
    }
    coo
}

fn dense<T: SparseScalar>(a: &SparseCsc<T>) -> Vec<Vec<T>> {
    let mut d = vec![vec![T::zero(); a.ncols()]; a.nrows()];
    for j in 0..a.ncols() {
        for (i, v) in a.col_iter(j) {
            d[i][j] = v;
        }
    }
    d
}

/// `L * L^H` for an `n × q` lower factor.
fn llh<T: SparseScalar>(l: &SparseCsc<T>) -> Vec<Vec<T>> {
    let d = dense(l);
    let (n, q) = (l.nrows(), l.ncols());
    let mut out = vec![vec![T::zero(); n]; n];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            let mut s = T::zero();
            for k in 0..q {
                s += d[i][k] * d[j][k].conj();
            }
            *cell = s;
        }
    }
    out
}

fn close<T: SparseScalar>(a: T, b: T, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn spd_matches_the_ordinary_factor_bit_for_bit() {
    let a = csc(36, lap2_coo(6));
    let full = SparseChol::factor(&a, &SpdAutoOrdering).unwrap();
    let flagged = SparseChol::factor_flagged(&a, &SpdAutoOrdering).unwrap();
    assert_eq!(flagged.failed(), None);
    assert_eq!(flagged.l().ncols(), 36);
    assert_eq!(flagged.l().col_ptr, full.factor_csc().col_ptr);
    assert_eq!(flagged.l().row_idx, full.factor_csc().row_idx);
    assert_eq!(flagged.l().values, full.factor_csc().values);
    assert_eq!(flagged.perm().perm(), full.perm().perm());
}

#[test]
fn factor_reproduces_the_permuted_matrix() {
    let a = csc(144, lap2_coo(12));
    let f = SparseChol::factor_flagged(&a, &NestedDissectionOrdering).unwrap();
    let p = f.perm().perm().to_vec();
    assert_ne!(p, (0..144).collect::<Vec<_>>(), "ND should move something");
    let ad = dense(&a);
    let r = llh(f.l());
    for i in 0..144 {
        for j in 0..144 {
            assert!(close(r[i][j], ad[p[i]][p[j]], 1e-12), "({i}, {j})");
        }
    }
}

#[test]
fn indefinite_reports_the_pivot_and_the_l_shaped_block() {
    // A 5×5 Laplacian with one diagonal pushed negative: the natural-order
    // factor fails at the first pivot that cannot be positive.
    let mut coo = lap2_coo(5);
    for e in coo.iter_mut() {
        if e.0 == 12 && e.1 == 12 {
            e.2 = -2.0;
        }
    }
    let a = csc(25, coo);
    let f = SparseChol::factor_flagged(&a, &IdentityOrdering).unwrap();
    let q = f.failed().expect("indefinite");
    assert_eq!(q, 12);
    assert_eq!(f.l().nrows(), 25);
    assert_eq!(f.l().ncols(), q);
    // The ordinary factor errors at the same column.
    let err = SparseChol::factor(&a, &IdentityOrdering).unwrap_err();
    assert!(format!("{err:?}").contains("12"), "{err:?}");
    // MATLAB: the first q rows and columns of L * L^H agree with A.
    let ad = dense(&a);
    let r = llh(f.l());
    for i in 0..25 {
        for j in 0..25 {
            if i < q || j < q {
                assert!(close(r[i][j], ad[i][j], 1e-12), "({i}, {j})");
            }
        }
    }
    // Rows past q carry the computed coupling, so the block is not empty.
    let below: usize = (0..q)
        .map(|j| f.l().col_iter(j).filter(|&(i, _)| i >= q).count())
        .sum();
    assert!(below > 0);
}

#[test]
fn complex_hermitian_factors() {
    let n = 16;
    let mut coo: Vec<(usize, usize, C64)> = lap2_coo(4)
        .into_iter()
        .map(|(i, j, v)| {
            let im = if i < j {
                0.3
            } else if i > j {
                -0.3
            } else {
                0.0
            };
            (i, j, Complex::new(v, if v == -1.0 { im } else { 0.0 }))
        })
        .collect();
    coo.retain(|e| e.2 != Complex::new(0.0, 0.0));
    let a = csc(n, coo);
    assert!(a.is_hermitian(0.0));
    let f = SparseChol::factor_flagged(&a, &SpdAutoOrdering).unwrap();
    assert_eq!(f.failed(), None);
    let p = f.perm().perm().to_vec();
    let ad = dense(&a);
    let r = llh(f.l());
    for i in 0..n {
        for j in 0..n {
            assert!(close(r[i][j], ad[p[i]][p[j]], 1e-12), "({i}, {j})");
        }
    }
}

#[test]
fn empty_one_by_one_and_zero_diagonal() {
    let empty: SparseCsc<f64> = csc(0, vec![]);
    let f = SparseChol::factor_flagged(&empty, &SpdAutoOrdering).unwrap();
    assert_eq!(f.failed(), None);
    assert_eq!((f.l().nrows(), f.l().ncols()), (0, 0));

    let neg = csc(1, vec![(0, 0, -4.0)]);
    let f = SparseChol::factor_flagged(&neg, &IdentityOrdering).unwrap();
    assert_eq!(f.failed(), Some(0));
    assert_eq!((f.l().nrows(), f.l().ncols()), (1, 0));

    let pos = csc(1, vec![(0, 0, 9.0)]);
    let f = SparseChol::factor_flagged(&pos, &IdentityOrdering).unwrap();
    assert_eq!(f.failed(), None);
    assert_eq!(f.l().values, vec![3.0]);

    let zero_diag = csc(3, vec![(0, 0, 1.0), (2, 2, 1.0)]);
    let f = SparseChol::factor_flagged(&zero_diag, &IdentityOrdering).unwrap();
    assert_eq!(f.failed(), Some(1));
    assert_eq!(f.l().ncols(), 1);
}

#[test]
fn not_square_is_an_error() {
    let a: SparseCsc<f64> = SparseCsc::from_coo_sorted(2, 3, &[(0, 0, 1.0)]);
    assert!(SparseChol::factor_flagged(&a, &IdentityOrdering).is_err());
}

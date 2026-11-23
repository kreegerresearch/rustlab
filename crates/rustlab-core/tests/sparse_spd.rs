//! Row-singleton reduction, nested dissection, and the automatic SPD
//! ordering behind sparse `\` (dev/plans/sparse_spd_solve.md, Phase 1).

use num_complex::Complex;
use rustlab_core::sparse_solve::{
    symbolic_col_counts, IdentityOrdering, NestedDissectionOrdering, OrderingMethod, ReducedChol,
    RowSingletonSplit, SparseChol, SparseCsc, SparseLU, SpdAutoOrdering,
};
use rustlab_core::types::C64;

fn csc(n: usize, mut coo: Vec<(usize, usize, f64)>) -> SparseCsc<f64> {
    coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    SparseCsc::from_coo_sorted(n, n, &coo)
}

/// Positive-definite 7-point Laplacian on an `nx × ny × nz` grid, y fastest
/// (the `laplacian_3d` order).
fn lap3(nx: usize, ny: usize, nz: usize) -> SparseCsc<f64> {
    let n = nx * ny * nz;
    let id = |i: usize, j: usize, k: usize| (k * nx + j) * ny + i;
    let mut coo = Vec::new();
    for k in 0..nz {
        for j in 0..nx {
            for i in 0..ny {
                let p = id(i, j, k);
                coo.push((p, p, 6.0));
                if i > 0 {
                    coo.push((p, id(i - 1, j, k), -1.0));
                }
                if i + 1 < ny {
                    coo.push((p, id(i + 1, j, k), -1.0));
                }
                if j > 0 {
                    coo.push((p, id(i, j - 1, k), -1.0));
                }
                if j + 1 < nx {
                    coo.push((p, id(i, j + 1, k), -1.0));
                }
                if k > 0 {
                    coo.push((p, id(i, j, k - 1), -1.0));
                }
                if k + 1 < nz {
                    coo.push((p, id(i, j, k + 1), -1.0));
                }
            }
        }
    }
    csc(n, coo)
}

/// `pin_dirichlet`-style pinning: rows in `pins` become `d·e_k`, columns
/// are left alone, so the matrix is no longer symmetric.
fn pin_rows(a: &SparseCsc<f64>, pins: &[usize], d: f64) -> SparseCsc<f64> {
    let n = a.ncols();
    let mut pinned = vec![false; n];
    for &k in pins {
        pinned[k] = true;
    }
    let mut coo = Vec::new();
    for j in 0..n {
        for (i, v) in a.col_iter(j) {
            if !pinned[i] {
                coo.push((i, j, v));
            }
        }
    }
    for &k in pins {
        coo.push((k, k, d));
    }
    csc(n, coo)
}

fn rel_resid(a: &SparseCsc<f64>, x: &[f64], b: &[f64]) -> f64 {
    let r = a.spmv(x);
    let num = r
        .iter()
        .zip(b)
        .map(|(p, q)| (p - q).powi(2))
        .sum::<f64>()
        .sqrt();
    let den = b.iter().map(|q| q * q).sum::<f64>().sqrt();
    num / den
}

fn is_permutation(p: &[usize], n: usize) -> bool {
    let mut seen = vec![false; n];
    p.len() == n
        && p.iter()
            .all(|&i| i < n && !std::mem::replace(&mut seen[i], true))
}

fn fill<O: OrderingMethod>(a: &SparseCsc<f64>, ord: &O) -> usize {
    symbolic_col_counts(a, ord).unwrap().iter().sum()
}

/// Boundary of a 3-D box pinned, plus a block in the middle.
fn box_pins(nx: usize, ny: usize, nz: usize) -> Vec<usize> {
    let mut pins = Vec::new();
    for k in 0..nz {
        for j in 0..nx {
            for i in 0..ny {
                let edge = i == 0 || j == 0 || k == 0 || i + 1 == ny || j + 1 == nx || k + 1 == nz;
                let core = (3..5).contains(&i) && (3..5).contains(&j) && (3..5).contains(&k);
                if edge || core {
                    pins.push((k * nx + j) * ny + i);
                }
            }
        }
    }
    pins
}

#[test]
fn split_finds_pinned_rows_and_free_block_is_symmetric() {
    let (nx, ny, nz) = (8, 8, 8);
    let a = lap3(nx, ny, nz);
    let pins = box_pins(nx, ny, nz);
    let p = pin_rows(&a, &pins, 6.0);
    assert!(
        !p.is_hermitian(0.0),
        "pinned operator should not be symmetric"
    );
    let split = RowSingletonSplit::find(&p).expect("pinned rows are singletons");
    assert_eq!(split.n_pinned(), pins.len());
    assert_eq!(split.n_free(), nx * ny * nz - pins.len());
    assert!(split.free_block().is_hermitian(0.0));
}

#[test]
fn split_is_none_without_singletons() {
    assert!(RowSingletonSplit::find(&lap3(4, 4, 4)).is_none());
}

#[test]
fn split_ignores_a_zero_diagonal_row() {
    // Row 1 stores only a zero diagonal: not a singleton (it is singular).
    let a = csc(2, vec![(0, 0, 2.0), (0, 1, 1.0), (1, 1, 0.0)]);
    assert!(RowSingletonSplit::find(&a).is_none());
}

#[test]
fn reduced_cholesky_matches_lu_and_pins_exactly() {
    let (nx, ny, nz) = (9, 8, 7);
    let a = lap3(nx, ny, nz);
    let pins = box_pins(nx, ny, nz);
    let p = pin_rows(&a, &pins, 6.0);
    let n = p.ncols();
    let mut b: Vec<f64> = (0..n).map(|k| ((k * 7 % 13) as f64) - 6.0).collect();
    for &k in &pins {
        b[k] = 6.0 * (1.0 + (k % 3) as f64);
    }
    let split = RowSingletonSplit::find(&p).unwrap();
    let chol = ReducedChol::factor(split, &SpdAutoOrdering).unwrap();
    let x = chol.solve(&b).unwrap();
    for &k in &pins {
        assert_eq!(x[k], b[k] / 6.0, "pinned value at {k} must be b/d exactly");
    }
    assert!(rel_resid(&p, &x, &b) < 1e-13);
    let lu = SparseLU::factor(&p, &IdentityOrdering, 0.1).unwrap();
    let x_lu = lu.solve(&b).unwrap();
    let diff = x
        .iter()
        .zip(&x_lu)
        .map(|(u, v)| (u - v).powi(2))
        .sum::<f64>()
        .sqrt();
    let norm = x_lu.iter().map(|v| v * v).sum::<f64>().sqrt();
    assert!(
        diff / norm < 1e-12,
        "reduced Cholesky vs natural LU: {}",
        diff / norm
    );
}

#[test]
fn reduced_cholesky_rejects_an_indefinite_free_block() {
    // Row 2 is pinned; the free block [[1, 2], [2, 1]] is symmetric but indefinite.
    let a = csc(
        3,
        vec![
            (0, 0, 1.0),
            (0, 1, 2.0),
            (0, 2, 1.0),
            (1, 0, 2.0),
            (1, 1, 1.0),
            (2, 2, 3.0),
        ],
    );
    let split = RowSingletonSplit::find(&a).unwrap();
    assert!(ReducedChol::factor(split, &SpdAutoOrdering).is_err());
}

#[test]
fn reduced_cholesky_complex_hermitian() {
    // Free block [[4, 1+i], [1-i, 3]] is Hermitian positive definite; row 2 pinned.
    let c = |re: f64, im: f64| Complex::new(re, im);
    let mut coo: Vec<(usize, usize, C64)> = vec![
        (0, 0, c(4.0, 0.0)),
        (0, 1, c(1.0, 1.0)),
        (0, 2, c(0.5, -0.5)),
        (1, 0, c(1.0, -1.0)),
        (1, 1, c(3.0, 0.0)),
        (2, 2, c(2.0, 0.0)),
    ];
    coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let a = SparseCsc::from_coo_sorted(3, 3, &coo);
    let b = vec![c(1.0, 2.0), c(-1.0, 0.5), c(4.0, -2.0)];
    let split = RowSingletonSplit::find(&a).unwrap();
    assert!(split.free_block().is_hermitian(0.0));
    let x = ReducedChol::factor(split, &SpdAutoOrdering)
        .unwrap()
        .solve(&b)
        .unwrap();
    let r = a.spmv(&x);
    for k in 0..3 {
        assert!((r[k] - b[k]).norm() < 1e-13, "row {k}: {:?}", r[k] - b[k]);
    }
}

#[test]
fn nested_dissection_is_a_permutation() {
    for a in [
        lap3(1, 1, 1),
        lap3(5, 1, 1),
        lap3(6, 5, 4),
        lap3(12, 11, 10),
    ] {
        let p = NestedDissectionOrdering.order(&a);
        assert!(is_permutation(p.perm(), a.ncols()));
    }
    let empty = csc(0, vec![]);
    assert_eq!(NestedDissectionOrdering.order(&empty).len(), 0);
}

#[test]
fn nested_dissection_handles_disconnected_graphs() {
    // Two disjoint 3-D grids plus isolated diagonal nodes.
    let g = lap3(6, 6, 6);
    let m = g.ncols();
    let mut coo = Vec::new();
    for j in 0..m {
        for (i, v) in g.col_iter(j) {
            coo.push((i, j, v));
            coo.push((i + m, j + m, v));
        }
    }
    for k in 0..5 {
        coo.push((2 * m + k, 2 * m + k, 1.0));
    }
    let a = csc(2 * m + 5, coo);
    let p = NestedDissectionOrdering.order(&a);
    assert!(is_permutation(p.perm(), a.ncols()));
    let chol = SparseChol::factor(&a, &NestedDissectionOrdering).unwrap();
    let b: Vec<f64> = (0..a.ncols()).map(|k| (k as f64).sin()).collect();
    assert!(rel_resid(&a, &chol.solve(&b).unwrap(), &b) < 1e-13);
}

#[test]
fn nested_dissection_cuts_3d_fill() {
    let a = lap3(20, 20, 20);
    let nat = fill(&a, &IdentityOrdering);
    let nd = fill(&a, &NestedDissectionOrdering);
    assert!(2 * nd <= nat, "ND fill {nd} vs natural {nat}");
}

#[test]
fn nested_dissection_is_deterministic() {
    let a = lap3(9, 10, 11);
    assert_eq!(
        NestedDissectionOrdering.order(&a).perm(),
        NestedDissectionOrdering.order(&a).perm()
    );
}

#[test]
fn spd_auto_keeps_natural_order_on_a_band() {
    // Tridiagonal: natural order has no fill; auto must not do worse.
    let a = lap3(200, 1, 1);
    assert_eq!(fill(&a, &SpdAutoOrdering), fill(&a, &IdentityOrdering));
    assert!(SpdAutoOrdering
        .order(&a)
        .perm()
        .iter()
        .enumerate()
        .all(|(k, &v)| k == v));
}

#[test]
fn spd_auto_takes_the_smaller_fill_on_a_cube() {
    let a = lap3(16, 16, 16);
    let auto = fill(&a, &SpdAutoOrdering);
    assert!(auto <= fill(&a, &NestedDissectionOrdering));
    assert!(auto < fill(&a, &IdentityOrdering));
}

#[test]
fn spd_auto_reads_the_symmetrized_pattern() {
    // Keep only the upper triangle of a 3-D grid: the pattern of A + Aᵀ is
    // the full grid, so the ordering must match the full grid's.
    let full = lap3(12, 12, 12);
    let n = full.ncols();
    let mut upper = Vec::new();
    for j in 0..n {
        for (i, v) in full.col_iter(j) {
            if i <= j {
                upper.push((i, j, v));
            }
        }
    }
    let upper = csc(n, upper);
    assert_eq!(
        SpdAutoOrdering.order(&upper).perm(),
        SpdAutoOrdering.order(&full).perm()
    );
    assert!(fill(&full, &SpdAutoOrdering) < fill(&full, &IdentityOrdering));
}

#[test]
fn csc_hermitian_check() {
    let a = lap3(3, 3, 3);
    assert!(a.is_hermitian(0.0));
    let b = csc(
        2,
        vec![(0, 0, 1.0), (0, 1, 2.0), (1, 0, 2.0 + 1e-9), (1, 1, 1.0)],
    );
    assert!(!b.is_hermitian(0.0));
    assert!(b.is_hermitian(1e-8));
    let c = csc(2, vec![(0, 0, 1.0), (0, 1, 2.0), (1, 1, 1.0)]);
    assert!(
        !c.is_hermitian(1e-8),
        "a missing mirror entry is not Hermitian"
    );
}

//! Supernodal multifrontal Cholesky against the simplicial up-looking
//! factor (dev/plans/sparse_spd_solve.md, Phase 5).
//!
//! The supernodal kernel must give the same factor structure, the same
//! values to roundoff, the same failed column, the same permutation rules,
//! and the same result for every thread count.

use num_complex::Complex;
use rustlab_core::sparse_solve::{
    CholKernel, IdentityOrdering, NestedDissectionOrdering, OrderingMethod, SparseChol, SparseCsc,
    SparseScalar, SparseSolveError, SpdAutoOrdering,
};
use rustlab_core::types::C64;

fn csc<T: SparseScalar>(n: usize, mut coo: Vec<(usize, usize, T)>) -> SparseCsc<T> {
    coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    coo.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    SparseCsc::from_coo_sorted(n, n, &coo)
}

/// 3-D 7-point Laplacian (positive definite), `nx × ny × nz`.
fn lap3(nx: usize, ny: usize, nz: usize, shift: f64) -> SparseCsc<f64> {
    let id = |i: usize, j: usize, k: usize| (k * nx + j) * ny + i;
    let mut coo = Vec::new();
    for k in 0..nz {
        for j in 0..nx {
            for i in 0..ny {
                let p = id(i, j, k);
                coo.push((p, p, 6.0 + shift));
                let mut nb = |q: usize| coo.push((p, q, -1.0));
                if i > 0 {
                    nb(id(i - 1, j, k));
                }
                if i + 1 < ny {
                    nb(id(i + 1, j, k));
                }
                if j > 0 {
                    nb(id(i, j - 1, k));
                }
                if j + 1 < nx {
                    nb(id(i, j + 1, k));
                }
                if k > 0 {
                    nb(id(i, j, k - 1));
                }
                if k + 1 < nz {
                    nb(id(i, j, k + 1));
                }
            }
        }
    }
    csc(nx * ny * nz, coo)
}

fn lap2(n: usize) -> SparseCsc<f64> {
    lap3(n, n, 1, -2.0)
}

/// Random sparse SPD: `B'B + n I` with a fixed LCG.
fn random_spd(n: usize, per_col: usize, seed: u64) -> SparseCsc<f64> {
    let mut s = seed;
    let mut next = || {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (s >> 33) as usize
    };
    let mut dense = vec![0.0; n * n];
    for j in 0..n {
        dense[j * n + j] += n as f64;
        for _ in 0..per_col {
            let i = next() % n;
            let v = (next() % 1000) as f64 / 500.0 - 1.0;
            if i != j {
                dense[i * n + j] += v;
                dense[j * n + i] += v;
            }
        }
    }
    let mut coo = Vec::new();
    for i in 0..n {
        for j in 0..n {
            if dense[i * n + j] != 0.0 {
                coo.push((i, j, dense[i * n + j]));
            }
        }
    }
    csc(n, coo)
}

/// Complex Hermitian positive definite: 3-D Laplacian plus `±0.3j` on the
/// first off-diagonal.
fn hermitian3(n: usize) -> SparseCsc<C64> {
    let a = lap3(n, n, n, 1.0);
    let nn = a.ncols();
    let mut coo = Vec::new();
    for j in 0..nn {
        for (i, v) in a.col_iter(j) {
            coo.push((i, j, Complex::new(v, 0.0)));
        }
        if j + 1 < nn {
            coo.push((j, j + 1, Complex::new(0.0, 0.3)));
            coo.push((j + 1, j, Complex::new(0.0, -0.3)));
        }
    }
    // Merge duplicates by summing.
    coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut merged: Vec<(usize, usize, C64)> = Vec::new();
    for (i, j, v) in coo {
        match merged.last_mut() {
            Some(last) if last.0 == i && last.1 == j => last.2 += v,
            _ => merged.push((i, j, v)),
        }
    }
    SparseCsc::from_coo_sorted(nn, nn, &merged)
}

fn max_abs<T: SparseScalar>(v: &[T]) -> f64 {
    v.iter().map(|x| x.abs()).fold(0.0, f64::max)
}

/// Same structure, values within `tol` relative to the largest entry.
fn assert_same_factor<T: SparseScalar>(sim: &SparseChol<T>, sup: &SparseChol<T>, tol: f64) {
    assert_eq!(sim.perm().perm(), sup.perm().perm(), "permutations differ");
    let (ls, lp) = (sim.factor_csc(), sup.factor_csc());
    assert_eq!(ls.col_ptr, lp.col_ptr, "column pointers differ");
    assert_eq!(ls.row_idx, lp.row_idx, "row indices differ");
    let scale = max_abs(&ls.values);
    for (k, (a, b)) in ls.values.iter().zip(&lp.values).enumerate() {
        assert!(
            (*a - *b).abs() <= tol * scale,
            "entry {k}: simplicial {a:?} vs supernodal {b:?}"
        );
    }
}

fn rel_residual<T: SparseScalar>(a: &SparseCsc<T>, x: &[T], b: &[T]) -> f64 {
    let r = a.spmv(x);
    let num: f64 = r
        .iter()
        .zip(b)
        .map(|(p, q)| (*p - *q).abs2())
        .sum::<f64>()
        .sqrt();
    let den: f64 = b.iter().map(|q| q.abs2()).sum::<f64>().sqrt();
    num / den
}

fn rhs(n: usize) -> Vec<f64> {
    (0..n).map(|i| 1.0 + (i % 7) as f64 / 3.0).collect()
}

fn both<T: SparseScalar, O: OrderingMethod>(
    a: &SparseCsc<T>,
    ord: &O,
) -> (SparseChol<T>, SparseChol<T>) {
    let sim = SparseChol::factor_with(a, ord, CholKernel::Simplicial).unwrap();
    let sup = SparseChol::factor_with(a, ord, CholKernel::Supernodal).unwrap();
    assert_eq!(sim.kernel(), CholKernel::Simplicial);
    assert_eq!(sup.kernel(), CholKernel::Supernodal);
    (sim, sup)
}

#[test]
fn matches_simplicial_on_3d_poisson_nested_dissection() {
    let a = lap3(14, 13, 12, 0.0);
    let (sim, sup) = both(&a, &NestedDissectionOrdering);
    assert_same_factor(&sim, &sup, 1e-13);
    let b = rhs(a.ncols());
    let x = sup.solve(&b).unwrap();
    assert!(rel_residual(&a, &x, &b) < 1e-13);
    let xs = sim.solve(&b).unwrap();
    let d: f64 = x
        .iter()
        .zip(&xs)
        .map(|(p, q)| (p - q).abs())
        .fold(0.0, f64::max);
    assert!(d <= 1e-12 * max_abs(&xs), "solutions differ by {d}");
}

#[test]
fn matches_simplicial_in_the_natural_order() {
    // Natural order must stay natural (one-output `chol`), no postorder.
    let a = lap3(9, 8, 7, 0.0);
    let (sim, sup) = both(&a, &IdentityOrdering);
    assert!(sup.perm().perm().iter().enumerate().all(|(i, &p)| i == p));
    assert_same_factor(&sim, &sup, 1e-13);
}

#[test]
fn matches_simplicial_on_2d_and_auto_ordering() {
    let a = lap2(45);
    let (sim, sup) = both(&a, &SpdAutoOrdering);
    assert_same_factor(&sim, &sup, 1e-13);
    let b = rhs(a.ncols());
    assert!(rel_residual(&a, &sup.solve(&b).unwrap(), &b) < 1e-13);
}

#[test]
fn matches_simplicial_on_random_spd() {
    for seed in 1..4 {
        let a = random_spd(150, 4, seed);
        let (sim, sup) = both(&a, &SpdAutoOrdering);
        assert_same_factor(&sim, &sup, 1e-12);
        let b = rhs(a.ncols());
        assert!(rel_residual(&a, &sup.solve(&b).unwrap(), &b) < 1e-12);
    }
}

#[test]
fn matches_simplicial_on_complex_hermitian() {
    let a = hermitian3(8);
    let (sim, sup) = both(&a, &SpdAutoOrdering);
    assert_same_factor(&sim, &sup, 1e-13);
    let b: Vec<C64> = (0..a.ncols())
        .map(|i| Complex::new(1.0 + i as f64 * 0.01, 0.5 - (i % 3) as f64))
        .collect();
    assert!(rel_residual(&a, &sup.solve(&b).unwrap(), &b) < 1e-13);
}

#[test]
fn indefinite_reports_the_same_column() {
    // A shift that leaves a few eigenvalues negative.
    let a = lap3(10, 10, 10, -5.5);
    for kernel in [CholKernel::Simplicial, CholKernel::Supernodal] {
        let err = SparseChol::factor_with(&a, &NestedDissectionOrdering, kernel).unwrap_err();
        assert!(matches!(err, SparseSolveError::NotSpd { .. }), "{err:?}");
    }
    let col = |k| match SparseChol::factor_with(&a, &IdentityOrdering, k).unwrap_err() {
        SparseSolveError::NotSpd { col } => col,
        e => panic!("{e:?}"),
    };
    assert_eq!(col(CholKernel::Simplicial), col(CholKernel::Supernodal));
}

#[test]
fn flagged_failure_matches_simplicial_exactly() {
    let a = lap3(10, 10, 10, -5.5);
    let sim =
        SparseChol::factor_flagged_with(&a, &IdentityOrdering, CholKernel::Simplicial).unwrap();
    let sup =
        SparseChol::factor_flagged_with(&a, &IdentityOrdering, CholKernel::Supernodal).unwrap();
    assert!(sim.failed().is_some());
    assert_eq!(sim.failed(), sup.failed());
    assert_eq!(sim.l().col_ptr, sup.l().col_ptr);
    assert_eq!(sim.l().row_idx, sup.l().row_idx);
    assert_eq!(sim.l().values, sup.l().values);
}

#[test]
fn flagged_success_equals_factor() {
    let a = lap3(12, 12, 12, 0.0);
    let f = SparseChol::factor_flagged_with(&a, &SpdAutoOrdering, CholKernel::Supernodal).unwrap();
    let c = SparseChol::factor_with(&a, &SpdAutoOrdering, CholKernel::Supernodal).unwrap();
    assert!(f.failed().is_none());
    assert_eq!(f.perm().perm(), c.perm().perm());
    let l = c.factor_csc();
    assert_eq!(f.l().col_ptr, l.col_ptr);
    assert_eq!(f.l().row_idx, l.row_idx);
    assert_eq!(f.l().values, l.values);
}

#[test]
fn same_bits_for_every_thread_count() {
    rustlab_core::sparse_solve::set_parallel_min_flops(0.0);
    let a = lap3(18, 18, 18, 0.0);
    let b = rhs(a.ncols());
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let c = SparseChol::factor_with(&a, &SpdAutoOrdering, CholKernel::Supernodal).unwrap();
            (c.factor_csc().into_owned().values, c.solve(&b).unwrap())
        })
    };
    let (l1, x1) = run(1);
    for t in [2, 3, 4] {
        let (lt, xt) = run(t);
        assert_eq!(l1, lt, "factor differs with {t} threads");
        assert_eq!(x1, xt, "solution differs with {t} threads");
    }
}

#[test]
fn auto_picks_supernodal_for_large_3d_and_simplicial_for_tiny() {
    let big = lap3(20, 20, 20, 0.0);
    let c = SparseChol::factor(&big, &SpdAutoOrdering).unwrap();
    assert_eq!(c.kernel(), CholKernel::Supernodal);
    let tiny = lap2(4);
    let c = SparseChol::factor(&tiny, &SpdAutoOrdering).unwrap();
    assert_eq!(c.kernel(), CholKernel::Simplicial);
    // A tridiagonal matrix gains nothing from dense blocks.
    let n = 20_000;
    let mut coo = Vec::new();
    for i in 0..n {
        coo.push((i, i, 2.0));
        if i > 0 {
            coo.push((i, i - 1, -1.0));
            coo.push((i - 1, i, -1.0));
        }
    }
    let tri = csc(n, coo);
    let c = SparseChol::factor(&tri, &IdentityOrdering).unwrap();
    assert_eq!(c.kernel(), CholKernel::Simplicial);
}

#[test]
fn edge_sizes() {
    for kernel in [CholKernel::Simplicial, CholKernel::Supernodal] {
        let empty = SparseCsc::<f64>::from_coo_sorted(0, 0, &[]);
        let c = SparseChol::factor_with(&empty, &SpdAutoOrdering, kernel).unwrap();
        assert_eq!(c.solve(&[]).unwrap(), Vec::<f64>::new());
        let one = csc(1, vec![(0, 0, 4.0)]);
        let c = SparseChol::factor_with(&one, &SpdAutoOrdering, kernel).unwrap();
        assert_eq!(c.solve(&[2.0]).unwrap(), vec![0.5]);
        let diag = csc(5, (0..5).map(|i| (i, i, (i + 1) as f64)).collect());
        let c = SparseChol::factor_with(&diag, &SpdAutoOrdering, kernel).unwrap();
        let x = c.solve(&[1.0; 5]).unwrap();
        for (i, xi) in x.iter().enumerate() {
            assert!((xi - 1.0 / (i + 1) as f64).abs() < 1e-15);
        }
        let zero = csc(3, vec![(0, 0, 1.0), (2, 2, 1.0)]);
        assert!(SparseChol::factor_with(&zero, &IdentityOrdering, kernel).is_err());
    }
}

#[test]
fn nan_input_is_not_spd() {
    let mut a = lap3(10, 10, 10, 0.0);
    let k = a.col_ptr[500];
    a.values[k] = f64::NAN;
    assert!(SparseChol::factor_with(&a, &SpdAutoOrdering, CholKernel::Supernodal).is_err());
}

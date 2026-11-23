//! `ichol` and preconditioned conjugate gradients
//! (dev/plans/sparse_spd_solve.md, Phase 3).

use num_complex::Complex;
use rustlab_core::sparse_solve::{
    ichol, pcg, IcholOptions, IcholType, MatrixPcg, PcgOperators, SparseCsc, SparseScalar,
    TriangularSolve,
};
use rustlab_core::types::C64;

fn csc<T: SparseScalar>(n: usize, mut coo: Vec<(usize, usize, T)>) -> SparseCsc<T> {
    coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    SparseCsc::from_coo_sorted(n, n, &coo)
}

/// Positive-definite 5-point Laplacian on an `n × n` grid (unit spacing).
fn lap2(n: usize) -> SparseCsc<f64> {
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
    csc(n * n, coo)
}

/// Positive-definite 7-point Laplacian on an `n³` grid.
fn lap3(n: usize) -> SparseCsc<f64> {
    let id = |i: usize, j: usize, k: usize| (k * n + j) * n + i;
    let mut coo = Vec::new();
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let p = id(i, j, k);
                coo.push((p, p, 6.0));
                for (di, dj, dk) in [
                    (-1i64, 0i64, 0i64),
                    (1, 0, 0),
                    (0, -1, 0),
                    (0, 1, 0),
                    (0, 0, -1),
                    (0, 0, 1),
                ] {
                    let (ii, jj, kk) = (i as i64 + di, j as i64 + dj, k as i64 + dk);
                    let r = 0..n as i64;
                    if r.contains(&ii) && r.contains(&jj) && r.contains(&kk) {
                        coo.push((p, id(ii as usize, jj as usize, kk as usize), -1.0));
                    }
                }
            }
        }
    }
    csc(n * n * n, coo)
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

/// Dense `L * L'` (conjugate transpose).
fn llt<T: SparseScalar>(l: &SparseCsc<T>) -> Vec<Vec<T>> {
    let d = dense(l);
    let n = d.len();
    let mut out = vec![vec![T::zero(); n]; n];
    for i in 0..n {
        for j in 0..n {
            let mut s = T::zero();
            for k in 0..n {
                s += d[i][k] * d[j][k].conj();
            }
            out[i][j] = s;
        }
    }
    out
}

fn is_lower<T: SparseScalar>(l: &SparseCsc<T>) -> bool {
    (0..l.ncols()).all(|j| l.col_iter(j).all(|(i, _)| i >= j))
}

fn opts(kind: IcholType) -> IcholOptions {
    IcholOptions {
        kind,
        michol: false,
        diagcomp: 0.0,
    }
}

fn norm(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

fn rel_resid(a: &SparseCsc<f64>, x: &[f64], b: &[f64]) -> f64 {
    let ax = a.spmv(x);
    let r: Vec<f64> = ax.iter().zip(b).map(|(p, q)| q - p).collect();
    norm(&r) / norm(b)
}

// ─── ichol ────────────────────────────────────────────────────────────────

#[test]
fn ichol_nofill_matches_a_on_its_lower_pattern() {
    let a = lap2(7);
    let l = ichol(&a, &opts(IcholType::NoFill)).unwrap();
    assert!(is_lower(&l));
    let da = dense(&a);
    let p = llt(&l);
    for j in 0..a.ncols() {
        // L has exactly the pattern of tril(A).
        let lrows: Vec<usize> = l.col_iter(j).map(|(i, _)| i).collect();
        let arows: Vec<usize> = a.col_iter(j).map(|(i, _)| i).filter(|&i| i >= j).collect();
        assert_eq!(lrows, arows, "column {j}");
        for &i in &arows {
            assert!((p[i][j] - da[i][j]).abs() < 1e-12, "({i},{j})");
        }
    }
}

#[test]
fn ichol_nofill_reads_only_the_lower_triangle() {
    // Garbage in the strict upper triangle must not matter.
    let a = lap2(5);
    let mut coo = Vec::new();
    for j in 0..a.ncols() {
        for (i, v) in a.col_iter(j) {
            coo.push((i, j, if i < j { 99.0 } else { v }));
        }
    }
    let b = csc(a.ncols(), coo);
    let la = ichol(&a, &opts(IcholType::NoFill)).unwrap();
    let lb = ichol(&b, &opts(IcholType::NoFill)).unwrap();
    assert_eq!(la.values, lb.values);
}

#[test]
fn ichol_ict_with_zero_droptol_is_the_full_cholesky_factor() {
    let a = lap2(6);
    let l = ichol(&a, &opts(IcholType::Threshold { droptol: 0.0 })).unwrap();
    assert!(is_lower(&l));
    let da = dense(&a);
    let p = llt(&l);
    for i in 0..a.nrows() {
        for j in 0..a.ncols() {
            assert!((p[i][j] - da[i][j]).abs() < 1e-12, "({i},{j})");
        }
    }
    // More entries than IC(0): the full factor fills in the band.
    let l0 = ichol(&a, &opts(IcholType::NoFill)).unwrap();
    assert!(l.nnz() > l0.nnz());
}

#[test]
fn ichol_ict_drops_small_entries_by_column_norm() {
    let a = lap3(6);
    let droptol = 0.05;
    let l = ichol(&a, &opts(IcholType::Threshold { droptol })).unwrap();
    let full = ichol(&a, &opts(IcholType::Threshold { droptol: 0.0 })).unwrap();
    assert!(l.nnz() < full.nnz());
    for j in 0..a.ncols() {
        let colnorm: f64 = a
            .col_iter(j)
            .filter(|&(i, _)| i >= j)
            .map(|(_, v)| v.abs())
            .sum();
        for (i, v) in l.col_iter(j) {
            if i != j {
                assert!(v.abs() >= droptol * colnorm, "({i},{j}) = {v}");
            }
        }
    }
}

#[test]
fn ichol_michol_preserves_row_sums() {
    let a = lap2(6);
    let n = a.ncols();
    let ones = vec![1.0; n];
    let ae = a.spmv(&ones);
    for kind in [IcholType::NoFill, IcholType::Threshold { droptol: 0.1 }] {
        let l = ichol(
            &a,
            &IcholOptions {
                kind,
                michol: true,
                diagcomp: 0.0,
            },
        )
        .unwrap();
        let p = llt(&l);
        for i in 0..n {
            let s: f64 = p[i].iter().sum();
            assert!(
                (s - ae[i]).abs() < 1e-12,
                "{kind:?} row {i}: {s} vs {}",
                ae[i]
            );
        }
    }
}

#[test]
fn ichol_diagcomp_factors_a_shifted_matrix() {
    let a = lap2(5);
    let alpha = 0.25;
    let l = ichol(
        &a,
        &IcholOptions {
            kind: IcholType::NoFill,
            michol: false,
            diagcomp: alpha,
        },
    )
    .unwrap();
    // Same as IC(0) of A + alpha * diag(diag(A)).
    let mut coo = Vec::new();
    for j in 0..a.ncols() {
        for (i, v) in a.col_iter(j) {
            coo.push((i, j, if i == j { v * (1.0 + alpha) } else { v }));
        }
    }
    let shifted = csc(a.ncols(), coo);
    let ls = ichol(&shifted, &opts(IcholType::NoFill)).unwrap();
    assert_eq!(l.row_idx, ls.row_idx);
    for (x, y) in l.values.iter().zip(&ls.values) {
        assert!((x - y).abs() < 1e-15);
    }
}

#[test]
fn ichol_reports_a_nonpositive_pivot() {
    // Symmetric indefinite: second pivot is 1 - 4 < 0.
    let a = csc(2, vec![(0, 0, 1.0), (0, 1, 2.0), (1, 0, 2.0), (1, 1, 1.0)]);
    let e = ichol(&a, &opts(IcholType::NoFill)).unwrap_err();
    assert!(e.to_string().contains("nonpositive pivot"), "{e}");
    // A diagonal shift rescues it.
    assert!(ichol(
        &a,
        &IcholOptions {
            kind: IcholType::NoFill,
            michol: false,
            diagcomp: 4.0,
        }
    )
    .is_ok());
    // A missing diagonal is a zero pivot.
    let z = csc(2, vec![(0, 0, 1.0), (1, 0, 0.5)]);
    assert!(ichol(&z, &opts(IcholType::NoFill)).is_err());
}

#[test]
fn ichol_complex_hermitian_matches_on_the_pattern() {
    // Hermitian positive definite tridiagonal with complex off-diagonals.
    let n = 12;
    let mut coo: Vec<(usize, usize, C64)> = Vec::new();
    for i in 0..n {
        coo.push((i, i, Complex::new(4.0, 0.0)));
        if i + 1 < n {
            let v = Complex::new(-1.0, 0.5);
            coo.push((i + 1, i, v));
            coo.push((i, i + 1, v.conj()));
        }
    }
    let a = csc(n, coo);
    let l = ichol(&a, &opts(IcholType::NoFill)).unwrap();
    let da = dense(&a);
    let p = llt(&l);
    for j in 0..n {
        for (i, _) in a.col_iter(j) {
            assert!((p[i][j] - da[i][j]).abs() < 1e-12);
        }
        // Real positive diagonal.
        let d = l.get(j, j);
        assert!(d.im == 0.0 && d.re > 0.0);
    }
    // A tridiagonal has no fill: IC(0) is the exact factor.
    for i in 0..n {
        for j in 0..n {
            assert!((p[i][j] - da[i][j]).abs() < 1e-12);
        }
    }
}

#[test]
fn ichol_rejects_a_non_square_matrix() {
    let a = SparseCsc::<f64>::from_coo_sorted(2, 3, &[(0, 0, 1.0)]);
    assert!(ichol(&a, &opts(IcholType::NoFill)).is_err());
}

// ─── triangular solves ────────────────────────────────────────────────────

#[test]
fn triangular_solves_invert_l_and_l_transpose() {
    let a = lap2(5);
    let l = ichol(&a, &opts(IcholType::Threshold { droptol: 0.0 })).unwrap();
    let lt = l.transpose();
    let b: Vec<f64> = (0..a.ncols()).map(|i| (i as f64).sin() + 1.0).collect();
    let lower = TriangularSolve::classify(&l).expect("lower");
    let upper = TriangularSolve::classify(&lt).expect("upper");
    assert!(matches!(lower, TriangularSolve::Lower(_)));
    assert!(matches!(upper, TriangularSolve::Upper(_)));
    // L (L' x) = b, i.e. A x = b.
    let x = upper.solve(&lower.solve(&b));
    assert!(rel_resid(&a, &x, &b) < 1e-13);
    // A full matrix is not triangular.
    assert!(TriangularSolve::classify(&a).is_none());
    // A diagonal matrix is the diagonal kind.
    let d = csc(3, vec![(0, 0, 2.0), (1, 1, 4.0), (2, 2, 8.0)]);
    let dk = TriangularSolve::classify(&d).unwrap();
    assert!(matches!(dk, TriangularSolve::Diagonal(_)));
    assert_eq!(dk.solve(&[2.0, 2.0, 2.0]), vec![1.0, 0.5, 0.25]);
}

#[test]
fn triangular_solve_with_a_zero_diagonal_is_not_finite() {
    let l = csc(2, vec![(0, 0, 1.0), (1, 0, 1.0)]);
    let s = TriangularSolve::classify(&l).unwrap();
    let x = s.solve(&[1.0, 1.0]);
    assert!(x.iter().any(|v| !v.is_finite()));
}

// ─── pcg ──────────────────────────────────────────────────────────────────

fn rhs(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 1.0 + ((i * 7919) % 13) as f64 / 13.0)
        .collect()
}

#[test]
fn pcg_converges_within_the_condition_number_bound() {
    // 2-D Dirichlet Laplacian on m×m: eigenvalues 4 sin²(πi/2(m+1)) + 4 sin²(πj/2(m+1)).
    let m = 30;
    let a = lap2(m);
    let h = std::f64::consts::PI / (2.0 * (m as f64 + 1.0));
    let lmin = 8.0 * h.sin().powi(2);
    let lmax = 8.0 * (m as f64 * h).sin().powi(2);
    let kappa = lmax / lmin;
    let tol = 1e-8;
    // ‖r‖/‖b‖ ≤ √κ ‖e‖_A/‖e0‖_A ≤ 2√κ ((√κ−1)/(√κ+1))^k.
    let bound = (0.5 * kappa.sqrt() * (2.0 * kappa.sqrt() / tol).ln()).ceil() as usize;
    let b = rhs(m * m);
    let mut ops = MatrixPcg::new(a.clone());
    let out = pcg(&mut ops, &b, None, tol, 1000).unwrap();
    assert_eq!(out.flag, 0);
    assert!(out.iter <= bound, "{} iterations > bound {bound}", out.iter);
    assert!(out.relres <= tol);
    assert!((rel_resid(&a, &out.x, &b) - out.relres).abs() < 1e-14);
    assert_eq!(out.resvec.len(), out.iter + 1);
    assert_eq!(out.resvec[0], norm(&b));
}

#[test]
fn pcg_with_ichol_needs_fewer_iterations_than_jacobi_and_none() {
    let a = lap3(12);
    let n = a.ncols();
    let b = rhs(n);
    let tol = 1e-10;
    let none = pcg(&mut MatrixPcg::new(a.clone()), &b, None, tol, 500).unwrap();
    let diag: Vec<(usize, usize, f64)> = (0..n).map(|i| (i, i, a.get(i, i))).collect();
    let jac = pcg(
        &mut MatrixPcg::new(a.clone()).with_m1(TriangularSolve::classify(&csc(n, diag)).unwrap()),
        &b,
        None,
        tol,
        500,
    )
    .unwrap();
    let l = ichol(&a, &opts(IcholType::NoFill)).unwrap();
    let ic = pcg(
        &mut MatrixPcg::new(a.clone())
            .with_m1(TriangularSolve::classify(&l).unwrap())
            .with_m2(TriangularSolve::classify(&l.transpose()).unwrap()),
        &b,
        None,
        tol,
        500,
    )
    .unwrap();
    for o in [&none, &jac, &ic] {
        assert_eq!(o.flag, 0);
        assert!(rel_resid(&a, &o.x, &b) <= tol);
    }
    // A constant diagonal: Jacobi is a rescaling and changes nothing much.
    assert!(jac.iter <= none.iter);
    assert!(
        ic.iter < none.iter,
        "ichol {} vs none {}",
        ic.iter,
        none.iter
    );
}

#[test]
fn pcg_zero_rhs_returns_zero_without_iterating() {
    let a = lap2(4);
    let out = pcg(
        &mut MatrixPcg::new(a),
        &[0.0; 16],
        Some(&[1.0; 16]),
        1e-6,
        20,
    )
    .unwrap();
    assert_eq!(out.flag, 0);
    assert_eq!(out.iter, 0);
    assert_eq!(out.relres, 0.0);
    assert_eq!(out.resvec, vec![0.0]);
    assert!(out.zero_rhs);
    assert!(out.x.iter().all(|&v| v == 0.0));
}

#[test]
fn pcg_returns_a_good_initial_guess_unchanged() {
    let a = lap2(4);
    let x0: Vec<f64> = (0..16).map(|i| i as f64).collect();
    let b = a.spmv(&x0);
    let out = pcg(&mut MatrixPcg::new(a), &b, Some(&x0), 1e-6, 20).unwrap();
    assert_eq!(out.flag, 0);
    assert_eq!(out.iter, 0);
    assert_eq!(out.x, x0);
    assert_eq!(out.resvec.len(), 1);
}

#[test]
fn pcg_initial_guess_is_used() {
    let a = lap2(10);
    let b = rhs(100);
    let cold = pcg(&mut MatrixPcg::new(a.clone()), &b, None, 1e-10, 500).unwrap();
    // Start near the answer: fewer iterations.
    let x0: Vec<f64> = cold.x.iter().map(|v| v * (1.0 + 1e-6)).collect();
    let warm = pcg(&mut MatrixPcg::new(a), &b, Some(&x0), 1e-10, 500).unwrap();
    assert_eq!(warm.flag, 0);
    assert!(warm.iter < cold.iter);
}

#[test]
fn pcg_flag_1_at_maxit_returns_the_best_iterate() {
    let a = lap2(20);
    let b = rhs(400);
    let out = pcg(&mut MatrixPcg::new(a.clone()), &b, None, 1e-12, 5).unwrap();
    assert_eq!(out.flag, 1);
    assert_eq!(out.stopped_at, 5);
    assert_eq!(out.resvec.len(), 6);
    assert!(out.iter <= 5);
    assert!((rel_resid(&a, &out.x, &b) - out.relres).abs() < 1e-14);
    // The returned iterate has the smallest residual seen.
    let best = out.resvec.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!((out.relres * norm(&b) - best).abs() <= 1e-12 * norm(&b));
}

#[test]
fn pcg_maxit_zero_returns_x0() {
    let a = lap2(4);
    let b = rhs(16);
    let out = pcg(&mut MatrixPcg::new(a), &b, None, 1e-6, 0).unwrap();
    assert_eq!(out.flag, 1);
    assert_eq!(out.iter, 0);
    assert_eq!(out.stopped_at, 0);
    assert_eq!(out.resvec.len(), 1);
    assert!(out.x.iter().all(|&v| v == 0.0));
}

#[test]
fn pcg_flag_2_on_a_singular_preconditioner() {
    let a = lap2(4);
    let b = rhs(16);
    let mut d: Vec<(usize, usize, f64)> = (0..16).map(|i| (i, i, 4.0)).collect();
    d[3].2 = 0.0;
    let m = csc(16, d);
    let out = pcg(
        &mut MatrixPcg::new(a).with_m1(TriangularSolve::classify(&m).unwrap()),
        &b,
        None,
        1e-6,
        20,
    )
    .unwrap();
    assert_eq!(out.flag, 2);
    assert_eq!(out.iter, 0);
    assert_eq!(out.stopped_at, 1);
    // flag 2 stops before the residual of iteration 1 exists.
    assert_eq!(out.resvec.len(), 1);
}

#[test]
fn pcg_flag_4_on_an_indefinite_matrix() {
    // p'Ap = 0 on the first step: [1 0; 0 -1] with b = [1; 1].
    let a = csc(2, vec![(0, 0, 1.0), (1, 1, -1.0)]);
    let out = pcg(&mut MatrixPcg::new(a), &[1.0, 1.0], None, 1e-6, 10).unwrap();
    assert_eq!(out.flag, 4);
    assert_eq!(out.stopped_at, 1);
    assert_eq!(out.resvec.len(), 1);
}

#[test]
fn pcg_flag_3_when_the_true_residual_cannot_reach_tol() {
    // Small n (maxmsteps = 0) and a tolerance below what the
    // conditioning allows: the recursive residual says converged, the
    // true residual does not.
    let n = 12;
    let mut coo = Vec::new();
    for i in 0..n {
        for j in 0..n {
            coo.push((i, j, 1.0 / (i + j + 1) as f64));
        }
    }
    let a = csc(n, coo);
    let b = vec![1.0; n];
    let out = pcg(&mut MatrixPcg::new(a), &b, None, f64::EPSILON, 1000).unwrap();
    assert_eq!(out.flag, 3, "{out:?}");
}

#[test]
fn pcg_complex_hermitian() {
    let n = 40;
    let mut coo: Vec<(usize, usize, C64)> = Vec::new();
    for i in 0..n {
        coo.push((i, i, Complex::new(3.0, 0.0)));
        if i + 1 < n {
            let v = Complex::new(-1.0, 0.7);
            coo.push((i + 1, i, v));
            coo.push((i, i + 1, v.conj()));
        }
    }
    let a = csc(n, coo);
    let b: Vec<C64> = (0..n)
        .map(|i| Complex::new(1.0, i as f64 / n as f64))
        .collect();
    let out = pcg(&mut MatrixPcg::new(a.clone()), &b, None, 1e-12, 200).unwrap();
    assert_eq!(out.flag, 0);
    let ax = a.spmv(&out.x);
    let r: f64 = ax
        .iter()
        .zip(&b)
        .map(|(p, q)| (q - p).norm_sqr())
        .sum::<f64>()
        .sqrt();
    let nb: f64 = b.iter().map(|q| q.norm_sqr()).sum::<f64>().sqrt();
    assert!(r / nb <= 1e-12);
}

#[test]
fn pcg_errors_from_the_operator_propagate() {
    struct Failing;
    impl PcgOperators<f64> for Failing {
        type Error = String;
        fn apply_a(&mut self, _x: &[f64]) -> Result<Vec<f64>, String> {
            Err("boom".into())
        }
        fn has_m1(&self) -> bool {
            false
        }
        fn has_m2(&self) -> bool {
            false
        }
        fn solve_m1(&mut self, r: &[f64]) -> Result<Vec<f64>, String> {
            Ok(r.to_vec())
        }
        fn solve_m2(&mut self, r: &[f64]) -> Result<Vec<f64>, String> {
            Ok(r.to_vec())
        }
    }
    assert_eq!(
        pcg(&mut Failing, &[1.0], None, 1e-6, 5).unwrap_err(),
        "boom"
    );
}

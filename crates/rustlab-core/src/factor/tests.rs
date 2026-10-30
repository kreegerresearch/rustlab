//! Householder QR and partial-pivoting LU.

use ndarray::array;
use num_complex::Complex;

use super::{chol, lu, qr};
use crate::C64;

fn c(re: f64) -> C64 {
    Complex::new(re, 0.0)
}

fn max_abs(a: &ndarray::Array2<C64>) -> f64 {
    a.iter().map(|z| z.norm()).fold(0.0_f64, f64::max)
}

#[test]
fn qr_reconstructs_a_tall_real() {
    let a = array![
        [c(1.0), c(2.0), c(3.0)],
        [c(4.0), c(5.0), c(6.0)],
        [c(7.0), c(8.0), c(10.0)],
        [c(2.0), c(-1.0), c(0.5)]
    ];
    let f = qr::qr(&a, false);
    let (q, r) = qr::explicit(&f, false);
    assert_eq!(q.dim(), (4, 4));
    assert_eq!(r.dim(), (4, 3));
    let prod = q.dot(&r);
    assert!(max_abs(&(&prod - &a)) < 1e-10);
    let qtq = q.t().mapv(|z| z.conj()).dot(&q);
    for i in 0..4 {
        for j in 0..4 {
            let expect = if i == j { 1.0 } else { 0.0 };
            assert!((qtq[[i, j]].re - expect).abs() < 1e-10, "{i},{j}");
            assert!(qtq[[i, j]].im.abs() < 1e-10);
        }
    }
    for i in 0..3 {
        assert!(r[[i, i]].im.abs() < 1e-12);
        assert!(r[[i, i]].re >= 0.0);
        for j in 0..i {
            assert!(r[[i, j]].norm() < 1e-12);
        }
    }
    assert!(r[[3, 0]].norm() < 1e-12);
}

#[test]
fn qr_economy_is_the_thin_factor() {
    let a = array![[c(1.0), c(1.0)], [c(1.0), c(2.0)], [c(1.0), c(3.0)]];
    let f = qr::qr(&a, false);
    let (q, r) = qr::explicit(&f, true);
    assert_eq!(q.dim(), (3, 2));
    assert_eq!(r.dim(), (2, 2));
    let prod = q.dot(&r);
    assert!(max_abs(&(&prod - &a)) < 1e-10);
    let qtq = q.t().mapv(|z| z.conj()).dot(&q);
    assert!((qtq[[0, 0]].re - 1.0).abs() < 1e-10);
    assert!(qtq[[0, 1]].norm() < 1e-10);
    assert!((qtq[[1, 1]].re - 1.0).abs() < 1e-10);
}

#[test]
fn qr_complex_is_unitary() {
    let a = array![
        [Complex::new(1.0, 1.0), Complex::new(2.0, -1.0)],
        [Complex::new(0.0, 3.0), Complex::new(1.0, 0.5)],
        [Complex::new(-1.0, 0.0), Complex::new(4.0, 1.0)]
    ];
    let f = qr::qr(&a, false);
    let (q, r) = qr::explicit(&f, false);
    assert!(max_abs(&(&q.dot(&r) - &a)) < 1e-10);
    let qtq = q.t().mapv(|z| z.conj()).dot(&q);
    for i in 0..3 {
        for j in 0..3 {
            let expect = if i == j { 1.0 } else { 0.0 };
            assert!((qtq[[i, j]] - c(expect)).norm() < 1e-10);
        }
        assert!(r[[i.min(1), i.min(1)]].im.abs() < 1e-10 || i >= 2);
    }
    assert!(r[[0, 0]].im.abs() < 1e-10 && r[[0, 0]].re >= 0.0);
    assert!(r[[1, 1]].im.abs() < 1e-10 && r[[1, 1]].re >= 0.0);
}

#[test]
fn least_squares_residual_is_orthogonal() {
    let a = array![
        [c(1.0), c(1.0)],
        [c(1.0), c(2.0)],
        [c(1.0), c(3.0)],
        [c(1.0), c(4.0)]
    ];
    let b = array![[c(1.0)], [c(2.0)], [c(2.0)], [c(3.0)]];
    let f = qr::qr(&a, true);
    let (x, rank, _) = qr::least_squares(&f, &b);
    assert_eq!(rank, 2);
    let ax = a.dot(&x);
    let resid = &b - &ax;
    let normal = a.t().mapv(|z| z.conj()).dot(&resid);
    assert!(max_abs(&normal) < 1e-10);
}

#[test]
fn least_squares_square_matches_known_solution() {
    let a = array![[c(2.0), c(1.0)], [c(1.0), c(3.0)]];
    let b = array![[c(5.0)], [c(10.0)]];
    let f = qr::qr(&a, true);
    let (x, rank, _) = qr::least_squares(&f, &b);
    assert_eq!(rank, 2);
    assert!((x[[0, 0]] - c(1.0)).norm() < 1e-10);
    assert!((x[[1, 0]] - c(3.0)).norm() < 1e-10);
}

#[test]
fn lu_partial_pivot_known_matrix() {
    let a = array![[c(1.0), c(2.0)], [c(3.0), c(4.0)]];
    let f = lu::lu(&a);
    assert_eq!(f.perm, vec![1, 0]);
    assert!((f.l[[0, 0]] - c(1.0)).norm() < 1e-15);
    assert!(f.l[[0, 1]].norm() < 1e-15);
    assert!((f.l[[1, 0]] - c(1.0 / 3.0)).norm() < 1e-12);
    assert!((f.l[[1, 1]] - c(1.0)).norm() < 1e-15);
    assert!((f.u[[0, 0]] - c(3.0)).norm() < 1e-12);
    assert!((f.u[[0, 1]] - c(4.0)).norm() < 1e-12);
    assert!(f.u[[1, 0]].norm() < 1e-12);
    assert!((f.u[[1, 1]] - c(2.0 / 3.0)).norm() < 1e-12);
    let p = lu::permutation_matrix(&f.perm);
    let pa = p.dot(&a);
    let lu_prod = f.l.dot(&f.u);
    assert!(max_abs(&(&pa - &lu_prod)) < 1e-12);
    let b = array![[c(5.0)], [c(11.0)]];
    let x = lu::solve(&f, &b);
    assert!((x[[0, 0]] - c(1.0)).norm() < 1e-10);
    assert!((x[[1, 0]] - c(2.0)).norm() < 1e-10);
}

#[test]
fn lu_singular_is_flagged() {
    let a = array![[c(1.0), c(2.0)], [c(2.0), c(4.0)]];
    let f = lu::lu(&a);
    assert!(f.singular);
    assert!(f.rcond < 1e-12);
}

#[test]
fn chol_upper_reconstructs_spd() {
    let a = array![[c(4.0), c(2.0)], [c(2.0), c(3.0)]];
    let f = chol::upper(&a).unwrap();
    assert_eq!(f.p, 0);
    let rt = f.factor.t().mapv(|z| z.conj());
    let prod = rt.dot(&f.factor);
    assert!(max_abs(&(&prod - &a)) < 1e-12);
    // [4, 2; 2, 3] [1; 2] = [8; 8]
    let b = array![[c(8.0)], [c(8.0)]];
    let x = chol::solve_upper(&f.factor, &b);
    assert!((x[[0, 0]] - c(1.0)).norm() < 1e-10);
    assert!((x[[1, 0]] - c(2.0)).norm() < 1e-10);
}

#[test]
fn chol_lower_reconstructs_spd() {
    let a = array![[c(4.0), c(2.0)], [c(2.0), c(3.0)]];
    let f = chol::lower(&a).unwrap();
    assert_eq!(f.p, 0);
    let lh = f.factor.t().mapv(|z| z.conj());
    let prod = f.factor.dot(&lh);
    assert!(max_abs(&(&prod - &a)) < 1e-12);
}

#[test]
fn chol_negative_pivot_is_reported() {
    let a = array![[c(-1.0), c(0.0)], [c(0.0), c(2.0)]];
    let f = chol::upper(&a).unwrap();
    assert_eq!(f.p, 1);
    assert_eq!(f.factor.dim(), (0, 0));
}

#[test]
fn chol_complex_hermitian_reconstructs() {
    let a = array![
        [c(4.0), Complex::new(1.0, -1.0)],
        [Complex::new(1.0, 1.0), c(3.0)]
    ];
    let f = chol::upper(&a).unwrap();
    assert_eq!(f.p, 0);
    let rt = f.factor.t().mapv(|z| z.conj());
    let prod = rt.dot(&f.factor);
    assert!(max_abs(&(&prod - &a)) < 1e-12);
}

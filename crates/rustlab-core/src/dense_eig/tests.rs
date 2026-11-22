use super::*;
use ndarray::Array2;

fn c(re: f64) -> C64 {
    C64::new(re, 0.0)
}

fn real(rows: &[&[f64]]) -> CMatrix {
    let n = rows.len();
    Array2::from_shape_fn((n, rows[0].len()), |(i, j)| c(rows[i][j]))
}

fn blkdiag(blocks: &[CMatrix]) -> CMatrix {
    let n: usize = blocks.iter().map(|b| b.nrows()).sum();
    let mut m = Array2::zeros((n, n));
    let mut o = 0;
    for b in blocks {
        for i in 0..b.nrows() {
            for j in 0..b.ncols() {
                m[[o + i, o + j]] = b[[i, j]];
            }
        }
        o += b.nrows();
    }
    m
}

/// A deterministic pseudo-random matrix (xorshift), entries in [-1, 1).
fn rand_matrix(n: usize, seed: u64, complex: bool) -> CMatrix {
    let mut s = seed.max(1);
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    };
    Array2::from_shape_fn((n, n), |_| C64::new(next(), if complex { next() } else { 0.0 }))
}

fn fro(m: &CMatrix) -> f64 {
    m.iter().map(|z| z.norm_sqr()).sum::<f64>().sqrt()
}

/// ‖A V - V D‖ / ‖A‖, unit columns, and (optionally) full rank of V.
fn check_decomposition(a: &CMatrix, tol: f64) -> (Vec<C64>, CMatrix) {
    let (vals, v) = eig(a, true).unwrap();
    let v = v.unwrap();
    let n = a.nrows();
    // Scale-free: divide A and D by the largest entry so the check itself
    // cannot overflow for 1e200-sized matrices.
    let m = a.iter().map(|z| z.norm()).fold(f64::MIN_POSITIVE, f64::max);
    let d = Array2::from_shape_fn((n, n), |(i, j)| if i == j { vals[i] / m } else { c(0.0) });
    let am = a.mapv(|z| z / m);
    let res = fro(&(am.dot(&v) - v.dot(&d))) / fro(&am).max(f64::MIN_POSITIVE);
    assert!(res < tol, "residual {res:e} for\n{a}");
    for j in 0..n {
        let norm = (0..n).map(|i| v[[i, j]].norm_sqr()).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-12, "column {j} has norm {norm}");
    }
    (vals, v)
}

/// Every expected eigenvalue matched to a distinct computed one.
fn assert_same_set(got: &[C64], want: &[C64], tol: f64) {
    assert_eq!(got.len(), want.len());
    let mut left: Vec<C64> = got.to_vec();
    for w in want {
        let (k, d) = left
            .iter()
            .enumerate()
            .map(|(k, g)| (k, (g - w).norm()))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert!(d <= tol * w.norm().max(1.0), "eigenvalue {w} missing (closest off by {d:e}); got {got:?}");
        left.remove(k);
    }
}

#[test]
fn random_real_and_complex_matrices_decompose() {
    for (n, seed) in [(3, 1), (5, 2), (12, 3), (40, 4)] {
        check_decomposition(&rand_matrix(n, seed, false), 1e-12 * n as f64);
        check_decomposition(&rand_matrix(n, seed + 100, true), 1e-12 * n as f64);
    }
}

#[test]
fn block_diagonal_matrices_get_every_block_right() {
    // Before the rewrite, a split in the middle of the Hessenberg matrix kept
    // the iteration on the whole matrix and forced unconverged values out.
    let a = rand_matrix(3, 7, false);
    let b = rand_matrix(3, 8, false);
    let ea = eigenvalues(&a).unwrap();
    let eb = eigenvalues(&b).unwrap();
    let rot = real(&[&[0.0, -2.0], &[2.0, 0.0]]);
    let both = |x: &[C64], y: &[C64], z: &[C64]| [x, y, z].concat();
    let cases = [
        (blkdiag(&[a.clone(), b.clone()]), both(&ea, &eb, &[])),
        (blkdiag(&[a.clone(), a.clone()]), both(&ea, &ea, &[])),
        (blkdiag(&[b.clone(), rot, a.clone()]), both(&eb, &[C64::new(0.0, 2.0), C64::new(0.0, -2.0)], &ea)),
    ];
    for (m, want) in cases {
        let (vals, _) = check_decomposition(&m, 1e-12);
        assert_same_set(&vals, &want, 1e-12);
    }
    let cb = rand_matrix(4, 9, true);
    check_decomposition(&blkdiag(&[cb.clone(), rand_matrix(3, 10, true), cb]), 1e-12);
}

#[test]
fn triangular_eigenvalues_are_the_diagonal() {
    let t = real(&[&[4.0, 1.0, 2.0, 3.0], &[0.0, -2.0, 5.0, 1.0], &[0.0, 0.0, 7.0, 2.0], &[0.0, 0.0, 0.0, 0.5]]);
    let (vals, _) = check_decomposition(&t, 1e-14);
    assert_same_set(&vals, &[c(4.0), c(-2.0), c(7.0), c(0.5)], 1e-14);
}

#[test]
fn rotations_and_roots_of_unity() {
    let (vals, _) = check_decomposition(&real(&[&[0.0, -1.0], &[1.0, 0.0]]), 1e-15);
    assert_same_set(&vals, &[C64::new(0.0, 1.0), C64::new(0.0, -1.0)], 1e-15);
    // Companion matrix of x⁴ - 1: a zero Wilkinson shift, needs the
    // exceptional shift.
    let comp = real(&[&[0.0, 0.0, 0.0, 1.0], &[1.0, 0.0, 0.0, 0.0], &[0.0, 1.0, 0.0, 0.0], &[0.0, 0.0, 1.0, 0.0]]);
    let (vals, _) = check_decomposition(&comp, 1e-14);
    assert_same_set(&vals, &[c(1.0), c(-1.0), C64::new(0.0, 1.0), C64::new(0.0, -1.0)], 1e-14);
}

#[test]
fn real_matrices_give_exact_conjugate_pairs() {
    let a = rand_matrix(12, 11, false);
    let (vals, v) = eig(&a, true).unwrap();
    let v = v.unwrap();
    for (k, z) in vals.iter().enumerate() {
        if z.im != 0.0 {
            assert!(vals.iter().any(|w| *w == z.conj()), "{z} has no exact conjugate");
        } else {
            // A real eigenvalue of a real matrix has a real eigenvector.
            assert!((0..12).all(|i| v[[i, k]].im == 0.0));
        }
    }
}

#[test]
fn hermitian_input_is_real_ascending_and_orthonormal() {
    let mut h = rand_matrix(6, 12, true);
    h = &h + &h.t().mapv(|z| z.conj());
    let sym = {
        let r = rand_matrix(6, 13, false);
        &r + &r.t()
    };
    for m in [h, sym] {
        let (vals, v) = check_decomposition(&m, 1e-13);
        assert!(vals.iter().all(|z| z.im == 0.0), "Hermitian eigenvalues must be real: {vals:?}");
        assert!(vals.windows(2).all(|w| w[0].re <= w[1].re), "not ascending: {vals:?}");
        let gram = v.t().mapv(|z| z.conj()).dot(&v);
        let off = fro(&(gram - Array2::<C64>::eye(6)));
        assert!(off < 1e-13, "VᴴV - I = {off:e}");
    }
}

#[test]
fn repeated_eigenvalues_get_independent_eigenvectors() {
    // eye(3) once returned three identical columns.
    for m in [
        Array2::<C64>::eye(3),
        real(&[&[2.0, 1.0, 1.0], &[1.0, 2.0, 1.0], &[1.0, 1.0, 2.0]]),
        blkdiag(&[real(&[&[2.0, 1.0], &[1.0, 3.0]]), real(&[&[2.0, 1.0], &[1.0, 3.0]])]),
        blkdiag(&[rand_matrix(3, 14, false), rand_matrix(3, 14, false)]),
    ] {
        let (_, v) = check_decomposition(&m, 1e-13);
        // Independent columns ⇔ VᴴV (Hermitian, PSD) has no eigenvalue near 0.
        let gram = v.t().mapv(|z| z.conj()).dot(&v);
        let smallest = eigenvalues(&gram).unwrap().iter().map(|z| z.re).fold(f64::INFINITY, f64::min);
        assert!(smallest > 1e-6, "eigenvectors nearly dependent (λmin(VᴴV) = {smallest:e})");
    }
}

#[test]
fn huge_and_tiny_entries_do_not_overflow() {
    for s in [1e200, 1e-200, 1e300] {
        let a = real(&[&[1.0, 2.0], &[3.0, 4.0]]).mapv(|z| z * s);
        let (vals, _) = check_decomposition(&a, 1e-14);
        let r = 33.0_f64.sqrt();
        assert_same_set(&vals, &[c((5.0 + r) / 2.0 * s), c((5.0 - r) / 2.0 * s)], 1e-14);
    }
    let big = rand_matrix(5, 15, false).mapv(|z| z * 1e250);
    check_decomposition(&big, 1e-13);
}

#[test]
fn badly_scaled_matrices_keep_accuracy() {
    let a = blkdiag(&[real(&[&[1e6, 1.0], &[0.0, 2e6]]), real(&[&[1e-6, 0.0], &[1.0, 3e-6]])]);
    let (vals, _) = check_decomposition(&a, 1e-14);
    assert_same_set(&vals, &[c(1e6), c(2e6), c(1e-6), c(3e-6)], 1e-12);
}

#[test]
fn zero_and_scalar_and_errors() {
    let (vals, v) = eig(&Array2::zeros((3, 3)), true).unwrap();
    assert!(vals.iter().all(|z| *z == c(0.0)));
    assert_eq!(v.unwrap(), Array2::<C64>::eye(3));
    assert_eq!(eigenvalues(&real(&[&[5.0]])).unwrap(), vec![c(5.0)]);
    assert!(eig(&real(&[&[1.0, f64::NAN], &[0.0, 1.0]]), false).is_err());
    assert!(eig(&Array2::zeros((2, 3)), false).is_err());
}

/// A Jordan block is not diagonalisable. The eigenvalues are still the
/// repeated diagonal; a full set of independent eigenvectors does not exist,
/// so this does not check the residual of V.
#[test]
fn defective_jordan_block_eigenvalues() {
    let j2 = real(&[&[2.0, 1.0], &[0.0, 2.0]]);
    let (vals, _) = eig(&j2, true).unwrap();
    assert_same_set(&vals, &[c(2.0), c(2.0)], 1e-12);

    let j3 = real(&[&[5.0, 1.0, 0.0], &[0.0, 5.0, 1.0], &[0.0, 0.0, 5.0]]);
    let vals = eigenvalues(&j3).unwrap();
    assert_same_set(&vals, &[c(5.0), c(5.0), c(5.0)], 1e-10);

    // Diagonalisable repeated eigenvalue: residual must be tiny.
    let a = real(&[&[2.0, 0.0, 0.0], &[0.0, 2.0, 0.0], &[0.0, 0.0, 2.0]]);
    let (vals, _) = check_decomposition(&a, 1e-12);
    assert_same_set(&vals, &[c(2.0), c(2.0), c(2.0)], 1e-12);
}

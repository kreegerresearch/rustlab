//! Cross-module integration tests for `sparse_eig`.

use crate::sparse_eig::{eigs, eigs_gen, sym_eig_jacobi, Which};
use crate::types::SparseMat;
use ndarray::Array2;
use num_complex::Complex;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * (1.0 + a.abs() + b.abs())
}

#[test]
fn jacobi_diagonalizes_symmetric_2x2() {
    // [[2, 1], [1, 2]]: eigenvalues 1, 3.
    let a = ndarray::arr2(&[[2.0, 1.0], [1.0, 2.0]]);
    let (vals, vecs) = sym_eig_jacobi(&a);
    let mut sorted = vals.clone();
    sorted.sort_by(|x, y| x.partial_cmp(y).unwrap());
    assert!(close(sorted[0], 1.0, 1e-10));
    assert!(close(sorted[1], 3.0, 1e-10));
    // Eigenvectors are orthonormal.
    let qt_q = vecs.t().dot(&vecs);
    for i in 0..2 {
        for j in 0..2 {
            let expected = if i == j { 1.0 } else { 0.0 };
            assert!(close(qt_q[[i, j]], expected, 1e-10));
        }
    }
}

#[test]
fn jacobi_handles_diagonal_input() {
    let a = Array2::from_diag(&ndarray::arr1(&[5.0, 1.0, 3.0]));
    let (vals, _vecs) = sym_eig_jacobi(&a);
    let mut sorted = vals.clone();
    sorted.sort_by(|x, y| x.partial_cmp(y).unwrap());
    assert!(close(sorted[0], 1.0, 1e-12));
    assert!(close(sorted[1], 3.0, 1e-12));
    assert!(close(sorted[2], 5.0, 1e-12));
}

/// Build the negated 5-point Laplacian on an n×n grid (SPD form).
fn negated_laplacian(nx: usize, ny: usize) -> SparseMat {
    let n = nx * ny;
    let mut entries = Vec::new();
    for j in 0..nx {
        for i in 0..ny {
            let k = j * ny + i;
            entries.push((k, k, Complex::new(4.0, 0.0)));
            if i > 0 {
                entries.push((k, k - 1, Complex::new(-1.0, 0.0)));
            }
            if i + 1 < ny {
                entries.push((k, k + 1, Complex::new(-1.0, 0.0)));
            }
            if j > 0 {
                entries.push((k, k - ny, Complex::new(-1.0, 0.0)));
            }
            if j + 1 < nx {
                entries.push((k, k + ny, Complex::new(-1.0, 0.0)));
            }
        }
    }
    SparseMat::new(n, n, entries)
}

#[test]
fn eigs_laplacian_smallest_matches_analytic() {
    // 2-D 5-point negated Laplacian on an 8x9 grid (n=72).
    // Non-square so eigenvalues stay distinct (square grids give
    // multiplicity-2 degeneracies that simple Lanczos can't enumerate
    // without restart). For Dirichlet BC the analytic eigenvalues are:
    //   λ_{m,n} = 4 - 2*cos(m π / (nx+1)) - 2*cos(n π / (ny+1))
    let nx = 8;
    let ny = 9;
    let a = negated_laplacian(nx, ny);
    let result = eigs(&a, 4, Which::SmallestMagnitude, None).unwrap();

    let mut analytic = Vec::new();
    for mm in 1..=nx {
        for nn in 1..=ny {
            let lam = 4.0
                - 2.0 * ((mm as f64) * std::f64::consts::PI / (nx as f64 + 1.0)).cos()
                - 2.0 * ((nn as f64) * std::f64::consts::PI / (ny as f64 + 1.0)).cos();
            analytic.push(lam);
        }
    }
    analytic.sort_by(|x, y| x.partial_cmp(y).unwrap());

    let mut got: Vec<f64> = result.values.iter().map(|c| c.re).collect();
    got.sort_by(|x, y| x.partial_cmp(y).unwrap());
    for k in 0..4 {
        let rel = (got[k] - analytic[k]).abs() / analytic[k].abs();
        assert!(
            rel < 0.02,
            "eigenvalue {k}: got {} expected {} (rel err {})",
            got[k],
            analytic[k],
            rel
        );
    }
}

#[test]
fn eigs_residual_is_small() {
    // 6x7 grid so eigenvalues are non-degenerate; Lanczos should
    // resolve the smallest three to at most 1e-4 residual with the
    // default Krylov dimension.
    let nx = 6;
    let ny = 7;
    let a = negated_laplacian(nx, ny);
    let result = eigs(&a, 3, Which::SmallestMagnitude, None).unwrap();
    assert!(
        result.residual < 1e-3,
        "residual {} too large",
        result.residual
    );
}

#[test]
fn eigs_largest_matches_analytic() {
    let nx = 7;
    let ny = 8;
    let a = negated_laplacian(nx, ny);
    let result = eigs(&a, 2, Which::LargestMagnitude, None).unwrap();

    let mut analytic = Vec::new();
    for mm in 1..=nx {
        for nn in 1..=ny {
            let lam = 4.0
                - 2.0 * ((mm as f64) * std::f64::consts::PI / (nx as f64 + 1.0)).cos()
                - 2.0 * ((nn as f64) * std::f64::consts::PI / (ny as f64 + 1.0)).cos();
            analytic.push(lam);
        }
    }
    analytic.sort_by(|x, y| y.partial_cmp(x).unwrap());

    let mut got: Vec<f64> = result.values.iter().map(|c| c.re).collect();
    got.sort_by(|x, y| y.partial_cmp(x).unwrap());
    for k in 0..2 {
        let rel = (got[k] - analytic[k]).abs() / analytic[k].abs();
        assert!(
            rel < 0.02,
            "largest eigenvalue {k}: got {} expected {}",
            got[k],
            analytic[k]
        );
    }
}

#[test]
fn eigs_gen_with_a_equals_b_returns_one() {
    // For non-singular A, A x = λ A x has all eigenvalues 1. Since
    // B^{-1} A = I in that case the Krylov subspace collapses after
    // one step; Lanczos can recover one eigenvalue but cannot
    // enumerate the multiplicity. Test the n=1 case which is well-
    // defined.
    let a = negated_laplacian(5, 5);
    let result = eigs_gen(&a, &a, 1, Which::SmallestMagnitude, None).unwrap();
    let v = result.values[0];
    assert!(
        (v.re - 1.0).abs() < 1e-6,
        "expected 1.0, got {} (residual {})",
        v.re,
        result.residual
    );
}

#[test]
fn eigs_gen_with_diagonal_b_scales_eigenvalues() {
    // For B = c*I, A x = λ B x has the same eigenvectors as A x = μ x
    // and λ = μ / c. Let c = 2; check that the smallest λ = (smallest μ) / 2.
    let nx = 5;
    let ny = 6;
    let a = negated_laplacian(nx, ny);

    // B = 2 * I as a sparse matrix.
    let n = nx * ny;
    let b_entries: Vec<_> = (0..n).map(|i| (i, i, Complex::new(2.0, 0.0))).collect();
    let b = SparseMat::new(n, n, b_entries);

    let result_gen = eigs_gen(&a, &b, 1, Which::SmallestMagnitude, None).unwrap();
    let result_std = eigs(&a, 1, Which::SmallestMagnitude, None).unwrap();

    let lambda_gen = result_gen.values[0].re;
    let mu_std = result_std.values[0].re;
    let expected = mu_std / 2.0;
    let rel = (lambda_gen - expected).abs() / expected.abs();
    assert!(
        rel < 1e-3,
        "expected λ ≈ {}, got {} (rel err {})",
        expected,
        lambda_gen,
        rel
    );
}

#[test]
fn eigs_rejects_non_square() {
    let a = SparseMat::new(3, 4, vec![]);
    let r = eigs(&a, 1, Which::SmallestMagnitude, None);
    assert!(r.is_err());
}

#[test]
fn eigs_rejects_too_many() {
    let a = negated_laplacian(3, 3);
    let r = eigs(&a, 100, Which::SmallestMagnitude, None);
    assert!(r.is_err());
}

// ── Krylov-iteration internals: basis orthonormality + reduced-matrix
//    structure (Lanczos tridiagonal, Arnoldi upper-Hessenberg) ─────────

#[test]
fn lanczos_basis_orthonormal_and_tridiagonal() {
    use crate::sparse_eig::Lanczos;
    use crate::sparse_solve::SparseCsc;

    // Symmetric tridiagonal SPD test matrix: diag 2, off-diag -1 (n = 8).
    let n = 8usize;
    let mut entries = Vec::new();
    for i in 0..n {
        entries.push((i, i, Complex::new(2.0, 0.0)));
        if i + 1 < n {
            entries.push((i, i + 1, Complex::new(-1.0, 0.0)));
            entries.push((i + 1, i, Complex::new(-1.0, 0.0)));
        }
    }
    let a = SparseMat::new(n, n, entries);
    let csc: SparseCsc<f64> = a.to_csc().unwrap();

    let m = 6usize;
    let mut lanczos = Lanczos::new(n);
    lanczos.run(|v| csc.spmv(v), m, 1e-12).unwrap();
    let (alpha, beta, basis) = lanczos.finish();
    assert_eq!(alpha.len(), m);
    assert_eq!(beta.len(), m - 1);
    assert_eq!(basis.len(), m);

    let dot = |a: &[f64], b: &[f64]| -> f64 { a.iter().zip(b).map(|(x, y)| x * y).sum() };

    // V^T V ≈ I — full reorthogonalization must hold this to ~machine eps.
    for i in 0..m {
        for j in 0..m {
            let expected = if i == j { 1.0 } else { 0.0 };
            let g = dot(&basis[i], &basis[j]);
            assert!(
                (g - expected).abs() < 1e-12,
                "V^T V at ({i},{j}): {g}"
            );
        }
    }

    // T = V^T A V must be tridiagonal with diag = alpha, sub/super = beta.
    for j in 0..m {
        let av = csc.spmv(&basis[j]);
        for i in 0..m {
            let t_ij = dot(&basis[i], &av);
            let expected = if i == j {
                alpha[j]
            } else if i + 1 == j {
                beta[i]
            } else if j + 1 == i {
                beta[j]
            } else {
                0.0
            };
            assert!(
                (t_ij - expected).abs() < 1e-10,
                "T({i},{j}) = {t_ij}, expected {expected}"
            );
        }
    }
}

#[test]
fn arnoldi_basis_orthonormal_and_hessenberg() {
    use crate::sparse_eig::Arnoldi;
    use crate::sparse_solve::SparseCsc;
    use crate::types::C64;

    // Non-symmetric test matrix (n = 7): diag 1..7, superdiag +1,
    // subdiag -0.5 — clearly not Hermitian.
    let n = 7usize;
    let mut entries = Vec::new();
    for i in 0..n {
        entries.push((i, i, Complex::new((i + 1) as f64, 0.0)));
        if i + 1 < n {
            entries.push((i, i + 1, Complex::new(1.0, 0.0)));
            entries.push((i + 1, i, Complex::new(-0.5, 0.0)));
        }
    }
    let a = SparseMat::new(n, n, entries);
    assert!(!a.is_hermitian(1e-10));
    let csc: SparseCsc<C64> = a.to_csc().unwrap();

    let m = 5usize;
    let mut arnoldi = Arnoldi::new(n);
    arnoldi.run(|v| csc.spmv(v), m, 1e-12).unwrap();
    let (h, basis) = arnoldi.finish();
    assert_eq!(h.nrows(), m);
    assert_eq!(h.ncols(), m);
    assert_eq!(basis.len(), m);

    let inner = |a: &[C64], b: &[C64]| -> C64 {
        a.iter()
            .zip(b)
            .map(|(x, y)| Complex::new(x.re, -x.im) * y)
            .sum()
    };

    // V^H V ≈ I.
    for i in 0..m {
        for j in 0..m {
            let expected = if i == j { 1.0 } else { 0.0 };
            let g = inner(&basis[i], &basis[j]);
            assert!(
                (g - Complex::new(expected, 0.0)).norm() < 1e-12,
                "V^H V at ({i},{j}): {g}"
            );
        }
    }

    // H is upper-Hessenberg: every entry below the first subdiagonal is 0.
    for i in 0..m {
        for j in 0..m {
            if i > j + 1 {
                assert!(
                    h[[i, j]].norm() < 1e-14,
                    "H({i},{j}) below subdiagonal: {}",
                    h[[i, j]]
                );
            }
        }
    }

    // H must equal the projection V^H A V entry-for-entry.
    for j in 0..m {
        let av = csc.spmv(&basis[j]);
        for i in 0..m {
            let proj = inner(&basis[i], &av);
            assert!(
                (proj - h[[i, j]]).norm() < 1e-10,
                "V^H A V vs H at ({i},{j}): {proj} vs {}",
                h[[i, j]]
            );
        }
    }
}

#[test]
fn eigs_nonsymmetric_arnoldi_residuals_small() {
    // Upper-triangular matrix: eigenvalues are exactly the diagonal
    // 1..6. With a full Krylov basis (max_dim = n) the Arnoldi path
    // must resolve the largest-magnitude pairs to near machine eps.
    let n = 6usize;
    let mut entries = Vec::new();
    for i in 0..n {
        entries.push((i, i, Complex::new((i + 1) as f64, 0.0)));
        if i + 1 < n {
            entries.push((i, i + 1, Complex::new(0.5, 0.0)));
        }
    }
    let a = SparseMat::new(n, n, entries);
    assert!(!a.is_hermitian(1e-10));

    let pairs = eigs(&a, 2, Which::LargestMagnitude, Some(n)).unwrap();
    assert!(pairs.residual < 1e-8, "residual = {}", pairs.residual);

    // Largest two eigenvalues are 6 and 5 (real).
    let mut mags: Vec<f64> = pairs.values.iter().map(|v| v.norm()).collect();
    mags.sort_by(|x, y| y.partial_cmp(x).unwrap());
    assert!(close(mags[0], 6.0, 1e-8), "lambda_max = {}", mags[0]);
    assert!(close(mags[1], 5.0, 1e-8), "lambda_2 = {}", mags[1]);

    // Independent residual check ||A v − λ v|| per returned pair.
    for k in 0..2 {
        let lambda = pairs.values[k];
        let v = pairs.vectors.column(k).to_owned();
        let av = a.spmv(&v).unwrap();
        let res: f64 = av
            .iter()
            .zip(v.iter())
            .map(|(avi, vi)| (avi - lambda * vi).norm_sqr())
            .sum::<f64>()
            .sqrt();
        assert!(res < 1e-8, "pair {k}: ||A v - lambda v|| = {res}");
    }
}

//! Eigenvalues + eigenvectors of a small dense complex Hessenberg matrix.
//!
//! Used by the Arnoldi path: after `m` Arnoldi iterations we have a
//! complex upper-Hessenberg `H` of size `(m+1) × m` (or `m × m` after
//! truncation). Its eigenpairs are the Ritz pairs of the underlying
//! sparse `A`.
//!
//! Implementation: small enough to densify and solve via the standard
//! complex QR algorithm with shifts. We adapt the same algorithmic
//! structure used by the script-side `eig` builtin (Hessenberg reduce +
//! shifted QR), but operate directly on the Hessenberg input, and
//! return eigenvectors in addition to eigenvalues.
//!
//! Approach:
//! 1. Run shifted QR until `H` is essentially upper triangular.
//!    Eigenvalues are the diagonal entries.
//! 2. Each eigenvector is recovered by inverse iteration on `H` with
//!    the corresponding shift — robust for the small sizes we hit
//!    (m ≤ 50) and avoids the bookkeeping of accumulating QR Q's.

use crate::sparse_eig::SparseEigError;
use crate::types::C64;
use ndarray::Array2;
use num_complex::Complex;

/// Eigenvalues + eigenvectors of a complex upper-Hessenberg matrix.
/// Returns (`eigenvalues`, `eigenvectors`) where eigenvectors are
/// columns of an `m × m` dense matrix in the same order.
pub fn hessenberg_eig(h: &Array2<C64>) -> Result<(Vec<C64>, Array2<C64>), SparseEigError> {
    let n = h.nrows();
    if n != h.ncols() {
        return Err(SparseEigError::Internal(format!(
            "hessenberg_eig: expected square, got {}x{}",
            h.nrows(),
            h.ncols()
        )));
    }

    let eigenvalues = compute_eigenvalues(h)?;

    // Inverse iteration to get each eigenvector.
    let mut eigenvectors = Array2::<C64>::zeros((n, n));
    for (k, &lambda) in eigenvalues.iter().enumerate() {
        let v = inverse_iteration(h, lambda)?;
        for i in 0..n {
            eigenvectors[[i, k]] = v[i];
        }
    }

    Ok((eigenvalues, eigenvectors))
}

/// Shifted QR on the upper-Hessenberg matrix until it becomes
/// effectively upper-triangular. Adapted from a textbook complex-QR
/// algorithm; sized for small matrices (m ≤ 50).
fn compute_eigenvalues(h_in: &Array2<C64>) -> Result<Vec<C64>, SparseEigError> {
    let n = h_in.nrows();
    if n == 0 {
        return Ok(vec![]);
    }
    if n == 1 {
        return Ok(vec![h_in[[0, 0]]]);
    }
    if n == 2 {
        return Ok(quadratic_roots(h_in));
    }

    let mut h = h_in.to_owned();
    let mut eigs: Vec<C64> = Vec::with_capacity(n);
    let max_iter_per_eig = 100;
    let mut p = n;

    while p > 0 {
        if p == 1 {
            eigs.push(h[[0, 0]]);
            break;
        }
        if p == 2 {
            for &v in quadratic_roots_view(&h, p).iter() {
                eigs.push(v);
            }
            break;
        }

        let mut converged = false;
        for _iter in 0..max_iter_per_eig {
            // Deflation: zero subdiagonal entries that are below tol.
            let mut split_at: Option<usize> = None;
            for i in (1..p).rev() {
                let tol = 1e-12 * (h[[i - 1, i - 1]].norm() + h[[i, i]].norm());
                if h[[i, i - 1]].norm() <= tol {
                    h[[i, i - 1]] = Complex::new(0.0, 0.0);
                    split_at = Some(i);
                    break;
                }
            }
            if let Some(s) = split_at {
                if s == p - 1 {
                    eigs.push(h[[p - 1, p - 1]]);
                    p -= 1;
                    converged = true;
                    break;
                } else if s == p - 2 {
                    for &v in quadratic_roots_view(&h, p).iter() {
                        eigs.push(v);
                    }
                    p -= 2;
                    converged = true;
                    break;
                }
                // Otherwise the matrix has split into upper-left and
                // lower-right blocks; we just continue iterating, the
                // QR step below will operate on the active block.
            }

            // Wilkinson shift from the trailing 2x2 block.
            let q = p - 1;
            let a = h[[q - 1, q - 1]];
            let b = h[[q - 1, q]];
            let c = h[[q, q - 1]];
            let d = h[[q, q]];
            let tr = a + d;
            let det = a * d - b * c;
            let disc = (tr * tr - Complex::new(4.0, 0.0) * det).sqrt();
            let r1 = (tr + disc) / Complex::new(2.0, 0.0);
            let r2 = (tr - disc) / Complex::new(2.0, 0.0);
            let shift = if (r1 - d).norm() < (r2 - d).norm() {
                r1
            } else {
                r2
            };

            // Implicit QR step: apply Givens rotations to chase the bulge.
            qr_step(&mut h, p, shift);
        }
        if !converged {
            return Err(SparseEigError::DidNotConverge {
                max_dim: max_iter_per_eig,
                residual: 0.0,
            });
        }
    }

    Ok(eigs)
}

fn quadratic_roots(h: &Array2<C64>) -> Vec<C64> {
    let a = h[[0, 0]];
    let b = h[[0, 1]];
    let c = h[[1, 0]];
    let d = h[[1, 1]];
    let tr = a + d;
    let det = a * d - b * c;
    let disc = (tr * tr - Complex::new(4.0, 0.0) * det).sqrt();
    vec![
        (tr + disc) / Complex::new(2.0, 0.0),
        (tr - disc) / Complex::new(2.0, 0.0),
    ]
}

fn quadratic_roots_view(h: &Array2<C64>, p: usize) -> Vec<C64> {
    let q = p - 1;
    let a = h[[q - 1, q - 1]];
    let b = h[[q - 1, q]];
    let c = h[[q, q - 1]];
    let d = h[[q, q]];
    let tr = a + d;
    let det = a * d - b * c;
    let disc = (tr * tr - Complex::new(4.0, 0.0) * det).sqrt();
    vec![
        (tr + disc) / Complex::new(2.0, 0.0),
        (tr - disc) / Complex::new(2.0, 0.0),
    ]
}

/// One implicit shifted QR step on the active `p × p` block of `h`.
fn qr_step(h: &mut Array2<C64>, p: usize, shift: C64) {
    let n = h.nrows();
    // Subtract the shift from the active block's diagonal.
    for i in 0..p {
        h[[i, i]] -= shift;
    }
    // Givens-rotate the subdiagonal away.
    let mut givens: Vec<(usize, C64, C64)> = Vec::with_capacity(p);
    for k in 0..p - 1 {
        let x = h[[k, k]];
        let y = h[[k + 1, k]];
        let r = (x.norm_sqr() + y.norm_sqr()).sqrt();
        if r < 1e-300 {
            givens.push((k, Complex::new(1.0, 0.0), Complex::new(0.0, 0.0)));
            continue;
        }
        let c = x / Complex::new(r, 0.0);
        let s = y / Complex::new(r, 0.0);
        // Apply rotation [c̄ s̄; -s c]^T from the left.
        for j in k..n {
            let hkj = h[[k, j]];
            let hkpj = h[[k + 1, j]];
            h[[k, j]] = c.conj() * hkj + s.conj() * hkpj;
            h[[k + 1, j]] = -s * hkj + c * hkpj;
        }
        givens.push((k, c, s));
    }
    // Now apply rotations from the right.
    for (k, c, s) in givens {
        let limit = (k + 2).min(n);
        for i in 0..limit {
            let hik = h[[i, k]];
            let hikp = h[[i, k + 1]];
            h[[i, k]] = c * hik + s * hikp;
            h[[i, k + 1]] = -s.conj() * hik + c.conj() * hikp;
        }
    }
    // Add the shift back.
    for i in 0..p {
        h[[i, i]] += shift;
    }
}

/// Inverse iteration: solve `(H - λ I) v = e_n` (or some random RHS),
/// normalize, repeat. Converges quickly because λ is near-exact.
fn inverse_iteration(h: &Array2<C64>, lambda: C64) -> Result<Vec<C64>, SparseEigError> {
    let n = h.nrows();
    if n == 0 {
        return Ok(vec![]);
    }

    // Build (H - λ I).
    let mut shifted = h.to_owned();
    for i in 0..n {
        shifted[[i, i]] -= lambda;
    }

    // Add a tiny perturbation to avoid exact singularity.
    let eps = Complex::new(1e-12, 0.0);
    for i in 0..n {
        shifted[[i, i]] += eps;
    }

    // Initial guess: e_n + small.
    let mut v: Vec<C64> = (0..n)
        .map(|i| Complex::new(((i + 1) as f64 * 0.7).sin(), 0.0))
        .collect();

    // Normalize.
    let nrm = vec_norm(&v);
    if nrm > 0.0 {
        for vi in v.iter_mut() {
            *vi /= Complex::new(nrm, 0.0);
        }
    }

    // 5 iterations is overkill for inverse iteration with a near-exact shift.
    for _ in 0..5 {
        let w = solve_dense(&shifted, &v)?;
        let nrm = vec_norm(&w);
        if nrm < 1e-300 {
            break;
        }
        v = w;
        for vi in v.iter_mut() {
            *vi /= Complex::new(nrm, 0.0);
        }
    }

    Ok(v)
}

fn vec_norm(v: &[C64]) -> f64 {
    v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt()
}

/// Solve `A x = b` for a small dense complex matrix via partial-pivoting LU.
/// Throws if `A` is exactly singular.
fn solve_dense(a: &Array2<C64>, b: &[C64]) -> Result<Vec<C64>, SparseEigError> {
    let n = a.nrows();
    let mut aug = Array2::<C64>::zeros((n, n + 1));
    for i in 0..n {
        for j in 0..n {
            aug[[i, j]] = a[[i, j]];
        }
        aug[[i, n]] = b[i];
    }

    for k in 0..n {
        // Partial pivoting
        let mut max_idx = k;
        let mut max_val = aug[[k, k]].norm();
        for i in (k + 1)..n {
            let v = aug[[i, k]].norm();
            if v > max_val {
                max_val = v;
                max_idx = i;
            }
        }
        if max_idx != k {
            for j in 0..n + 1 {
                let tmp = aug[[k, j]];
                aug[[k, j]] = aug[[max_idx, j]];
                aug[[max_idx, j]] = tmp;
            }
        }
        if aug[[k, k]].norm() < 1e-30 {
            return Err(SparseEigError::Internal(
                "inverse_iteration: shifted matrix is singular".into(),
            ));
        }
        for i in (k + 1)..n {
            let factor = aug[[i, k]] / aug[[k, k]];
            for j in k..n + 1 {
                let sub = factor * aug[[k, j]];
                aug[[i, j]] -= sub;
            }
        }
    }

    let mut x = vec![Complex::new(0.0, 0.0); n];
    for i in (0..n).rev() {
        let mut s = aug[[i, n]];
        for j in (i + 1)..n {
            s -= aug[[i, j]] * x[j];
        }
        x[i] = s / aug[[i, i]];
    }
    Ok(x)
}

//! Dense eigenvalue decomposition, hand-rolled (no LAPACK).
//!
//! [`eig`] dispatches on the structure of the input:
//!
//! - **Hermitian** (`A == Aᴴ` exactly, real symmetric included): Householder
//!   reduction to a real symmetric tridiagonal matrix, then the implicit QL
//!   iteration (`tql2`, Wilkinson & Reinsch). Eigenvalues are real and
//!   ascending, eigenvectors orthonormal, repeated eigenvalues included.
//! - **Real, not symmetric**: Householder reduction to Hessenberg form
//!   (`orthes`), then the Francis double-shift QR iteration in real
//!   arithmetic (`hqr2`). Complex eigenvalues come in exact conjugate pairs
//!   and real ones are exactly real.
//! - **Complex, not Hermitian**: Householder reduction to Hessenberg form,
//!   then the single-shift complex QR iteration.
//!
//! The two general paths keep the accumulated unitary transform, so the
//! iteration ends in a Schur form `A = Q T Qᴴ` with `T` (quasi-)triangular.
//! Eigenvectors come from back substitution on `T`, not inverse iteration:
//! a repeated eigenvalue of a diagonalisable matrix gets independent
//! eigenvectors.
//!
//! The general paths scale the matrix by its largest entry and balance it
//! (diagonal similarity by powers of two) before iterating, so neither very
//! large nor very small entries overflow, and badly scaled matrices keep
//! their accuracy. Eigenvectors have unit 2-norm; a complex eigenvector of a
//! non-Hermitian matrix is rotated so its largest component is real.
//!
//! The QR iterations deflate on the active block only: a negligible
//! subdiagonal entry anywhere splits the problem, so block-diagonal and
//! reducible matrices converge block by block. An iteration that does not
//! converge is an error, never a silently inaccurate result.
//!
//! References: Golub & Van Loan, *Matrix Computations* (4th ed.), ch. 7–8;
//! Wilkinson & Reinsch, *Handbook for Automatic Computation* vol. II
//! (`tred2`/`tql2`, `orthes`/`hqr2`, `balanc`).

mod balance;
mod complex;
mod hermitian;
mod real;
#[cfg(test)]
mod tests;

use crate::{CMatrix, CoreError, C64};
use ndarray::Array2;
use num_complex::Complex;

/// Iterations allowed per eigenvalue before a QR iteration gives up.
const MAX_ITER_PER_EIGENVALUE: usize = 30;

/// Eigenvalues, and eigenvectors (one per column) when `want_vectors`.
///
/// Hermitian input returns real eigenvalues in ascending order. Otherwise
/// the order is the diagonal of the Schur form.
pub fn eig(a: &CMatrix, want_vectors: bool) -> Result<(Vec<C64>, Option<CMatrix>), CoreError> {
    let n = a.nrows();
    if n != a.ncols() {
        return Err(CoreError::InvalidParameter(format!(
            "eig: matrix must be square, got {}×{}",
            n,
            a.ncols()
        )));
    }
    if n == 0 {
        return Ok((vec![], want_vectors.then(|| Array2::zeros((0, 0)))));
    }
    if a.iter().any(|z| !z.re.is_finite() || !z.im.is_finite()) {
        return Err(CoreError::InvalidParameter(
            "eig: matrix must not contain Inf or NaN".to_string(),
        ));
    }
    // Scale by the largest magnitude so squares and products stay in range.
    let scale = a.iter().map(|z| z.norm()).fold(0.0_f64, f64::max);
    if scale == 0.0 {
        let v = want_vectors.then(|| Array2::from_shape_fn((n, n), |(i, j)| unit(i == j)));
        return Ok((vec![C64::new(0.0, 0.0); n], v));
    }
    let s = a.mapv(|z| z / scale);

    let (mut vals, vecs) = if is_hermitian(a) {
        let (d, v) = hermitian::eig(&s, want_vectors)?;
        (d.into_iter().map(|x| C64::new(x, 0.0)).collect::<Vec<_>>(), v)
    } else {
        let (b, d) = balance::balance(&s);
        let (vals, y) = if a.iter().all(|z| z.im == 0.0) {
            real::eig(&b.mapv(|z| z.re), want_vectors)?
        } else {
            complex::eig(&b, want_vectors)?
        };
        let v = y.map(|mut y| {
            // Undo the balancing similarity: eigenvectors of A are D·y.
            for i in 0..n {
                for j in 0..n {
                    y[[i, j]] *= d[i];
                }
            }
            normalize_columns(&mut y);
            y
        });
        (vals, v)
    };
    for v in vals.iter_mut() {
        *v *= scale;
    }
    Ok((vals, vecs))
}

/// Eigenvalues only.
pub fn eigenvalues(a: &CMatrix) -> Result<Vec<C64>, CoreError> {
    eig(a, false).map(|(v, _)| v)
}

/// `A == Aᴴ` exactly (MATLAB's `ishermitian` test, which selects the
/// symmetric solver).
pub fn is_hermitian(a: &CMatrix) -> bool {
    let n = a.nrows();
    n == a.ncols() && (0..n).all(|i| (i..n).all(|j| a[[i, j]] == a[[j, i]].conj()))
}

fn unit(one: bool) -> C64 {
    Complex::new(if one { 1.0 } else { 0.0 }, 0.0)
}

/// Unit 2-norm columns. A column with a nonzero imaginary part is rotated
/// so its largest component is real and positive (LAPACK `zgeev`'s
/// convention); a real column keeps its sign.
fn normalize_columns(v: &mut CMatrix) {
    let n = v.nrows();
    for j in 0..v.ncols() {
        let norm = (0..n).map(|i| v[[i, j]].norm_sqr()).sum::<f64>().sqrt();
        if norm == 0.0 {
            continue;
        }
        if (0..n).all(|i| v[[i, j]].im == 0.0) {
            for i in 0..n {
                v[[i, j]] /= norm;
            }
            continue;
        }
        let big = (0..n)
            .max_by(|&p, &q| v[[p, j]].norm().total_cmp(&v[[q, j]].norm()))
            .unwrap();
        let z = v[[big, j]];
        let phase = z.conj() / (z.norm() * norm);
        for i in 0..n {
            v[[i, j]] *= phase;
        }
        v[[big, j]] = C64::new(v[[big, j]].re, 0.0);
    }
}

/// `|re| + |im|`, the cheap modulus the deflation tests use.
fn abs1(z: C64) -> f64 {
    z.re.abs() + z.im.abs()
}

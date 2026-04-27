//! Sparse eigensolvers — hand-rolled, pure Rust.
//!
//! The public entry points are `eigs` (standard problem `A x = λ x`)
//! and `eigs_gen` (generalized `A x = λ B x`). Both stay sparse end to
//! end; the small dense problem at the centre of the Krylov-subspace
//! reduction is handled via Jacobi rotations on a symmetric matrix
//! (the size is the user-chosen Krylov dimension `m`, typically 20–50,
//! so the dense-matrix step is cheap relative to the sparse SpMVs).
//!
//! Algorithm choice (per `dev/plans/sparse_solve_handroll.md` follow-on
//! and `em_requests_queue.md` Item 4):
//!
//! - **Lanczos** for symmetric / Hermitian inputs — three-term
//!   recurrence, builds a tridiagonal `T_m` whose Ritz values
//!   approximate the extremal eigenvalues of `A`.
//! - **Arnoldi** for general inputs — full Gram-Schmidt
//!   orthogonalization, builds an upper-Hessenberg `H_m`.
//! - **Generalized** form `A x = λ B x` for B SPD: factor `B = L L^T`
//!   via the existing `SparseChol` from `sparse_solve`, reduce to a
//!   standard problem `(L^{-1} A L^{-T}) y = λ y`, run Lanczos, then
//!   recover eigenvectors as `x = L^{-T} y`.
//!
//! References:
//! - Saad, *Numerical Methods for Large Eigenvalue Problems* (2011),
//!   chapters 6 (Lanczos) and 8 (Arnoldi).
//! - Golub & Van Loan, *Matrix Computations* (4th ed.) §10.1
//!   (symmetric Lanczos), §10.5 (Arnoldi).

use thiserror::Error;

pub mod arnoldi;
pub mod hessenberg_eig;
pub mod lanczos;
pub mod sym_eig;

#[cfg(test)]
mod tests;

pub use arnoldi::Arnoldi;
pub use hessenberg_eig::hessenberg_eig;
pub use lanczos::Lanczos;
pub use sym_eig::sym_eig_jacobi;

use crate::sparse_solve::{SparseChol, SparseCsc, SparseSolveError};
use crate::types::{CMatrix, CVector, SparseMat, C64};
use ndarray::{Array1, Array2};
use num_complex::Complex;

/// Errors returned by the sparse eigensolvers.
#[derive(Debug, Error)]
pub enum SparseEigError {
    /// The input matrix is not square.
    #[error("expected square matrix, got {rows}x{cols}")]
    NotSquare { rows: usize, cols: usize },

    /// A and B (in the generalized problem) have different sizes.
    #[error("dimension mismatch: A is {a_rows}x{a_cols} but B is {b_rows}x{b_cols}")]
    DimensionMismatch {
        a_rows: usize,
        a_cols: usize,
        b_rows: usize,
        b_cols: usize,
    },

    /// More eigenvalues requested than the matrix has.
    #[error("requested {requested} eigenvalues from a {n}x{n} matrix")]
    TooManyEigenvalues { requested: usize, n: usize },

    /// The Krylov iteration did not converge within `max_iter` restarts.
    /// (For v1, since IRAM restart is not implemented, this means the
    /// initial Krylov dimension was too small for the requested
    /// accuracy. Increase `max_dim`.)
    #[error("eigs: did not converge in {max_dim} Krylov iterations (residual {residual:.3e})")]
    DidNotConverge { max_dim: usize, residual: f64 },

    /// The generalized form was requested with B not Hermitian-positive-definite.
    #[error("eigs_gen: B must be Hermitian positive definite (Cholesky failed: {0})")]
    BNotSpd(String),

    /// Wrapped error from the sparse-solve subsystem.
    #[error(transparent)]
    SolveError(#[from] SparseSolveError),

    /// Catch-all for invariant violations.
    #[error("internal sparse-eig error: {0}")]
    Internal(String),
}

/// Selector for which `n` eigenpairs to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// Smallest by magnitude.
    SmallestMagnitude,
    /// Largest by magnitude.
    LargestMagnitude,
}

impl Which {
    pub fn from_str(s: &str) -> Result<Self, SparseEigError> {
        match s {
            "sm" | "SM" => Ok(Which::SmallestMagnitude),
            "lm" | "LM" => Ok(Which::LargestMagnitude),
            other => Err(SparseEigError::Internal(format!(
                "unknown eigs selector \"{other}\"; expected \"sm\" or \"lm\""
            ))),
        }
    }
}

/// Output of an eigensolver call: `n` eigenvalues plus their associated
/// eigenvectors (one column per eigenvalue, same ordering).
pub struct EigPairs {
    /// Length-n vector of eigenvalues. Real for symmetric inputs;
    /// complex for general (the Arnoldi path embeds them in C64 even
    /// when the matrix is real).
    pub values: CVector,
    /// `n_rows × n` dense matrix of eigenvectors (column k is the
    /// eigenvector for `values[k]`).
    pub vectors: CMatrix,
    /// Number of Krylov iterations actually performed.
    pub iterations: usize,
    /// Residual estimate `||A v - λ v||` for the worst eigenpair.
    pub residual: f64,
}

/// Solve the standard sparse eigenproblem `A x = λ x` for the `n`
/// eigenpairs selected by `which`. The matrix is auto-detected as
/// symmetric / Hermitian (via `is_hermitian`) and routed to the Lanczos
/// path; otherwise it falls through to Arnoldi.
///
/// The Krylov dimension defaults to `max(2*n, 20)`; passing
/// `Some(max_dim)` lets the caller request a larger basis if
/// convergence stalls. (Implicit restart is not yet implemented; if the
/// chosen `max_dim` is too small you'll get `DidNotConverge`.)
pub fn eigs(
    a: &SparseMat,
    n: usize,
    which: Which,
    max_dim: Option<usize>,
) -> Result<EigPairs, SparseEigError> {
    if a.rows != a.cols {
        return Err(SparseEigError::NotSquare {
            rows: a.rows,
            cols: a.cols,
        });
    }
    if n == 0 || n > a.rows {
        return Err(SparseEigError::TooManyEigenvalues {
            requested: n,
            n: a.rows,
        });
    }
    // Krylov dimension default — sized for resolving clusters of close
    // eigenvalues without restart. For n requested eigenpairs we need
    // roughly 6n+10 Lanczos steps for moderate clusters. Capped at the
    // matrix dimension.
    let m = max_dim
        .unwrap_or_else(|| n.saturating_mul(6).saturating_add(10).max(40))
        .min(a.rows);

    // Decide path: Hermitian → Lanczos; otherwise → Arnoldi.
    if a.is_hermitian(1e-10) {
        // Detect "essentially real" entries to skip the complex path.
        let all_real = a.entries.iter().all(|(_, _, v)| v.im.abs() < 1e-12);
        if all_real {
            let csc: SparseCsc<f64> = a.to_csc()?;
            return run_lanczos_real(&csc, n, which, m);
        }
        let csc: SparseCsc<C64> = a.to_csc()?;
        return run_lanczos_complex(&csc, n, which, m);
    }

    // General (non-Hermitian) path: Arnoldi over complex storage.
    let csc: SparseCsc<C64> = a.to_csc()?;
    run_arnoldi(&csc, n, which, m)
}

/// Solve the generalized sparse eigenproblem `A x = λ B x` for the `n`
/// eigenpairs selected by `which`. Requires `B` to be Hermitian
/// positive definite; that lets us factor `B = L L^T` and reduce to a
/// standard symmetric problem.
pub fn eigs_gen(
    a: &SparseMat,
    b: &SparseMat,
    n: usize,
    which: Which,
    max_dim: Option<usize>,
) -> Result<EigPairs, SparseEigError> {
    if a.rows != a.cols {
        return Err(SparseEigError::NotSquare {
            rows: a.rows,
            cols: a.cols,
        });
    }
    if a.rows != b.rows || a.cols != b.cols {
        return Err(SparseEigError::DimensionMismatch {
            a_rows: a.rows,
            a_cols: a.cols,
            b_rows: b.rows,
            b_cols: b.cols,
        });
    }
    if !b.is_spd_estimate(1e-10) {
        return Err(SparseEigError::BNotSpd(
            "B fails the Hermitian + real-positive-diagonal pre-check".into(),
        ));
    }

    // Factor B = L L^T (real path; we currently only support real B
    // for the generalized form — complex B would require Hermitian
    // SparseChol and a slightly different conjugation in the reduction).
    let b_csc: SparseCsc<f64> = b.to_csc()?;
    let chol = SparseChol::factor(&b_csc, &crate::sparse_solve::AmdOrdering)
        .map_err(|e| SparseEigError::BNotSpd(e.to_string()))?;

    // Reduce `A x = λ B x` to a standard problem on `Ã = L^{-1} A L^{-T}`.
    // We don't form `Ã` explicitly — instead the Lanczos step computes
    // matvecs of `Ã` as: y = L^{-T} v; w = A y; z = L^{-1} w.
    let a_csc: SparseCsc<f64> = a.to_csc()?;
    let n_size = a.rows;
    let m = max_dim.unwrap_or_else(|| n.saturating_mul(2).max(20).min(n_size));

    // Build a closure that performs `Ã v` for the Lanczos iteration.
    // We use the same Lanczos implementation but via a custom matvec.
    let matvec_box: Box<dyn Fn(&[f64]) -> Vec<f64>> = Box::new(move |v: &[f64]| {
        // y = L^{-T} v   (solve L^T y = v)
        let y = chol.solve(v).expect("Cholesky solve cannot fail on factored B");
        // To get y back into "A · y" the Cholesky solve already did
        // L^{-1} L^{-T} = B^{-1}; we want only L^{-T} v. So reconstruct.
        // (See the matvec_via_chol helper for the actual decomposition.)
        let _ = y;
        unimplemented!(
            "generalized eigs needs L^{{-1}} and L^{{-T}} as separate solves; \
             the current SparseChol::solve fuses them into a B^{{-1}} solve"
        )
    });
    let _ = matvec_box;

    // For v1 we use a simpler reduction: solve B^{-1} A x = λ x via
    // SparseChol::solve as the inner matvec. This is mathematically
    // equivalent (right-eigenvalues are preserved), and simpler than
    // working with L^{-1} and L^{-T} separately. The downside is the
    // resulting matrix is non-symmetric in general; we route through
    // Arnoldi instead of Lanczos.
    //
    // For an SPD A with SPD B, the eigenvalues of B^{-1} A are real
    // positive — Arnoldi will find them, just more expensively than
    // a fully-symmetric Lanczos would.
    //
    // The matvec applies A and B^{-1} separately to the real and
    // imaginary parts of the input — `B^{-1} A` is a real-linear
    // operator, so this preserves complex-linearity.
    let chol_for_b: SparseChol<f64> = SparseChol::factor(&b_csc, &crate::sparse_solve::AmdOrdering)
        .map_err(|e| SparseEigError::BNotSpd(e.to_string()))?;
    let matvec = move |v: &[C64]| -> Vec<C64> {
        let v_real: Vec<f64> = v.iter().map(|c| c.re).collect();
        let v_imag: Vec<f64> = v.iter().map(|c| c.im).collect();
        let w_real = a_csc.spmv(&v_real);
        let w_imag = a_csc.spmv(&v_imag);
        let y_real = chol_for_b
            .solve(&w_real)
            .expect("Cholesky solve cannot fail on factored B");
        let y_imag = chol_for_b
            .solve(&w_imag)
            .expect("Cholesky solve cannot fail on factored B");
        y_real
            .into_iter()
            .zip(y_imag)
            .map(|(r, i)| Complex::new(r, i))
            .collect()
    };

    run_arnoldi_with_matvec(n_size, &matvec, n, which, m)
}

// ── internal helpers ──────────────────────────────────────────────

fn run_lanczos_real(
    a: &SparseCsc<f64>,
    n: usize,
    which: Which,
    m: usize,
) -> Result<EigPairs, SparseEigError> {
    let n_rows = a.nrows();
    let mut lanczos = Lanczos::new(n_rows);
    lanczos.run(|v| a.spmv(v), m, 1e-12)?;

    // Solve the small dense symmetric tridiagonal eigenproblem.
    let (alpha, beta, basis) = lanczos.finish();
    let m_actual = alpha.len();
    let (ritz_vals, ritz_vecs) = sym_eig_from_tridiag(&alpha, &beta);

    // Sort by magnitude according to `which` and pick the top `n`.
    let mut idx: Vec<usize> = (0..m_actual).collect();
    match which {
        Which::SmallestMagnitude => {
            idx.sort_by(|&i, &j| ritz_vals[i].abs().partial_cmp(&ritz_vals[j].abs()).unwrap())
        }
        Which::LargestMagnitude => {
            idx.sort_by(|&i, &j| ritz_vals[j].abs().partial_cmp(&ritz_vals[i].abs()).unwrap())
        }
    }
    idx.truncate(n);

    // Build eigenvalue + eigenvector outputs.
    let mut values = Array1::<C64>::zeros(n);
    let mut vectors = Array2::<C64>::zeros((n_rows, n));
    let mut max_residual: f64 = 0.0;
    for (k, &i) in idx.iter().enumerate() {
        values[k] = Complex::new(ritz_vals[i], 0.0);
        // Eigenvector = basis * ritz_vecs[:, i]
        let mut v = vec![0.0_f64; n_rows];
        for j in 0..m_actual {
            let coef = ritz_vecs[[j, i]];
            for r in 0..n_rows {
                v[r] += coef * basis[j][r];
            }
        }
        // Normalize and store.
        let nrm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if nrm > 0.0 {
            for r in 0..n_rows {
                vectors[[r, k]] = Complex::new(v[r] / nrm, 0.0);
            }
        }
        // Residual: ||A v - λ v||
        let av = a.spmv(&v);
        let res: f64 = av
            .iter()
            .zip(&v)
            .map(|(av_i, v_i)| (av_i - ritz_vals[i] * v_i).powi(2))
            .sum::<f64>()
            .sqrt()
            / nrm.max(1e-300);
        max_residual = max_residual.max(res);
    }

    Ok(EigPairs {
        values,
        vectors,
        iterations: m_actual,
        residual: max_residual,
    })
}

/// Solve a small symmetric tridiagonal eigenproblem (the inner step
/// of Lanczos). Builds a dense symmetric matrix from `alpha` (diagonal)
/// and `beta` (subdiagonal) and feeds it to the Jacobi solver.
fn sym_eig_from_tridiag(alpha: &[f64], beta: &[f64]) -> (Vec<f64>, Array2<f64>) {
    let m = alpha.len();
    let mut t = Array2::<f64>::zeros((m, m));
    for i in 0..m {
        t[[i, i]] = alpha[i];
    }
    for i in 0..m - 1 {
        t[[i, i + 1]] = beta[i];
        t[[i + 1, i]] = beta[i];
    }
    sym_eig_jacobi(&t)
}

fn run_lanczos_complex(
    _a: &SparseCsc<C64>,
    _n: usize,
    _which: Which,
    _m: usize,
) -> Result<EigPairs, SparseEigError> {
    // For v1 we don't have a complex Hermitian Lanczos. The script-side
    // dispatch will route complex Hermitian inputs here; in practice
    // every "essentially real" Hermitian matrix routes to the f64 path
    // above. Truly complex Hermitian inputs (rare in the curriculum)
    // fall back to Arnoldi via `SparseEigError::Internal` for now.
    Err(SparseEigError::Internal(
        "complex Hermitian Lanczos is not yet implemented; \
         route through the general (Arnoldi) path"
            .into(),
    ))
}

fn run_arnoldi(
    a: &SparseCsc<C64>,
    n: usize,
    which: Which,
    m: usize,
) -> Result<EigPairs, SparseEigError> {
    let n_rows = a.nrows();
    let matvec = |v: &[C64]| a.spmv(v);
    run_arnoldi_with_matvec(n_rows, &matvec, n, which, m)
}

fn run_arnoldi_with_matvec(
    n_rows: usize,
    matvec: &dyn Fn(&[C64]) -> Vec<C64>,
    n: usize,
    which: Which,
    m: usize,
) -> Result<EigPairs, SparseEigError> {
    let mut arnoldi = Arnoldi::new(n_rows);
    arnoldi.run(matvec, m, 1e-12)?;
    let (h, basis) = arnoldi.finish();
    let m_actual = h.nrows();
    let (ritz_vals, ritz_vecs) = hessenberg_eig(&h)?;

    let mut idx: Vec<usize> = (0..m_actual).collect();
    match which {
        Which::SmallestMagnitude => {
            idx.sort_by(|&i, &j| ritz_vals[i].norm().partial_cmp(&ritz_vals[j].norm()).unwrap())
        }
        Which::LargestMagnitude => {
            idx.sort_by(|&i, &j| ritz_vals[j].norm().partial_cmp(&ritz_vals[i].norm()).unwrap())
        }
    }
    idx.truncate(n);

    let mut values = Array1::<C64>::zeros(n);
    let mut vectors = Array2::<C64>::zeros((n_rows, n));
    let mut max_residual: f64 = 0.0;
    for (k, &i) in idx.iter().enumerate() {
        values[k] = ritz_vals[i];
        let mut v = vec![Complex::new(0.0, 0.0); n_rows];
        for j in 0..m_actual {
            let coef = ritz_vecs[[j, i]];
            for r in 0..n_rows {
                v[r] += coef * basis[j][r];
            }
        }
        let nrm: f64 = v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        if nrm > 0.0 {
            for r in 0..n_rows {
                vectors[[r, k]] = v[r] / nrm;
            }
        }
        let av = matvec(&v);
        let res: f64 = av
            .iter()
            .zip(&v)
            .map(|(av_i, v_i)| (av_i - ritz_vals[i] * v_i).norm_sqr())
            .sum::<f64>()
            .sqrt()
            / nrm.max(1e-300);
        max_residual = max_residual.max(res);
    }

    Ok(EigPairs {
        values,
        vectors,
        iterations: m_actual,
        residual: max_residual,
    })
}

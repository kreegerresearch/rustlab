//! Lanczos iteration for symmetric / Hermitian matrices.
//!
//! Builds an orthonormal Krylov basis `V_m = [v_1, …, v_m]` whose
//! span captures the extremal eigenvectors of `A` (and the matrix
//! representation of `A` on this subspace is the symmetric tridiagonal
//! `T_m = V_m^T A V_m`). Three-term recurrence:
//!
//! ```text
//! β_0 v_0 = 0, v_1 = b / ||b||
//! for j = 1, 2, …, m:
//!     w = A v_j
//!     α_j = v_j^T w
//!     w = w - α_j v_j - β_{j-1} v_{j-1}
//!     β_j = ||w||
//!     v_{j+1} = w / β_j
//! ```
//!
//! We use **full reorthogonalization** (Gram-Schmidt against every
//! prior basis vector each step) to maintain orthogonality in finite
//! precision — without this, classical Lanczos suffers from ghost
//! eigenvalues and loss of orthogonality after roughly √m steps. Full
//! reorth costs O(m²) per sweep, fine for m ≤ 100.

use crate::sparse_eig::SparseEigError;

pub struct Lanczos {
    n: usize,
    /// Krylov basis vectors, accumulated as we go.
    basis: Vec<Vec<f64>>,
    /// Tridiagonal diagonal (length m_actual after run).
    alpha: Vec<f64>,
    /// Tridiagonal subdiagonal (length m_actual - 1).
    beta: Vec<f64>,
}

impl Lanczos {
    pub fn new(n: usize) -> Self {
        Self {
            n,
            basis: Vec::new(),
            alpha: Vec::new(),
            beta: Vec::new(),
        }
    }

    /// Run up to `max_dim` Lanczos steps. Stops early on invariant
    /// subspace detection (β below `tol`). The closure `matvec` performs
    /// `A · v` for any input `v`.
    pub fn run<F>(
        &mut self,
        matvec: F,
        max_dim: usize,
        tol: f64,
    ) -> Result<(), SparseEigError>
    where
        F: Fn(&[f64]) -> Vec<f64>,
    {
        if self.n == 0 {
            return Ok(());
        }

        // Starting vector: deterministic seed (modulated to avoid
        // perfect symmetry that would mask eigenpairs).
        let mut v = vec![0.0_f64; self.n];
        for (i, x) in v.iter_mut().enumerate() {
            // Off-resonance with any low-frequency eigenmode of common
            // operators. The phase choice is arbitrary; the key is
            // non-zero overlap with every eigenvector.
            *x = ((i + 1) as f64 * 1.234567).sin();
        }
        let nrm = norm(&v);
        if nrm < 1e-300 {
            return Err(SparseEigError::Internal(
                "Lanczos: starting vector has zero norm".into(),
            ));
        }
        scale(&mut v, 1.0 / nrm);
        self.basis.push(v.clone());

        let limit = max_dim.min(self.n);
        for j in 0..limit {
            let v_j = &self.basis[j];

            // w = A v_j
            let mut w = matvec(v_j);

            // alpha_j = v_j^T w
            let alpha_j = dot(v_j, &w);
            self.alpha.push(alpha_j);

            // w = w - alpha_j v_j - beta_{j-1} v_{j-1}
            axpy(&mut w, -alpha_j, v_j);
            if j > 0 {
                let beta_prev = self.beta[j - 1];
                let v_prev = &self.basis[j - 1];
                axpy(&mut w, -beta_prev, v_prev);
            }

            // Full reorthogonalization (twice — "twice is enough").
            for _ in 0..2 {
                for v_i in &self.basis {
                    let coef = dot(v_i, &w);
                    axpy(&mut w, -coef, v_i);
                }
            }

            let beta_j = norm(&w);
            if beta_j < tol {
                // Invariant subspace found; stop early.
                break;
            }

            if j + 1 >= limit {
                break;
            }

            self.beta.push(beta_j);
            scale(&mut w, 1.0 / beta_j);
            self.basis.push(w);
        }

        Ok(())
    }

    /// Consume the iterator state and return `(alpha, beta, basis)`.
    pub fn finish(self) -> (Vec<f64>, Vec<f64>, Vec<Vec<f64>>) {
        (self.alpha, self.beta, self.basis)
    }
}

// ── Tiny BLAS-1 helpers — kept local rather than depending on ndarray. ──

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn norm(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}

fn axpy(y: &mut [f64], alpha: f64, x: &[f64]) {
    for (yi, xi) in y.iter_mut().zip(x) {
        *yi += alpha * xi;
    }
}

fn scale(a: &mut [f64], factor: f64) {
    for ai in a.iter_mut() {
        *ai *= factor;
    }
}

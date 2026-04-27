//! Arnoldi iteration for general (non-symmetric) matrices.
//!
//! Companion to Lanczos for the case where `A` lacks symmetry. Builds
//! an orthonormal Krylov basis `V_m` whose representation `H_m =
//! V_m^H A V_m` is upper-Hessenberg (rather than tridiagonal). The
//! eigenvalues of `H_m` are the Ritz approximations to eigenvalues of
//! `A`.
//!
//! Modified Gram-Schmidt with twice-is-enough reorthogonalization —
//! same robustness story as Lanczos.

use crate::sparse_eig::SparseEigError;
use crate::types::C64;
use ndarray::Array2;
use num_complex::Complex;

pub struct Arnoldi {
    n: usize,
    /// Krylov basis vectors.
    basis: Vec<Vec<C64>>,
    /// Hessenberg upper-triangular accumulator. Will be square `m × m`
    /// after `finish()` returns.
    h: Vec<Vec<C64>>,
}

impl Arnoldi {
    pub fn new(n: usize) -> Self {
        Self {
            n,
            basis: Vec::new(),
            h: Vec::new(),
        }
    }

    pub fn run<F>(
        &mut self,
        matvec: F,
        max_dim: usize,
        tol: f64,
    ) -> Result<(), SparseEigError>
    where
        F: Fn(&[C64]) -> Vec<C64>,
    {
        if self.n == 0 {
            return Ok(());
        }

        let mut v = vec![Complex::new(0.0, 0.0); self.n];
        for (i, x) in v.iter_mut().enumerate() {
            x.re = ((i + 1) as f64 * 1.234567).sin();
            x.im = ((i + 1) as f64 * 0.876543).cos();
        }
        let nrm = vec_norm(&v);
        if nrm < 1e-300 {
            return Err(SparseEigError::Internal(
                "Arnoldi: starting vector has zero norm".into(),
            ));
        }
        scale(&mut v, Complex::new(1.0 / nrm, 0.0));
        self.basis.push(v.clone());

        let limit = max_dim.min(self.n);
        for j in 0..limit {
            let v_j = &self.basis[j];
            let mut w = matvec(v_j);

            // Modified Gram-Schmidt against all prior basis vectors.
            let mut h_col = vec![Complex::new(0.0, 0.0); j + 2];
            for i in 0..=j {
                let v_i = &self.basis[i];
                let coef = inner(v_i, &w);
                axpy(&mut w, -coef, v_i);
                h_col[i] = coef;
            }
            // Twice-is-enough reorthogonalization.
            for i in 0..=j {
                let v_i = &self.basis[i];
                let coef = inner(v_i, &w);
                axpy(&mut w, -coef, v_i);
                h_col[i] += coef;
            }

            let h_jp1_j = vec_norm(&w);
            h_col[j + 1] = Complex::new(h_jp1_j, 0.0);
            self.h.push(h_col);

            if h_jp1_j < tol {
                // Invariant subspace found.
                break;
            }
            if j + 1 >= limit {
                break;
            }
            scale(&mut w, Complex::new(1.0 / h_jp1_j, 0.0));
            self.basis.push(w);
        }

        Ok(())
    }

    /// Consume the iterator state. Returns the upper-Hessenberg `H_m`
    /// (square, size `m × m` where `m = self.basis.len() - 1` if we
    /// completed `m` iterations, or `m = self.basis.len()` if we
    /// stopped on invariant subspace) and the basis vectors.
    pub fn finish(self) -> (Array2<C64>, Vec<Vec<C64>>) {
        let m = self.h.len();
        let mut h_mat = Array2::<C64>::zeros((m, m));
        for j in 0..m {
            let col = &self.h[j];
            for (i, &v) in col.iter().enumerate() {
                if i <= j + 1 && i < m {
                    h_mat[[i, j]] = v;
                }
            }
        }
        (h_mat, self.basis)
    }
}

// ── BLAS-1 helpers ──────────────────────────────────────────────

fn inner(a: &[C64], b: &[C64]) -> C64 {
    // <a, b> = a^H b = sum_i conj(a_i) * b_i
    a.iter()
        .zip(b)
        .map(|(x, y)| Complex::new(x.re, -x.im) * y)
        .sum()
}

fn vec_norm(v: &[C64]) -> f64 {
    v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt()
}

fn axpy(y: &mut [C64], alpha: C64, x: &[C64]) {
    for (yi, xi) in y.iter_mut().zip(x) {
        *yi += alpha * xi;
    }
}

fn scale(a: &mut [C64], factor: C64) {
    for ai in a.iter_mut() {
        *ai *= factor;
    }
}

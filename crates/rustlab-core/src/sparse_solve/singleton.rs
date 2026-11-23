//! Row-singleton reduction ahead of a sparse direct solve.
//!
//! A row whose only nonzero is its diagonal fixes its unknown outright:
//! `x_k = b_k / a_kk`. `pin_dirichlet` writes exactly such rows, but it
//! leaves column `k` in the neighbours' rows, so the full matrix is not
//! symmetric even when the operator was. Ordering the pinned unknowns
//! first makes `A` block lower triangular,
//!
//! ```text
//! [ D  0 ] [x_p]   [b_p]        x_p = D⁻¹ b_p
//! [ C  F ] [x_f] = [b_f]   ⇒    F x_f = b_f − C x_p
//! ```
//!
//! and the free block `F` keeps the operator's symmetry, so it can go to
//! Cholesky. UMFPACK removes singletons the same way before its LU.

use crate::sparse_solve::cholesky::SparseChol;
use crate::sparse_solve::csc::{SparseCsc, SparseScalar};
use crate::sparse_solve::ordering::OrderingMethod;
use crate::sparse_solve::SparseSolveError;

/// Tolerance for "the free block is Hermitian", the same absolute 1e-10
/// that `SparseMat::is_spd_estimate` uses for the whole matrix.
pub const HERMITIAN_TOL: f64 = 1e-10;

/// The split of a square matrix into row singletons (pinned unknowns)
/// and the remaining free block.
#[derive(Debug, Clone)]
pub struct RowSingletonSplit<T: SparseScalar> {
    n: usize,
    free: Vec<usize>,
    pinned: Vec<usize>,
    pinned_diag: Vec<T>,
    free_block: SparseCsc<T>,
    /// `A(free, pinned)`, rows and columns numbered within each set.
    coupling: SparseCsc<T>,
}

impl<T: SparseScalar> RowSingletonSplit<T> {
    /// `None` when `a` is not square or has no row singleton. A row
    /// counts when every stored off-diagonal entry is exactly zero and
    /// the diagonal is nonzero.
    pub fn find(a: &SparseCsc<T>) -> Option<Self> {
        if !a.is_square() || a.ncols() == 0 {
            return None;
        }
        let n = a.ncols();
        let mut off_diag = vec![false; n];
        let mut diag = vec![T::zero(); n];
        for j in 0..n {
            for (i, v) in a.col_iter(j) {
                if i == j {
                    diag[i] = v;
                } else if v.abs() != 0.0 {
                    off_diag[i] = true;
                }
            }
        }
        let is_pinned: Vec<bool> = (0..n).map(|i| !off_diag[i] && diag[i].abs() != 0.0).collect();
        if !is_pinned.iter().any(|&p| p) {
            return None;
        }
        let mut local = vec![0usize; n];
        let mut free = Vec::new();
        let mut pinned = Vec::new();
        for i in 0..n {
            if is_pinned[i] {
                local[i] = pinned.len();
                pinned.push(i);
            } else {
                local[i] = free.len();
                free.push(i);
            }
        }
        let pinned_diag = pinned.iter().map(|&i| diag[i]).collect();
        let take = |cols: &[usize]| -> SparseCsc<T> {
            let mut col_ptr = Vec::with_capacity(cols.len() + 1);
            let mut row_idx = Vec::new();
            let mut values = Vec::new();
            col_ptr.push(0);
            for &j in cols {
                for (i, v) in a.col_iter(j) {
                    if !is_pinned[i] {
                        row_idx.push(local[i]);
                        values.push(v);
                    }
                }
                col_ptr.push(row_idx.len());
            }
            SparseCsc {
                rows: free.len(),
                cols: cols.len(),
                col_ptr,
                row_idx,
                values,
            }
        };
        let free_block = take(&free);
        let coupling = take(&pinned);
        Some(RowSingletonSplit {
            n,
            free,
            pinned,
            pinned_diag,
            free_block,
            coupling,
        })
    }

    pub fn n(&self) -> usize {
        self.n
    }

    pub fn n_free(&self) -> usize {
        self.free.len()
    }

    pub fn n_pinned(&self) -> usize {
        self.pinned.len()
    }

    /// `A(free, free)`.
    pub fn free_block(&self) -> &SparseCsc<T> {
        &self.free_block
    }

    /// `(x_pinned, b_free − A(free, pinned)·x_pinned)`.
    pub fn reduce_rhs(&self, b: &[T]) -> Result<(Vec<T>, Vec<T>), SparseSolveError> {
        if b.len() != self.n {
            return Err(SparseSolveError::DimensionMismatch {
                a_rows: self.n,
                a_cols: self.n,
                b_len: b.len(),
            });
        }
        let x_pinned: Vec<T> = self
            .pinned
            .iter()
            .zip(&self.pinned_diag)
            .map(|(&i, &d)| b[i] / d)
            .collect();
        let mut b_free: Vec<T> = self.free.iter().map(|&i| b[i]).collect();
        for (q, &xq) in x_pinned.iter().enumerate() {
            for (r, v) in self.coupling.col_iter(q) {
                b_free[r] -= v * xq;
            }
        }
        Ok((x_pinned, b_free))
    }

    /// Scatter the two parts back into the original numbering.
    pub fn assemble(&self, x_pinned: &[T], x_free: &[T]) -> Vec<T> {
        let mut x = vec![T::zero(); self.n];
        for (&i, &v) in self.pinned.iter().zip(x_pinned) {
            x[i] = v;
        }
        for (&i, &v) in self.free.iter().zip(x_free) {
            x[i] = v;
        }
        x
    }
}

/// Cholesky of the free block of a [`RowSingletonSplit`].
#[derive(Debug)]
pub struct ReducedChol<T: SparseScalar> {
    split: RowSingletonSplit<T>,
    chol: Option<SparseChol<T>>,
}

impl<T: SparseScalar> ReducedChol<T> {
    /// Errors with `NotHermitian` when the free block is not Hermitian
    /// within [`HERMITIAN_TOL`], and `NotSpd` when it is not positive
    /// definite.
    pub fn factor<O: OrderingMethod>(
        split: RowSingletonSplit<T>,
        ord: &O,
    ) -> Result<Self, SparseSolveError> {
        if split.n_free() == 0 {
            return Ok(ReducedChol { split, chol: None });
        }
        if !split.free_block.is_hermitian(HERMITIAN_TOL) {
            return Err(SparseSolveError::NotHermitian);
        }
        let chol = SparseChol::factor(&split.free_block, ord)?;
        Ok(ReducedChol {
            split,
            chol: Some(chol),
        })
    }

    pub fn solve(&self, b: &[T]) -> Result<Vec<T>, SparseSolveError> {
        let (x_pinned, b_free) = self.split.reduce_rhs(b)?;
        let x_free = match &self.chol {
            Some(c) => c.solve(&b_free)?,
            None => Vec::new(),
        };
        Ok(self.split.assemble(&x_pinned, &x_free))
    }

    pub fn split(&self) -> &RowSingletonSplit<T> {
        &self.split
    }

    /// Stored entries of the free block's factor plus one per pinned row.
    pub fn nnz(&self) -> usize {
        self.chol.as_ref().map_or(0, |c| c.nnz()) + self.split.n_pinned()
    }
}

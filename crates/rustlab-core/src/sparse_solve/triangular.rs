//! Sparse triangular solves, used for `pcg` preconditioners such as the
//! `ichol` factor `L` and its conjugate transpose.

use crate::sparse_solve::{SparseCsc, SparseScalar};

/// A square matrix that can be inverted by substitution alone.
#[derive(Debug, Clone)]
pub enum TriangularSolve<T: SparseScalar> {
    /// Only diagonal entries (a missing one is zero).
    Diagonal(Vec<T>),
    /// No entry above the diagonal.
    Lower(SparseCsc<T>),
    /// No entry below the diagonal.
    Upper(SparseCsc<T>),
}

impl<T: SparseScalar> TriangularSolve<T> {
    /// `None` when `m` is not square or has entries on both sides of the
    /// diagonal. A diagonal matrix is `Diagonal`.
    pub fn classify(m: &SparseCsc<T>) -> Option<Self> {
        if !m.is_square() {
            return None;
        }
        let (mut below, mut above) = (false, false);
        for j in 0..m.ncols() {
            for (i, _) in m.col_iter(j) {
                below |= i > j;
                above |= i < j;
            }
        }
        match (below, above) {
            (false, false) => {
                let mut d = vec![T::zero(); m.ncols()];
                for (j, dj) in d.iter_mut().enumerate() {
                    *dj = m.get(j, j);
                }
                Some(TriangularSolve::Diagonal(d))
            }
            (true, false) => Some(TriangularSolve::Lower(m.clone())),
            (false, true) => Some(TriangularSolve::Upper(m.clone())),
            (true, true) => None,
        }
    }

    pub fn dim(&self) -> usize {
        match self {
            TriangularSolve::Diagonal(d) => d.len(),
            TriangularSolve::Lower(m) | TriangularSolve::Upper(m) => m.ncols(),
        }
    }

    /// `M \ b`. A zero (or missing) diagonal gives `Inf` / `NaN`, as
    /// MATLAB's triangular `\` does; nothing is checked here.
    pub fn solve(&self, b: &[T]) -> Vec<T> {
        let mut x = b.to_vec();
        match self {
            TriangularSolve::Diagonal(d) => {
                for (xi, &di) in x.iter_mut().zip(d) {
                    *xi = *xi / di;
                }
            }
            TriangularSolve::Lower(l) => {
                for j in 0..l.ncols() {
                    let (lo, hi) = (l.col_ptr[j], l.col_ptr[j + 1]);
                    let has_diag = lo < hi && l.row_idx[lo] == j;
                    let d = if has_diag { l.values[lo] } else { T::zero() };
                    let xj = x[j] / d;
                    x[j] = xj;
                    for p in lo + usize::from(has_diag)..hi {
                        let v = l.values[p];
                        x[l.row_idx[p]] -= v * xj;
                    }
                }
            }
            TriangularSolve::Upper(u) => {
                for j in (0..u.ncols()).rev() {
                    let (lo, hi) = (u.col_ptr[j], u.col_ptr[j + 1]);
                    let has_diag = lo < hi && u.row_idx[hi - 1] == j;
                    let d = if has_diag { u.values[hi - 1] } else { T::zero() };
                    let xj = x[j] / d;
                    x[j] = xj;
                    for p in lo..hi - usize::from(has_diag) {
                        let v = u.values[p];
                        x[u.row_idx[p]] -= v * xj;
                    }
                }
            }
        }
        x
    }
}

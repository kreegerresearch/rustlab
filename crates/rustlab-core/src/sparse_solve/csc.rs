//! Compressed Sparse Column storage and the scalar trait that lets the
//! factorization code work uniformly across `f64` and `Complex<f64>`.

use crate::sparse_solve::SparseSolveError;
use crate::types::{SparseMat, C64};
use num_complex::Complex;
use std::fmt::Debug;
use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

/// Scalar field for a sparse solver. Implementations cover the algebraic
/// operations the factorization code needs without committing to a
/// specific concrete type. Only `f64` and `Complex<f64>` are provided —
/// the trait is sealed in spirit (no `pub` constructor that would let
/// downstream crates implement it).
pub trait SparseScalar:
    Copy
    + Default
    + Debug
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
    + AddAssign
    + SubAssign
    + MulAssign
{
    fn zero() -> Self;
    fn one() -> Self;

    /// Magnitude. Used for pivot ordering and singularity tests.
    fn abs(&self) -> f64;

    /// Conjugate. Identity on real types; complex conjugate otherwise.
    fn conj(&self) -> Self;

    fn is_zero_tol(&self, tol: f64) -> bool {
        self.abs() < tol
    }

    /// Cholesky-specific: compute `sqrt(x)` for a value that is required
    /// to be real-positive (the diagonal of a Cholesky factor of an SPD
    /// matrix). Returns `None` if the input is not real-positive within
    /// the given tolerance — that signals the matrix isn't SPD.
    fn checked_sqrt_real_pos(&self, tol: f64) -> Option<Self>;
}

impl SparseScalar for f64 {
    fn zero() -> Self {
        0.0
    }
    fn one() -> Self {
        1.0
    }
    fn abs(&self) -> f64 {
        f64::abs(*self)
    }
    fn conj(&self) -> Self {
        *self
    }
    fn checked_sqrt_real_pos(&self, tol: f64) -> Option<Self> {
        if *self > tol {
            Some(self.sqrt())
        } else {
            None
        }
    }
}

impl SparseScalar for C64 {
    fn zero() -> Self {
        Complex::new(0.0, 0.0)
    }
    fn one() -> Self {
        Complex::new(1.0, 0.0)
    }
    fn abs(&self) -> f64 {
        self.norm()
    }
    fn conj(&self) -> Self {
        Complex::new(self.re, -self.im)
    }
    fn checked_sqrt_real_pos(&self, tol: f64) -> Option<Self> {
        // Cholesky pivots on Hermitian-positive-definite inputs are real positive.
        if self.re > tol && self.im.abs() < tol.max(1e-10) {
            Some(Complex::new(self.re.sqrt(), 0.0))
        } else {
            None
        }
    }
}

/// Lossless conversion from the rustlab native `C64` into a chosen scalar
/// type. The real-only path errors if the input has a non-trivial
/// imaginary component; the complex path is the identity.
pub trait FromComplex: SparseScalar + Sized {
    fn from_c64(c: C64, real_tol: f64) -> Result<Self, SparseSolveError>;
}

impl FromComplex for f64 {
    fn from_c64(c: C64, real_tol: f64) -> Result<Self, SparseSolveError> {
        if c.im.abs() > real_tol {
            Err(SparseSolveError::ComplexInRealPath { imag: c.im })
        } else {
            Ok(c.re)
        }
    }
}

impl FromComplex for C64 {
    fn from_c64(c: C64, _real_tol: f64) -> Result<Self, SparseSolveError> {
        Ok(c)
    }
}

/// Compressed Sparse Column storage. `col_ptr[j..j+1]` is the half-open
/// slice of `row_idx` and `values` belonging to column `j`. Within each
/// column, row indices are sorted ascending.
#[derive(Debug, Clone)]
pub struct SparseCsc<T: SparseScalar> {
    pub rows: usize,
    pub cols: usize,
    pub col_ptr: Vec<usize>,
    pub row_idx: Vec<usize>,
    pub values: Vec<T>,
}

impl<T: SparseScalar> SparseCsc<T> {
    pub fn nrows(&self) -> usize {
        self.rows
    }
    pub fn ncols(&self) -> usize {
        self.cols
    }
    pub fn nnz(&self) -> usize {
        self.values.len()
    }
    pub fn is_square(&self) -> bool {
        self.rows == self.cols
    }

    /// Build CSC from triplets (row, col, value) that are already sorted
    /// row-major (the format produced by `SparseMat::new`). Single-pass
    /// O(nnz). Within each column the row indices end up sorted because
    /// the input is sorted by `(row, col)` lexicographically.
    pub fn from_coo_sorted(rows: usize, cols: usize, coo: &[(usize, usize, T)]) -> Self {
        let mut col_ptr = vec![0usize; cols + 1];
        for &(_, c, _) in coo {
            col_ptr[c + 1] += 1;
        }
        for j in 0..cols {
            col_ptr[j + 1] += col_ptr[j];
        }

        let mut row_idx = vec![0usize; coo.len()];
        let mut values = vec![T::zero(); coo.len()];
        let mut next = col_ptr[..cols].to_vec();
        for &(r, c, v) in coo {
            let p = next[c];
            row_idx[p] = r;
            values[p] = v;
            next[c] = p + 1;
        }

        // Within each column, sort row indices ascending. The input is
        // row-major so this is already the case for typical inputs, but
        // we sort defensively to absorb any caller pathology.
        for j in 0..cols {
            let lo = col_ptr[j];
            let hi = col_ptr[j + 1];
            if hi - lo > 1 {
                let mut combined: Vec<(usize, T)> =
                    (lo..hi).map(|p| (row_idx[p], values[p])).collect();
                combined.sort_by_key(|&(r, _)| r);
                for (k, (r, v)) in combined.into_iter().enumerate() {
                    row_idx[lo + k] = r;
                    values[lo + k] = v;
                }
            }
        }

        SparseCsc {
            rows,
            cols,
            col_ptr,
            row_idx,
            values,
        }
    }

    /// Iterate `(row, value)` pairs of column `j` in ascending-row order.
    pub fn col_iter(&self, j: usize) -> impl Iterator<Item = (usize, T)> + '_ {
        let lo = self.col_ptr[j];
        let hi = self.col_ptr[j + 1];
        (lo..hi).map(move |p| (self.row_idx[p], self.values[p]))
    }

    /// Sparse matrix–dense vector product. O(nnz).
    pub fn spmv(&self, x: &[T]) -> Vec<T> {
        assert_eq!(
            self.cols,
            x.len(),
            "spmv: matrix is {}x{} but x has length {}",
            self.rows,
            self.cols,
            x.len()
        );
        let mut y = vec![T::zero(); self.rows];
        for j in 0..self.cols {
            let xj = x[j];
            for (r, v) in self.col_iter(j) {
                y[r] += v * xj;
            }
        }
        y
    }

    /// Non-conjugate transpose. Builds a fresh CSC by counting per-row
    /// non-zeros, then walking the original.
    pub fn transpose(&self) -> Self {
        let mut col_ptr = vec![0usize; self.rows + 1];
        for &r in &self.row_idx {
            col_ptr[r + 1] += 1;
        }
        for j in 0..self.rows {
            col_ptr[j + 1] += col_ptr[j];
        }

        let mut row_idx = vec![0usize; self.nnz()];
        let mut values = vec![T::zero(); self.nnz()];
        let mut next = col_ptr[..self.rows].to_vec();
        for c in 0..self.cols {
            for p in self.col_ptr[c]..self.col_ptr[c + 1] {
                let r = self.row_idx[p];
                let v = self.values[p];
                let q = next[r];
                row_idx[q] = c;
                values[q] = v;
                next[r] = q + 1;
            }
        }

        SparseCsc {
            rows: self.cols,
            cols: self.rows,
            col_ptr,
            row_idx,
            values,
        }
    }

    /// Look up A(i, j); returns `T::zero()` if the entry is structurally absent.
    pub fn get(&self, i: usize, j: usize) -> T {
        let lo = self.col_ptr[j];
        let hi = self.col_ptr[j + 1];
        match self.row_idx[lo..hi].binary_search(&i) {
            Ok(off) => self.values[lo + off],
            Err(_) => T::zero(),
        }
    }
}

/// Convert the COO `SparseMat` (always `C64`-valued) into CSC of the
/// requested scalar type. The real-only path enforces that all entries
/// have negligible imaginary component.
impl SparseMat {
    pub fn to_csc<T: FromComplex>(&self) -> Result<SparseCsc<T>, SparseSolveError> {
        const REAL_TOL: f64 = 1e-12;
        let mut converted: Vec<(usize, usize, T)> = Vec::with_capacity(self.entries.len());
        for &(r, c, v) in &self.entries {
            converted.push((r, c, T::from_c64(v, REAL_TOL)?));
        }
        Ok(SparseCsc::from_coo_sorted(self.rows, self.cols, &converted))
    }
}

//! Sparse direct solvers — hand-rolled, pure Rust.
//!
//! Module organization (per `dev/plans/sparse_solve_handroll.md`):
//!
//! - `csc.rs` — `SparseCsc<T>` storage type and `SparseScalar` trait that
//!   gives `f64` and `Complex<f64>` a uniform algebraic interface.
//! - `ordering.rs` — fill-reducing column orderings (`OrderingMethod`
//!   trait, `IdentityOrdering`, `ColCountOrdering`; `AmdOrdering`
//!   arrives in Phase 4).
//! - `elimination_tree.rs` — column elimination tree construction and
//!   post-order traversal (shared by Cholesky and LU).
//! - `cholesky.rs` — sparse Cholesky factorization for SPD matrices.
//! - `lu.rs` — sparse LU with partial pivoting (Phase 3).
//!
//! Algorithm references throughout are to Davis, *Direct Methods for
//! Sparse Linear Systems* (SIAM, 2006) — chapter 4 (Cholesky), chapter 6
//! (LU), chapter 7 (AMD ordering).

use thiserror::Error;

pub mod csc;
pub mod ordering;
pub mod elimination_tree;
pub mod cholesky;
pub mod lu;

pub use csc::{FromComplex, SparseCsc, SparseScalar};
pub use cholesky::{symbolic_col_counts, SparseChol};
pub use lu::SparseLU;
pub use ordering::{
    AmdOrdering, ColCountOrdering, IdentityOrdering, OrderingMethod, Permutation,
};

#[cfg(test)]
mod tests;

/// Errors returned by sparse-solver factorization and solve paths.
#[derive(Debug, Error)]
pub enum SparseSolveError {
    /// `A` and `b` shapes do not match.
    #[error("dimension mismatch: A is {a_rows}x{a_cols} but b has length {b_len}")]
    DimensionMismatch {
        a_rows: usize,
        a_cols: usize,
        b_len: usize,
    },

    /// `A` is not square — direct solve requires a square coefficient matrix.
    #[error("expected square matrix, got {rows}x{cols}")]
    NotSquare { rows: usize, cols: usize },

    /// Pivot dropped below the singularity threshold during factorization.
    #[error("matrix is singular at column {col} (pivot {pivot:.3e} below threshold {threshold:.3e})")]
    Singular {
        col: usize,
        pivot: f64,
        threshold: f64,
    },

    /// Cholesky was requested or auto-detected but the matrix is not Hermitian
    /// positive definite (a negative or non-real diagonal pivot was encountered).
    #[error("Cholesky path: matrix is not Hermitian positive definite (column {col})")]
    NotSpd { col: usize },

    /// A complex entry could not be promoted into a real-only solver path.
    #[error("entry has imaginary part {imag:.3e} which exceeds the real-only tolerance")]
    ComplexInRealPath { imag: f64 },

    /// An internal invariant was violated. Bug in the solver, not the input.
    #[error("internal sparse-solve error: {0}")]
    Internal(String),
}

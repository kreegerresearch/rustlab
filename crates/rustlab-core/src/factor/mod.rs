//! Dense QR (Householder), LU (partial pivoting), and Cholesky.
//!
//! These are the factorizations behind `qr`, dense `[L,U,P] = lu(A)`,
//! dense `chol`, and `\`. No column pivoting in the public `qr` itself;
//! least squares turns pivoting on so a rank-deficient system still has
//! a defined basic solution.

pub mod chol;
pub mod lu;
pub mod qr;

#[cfg(test)]
mod tests;

pub use lu::Lu;
pub use qr::Qr;

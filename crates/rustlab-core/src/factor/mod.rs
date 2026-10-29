//! Dense QR (Householder) and LU (partial pivoting).
//!
//! These are the factorizations behind `qr`, dense `[L,U,P] = lu(A)`,
//! and the rectangular branch of `\`. No column pivoting in `qr` itself;
//! least squares turns pivoting on so a rank-deficient system still has
//! a defined basic solution.

pub mod lu;
pub mod qr;

#[cfg(test)]
mod tests;

pub use lu::Lu;
pub use qr::Qr;

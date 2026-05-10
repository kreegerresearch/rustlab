//! Property-based tests for the sparse direct solvers.
//!
//! These tests generate random matrices and check invariants that
//! should hold for *every* input in their class:
//!
//! - **Sparse Cholesky:** for any SPD matrix `A` and right-hand side `b`,
//!   `||A·x − b||` should be < tol after `chol(A); solve(F, b)`.
//! - **Sparse LU:** for any non-singular matrix `A`, the same residual
//!   bound should hold via `lu(A); solve(F, b)`.
//! - **Symbolic counts:** for any SPD matrix, `symbolic_col_counts`
//!   should match the actual nnz per column of the numeric factor.
//!
//! Property tests catch corner cases that hand-written unit tests miss
//! — near-singular pivots, unusual sparsity patterns, scaling
//! pathologies, etc. Sizes are kept small (n ≤ 8) so the suite runs in
//! well under a second total.

use crate::sparse_solve::{
    cholesky::symbolic_col_counts, AmdOrdering, IdentityOrdering, SparseChol, SparseCsc, SparseLU,
};
use crate::types::C64;
use num_complex::Complex;
use proptest::prelude::*;

const RESIDUAL_TOL_F64: f64 = 1e-8;
const RESIDUAL_TOL_C64: f64 = 1e-7;

/// Generate a random SPD matrix as `A = M·M^T + n·I` so the diagonal
/// dominates and Cholesky is numerically robust.
fn arb_spd_real(n: usize) -> impl Strategy<Value = SparseCsc<f64>> {
    let entries = n * n;
    proptest::collection::vec(-2.0_f64..2.0_f64, entries).prop_map(move |raw| {
        // Build M from the raw entries.
        let mut m = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            for j in 0..n {
                m[i][j] = raw[i * n + j];
            }
        }
        // A = M · M^T (PSD by construction).
        let mut a = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            for j in 0..n {
                let mut s = 0.0_f64;
                for k in 0..n {
                    s += m[i][k] * m[j][k];
                }
                a[i][j] = s;
            }
        }
        // Add n·I to push it strictly positive definite and avoid the
        // singular boundary when M happens to be near-rank-deficient.
        for i in 0..n {
            a[i][i] += n as f64 * 4.0;
        }
        // Convert to CSC (column-major sorted COO triples).
        let mut coo: Vec<(usize, usize, f64)> = Vec::with_capacity(n * n);
        for i in 0..n {
            for j in 0..n {
                coo.push((i, j, a[i][j]));
            }
        }
        coo.sort_by(|p, q| p.0.cmp(&q.0).then(p.1.cmp(&q.1)));
        SparseCsc::<f64>::from_coo_sorted(n, n, &coo)
    })
}

/// Generate a random invertible matrix `A = I + ε·M` with small ε so
/// the matrix stays well-conditioned (LU with partial pivoting copes
/// with much worse, but for property tests we want stable residuals).
fn arb_invertible_real(n: usize) -> impl Strategy<Value = SparseCsc<f64>> {
    let entries = n * n;
    proptest::collection::vec(-1.0_f64..1.0_f64, entries).prop_map(move |raw| {
        let mut a = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            for j in 0..n {
                a[i][j] = 0.1 * raw[i * n + j];
            }
            a[i][i] += 1.0; // diagonal dominance
        }
        let mut coo: Vec<(usize, usize, f64)> = Vec::with_capacity(n * n);
        for i in 0..n {
            for j in 0..n {
                coo.push((i, j, a[i][j]));
            }
        }
        coo.sort_by(|p, q| p.0.cmp(&q.0).then(p.1.cmp(&q.1)));
        SparseCsc::<f64>::from_coo_sorted(n, n, &coo)
    })
}

/// Compute `||A·x − b||_∞`.
fn residual_inf(a: &SparseCsc<f64>, x: &[f64], b: &[f64]) -> f64 {
    let ax = a.spmv(x);
    ax.iter()
        .zip(b.iter())
        .map(|(ax_i, b_i)| (ax_i - b_i).abs())
        .fold(0.0_f64, f64::max)
}

fn residual_inf_c(a: &SparseCsc<C64>, x: &[C64], b: &[C64]) -> f64 {
    let ax = a.spmv(x);
    ax.iter()
        .zip(b.iter())
        .map(|(ax_i, b_i)| (ax_i - b_i).norm())
        .fold(0.0_f64, f64::max)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    })]

    /// Cholesky on random SPD: `||A·x − b|| < tol` for any SPD A, any b.
    /// Both Identity and AMD orderings.
    #[test]
    fn cholesky_residual_bounded_real_identity(
        a in (3usize..=8).prop_flat_map(arb_spd_real),
        seed in any::<u64>(),
    ) {
        let n = a.nrows();
        let b = rng_vec(seed, n);
        let chol = SparseChol::factor(&a, &IdentityOrdering)
            .expect("SPD construction should be well-conditioned");
        let x = chol.solve(&b).expect("dim ok");
        let res = residual_inf(&a, &x, &b);
        prop_assert!(
            res < RESIDUAL_TOL_F64,
            "residual {res} exceeded {RESIDUAL_TOL_F64} (n={n})"
        );
    }

    #[test]
    fn cholesky_residual_bounded_real_amd(
        a in (3usize..=8).prop_flat_map(arb_spd_real),
        seed in any::<u64>(),
    ) {
        let n = a.nrows();
        let b = rng_vec(seed, n);
        let chol = SparseChol::factor(&a, &AmdOrdering).expect("SPD");
        let x = chol.solve(&b).expect("dim ok");
        let res = residual_inf(&a, &x, &b);
        prop_assert!(res < RESIDUAL_TOL_F64, "residual {res} exceeded tol (n={n})");
    }

    /// Symbolic counts must match the numeric factor's per-column nnz
    /// for any SPD input. The in-factor debug_assert covers this in
    /// debug builds; the property test enforces it in release.
    #[test]
    fn cholesky_symbolic_matches_numeric(
        a in (3usize..=8).prop_flat_map(arb_spd_real),
    ) {
        let n = a.nrows();
        let pred_id = symbolic_col_counts(&a, &IdentityOrdering).unwrap();
        let chol = SparseChol::factor(&a, &IdentityOrdering).unwrap();
        let l = chol.factor_csc();
        let actual: Vec<usize> = (0..n)
            .map(|j| l.col_ptr[j + 1] - l.col_ptr[j])
            .collect();
        prop_assert_eq!(pred_id, actual, "identity ordering counts disagree");

        let pred_amd = symbolic_col_counts(&a, &AmdOrdering).unwrap();
        let chol_amd = SparseChol::factor(&a, &AmdOrdering).unwrap();
        let l_amd = chol_amd.factor_csc();
        let actual_amd: Vec<usize> = (0..n)
            .map(|j| l_amd.col_ptr[j + 1] - l_amd.col_ptr[j])
            .collect();
        prop_assert_eq!(pred_amd, actual_amd, "AMD ordering counts disagree");
    }

    /// LU with partial pivoting: residual bound for any well-conditioned
    /// non-singular matrix.
    #[test]
    fn lu_residual_bounded_real(
        a in (3usize..=8).prop_flat_map(arb_invertible_real),
        seed in any::<u64>(),
    ) {
        let n = a.nrows();
        let b = rng_vec(seed, n);
        let lu = SparseLU::factor(&a, &AmdOrdering, 0.1).expect("invertible");
        let x = lu.solve(&b).expect("dim ok");
        let res = residual_inf(&a, &x, &b);
        prop_assert!(res < RESIDUAL_TOL_F64, "lu residual {res} > tol (n={n})");
    }

    /// Cholesky and LU should agree on SPD inputs to within combined
    /// tolerance — both should produce the same answer (subject to FP
    /// accumulation order).
    #[test]
    fn cholesky_and_lu_agree_on_spd(
        a in (3usize..=6).prop_flat_map(arb_spd_real),
        seed in any::<u64>(),
    ) {
        let n = a.nrows();
        let b = rng_vec(seed, n);
        let x_chol = SparseChol::factor(&a, &AmdOrdering).unwrap().solve(&b).unwrap();
        let x_lu = SparseLU::factor(&a, &AmdOrdering, 0.1).unwrap().solve(&b).unwrap();
        let max_diff = x_chol
            .iter()
            .zip(x_lu.iter())
            .map(|(a_i, b_i)| (a_i - b_i).abs())
            .fold(0.0_f64, f64::max);
        prop_assert!(
            max_diff < 1e-6,
            "chol vs lu disagree by {max_diff} (n={n})"
        );
    }

    /// Complex SPD (Hermitian + diag-dominant) round-trip.
    #[test]
    fn cholesky_residual_bounded_complex(
        seed in any::<u64>(),
        n in 3usize..=6,
    ) {
        let a = build_hermitian_spd(seed, n);
        let b = rng_vec_c(seed.wrapping_add(1), n);
        let chol = SparseChol::factor(&a, &AmdOrdering).expect("Hermitian SPD");
        let x = chol.solve(&b).expect("dim ok");
        let res = residual_inf_c(&a, &x, &b);
        prop_assert!(
            res < RESIDUAL_TOL_C64,
            "complex chol residual {res} > tol (n={n})"
        );
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Deterministic random Vec<f64> from a seed. Avoids depending on
/// proptest's float generation for the RHS, which can produce NaN /
/// inf and wreck the residual check.
fn rng_vec(seed: u64, n: usize) -> Vec<f64> {
    let mut state = seed.wrapping_add(1);
    (0..n)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as f64) / (u32::MAX as f64) - 0.5
        })
        .collect()
}

fn rng_vec_c(seed: u64, n: usize) -> Vec<C64> {
    let re = rng_vec(seed, n);
    let im = rng_vec(seed.wrapping_add(0xFFFF_FFFF), n);
    re.into_iter()
        .zip(im.into_iter())
        .map(|(r, i)| Complex::new(r, i))
        .collect()
}

/// Build a Hermitian positive-definite n×n complex matrix `A = M·M^H + n·I`.
fn build_hermitian_spd(seed: u64, n: usize) -> SparseCsc<C64> {
    let mut state = seed.wrapping_add(1);
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 33) as f64) / (u32::MAX as f64) - 0.5
    };
    let mut m = vec![vec![Complex::new(0.0, 0.0); n]; n];
    for i in 0..n {
        for j in 0..n {
            m[i][j] = Complex::new(next(), next());
        }
    }
    let mut a = vec![vec![Complex::new(0.0, 0.0); n]; n];
    for i in 0..n {
        for j in 0..n {
            let mut s = Complex::new(0.0, 0.0);
            for k in 0..n {
                s += m[i][k] * m[j][k].conj();
            }
            a[i][j] = s;
        }
    }
    for i in 0..n {
        a[i][i] += Complex::new(n as f64 * 4.0, 0.0);
    }
    let mut coo: Vec<(usize, usize, C64)> = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            coo.push((i, j, a[i][j]));
        }
    }
    coo.sort_by(|p, q| p.0.cmp(&q.0).then(p.1.cmp(&q.1)));
    SparseCsc::<C64>::from_coo_sorted(n, n, &coo)
}

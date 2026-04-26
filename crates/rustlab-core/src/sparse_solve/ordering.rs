//! Fill-reducing column orderings for sparse direct solvers.
//!
//! Without a fill-reducing ordering, factorizing a sparse matrix produces
//! catastrophic fill-in: a 100x100 Laplacian's Cholesky factor goes from
//! O(N) entries to O(N^1.5). The factorization code consumes a
//! `Permutation` produced by an `OrderingMethod`.
//!
//! Phase 2 (this file at first commit) ships `IdentityOrdering` (no-op,
//! for testing) and `ColCountOrdering` (sort columns by initial nnz,
//! ascending — a simple but functional heuristic). Phase 4 adds
//! `AmdOrdering` and makes it the default.

use crate::sparse_solve::csc::{SparseCsc, SparseScalar};

/// A square permutation of `n` indices. `perm[new_position] = old_index`.
/// `inv_perm[old_index] = new_position`.
#[derive(Debug, Clone)]
pub struct Permutation {
    perm: Vec<usize>,
    inv_perm: Vec<usize>,
}

impl Permutation {
    /// Build a permutation from a slice where `perm[i] = old_index`.
    /// Validates that every index in `0..n` appears exactly once.
    pub fn from_perm(perm: Vec<usize>) -> Self {
        let n = perm.len();
        let mut inv_perm = vec![usize::MAX; n];
        for (new_pos, &old) in perm.iter().enumerate() {
            assert!(old < n, "permutation entry {old} out of range 0..{n}");
            assert!(
                inv_perm[old] == usize::MAX,
                "permutation contains duplicate index {old}"
            );
            inv_perm[old] = new_pos;
        }
        Permutation { perm, inv_perm }
    }

    pub fn identity(n: usize) -> Self {
        let perm: Vec<usize> = (0..n).collect();
        Self::from_perm(perm)
    }

    pub fn len(&self) -> usize {
        self.perm.len()
    }

    pub fn is_empty(&self) -> bool {
        self.perm.is_empty()
    }

    /// `perm[new_position] -> old_index`.
    pub fn perm(&self) -> &[usize] {
        &self.perm
    }

    /// `inv_perm[old_index] -> new_position`.
    pub fn inv_perm(&self) -> &[usize] {
        &self.inv_perm
    }

    /// Apply the permutation to a vector in the new ordering: returns a
    /// vector `y` such that `y[new_pos] = x[old_index]` for each
    /// `new_pos`. Used to permute a right-hand side before a permuted solve.
    pub fn permute_vec<T: Copy>(&self, x: &[T]) -> Vec<T> {
        debug_assert_eq!(x.len(), self.perm.len());
        let mut y = Vec::with_capacity(x.len());
        for &old in &self.perm {
            y.push(x[old]);
        }
        y
    }

    /// Inverse permute a vector: returns `x` such that
    /// `x[old_index] = y[new_pos]`. Used to unpermute a solve result.
    pub fn unpermute_vec<T: Copy>(&self, y: &[T]) -> Vec<T> {
        debug_assert_eq!(y.len(), self.inv_perm.len());
        let mut x = Vec::with_capacity(y.len());
        for &new_pos in &self.inv_perm {
            x.push(y[new_pos]);
        }
        x
    }

    /// Apply a symmetric permutation `P A P^T` to a CSC matrix.
    pub fn permute_symmetric<T: SparseScalar>(&self, a: &SparseCsc<T>) -> SparseCsc<T> {
        assert!(a.is_square(), "symmetric permutation requires square matrix");
        assert_eq!(a.nrows(), self.perm.len());

        let n = a.nrows();
        let mut coo: Vec<(usize, usize, T)> = Vec::with_capacity(a.nnz());
        for new_col in 0..n {
            let old_col = self.perm[new_col];
            for (old_row, v) in a.col_iter(old_col) {
                let new_row = self.inv_perm[old_row];
                coo.push((new_row, new_col, v));
            }
        }
        // CSC builder needs row-major sort for its single-pass conversion.
        coo.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.cmp(&y.1)));
        SparseCsc::from_coo_sorted(n, n, &coo)
    }
}

/// A method that produces a column permutation for a sparse matrix.
/// Operates on the symbolic structure only — the values don't matter.
pub trait OrderingMethod {
    fn order<T: SparseScalar>(&self, a: &SparseCsc<T>) -> Permutation;
}

/// No-op ordering. Useful for testing the factorization without
/// permutations interfering.
pub struct IdentityOrdering;

impl OrderingMethod for IdentityOrdering {
    fn order<T: SparseScalar>(&self, a: &SparseCsc<T>) -> Permutation {
        Permutation::identity(a.ncols())
    }
}

/// Sort columns by initial non-zero count, ascending. Simple but
/// effective: columns with fewer entries get eliminated first, which
/// reduces fill in many practical patterns including banded and
/// star-shaped graphs. Roughly 3x worse than AMD on Laplacian assemblies
/// but still a substantial improvement over no ordering at all.
pub struct ColCountOrdering;

impl OrderingMethod for ColCountOrdering {
    fn order<T: SparseScalar>(&self, a: &SparseCsc<T>) -> Permutation {
        let n = a.ncols();
        let mut order: Vec<(usize, usize)> = (0..n)
            .map(|j| (a.col_ptr[j + 1] - a.col_ptr[j], j))
            .collect();
        // Stable sort: original index is the tiebreaker, preserving locality.
        order.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.cmp(&y.1)));
        let perm: Vec<usize> = order.into_iter().map(|(_, j)| j).collect();
        Permutation::from_perm(perm)
    }
}

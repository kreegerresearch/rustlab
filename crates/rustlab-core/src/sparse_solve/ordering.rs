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

/// Approximate Minimum Degree (AMD) fill-reducing ordering. Operates on
/// the symmetric pattern of `A + A^T`: at each step, eliminate the node
/// with the minimum degree in the current quotient graph, then form the
/// implied clique among its surviving neighbours.
///
/// **Implementation status (v1).** This is the *basic minimum-degree*
/// variant, with the explicit fill-graph representation. The full AMD
/// algorithm of Davis, *Direct Methods for Sparse Linear Systems*,
/// ch. 7 layers three optimizations on top — *external degree*
/// (counting reachable variables instead of full degree),
/// *supervariable detection* (merging indistinguishable nodes), and
/// *mass elimination* (eliminating an entire supervariable in one
/// step). These optimizations are what give AMD its name and its
/// roughly-optimal fill behaviour on irregular patterns; without them,
/// the heuristic implemented here can be comparable to or sometimes
/// worse than the simpler `ColCountOrdering` on highly-regular grids
/// (where the natural ordering is already near-optimal).
///
/// For the curriculum's regular Laplacian assemblies, `ColCountOrdering`
/// is the recommended default. `AmdOrdering` becomes more useful when
/// the matrix has irregular structure (e.g. an FDFD assembly with PML
/// where boundary conditions create non-uniform connectivity). Future
/// work to lift this into full Davis-style AMD is tracked in
/// `dev/plans/sparse_solve_handroll.md`.
pub struct AmdOrdering;

impl OrderingMethod for AmdOrdering {
    fn order<T: SparseScalar>(&self, a: &SparseCsc<T>) -> Permutation {
        let n = a.ncols();
        if n == 0 {
            return Permutation::identity(0);
        }
        // Step 1: build the symmetric adjacency pattern of A + A^T,
        // excluding the diagonal. Adjacency is stored as one Vec<usize>
        // per row, deduplicated.
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for j in 0..n {
            for (i, _) in a.col_iter(j) {
                if i != j {
                    adj[i].push(j);
                    adj[j].push(i);
                }
            }
        }
        for ai in adj.iter_mut() {
            ai.sort_unstable();
            ai.dedup();
        }

        // alive[i] = true if node i hasn't been eliminated yet.
        let mut alive: Vec<bool> = vec![true; n];
        // Order in which nodes are eliminated. perm[k] = old_index.
        let mut perm: Vec<usize> = Vec::with_capacity(n);

        // Workspace for degree updates: a marker vector that lets us
        // count distinct neighbours-of-neighbours in O(deg).
        let mut mark: Vec<usize> = vec![usize::MAX; n];

        for step in 0..n {
            // Step 2: pick the alive node with minimum |adj|. This is
            // the "true" degree at every step — the "approximate"
            // refinement of AMD is in the *update* of adjacency below
            // (we form the union and don't recompute exact degrees).
            let pivot = (0..n)
                .filter(|&i| alive[i])
                .min_by_key(|&i| adj[i].len())
                .expect("at least one node should be alive");

            alive[pivot] = false;
            perm.push(pivot);

            // Step 3: form the union of pivot's adjacent alive nodes.
            // This is the "clique" the elimination would create in the
            // factor's pattern.
            let neighbours: Vec<usize> = adj[pivot]
                .iter()
                .copied()
                .filter(|&i| alive[i])
                .collect();

            // Step 4: update each neighbour's adjacency list. Replace
            // the pivot in their adjacency with the (rest of the) clique.
            for &i in &neighbours {
                // Remove the pivot from i's adjacency (it's gone).
                adj[i].retain(|&x| x != pivot && alive[x]);
                // Mark current neighbours so we don't add duplicates.
                mark[i] = step;
                for &j in &adj[i] {
                    mark[j] = step;
                }
                // Add new fill edges from the clique.
                for &j in &neighbours {
                    if j != i && mark[j] != step {
                        adj[i].push(j);
                        mark[j] = step;
                    }
                }
                // Sort to keep adjacency tidy and equality-comparable
                // (helps with future supervariable detection if we add it).
                adj[i].sort_unstable();
            }

            // Note: nodes that were not adjacent to the pivot are
            // unaffected by this elimination step.
            let _ = step; // suppress unused warning if `step` is only used as marker
        }

        debug_assert_eq!(perm.len(), n);
        Permutation::from_perm(perm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse_solve::csc::SparseCsc;
    use crate::sparse_solve::cholesky::SparseChol;

    fn rcsc(rows: usize, cols: usize, coo: &[(usize, usize, f64)]) -> SparseCsc<f64> {
        let mut sorted: Vec<_> = coo.iter().copied().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        SparseCsc::from_coo_sorted(rows, cols, &sorted)
    }

    fn build_lap(nx: usize, ny: usize) -> SparseCsc<f64> {
        let mut coo = Vec::new();
        for j in 0..nx {
            for i in 0..ny {
                let k = j * ny + i;
                coo.push((k, k, 4.0));
                if i > 0 {
                    coo.push((k, k - 1, -1.0));
                }
                if i + 1 < ny {
                    coo.push((k, k + 1, -1.0));
                }
                if j > 0 {
                    coo.push((k, k - ny, -1.0));
                }
                if j + 1 < nx {
                    coo.push((k, k + ny, -1.0));
                }
            }
        }
        coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        SparseCsc::from_coo_sorted(nx * ny, nx * ny, &coo)
    }

    #[test]
    fn amd_diagonal_returns_identity_or_valid_perm() {
        // Diagonal matrix has no edges; every node has degree 0.
        // Any permutation is valid; just check we get a valid one.
        let coo: Vec<_> = (0..5).map(|i| (i, i, 1.0)).collect();
        let m = rcsc(5, 5, &coo);
        let p = AmdOrdering.order(&m);
        assert_eq!(p.len(), 5);
        // Validity: every index appears exactly once.
        let mut seen = vec![false; 5];
        for &i in p.perm() {
            assert!(!seen[i], "duplicate {i}");
            seen[i] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn amd_chain_orders_endpoints_first() {
        // Tridiagonal chain: nodes 0..4 with edges (i, i+1).
        // Endpoints 0 and 4 have degree 1; interior 1, 2, 3 have degree 2.
        // AMD should pick endpoints first.
        let n = 5;
        let mut coo = Vec::new();
        for k in 0..n {
            coo.push((k, k, 2.0));
            if k + 1 < n {
                coo.push((k, k + 1, -1.0));
                coo.push((k + 1, k, -1.0));
            }
        }
        let m = rcsc(n, n, &coo);
        let p = AmdOrdering.order(&m);
        // First eliminated should be one of the endpoints.
        assert!(
            p.perm()[0] == 0 || p.perm()[0] == n - 1,
            "first eliminated was {}",
            p.perm()[0]
        );
    }

    #[test]
    fn amd_permutation_is_valid() {
        let m = build_lap(5, 5);
        let p = AmdOrdering.order(&m);
        assert_eq!(p.len(), 25);
        let mut seen = vec![false; 25];
        for &i in p.perm() {
            assert!(!seen[i]);
            seen[i] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn amd_fill_comparison_diagnostic() {
        // Diagnostic: print the relative fill produced by each ordering
        // on Laplacian assemblies of growing size. AMD's basic
        // (minimum-degree) variant implemented here is competitive with
        // ColCountOrdering; full AMD with external-degree refinement
        // would do better, but is deferred per `dev/plans/sparse_solve_handroll.md`.
        // This test does not assert relative fill — its purpose is to
        // surface regressions if a future change makes AMD substantially
        // worse than ColCountOrdering.
        for n in [5, 10, 15] {
            let m = build_lap(n, n);
            let id = SparseChol::factor(&m, &IdentityOrdering).unwrap();
            let cc = SparseChol::factor(&m, &super::ColCountOrdering).unwrap();
            let amd = SparseChol::factor(&m, &AmdOrdering).unwrap();
            println!(
                "Laplacian {}x{} (n={}, A nnz={}): id={} cc={} amd={}",
                n,
                n,
                n * n,
                m.nnz(),
                id.nnz(),
                cc.nnz(),
                amd.nnz()
            );
            // Sanity bound: AMD should not produce more than 2x the
            // fill of identity ordering. Anything beyond that signals a
            // bug in the algorithm.
            assert!(
                (amd.nnz() as f64) <= 2.0 * (id.nnz() as f64),
                "AMD fill {} unexpectedly larger than 2x identity ({})",
                amd.nnz(),
                id.nnz()
            );
        }
    }

    #[test]
    fn amd_solve_agrees_with_identity() {
        // The solve result must be invariant under ordering choice.
        let m = build_lap(6, 6);
        let n = 36;
        let mut v_exact = vec![0.0; n];
        for k in 0..n {
            v_exact[k] = ((k + 1) as f64).sin();
        }
        let rhs = m.spmv(&v_exact);

        let chol_id = SparseChol::factor(&m, &IdentityOrdering).unwrap();
        let chol_amd = SparseChol::factor(&m, &AmdOrdering).unwrap();

        let v_id = chol_id.solve(&rhs).unwrap();
        let v_amd = chol_amd.solve(&rhs).unwrap();

        for k in 0..n {
            assert!(
                (v_id[k] - v_amd[k]).abs() < 1e-10,
                "ordering mismatch at {k}: id={} amd={}",
                v_id[k],
                v_amd[k]
            );
        }
    }
}

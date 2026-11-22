//! Column elimination tree and post-order traversal — symbolic
//! infrastructure shared by sparse Cholesky and sparse LU.
//!
//! Algorithm reference: Davis, *Direct Methods for Sparse Linear Systems*,
//! ch. 4 (`cs_etree` and `cs_post` in CSparse). The elimination tree
//! describes which columns of a sparse factor depend on which: the
//! parent of column `k` is the row index `i > k` of the first non-zero
//! in column `k` of the lower-triangular factor `L`.
//!
//! For Cholesky on a symmetric A we treat A as symmetric and use the
//! upper-triangular entries (column k, rows i < k) to grow the tree.

use crate::sparse_solve::csc::{SparseCsc, SparseScalar};

/// Compute the column elimination tree of a square sparse matrix. The
/// returned vector has length `a.ncols()`; entry `parent[k]` is the
/// parent of column `k` in the elimination tree, or `None` if `k` is a
/// root. The construction uses path compression and is nearly linear in
/// `nnz(A)`.
pub fn column_elimination_tree<T: SparseScalar>(a: &SparseCsc<T>) -> Vec<Option<usize>> {
    debug_assert!(a.is_square(), "elimination tree requires square matrix");
    let n = a.ncols();
    let mut parent: Vec<Option<usize>> = vec![None; n];
    let mut ancestor: Vec<Option<usize>> = vec![None; n];

    for k in 0..n {
        // ancestor[k] starts at None (no ancestor for the new node yet).
        // It is left at None below — initialization is implicit.
        for (i_orig, _) in a.col_iter(k) {
            // Walk strictly-upper entries of column k toward the partial root.
            let mut i_opt = if i_orig < k { Some(i_orig) } else { None };
            while let Some(i) = i_opt {
                if i >= k {
                    break;
                }
                let next = ancestor[i];
                ancestor[i] = Some(k); // path compression
                if next.is_none() {
                    parent[i] = Some(k);
                    break;
                }
                i_opt = next;
            }
        }
    }

    parent
}

/// Post-order traversal of the elimination tree. Returns a vector `post`
/// of length `n` such that visiting `post[0], post[1], ...` walks the
/// tree in post-order — children before their parent, with siblings in
/// the order they appear in the parent's child list.
///
/// Used by symbolic Cholesky to compute the row counts of the factor.
pub fn post_order(parent: &[Option<usize>]) -> Vec<usize> {
    let n = parent.len();

    // Build linked-list-of-children: head[p] is the first child of p,
    // next[c] is the next sibling of c. We walk the parent array in
    // reverse so children end up in ascending order in the lists.
    let mut head: Vec<Option<usize>> = vec![None; n];
    let mut next: Vec<Option<usize>> = vec![None; n];
    for j in (0..n).rev() {
        if let Some(p) = parent[j] {
            next[j] = head[p];
            head[p] = Some(j);
        }
    }

    let mut post: Vec<usize> = Vec::with_capacity(n);
    let mut stack: Vec<usize> = Vec::with_capacity(n);

    for j in 0..n {
        if parent[j].is_some() {
            continue; // not a root
        }
        // Iterative DFS rooted at j.
        stack.push(j);
        while let Some(&top) = stack.last() {
            if let Some(child) = head[top] {
                head[top] = next[child];
                stack.push(child);
            } else {
                post.push(top);
                stack.pop();
            }
        }
    }

    post
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse_solve::csc::SparseCsc;

    /// Helper: build CSC from a list of (row, col, value) triplets.
    fn csc_from(rows: usize, cols: usize, coo: &[(usize, usize, f64)]) -> SparseCsc<f64> {
        let mut sorted: Vec<_> = coo.iter().copied().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        SparseCsc::from_coo_sorted(rows, cols, &sorted)
    }

    #[test]
    fn etree_diagonal_has_no_parents() {
        // Diagonal matrix → every column is its own root.
        let m = csc_from(
            5,
            5,
            &[
                (0, 0, 1.0),
                (1, 1, 1.0),
                (2, 2, 1.0),
                (3, 3, 1.0),
                (4, 4, 1.0),
            ],
        );
        let parent = column_elimination_tree(&m);
        for p in parent {
            assert!(p.is_none());
        }
    }

    #[test]
    fn etree_chain() {
        // Tridiagonal: each column k has an entry at row k-1 (and k, k+1).
        // Elimination tree is a chain: parent[k] = k+1 for k = 0..n-1.
        let n = 5;
        let mut coo = Vec::new();
        for k in 0..n {
            coo.push((k, k, 2.0));
            if k + 1 < n {
                coo.push((k, k + 1, -1.0));
                coo.push((k + 1, k, -1.0));
            }
        }
        let m = csc_from(n, n, &coo);
        let parent = column_elimination_tree(&m);
        for k in 0..n - 1 {
            assert_eq!(parent[k], Some(k + 1), "parent[{k}] should be {}", k + 1);
        }
        assert_eq!(parent[n - 1], None);
    }

    #[test]
    fn post_order_chain() {
        // Chain elimination tree: 0 → 1 → 2 → 3
        let parent: Vec<Option<usize>> = vec![Some(1), Some(2), Some(3), None];
        let post = post_order(&parent);
        assert_eq!(post, vec![0, 1, 2, 3]);
    }

    #[test]
    fn post_order_branching_tree() {
        // Tree:    3
        //         /|
        //        1 2
        //        |
        //        0
        let parent: Vec<Option<usize>> = vec![Some(1), Some(3), Some(3), None];
        let post = post_order(&parent);
        // post-order should list 0 then 1 (its parent), then 2, then 3 (root).
        assert_eq!(post, vec![0, 1, 2, 3]);
    }

    #[test]
    fn post_order_forest() {
        // Two disjoint chains: 0→1, 2→3
        let parent: Vec<Option<usize>> = vec![Some(1), None, Some(3), None];
        let post = post_order(&parent);
        // Both chains must appear, children before parents.
        assert_eq!(post.len(), 4);
        let pos: Vec<_> = (0..4).map(|k| post.iter().position(|&x| x == k).unwrap()).collect();
        assert!(pos[0] < pos[1], "0 before its parent 1");
        assert!(pos[2] < pos[3], "2 before its parent 3");
    }
}

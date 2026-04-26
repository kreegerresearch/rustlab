//! Sparse LU factorization with partial pivoting.
//!
//! Gilbert-Peierls algorithm following Davis, *Direct Methods for Sparse
//! Linear Systems*, ch. 6 (`cs_lu`). For a square non-singular `A`,
//! factor `P A Q = L U` where:
//! - `Q` is the column permutation supplied by the caller's
//!   `OrderingMethod` (chosen for fill reduction).
//! - `P` is the row permutation chosen at numeric time for stability.
//! - `L` is unit lower triangular (`L(k, k) = 1` after pivoting).
//! - `U` is upper triangular.
//!
//! For each column `k` we:
//! 1. Symbolic step: find the row pattern of `L(:, k) ∪ U(:, k)` via DFS
//!    on the lower-triangular pattern of L assembled so far, using the
//!    `pinv` permutation to interpret which rows have already been
//!    pivoted into the upper triangle.
//! 2. Triangular solve `L · x = a_perm` for the values, where `a_perm`
//!    is the chosen column of A in the column-permuted ordering.
//! 3. Pivot search in the unfactored rows of `x` (those with
//!    `pinv[row] < 0`). The largest-magnitude unfactored entry becomes
//!    the new pivot, with a tolerance multiplier that prefers diagonal
//!    pivots when they are competitive.
//! 4. Record `U(:, k)` (the already-factored entries plus the pivot)
//!    and `L(:, k)` (the remaining entries divided by the pivot).

use crate::sparse_solve::csc::{SparseCsc, SparseScalar};
use crate::sparse_solve::ordering::{OrderingMethod, Permutation};
use crate::sparse_solve::SparseSolveError;

/// Sparse LU factor of a square matrix.
#[derive(Debug)]
pub struct SparseLU<T: SparseScalar> {
    /// Unit lower-triangular factor in CSC. Diagonal is implicit (== 1)
    /// and is NOT stored — column j contains only the strict-lower
    /// entries `L(i, j)` with `i > j`.
    l: SparseCsc<T>,
    /// Upper-triangular factor in CSC. Diagonal is the first entry of
    /// each column; below-diagonal entries do not exist.
    u: SparseCsc<T>,
    /// Row permutation chosen during numeric factorization. `p[new] = old`.
    p: Permutation,
    /// Column permutation supplied by the ordering method. `q[new] = old`.
    q: Permutation,
    /// Cached size for shape checks.
    n: usize,
}

impl<T: SparseScalar> SparseLU<T> {
    /// Factor a square sparse matrix `A`. The `tol` parameter controls
    /// partial pivoting: a non-diagonal candidate is preferred only when
    /// it is at least `tol *` the largest unpivoted magnitude. A value
    /// of `1.0` is full partial pivoting (always pick the largest);
    /// smaller values like `0.1` allow more diagonal pivots, reducing
    /// fill-in at a small stability cost. The standard choice is `0.1`.
    pub fn factor<O: OrderingMethod>(
        a: &SparseCsc<T>,
        ord: &O,
        tol: f64,
    ) -> Result<Self, SparseSolveError> {
        if !a.is_square() {
            return Err(SparseSolveError::NotSquare {
                rows: a.nrows(),
                cols: a.ncols(),
            });
        }
        if !(0.0..=1.0).contains(&tol) {
            return Err(SparseSolveError::Internal(format!(
                "LU pivoting tolerance must be in [0, 1], got {tol}"
            )));
        }
        let n = a.nrows();
        let q = ord.order(a);

        // pinv[orig_row] = new_row chosen as pivot for column new_row,
        // or usize::MAX if not yet chosen.
        let mut pinv: Vec<usize> = vec![usize::MAX; n];

        // L and U accumulators, column by column.
        let mut l_cols: Vec<Vec<(usize, T)>> = vec![Vec::new(); n];
        let mut u_cols: Vec<Vec<(usize, T)>> = vec![Vec::new(); n];

        // Sparse-accumulator workspace.
        let mut x: Vec<T> = vec![T::zero(); n];
        // Pattern stack for DFS reach. xi[top..n] holds the topo-ordered pattern.
        let mut xi: Vec<usize> = vec![0usize; n];
        // DFS auxiliary: marker per node for visited / on-stack states.
        let mut marker: Vec<usize> = vec![usize::MAX; n];
        // Per-node iterator state for iterative DFS.
        let mut next_p: Vec<usize> = vec![0usize; n];
        let mut path_top: Vec<usize> = vec![0usize; n];

        for k in 0..n {
            let col = q.perm()[k];

            // ---- 1. Symbolic phase: find the reachable pattern of
            // L(:, k) ∪ U(:, k) via DFS on the assembled L. ----
            let top = lu_reach(
                &l_cols, a, col, &pinv, &mut xi, &mut marker, &mut next_p, &mut path_top, k,
            );

            // Clear x at the touched indices.
            for p in top..n {
                x[xi[p]] = T::zero();
            }
            // Scatter A(:, col) into x.
            for (i, v) in a.col_iter(col) {
                x[i] = v;
            }

            // ---- 2. Triangular solve L · x = a_perm via the topo-ordered pattern. ----
            // For each j in xi[top..n] (topo order, leaves first), if row j
            // has already been pivoted into some column J, eliminate via
            // L's column J: x[i] -= L(i, J) * x[j] for each below-diagonal
            // entry (i, L(i, J)).
            for p in top..n {
                let j = xi[p];
                let big_j = pinv[j];
                if big_j == usize::MAX {
                    continue; // not yet pivoted — skip elimination
                }
                let xj = x[j];
                for &(row, l_row_j_val) in &l_cols[big_j] {
                    x[row] -= l_row_j_val * xj;
                }
            }

            // ---- 3. Pivot search in unfactored rows. ----
            // Walk the pattern; entries already pivoted go into U(:, k),
            // entries not yet pivoted are pivot candidates. Pick the row
            // with the largest |x[row]| as the new pivot, with a
            // diagonal-preference rule.
            let mut ipiv: Option<usize> = None;
            let mut best_mag: f64 = -1.0;
            let mut diag_mag: f64 = -1.0;

            for p in top..n {
                let i = xi[p];
                if pinv[i] != usize::MAX {
                    // already pivoted -> this is a U(:, k) entry
                    let new_row = pinv[i];
                    u_cols[k].push((new_row, x[i]));
                } else {
                    let mag = x[i].abs();
                    if mag > best_mag {
                        best_mag = mag;
                        ipiv = Some(i);
                    }
                    if i == col {
                        diag_mag = mag;
                    }
                }
            }

            // If the diagonal candidate is at least `tol *` the best,
            // prefer the diagonal (better fill behavior).
            if diag_mag >= 0.0 && diag_mag >= tol * best_mag {
                ipiv = Some(col);
            }

            let ipiv = match ipiv {
                Some(i) => i,
                None => {
                    return Err(SparseSolveError::Singular {
                        col: k,
                        pivot: 0.0,
                        threshold: 1e-14,
                    });
                }
            };

            let pivot = x[ipiv];
            if pivot.abs() < 1e-14 {
                return Err(SparseSolveError::Singular {
                    col: k,
                    pivot: pivot.abs(),
                    threshold: 1e-14,
                });
            }

            // Record the diagonal of U.
            u_cols[k].push((k, pivot));
            // Sort U column entries by row (the symbolic walk emits them
            // in topo order, not row order).
            u_cols[k].sort_by_key(|&(r, _)| r);
            pinv[ipiv] = k;

            // ---- 4. Record L(:, k) — entries below the pivot, scaled. ----
            // Walk the pattern again for the unpivoted rows (everything
            // we didn't push to U above except the new pivot itself).
            for p in top..n {
                let i = xi[p];
                if i == ipiv {
                    continue; // pivot is L(k, k) = 1, not stored
                }
                if pinv[i] == usize::MAX {
                    // not pivoted (yet) AND not the chosen pivot —
                    // BUT wait, we set pinv[ipiv] above, so if pinv[i] is
                    // still MAX it means i is genuinely an L entry.
                    // ... however we just set pinv[ipiv] = k, so checking
                    // pinv[i] == MAX excludes the pivot already. Good.
                    let val = x[i] / pivot;
                    l_cols[k].push((i, val));
                }
            }
            // Sort L column entries by ORIGINAL row (we'll remap to new
            // row indices via pinv after the whole factorization completes).
            l_cols[k].sort_by_key(|&(r, _)| r);
        }

        // ---- 5. Build the row permutation P from pinv. ----
        // pinv[old_row] = new_row → invert to get permutation new -> old.
        let mut p_new_to_old: Vec<usize> = vec![usize::MAX; n];
        for (old, &new) in pinv.iter().enumerate() {
            if new == usize::MAX {
                return Err(SparseSolveError::Internal(
                    "LU factorization left a row unpivoted (matrix likely singular)"
                        .to_string(),
                ));
            }
            p_new_to_old[new] = old;
        }
        let p = Permutation::from_perm(p_new_to_old);

        // ---- 6. Remap L's row indices from original-row to new-row numbering. ----
        // L(i_orig, k) entries become L(pinv[i_orig], k) = L(new_row, k).
        for col in l_cols.iter_mut() {
            for entry in col.iter_mut() {
                entry.0 = pinv[entry.0];
            }
            col.sort_by_key(|&(r, _)| r);
        }

        // ---- 7. Pack accumulators into CSC. ----
        let l = pack_csc(n, &l_cols);
        let u = pack_csc(n, &u_cols);

        Ok(SparseLU {
            l,
            u,
            p,
            q,
            n,
        })
    }

    /// Solve `A x = b` via `(P A Q) (Q^T x) = P b`, then unpermute.
    pub fn solve(&self, b: &[T]) -> Result<Vec<T>, SparseSolveError> {
        if b.len() != self.n {
            return Err(SparseSolveError::DimensionMismatch {
                a_rows: self.n,
                a_cols: self.n,
                b_len: b.len(),
            });
        }
        // P b: y[new] = b[old]
        let pb = self.p.permute_vec(b);
        // L y' = pb   (forward, L is unit lower)
        let yp = forward_solve_unit_lower(&self.l, &pb);
        // U y'' = y'  (backward)
        let ypp = backward_solve_upper(&self.u, &yp)?;
        // x = Q y''   (column permutation undoes — q maps new->old, so we apply unpermute)
        Ok(self.q.unpermute_vec(&ypp))
    }

    /// Number of non-zeros across L and U combined (excluding L's
    /// implicit unit diagonal).
    pub fn nnz(&self) -> usize {
        self.l.nnz() + self.u.nnz()
    }

    /// Read access to the L factor (for tests / introspection).
    pub fn l_factor(&self) -> &SparseCsc<T> {
        &self.l
    }

    /// Read access to the U factor (for tests / introspection).
    pub fn u_factor(&self) -> &SparseCsc<T> {
        &self.u
    }
}

/// Iterative DFS reach in the lower-triangular pattern of partial L,
/// starting from each non-zero of A's column `col`. Uses `pinv` to
/// know which rows have been pivoted (those have an L column to descend
/// into) and which haven't. Returns `top` such that
/// `xi[top..n]` is the topologically-ordered reachable pattern.
fn lu_reach<T: SparseScalar>(
    l_cols: &[Vec<(usize, T)>],
    a: &SparseCsc<T>,
    col: usize,
    pinv: &[usize],
    xi: &mut [usize],
    marker: &mut [usize],
    next_p: &mut [usize],
    path_top: &mut [usize],
    iter_k: usize,
) -> usize {
    let n = a.ncols();
    let mut top = n;

    for (i_root, _) in a.col_iter(col) {
        if marker[i_root] == iter_k {
            continue; // already in the pattern
        }
        // Iterative DFS rooted at i_root.
        let mut path_len: usize = 0;
        path_top[path_len] = i_root;
        next_p[path_len] = 0;
        marker[i_root] = iter_k;
        path_len += 1;

        while path_len > 0 {
            let depth = path_len - 1;
            let node = path_top[depth];
            let big_j = pinv[node];
            // If node is unpivoted, it has no children in L. Just push.
            let done = if big_j == usize::MAX {
                true
            } else {
                let l_col = &l_cols[big_j];
                if next_p[depth] >= l_col.len() {
                    true
                } else {
                    // descend into the next unmarked child
                    let mut descended = false;
                    while next_p[depth] < l_col.len() {
                        let child = l_col[next_p[depth]].0;
                        next_p[depth] += 1;
                        if marker[child] != iter_k {
                            marker[child] = iter_k;
                            path_top[path_len] = child;
                            next_p[path_len] = 0;
                            path_len += 1;
                            descended = true;
                            break;
                        }
                    }
                    !descended
                }
            };
            if done {
                top -= 1;
                xi[top] = node;
                path_len -= 1;
            }
        }
    }

    top
}

/// Build a `SparseCsc<T>` from per-column `(row, value)` lists. Each
/// column is consumed in the order given (caller must sort if a
/// particular row ordering is required for downstream use).
fn pack_csc<T: SparseScalar>(n: usize, cols: &[Vec<(usize, T)>]) -> SparseCsc<T> {
    let mut col_ptr = vec![0usize; n + 1];
    for j in 0..n {
        col_ptr[j + 1] = col_ptr[j] + cols[j].len();
    }
    let nnz = col_ptr[n];
    let mut row_idx = Vec::with_capacity(nnz);
    let mut values = Vec::with_capacity(nnz);
    for col in cols {
        for &(r, v) in col {
            row_idx.push(r);
            values.push(v);
        }
    }
    SparseCsc {
        rows: n,
        cols: n,
        col_ptr,
        row_idx,
        values,
    }
}

/// Solve `L y = b` where `L` is unit-lower (diagonal is implicit and
/// equal to 1.0; `L(j, j)` is NOT stored in the factor). O(nnz(L)).
fn forward_solve_unit_lower<T: SparseScalar>(l: &SparseCsc<T>, b: &[T]) -> Vec<T> {
    let n = l.nrows();
    let mut y = b.to_vec();
    for j in 0..n {
        let yj = y[j]; // L(j, j) = 1, no division
        for p in l.col_ptr[j]..l.col_ptr[j + 1] {
            let r = l.row_idx[p];
            y[r] -= l.values[p] * yj;
        }
    }
    y
}

/// Solve `U x = y` where `U` is upper-triangular with explicit diagonal
/// (first entry of each column). O(nnz(U)).
fn backward_solve_upper<T: SparseScalar>(
    u: &SparseCsc<T>,
    y: &[T],
) -> Result<Vec<T>, SparseSolveError> {
    let n = u.nrows();
    let mut x = y.to_vec();
    for j in (0..n).rev() {
        let lo = u.col_ptr[j];
        let hi = u.col_ptr[j + 1];
        if hi == lo {
            return Err(SparseSolveError::Singular {
                col: j,
                pivot: 0.0,
                threshold: 1e-14,
            });
        }
        // The diagonal of U is the LAST entry in each column when the
        // column entries are sorted ascending by row (since the pivot
        // ends at row j, the largest row index in column j).
        let diag = u.values[hi - 1];
        let xj = x[j] / diag;
        x[j] = xj;
        // Subtract column j contribution from the upper rows.
        for p in lo..hi - 1 {
            let r = u.row_idx[p];
            x[r] -= u.values[p] * xj;
        }
    }
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse_solve::ordering::{ColCountOrdering, IdentityOrdering};
    use crate::types::C64;
    use num_complex::Complex;

    fn rcsc(rows: usize, cols: usize, coo: &[(usize, usize, f64)]) -> SparseCsc<f64> {
        let mut sorted: Vec<_> = coo.iter().copied().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        SparseCsc::from_coo_sorted(rows, cols, &sorted)
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + a.abs() + b.abs())
    }

    #[test]
    fn lu_4x4_non_spd_hand_built() {
        // [[1,2,0,0],[3,4,5,0],[0,6,7,8],[0,0,9,10]]
        let a = rcsc(
            4,
            4,
            &[
                (0, 0, 1.0),
                (0, 1, 2.0),
                (1, 0, 3.0),
                (1, 1, 4.0),
                (1, 2, 5.0),
                (2, 1, 6.0),
                (2, 2, 7.0),
                (2, 3, 8.0),
                (3, 2, 9.0),
                (3, 3, 10.0),
            ],
        );
        let lu = SparseLU::factor(&a, &IdentityOrdering, 0.1).unwrap();
        let b = vec![1.0, 1.0, 1.0, 1.0];
        let x = lu.solve(&b).unwrap();
        let r = a.spmv(&x);
        for k in 0..4 {
            assert!(close(r[k], b[k], 1e-10), "residual at {k}: {}", r[k] - b[k]);
        }
    }

    #[test]
    fn lu_identity() {
        let n = 50;
        let coo: Vec<_> = (0..n).map(|i| (i, i, 1.0)).collect();
        let a = rcsc(n, n, &coo);
        let lu = SparseLU::factor(&a, &IdentityOrdering, 0.1).unwrap();
        let b: Vec<f64> = (0..n).map(|i| (i as f64 + 1.0) * 0.5).collect();
        let x = lu.solve(&b).unwrap();
        for i in 0..n {
            assert!(close(x[i], b[i], 1e-12));
        }
    }

    #[test]
    fn lu_singular_returns_error() {
        // [[1, 1], [1, 1]] is rank 1 → singular
        let a = rcsc(2, 2, &[(0, 0, 1.0), (0, 1, 1.0), (1, 0, 1.0), (1, 1, 1.0)]);
        let res = SparseLU::factor(&a, &IdentityOrdering, 0.1);
        assert!(matches!(res, Err(SparseSolveError::Singular { .. })));
    }

    #[test]
    fn lu_non_square_errors() {
        let a = SparseCsc::<f64>::from_coo_sorted(3, 4, &[(0, 0, 1.0)]);
        let res = SparseLU::factor(&a, &IdentityOrdering, 0.1);
        assert!(matches!(res, Err(SparseSolveError::NotSquare { .. })));
    }

    #[test]
    fn lu_pivoting_chooses_largest() {
        // Diagonal is small, off-diagonal is large → pivoting must swap.
        // [[0.001, 1], [1, 0.001]]: without pivoting LU diverges.
        let a = rcsc(
            2,
            2,
            &[(0, 0, 0.001), (0, 1, 1.0), (1, 0, 1.0), (1, 1, 0.001)],
        );
        let lu = SparseLU::factor(&a, &IdentityOrdering, 1.0).unwrap();
        let b = vec![1.0, 1.0];
        let x = lu.solve(&b).unwrap();
        let r = a.spmv(&x);
        assert!(close(r[0], b[0], 1e-10));
        assert!(close(r[1], b[1], 1e-10));
    }

    #[test]
    fn lu_solve_complex() {
        // 2x2 complex non-Hermitian: [[1+i, 2], [3, 4-i]]
        let coo: Vec<(usize, usize, C64)> = vec![
            (0, 0, Complex::new(1.0, 1.0)),
            (0, 1, Complex::new(2.0, 0.0)),
            (1, 0, Complex::new(3.0, 0.0)),
            (1, 1, Complex::new(4.0, -1.0)),
        ];
        let mut sorted = coo.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let a = SparseCsc::<C64>::from_coo_sorted(2, 2, &sorted);
        let lu = SparseLU::factor(&a, &IdentityOrdering, 0.1).unwrap();
        let b = vec![Complex::new(1.0, 0.0), Complex::new(0.0, 1.0)];
        let x = lu.solve(&b).unwrap();
        let r = a.spmv(&x);
        for k in 0..2 {
            let diff = r[k] - b[k];
            assert!(diff.norm() < 1e-10, "residual at {k}: {diff:?}");
        }
    }

    #[test]
    fn lu_laplacian_round_trip() {
        // 5-point Laplacian (-∇²) on a 10x10 grid. Symmetric and
        // positive-definite — LU should produce the same answer as
        // Cholesky to numerical precision, even though LU doesn't know
        // the matrix is SPD.
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

        let nx = 10;
        let ny = 10;
        let a = build_lap(nx, ny);
        let n = nx * ny;
        // Build a known solution and the corresponding RHS.
        let mut v_exact = vec![0.0; n];
        for j in 0..nx {
            for i in 0..ny {
                let k = j * ny + i;
                let xi = (i + 1) as f64 / (ny + 1) as f64;
                let xj = (j + 1) as f64 / (nx + 1) as f64;
                v_exact[k] =
                    (std::f64::consts::PI * xi).sin() * (std::f64::consts::PI * xj).sin();
            }
        }
        let rhs = a.spmv(&v_exact);

        let lu = SparseLU::factor(&a, &ColCountOrdering, 0.1).unwrap();
        let v_solved = lu.solve(&rhs).unwrap();
        let err: f64 = v_solved
            .iter()
            .zip(&v_exact)
            .map(|(s, e)| (s - e).powi(2))
            .sum::<f64>()
            .sqrt();
        let ref_norm: f64 = v_exact.iter().map(|e| e * e).sum::<f64>().sqrt();
        assert!(err / ref_norm < 1e-9, "rel err {}", err / ref_norm);
    }

    #[test]
    fn lu_dim_mismatch_on_solve() {
        let a = rcsc(3, 3, &[(0, 0, 1.0), (1, 1, 1.0), (2, 2, 1.0)]);
        let lu = SparseLU::factor(&a, &IdentityOrdering, 0.1).unwrap();
        let res = lu.solve(&[1.0, 2.0]);
        assert!(matches!(res, Err(SparseSolveError::DimensionMismatch { .. })));
    }
}

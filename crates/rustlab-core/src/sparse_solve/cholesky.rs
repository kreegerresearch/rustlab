//! Sparse Cholesky factorization for Hermitian-positive-definite matrices.
//!
//! Up-looking left-looking algorithm following Davis, *Direct Methods for
//! Sparse Linear Systems*, ch. 4.6 (`cs_chol`). For a square Hermitian
//! input `A`, factor `P A P^T = L L^H` where `L` is lower triangular with
//! real-positive diagonal and `P` is the permutation chosen by the
//! supplied `OrderingMethod`. The result is a `SparseChol` that can solve
//! arbitrary right-hand sides via two triangular sweeps.
//!
//! For real symmetric `A` the formulas reduce to the textbook real
//! Cholesky `A = L L^T`; the complex path uses conjugates where the
//! Hermitian symmetry requires.

use crate::sparse_solve::csc::{SparseCsc, SparseScalar};
use crate::sparse_solve::elimination_tree::column_elimination_tree;
use crate::sparse_solve::ordering::{OrderingMethod, Permutation};
use crate::sparse_solve::SparseSolveError;

/// Cholesky factor of a Hermitian-positive-definite sparse matrix.
#[derive(Debug)]
pub struct SparseChol<T: SparseScalar> {
    /// Lower-triangular factor in CSC. `L * L^H` reproduces `P A P^T`.
    /// The first entry of each column is the (real-positive) diagonal.
    l: SparseCsc<T>,
    /// Symmetric permutation applied at factor time.
    perm: Permutation,
    /// Cached for shape checks.
    n: usize,
}

impl<T: SparseScalar> SparseChol<T> {
    /// Factor a Hermitian-positive-definite sparse matrix.
    ///
    /// The caller is responsible for guaranteeing `a` is Hermitian — the
    /// algorithm reads only the upper triangle and trusts the symmetry.
    /// Non-Hermitian or non-positive-definite inputs return
    /// `SparseSolveError::NotSpd` when a diagonal pivot turns negative or
    /// non-real during elimination.
    pub fn factor<O: OrderingMethod>(
        a: &SparseCsc<T>,
        ord: &O,
    ) -> Result<Self, SparseSolveError> {
        if !a.is_square() {
            return Err(SparseSolveError::NotSquare {
                rows: a.nrows(),
                cols: a.ncols(),
            });
        }
        let n = a.ncols();
        let perm = ord.order(a);
        let c = perm.permute_symmetric(a);
        let parent = column_elimination_tree(&c);

        // Phase 6 of `dev/plans/em_performance.md`: replace the per-column
        // `Vec<Vec<(usize, T)>>` accumulator (one heap allocation per
        // column, growing dynamically) with a flat CSC layout sized from
        // an up-front symbolic counts pass. The numeric pass writes
        // directly into preallocated `Lp / Li / Lx` arrays, with one
        // per-column write cursor.

        // ── Symbolic pass — exact column counts of L ───────────────────
        let col_count = compute_col_counts(&c, &parent);

        // Prefix-sum to col_ptr. Allocate Li/Lx exactly once.
        let mut col_ptr = vec![0usize; n + 1];
        for j in 0..n {
            col_ptr[j + 1] = col_ptr[j] + col_count[j];
        }
        let nnz = col_ptr[n];
        let mut row_idx: Vec<usize> = vec![0usize; nnz];
        let mut values: Vec<T> = vec![T::zero(); nnz];

        // Per-column write cursor. Slot `col_ptr[j]` is reserved for the
        // diagonal (written at the end of iteration j); below-diagonal
        // entries go into `col_ptr[j] + 1 ..` in increasing-row order.
        let mut next: Vec<usize> = (0..n).map(|j| col_ptr[j] + 1).collect();

        // ── Numeric pass — same algorithm, flat-array writes ───────────
        let mut x: Vec<T> = vec![T::zero(); n];
        let mut mark: Vec<usize> = vec![usize::MAX; n];
        let mut s: Vec<usize> = vec![0usize; n];

        for k in 0..n {
            // Pattern of row k of L (columns j < k where L(k, j) != 0).
            let top = ereach(&c, k, &parent, &mut s, &mut mark);

            // Initialize x with row k of A (upper-triangle of CSC, conjed
            // by Hermitian symmetry to get the row).
            x[k] = T::zero();
            for (i, val) in c.col_iter(k) {
                if i <= k {
                    x[i] = val.conj();
                }
            }

            let mut d = x[k];
            x[k] = T::zero();

            // Triangular solve, processing pattern in topo order.
            for p in top..n {
                let j = s[p];
                let l_jj = values[col_ptr[j]]; // diagonal of column j
                let lkj = x[j] / l_jj;
                x[j] = T::zero();

                // Propagate L(k, j) into x[row] for already-written
                // below-diagonal entries of column j (indices
                // col_ptr[j]+1 .. next[j]).
                let lo = col_ptr[j] + 1;
                let hi = next[j];
                for off in lo..hi {
                    let row = row_idx[off];
                    let l_row_j = values[off];
                    debug_assert!(row > j);
                    debug_assert!(row < k, "column j entry row should be < k at iter k");
                    x[row] -= lkj * l_row_j.conj();
                }

                // Diagonal update: d -= |L(k, j)|^2.
                d -= lkj * lkj.conj();

                // Append L(k, j) at column j's next free slot.
                let slot = next[j];
                debug_assert!(slot < col_ptr[j + 1], "column j over-filled at iter k");
                row_idx[slot] = k;
                values[slot] = lkj;
                next[j] = slot + 1;
            }

            // Diagonal pivot: L(k, k) = sqrt(d). Goes in the reserved
            // slot at col_ptr[k].
            let pivot = match d.checked_sqrt_real_pos(1e-12) {
                Some(p) => p,
                None => return Err(SparseSolveError::NotSpd { col: k }),
            };
            row_idx[col_ptr[k]] = k;
            values[col_ptr[k]] = pivot;
        }

        // Sanity check (debug builds): every column was filled exactly to
        // its symbolic count.
        debug_assert!(
            (0..n).all(|j| next[j] == col_ptr[j + 1]),
            "symbolic count disagrees with numeric writes"
        );

        Ok(SparseChol {
            l: SparseCsc {
                rows: n,
                cols: n,
                col_ptr,
                row_idx,
                values,
            },
            perm,
            n,
        })
    }

    /// Solve `A x = b`. Internally: `P A P^T y = P b` then `x = P^T y`.
    pub fn solve(&self, b: &[T]) -> Result<Vec<T>, SparseSolveError> {
        if b.len() != self.n {
            return Err(SparseSolveError::DimensionMismatch {
                a_rows: self.n,
                a_cols: self.n,
                b_len: b.len(),
            });
        }
        let b_perm = self.perm.permute_vec(b);
        let y = forward_solve_lower(&self.l, &b_perm);
        let z = backward_solve_lower_transpose(&self.l, &y);
        Ok(self.perm.unpermute_vec(&z))
    }

    /// Number of non-zeros in the factor.
    pub fn nnz(&self) -> usize {
        self.l.nnz()
    }

    /// Read-only access to the factor (mostly useful for tests / introspection).
    pub fn factor_csc(&self) -> &SparseCsc<T> {
        &self.l
    }
}

/// Symbolic Cholesky pass: per-column nnz of the lower-triangular
/// factor `L` for the matrix `c` (already in permuted form) with the
/// given column elimination tree `parent`. Each column count includes
/// the diagonal (i.e., `col_counts[j]` is total nnz in column `j`,
/// not just below-diagonal entries).
///
/// Linear in `nnz(c)` for trees of bounded depth, and the standard
/// O(nnz(c) log n) in pathological cases. Reuses `ereach` — the same
/// machinery the numeric pass uses to walk the row pattern.
fn compute_col_counts<T: SparseScalar>(
    c: &SparseCsc<T>,
    parent: &[Option<usize>],
) -> Vec<usize> {
    let n = c.ncols();
    let mut col_count: Vec<usize> = vec![1; n]; // 1 per diagonal
    let mut mark: Vec<usize> = vec![usize::MAX; n];
    let mut s: Vec<usize> = vec![0usize; n];
    for k in 0..n {
        let top = ereach(c, k, parent, &mut s, &mut mark);
        for p in top..n {
            let j = s[p];
            col_count[j] += 1;
        }
    }
    col_count
}

/// Predict per-column nnz of the Cholesky factor `L` of `a` under the
/// given fill-reducing ordering, **without** running the numeric pass.
/// Useful for ordering selection, fill estimation, and pre-allocating
/// downstream buffers when the factorization will run repeatedly with
/// different numeric values but the same sparsity pattern.
///
/// Returns a `Vec<usize>` of length `n` where index `j` is the total
/// nnz in column `j` of `L` (diagonal included).
///
/// Errors only on shape (`NotSquare`); never reads the values of `a`.
pub fn symbolic_col_counts<T: SparseScalar, O: OrderingMethod>(
    a: &SparseCsc<T>,
    ord: &O,
) -> Result<Vec<usize>, SparseSolveError> {
    if !a.is_square() {
        return Err(SparseSolveError::NotSquare {
            rows: a.nrows(),
            cols: a.ncols(),
        });
    }
    let perm = ord.order(a);
    let c = perm.permute_symmetric(a);
    let parent = column_elimination_tree(&c);
    Ok(compute_col_counts(&c, &parent))
}

/// Pattern of row k of L given the upper-triangular pattern of `c`'s
/// column k and the elimination tree. Returns `top` such that
/// `s[top..n]` lists the columns `j < k` with `L(k, j) != 0` in
/// topological order (ancestors after descendants).
///
/// Davis ch. 4.5 / `cs_ereach`.
fn ereach<T: SparseScalar>(
    c: &SparseCsc<T>,
    k: usize,
    parent: &[Option<usize>],
    s: &mut [usize],
    mark: &mut [usize],
) -> usize {
    let n = c.ncols();
    let mut top = n;

    // Mark node k so the upward walk terminates if it loops back.
    mark[k] = k;

    for (i_orig, _) in c.col_iter(k) {
        if i_orig > k {
            continue; // skip lower-triangular entries
        }

        // Walk up the etree from i_orig, pushing unmarked ancestors onto
        // a temporary path that we then reverse onto s[top..n].
        let mut len: usize = 0;
        let mut node_opt = Some(i_orig);
        while let Some(node) = node_opt {
            if mark[node] == k {
                break;
            }
            s[len] = node;
            len += 1;
            mark[node] = k;
            node_opt = parent[node];
        }

        // Push the path onto the stack in reverse so ancestors are above
        // descendants in s[top..n] (topological order on consumption).
        while len > 0 {
            len -= 1;
            top -= 1;
            s[top] = s[len];
        }
    }

    top
}

/// Forward substitution: solve `L y = b` where `L` is lower triangular
/// with the diagonal at offset 0 of each column. O(nnz(L)).
fn forward_solve_lower<T: SparseScalar>(l: &SparseCsc<T>, b: &[T]) -> Vec<T> {
    let n = l.nrows();
    let mut y = b.to_vec();
    for j in 0..n {
        let lo = l.col_ptr[j];
        let hi = l.col_ptr[j + 1];
        debug_assert!(hi > lo, "column {j} of L has no diagonal");
        let yj = y[j] / l.values[lo];
        y[j] = yj;
        for p in (lo + 1)..hi {
            let r = l.row_idx[p];
            y[r] -= l.values[p] * yj;
        }
    }
    y
}

/// Backward substitution: solve `L^H x = y` where `L` is lower
/// triangular with diagonal at offset 0. O(nnz(L)).
fn backward_solve_lower_transpose<T: SparseScalar>(l: &SparseCsc<T>, y: &[T]) -> Vec<T> {
    let n = l.nrows();
    let mut x = y.to_vec();
    for j in (0..n).rev() {
        let lo = l.col_ptr[j];
        let hi = l.col_ptr[j + 1];
        // Subtract contributions from L(i, j)^H * x[i] for i > j.
        let mut sum = x[j];
        for p in (lo + 1)..hi {
            let r = l.row_idx[p];
            sum -= l.values[p].conj() * x[r];
        }
        // Divide by L(j, j) (real positive, so conj is identity).
        x[j] = sum / l.values[lo];
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse_solve::csc::SparseCsc;
    use crate::sparse_solve::ordering::{ColCountOrdering, IdentityOrdering};
    use crate::types::C64;
    use num_complex::Complex;

    /// Build a real CSC from sorted-by-(row,col) triplets.
    fn rcsc(rows: usize, cols: usize, coo: &[(usize, usize, f64)]) -> SparseCsc<f64> {
        let mut sorted: Vec<_> = coo.iter().copied().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        SparseCsc::from_coo_sorted(rows, cols, &sorted)
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + a.abs() + b.abs())
    }

    #[test]
    fn cholesky_4x4_hand_built_spd() {
        // Tridiagonal SPD: diagonally dominant, eigenvalues all positive.
        // A = [[4,1,0,0],[1,3,1,0],[0,1,3,1],[0,0,1,4]]
        let a = rcsc(
            4,
            4,
            &[
                (0, 0, 4.0),
                (0, 1, 1.0),
                (1, 0, 1.0),
                (1, 1, 3.0),
                (1, 2, 1.0),
                (2, 1, 1.0),
                (2, 2, 3.0),
                (2, 3, 1.0),
                (3, 2, 1.0),
                (3, 3, 4.0),
            ],
        );
        let chol = SparseChol::factor(&a, &IdentityOrdering).unwrap();
        let b = vec![1.0, 2.0, 3.0, 4.0];
        let x = chol.solve(&b).unwrap();
        let r = a.spmv(&x);
        for k in 0..4 {
            assert!(close(r[k], b[k], 1e-10), "residual {} at row {k}", r[k] - b[k]);
        }
    }

    #[test]
    fn cholesky_identity() {
        let n = 50;
        let coo: Vec<_> = (0..n).map(|i| (i, i, 1.0)).collect();
        let a = rcsc(n, n, &coo);
        let chol = SparseChol::factor(&a, &IdentityOrdering).unwrap();
        let b: Vec<f64> = (0..n).map(|i| (i as f64 + 1.0) * 0.5).collect();
        let x = chol.solve(&b).unwrap();
        for i in 0..n {
            assert!(close(x[i], b[i], 1e-12));
        }
    }

    #[test]
    fn cholesky_2x2_indefinite_returns_notspd() {
        // [[1, 2], [2, 1]] has eigenvalues 3 and -1 → indefinite.
        let a = rcsc(2, 2, &[(0, 0, 1.0), (0, 1, 2.0), (1, 0, 2.0), (1, 1, 1.0)]);
        let err = SparseChol::factor(&a, &IdentityOrdering).unwrap_err();
        assert!(matches!(err, SparseSolveError::NotSpd { .. }));
    }

    #[test]
    fn cholesky_zero_diagonal_returns_notspd() {
        // [[0, 1], [1, 0]] is symmetric but with zero diagonals → not SPD.
        let a = rcsc(2, 2, &[(0, 1, 1.0), (1, 0, 1.0)]);
        let err = SparseChol::factor(&a, &IdentityOrdering).unwrap_err();
        assert!(matches!(err, SparseSolveError::NotSpd { .. }));
    }

    #[test]
    fn cholesky_non_square_errors() {
        let a = SparseCsc::<f64>::from_coo_sorted(3, 4, &[(0, 0, 1.0)]);
        let err = SparseChol::factor(&a, &IdentityOrdering).unwrap_err();
        assert!(matches!(err, SparseSolveError::NotSquare { .. }));
    }

    /// Build a 2-D 5-point Laplacian on an `n x n` grid as CSC. Same
    /// stencil as `laplacian_2d` but real-valued and no special-case
    /// handling — a clean SPD test matrix.
    fn laplacian_2d_real(nx: usize, ny: usize, dx: f64, dy: f64) -> SparseCsc<f64> {
        let inv_dx2 = 1.0 / (dx * dx);
        let inv_dy2 = 1.0 / (dy * dy);
        let diag = -2.0 * (inv_dx2 + inv_dy2);
        let mut coo: Vec<(usize, usize, f64)> = Vec::new();
        for j in 0..nx {
            for i in 0..ny {
                let k = j * ny + i;
                coo.push((k, k, diag));
                if i > 0 {
                    coo.push((k, k - 1, inv_dy2));
                }
                if i + 1 < ny {
                    coo.push((k, k + 1, inv_dy2));
                }
                if j > 0 {
                    coo.push((k, k - ny, inv_dx2));
                }
                if j + 1 < nx {
                    coo.push((k, k + ny, inv_dx2));
                }
            }
        }
        coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        SparseCsc::from_coo_sorted(nx * ny, nx * ny, &coo)
    }

    #[test]
    fn cholesky_laplacian_20x20_round_trip() {
        // The 2-D Laplacian as built above is *negative* SPD: diag is
        // -2*(1/dx^2 + 1/dy^2). Negate so the resulting matrix is
        // positive-definite — equivalent to solving `(-L) x = -b`.
        let nx = 20;
        let ny = 20;
        let dx = 1.0;
        let dy = 1.0;
        let l_neg = laplacian_2d_real(nx, ny, dx, dy);
        let n = nx * ny;
        // Negate the matrix (entrywise).
        let mut neg_coo = Vec::new();
        for j in 0..n {
            for (r, v) in l_neg.col_iter(j) {
                neg_coo.push((r, j, -v));
            }
        }
        neg_coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let a = SparseCsc::<f64>::from_coo_sorted(n, n, &neg_coo);

        // Build a known interior solution: V_exact(i, j) = sin(pi*i/(ny+1))*sin(pi*j/(nx+1)).
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

        let chol = SparseChol::factor(&a, &ColCountOrdering).unwrap();
        let v_solved = chol.solve(&rhs).unwrap();

        let err_norm: f64 = v_solved
            .iter()
            .zip(&v_exact)
            .map(|(s, e)| (s - e).powi(2))
            .sum::<f64>()
            .sqrt();
        let ref_norm: f64 = v_exact.iter().map(|e| e * e).sum::<f64>().sqrt();
        let rel = err_norm / ref_norm;
        assert!(rel < 1e-9, "relative error {rel} too large");
    }

    #[test]
    fn cholesky_complex_hermitian() {
        // Build a 3x3 complex Hermitian SPD: diag real, off-diagonal complex
        // with conjugate symmetry. Eigenvalues all positive.
        // A = [[3, 1+i, 0], [1-i, 4, 2-i], [0, 2+i, 5]]
        let coo: Vec<(usize, usize, C64)> = vec![
            (0, 0, Complex::new(3.0, 0.0)),
            (0, 1, Complex::new(1.0, 1.0)),
            (1, 0, Complex::new(1.0, -1.0)),
            (1, 1, Complex::new(4.0, 0.0)),
            (1, 2, Complex::new(2.0, -1.0)),
            (2, 1, Complex::new(2.0, 1.0)),
            (2, 2, Complex::new(5.0, 0.0)),
        ];
        let mut sorted = coo.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let a = SparseCsc::<C64>::from_coo_sorted(3, 3, &sorted);

        let chol = SparseChol::factor(&a, &IdentityOrdering).unwrap();

        let b = vec![
            Complex::new(1.0, 0.5),
            Complex::new(2.0, -1.0),
            Complex::new(0.5, 0.5),
        ];
        let x = chol.solve(&b).unwrap();
        let r = a.spmv(&x);
        for k in 0..3 {
            let diff = r[k] - b[k];
            assert!(
                diff.norm() < 1e-10,
                "residual at {k}: {diff:?}, |.|={}",
                diff.norm()
            );
        }
    }

    #[test]
    fn symbolic_col_counts_match_factor_nnz_4x4() {
        // Phase 6 invariant: the standalone symbolic pass must produce
        // the same per-column nnz that the numeric factorization ends
        // up writing. Tested at release-build (the in-factor
        // debug_assert covers this only in debug builds).
        let a = rcsc(
            4,
            4,
            &[
                (0, 0, 4.0),
                (0, 1, 1.0),
                (1, 0, 1.0),
                (1, 1, 3.0),
                (1, 2, 1.0),
                (2, 1, 1.0),
                (2, 2, 3.0),
                (2, 3, 1.0),
                (3, 2, 1.0),
                (3, 3, 4.0),
            ],
        );
        let pred = symbolic_col_counts(&a, &IdentityOrdering).unwrap();
        let chol = SparseChol::factor(&a, &IdentityOrdering).unwrap();
        let l = chol.factor_csc();
        let actual: Vec<usize> = (0..l.ncols())
            .map(|j| l.col_ptr[j + 1] - l.col_ptr[j])
            .collect();
        assert_eq!(pred, actual, "symbolic vs actual nnz disagree");
    }

    #[test]
    fn symbolic_col_counts_match_factor_nnz_grid_laplacian() {
        // 12x12 grid → 144x144 SPD matrix. Identity ordering keeps the
        // matrix banded, so we hit a non-trivial column-count profile
        // (some cols dense-ish, some sparse).
        let nx = 12;
        let ny = 12;
        let n = nx * ny;
        let l_neg = laplacian_2d_real(nx, ny, 1.0, 1.0);
        let mut neg_coo = Vec::new();
        for j in 0..n {
            for (r, v) in l_neg.col_iter(j) {
                neg_coo.push((r, j, -v));
            }
        }
        neg_coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let a = SparseCsc::<f64>::from_coo_sorted(n, n, &neg_coo);
        let pred = symbolic_col_counts(&a, &IdentityOrdering).unwrap();
        let chol = SparseChol::factor(&a, &IdentityOrdering).unwrap();
        let l = chol.factor_csc();
        for j in 0..n {
            let actual = l.col_ptr[j + 1] - l.col_ptr[j];
            assert_eq!(
                pred[j], actual,
                "col {j}: symbolic predicted {} but factor has {actual}",
                pred[j]
            );
        }
        // Sanity: total predicted nnz == factor nnz.
        let pred_total: usize = pred.iter().sum();
        assert_eq!(pred_total, l.nnz());
    }

    #[test]
    fn symbolic_col_counts_non_square_errors() {
        let a = SparseCsc::<f64>::from_coo_sorted(3, 4, &[(0, 0, 1.0)]);
        let err = symbolic_col_counts(&a, &IdentityOrdering).unwrap_err();
        assert!(matches!(err, SparseSolveError::NotSquare { .. }));
    }

    #[test]
    fn cholesky_solve_dim_mismatch() {
        let a = rcsc(3, 3, &[(0, 0, 1.0), (1, 1, 1.0), (2, 2, 1.0)]);
        let chol = SparseChol::factor(&a, &IdentityOrdering).unwrap();
        let b = vec![1.0, 2.0]; // wrong length
        let err = chol.solve(&b).unwrap_err();
        assert!(matches!(err, SparseSolveError::DimensionMismatch { .. }));
    }
}

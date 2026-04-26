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

        // Per-column accumulators for the lower-triangular factor. Each
        // column starts empty; the diagonal goes in first, then the
        // below-diagonal entries appear (in ascending row order) as later
        // iterations of `k` write `L(k, j)` into column `j`.
        let mut cols_l: Vec<Vec<(usize, T)>> = vec![Vec::new(); n];

        // Sparse-accumulator workspace for the active column.
        let mut x: Vec<T> = vec![T::zero(); n];
        // Mark vector for ereach. mark[i] == k means node i was visited in column k.
        let mut mark: Vec<usize> = vec![usize::MAX; n];
        // Pattern stack used by ereach (top-of-stack convention).
        let mut s: Vec<usize> = vec![0usize; n];

        for k in 0..n {
            // ---- Pattern of row k of L (columns j < k where L(k, j) != 0). ----
            let top = ereach(&c, k, &parent, &mut s, &mut mark);

            // ---- Initialize x with row k of A. ----
            // The CSC stores entries by column; row k of column j (for j < k)
            // is the upper-triangular entry A(j, k). We need A(k, j), which
            // by Hermitian symmetry is conj(A(j, k)). For real A, conj is
            // the identity. The diagonal A(k, k) is real-valued for Hermitian
            // matrices, so the conjugate also leaves it alone.
            x[k] = T::zero();
            for (i, val) in c.col_iter(k) {
                if i <= k {
                    x[i] = val.conj();
                }
            }

            let mut d = x[k];
            x[k] = T::zero();

            // ---- Triangular solve: process pattern in topo order. ----
            for p in top..n {
                let j = s[p];
                let l_jj = cols_l[j][0].1; // diagonal entry of column j (real-positive)
                let lkj = x[j] / l_jj;
                x[j] = T::zero();

                // Propagate the contribution of L(k, j) to remaining
                // pattern entries that are still ahead in topo order:
                //   x[r] -= L(k, j) * conj(L(r, j))  for r > j with L(r, j) != 0.
                // Entries [1..] of column j of L are below-diagonal in
                // ascending row order. Their row indices are < k (entries
                // for rows > k haven't been written yet at iteration k).
                for &(row, l_row_j) in cols_l[j].iter().skip(1) {
                    debug_assert!(row > j);
                    debug_assert!(row < k, "column j entry row should be < k at iter k");
                    x[row] -= lkj * l_row_j.conj();
                }

                // Diagonal update: d -= |L(k, j)|^2 = L(k, j) * conj(L(k, j)).
                d -= lkj * lkj.conj();

                // Append L(k, j) to column j of L.
                cols_l[j].push((k, lkj));
            }

            // ---- Diagonal pivot: L(k, k) = sqrt(d). ----
            let pivot = match d.checked_sqrt_real_pos(1e-12) {
                Some(p) => p,
                None => return Err(SparseSolveError::NotSpd { col: k }),
            };
            // Diagonal goes at the FRONT of column k so it's at offset 0 —
            // matches the algorithm's expectation that `cols_l[j][0]` is
            // always L(j, j).
            cols_l[k].insert(0, (k, pivot));
        }

        // ---- Convert column accumulators into CSC. ----
        let mut col_ptr = vec![0usize; n + 1];
        for j in 0..n {
            col_ptr[j + 1] = col_ptr[j] + cols_l[j].len();
        }
        let nnz = col_ptr[n];
        let mut row_idx: Vec<usize> = Vec::with_capacity(nnz);
        let mut values: Vec<T> = Vec::with_capacity(nnz);
        for col in cols_l {
            for (r, v) in col {
                row_idx.push(r);
                values.push(v);
            }
        }

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
    fn cholesky_solve_dim_mismatch() {
        let a = rcsc(3, 3, &[(0, 0, 1.0), (1, 1, 1.0), (2, 2, 1.0)]);
        let chol = SparseChol::factor(&a, &IdentityOrdering).unwrap();
        let b = vec![1.0, 2.0]; // wrong length
        let err = chol.solve(&b).unwrap_err();
        assert!(matches!(err, SparseSolveError::DimensionMismatch { .. }));
    }
}

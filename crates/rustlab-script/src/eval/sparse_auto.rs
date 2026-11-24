//! The direct-solver choice behind sparse `A \ b` and `spsolve(A, b)`.
//!
//! MATLAB's sparse `\` checks for a triangular matrix first, tries Cholesky
//! when `A` is Hermitian with a positive real diagonal, and otherwise runs an
//! LU that first strips row and column singletons. Here:
//!
//! 0. Diagonal, upper, or lower triangular with no zero on the diagonal →
//!    substitution (`"auto"` only). This is what `R \ (R' \ b)` takes for
//!    a `chol` factor.
//! 1. Hermitian with a positive real diagonal → sparse Cholesky.
//! 2. Not Hermitian, but the block left after removing the row singletons
//!    (the rows `pin_dirichlet` writes) is → solve the pinned unknowns
//!    directly and Cholesky the free block (`ReducedChol`).
//! 3. Otherwise, or when either Cholesky meets a non-positive pivot → LU on
//!    the whole matrix, as before.
//!
//! `"cholesky"` mode allows 1 and 2 only and errors instead of falling
//! back; `"lu"` mode is 3 alone. See `dev/plans/sparse_spd_solve.md`.

use super::builtins::{
    factor_sparse_cholesky, factor_sparse_lu, solve_with_factor, OrderingChoice,
};
use super::value::SparseFactor;
use ndarray::Array1;
use num_complex::Complex;
use rustlab_core::sparse_solve::{
    AmdOrdering, IdentityOrdering, NestedDissectionOrdering, ReducedChol, RowSingletonSplit,
    SparseCsc, SparseScalar, SparseSolveError, SpdAutoOrdering,
};
use rustlab_core::{CVector, SparseMat, C64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Auto,
    Cholesky,
    Lu,
}

pub(crate) enum AutoFactor {
    Plain(SparseFactor),
    ReducedReal(ReducedChol<f64>),
    ReducedComplex(ReducedChol<C64>),
    TriReal(RowTriangular<f64>),
    TriComplex(RowTriangular<C64>),
}

/// A diagonal or triangular matrix kept row by row, solved by row-oriented
/// substitution. `SparseMat` entries are row-major, so building it is one
/// sequential pass (no column scatter), which matters for `R \ b` on a
/// large `chol` factor.
pub(crate) struct RowTriangular<T> {
    row_ptr: Vec<usize>,
    cols: Vec<usize>,
    vals: Vec<T>,
    upper: bool,
    diagonal: bool,
}

impl<T: SparseScalar> RowTriangular<T> {
    fn build(sm: &SparseMat, upper: bool, diagonal: bool, conv: impl Fn(C64) -> T) -> Self {
        let n = sm.rows;
        let mut row_ptr = vec![0usize; n + 1];
        for &(i, _, _) in &sm.entries {
            row_ptr[i + 1] += 1;
        }
        for i in 0..n {
            row_ptr[i + 1] += row_ptr[i];
        }
        let mut next = row_ptr[..n].to_vec();
        let mut cols = vec![0usize; sm.entries.len()];
        let mut vals = vec![T::zero(); sm.entries.len()];
        for &(i, j, v) in &sm.entries {
            cols[next[i]] = j;
            vals[next[i]] = conv(v);
            next[i] += 1;
        }
        RowTriangular {
            row_ptr,
            cols,
            vals,
            upper,
            diagonal,
        }
    }

    /// Every diagonal entry is stored and nonzero (checked when built).
    fn solve(&self, b: &[T]) -> Vec<T> {
        let n = b.len();
        let mut x = vec![T::zero(); n];
        let row = |i: usize, x: &mut [T]| {
            let mut s = b[i];
            let mut d = T::one();
            for p in self.row_ptr[i]..self.row_ptr[i + 1] {
                let j = self.cols[p];
                if j == i {
                    d = self.vals[p];
                } else {
                    s -= self.vals[p] * x[j];
                }
            }
            x[i] = s / d;
        };
        if self.upper {
            for i in (0..n).rev() {
                row(i, &mut x);
            }
        } else {
            for i in 0..n {
                row(i, &mut x);
            }
        }
        x
    }
}

impl AutoFactor {
    /// `"cholesky"`, `"reduced-cholesky"`, `"lu"`, `"triangular"`, or
    /// `"diagonal"`.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            AutoFactor::Plain(f) if f.kind() == "chol" => "cholesky",
            AutoFactor::Plain(_) => "lu",
            AutoFactor::ReducedReal(_) | AutoFactor::ReducedComplex(_) => "reduced-cholesky",
            AutoFactor::TriReal(t) if t.diagonal => "diagonal",
            AutoFactor::TriComplex(t) if t.diagonal => "diagonal",
            AutoFactor::TriReal(_) | AutoFactor::TriComplex(_) => "triangular",
        }
    }

    #[cfg(test)]
    pub(crate) fn nnz(&self) -> usize {
        match self {
            AutoFactor::Plain(f) => f.nnz(),
            AutoFactor::ReducedReal(r) => r.nnz(),
            AutoFactor::ReducedComplex(r) => r.nnz(),
            AutoFactor::TriReal(_) | AutoFactor::TriComplex(_) => 0,
        }
    }

    /// A real factor takes a complex `b` as two real solves.
    pub(crate) fn solve(&self, b: &CVector) -> Result<CVector, String> {
        let real_factor = match self {
            AutoFactor::Plain(f) => f.scalar() == "real",
            AutoFactor::ReducedReal(_) | AutoFactor::TriReal(_) => true,
            AutoFactor::ReducedComplex(_) | AutoFactor::TriComplex(_) => false,
        };
        if real_factor && b.iter().any(|c| c.im != 0.0) {
            let re = self.solve_one(&b.mapv(|c| Complex::new(c.re, 0.0)))?;
            let im = self.solve_one(&b.mapv(|c| Complex::new(c.im, 0.0)))?;
            return Ok(Array1::from_iter(
                re.iter()
                    .zip(im.iter())
                    .map(|(r, i)| Complex::new(r.re, i.re)),
            ));
        }
        self.solve_one(b)
    }

    fn solve_one(&self, b: &CVector) -> Result<CVector, String> {
        match self {
            AutoFactor::Plain(f) => solve_with_factor(f, b),
            AutoFactor::ReducedReal(r) => {
                let br: Vec<f64> = b.iter().map(|c| c.re).collect();
                let x = r.solve(&br).map_err(|e| e.to_string())?;
                Ok(Array1::from_iter(
                    x.into_iter().map(|v| Complex::new(v, 0.0)),
                ))
            }
            AutoFactor::ReducedComplex(r) => {
                let bc: Vec<C64> = b.iter().copied().collect();
                r.solve(&bc)
                    .map(Array1::from_vec)
                    .map_err(|e| e.to_string())
            }
            AutoFactor::TriReal(t) => {
                let br: Vec<f64> = b.iter().map(|c| c.re).collect();
                Ok(Array1::from_iter(
                    t.solve(&br).into_iter().map(|v| Complex::new(v, 0.0)),
                ))
            }
            AutoFactor::TriComplex(t) => {
                let bc: Vec<C64> = b.iter().copied().collect();
                Ok(Array1::from_vec(t.solve(&bc)))
            }
        }
    }
}

/// Factor `sm` (square) for repeated solves. Errors are plain messages;
/// callers prefix their own function name.
pub(crate) fn factor(
    sm: &SparseMat,
    mode: Mode,
    ordering: OrderingChoice,
) -> Result<AutoFactor, String> {
    if mode == Mode::Lu {
        return factor_sparse_lu(sm, ordering).map(AutoFactor::Plain);
    }
    if mode == Mode::Auto {
        if let Some(f) = factor_triangular(sm) {
            return Ok(f);
        }
    }
    if sm.is_spd_estimate(1e-10) {
        match factor_sparse_cholesky(sm, ordering) {
            Ok(f) => return Ok(AutoFactor::Plain(f)),
            Err(e) if mode == Mode::Cholesky => return Err(e),
            Err(_) => {}
        }
    } else {
        match factor_reduced(sm, ordering) {
            Some(Ok(f)) => return Ok(f),
            Some(Err(e)) if mode == Mode::Cholesky => return Err(e.to_string()),
            None if mode == Mode::Cholesky => {
                return Err(SparseSolveError::NotHermitian.to_string());
            }
            _ => {}
        }
    }
    factor_sparse_lu(sm, ordering).map(AutoFactor::Plain)
}

/// Substitution for a square matrix with entries on at most one side of
/// the diagonal and every diagonal entry stored (so nonzero) and every
/// entry finite. `None` otherwise, including the empty matrix.
fn factor_triangular(sm: &SparseMat) -> Option<AutoFactor> {
    let n = sm.rows;
    if n == 0 || n != sm.cols {
        return None;
    }
    let (mut below, mut above) = (false, false);
    let mut diag = 0usize;
    for &(i, j, v) in &sm.entries {
        if !v.re.is_finite() || !v.im.is_finite() {
            return None;
        }
        below |= i > j;
        above |= i < j;
        if below && above {
            return None;
        }
        diag += usize::from(i == j);
    }
    if diag != n {
        return None;
    }
    let (upper, diagonal) = (!below, !below && !above);
    if super::builtins::sparse_all_real(sm, None) {
        Some(AutoFactor::TriReal(RowTriangular::build(
            sm,
            upper,
            diagonal,
            |v| v.re,
        )))
    } else {
        Some(AutoFactor::TriComplex(RowTriangular::build(
            sm,
            upper,
            diagonal,
            |v| v,
        )))
    }
}

#[cfg(test)]
thread_local! {
    static SPARSE_TRANSPOSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static FUSED_SOLVES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Called wherever a sparse transpose is materialized (test counter).
#[inline]
pub(crate) fn note_sparse_transpose() {
    #[cfg(test)]
    SPARSE_TRANSPOSES.with(|c| c.set(c.get() + 1));
}

#[cfg(test)]
pub(crate) fn sparse_transposes() -> usize {
    SPARSE_TRANSPOSES.with(|c| c.get())
}

#[cfg(test)]
pub(crate) fn fused_solves() -> usize {
    FUSED_SOLVES.with(|c| c.get())
}

/// `(upper, real)` for a square triangular `sm` whose entries are
/// row-major sorted, finite, and include every diagonal entry (what
/// `chol` returns). `None` otherwise. A diagonal matrix is upper.
fn in_place_triangular(sm: &SparseMat) -> Option<(bool, bool)> {
    let n = sm.rows;
    if n == 0 || n != sm.cols {
        return None;
    }
    let (mut below, mut above, mut real) = (false, false, true);
    let mut diag = 0usize;
    let mut prev: Option<(usize, usize)> = None;
    for &(i, j, v) in &sm.entries {
        if !v.re.is_finite() || !v.im.is_finite() || prev.is_some_and(|p| p >= (i, j)) {
            return None;
        }
        prev = Some((i, j));
        below |= i > j;
        above |= i < j;
        if below && above {
            return None;
        }
        diag += usize::from(i == j);
        real &= v.im.abs() < 1e-12;
    }
    (diag == n).then_some((!below, real))
}

/// Solve every column with `sweep_real` / `sweep_complex`. A real matrix
/// takes a complex column as two real solves.
fn solve_columns_with(
    columns: &[CVector],
    real: bool,
    sweep_real: impl Fn(&[f64]) -> Vec<f64>,
    sweep_complex: impl Fn(&[C64]) -> Vec<C64>,
) -> Vec<CVector> {
    let solve_one = |b: &CVector| -> CVector {
        if real {
            let part = |pick: fn(C64) -> f64| {
                let bv: Vec<f64> = b.iter().map(|&z| pick(z)).collect();
                sweep_real(&bv)
            };
            let re = part(|z| z.re);
            if b.iter().all(|z| z.im == 0.0) {
                return Array1::from_iter(re.into_iter().map(|v| Complex::new(v, 0.0)));
            }
            let im = part(|z| z.im);
            Array1::from_iter(re.into_iter().zip(im).map(|(r, i)| Complex::new(r, i)))
        } else {
            let bv: Vec<C64> = b.iter().copied().collect();
            Array1::from_vec(sweep_complex(&bv))
        }
    };
    columns.iter().map(solve_one).collect()
}

/// One-shot `\` on a triangular `sm` whose entries are row-major sorted
/// (what `chol` returns): substitution read straight from the entries, no
/// copy. `None` when `sm` is not such a matrix; the caller then factors.
pub(crate) fn solve_triangular_in_place(
    sm: &SparseMat,
    columns: &[CVector],
) -> Option<Vec<CVector>> {
    let (upper, real) = in_place_triangular(sm)?;
    Some(solve_columns_with(
        columns,
        real,
        |b| sweep(&sm.entries, b, upper, |v| v.re),
        |b| sweep(&sm.entries, b, upper, |v| v),
    ))
}

/// `A' \ b` (`conj`) or `A.' \ b` for the same kind of `A`, without
/// forming the transpose. Row `i` of `A` is column `i` of `A'`, so a
/// column-oriented substitution on `A'` reads `A`'s rows in order: forward
/// for upper `A` (`A'` is lower), backward for lower `A`. Each unknown
/// receives its updates in the same order as the row-oriented solve on a
/// materialized transpose, so the result is bit-identical to it.
pub(crate) fn solve_transposed_triangular_in_place(
    sm: &SparseMat,
    columns: &[CVector],
    conj: bool,
) -> Option<Vec<CVector>> {
    let (upper, real) = in_place_triangular(sm)?;
    #[cfg(test)]
    FUSED_SOLVES.with(|c| c.set(c.get() + 1));
    let f = move |v: C64| if conj { v.conj() } else { v };
    Some(solve_columns_with(
        columns,
        real,
        |b| sweep_transposed(&sm.entries, b, upper, |v| v.re),
        |b| sweep_transposed(&sm.entries, b, upper, f),
    ))
}

/// Column-oriented substitution on the transpose of a triangular `A`
/// given by row-major entries with every diagonal stored: the diagonal is
/// the first entry of each row when `A` is upper and the last when lower.
fn sweep_transposed<T: SparseScalar>(
    entries: &[(usize, usize, C64)],
    b: &[T],
    upper: bool,
    conv: impl Fn(C64) -> T,
) -> Vec<T> {
    let mut y = b.to_vec();
    let mut x = vec![T::zero(); b.len()];
    let mut row = |seg: &[(usize, usize, C64)]| {
        let (diag, rest) = if upper {
            (seg[0], &seg[1..])
        } else {
            (seg[seg.len() - 1], &seg[..seg.len() - 1])
        };
        let i = diag.0;
        let xi = y[i] / conv(diag.2);
        x[i] = xi;
        for &(_, j, v) in rest {
            y[j] -= conv(v) * xi;
        }
    };
    if upper {
        let mut start = 0;
        while start < entries.len() {
            let i = entries[start].0;
            let mut end = start + 1;
            while end < entries.len() && entries[end].0 == i {
                end += 1;
            }
            row(&entries[start..end]);
            start = end;
        }
    } else {
        let mut end = entries.len();
        while end > 0 {
            let i = entries[end - 1].0;
            let mut start = end - 1;
            while start > 0 && entries[start - 1].0 == i {
                start -= 1;
            }
            row(&entries[start..end]);
            end = start;
        }
    }
    x
}

/// Row-oriented substitution over row-major entries in which every row
/// holds its (nonzero) diagonal: forward for lower, backward for upper.
fn sweep<T: SparseScalar>(
    entries: &[(usize, usize, C64)],
    b: &[T],
    upper: bool,
    conv: impl Fn(C64) -> T,
) -> Vec<T> {
    let mut x = vec![T::zero(); b.len()];
    let mut run = |it: &mut dyn Iterator<Item = &(usize, usize, C64)>| {
        let mut cur: Option<usize> = None;
        let (mut s, mut d) = (T::zero(), T::one());
        for &(i, j, v) in it {
            if cur != Some(i) {
                if let Some(r) = cur {
                    x[r] = s / d;
                }
                cur = Some(i);
                s = b[i];
            }
            if i == j {
                d = conv(v);
            } else {
                s -= conv(v) * x[j];
            }
        }
        if let Some(r) = cur {
            x[r] = s / d;
        }
    };
    if upper {
        run(&mut entries.iter().rev());
    } else {
        run(&mut entries.iter());
    }
    x
}

/// `None` when `sm` has no row singleton. Otherwise the Cholesky of the
/// free block, or why it does not apply.
fn factor_reduced(
    sm: &SparseMat,
    ordering: OrderingChoice,
) -> Option<Result<AutoFactor, SparseSolveError>> {
    if sm.rows != sm.cols {
        return None;
    }
    if super::builtins::sparse_all_real(sm, None) {
        let csc: SparseCsc<f64> = sm.to_csc().ok()?;
        let split = RowSingletonSplit::find(&csc)?;
        Some(reduced_chol(split, ordering).map(AutoFactor::ReducedReal))
    } else {
        let csc: SparseCsc<C64> = sm.to_csc().ok()?;
        let split = RowSingletonSplit::find(&csc)?;
        Some(reduced_chol(split, ordering).map(AutoFactor::ReducedComplex))
    }
}

fn reduced_chol<T: SparseScalar>(
    split: RowSingletonSplit<T>,
    ordering: OrderingChoice,
) -> Result<ReducedChol<T>, SparseSolveError> {
    match ordering {
        OrderingChoice::Auto => ReducedChol::factor(split, &SpdAutoOrdering),
        OrderingChoice::Identity => ReducedChol::factor(split, &IdentityOrdering),
        OrderingChoice::Amd => ReducedChol::factor(split, &AmdOrdering),
        OrderingChoice::Nd => ReducedChol::factor(split, &NestedDissectionOrdering),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn matrix(src: &str) -> SparseMat {
        let stmts = parser::parse(lexer::tokenize(&format!("{src}\n")).unwrap()).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        match ev.get("A") {
            Some(Value::SparseMatrix(sm)) => sm.clone(),
            other => panic!("A: {other:?}"),
        }
    }

    const PINNED: &str = "n = 6; A = -1 * laplacian_2d(n, n); b = zeros(n * n, 1);
        G = ones(n, n); G(2:n-1, 2:n-1) = 0;
        [A, b] = pin_dirichlet(A, b, G, 1.0);";

    #[test]
    fn pinned_operator_takes_reduced_cholesky() {
        let sm = matrix(PINNED);
        let f = factor(&sm, Mode::Auto, OrderingChoice::Auto).unwrap();
        assert_eq!(f.kind(), "reduced-cholesky");
    }

    #[test]
    fn spd_operator_takes_cholesky() {
        let sm = matrix("A = -1 * laplacian_2d(6, 6);");
        assert_eq!(
            factor(&sm, Mode::Auto, OrderingChoice::Auto)
                .unwrap()
                .kind(),
            "cholesky"
        );
    }

    #[test]
    fn non_symmetric_operator_takes_lu() {
        let sm = matrix("A = sparse([4, 1, 0; 0, 3, 1; 0, 2, 5]);");
        assert_eq!(
            factor(&sm, Mode::Auto, OrderingChoice::Auto)
                .unwrap()
                .kind(),
            "lu"
        );
        assert!(factor(&sm, Mode::Cholesky, OrderingChoice::Auto).is_err());
    }

    /// Complex, non-symmetric 2-D operator (an FDFD-style row scaling of the
    /// Laplacian) with no ordering hint.
    const STRETCHED: &str = "n = 40; N = n * n; L = laplacian_2d(n, n);
        s = 1 + 0.5j * linspace(0, 1, N);
        [I, J, V] = find(L);
        A = sparse(I, J, V(:) .* s(I(:)).', N, N) + 3 * speye(N);";

    fn lu_nnz(sm: &SparseMat, ordering: OrderingChoice) -> usize {
        factor_sparse_lu(sm, ordering).unwrap().nnz()
    }

    #[test]
    fn auto_lu_uses_nested_dissection_on_a_grid() {
        let sm = matrix(STRETCHED);
        assert!(sm.ordering_hint.is_none());
        let f = factor(&sm, Mode::Auto, OrderingChoice::Auto).unwrap();
        assert_eq!(f.kind(), "lu");
        let nd = lu_nnz(&sm, OrderingChoice::Nd);
        assert_eq!(f.nnz(), nd);
        assert!(nd < lu_nnz(&sm, OrderingChoice::Identity));
        assert!(nd < lu_nnz(&sm, OrderingChoice::Amd));
    }

    #[test]
    fn auto_lu_ignores_the_natural_hint_on_a_grid() {
        let sm = matrix("A = laplacian_2d(40, 40) + 0.1 * spdiags(ones(1600, 1), 1, 1600, 1600);");
        assert_eq!(
            factor(&sm, Mode::Lu, OrderingChoice::Auto).unwrap().nnz(),
            lu_nnz(&sm, OrderingChoice::Nd)
        );
    }

    #[test]
    fn auto_lu_keeps_the_natural_order_on_a_band() {
        let sm = matrix("A = laplacian_1d(300) + 0.1 * spdiags(ones(300, 1), 1, 300, 300);");
        assert_eq!(
            factor(&sm, Mode::Lu, OrderingChoice::Auto).unwrap().nnz(),
            lu_nnz(&sm, OrderingChoice::Identity)
        );
    }

    fn run_counting(src: &str) -> (usize, usize) {
        let stmts = parser::parse(lexer::tokenize(&format!("{src}\n")).unwrap()).unwrap();
        let mut ev = Evaluator::new();
        let (t0, f0) = (sparse_transposes(), fused_solves());
        ev.run(&stmts).unwrap();
        (sparse_transposes() - t0, fused_solves() - f0)
    }

    #[test]
    fn transposed_triangular_solve_is_fused() {
        let setup = "R = chol(-1 * laplacian_2d(8, 8)); b = ones(64, 1);";
        assert_eq!(run_counting(setup), (0, 0));
        assert_eq!(run_counting(&format!("{setup} x = R' \\ b;")), (0, 1));
        assert_eq!(run_counting(&format!("{setup} x = R.' \\ [b, b];")), (0, 1));
        assert_eq!(
            run_counting(&format!("{setup} [R, p, P] = chol(-1 * laplacian_2d(8, 8)); x = P * (R \\ (R' \\ (P' * b)));")),
            (1, 1),
            "in the MATLAB idiom only P' (n entries) is transposed; R' \\ c is fused"
        );
        // A general matrix still materializes its transpose.
        assert_eq!(
            run_counting("A = sparse([4, 1, 0; 2, 3, 1; 0, 2, 5]); x = A' \\ [1; 2; 3];"),
            (1, 0)
        );
    }

    #[test]
    fn a_chol_factor_takes_the_triangular_solve() {
        let sm = matrix("R = chol(-1 * laplacian_2d(9, 9)); A = R;");
        let f = factor(&sm, Mode::Auto, OrderingChoice::Auto).unwrap();
        assert_eq!(f.kind(), "triangular");
        let sm = matrix("R = chol(-1 * laplacian_2d(9, 9)); A = R';");
        let f = factor(&sm, Mode::Auto, OrderingChoice::Auto).unwrap();
        assert_eq!(f.kind(), "triangular");
        let sm = matrix("A = 3 * speye(5);");
        let f = factor(&sm, Mode::Auto, OrderingChoice::Auto).unwrap();
        assert_eq!(f.kind(), "diagonal");
        // A zero on the diagonal keeps the old path; explicit modes too.
        let sm = matrix("A = sparse([1, 1; 0, 0]) + sparse([0, 0; 0, 0]);");
        assert_ne!(
            factor(&sm, Mode::Auto, OrderingChoice::Auto)
                .map(|f| f.kind())
                .unwrap_or("err"),
            "triangular"
        );
        let sm = matrix("A = sparse([2, 1; 0, 4]);");
        assert_eq!(
            factor(&sm, Mode::Lu, OrderingChoice::Auto).unwrap().kind(),
            "lu"
        );
    }

    #[test]
    fn lu_mode_stays_lu_on_a_pinned_operator() {
        let sm = matrix(PINNED);
        assert_eq!(
            factor(&sm, Mode::Lu, OrderingChoice::Auto).unwrap().kind(),
            "lu"
        );
    }
}

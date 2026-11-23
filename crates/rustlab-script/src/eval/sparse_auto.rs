//! The direct-solver choice behind sparse `A \ b` and `spsolve(A, b)`.
//!
//! MATLAB's sparse `\` tries Cholesky when `A` is Hermitian with a positive
//! real diagonal and otherwise runs an LU that first strips row and column
//! singletons. Here:
//!
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
}

impl AutoFactor {
    /// `"cholesky"`, `"reduced-cholesky"`, or `"lu"`.
    #[cfg(test)]
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            AutoFactor::Plain(f) if f.kind() == "chol" => "cholesky",
            AutoFactor::Plain(_) => "lu",
            AutoFactor::ReducedReal(_) | AutoFactor::ReducedComplex(_) => "reduced-cholesky",
        }
    }

    /// A real factor takes a complex `b` as two real solves.
    pub(crate) fn solve(&self, b: &CVector) -> Result<CVector, String> {
        let real_factor = match self {
            AutoFactor::Plain(f) => f.scalar() == "real",
            AutoFactor::ReducedReal(_) => true,
            AutoFactor::ReducedComplex(_) => false,
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

    #[test]
    fn lu_mode_stays_lu_on_a_pinned_operator() {
        let sm = matrix(PINNED);
        assert_eq!(
            factor(&sm, Mode::Lu, OrderingChoice::Auto).unwrap().kind(),
            "lu"
        );
    }
}

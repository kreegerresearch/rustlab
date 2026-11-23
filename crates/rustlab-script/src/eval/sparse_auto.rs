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
        assert_eq!(factor(&sm, Mode::Auto, OrderingChoice::Auto).unwrap().kind(), "cholesky");
    }

    #[test]
    fn non_symmetric_operator_takes_lu() {
        let sm = matrix("A = sparse([4, 1, 0; 0, 3, 1; 0, 2, 5]);");
        assert_eq!(factor(&sm, Mode::Auto, OrderingChoice::Auto).unwrap().kind(), "lu");
        assert!(factor(&sm, Mode::Cholesky, OrderingChoice::Auto).is_err());
    }

    #[test]
    fn lu_mode_stays_lu_on_a_pinned_operator() {
        let sm = matrix(PINNED);
        assert_eq!(factor(&sm, Mode::Lu, OrderingChoice::Auto).unwrap().kind(), "lu");
    }
}

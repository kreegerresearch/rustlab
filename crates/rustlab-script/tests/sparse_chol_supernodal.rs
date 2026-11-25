//! Sparse `\`, `chol`, and `decomposition` on systems large enough for the
//! supernodal Cholesky kernel (dev/plans/sparse_spd_solve.md, Phase 5).
//! Every check is an identity or a residual, so it holds for either kernel.

use rustlab_script::{Evaluator, Value};

fn run(src: &str) -> Result<Evaluator, String> {
    let src = format!("{src}\n");
    let tokens = rustlab_script::lexer::tokenize(&src).map_err(|e| e.to_string())?;
    let stmts = rustlab_script::parser::parse(tokens).map_err(|e| e.to_string())?;
    let mut ev = Evaluator::new();
    ev.run(&stmts).map_err(|e| e.to_string())?;
    Ok(ev)
}

fn eval(src: &str) -> Evaluator {
    run(src).unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"))
}

fn eval_err(src: &str) -> String {
    match run(src) {
        Err(e) => e,
        Ok(_) => panic!("expected an error\n{src}"),
    }
}

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name) {
        Some(Value::Scalar(x)) => *x,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

/// 3-D Poisson, 16³ = 4,096 unknowns, and a right-hand side.
const GRID: &str = "n = 16; N = n ^ 3; S = -1 * laplacian_3d(n, n, n);
     b = ones(N, 1); b(1:3:N) = 2;
     function m = maxabs(K)
       if issparse(K)
         v = nonzeros(K);
       else
         v = K(:);
       end
       m = 0;
       if numel(v) > 0
         m = max(abs(v));
       end
     end";

/// `S - 5.5 I` on 16³: symmetric and indefinite.
const INDEFINITE: &str = "n = 16; N = n ^ 3; S = -1 * laplacian_3d(n, n, n) - 5.5 * speye(n ^ 3);
     function m = maxabs(K)
       if issparse(K)
         v = nonzeros(K);
       else
         v = K(:);
       end
       m = 0;
       if numel(v) > 0
         m = max(abs(v));
       end
     end";

#[test]
fn backslash_residual() {
    let ev = eval(&format!(
        "{GRID}
         x = S \\ b; r = norm(S * x - b) / norm(b);
         sz = size(x); col = sz(1) == N && sz(2) == 1;
         X = S \\ [b, 2 * b]; r2 = norm(S * X(:, 2) - 2 * b) / norm(2 * b);"
    ));
    assert!(scalar(&ev, "r") < 1e-13, "{}", scalar(&ev, "r"));
    assert!(scalar(&ev, "r2") < 1e-13);
    assert_eq!(scalar(&ev, "col"), 1.0);
}

#[test]
fn backslash_on_a_pinned_operator() {
    let ev = eval(
        "n = 14; N = n ^ 3; A = -1 * laplacian_3d(n, n, n); b = zeros(N, 1);
         mask = zeros3(n, n, n); mask(1, :, :) = 1; mask(n, :, :) = 1;
         [A2, b2] = pin_dirichlet(A, b, mask, 1);
         x = A2 \\ b2; r = norm(A2 * x - b2) / norm(b2);",
    );
    assert!(scalar(&ev, "r") < 1e-12, "{}", scalar(&ev, "r"));
}

#[test]
fn backslash_complex_hermitian() {
    let ev = eval(
        "n = 14; N = n ^ 3; L = -1 * laplacian_3d(n, n, n);
         U = spdiags(0.3j * ones(N, 1), 1, N, N); S = L + U + U';
         b = (1:N)' / N + 0.5j;
         x = S \\ b; r = norm(S * x - b) / norm(b);",
    );
    assert!(scalar(&ev, "r") < 1e-13, "{}", scalar(&ev, "r"));
}

#[test]
fn one_output_chol_is_the_natural_factor() {
    let ev = eval(&format!(
        "{GRID}
         R = chol(S); [I, J] = find(R); upper = all(I <= J);
         e = maxabs(R' * R - S);
         x = R \\ (R' \\ b); r = norm(S * x - b) / norm(b);"
    ));
    assert_eq!(scalar(&ev, "upper"), 1.0);
    assert!(scalar(&ev, "e") < 1e-12, "{}", scalar(&ev, "e"));
    assert!(scalar(&ev, "r") < 1e-12);
}

#[test]
fn lower_form_is_the_transpose() {
    let ev = eval(&format!(
        "{GRID}
         R = chol(S); L = chol(S, \"lower\");
         d = maxabs(L - R'); e = maxabs(L * L' - S);"
    ));
    assert_eq!(scalar(&ev, "d"), 0.0);
    assert!(scalar(&ev, "e") < 1e-12);
}

#[test]
fn three_outputs_and_vector_form() {
    let ev = eval(&format!(
        "{GRID}
         [R, p, P] = chol(S);
         e = maxabs(R' * R - P' * S * P);
         [Rv, pv, q] = chol(S, \"vector\");
         ev = maxabs(Rv' * Rv - S(q, q));
         same = maxabs(Rv - R) == 0;
         [Ll, pl, Pl] = chol(S, \"lower\"); el = maxabs(Ll * Ll' - Pl' * S * Pl);
         x = P * (R \\ (R' \\ (P' * b))); r = norm(S * x - b) / norm(b);
         R0 = chol(S); fill = nnz(R) < 0.5 * nnz(R0);"
    ));
    assert_eq!(scalar(&ev, "p"), 0.0);
    assert_eq!(scalar(&ev, "pv"), 0.0);
    assert_eq!(scalar(&ev, "pl"), 0.0);
    assert!(scalar(&ev, "e") < 1e-12, "{}", scalar(&ev, "e"));
    assert!(scalar(&ev, "ev") < 1e-12);
    assert!(scalar(&ev, "el") < 1e-12);
    assert_eq!(scalar(&ev, "same"), 1.0);
    assert!(scalar(&ev, "r") < 1e-12);
    assert_eq!(scalar(&ev, "fill"), 1.0);
}

#[test]
fn flag_form_on_an_indefinite_matrix() {
    let ev = eval(&format!(
        "{INDEFINITE}
         [R, p] = chol(S); q = p - 1;
         sz = size(R); rows = sz(1); cols = sz(2);
         K = R' * R - S; top = maxabs(K(1:q, :)); left = maxabs(K(:, 1:q));
         [R3, p3, P] = chol(S); q3 = p3 - 1;
         K3 = R3' * R3 - P' * S * P; top3 = maxabs(K3(1:q3, :));"
    ));
    let p = scalar(&ev, "p");
    assert!(p > 1.0 && p <= 4096.0, "p = {p}");
    assert_eq!(scalar(&ev, "rows"), p - 1.0);
    assert_eq!(scalar(&ev, "cols"), 4096.0);
    assert!(scalar(&ev, "top") < 1e-12);
    assert!(scalar(&ev, "left") < 1e-12);
    assert!(scalar(&ev, "p3") > 1.0);
    assert!(scalar(&ev, "top3") < 1e-12);
    let e = eval_err(&format!("{INDEFINITE} R = chol(S);"));
    assert!(e.contains("positive definite"), "{e}");
}

#[test]
fn indefinite_backslash_falls_back_to_lu() {
    let ev = eval(&format!(
        "{INDEFINITE}
         b = ones(N, 1); x = S \\ b; r = norm(S * x - b) / norm(b);"
    ));
    assert!(scalar(&ev, "r") < 1e-10, "{}", scalar(&ev, "r"));
}

#[test]
fn decomposition_is_reused() {
    let ev = eval(&format!(
        "{GRID}
         dS = decomposition(S, \"chol\");
         x1 = dS \\ b; x2 = dS \\ (2 * b);
         r1 = norm(S * x1 - b) / norm(b); r2 = norm(x2 - 2 * x1) / norm(x2);
         y = S \\ b; agree = norm(x1 - y) / norm(y);
         dA = decomposition(S); xa = dA \\ b; ra = norm(S * xa - b) / norm(b);
         dL = decomposition(S, \"chol\", \"lower\"); xl = dL \\ b;
         rl = norm(S * xl - b) / norm(b);"
    ));
    assert!(scalar(&ev, "r1") < 1e-13);
    assert!(scalar(&ev, "r2") < 1e-14);
    assert!(scalar(&ev, "agree") < 1e-13);
    assert!(scalar(&ev, "ra") < 1e-13);
    assert!(scalar(&ev, "rl") < 1e-13);
}

#[test]
fn two_dimensional_laplacian() {
    let ev = eval(
        "n = 120; N = n ^ 2; S = -1 * laplacian_2d(n, n); b = ones(N, 1);
         x = S \\ b; r = norm(S * x - b) / norm(b);
         [R, p, P] = chol(S); x2 = P * (R \\ (R' \\ (P' * b)));
         d = norm(x2 - x) / norm(x);",
    );
    assert!(scalar(&ev, "r") < 1e-12, "{}", scalar(&ev, "r"));
    assert_eq!(scalar(&ev, "p"), 0.0);
    assert!(scalar(&ev, "d") < 1e-12);
}

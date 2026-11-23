//! Sparse `\` and `spsolve`: nested-dissection LU for non-symmetric
//! systems and the MATLAB `n×1` result shape
//! (dev/plans/sparse_spd_solve.md, Phase 2).

use rustlab_script::{Evaluator, Value};

fn eval(src: &str) -> Evaluator {
    let src = format!("{src}\n");
    let tokens = rustlab_script::lexer::tokenize(&src).unwrap();
    let stmts = rustlab_script::parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    ev.run(&stmts)
        .unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"));
    ev
}

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name) {
        Some(Value::Scalar(x)) => *x,
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

/// Script lines that record `size(v)` as `v_r`, `v_c` for each name.
fn sizes(names: &[&str]) -> String {
    names
        .iter()
        .map(|v| format!("{v}_r = size({v}, 1); {v}_c = size({v}, 2);\n"))
        .collect()
}

fn shape(ev: &Evaluator, name: &str) -> (usize, usize) {
    (
        scalar(ev, &format!("{name}_r")) as usize,
        scalar(ev, &format!("{name}_c")) as usize,
    )
}

/// FDFD-style complex, non-symmetric operator with no ordering hint.
const STRETCHED: &str = "n = 30; N = n * n; L = laplacian_2d(n, n);
    s = 1 + 0.5j * linspace(0, 1, N);
    [I, J, V] = find(L);
    A = sparse(I, J, V(:) .* s(I(:)).', N, N) + 3 * speye(N);
    b = ones(N, 1) + 1j * (1:N)' / N;";

#[test]
fn non_symmetric_complex_solve_matches_the_amd_lu() {
    let ev = eval(&format!(
        "{STRETCHED}
         x = A \\ b;
         y = spsolve(A, b, \"lu\", \"amd\");
         r = norm(A * x - b) / norm(b);
         d = norm(x - y) / norm(y);"
    ));
    assert!(scalar(&ev, "r") < 1e-12, "residual {}", scalar(&ev, "r"));
    assert!(scalar(&ev, "d") < 1e-12, "vs AMD LU {}", scalar(&ev, "d"));
}

#[test]
fn every_path_returns_a_column() {
    let ev = eval(&format!(
        "{STRETCHED}
         x_lu = A \\ b;
         s_lu = spsolve(A, b);
         P = -1 * laplacian_2d(8, 8); p = ones(64, 1);
         x_spd = P \\ p;
         s_spd = spsolve(P, p, \"cholesky\");
         G = ones(8, 8); G(2:7, 2:7) = 0;
         [Q, q] = pin_dirichlet(P, p, G, 0.5);
         x_pin = Q \\ q;
         s_pin = spsolve(Q, q);
         s_row = spsolve(P, ones(1, 64));
         s_vec = spsolve(P, linspace(1, 2, 64));
         x_real = A \\ real(b);
         {}",
        sizes(&["x_lu", "s_lu", "x_spd", "s_spd", "x_pin", "s_pin", "s_row", "s_vec", "x_real"])
    ));
    assert_eq!(shape(&ev, "x_lu"), (900, 1));
    assert_eq!(shape(&ev, "s_lu"), (900, 1));
    for v in ["x_spd", "s_spd", "x_pin", "s_pin", "s_row", "s_vec"] {
        assert_eq!(shape(&ev, v), (64, 1), "{v}");
    }
    assert_eq!(shape(&ev, "x_real"), (900, 1));
}

#[test]
fn column_result_composes_like_matlab() {
    // The 0.3.85 1-D result made `A * x` an error ("matrix by row vector").
    let ev = eval(
        "A = -1 * laplacian_2d(10, 10); b = (1:100)';
         x = A \\ b; r = norm(A * x - b) / norm(b);
         y = spsolve(A, b); s = norm(A * y - b) / norm(b);
         t = size([x, y], 2); u = size(x', 1);",
    );
    assert!(scalar(&ev, "r") < 1e-12);
    assert!(scalar(&ev, "s") < 1e-12);
    assert_eq!(scalar(&ev, "t"), 2.0);
    assert_eq!(scalar(&ev, "u"), 1.0);
}

#[test]
fn one_by_one_system_is_a_scalar() {
    let ev = eval("x = sparse(4) \\ 2; y = spsolve(sparse(4), 2);");
    assert_eq!(scalar(&ev, "x"), 0.5);
    assert_eq!(scalar(&ev, "y"), 0.5);
}

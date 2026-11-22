//! `pin_dirichlet` scales each pinned row to the operator's own diagonal.
//!
//! An identity pinned row next to stencil entries of size `ε/h²` loses the
//! LU pivot in its column at small `h`, and the pinned potentials come back
//! off by ~1e-7 at `h = 1e-4` (`rustlab_em`
//! `dev/rustlab/requests/pin-dirichlet-row-scaling.md`). With the default
//! `"scale", "auto"` the pinned row is `d_k·e_k` and `b(k) = d_k·value`.

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

fn eval_err(src: &str) -> String {
    let src = format!("{src}\n");
    let tokens = rustlab_script::lexer::tokenize(&src).unwrap();
    let stmts = rustlab_script::parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    match ev.run(&stmts) {
        Err(e) => e.to_string(),
        Ok(()) => panic!("expected an error\n{src}"),
    }
}

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name) {
        Some(Value::Scalar(x)) => *x,
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

/// The request's repro: a square conductor at 1 V inside a grounded box,
/// operator `-laplacian_2d(n, n, h, h)`.
fn capacitor(h: &str, scale: &str) -> String {
    format!(
        "n = 41; h = {h}; M = zeros(n, n); M(15:25, 15:25) = 1;
         G = ones(n, n); G(2:n-1, 2:n-1) = 0;
         A = -1 * laplacian_2d(n, n, h, h); b = zeros(n*n, 1);
         [A, b] = pin_dirichlet(A, b, M, 1.0{scale});
         [A, b] = pin_dirichlet(A, b, G, 0.0{scale});
         V = reshape(spsolve(A, b), n, n);
         pin_err = max(max(abs(V .* M - M)));
         gnd_err = max(max(abs(V .* G)));
         mid = V(20, 8);"
    )
}

#[test]
fn pinned_nodes_are_exact_at_small_spacing() {
    for h in ["1", "1e-2", "1e-4", "1e-6"] {
        let ev = eval(&capacitor(h, ""));
        let pin = scalar(&ev, "pin_err");
        let gnd = scalar(&ev, "gnd_err");
        assert!(
            pin <= 1e-14,
            "h = {h}: max |V - 1| on the conductor = {pin:e}"
        );
        assert!(gnd <= 1e-14, "h = {h}: max |V| on the ground = {gnd:e}");
    }
}

#[test]
fn interior_solution_does_not_depend_on_spacing() {
    // Laplace's equation on a uniform grid is scale-free, so the interior
    // potential must agree between h = 1 and h = 1e-4.
    let a = scalar(&eval(&capacitor("1", "")), "mid");
    let b = scalar(&eval(&capacitor("1e-4", "")), "mid");
    assert!(a > 0.0 && a < 1.0, "mid = {a}");
    assert!((a - b).abs() <= 1e-13, "h = 1: {a}, h = 1e-4: {b}");
}

#[test]
fn identity_rows_still_available_and_lose_accuracy() {
    // `"scale", 1` keeps the 0.3.68 identity rows; the request's repro then
    // shows the pivot-loss error. This pins the reason for the default.
    let ev = eval(&capacitor("1e-4", ", \"scale\", 1"));
    let pin = scalar(&ev, "pin_err");
    assert!(pin > 1e-10, "identity rows unexpectedly exact: {pin:e}");
}

#[test]
fn auto_scale_uses_the_original_diagonal() {
    let ev = eval(
        "h = 1e-3; A = -1 * laplacian_2d(3, 3, h, h); b = zeros(9, 1);
         mask = zeros(3, 3); mask(2, 2) = 1; k = ij2k(2, 2, 3);
         d0 = full(A)(k, k);
         [A2, b2] = pin_dirichlet(A, b, mask, 5.0);
         D = full(A2); dk = D(k, k); off = sum(abs(D(k, :))) - abs(D(k, k));
         rk = b2(k); other = D(1, 1);",
    );
    let d0 = scalar(&ev, "d0");
    assert!((d0 - 4e6).abs() < 1e-6, "d0 = {d0}");
    assert_eq!(scalar(&ev, "dk"), d0.abs());
    assert_eq!(scalar(&ev, "off"), 0.0);
    assert_eq!(scalar(&ev, "rk"), 5.0 * d0.abs());
    assert_eq!(scalar(&ev, "other"), d0, "unpinned rows are untouched");
}

#[test]
fn negative_diagonal_gets_a_positive_scale() {
    // laplacian_2d itself has a negative diagonal (-4/h²); the pinned row
    // uses its magnitude, so the pinned value keeps its sign.
    let ev = eval(
        "A = laplacian_1d(5, 0.5); b = zeros(1, 5);
         [A2, b2] = pin_dirichlet(A, b, [1, 5], [2.0, -3.0]);
         D = full(A2); d1 = D(1, 1); r1 = b2(1); r5 = b2(5);
         v = spsolve(A2, b2); v1 = v(1); v3 = v(3); v5 = v(5);",
    );
    assert_eq!(scalar(&ev, "d1"), 8.0);
    assert_eq!(scalar(&ev, "r1"), 16.0);
    assert_eq!(scalar(&ev, "r5"), -24.0);
    assert_eq!(scalar(&ev, "v1"), 2.0);
    assert_eq!(scalar(&ev, "v5"), -3.0);
    assert!((scalar(&ev, "v3") - (-0.5)).abs() < 1e-12);
}

#[test]
fn zero_diagonal_falls_back_to_row_max_then_one() {
    let ev = eval(
        "A = [0, 3, -7; 1, 2, 1; 0, 0, 0]; b = [1; 1; 1];
         [A2, b2] = pin_dirichlet(A, b, [1, 3], [2.0, 4.0]);
         a11 = A2(1, 1); a12 = A2(1, 2); r1 = b2(1); a33 = A2(3, 3); r3 = b2(3);",
    );
    assert_eq!(scalar(&ev, "a11"), 7.0);
    assert_eq!(scalar(&ev, "a12"), 0.0);
    assert_eq!(scalar(&ev, "r1"), 14.0);
    assert_eq!(scalar(&ev, "a33"), 1.0);
    assert_eq!(scalar(&ev, "r3"), 4.0);
}

#[test]
fn explicit_numeric_scale() {
    let ev = eval(
        "A = laplacian_1d(4); b = zeros(1, 4);
         [A2, b2] = pin_dirichlet(A, b, [2], 3.0, \"scale\", 10);
         d = full(A2)(2, 2); r = b2(2);",
    );
    assert_eq!(scalar(&ev, "d"), 10.0);
    assert_eq!(scalar(&ev, "r"), 30.0);
}

#[test]
fn scale_option_errors() {
    let base = "A = speye(3); b = zeros(1, 3);";
    let e = eval_err(&format!(
        "{base} [X, y] = pin_dirichlet(A, b, [1], 1.0, \"scale\", 0);"
    ));
    assert!(e.contains("scale"), "{e}");
    let e = eval_err(&format!(
        "{base} [X, y] = pin_dirichlet(A, b, [1], 1.0, \"scale\", \"big\");"
    ));
    assert!(e.contains("auto"), "{e}");
    let e = eval_err(&format!(
        "{base} [X, y] = pin_dirichlet(A, b, [1], 1.0, \"size\", 1);"
    ));
    assert!(e.contains("scale"), "{e}");
}

#[test]
fn complex_diagonal_uses_its_magnitude() {
    let ev = eval(
        "A = [3 + 4j, 1; 1, 2]; b = [0; 0];
         [A2, b2] = pin_dirichlet(A, b, [1], 2.0);
         d = A2(1, 1); r = b2(1);",
    );
    assert_eq!(scalar(&ev, "d"), 5.0);
    assert_eq!(scalar(&ev, "r"), 10.0);
}

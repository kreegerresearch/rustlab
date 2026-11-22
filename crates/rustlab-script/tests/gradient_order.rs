//! 1-D `gradient` endpoint order.
//!
//! The default endpoints are first-order (MATLAB): `(f(2) - f(1)) / h`.
//! `"order", 2` uses three-point one-sided endpoints, the order the 2-D and
//! 3-D stencils already use (`rustlab_em`
//! `lesson-review-findings-2026-11-19.md` §2).

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

fn reals(ev: &Evaluator, name: &str) -> Vec<f64> {
    match ev.get(name) {
        Some(Value::Scalar(x)) => vec![*x],
        Some(Value::Vector(v)) => v.iter().map(|c| c.re).collect(),
        Some(Value::Matrix(m)) => m.iter().map(|c| c.re).collect(),
        other => panic!("{name}: expected a real array, got {other:?}"),
    }
}

fn assert_close(got: &[f64], want: &[f64], tol: f64) {
    assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() <= tol, "{got:?} vs {want:?}");
    }
}

const XS: &str = "x = [0.4, 0.6, 0.8, 1.0, 1.4, 2.0]; y = x.^2;";

#[test]
fn default_endpoints_stay_first_order() {
    // The request's numbers: interior exact, ends 1.0 and 3.4 (exact 0.8, 4.0).
    let ev = eval(&format!(
        "{XS} g = gradient(y, x); g1 = gradient(y, x, \"order\", 1);"
    ));
    assert_close(&reals(&ev, "g"), &[1.0, 1.2, 1.6, 2.0, 2.8, 3.4], 1e-12);
    assert_eq!(reals(&ev, "g"), reals(&ev, "g1"));
}

#[test]
fn second_order_is_exact_for_a_quadratic_on_a_nonuniform_grid() {
    let ev = eval(&format!("{XS} g = gradient(y, x, \"order\", 2);"));
    assert_close(&reals(&ev, "g"), &[0.8, 1.2, 1.6, 2.0, 2.8, 4.0], 1e-12);
}

#[test]
fn second_order_with_uniform_spacing() {
    // y = 3x² - x + 2, h = 0.25: dy/dx = 6x - 1 exactly at every sample.
    let ev = eval(
        "h = 0.25; x = 0:h:2; y = 3*x.^2 - x + 2;
         g = gradient(y, h, \"order\", 2); e = 6*x - 1;
         u = gradient(y, \"order\", 2); eu = (6*x - 1) * h;",
    );
    assert_close(&reals(&ev, "g"), &reals(&ev, "e"), 1e-12);
    assert_close(&reals(&ev, "u"), &reals(&ev, "eu"), 1e-12);
}

#[test]
fn endpoint_error_converges_at_the_requested_order() {
    // d/dx exp at x = 0 and x = 1 (f'' is nonzero at both ends). Halving h
    // divides the endpoint error by about 2 for order 1 and 4 for order 2.
    let err = |n: usize, order: u32| -> (f64, f64) {
        let ev = eval(&format!(
            "x = linspace(0, 1, {n}); y = exp(x); g = gradient(y, x, \"order\", {order});
             a = abs(g(1) - 1); b = abs(g(end) - exp(1));"
        ));
        (reals(&ev, "a")[0], reals(&ev, "b")[0])
    };
    for order in [1, 2] {
        let (a1, b1) = err(21, order);
        let (a2, b2) = err(41, order);
        let (ra, rb) = (a1 / a2, b1 / b2);
        let want = if order == 1 { 2.0 } else { 4.0 };
        assert!((ra - want).abs() < 0.3, "order {order}: left ratio {ra}");
        assert!((rb - want).abs() < 0.3, "order {order}: right ratio {rb}");
    }
    let (a1, _) = err(21, 1);
    let (a2, _) = err(21, 2);
    assert!(a2 < a1 / 10.0, "order 2 endpoint {a2} vs order 1 {a1}");
}

#[test]
fn second_order_keeps_orientation_and_complex_values() {
    let ev = eval(
        "c = [0; 1; 4; 9]; g = gradient(c, \"order\", 2);
         z = (0:4) * (1 + 2j); w = gradient(z.^2, \"order\", 2); wi = imag(w);",
    );
    assert!(matches!(ev.get("g"), Some(Value::Matrix(m)) if m.dim() == (4, 1)));
    assert_close(&reals(&ev, "g"), &[0.0, 2.0, 4.0, 6.0], 1e-12);
    // (k(1+2j))² = k²(−3 + 4j); derivative in k is 2k(−3 + 4j).
    assert_close(&reals(&ev, "w"), &[0.0, -6.0, -12.0, -18.0, -24.0], 1e-12);
    assert_close(&reals(&ev, "wi"), &[0.0, 8.0, 16.0, 24.0, 32.0], 1e-12);
}

#[test]
fn order_option_errors() {
    assert!(eval_err("g = gradient([1, 2, 3], \"order\", 3);").contains("order"));
    assert!(eval_err("g = gradient([1, 2], \"order\", 2);").contains("3"));
    assert!(eval_err("g = gradient([1, 2, 3], \"degree\", 2);").contains("order"));
    let e = eval_err("[a, b] = gradient(ones(3, 3), \"order\", 1);");
    assert!(e.contains("second-order"), "{e}");
}

#[test]
fn two_d_accepts_order_two_unchanged() {
    let ev = eval(
        "F = [1, 5, 2, 8; 3, 9, 4, 1; 7, 2, 6, 3; 4, 8, 1, 5]; [a, b] = gradient(F); [c, d] = gradient(F, \"order\", 2);
         e1 = max(max(abs(a - c))); e2 = max(max(abs(b - d)));",
    );
    assert_eq!(reals(&ev, "e1"), vec![0.0]);
    assert_eq!(reals(&ev, "e2"), vec![0.0]);
}

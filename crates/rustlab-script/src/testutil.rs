//! Shared helpers for script-level math tests.
//!
//! One evaluator, one tolerance check, and one shape reader. The audit
//! modules call these instead of copying the harness.

use crate::eval::value::Value;
use crate::{lexer, parser, Evaluator};
use num_complex::Complex64;

pub(crate) fn eval(src: &str) -> Evaluator {
    let src = if src.ends_with('\n') {
        src.to_string()
    } else {
        format!("{src}\n")
    };
    let tokens = lexer::tokenize(&src).unwrap();
    let stmts = parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    for stmt in &stmts {
        ev.exec_stmt(stmt).unwrap();
    }
    ev
}

pub(crate) fn eval_err(src: &str) -> String {
    let src = if src.ends_with('\n') {
        src.to_string()
    } else {
        format!("{src}\n")
    };
    let tokens = lexer::tokenize(&src).unwrap();
    let stmts = parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    for stmt in &stmts {
        if let Err(e) = ev.exec_stmt(stmt) {
            return e.to_string();
        }
    }
    panic!("expected an error from: {src}");
}

pub(crate) fn num(ev: &Evaluator, name: &str) -> Complex64 {
    match ev.get(name).unwrap() {
        Value::Scalar(x) => Complex64::new(*x, 0.0),
        Value::Complex(c) => *c,
        Value::Vector(v) if v.len() == 1 => v[0],
        Value::Matrix(m) if m.nrows() == 1 && m.ncols() == 1 => m[[0, 0]],
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

pub(crate) fn re(ev: &Evaluator, name: &str) -> f64 {
    let z = num(ev, name);
    assert!(z.im.abs() < 1e-9, "{name} imag {}", z.im);
    z.re
}

pub(crate) fn vec_c(ev: &Evaluator, name: &str) -> Vec<Complex64> {
    match ev.get(name).unwrap() {
        Value::Vector(v) => v.iter().copied().collect(),
        Value::Matrix(m) if m.nrows() == 1 || m.ncols() == 1 => m.iter().copied().collect(),
        other => panic!("{name}: expected a vector, got {other:?}"),
    }
}

pub(crate) fn vec_re(ev: &Evaluator, name: &str) -> Vec<f64> {
    vec_c(ev, name).into_iter().map(|z| z.re).collect()
}

pub(crate) fn mat(ev: &Evaluator, name: &str) -> Vec<Vec<Complex64>> {
    match ev.get(name).unwrap() {
        Value::Matrix(m) => (0..m.nrows())
            .map(|r| (0..m.ncols()).map(|c| m[[r, c]]).collect())
            .collect(),
        other => panic!("{name}: expected a matrix, got {other:?}"),
    }
}

pub(crate) fn mat_re(ev: &Evaluator, name: &str) -> Vec<Vec<f64>> {
    mat(ev, name)
        .into_iter()
        .map(|row| row.into_iter().map(|z| z.re).collect())
        .collect()
}

/// `(rows, cols)`. A vector is a row. A scalar is 1×1.
pub(crate) fn shape(ev: &Evaluator, name: &str) -> (usize, usize) {
    match ev.get(name).unwrap() {
        Value::Matrix(m) => m.dim(),
        Value::Vector(v) => (1, v.len()),
        Value::Scalar(_) | Value::Complex(_) => (1, 1),
        other => panic!("{name}: no shape, got {other:?}"),
    }
}

pub(crate) fn matrix_dim(ev: &Evaluator, name: &str) -> (usize, usize) {
    match ev.get(name).unwrap() {
        Value::Matrix(m) => m.dim(),
        other => panic!("{name}: expected a matrix, got {other:?}"),
    }
}

pub(crate) fn assert_c(got: Complex64, want: Complex64, tol: f64) {
    let d = (got - want).norm();
    assert!(d <= tol, "got {got}, want {want}, |diff| {d:e}");
}

pub(crate) fn assert_re(ev: &Evaluator, name: &str, want: f64, tol: f64) {
    let g = re(ev, name);
    assert!((g - want).abs() <= tol, "{name} = {g}, want {want}");
}

pub(crate) fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}

/// Each script binds one name. `want` is a closed form or a MATLAB value.
pub(crate) fn check_re(cases: &[(&str, &str, f64, f64)]) {
    for (src, name, want, tol) in cases {
        assert_re(&eval(src), name, *want, *tol);
    }
}

/// Each script must fail, and the message must contain `needle`.
pub(crate) fn check_err(cases: &[(&str, &str)]) {
    for (src, needle) in cases {
        let err = eval_err(src);
        assert!(
            err.contains(needle),
            "{src}\n  error: {err}\n  missing: {needle}"
        );
    }
}

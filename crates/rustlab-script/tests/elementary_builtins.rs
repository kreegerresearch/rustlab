//! `tan`, `cot`, `sec`, `csc`, `asinh`, `acosh`, `atanh`, `isnan`, `isinf`,
//! `isfinite`.
//!
//! Reference values are MATLAB's: `acosh(0.5)` is `0 + 1.0472i`,
//! `atanh(2)` is `0.5493 + 1.5708i`, `cot(0)` is `Inf`, and a complex
//! number is NaN (or Inf) when either part is.

use num_complex::Complex;
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
        other => panic!("{name}: expected a real scalar, got {other:?}"),
    }
}

fn complex(ev: &Evaluator, name: &str) -> Complex<f64> {
    match ev.get(name) {
        Some(Value::Complex(c)) => *c,
        other => panic!("{name}: expected a complex scalar, got {other:?}"),
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

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * (1.0 + b.abs())
}

fn assert_close_all(got: &[f64], expect: &[f64]) {
    assert_eq!(got.len(), expect.len(), "{got:?} vs {expect:?}");
    for (g, e) in got.iter().zip(expect) {
        assert!(close(*g, *e), "{got:?} vs {expect:?}");
    }
}

#[test]
fn tan_matches_sin_over_cos() {
    let ev = eval("x = [-1.2, -0.3, 0, 0.5, 1.1]; t = tan(x); r = sin(x) ./ cos(x); s = tan(0.5);");
    assert_close_all(&reals(&ev, "t"), &reals(&ev, "r"));
    assert!(close(scalar(&ev, "s"), 0.5f64.tan()));
}

#[test]
fn reciprocal_trig_functions() {
    let ev = eval("x = [0.3, 0.7, 1.9]; a = cot(x); b = sec(x); c = csc(x);");
    let x = [0.3f64, 0.7, 1.9];
    assert_close_all(&reals(&ev, "a"), &x.map(|v| 1.0 / v.tan()));
    assert_close_all(&reals(&ev, "b"), &x.map(|v| 1.0 / v.cos()));
    assert_close_all(&reals(&ev, "c"), &x.map(|v| 1.0 / v.sin()));
}

#[test]
fn reciprocal_trig_poles_are_infinite() {
    // MATLAB: cot(0) = Inf, csc(0) = Inf, sec(0) = 1.
    let ev = eval("a = cot(0); b = csc(0); c = sec(0);");
    assert_eq!(scalar(&ev, "a"), f64::INFINITY);
    assert_eq!(scalar(&ev, "b"), f64::INFINITY);
    assert_eq!(scalar(&ev, "c"), 1.0);
}

#[test]
fn tan_of_complex_is_complex() {
    let ev = eval("z = tan(1 + 2j);");
    let want = Complex::new(1.0, 2.0).tan();
    let got = complex(&ev, "z");
    assert!(close(got.re, want.re) && close(got.im, want.im));
}

#[test]
fn inverse_hyperbolics_on_their_real_domains() {
    let ev =
        eval("a = asinh([-2, 0, 0.5, 3]); b = acosh([1, 1.5, 10]); c = atanh([-0.5, 0, 0.9]);");
    assert_close_all(&reals(&ev, "a"), &[-2.0f64, 0.0, 0.5, 3.0].map(f64::asinh));
    assert_close_all(&reals(&ev, "b"), &[1.0f64, 1.5, 10.0].map(f64::acosh));
    assert_close_all(&reals(&ev, "c"), &[-0.5f64, 0.0, 0.9].map(f64::atanh));
}

#[test]
fn acosh_round_trips_cosh() {
    let ev = eval("x = [1, 1.25, 4, 20]; y = acosh(cosh(x));");
    assert_close_all(&reals(&ev, "y"), &[1.0, 1.25, 4.0, 20.0]);
}

#[test]
fn acosh_below_one_is_complex_like_matlab() {
    // MATLAB: acosh(0.5) = 0 + 1.0472i, acosh(-2) = 1.3170 + 3.1416i.
    let ev = eval("a = acosh(0.5); b = acosh(-2);");
    let a = complex(&ev, "a");
    assert!(close(a.re, 0.0) && close(a.im, (0.5f64).acos()), "{a}");
    let b = complex(&ev, "b");
    assert!(
        close(b.re, 1.316957896924817) && close(b.im, std::f64::consts::PI),
        "{b}"
    );
}

#[test]
fn atanh_outside_unit_interval_is_complex_like_matlab() {
    // MATLAB: atanh(2) = 0.5493 + 1.5708i, atanh(-2) = -0.5493 + 1.5708i.
    let ev = eval("a = atanh(2); b = atanh(-2); c = atanh(1); d = atanh(-1);");
    let a = complex(&ev, "a");
    assert!(
        close(a.re, 0.549_306_144_334_054_9) && close(a.im, std::f64::consts::FRAC_PI_2),
        "{a}"
    );
    let b = complex(&ev, "b");
    assert!(
        close(b.re, -0.549_306_144_334_054_9) && close(b.im, std::f64::consts::FRAC_PI_2),
        "{b}"
    );
    assert_eq!(scalar(&ev, "c"), f64::INFINITY);
    assert_eq!(scalar(&ev, "d"), f64::NEG_INFINITY);
}

#[test]
fn twin_wire_impedance_with_acosh() {
    // Z0 = (eta0 / pi) * acosh(d / (2a)); the lessons' workaround formula.
    let ev = eval("r = 3.7; a = acosh(r); b = log(r + sqrt(r * r - 1));");
    assert!(close(scalar(&ev, "a"), scalar(&ev, "b")));
}

fn logical_flags(ev: &Evaluator, name: &str) -> (usize, usize, Vec<bool>) {
    match ev.get(name) {
        Some(Value::Logical(a)) => (a.rows(), a.cols(), a.to_flags()),
        other => panic!("{name}: expected a logical array, got {other:?}"),
    }
}

fn bool_of(ev: &Evaluator, name: &str) -> bool {
    match ev.get(name) {
        Some(Value::Bool(b)) => *b,
        other => panic!("{name}: expected a bool, got {other:?}"),
    }
}

#[test]
fn isnan_isinf_isfinite_on_a_row() {
    let ev = eval("v = [1, NaN, Inf, -Inf, 0]; a = isnan(v); b = isinf(v); c = isfinite(v);");
    assert_eq!(
        logical_flags(&ev, "a"),
        (1, 5, vec![false, true, false, false, false])
    );
    assert_eq!(
        logical_flags(&ev, "b"),
        (1, 5, vec![false, false, true, true, false])
    );
    assert_eq!(
        logical_flags(&ev, "c"),
        (1, 5, vec![true, false, false, false, true])
    );
}

#[test]
fn predicates_keep_matrix_shape() {
    let ev = eval("M = [1, NaN, 3; Inf, 5, NaN]; a = isnan(M); n = sum(sum(a)); c = isfinite(M');");
    assert_eq!(
        logical_flags(&ev, "a"),
        (2, 3, vec![false, true, false, false, false, true])
    );
    assert_eq!(scalar(&ev, "n"), 2.0);
    let (r, c, _) = logical_flags(&ev, "c");
    assert_eq!((r, c), (3, 2));
}

#[test]
fn predicates_on_scalars_are_bools() {
    let ev =
        eval("a = isnan(NaN); b = isnan(1); c = isinf(-Inf); d = isfinite(Inf); e = isfinite(2);");
    assert!(bool_of(&ev, "a"));
    assert!(!bool_of(&ev, "b"));
    assert!(bool_of(&ev, "c"));
    assert!(!bool_of(&ev, "d"));
    assert!(bool_of(&ev, "e"));
}

#[test]
fn complex_predicates_check_both_parts() {
    // MATLAB: isnan(complex(1, NaN)) and isinf(complex(Inf, 1)) are true.
    let ev =
        eval("z = [1 + NaN*1j, Inf + 1j, 2 + 3j]; a = isnan(z); b = isinf(z); c = isfinite(z);");
    assert_eq!(logical_flags(&ev, "a").2, vec![true, false, false]);
    assert_eq!(logical_flags(&ev, "b").2, vec![false, true, false]);
    assert_eq!(logical_flags(&ev, "c").2, vec![false, false, true]);
}

#[test]
fn isnan_masks_index_and_replace() {
    let ev = eval("v = [4, NaN, 6, NaN]; v(isnan(v)) = 0; k = find(isnan([NaN, 1, NaN]));");
    assert_eq!(reals(&ev, "v"), vec![4.0, 0.0, 6.0, 0.0]);
    assert_eq!(reals(&ev, "k"), vec![1.0, 3.0]);
}

#[test]
fn integers_are_never_nan_or_inf() {
    let ev = eval("a = isnan(int32(5)); b = isfinite(int8([1, 2]));");
    assert!(!bool_of(&ev, "a"));
    assert_eq!(logical_flags(&ev, "b").2, vec![true, true]);
}

#[test]
fn predicates_reject_strings_and_wrong_arity() {
    assert!(eval_err("isnan(\"a\")").contains("isnan"));
    assert!(eval_err("isinf(1, 2)").contains("isinf"));
}

//! A stored complex sample keeps its imaginary part, however small.
//!
//! `abs` of a vector uses the modulus of each element. A reduction such as
//! `sum` still drops an imaginary part below `1e-12`.

use rustlab_script::{Evaluator, Value};

fn parse(src: &str) -> Vec<rustlab_script::ast::Stmt> {
    let tokens = rustlab_script::lexer::tokenize(src).unwrap();
    rustlab_script::parser::parse(tokens).unwrap()
}

fn eval(src: &str) -> Evaluator {
    let mut ev = Evaluator::new();
    ev.run(&parse(src))
        .unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"));
    ev
}

fn complex(ev: &Evaluator, name: &str) -> (f64, f64) {
    match ev.get(name) {
        Some(Value::Complex(c)) => (c.re, c.im),
        other => panic!("{name}: expected complex, got {other:?}"),
    }
}

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name) {
        Some(Value::Scalar(x)) => *x,
        other => panic!("{name}: expected scalar, got {other:?}"),
    }
}

fn vec_re(ev: &Evaluator, name: &str) -> Vec<f64> {
    match ev.get(name) {
        Some(Value::Vector(v)) => v.iter().map(|c| c.re).collect(),
        other => panic!("{name}: expected vector, got {other:?}"),
    }
}

#[test]
fn one_element_keeps_a_tiny_imaginary_part() {
    let ev = eval(
        r#"
        v = [1e-15j, 1 + 1e-18j, -2e-20j, 1e-15 + 1e-15j]
        a = v(1); b = v(2); c = v(3); d = v(4)
        M = [1e-15j, 2; 3, 1e-15 + 1e-15j]
        m11 = M(1, 1); m22 = M(2, 2); mlin = M(4)
        T = zeros3(1, 1, 1)
        T(1, 1, 1) = -2e-20j
        t = T(1, 1, 1)
        s = squeeze(T)
        det1 = det(diag([1e-15 + 1e-15j]))
        "#,
    );
    assert_eq!(complex(&ev, "a"), (0.0, 1e-15));
    assert_eq!(complex(&ev, "b"), (1.0, 1e-18));
    assert_eq!(complex(&ev, "c"), (0.0, -2e-20));
    assert_eq!(complex(&ev, "d"), (1e-15, 1e-15));
    assert_eq!(complex(&ev, "m11"), (0.0, 1e-15));
    assert_eq!(complex(&ev, "m22"), (1e-15, 1e-15));
    assert_eq!(complex(&ev, "mlin"), (1e-15, 1e-15));
    assert_eq!(complex(&ev, "t"), (0.0, -2e-20));
    assert_eq!(complex(&ev, "s"), (0.0, -2e-20));
    assert_eq!(complex(&ev, "det1"), (1e-15, 1e-15));
}

#[test]
fn abs_and_angle_of_a_tiny_phasor_keep_magnitude_and_phase() {
    let ev = eval(
        r#"
        v = [1e-15j, 1 + 1e-18j, -2e-20j, 1e-15 + 1e-15j]
        m = abs(v)
        p = angle(v)
        M = [1e-15j, 1 + 1e-18j; 0, -2e-20j]
        mm = abs(M)
        one = abs(v(1))
        "#,
    );
    let m = vec_re(&ev, "m");
    let p = vec_re(&ev, "p");
    let half = std::f64::consts::FRAC_PI_2;
    let quarter = std::f64::consts::FRAC_PI_4;
    assert!((m[0] - 1e-15).abs() <= 1e-15 * 1e-12);
    assert!((p[0] - half).abs() < 1e-12);
    let hyp = (1.0_f64).hypot(1e-18);
    assert!((m[1] - hyp).abs() <= hyp * 1e-12);
    assert!((p[1] - 1e-18_f64.atan2(1.0)).abs() < 1e-18);
    assert!((m[2] - 2e-20).abs() <= 2e-20 * 1e-12);
    assert!((p[2] + half).abs() < 1e-12);
    assert!((m[3] - (1e-15_f64).hypot(1e-15)).abs() <= 1e-15 * 1e-12);
    assert!((p[3] - quarter).abs() < 1e-12);
    match ev.get("mm") {
        Some(Value::Matrix(a)) => {
            assert!((a[[0, 0]].re - 1e-15).abs() <= 1e-15 * 1e-12);
            assert!((a[[0, 1]].re - hyp).abs() <= hyp * 1e-12);
            assert!((a[[1, 1]].re - 2e-20).abs() <= 2e-20 * 1e-12);
        }
        other => panic!("mm: {other:?}"),
    }
    assert!((scalar(&ev, "one") - 1e-15).abs() <= 1e-15 * 1e-12);
}

#[test]
fn an_exact_zero_imaginary_part_stays_real() {
    let ev = eval(
        r#"
        r = [1, -2, 0]
        a = abs(r)
        e = r(2)
        w = 1:3
        b = abs(w)
        f = w(2)
        n = 0
        n = -n
        z = [1 + n * j]
        g = z(1)
        "#,
    );
    assert_eq!(vec_re(&ev, "a"), vec![1.0, 2.0, 0.0]);
    assert_eq!(scalar(&ev, "e"), -2.0);
    assert_eq!(vec_re(&ev, "b"), vec![1.0, 2.0, 3.0]);
    assert_eq!(scalar(&ev, "f"), 2.0);
    // `-0` times `j` is an imaginary part of `-0`, which is zero.
    assert_eq!(scalar(&ev, "g"), 1.0);
}

#[test]
fn a_vector_of_tiny_pure_imaginary_values_uses_the_complex_formula() {
    let ev = eval("s = sqrt([1e-15j])");
    let v = match ev.get("s") {
        Some(Value::Vector(v)) => v.clone(),
        other => panic!("s: {other:?}"),
    };
    let want = 1e-15_f64.sqrt();
    let half = std::f64::consts::FRAC_1_SQRT_2;
    assert!((v[0].re - want * half).abs() <= want * 1e-12);
    assert!((v[0].im - want * half).abs() <= want * 1e-12);
}

#[test]
fn a_reduction_still_drops_imaginary_roundoff() {
    // The sum is a reduction. An imaginary part of the result below 1e-12
    // is dropped, including when the input is one stored sample.
    let ev = eval("s = sum([1e-13j])");
    assert_eq!(scalar(&ev, "s"), 0.0);
}

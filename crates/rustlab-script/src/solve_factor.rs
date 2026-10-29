//! `A\b`, `qr`, dense `lu`, `polyfit`, `polyval`, `interp1`, `null`, `orth`.
//!
//! References are small closed forms. Properties (orthonormal Q, `P A = L U`,
//! a least-squares residual orthogonal to the columns) guard the factorizations
//! when a single numeric answer would not.

use crate::eval::value::Value;
use crate::{lexer, parser, Evaluator};
use num_complex::Complex64;

fn eval(src: &str) -> Evaluator {
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

fn eval_err(src: &str) -> String {
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

fn re(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name).unwrap() {
        Value::Scalar(x) => *x,
        Value::Complex(z) => {
            assert!(z.im.abs() < 1e-9, "{name} imag {}", z.im);
            z.re
        }
        Value::Vector(v) if v.len() == 1 => {
            assert!(v[0].im.abs() < 1e-9);
            v[0].re
        }
        Value::Matrix(m) if m.len() == 1 => {
            assert!(m[[0, 0]].im.abs() < 1e-9);
            m[[0, 0]].re
        }
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

fn vec_re(ev: &Evaluator, name: &str) -> Vec<f64> {
    match ev.get(name).unwrap() {
        Value::Vector(v) => v.iter().map(|z| z.re).collect(),
        Value::Matrix(m) if m.nrows() == 1 || m.ncols() == 1 => m.iter().map(|z| z.re).collect(),
        other => panic!("{name}: expected a vector, got {other:?}"),
    }
}

fn mat_re(ev: &Evaluator, name: &str) -> Vec<Vec<f64>> {
    match ev.get(name).unwrap() {
        Value::Matrix(m) => (0..m.nrows())
            .map(|i| (0..m.ncols()).map(|j| m[[i, j]].re).collect())
            .collect(),
        other => panic!("{name}: expected a matrix, got {other:?}"),
    }
}

fn shape(ev: &Evaluator, name: &str) -> (usize, usize) {
    match ev.get(name).unwrap() {
        Value::Matrix(m) => m.dim(),
        Value::Vector(v) => (1, v.len()),
        Value::Scalar(_) | Value::Complex(_) => (1, 1),
        other => panic!("{name}: no shape, got {other:?}"),
    }
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}

#[test]
fn backslash_square_known_solution() {
    let ev = eval(
        "A = [2, 1; 1, 3];
         x = A \\ [5; 10];
         y = mldivide(A, [5; 10]);
         M = A \\ [5, 6; 10, 13];
         s = speye(2) \\ [3; 4];",
    );
    let x = vec_re(&ev, "x");
    close(x[0], 1.0);
    close(x[1], 3.0);
    assert_eq!(shape(&ev, "x"), (2, 1));
    let y = vec_re(&ev, "y");
    close(y[0], 1.0);
    close(y[1], 3.0);
    let m = mat_re(&ev, "M");
    close(m[0][0], 1.0);
    close(m[0][1], 1.0);
    close(m[1][0], 3.0);
    close(m[1][1], 4.0);
    let s = vec_re(&ev, "s");
    close(s[0], 3.0);
    close(s[1], 4.0);
}

#[test]
fn backslash_scalar_is_elementwise() {
    let ev = eval("y = 2 \\ [2, 4, 6];");
    let y = vec_re(&ev, "y");
    assert_eq!(y, vec![1.0, 2.0, 3.0]);
}

#[test]
fn backslash_row_count_mismatch_names_the_column() {
    let err = eval_err("A = [1, 0; 0, 1]; x = A \\ [1, 2];");
    assert!(err.contains("b'"), "{err}");
}

#[test]
fn least_squares_residual_orthogonal_to_columns() {
    let ev = eval(
        "A = [1, 1; 1, 2; 1, 3; 1, 4];
         b = [1; 2; 2; 3];
         x = A \\ b;
         r = b - A * x;
         n = A' * r;",
    );
    for v in vec_re(&ev, "n") {
        close(v, 0.0);
    }
}

#[test]
fn singular_consistent_system_still_solves() {
    let ev = eval("A = [1, 2; 2, 4]; x = A \\ [3; 6]; r = A * x - [3; 6];");
    for v in vec_re(&ev, "r") {
        close(v, 0.0);
    }
}

#[test]
fn right_division_solves_x_times_a() {
    // X * A = B. Solving A.' \ B.' and transposing gives
    // X = [9/5, 7/5; 17/5, 16/5].
    let ev = eval(
        "A = [2, 1; 1, 3];
         B = [5, 6; 10, 13];
         X = B / A;
         R = X * A - B;",
    );
    let x = mat_re(&ev, "X");
    close(x[0][0], 9.0 / 5.0);
    close(x[0][1], 7.0 / 5.0);
    close(x[1][0], 17.0 / 5.0);
    close(x[1][1], 16.0 / 5.0);
    for row in mat_re(&ev, "R") {
        for v in row {
            close(v, 0.0);
        }
    }
}

#[test]
fn right_division_of_a_row() {
    let ev = eval("A = [2, 1; 1, 3]; x = [5, 10] / A;");
    let x = vec_re(&ev, "x");
    close(x[0], 1.0);
    close(x[1], 3.0);
}

#[test]
fn scalar_divide_matrix_stays_elementwise() {
    let ev = eval("M = [2, 4; 6, 8]; Y = 2 / M;");
    let y = mat_re(&ev, "Y");
    close(y[0][0], 1.0);
    close(y[0][1], 0.5);
    close(y[1][0], 2.0 / 6.0);
    close(y[1][1], 0.25);
}

#[test]
fn qr_full_is_orthonormal_and_reconstructs() {
    let ev = eval(
        "A = [1, 2, 3; 4, 5, 6; 7, 8, 10; 2, -1, 0.5];
         [Q, R] = qr(A);
         E = Q' * Q;
         P = Q * R - A;",
    );
    assert_eq!(shape(&ev, "Q"), (4, 4));
    assert_eq!(shape(&ev, "R"), (4, 3));
    let e = mat_re(&ev, "E");
    for i in 0..4 {
        for j in 0..4 {
            close(e[i][j], if i == j { 1.0 } else { 0.0 });
        }
    }
    for row in mat_re(&ev, "P") {
        for v in row {
            close(v, 0.0);
        }
    }
    let r = mat_re(&ev, "R");
    assert!(r[0][0] >= 0.0 && r[1][1] >= 0.0 && r[2][2] >= 0.0);
    close(r[1][0], 0.0);
    close(r[2][0], 0.0);
    close(r[2][1], 0.0);
    close(r[3][0], 0.0);
}

#[test]
fn qr_economy_is_thin() {
    let ev = eval(
        "A = [1, 1; 1, 2; 1, 3];
         [Q, R] = qr(A, 0);
         P = Q * R - A;",
    );
    assert_eq!(shape(&ev, "Q"), (3, 2));
    assert_eq!(shape(&ev, "R"), (2, 2));
    for row in mat_re(&ev, "P") {
        for v in row {
            close(v, 0.0);
        }
    }
}

#[test]
fn qr_complex_reconstructs() {
    let ev = eval(
        "A = [1+j, 2-j; 0, 1-j; -1, 4+j];
         [Q, R] = qr(A);
         P = Q * R - A;
         E = Q' * Q;",
    );
    let p = match ev.get("P").unwrap() {
        Value::Matrix(m) => m.clone(),
        other => panic!("{other:?}"),
    };
    for z in p.iter() {
        assert!(z.norm() < 1e-8, "{z}");
    }
    let e = match ev.get("E").unwrap() {
        Value::Matrix(m) => m.clone(),
        other => panic!("{other:?}"),
    };
    for i in 0..3 {
        for j in 0..3 {
            let expect = if i == j {
                Complex64::new(1.0, 0.0)
            } else {
                Complex64::new(0.0, 0.0)
            };
            assert!((e[[i, j]] - expect).norm() < 1e-8);
        }
    }
}

#[test]
fn lu_permutation_identity() {
    let ev = eval(
        "A = [1, 2; 3, 4];
         [L, U, P] = lu(A);
         D = P * A - L * U;
         [L2, U2] = lu(A);
         D2 = L2 * U2 - A;",
    );
    for row in mat_re(&ev, "D") {
        for v in row {
            close(v, 0.0);
        }
    }
    for row in mat_re(&ev, "D2") {
        for v in row {
            close(v, 0.0);
        }
    }
    let l = mat_re(&ev, "L");
    close(l[0][0], 1.0);
    close(l[1][0], 1.0 / 3.0);
    close(l[1][1], 1.0);
    let u = mat_re(&ev, "U");
    close(u[0][0], 3.0);
    close(u[0][1], 4.0);
    close(u[1][1], 2.0 / 3.0);
    let p = mat_re(&ev, "P");
    close(p[0][1], 1.0);
    close(p[1][0], 1.0);
}

#[test]
fn dense_lu_one_output_explains_itself() {
    let err = eval_err("A = [1, 2; 3, 4]; F = lu(A);");
    assert!(err.contains("[L, U]"), "{err}");
}

#[test]
fn polyfit_recovers_an_exact_quadratic() {
    let ev = eval(
        "x = 0:4;
         y = 4 * x .^ 2 + 3 * x + 2;
         p = polyfit(x, y, 2);
         [p2, S] = polyfit(x, y, 2);
         df = S.df;
         nr = S.normr;
         [p3, S3, mu] = polyfit(x, y, 2);
         yhat = polyval(p3, x, S3, mu);",
    );
    let p = vec_re(&ev, "p");
    close(p[0], 4.0);
    close(p[1], 3.0);
    close(p[2], 2.0);
    let p2 = vec_re(&ev, "p2");
    close(p2[0], 4.0);
    close(p2[1], 3.0);
    close(p2[2], 2.0);
    close(re(&ev, "df"), 2.0);
    assert!(re(&ev, "nr").abs() < 1e-8);
    let mu = vec_re(&ev, "mu");
    close(mu[0], 2.0);
    close(mu[1], (2.5_f64).sqrt());
    let p3 = vec_re(&ev, "p3");
    assert!(
        (p3[0] - 4.0).abs() > 1e-6 || (p3[1] - 3.0).abs() > 1e-6,
        "centered coefficients should differ from the raw fit: {p3:?}"
    );
    let yhat = vec_re(&ev, "yhat");
    let y = vec_re(&ev, "y");
    for (a, b) in yhat.iter().zip(y.iter()) {
        close(*a, *b);
    }
}

#[test]
fn polyfit_refuses_a_huge_vandermonde() {
    let err = eval_err("p = polyfit(1:1000, 1:1000, 100000);");
    assert!(err.contains("100000000"), "{err}");
}

#[test]
fn polyval_horner_and_roots() {
    let ev = eval(
        "y4 = polyval([1, -3, 2], 4);
         z = polyval([1, -3, 2], [1, 2]);
         M = polyval([1, 0], [1, 2; 3, 4]);",
    );
    close(re(&ev, "y4"), 6.0);
    let z = vec_re(&ev, "z");
    close(z[0], 0.0);
    close(z[1], 0.0);
    let m = mat_re(&ev, "M");
    close(m[0][0], 1.0);
    close(m[1][1], 4.0);
}

#[test]
fn interp1_linear_nearest_previous_next_and_extrap() {
    let ev = eval(
        "x = [0, 1, 2];
         y = [0, 1, 0];
         a = interp1(x, y, 0.5);
         b = interp1(x, y, 0.5, \"nearest\");
         c = interp1(x, y, 1.2, \"previous\");
         d = interp1(x, y, 1.2, \"next\");
         e = interp1(x, y, -1);
         f = interp1(x, y, 3, \"linear\", \"extrap\");
         g = interp1([2, 1, 0], [0, 1, 0], 0.5);
         h = interp1(x, y, 1, \"previous\");
         k = interp1(x, y, 0, \"next\");
         z = interp1([0, 1], [0, 1+j], 0.5);",
    );
    close(re(&ev, "a"), 0.5);
    close(re(&ev, "b"), 1.0);
    close(re(&ev, "c"), 1.0);
    close(re(&ev, "d"), 0.0);
    assert!(re(&ev, "e").is_nan());
    close(re(&ev, "f"), -1.0);
    close(re(&ev, "g"), 0.5);
    close(re(&ev, "h"), 1.0);
    close(re(&ev, "k"), 0.0);
    match ev.get("z").unwrap() {
        Value::Complex(z) => {
            close(z.re, 0.5);
            close(z.im, 0.5);
        }
        other => panic!("complex interp1: {other:?}"),
    }
}

#[test]
fn interp1_spline_reproduces_a_cubic() {
    let ev = eval(
        "x = 0:5;
         y = x .^ 3 - 2 * x;
         q = interp1(x, y, 2.5, \"spline\");
         two = interp1([0, 1], [0, 3], 0.25, \"spline\");",
    );
    let expect = 2.5_f64.powi(3) - 2.0 * 2.5;
    close(re(&ev, "q"), expect);
    close(re(&ev, "two"), 0.75);
}

#[test]
fn interp1_pchip_slope_and_makima_and_fill() {
    // Fritsch–Carlson on [0, 1, 2] / [0, 1, 0]: interior slope 0 (the
    // secants change sign), left slope 2. At 0.5 the Hermite value is 0.75,
    // not the linear 0.5.
    let ev = eval(
        "x = [0, 1, 2];
         y = [0, 1, 0];
         q = interp1(x, y, 0.5, \"pchip\");
         nodes = interp1(x, y, x, \"pchip\");
         m = interp1([0, 1, 2, 3], [0, 1, 2, 1], 2.5, \"makima\");
         z = interp1(x, y, -1, \"linear\", 0);
         inside = interp1(x, y, 0.5, \"linear\", 0);",
    );
    close(re(&ev, "q"), 0.75);
    let nodes = vec_re(&ev, "nodes");
    close(nodes[0], 0.0);
    close(nodes[1], 1.0);
    close(nodes[2], 0.0);
    close(re(&ev, "m"), 1.7625);
    close(re(&ev, "z"), 0.0);
    close(re(&ev, "inside"), 0.5);
}

fn interp1_matrix_columns_and_nonmonotonic_error() {
    let ev = eval(
        "x = [0, 1, 2];
         Y = [0, 0; 1, 2; 0, 4];
         Z = interp1(x, Y, 0.5);",
    );
    let z = mat_re(&ev, "Z");
    close(z[0][0], 0.5);
    close(z[0][1], 1.0);
    let err = eval_err("interp1([0, 1, 0], [0, 1, 0], 0.5);");
    assert!(err.contains("monotonic"), "{err}");
}

#[test]
fn null_and_orth_from_svd() {
    let ev = eval(
        "A = [1, 2, 3; 2, 4, 6];
         Z = null(A);
         Q = orth(A);
         N = A * Z;
         G = Z' * Z;
         H = Q' * Q;
         E = null(eye(3));",
    );
    assert_eq!(shape(&ev, "Z"), (3, 2));
    assert_eq!(shape(&ev, "Q"), (2, 1));
    for row in mat_re(&ev, "N") {
        for v in row {
            close(v, 0.0);
        }
    }
    let g = mat_re(&ev, "G");
    close(g[0][0], 1.0);
    close(g[1][1], 1.0);
    close(g[0][1], 0.0);
    close(re(&ev, "H"), 1.0);
    assert_eq!(shape(&ev, "E"), (3, 0));
}

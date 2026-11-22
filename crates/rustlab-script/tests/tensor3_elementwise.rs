//! Element-wise comparisons, logic, unary math, two-argument `min` / `max`,
//! and `find` on `Tensor3`.
//!
//! A `Tensor3` result has the input's shape. There is no rank-3 logical, so
//! comparisons and `!` return 0/1 doubles (the same rule as `cat(3, L, L)`).
//! `find` walks column-major (the `T(:)` order) and, as in MATLAB, the
//! two-output column index runs over the trailing dimensions merged.

use rustlab_script::eval::shared::Shared;
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

/// Shape and column-major real parts of a `Tensor3`.
fn tensor(ev: &Evaluator, name: &str) -> ([usize; 3], Vec<f64>) {
    match ev.get(name) {
        Some(Value::Tensor3(t)) => {
            let s = Shared::shape(t);
            let (m, n, p) = (s[0], s[1], s[2]);
            let mut out = Vec::with_capacity(m * n * p);
            for k in 0..p {
                for j in 0..n {
                    for i in 0..m {
                        out.push(t[[i, j, k]].re);
                    }
                }
            }
            ([m, n, p], out)
        }
        other => panic!("{name}: expected a Tensor3, got {other:?}"),
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

/// 2×2×2 with T(:) = [-1.5, 2, 0, 3.5, -4, 0.25, 6, -0.5].
const T: &str = "T = reshape([-1.5, 2, 0, 3.5, -4, 0.25, 6, -0.5], 2, 2, 2);";
const VALS: [f64; 8] = [-1.5, 2.0, 0.0, 3.5, -4.0, 0.25, 6.0, -0.5];

fn flags(f: impl Fn(f64) -> bool) -> Vec<f64> {
    VALS.iter().map(|&x| if f(x) { 1.0 } else { 0.0 }).collect()
}

#[test]
fn comparisons_with_a_scalar_keep_shape() {
    let ev = eval(&format!(
        "{T} a = T > 0; b = T <= 0; c = T == 0; d = T != 0; e = 0 < T; f = T >= 2;"
    ));
    assert_eq!(tensor(&ev, "a"), ([2, 2, 2], flags(|x| x > 0.0)));
    assert_eq!(tensor(&ev, "b").1, flags(|x| x <= 0.0));
    assert_eq!(tensor(&ev, "c").1, flags(|x| x == 0.0));
    assert_eq!(tensor(&ev, "d").1, flags(|x| x != 0.0));
    assert_eq!(tensor(&ev, "e").1, flags(|x| x > 0.0));
    assert_eq!(tensor(&ev, "f").1, flags(|x| x >= 2.0));
}

#[test]
fn comparison_of_two_same_shape_tensors() {
    let ev = eval(&format!("{T} U = -T; a = T > U; b = T == T;"));
    assert_eq!(tensor(&ev, "a").1, flags(|x| x > -x));
    assert_eq!(tensor(&ev, "b").1, vec![1.0; 8]);
}

#[test]
fn comparison_shape_mismatch_is_an_error() {
    let e = eval_err("A = zeros3(2, 2, 2); B = zeros3(2, 2, 3); c = A < B;");
    assert!(e.contains("shape"), "{e}");
    let e = eval_err("A = zeros3(2, 2, 2); c = A < ones(2, 2);");
    assert!(e.contains("Tensor3") || e.contains("tensor3"), "{e}");
}

#[test]
fn radius_mask_builds_a_conductor() {
    // The EM use case: a 3-D conductor mask from a radius field.
    let ev = eval(
        "n = 5; R = zeros3(n, n, n);
         for i = 1:n
           for j = 1:n
             for k = 1:n
               R(i, j, k) = sqrt((i - 3)^2 + (j - 3)^2 + (k - 3)^2);
             end
           end
         end
         M = R <= 1; count = sum(M(:));",
    );
    assert_eq!(reals(&ev, "count"), vec![7.0]);
}

#[test]
fn not_and_or_xor_on_tensors() {
    let ev = eval(&format!(
        "{T} p = T > 0; q = T < 1; a = !p; b = and(p, q); c = or(p, q); d = xor(p, q); e = and(p, true);"
    ));
    assert_eq!(tensor(&ev, "a").1, flags(|x| !(x > 0.0)));
    assert_eq!(tensor(&ev, "b").1, flags(|x| x > 0.0 && x < 1.0));
    assert_eq!(tensor(&ev, "c").1, flags(|x| x > 0.0 || x < 1.0));
    assert_eq!(tensor(&ev, "d").1, flags(|x| (x > 0.0) != (x < 1.0)));
    assert_eq!(tensor(&ev, "e").1, flags(|x| x > 0.0));
}

#[test]
fn unary_math_keeps_shape() {
    let ev = eval(&format!(
        "{T} a = abs(T); b = sign(T); c = floor(T); d = ceil(T); e = round(T); f = exp(T); g = sqrt(abs(T));"
    ));
    let map = |f: fn(f64) -> f64| VALS.map(f).to_vec();
    assert_eq!(tensor(&ev, "a"), ([2, 2, 2], map(f64::abs)));
    assert_eq!(
        tensor(&ev, "b").1,
        map(|x| if x == 0.0 { 0.0 } else { x.signum() })
    );
    assert_eq!(tensor(&ev, "c").1, map(f64::floor));
    assert_eq!(tensor(&ev, "d").1, map(f64::ceil));
    assert_eq!(tensor(&ev, "e").1, map(f64::round));
    assert_eq!(tensor(&ev, "f").1, map(f64::exp));
    assert_eq!(tensor(&ev, "g").1, map(|x| x.abs().sqrt()));
}

#[test]
fn sqrt_and_log_of_negative_entries_go_complex() {
    let ev = eval(&format!(
        "{T} s = sqrt(T); im1 = imag(s(1, 1, 1)); re4 = real(s(2, 2, 1));"
    ));
    assert!((reals(&ev, "im1")[0] - 1.5f64.sqrt()).abs() < 1e-15);
    assert!((reals(&ev, "re4")[0] - 3.5f64.sqrt()).abs() < 1e-15);
    let ev = eval("L = log(reshape([1, 2, 3, 4, 5, 6, 7, 8], 2, 2, 2)); x = L(2, 1, 2);");
    assert!((reals(&ev, "x")[0] - 6f64.ln()).abs() < 1e-15);
}

#[test]
fn real_imag_conj_angle_on_complex_tensor() {
    let ev = eval(
        "Z = reshape([1 + 2j, 3 - 1j, -2j, 4, 1j, -1, 2 + 2j, 0], 2, 2, 2);
         a = real(Z); b = imag(Z); c = conj(Z); d = imag(c); e = angle(Z);",
    );
    assert_eq!(
        tensor(&ev, "a"),
        ([2, 2, 2], vec![1.0, 3.0, 0.0, 4.0, 0.0, -1.0, 2.0, 0.0])
    );
    assert_eq!(
        tensor(&ev, "b").1,
        vec![2.0, -1.0, -2.0, 0.0, 1.0, 0.0, 2.0, 0.0]
    );
    assert_eq!(
        tensor(&ev, "d").1,
        vec![-2.0, 1.0, 2.0, 0.0, -1.0, 0.0, -2.0, 0.0]
    );
    let e = tensor(&ev, "e").1;
    assert!((e[0] - 2f64.atan2(1.0)).abs() < 1e-15);
    assert!((e[5] - std::f64::consts::PI).abs() < 1e-15);
}

#[test]
fn two_argument_min_max_with_scalar_and_tensor() {
    let ev = eval(&format!(
        "{T} a = min(T, 1); b = max(T, 0); c = max(0, T); d = min(T, -T);"
    ));
    assert_eq!(
        tensor(&ev, "a"),
        ([2, 2, 2], VALS.map(|x| x.min(1.0)).to_vec())
    );
    assert_eq!(tensor(&ev, "b").1, VALS.map(|x| x.max(0.0)).to_vec());
    assert_eq!(tensor(&ev, "c").1, VALS.map(|x| x.max(0.0)).to_vec());
    assert_eq!(tensor(&ev, "d").1, VALS.map(|x| x.min(-x)).to_vec());
}

#[test]
fn tensor_and_matrix_do_not_broadcast() {
    let e = eval_err("T = zeros3(2, 2, 2); m = min(T, ones(2, 2));");
    assert!(e.contains("min"), "{e}");
}

#[test]
fn single_page_tensor_stays_a_tensor() {
    let ev = eval("T = zeros3(1, 1, 1); a = abs(T - 2); b = T > -1;");
    assert_eq!(tensor(&ev, "a"), ([1, 1, 1], vec![2.0]));
    assert_eq!(tensor(&ev, "b"), ([1, 1, 1], vec![1.0]));
}

#[test]
fn find_returns_column_major_linear_indices() {
    let ev = eval(&format!("{T} k = find(T); kp = find(T > 0); v = T(kp);"));
    // T(:) = [-1.5, 2, 0, 3.5, -4, 0.25, 6, -0.5]; nonzero all but 3.
    assert_eq!(reals(&ev, "k"), vec![1.0, 2.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    assert_eq!(reals(&ev, "kp"), vec![2.0, 4.0, 6.0, 7.0]);
    assert_eq!(reals(&ev, "v"), vec![2.0, 3.5, 0.25, 6.0]);
}

#[test]
fn find_agrees_with_flattened_find() {
    let ev = eval("seed(3); T = randn3(3, 4, 5); a = find(T > 0.3); b = find(T(:) > 0.3);");
    assert_eq!(reals(&ev, "a"), reals(&ev, "b"));
}

#[test]
fn find_two_and_three_outputs_match_matlab() {
    // MATLAB: for an N-D array, [row, col] = find(X) merges the trailing
    // dimensions into col; the third output is the values.
    let ev = eval(
        "T = zeros3(2, 3, 2); T(2, 1, 1) = 5; T(1, 3, 2) = -7; T(2, 2, 2) = 9;
         [r, c] = find(T); [r3, c3, v] = find(T);
         [i, j, k] = ind2sub(size(T), find(T));",
    );
    assert_eq!(reals(&ev, "r"), vec![2.0, 2.0, 1.0]);
    assert_eq!(reals(&ev, "c"), vec![1.0, 5.0, 6.0]);
    assert_eq!(reals(&ev, "r3"), vec![2.0, 2.0, 1.0]);
    assert_eq!(reals(&ev, "c3"), vec![1.0, 5.0, 6.0]);
    assert_eq!(reals(&ev, "v"), vec![5.0, 9.0, -7.0]);
    assert_eq!(reals(&ev, "i"), vec![2.0, 2.0, 1.0]);
    assert_eq!(reals(&ev, "j"), vec![1.0, 2.0, 3.0]);
    assert_eq!(reals(&ev, "k"), vec![1.0, 2.0, 2.0]);
}

#[test]
fn find_of_an_all_zero_tensor_is_empty() {
    let ev = eval("k = find(zeros3(2, 2, 2)); n = numel(k);");
    assert_eq!(reals(&ev, "n"), vec![0.0]);
}

#[test]
fn mod_and_trig_keep_shape() {
    let ev = eval(&format!("{T} a = mod(T, 3); b = cos(T);"));
    assert_eq!(
        tensor(&ev, "a"),
        (
            [2, 2, 2],
            VALS.map(|x| x - 3.0 * (x / 3.0).floor()).to_vec()
        )
    );
    assert_eq!(tensor(&ev, "b").1, VALS.map(f64::cos).to_vec());
}

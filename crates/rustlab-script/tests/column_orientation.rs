//! Results MATLAB returns as a column are an `n×1` matrix.
//!
//! `reshape(x, N, 1)`, `x(:)`, a vector subscript into a column, `M(rows, k)`,
//! and `T(:, j, k)` used to come back as a 1-D vector (`size` `[1, n]`), so
//! `L * V(:)` failed and scripts wrote `L * V(:)'`, which conjugates a complex
//! `V` (`rustlab_em` `column-reshape-and-sparse-submatrix.md` §1 and
//! `lesson-review-findings-2026-11-19.md` §3). A single row (`M(k, :)`) and
//! a subscript into a row vector stay a row.

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
        Some(Value::Matrix(m)) => {
            // Column-major, the order MATLAB prints `x(:)` in.
            let (r, c) = m.dim();
            (0..c).flat_map(|j| (0..r).map(move |i| (i, j))).map(|(i, j)| m[[i, j]].re).collect()
        }
        other => panic!("{name}: expected a real array, got {other:?}"),
    }
}

fn size(ev: &Evaluator, name: &str) -> Vec<f64> {
    reals(ev, &format!("size_{name}"))
}

/// Evaluate `prelude`, then `size_x = size(x)` for each name.
fn sizes(prelude: &str, names: &[&str]) -> Evaluator {
    let mut src = prelude.to_string();
    for n in names {
        src.push_str(&format!("\nsize_{n} = size({n});"));
    }
    eval(&src)
}

fn is_column(ev: &Evaluator, name: &str, n: usize) {
    match ev.get(name) {
        Some(Value::Matrix(m)) => assert_eq!(m.dim(), (n, 1), "{name}"),
        Some(Value::Logical(a)) => assert_eq!((a.rows(), a.cols()), (n, 1), "{name}"),
        other => panic!("{name}: expected an {n}×1 column, got {other:?}"),
    }
}

#[test]
fn reshape_to_n_by_1_is_a_column() {
    let ev = sizes(
        "a = reshape(1:6, 6, 1); b = reshape(ones(2, 3), 6, 1); c = reshape([1; 2; 3], 3, 1);
         d = reshape(1:6, 1, 6); e = reshape(ones(2, 3), 3, 2); f = reshape(5, 1, 1);
         g = reshape([1, 2; 3, 4] > 2, 4, 1);",
        &["a", "b", "c", "d", "e", "f", "g"],
    );
    assert_eq!(size(&ev, "a"), vec![6.0, 1.0]);
    assert_eq!(size(&ev, "b"), vec![6.0, 1.0]);
    assert_eq!(size(&ev, "c"), vec![3.0, 1.0]);
    assert_eq!(size(&ev, "d"), vec![1.0, 6.0]);
    assert_eq!(size(&ev, "e"), vec![3.0, 2.0]);
    assert_eq!(size(&ev, "f"), vec![1.0, 1.0]);
    assert_eq!(size(&ev, "g"), vec![4.0, 1.0]);
    assert_eq!(reals(&ev, "a"), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    is_column(&ev, "b", 6);
}

#[test]
fn colon_is_a_column_for_every_array() {
    let ev = sizes(
        "v = [1, 2, 3]; M = [1, 2; 3, 4; 5, 6]; c = [7; 8];
         T = reshape(1:8, 2, 2, 2); L = M > 2;
         a = v(:); b = M(:); d = c(:); t = T(:); l = L(:);",
        &["a", "b", "d", "t", "l"],
    );
    assert_eq!(size(&ev, "a"), vec![3.0, 1.0]);
    assert_eq!(size(&ev, "b"), vec![6.0, 1.0]);
    assert_eq!(size(&ev, "d"), vec![2.0, 1.0]);
    assert_eq!(size(&ev, "t"), vec![8.0, 1.0]);
    assert_eq!(size(&ev, "l"), vec![6.0, 1.0]);
    assert_eq!(reals(&ev, "b"), vec![1.0, 3.0, 5.0, 2.0, 4.0, 6.0]);
    assert_eq!(reals(&ev, "t"), (1..=8).map(f64::from).collect::<Vec<_>>());
    is_column(&ev, "l", 6);
}

#[test]
fn matrix_column_slices_are_columns() {
    let ev = sizes(
        "D = reshape(1:(61*120), 61, 120);
         a = D(2:end, 61); b = D(:, 3); c = D([5, 1, 9], 2); r = D(4, :); s = D(4, 2:5);
         e = D(2, 7);",
        &["a", "b", "c", "r", "s", "e"],
    );
    assert_eq!(size(&ev, "a"), vec![60.0, 1.0]);
    assert_eq!(size(&ev, "b"), vec![61.0, 1.0]);
    assert_eq!(size(&ev, "c"), vec![3.0, 1.0]);
    assert_eq!(size(&ev, "r"), vec![1.0, 120.0], "a single row stays a row");
    assert_eq!(size(&ev, "s"), vec![1.0, 4.0]);
    assert_eq!(size(&ev, "e"), vec![1.0, 1.0]);
    assert_eq!(reals(&ev, "c"), vec![1.0 * 61.0 + 5.0, 1.0 * 61.0 + 1.0, 1.0 * 61.0 + 9.0]);
}

#[test]
fn far_field_unfold_from_the_lesson() {
    // [fliplr(D(2:end, 61)'), D(:, 1)'] — the MATLAB way to unfold a cut.
    let ev = sizes(
        "D = reshape(1:(61*120), 61, 120); u = [fliplr(D(2:end, 61)'), D(:, 1)'];",
        &["u"],
    );
    assert_eq!(size(&ev, "u"), vec![1.0, 121.0]);
    let u = reals(&ev, "u");
    assert_eq!(u[0], 60.0 * 61.0 + 61.0, "last element of column 61 first");
    assert_eq!(u[59], 60.0 * 61.0 + 2.0);
    assert_eq!(u[60], 1.0, "then column 1 from its top");
    assert_eq!(u[120], 61.0);
}

#[test]
fn subscript_into_a_column_keeps_the_column() {
    let ev = sizes(
        "c = [10; 20; 30; 40]; a = c(2:3); b = c([4, 1]); m = c(c > 15); k = c(end);
         v = [10, 20, 30, 40]; r = v(2:3); w = v(:)';",
        &["a", "b", "m", "k", "r", "w"],
    );
    assert_eq!(size(&ev, "a"), vec![2.0, 1.0]);
    assert_eq!(size(&ev, "b"), vec![2.0, 1.0]);
    assert_eq!(size(&ev, "m"), vec![3.0, 1.0]);
    assert_eq!(size(&ev, "k"), vec![1.0, 1.0]);
    assert_eq!(size(&ev, "r"), vec![1.0, 2.0], "a row vector's slice stays a row");
    assert_eq!(size(&ev, "w"), vec![1.0, 4.0]);
    assert_eq!(reals(&ev, "b"), vec![40.0, 10.0]);
    assert_eq!(reals(&ev, "m"), vec![20.0, 30.0, 40.0]);
}

#[test]
fn tensor_column_slice_is_a_column() {
    let ev = sizes(
        "T = reshape(1:24, 3, 4, 2); a = T(:, 2, 1); b = T(2, :, 1); c = T(:, :, 2);",
        &["a", "b", "c"],
    );
    assert_eq!(size(&ev, "a"), vec![3.0, 1.0]);
    assert_eq!(size(&ev, "b"), vec![1.0, 4.0]);
    assert_eq!(size(&ev, "c"), vec![3.0, 4.0]);
    assert_eq!(reals(&ev, "a"), vec![4.0, 5.0, 6.0]);
}

#[test]
fn operator_times_flattened_field_needs_no_transpose() {
    // L * V(:) is the MATLAB idiom; it used to fail with "cannot multiply
    // matrix by row vector".
    let ev = sizes(
        "n = 4; V = reshape(1:16, n, n); L = laplacian_2d(n, n);
         y = L * V(:); z = L * reshape(V, n*n, 1); d = max(abs(y - z));
         q = full(L) * V(:);",
        &["y", "q"],
    );
    assert_eq!(size(&ev, "y"), vec![16.0, 1.0]);
    assert_eq!(size(&ev, "q"), vec![16.0, 1.0]);
    assert_eq!(reals(&ev, "d"), vec![0.0]);
}

#[test]
fn complex_field_is_not_conjugated() {
    // The old workaround L * V(:)' conjugated a complex V. The column form
    // keeps the values; ' still conjugates and .' does not.
    let ev = eval(
        "V = [1 + 2j, 3 - 1j; -2j, 4];
         A = [2, 0, 0, 0; 0, 1, 0, 0; 0, 0, 1, 0; 0, 0, 0, 1];
         y = A * V(:); im1 = imag(y(1)); im3 = imag(y(3));
         h = V(:)'; hc = imag(h(1));
         t = V(:).'; tc = imag(t(1));
         size_h = size(h); size_t = size(t);",
    );
    assert_eq!(reals(&ev, "im1"), vec![4.0], "2 * imag(1 + 2j), not conjugated");
    assert_eq!(reals(&ev, "im3"), vec![-1.0], "V(1, 2) = 3 - 1j");
    assert_eq!(reals(&ev, "hc"), vec![-2.0], "' conjugates");
    assert_eq!(reals(&ev, "tc"), vec![2.0], ".' does not");
    assert_eq!(reals(&ev, "size_h"), vec![1.0, 4.0]);
    assert_eq!(reals(&ev, "size_t"), vec![1.0, 4.0]);
}

#[test]
fn old_transposed_idiom_is_now_a_row() {
    // Migration check: A * V(:)' multiplies by a 1×n row and is an error.
    let e = eval_err("V = ones(2, 2); A = eye(4); y = A * V(:)';");
    assert!(e.contains("dimension") || e.contains("multiply") || e.contains("mismatch"), "{e}");
}

#[test]
fn column_with_vector_arithmetic_is_unchanged() {
    // A column and a same-length vector are element-wise (0.3.62 rule), so
    // `M(:, 1) + v` keeps giving a length-n result.
    let ev = sizes(
        "M = [1, 2; 3, 4; 5, 6]; v = [10, 20, 30]; a = M(:, 1) + v; b = M(:, 2) .* v;
         s = sum(M(:, 1)); m = max(M(:, 2)); n = norm(M(:));",
        &["a", "b"],
    );
    assert_eq!(reals(&ev, "a"), vec![11.0, 23.0, 35.0]);
    assert_eq!(reals(&ev, "b"), vec![20.0, 80.0, 180.0]);
    assert_eq!(reals(&ev, "s"), vec![9.0]);
    assert_eq!(reals(&ev, "m"), vec![6.0]);
    assert!((reals(&ev, "n")[0] - 91f64.sqrt()).abs() < 1e-12);
}

#[test]
fn column_round_trips_through_reshape_and_assignment() {
    let ev = sizes(
        "V = reshape(1:12, 3, 4); x = V(:); W = reshape(x, 3, 4); d = max(max(abs(W - V)));
         U = zeros(3, 4); U(:) = x; e = max(max(abs(U - V)));
         c = V(:, 2); V(:, 3) = c; f = V(2, 3);",
        &["x"],
    );
    assert_eq!(reals(&ev, "d"), vec![0.0]);
    assert_eq!(reals(&ev, "e"), vec![0.0]);
    assert_eq!(reals(&ev, "f"), vec![5.0]);
}

#[test]
fn concatenation_with_columns() {
    let ev = sizes(
        "M = [1, 2; 3, 4]; a = [M(:, 1), M(:, 2)]; b = [M(:, 1); M(:, 2)]; c = [M(:); 9];",
        &["a", "b", "c"],
    );
    assert_eq!(size(&ev, "a"), vec![2.0, 2.0]);
    assert_eq!(size(&ev, "b"), vec![4.0, 1.0]);
    assert_eq!(size(&ev, "c"), vec![5.0, 1.0]);
    assert_eq!(reals(&ev, "a"), vec![1.0, 3.0, 2.0, 4.0]);
}

#[test]
fn arrayfun_and_parmap_take_and_return_columns() {
    let ev = sizes(
        "M = [1, 2; 3, 4; 5, 6];
         a = arrayfun(@(x) 2 * x, M(:, 1));
         p = parmap(@(x) x + 1, M(:));
         R = arrayfun(@(k) M(:, 2) * k, [1, 10]);
         Q = parmap(@(k) M(:, 1) * k, [1, 2, 3]);",
        &["a", "p", "R", "Q"],
    );
    assert_eq!(reals(&ev, "a"), vec![2.0, 6.0, 10.0]);
    assert_eq!(reals(&ev, "p"), vec![2.0, 4.0, 6.0, 3.0, 5.0, 7.0]);
    // A column return is one vector result per call: rows of the output,
    // as when x(:) was a vector.
    assert_eq!(size(&ev, "R"), vec![2.0, 3.0]);
    assert_eq!(size(&ev, "Q"), vec![3.0, 3.0]);
}

#[test]
fn for_over_a_column_explains_the_fix() {
    let e = eval_err("M = [1, 2; 3, 4]; for x = M(:, 1)\n y = x;\nend");
    assert!(e.contains("column") && e.contains(".'"), "{e}");
    let ev = eval("M = [1, 2; 3, 4]; s = 0; for x = M(:, 1).'\n s = s + x;\nend");
    assert_eq!(reals(&ev, "s"), vec![4.0]);
}

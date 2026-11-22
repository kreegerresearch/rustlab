//! Single-row and single-column reads on sparse matrices, and empty
//! selections.
//!
//! A non-empty `S(r, cols)` / `S(rows, c)` is a dense row / dense `N×1`
//! column filled from that line's stored entries; it must never densify the
//! whole matrix (`rustlab_em`, 0.3.77: ~76 GB at 41³). Any selection with no
//! rows or no columns is an empty sparse matrix of the selected shape, as in
//! MATLAB.

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
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

fn reals(ev: &Evaluator, name: &str) -> Vec<f64> {
    match ev.get(name) {
        Some(v) => v.to_cvector().unwrap().iter().map(|z| z.re).collect(),
        None => panic!("{name} missing"),
    }
}

fn sparse_dims(ev: &Evaluator, name: &str) -> (usize, usize, usize) {
    match ev.get(name) {
        Some(Value::SparseMatrix(s)) => (s.rows, s.cols, s.entries.len()),
        other => panic!("{name}: expected a sparse matrix, got {other:?}"),
    }
}

/// 4×5 fixture: row 3 and column 4 hold no entries; column 2 is real only,
/// column 3 complex only, column 1 mixed.
const FIXTURE: &str = "S = sparse([1, 2, 4, 1, 4, 2, 4, 1], [1, 1, 1, 2, 2, 3, 3, 5], \
                       [5, 2j, -1, 7, 3, 1+4j, -2j, 9], 4, 5); D = full(S);";

// ---- empty selections -----------------------------------------------------

#[test]
fn empty_selections_are_empty_sparse_of_the_selected_shape() {
    let ev = eval(&format!(
        "{FIXTURE}
         fr = false(1, 4); fc = false(1, 5);
         A = S(:, []); B = S([], :); C = S([], []);
         E = S(fr, :); F = S(:, fc); G = S(fr, fc);
         H = S(2, []); I = S([], 2); J = S(fr, 3); K = S(3, fc);
         L = S(fr, []); M = S([], fc);"
    ));
    for (name, shape) in [
        ("A", (4, 0)),
        ("B", (0, 5)),
        ("C", (0, 0)),
        ("E", (0, 5)),
        ("F", (4, 0)),
        ("G", (0, 0)),
        ("H", (1, 0)),
        ("I", (0, 1)),
        ("J", (0, 1)),
        ("K", (1, 0)),
        ("L", (0, 0)),
        ("M", (0, 0)),
    ] {
        let (r, c, nnz) = sparse_dims(&ev, name);
        assert_eq!((r, c), shape, "{name} shape");
        assert_eq!(nnz, 0, "{name} nnz");
    }
}

#[test]
fn numel_size_isempty_issparse_of_empty_selections() {
    let ev = eval(&format!(
        "{FIXTURE}
         fr = false(1, 4); fc = false(1, 5);
         A = S(:, []); B = S([], :); C = S([], []); F = S(:, fc); E = S(fr, :);
         nA = numel(A); nB = numel(B); nC = numel(C); nF = numel(F); nE = numel(E);
         lA = length(A); lB = length(B);
         eA = isempty(A); eB = isempty(B); eC = isempty(C); eF = isempty(F); eE = isempty(E);
         pA = issparse(A); pB = issparse(B); pC = issparse(C); pF = issparse(F); pE = issparse(E);
         sA = size(A); sB = size(B); sC = size(C); sF = size(F); sE = size(E);
         a1 = size(A, 1); a2 = size(A, 2); b1 = size(B, 1); b2 = size(B, 2);"
    ));
    for n in ["nA", "nB", "nC", "nF", "nE"] {
        assert_eq!(scalar(&ev, n), 0.0, "{n}");
    }
    for n in ["eA", "eB", "eC", "eF", "eE", "pA", "pB", "pC", "pF", "pE"] {
        assert_eq!(scalar(&ev, n), 1.0, "{n}");
    }
    assert_eq!(reals(&ev, "sA"), vec![4.0, 0.0]);
    assert_eq!(reals(&ev, "sB"), vec![0.0, 5.0]);
    assert_eq!(reals(&ev, "sC"), vec![0.0, 0.0]);
    assert_eq!(reals(&ev, "sF"), vec![4.0, 0.0]);
    assert_eq!(reals(&ev, "sE"), vec![0.0, 5.0]);
    for (n, want) in [("a1", 4.0), ("a2", 0.0), ("b1", 0.0), ("b2", 5.0)] {
        assert_eq!(scalar(&ev, n), want, "{n}");
    }
    // MATLAB rule (same as dense): an empty array has length 0.
    assert_eq!(scalar(&ev, "lA"), 0.0);
    assert_eq!(scalar(&ev, "lB"), 0.0);
}

#[test]
fn empty_selection_of_a_one_row_or_one_column_sparse_matrix() {
    let ev = eval(
        "R = sparse([1, 1], [2, 5], [3, 4], 1, 5); C = sparse([2, 3], [1, 1], [6, 7], 3, 1);
         a = R(:, []); b = R([], :); c = C([], :); d = C(:, []);",
    );
    assert_eq!(sparse_dims(&ev, "a"), (1, 0, 0));
    assert_eq!(sparse_dims(&ev, "b"), (0, 5, 0));
    assert_eq!(sparse_dims(&ev, "c"), (0, 1, 0));
    assert_eq!(sparse_dims(&ev, "d"), (3, 0, 0));
}

#[test]
fn numel_length_isempty_of_nonempty_sparse() {
    let ev = eval(&format!(
        "{FIXTURE}
         n = numel(S); l = length(S); e = isempty(S);
         v = sparsevec([2, 7], [1, 2], 9); nv = numel(v); lv = length(v); ev = isempty(v);
         ed = isempty(zeros(3, 0)); ef = isempty(5); es = isempty(\"\"); et = isempty(\"ab\");"
    ));
    assert_eq!(scalar(&ev, "n"), 20.0);
    assert_eq!(scalar(&ev, "l"), 5.0);
    assert_eq!(scalar(&ev, "e"), 0.0);
    assert_eq!(scalar(&ev, "nv"), 9.0);
    assert_eq!(scalar(&ev, "lv"), 9.0);
    assert_eq!(scalar(&ev, "ev"), 0.0);
    assert_eq!(scalar(&ev, "ed"), 1.0);
    assert_eq!(scalar(&ev, "ef"), 0.0);
    assert_eq!(scalar(&ev, "es"), 1.0);
    assert_eq!(scalar(&ev, "et"), 0.0);
}

// ---- single element -------------------------------------------------------

#[test]
fn single_element_is_a_scalar() {
    let ev = eval(&format!(
        "{FIXTURE} a = S(4, 2); z = S(3, 3); c = S(2, 3); e = S(end, end - 3);"
    ));
    assert!(matches!(ev.get("a"), Some(Value::Scalar(x)) if *x == 3.0));
    assert!(matches!(ev.get("z"), Some(Value::Scalar(x)) if *x == 0.0));
    assert!(
        matches!(ev.get("c"), Some(Value::Complex(z)) if z.re == 1.0 && z.im == 4.0),
        "{:?}",
        ev.get("c")
    );
    assert!(matches!(ev.get("e"), Some(Value::Scalar(x)) if *x == 3.0));
}

// ---- subscripts against the dense read ------------------------------------

#[test]
fn row_and_column_subscripts_match_dense() {
    let ev = eval(&format!(
        "{FIXTURE}
         mr = [true, false, true, true]; mc = [false, true, true, false, true];
         r1 = max(abs(S(2, mc) - D(2, mc)));
         r2 = max(abs(S(1, [5, 1, 1, 3]) - D(1, [5, 1, 1, 3])));
         r3 = max(abs(S(4, end:-1:1) - D(4, end:-1:1)));
         r4 = max(abs(S(end - 1, :) - D(end - 1, :)));
         r5 = max(abs(S(end, [end, end - 1]) - D(end, [end, end - 1])));
         c1 = max(abs(S(mr, 1) - D(mr, 1)));
         c2 = max(abs(S([4, 2, 2, 1], 3) - D([4, 2, 2, 1], 3)));
         c3 = max(abs(S(end:-1:1, 1) - D(end:-1:1, 1)));
         c4 = max(abs(S(:, end - 1) - D(:, end - 1)));
         c5 = max(abs(S([end, end - 1], end) - D([end, end - 1], end)));
         s1 = size(S(2, mc)); s2 = size(S([4, 2, 2, 1], 3)); s3 = size(S(mr, 1));"
    ));
    for n in ["r1", "r2", "r3", "r4", "r5", "c1", "c2", "c3", "c4", "c5"] {
        assert_eq!(scalar(&ev, n), 0.0, "{n}");
    }
    assert_eq!(reals(&ev, "s1"), vec![1.0, 3.0]);
    assert_eq!(reals(&ev, "s2"), vec![4.0, 1.0]);
    assert_eq!(reals(&ev, "s3"), vec![3.0, 1.0]);
}

// ---- complex, real-only, mixed, implicit-zero lines -----------------------

#[test]
fn complex_real_mixed_and_zero_lines() {
    let ev = eval(&format!(
        "{FIXTURE}
         re = S(:, 2); cx = S(:, 3); mx = S(:, 1); zc = S(:, 4); zr = S(3, :);
         rr = S(4, :);
         e1 = max(abs(re - D(:, 2))); e2 = max(abs(cx - D(:, 3)));
         e3 = max(abs(mx - D(:, 1))); e4 = max(abs(rr - D(4, :)));
         i1 = max(abs(imag(re))); n4 = max(abs(zc)); n5 = max(abs(zr));"
    ));
    for n in ["e1", "e2", "e3", "e4", "i1", "n4", "n5"] {
        assert_eq!(scalar(&ev, n), 0.0, "{n}");
    }
    let col = |name: &str| match ev.get(name) {
        Some(Value::Matrix(m)) => {
            assert_eq!(m.dim(), (4, 1), "{name}");
            m.iter().copied().collect::<Vec<_>>()
        }
        other => panic!("{name}: expected a 4×1 column, got {other:?}"),
    };
    let cx = col("cx");
    assert_eq!((cx[1].re, cx[1].im), (1.0, 4.0));
    assert_eq!((cx[3].re, cx[3].im), (0.0, -2.0));
    let mx = col("mx");
    assert_eq!((mx[0].re, mx[1].im, mx[3].re), (5.0, 2.0, -1.0));
    assert!(col("zc").iter().all(|z| z.re == 0.0 && z.im == 0.0));
    assert!(matches!(ev.get("zr"), Some(Value::Vector(v)) if v.len() == 5));
}

// ---- edge lines -----------------------------------------------------------

#[test]
fn first_last_and_degenerate_shapes() {
    let ev = eval(&format!(
        "{FIXTURE}
         e1 = max(abs(S(1, :) - D(1, :))); e2 = max(abs(S(end, :) - D(end, :)));
         e3 = max(abs(S(:, 1) - D(:, 1))); e4 = max(abs(S(:, end) - D(:, end)));
         R = sparse([1, 1], [2, 5], [3, 4], 1, 5); C = sparse([2, 3], [1, 1], [6, 7], 3, 1);
         r = R(1, :); c = C(:, 1); rs = R(1, 2:4); cs = C(2:3, 1);
         rk = R(:, 5); ck = C(3, :);"
    ));
    for n in ["e1", "e2", "e3", "e4"] {
        assert_eq!(scalar(&ev, n), 0.0, "{n}");
    }
    assert_eq!(reals(&ev, "r"), vec![0.0, 3.0, 0.0, 0.0, 4.0]);
    assert!(matches!(ev.get("r"), Some(Value::Vector(_))));
    assert!(matches!(ev.get("c"), Some(Value::Matrix(m)) if m.dim() == (3, 1)));
    assert_eq!(reals(&ev, "c"), vec![0.0, 6.0, 7.0]);
    assert_eq!(reals(&ev, "rs"), vec![3.0, 0.0, 0.0]);
    assert_eq!(reals(&ev, "cs"), vec![6.0, 7.0]);
    assert!(matches!(ev.get("rk"), Some(Value::Scalar(x)) if *x == 4.0));
    assert!(matches!(ev.get("ck"), Some(Value::Scalar(x)) if *x == 7.0));
}

// ---- errors match dense ---------------------------------------------------

#[test]
fn bad_subscripts_give_the_dense_error() {
    for sub in [
        "(5, 1)",
        "(1, 6)",
        "(5, :)",
        "(:, 6)",
        "(0, 1)",
        "(1, 0)",
        "(0, :)",
        "(:, -1)",
        "(-2, 3)",
        "([1, 5], 2)",
        "(2, [1, 6])",
        "(1.5, 2)",
        "(2, [true, false])",
        "([true, false, true, true, true], 1)",
    ] {
        let sparse = eval_err(&format!("{FIXTURE} x = S{sub};"));
        let dense = eval_err(&format!("{FIXTURE} x = D{sub};"));
        assert_eq!(sparse, dense, "S{sub}");
    }
}

// ---- densify-all guard ----------------------------------------------------

/// 10⁶ × 10⁶ with three entries: a dense copy is 16 TB, so any read that
/// densifies the matrix fails. Both orientations, plus reads inside
/// expressions and assignments.
#[test]
fn huge_sparse_lines_do_not_densify() {
    let ev = eval(
        "n = 1000000; S = sparse([1, n, 7], [1, n, n], [3, 4, 2j], n, n);
         c = S(:, n); r = S(n, :); c1 = S(:, 1); r7 = S(7, :);
         sc = size(c); sr = size(r);
         t = sum(c); u = sum(r); w = sum(abs(c1)) + sum(abs(r7));
         y = 2 * S(:, n) + 1; ys = sum(y);
         v = zeros(1, 3); v(2) = S(n, n); v(3) = sum(S(1, 1:10));
         z = S(n, [n, 1, n]); E = S(:, []); nE = numel(E); kE = isempty(E);",
    );
    assert_eq!(reals(&ev, "sc"), vec![1e6, 1.0]);
    assert_eq!(reals(&ev, "sr"), vec![1.0, 1e6]);
    match ev.get("t") {
        Some(Value::Complex(z)) => assert_eq!((z.re, z.im), (4.0, 2.0)),
        other => panic!("t: {other:?}"),
    }
    assert_eq!(scalar(&ev, "u"), 4.0);
    assert_eq!(scalar(&ev, "w"), 5.0);
    match ev.get("ys") {
        Some(Value::Complex(z)) => assert_eq!((z.re, z.im), (1e6 + 8.0, 4.0)),
        other => panic!("ys: {other:?}"),
    }
    assert_eq!(reals(&ev, "v"), vec![0.0, 4.0, 3.0]);
    assert_eq!(reals(&ev, "z"), vec![4.0, 0.0, 4.0]);
    assert_eq!(sparse_dims(&ev, "E"), (1_000_000, 0, 0));
    assert_eq!(scalar(&ev, "nE"), 0.0);
    assert_eq!(scalar(&ev, "kE"), 1.0);
}

// ---- Value API, no parser -------------------------------------------------

fn fixture_value() -> Value {
    use num_complex::Complex;
    let c = |re: f64, im: f64| Complex::new(re, im);
    Value::SparseMatrix(rustlab_core::SparseMat::new(
        4,
        5,
        vec![
            (0, 0, c(5.0, 0.0)),
            (1, 2, c(1.0, 4.0)),
            (3, 4, c(9.0, 0.0)),
        ],
    ))
}

fn empty_index() -> Value {
    Value::Vector(rustlab_core::CVector::from_vec(vec![]).into())
}

fn all_false(n: usize) -> Value {
    Value::Logical(rustlab_core::LogicalArray::row(vec![false; n]).unwrap())
}

fn dims(v: &Value) -> (usize, usize, usize) {
    match v {
        Value::SparseMatrix(s) => (s.rows, s.cols, s.entries.len()),
        other => panic!("expected a sparse matrix, got {other:?}"),
    }
}

#[test]
fn value_index_empty_selections() {
    let s = fixture_value();
    let at = |r: Value, c: Value| s.index_ref(vec![r, c]).unwrap();
    assert_eq!(dims(&at(Value::All, empty_index())), (4, 0, 0));
    assert_eq!(dims(&at(empty_index(), Value::All)), (0, 5, 0));
    assert_eq!(dims(&at(empty_index(), empty_index())), (0, 0, 0));
    assert_eq!(dims(&at(all_false(4), Value::All)), (0, 5, 0));
    assert_eq!(dims(&at(Value::All, all_false(5))), (4, 0, 0));
    assert_eq!(dims(&at(all_false(4), all_false(5))), (0, 0, 0));
    assert_eq!(dims(&at(Value::Scalar(2.0), all_false(5))), (1, 0, 0));
    assert_eq!(dims(&at(all_false(4), Value::Scalar(2.0))), (0, 1, 0));
    assert_eq!(dims(&at(Value::Scalar(2.0), empty_index())), (1, 0, 0));
    assert_eq!(dims(&at(empty_index(), Value::Scalar(2.0))), (0, 1, 0));
}

#[test]
fn value_index_single_line_and_element() {
    let s = fixture_value();
    let col = s.index_ref(vec![Value::All, Value::Scalar(3.0)]).unwrap();
    match &col {
        Value::Matrix(m) => {
            assert_eq!(m.dim(), (4, 1));
            assert_eq!((m[[1, 0]].re, m[[1, 0]].im), (1.0, 4.0));
            assert_eq!(m.iter().filter(|z| z.norm() != 0.0).count(), 1);
        }
        other => panic!("expected a 4×1 column, got {other:?}"),
    }
    let row = s.index_ref(vec![Value::Scalar(4.0), Value::All]).unwrap();
    match &row {
        Value::Vector(v) => assert_eq!(
            v.iter().map(|z| z.re).collect::<Vec<_>>(),
            vec![0.0, 0.0, 0.0, 0.0, 9.0]
        ),
        other => panic!("expected a dense row, got {other:?}"),
    }
    assert!(matches!(
        s.index_ref(vec![Value::Scalar(1.0), Value::Scalar(1.0)]),
        Ok(Value::Scalar(x)) if x == 5.0
    ));
    assert!(matches!(
        s.index_ref(vec![Value::Scalar(3.0), Value::Scalar(3.0)]),
        Ok(Value::Scalar(x)) if x == 0.0
    ));
    assert!(matches!(
        s.index_ref(vec![Value::Scalar(2.0), Value::Scalar(3.0)]),
        Ok(Value::Complex(z)) if z.re == 1.0 && z.im == 4.0
    ));
}

#[test]
fn value_index_huge_line_does_not_densify() {
    let n = 1_000_000;
    let one = num_complex::Complex::new(1.0, 0.0);
    let s = Value::SparseMatrix(rustlab_core::SparseMat::new(
        n,
        n,
        vec![(n - 1, n - 1, one * 4.0), (6, n - 1, one * 2.0)],
    ));
    let col = s
        .index_ref(vec![Value::All, Value::Scalar(n as f64)])
        .unwrap();
    assert!(matches!(&col, Value::Matrix(m) if m.dim() == (n, 1) && m[[6, 0]].re == 2.0));
    let row = s
        .index_ref(vec![Value::Scalar(n as f64), Value::All])
        .unwrap();
    assert!(matches!(&row, Value::Vector(v) if v.len() == n && v[n - 1].re == 4.0));
    assert_eq!(
        dims(&s.index_ref(vec![Value::All, empty_index()]).unwrap()),
        (n, 0, 0)
    );
}

// ---- .rlab script ---------------------------------------------------------

/// The same reads through a script file: `parse_file` + `run_script`, with
/// in-script checks that call `error` on a mismatch.
#[test]
fn rlab_script_sparse_line_reads() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/scripts/sparse_line_reads.rlab"
    );
    let stmts = rustlab_script::parse_file(path).expect("parse");
    let mut ev = Evaluator::new();
    ev.run_script(&stmts).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(scalar(&ev, "checks"), 26.0);
}

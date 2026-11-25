//! Indexing and indexed assignment on `Tensor3`, with the result shape of
//! every form pinned.
//!
//! Three subscripts drop every singleton dimension: `T(i, :, :)` is an
//! `n×p` matrix, where MATLAB returns `1×n×p` and needs `squeeze`. Two
//! singletons give a 1-D row (`T(i, j, :)`, `T(i, :, k)`), except
//! `T(:, j, k)`, which is an `m×1` column. One subscript is linear over the
//! column-major order (`T(:)`), as in MATLAB. Two subscripts fold the
//! trailing dimensions (`T(i, j)` with `j` in `1:n*p`), as in MATLAB.

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

/// Kind, shape and column-major real parts of a value.
#[derive(Debug, PartialEq)]
enum Shape {
    Scalar(f64),
    /// 1-D vector (a row).
    Vector(Vec<f64>),
    Matrix(usize, usize, Vec<f64>),
    Tensor3(usize, usize, usize, Vec<f64>),
}

fn shape(ev: &Evaluator, name: &str) -> Shape {
    match ev.get(name) {
        Some(Value::Scalar(x)) => Shape::Scalar(*x),
        Some(Value::Vector(v)) => Shape::Vector(v.iter().map(|c| c.re).collect()),
        Some(Value::Matrix(m)) => {
            let s = Shared::shape(m);
            let mut out = Vec::new();
            for j in 0..s[1] {
                for i in 0..s[0] {
                    out.push(m[[i, j]].re);
                }
            }
            Shape::Matrix(s[0], s[1], out)
        }
        Some(Value::Tensor3(t)) => {
            let s = Shared::shape(t);
            let mut out = Vec::new();
            for k in 0..s[2] {
                for j in 0..s[1] {
                    for i in 0..s[0] {
                        out.push(t[[i, j, k]].re);
                    }
                }
            }
            Shape::Tensor3(s[0], s[1], s[2], out)
        }
        other => panic!("{name}: unexpected {other:?}"),
    }
}

/// `x = <expr>` on `T = reshape(1:24, 2, 3, 4)`, so `T(i, j, k) =
/// i + 2(j-1) + 6(k-1)`.
fn read(expr: &str) -> Shape {
    let ev = eval(&format!("T = reshape(1:24, 2, 3, 4);\nx = {expr};"));
    shape(&ev, "x")
}

fn t(i: usize, j: usize, k: usize) -> f64 {
    (i + 2 * (j - 1) + 6 * (k - 1)) as f64
}

fn range(a: usize, b: usize) -> Vec<f64> {
    (a..=b).map(|x| x as f64).collect()
}

#[test]
fn first_axis_slice_is_n_by_p_matrix() {
    let mut want = Vec::new();
    for k in 1..=4 {
        for j in 1..=3 {
            want.push(t(2, j, k));
        }
    }
    assert_eq!(read("T(2, :, :)"), Shape::Matrix(3, 4, want.clone()));
    assert_eq!(read("T(end, :, :)"), Shape::Matrix(3, 4, want.clone()));
    // A one-element range drops the dimension too (MATLAB keeps 1×n×p).
    assert_eq!(read("T(2:2, :, :)"), Shape::Matrix(3, 4, want.clone()));
    // squeeze passes a matrix through, so the MATLAB idiom gives the same.
    assert_eq!(read("squeeze(T(2, :, :))"), Shape::Matrix(3, 4, want));
}

#[test]
fn second_axis_slice_is_m_by_p_matrix() {
    let mut want = Vec::new();
    for k in 1..=4 {
        for i in 1..=2 {
            want.push(t(i, 2, k));
        }
    }
    assert_eq!(read("T(:, 2, :)"), Shape::Matrix(2, 4, want));
}

#[test]
fn page_slice_is_m_by_n_matrix() {
    assert_eq!(read("T(:, :, 3)"), Shape::Matrix(2, 3, range(13, 18)));
    assert_eq!(read("T(:, :, end)"), Shape::Matrix(2, 3, range(19, 24)));
}

#[test]
fn ranges_on_several_axes_stay_tensor3() {
    assert_eq!(read("T(1:2, :, :)"), Shape::Tensor3(2, 3, 4, range(1, 24)));
    assert_eq!(read("T(:, :, :)"), Shape::Tensor3(2, 3, 4, range(1, 24)));
    assert_eq!(
        read("T(:, 1:2, 2:3)"),
        Shape::Tensor3(2, 2, 2, vec![7.0, 8.0, 9.0, 10.0, 13.0, 14.0, 15.0, 16.0])
    );
}

#[test]
fn two_singletons_give_a_row_except_the_column_fibre() {
    assert_eq!(read("T(1, 2, :)"), Shape::Vector(vec![3.0, 9.0, 15.0, 21.0]));
    assert_eq!(read("T(1, :, 3)"), Shape::Vector(vec![13.0, 15.0, 17.0]));
    assert_eq!(read("T(1, :, end-1)"), Shape::Vector(vec![13.0, 15.0, 17.0]));
    assert_eq!(read("T(:, 2, 3)"), Shape::Matrix(2, 1, vec![15.0, 16.0]));
    assert_eq!(read("T(:, end, 1)"), Shape::Matrix(2, 1, vec![5.0, 6.0]));
    // squeeze of a row vector is that row (MATLAB's squeeze of 1×1×p is p×1).
    assert_eq!(
        read("squeeze(T(1, 2, :))"),
        Shape::Vector(vec![3.0, 9.0, 15.0, 21.0])
    );
}

#[test]
fn single_element_is_scalar() {
    assert_eq!(read("T(2, 3, 4)"), Shape::Scalar(24.0));
    assert_eq!(read("T(end, end, end)"), Shape::Scalar(24.0));
}

#[test]
fn logical_subscripts_on_each_axis() {
    assert_eq!(
        read("T(:, [true, false, true], 1)"),
        Shape::Matrix(2, 2, vec![1.0, 2.0, 5.0, 6.0])
    );
    let mut want = range(7, 12);
    want.extend(range(19, 24));
    assert_eq!(
        read("T(:, :, logical([0, 1, 0, 1]))"),
        Shape::Tensor3(2, 3, 2, want)
    );
}

#[test]
fn linear_reads_walk_column_major() {
    assert_eq!(read("T(:)"), Shape::Matrix(24, 1, range(1, 24)));
    assert_eq!(read("T(5)"), Shape::Scalar(5.0));
    assert_eq!(read("T(end)"), Shape::Scalar(24.0));
    assert_eq!(read("T(end-1)"), Shape::Scalar(23.0));
    assert_eq!(read("T(3:6)"), Shape::Vector(range(3, 6)));
    assert_eq!(read("T([3, 4])"), Shape::Vector(vec![3.0, 4.0]));
}

#[test]
fn linear_read_with_a_column_of_subscripts_is_a_column() {
    assert_eq!(read("T([3; 4])"), Shape::Matrix(2, 1, vec![3.0, 4.0]));
}

#[test]
fn linear_logical_mask_read() {
    assert_eq!(
        read("T(logical(mod(1:24, 5) == 0))"),
        Shape::Vector(vec![5.0, 10.0, 15.0, 20.0])
    );
}

#[test]
fn same_shape_comparison_mask_read_is_a_column() {
    assert_eq!(read("T(T > 20)"), Shape::Matrix(4, 1, range(21, 24)));
    assert_eq!(read("T(find(T > 20))"), Shape::Vector(range(21, 24)));
}

#[test]
fn two_subscripts_fold_trailing_dimensions() {
    // T(i, j) with j in 1:n*p is T(i, mod(j-1, n)+1, floor((j-1)/n)+1).
    assert_eq!(read("T(2, 5)"), Shape::Scalar(t(2, 2, 2)));
    assert_eq!(read("T(end, end)"), Shape::Scalar(24.0));
    assert_eq!(read("T(:, 1)"), Shape::Matrix(2, 1, vec![1.0, 2.0]));
    assert_eq!(read("T(:, :)"), Shape::Matrix(2, 12, range(1, 24)));
    let odd: Vec<f64> = (0..12).map(|c| (2 * c + 1) as f64).collect();
    assert_eq!(read("T(1, :)"), Shape::Vector(odd));
}

/// `U = zeros3(2, 3, 4); <stmt>;` then read `U`.
fn write(stmt: &str) -> Shape {
    let ev = eval(&format!("U = zeros3(2, 3, 4);\n{stmt};"));
    shape(&ev, "U")
}

fn with(sets: &[(usize, usize, usize, f64)]) -> Shape {
    let mut out = vec![0.0; 24];
    for &(i, j, k, v) in sets {
        out[(i - 1) + 2 * (j - 1) + 6 * (k - 1)] = v;
    }
    Shape::Tensor3(2, 3, 4, out)
}

#[test]
fn assign_first_axis_slice() {
    let mut sets = Vec::new();
    for k in 1..=4 {
        for j in 1..=3 {
            sets.push((2, j, k, (j + 3 * (k - 1)) as f64));
        }
    }
    assert_eq!(write("U(2, :, :) = reshape(1:12, 3, 4)"), with(&sets));
    assert_eq!(write("U(end, :, :) = reshape(1:12, 3, 4)"), with(&sets));
    assert_eq!(write("U(2, :, :) = reshape(1:12, 1, 3, 4)"), with(&sets));
}

#[test]
fn assign_second_axis_and_page_slices() {
    let mut sets = Vec::new();
    for k in 1..=4 {
        for i in 1..=2 {
            sets.push((i, 2, k, (i + 2 * (k - 1)) as f64));
        }
    }
    assert_eq!(write("U(:, 2, :) = reshape(1:8, 2, 4)"), with(&sets));
    assert_eq!(
        write("U(:, :, 3) = [1, 2, 3; 4, 5, 6]"),
        with(&[
            (1, 1, 3, 1.0),
            (1, 2, 3, 2.0),
            (1, 3, 3, 3.0),
            (2, 1, 3, 4.0),
            (2, 2, 3, 5.0),
            (2, 3, 3, 6.0)
        ])
    );
}

#[test]
fn assign_fibre_from_row_or_column() {
    let want = with(&[(1, 2, 1, 1.0), (1, 2, 2, 2.0), (1, 2, 3, 3.0), (1, 2, 4, 4.0)]);
    assert_eq!(write("U(1, 2, :) = [1, 2, 3, 4]"), want);
    assert_eq!(write("U(1, 2, :) = [1; 2; 3; 4]"), want);
    let col = with(&[(1, 2, 3, 7.0), (2, 2, 3, 8.0)]);
    assert_eq!(write("U(:, 2, 3) = [7; 8]"), col);
    assert_eq!(write("U(:, 2, 3) = [7, 8]"), col);
}

#[test]
fn assign_scalar_broadcast_and_sub_tensor() {
    let mut sets = Vec::new();
    for k in 1..=4 {
        for j in 1..=3 {
            sets.push((2, j, k, 5.0));
        }
    }
    assert_eq!(write("U(2, :, :) = 5"), with(&sets));
    let mut cube = Vec::new();
    for k in 1..=2 {
        for j in 1..=2 {
            for i in 1..=2 {
                cube.push((i, j, k, 1.0));
            }
        }
    }
    assert_eq!(write("U(1:2, 1:2, 1:2) = ones3(2, 2, 2)"), with(&cube));
    assert_eq!(write("U(end, end, end) = 9"), with(&[(2, 3, 4, 9.0)]));
}

#[test]
fn assign_region_shape_mismatch_is_an_error() {
    assert!(eval_err("U = zeros3(2, 3, 4);\nU(:, :, 2) = [1, 2; 3, 4];").contains("2×3"));
    assert!(eval_err("U = zeros3(2, 3, 4);\nU(2, :, :) = ones(4, 3);").contains("3×4"));
}

#[test]
fn assign_past_the_end_with_three_subscripts_is_an_error() {
    // MATLAB grows the array; rustlab does not.
    assert!(eval_err("U = zeros3(2, 3, 4);\nU(:, :, 5) = ones(2, 3);").contains("out of bounds"));
}

#[test]
fn linear_assign_keeps_the_tensor() {
    assert_eq!(write("U(5) = 9"), with(&[(1, 3, 1, 9.0)]));
    assert_eq!(write("U(end) = 7"), with(&[(2, 3, 4, 7.0)]));
    assert_eq!(
        write("U([1, 24]) = [5, 6]"),
        with(&[(1, 1, 1, 5.0), (2, 3, 4, 6.0)])
    );
    assert_eq!(write("U(:) = 1:24"), Shape::Tensor3(2, 3, 4, range(1, 24)));
    assert_eq!(
        write("U(:) = reshape(1:24, 2, 3, 4)"),
        Shape::Tensor3(2, 3, 4, range(1, 24))
    );
}

#[test]
fn linear_assign_with_masks() {
    let ev = eval("T = reshape(1:24, 2, 3, 4);\nT(T > 20) = 0;");
    let mut want = range(1, 20);
    want.extend([0.0; 4]);
    assert_eq!(shape(&ev, "T"), Shape::Tensor3(2, 3, 4, want));
    let ev = eval("T = reshape(1:24, 2, 3, 4);\nT(logical(mod(1:24, 5) == 0)) = -1;");
    let want: Vec<f64> = (1..=24)
        .map(|x| if x % 5 == 0 { -1.0 } else { x as f64 })
        .collect();
    assert_eq!(shape(&ev, "T"), Shape::Tensor3(2, 3, 4, want));
}

#[test]
fn linear_assign_past_the_end_is_an_error() {
    assert!(eval_err("U = zeros3(2, 3, 4);\nU(25) = 1;").contains("out of bounds"));
}

#[test]
fn two_subscript_assign_is_an_error() {
    assert!(eval_err("U = zeros3(2, 3, 4);\nU(1, 5) = 9;").contains("tensor3"));
}

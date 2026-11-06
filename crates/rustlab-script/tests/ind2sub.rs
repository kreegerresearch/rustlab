//! MATLAB-compatible `ind2sub` / `sub2ind`.
//!
//! Reference values are the MathWorks 3×3 example and the Octave
//! `ind2sub` / `sub2ind` tests (column-major, 1-based). An index outside
//! the array is an error here; Octave `ind2sub` does not range-check.

use rustlab_script::eval::shared::Shared;
use rustlab_script::{Evaluator, Value};

fn eval(src: &str) -> Evaluator {
    let src = with_nl(src);
    let tokens = rustlab_script::lexer::tokenize(&src).unwrap();
    let stmts = rustlab_script::parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    ev.run(&stmts)
        .unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"));
    ev
}

fn eval_err(src: &str) -> String {
    let src = with_nl(src);
    let tokens = rustlab_script::lexer::tokenize(&src).unwrap();
    let stmts = rustlab_script::parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    match ev.run(&stmts) {
        Err(e) => e.to_string(),
        Ok(()) => panic!("expected an error\n{src}"),
    }
}

fn with_nl(src: &str) -> String {
    if src.ends_with('\n') {
        src.to_string()
    } else {
        format!("{src}\n")
    }
}

fn reals(ev: &Evaluator, name: &str) -> Vec<f64> {
    match ev.get(name) {
        Some(Value::Scalar(x)) => vec![*x],
        Some(Value::Vector(v)) => v.iter().map(|c| c.re).collect(),
        Some(Value::Matrix(m)) => m.iter().map(|c| c.re).collect(),
        Some(Value::Tensor3(t)) => t.iter().map(|c| c.re).collect(),
        other => panic!("{name}: expected a number or array, got {other:?}"),
    }
}

fn assert_reals(ev: &Evaluator, name: &str, expect: &[f64]) {
    let got = reals(ev, name);
    assert_eq!(got, expect, "{name}");
}

#[test]
fn mathworks_3_by_3_vectors() {
    let ev = eval(
        "
        ind = [3, 4, 5, 6];
        [row, col] = ind2sub([3, 3], ind);
        back = sub2ind([3, 3], row, col);
        one = ind2sub([3, 3], ind);
        ",
    );
    assert_reals(&ev, "row", &[3.0, 1.0, 2.0, 3.0]);
    assert_reals(&ev, "col", &[1.0, 2.0, 2.0, 2.0]);
    assert_reals(&ev, "back", &[3.0, 4.0, 5.0, 6.0]);
    // One output merges every dimension, so it is the linear index.
    assert_reals(&ev, "one", &[3.0, 4.0, 5.0, 6.0]);
}

#[test]
fn octave_pairs_and_extra_page_of_ones() {
    let ev = eval(
        "
        [r, c] = ind2sub([3, 3], [2, 8]);
        [r3, c3, s] = ind2sub([3, 3], [2, 8]);
        [r1, c1, s1] = ind2sub([3, 3, 1], [2, 8]);
        scalar_size = ind2sub(9, [2, 8]);
        ",
    );
    assert_reals(&ev, "r", &[2.0, 2.0]);
    assert_reals(&ev, "c", &[1.0, 3.0]);
    assert_reals(&ev, "s", &[1.0, 1.0]);
    assert_reals(&ev, "s1", &[1.0, 1.0]);
    assert_reals(&ev, "r3", &[2.0, 2.0]);
    assert_reals(&ev, "c3", &[1.0, 3.0]);
    assert_reals(&ev, "scalar_size", &[2.0, 8.0]);
}

#[test]
fn cube_full_and_merged_trailing_dimension() {
    let ev = eval(
        "
        k = 1:8;
        [r, c, s] = ind2sub([2, 2, 2], k);
        [rm, cm] = ind2sub([2, 2, 2], k);
        back = sub2ind([2, 2, 2], r, c, s);
        merged = sub2ind([2, 4], rm, cm);
        corner = sub2ind([10, 10, 10], 10, 10, 10);
        origin = sub2ind([10, 10, 10], 1, 1, 1);
        ",
    );
    assert_reals(&ev, "r", &[1.0, 2.0, 1.0, 2.0, 1.0, 2.0, 1.0, 2.0]);
    assert_reals(&ev, "c", &[1.0, 1.0, 2.0, 2.0, 1.0, 1.0, 2.0, 2.0]);
    assert_reals(&ev, "s", &[1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]);
    assert_reals(&ev, "rm", &[1.0, 2.0, 1.0, 2.0, 1.0, 2.0, 1.0, 2.0]);
    assert_reals(&ev, "cm", &[1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0]);
    assert_reals(&ev, "back", &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    assert_reals(&ev, "merged", &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    assert_reals(&ev, "corner", &[1000.0]);
    assert_reals(&ev, "origin", &[1.0]);
}

#[test]
fn matrix_subscripts_match_octave() {
    let ev = eval(
        "
        s1 = [1, 1, 1, 1; 2, 2, 2, 2];
        s2 = [1, 1, 2, 2; 1, 1, 2, 2];
        s3 = [1, 2, 1, 2; 1, 2, 1, 2];
        k = sub2ind([10, 10, 10], s1, s2, s3);
        ",
    );
    assert_reals(
        &ev,
        "k",
        &[1.0, 101.0, 11.0, 111.0, 2.0, 102.0, 12.0, 112.0],
    );
    match ev.get("k") {
        Some(Value::Matrix(m)) => {
            assert_eq!((m.nrows(), m.ncols()), (2, 4));
            if std::env::var_os("RUSTLAB_COMPLEX_STORAGE").is_none() {
                assert!(Shared::is_real(m));
            }
        }
        other => panic!("expected a matrix, got {other:?}"),
    }
}

#[test]
fn index_matrix_keeps_its_shape() {
    let ev = eval(
        "
        M = [3, 4; 5, 6];
        [I, J] = ind2sub([3, 3], M);
        flat = ind2sub([3, 3], M);
        col = [2; 8];
        [cr, cc] = ind2sub([3, 3], col);
        ",
    );
    assert_reals(&ev, "I", &[3.0, 1.0, 2.0, 3.0]);
    assert_reals(&ev, "J", &[1.0, 2.0, 2.0, 2.0]);
    assert_reals(&ev, "flat", &[3.0, 4.0, 5.0, 6.0]);
    assert_reals(&ev, "cr", &[2.0, 2.0]);
    assert_reals(&ev, "cc", &[1.0, 3.0]);
    match ev.get("I") {
        Some(Value::Matrix(m)) => assert_eq!((m.nrows(), m.ncols()), (2, 2)),
        other => panic!("expected a matrix, got {other:?}"),
    }
    match ev.get("cr") {
        Some(Value::Matrix(m)) => assert_eq!((m.nrows(), m.ncols()), (2, 1)),
        other => panic!("expected a column, got {other:?}"),
    }
}

#[test]
fn scalar_broadcast_and_trailing_ones() {
    let ev = eval(
        "
        row = sub2ind([3, 4], [1, 2, 3], 2);
        swapped = sub2ind([3, 4], 2, [1, 2, 3]);
        drop = sub2ind([3, 4, 1], 2, 3);
        pad = sub2ind([10, 1], 2, 1, 1);
        pad2 = sub2ind([10, 10], 2, 2, 1);
        alone = sub2ind(5, 3);
        [i, j] = ind2sub(5, 3);
        ",
    );
    assert_reals(&ev, "row", &[4.0, 5.0, 6.0]);
    // Row 2, columns 1:3. The scalar is the first subscript, not the column.
    assert_reals(&ev, "swapped", &[2.0, 5.0, 8.0]);
    assert_reals(&ev, "drop", &[8.0]);
    assert_reals(&ev, "pad", &[2.0]);
    assert_reals(&ev, "pad2", &[12.0]);
    assert_reals(&ev, "alone", &[3.0]);
    assert_reals(&ev, "i", &[3.0]);
    assert_reals(&ev, "j", &[1.0]);
    match ev.get("row") {
        Some(Value::Vector(v)) => assert_eq!(v.len(), 3),
        other => panic!("expected a vector, got {other:?}"),
    }
    match ev.get("swapped") {
        Some(Value::Vector(_)) => {}
        other => panic!("expected a vector, got {other:?}"),
    }
}

#[test]
fn empty_inputs_keep_their_shape() {
    let ev = eval(
        "
        [r, c] = ind2sub([3, 3], []);
        E = zeros(0, 2);
        [er, ec] = ind2sub([3, 4], E);
        z = sub2ind([10, 10], zeros(0, 0), zeros(0, 0));
        ",
    );
    assert_reals(&ev, "r", &[]);
    assert_reals(&ev, "c", &[]);
    match ev.get("r") {
        Some(Value::Vector(v)) => assert_eq!(v.len(), 0),
        other => panic!("expected an empty vector, got {other:?}"),
    }
    match ev.get("er") {
        Some(Value::Matrix(m)) => assert_eq!((m.nrows(), m.ncols()), (0, 2)),
        other => panic!("expected a 0x2 matrix, got {other:?}"),
    }
    match ev.get("ec") {
        Some(Value::Matrix(m)) => assert_eq!((m.nrows(), m.ncols()), (0, 2)),
        other => panic!("expected a 0x2 matrix, got {other:?}"),
    }
    match ev.get("z") {
        Some(Value::Matrix(m)) => assert_eq!((m.nrows(), m.ncols()), (0, 0)),
        other => panic!("expected a 0x0 matrix, got {other:?}"),
    }
}

#[test]
fn find_round_trip() {
    let ev = eval(
        "
        M = [0, 2; 3, 0];
        lin = find(M);
        [I, J] = find(M);
        [ri, rj] = ind2sub(size(M), lin);
        back = sub2ind(size(M), I, J);
        ",
    );
    assert_reals(&ev, "lin", &[2.0, 3.0]);
    assert_reals(&ev, "I", &[2.0, 1.0]);
    assert_reals(&ev, "J", &[1.0, 2.0]);
    assert_reals(&ev, "ri", &[2.0, 1.0]);
    assert_reals(&ev, "rj", &[1.0, 2.0]);
    assert_reals(&ev, "back", &[2.0, 3.0]);
}

#[test]
fn integer_and_real_complex_indexes() {
    let ev = eval(
        "
        k = ind2sub([3, 3], int32([3, 4, 5, 6]));
        z = sub2ind([3, 3], 2+0*j, 2);
        ",
    );
    assert_reals(&ev, "k", &[3.0, 4.0, 5.0, 6.0]);
    assert_reals(&ev, "z", &[5.0]);
}

#[test]
fn bare_call_is_one_output_and_binds_ans() {
    let ev = eval("ind2sub([3, 3], 5)");
    assert_reals(&ev, "ans", &[5.0]);
}

#[test]
fn out_of_range_and_bad_size_are_errors() {
    let err = eval_err("ind2sub([3, 3], 10)");
    assert!(err.contains("linear index 10 is outside 1:9"), "{err}");
    let err = eval_err("ind2sub([3, 3], 0)");
    assert!(
        err.contains("index must be a positive integer, got 0"),
        "{err}"
    );
    let err = eval_err("ind2sub([3, 3], 1.5)");
    assert!(
        err.contains("index must be a positive integer, got 1.5"),
        "{err}"
    );
    let err = eval_err("ind2sub([3, 3], -1)");
    assert!(
        err.contains("index must be a positive integer, got -1"),
        "{err}"
    );
    let err = eval_err("ind2sub([2, -2], 3)");
    assert!(
        err.contains("size must be a non-negative integer, got -2"),
        "{err}"
    );
    let err = eval_err("ind2sub([], 1)");
    assert!(err.contains("size must not be empty"), "{err}");
    let err = eval_err("ind2sub([1, 2; 3, 4], 1)");
    assert!(err.contains("size must be a scalar or a vector"), "{err}");
    let err = eval_err("ind2sub([2, 2, 2, 2], 1)");
    assert!(err.contains("rustlab arrays are at most rank 3"), "{err}");
    let err = eval_err("[a, b, c, d] = ind2sub([2, 2], 1)");
    assert!(err.contains("rustlab arrays are at most rank 3"), "{err}");
    let err = eval_err("ind2sub([0, 3], 1)");
    assert!(err.contains("linear index 1 is outside 1:0"), "{err}");
    let err = eval_err("sub2ind([3, 3], 4, 1)");
    assert!(
        err.contains("subscript 4 on dimension 1 is outside 1:3"),
        "{err}"
    );
    let err = eval_err("sub2ind([0, 3], 1, 1)");
    assert!(
        err.contains("subscript 1 on dimension 1 is outside 1:0"),
        "{err}"
    );
    let err = eval_err("sub2ind([2, 2, 2], 1, 1)");
    assert!(err.contains("expected 3 subscript arrays"), "{err}");
    let err = eval_err("sub2ind([10, 10], 2, 1, 2)");
    assert!(
        err.contains("subscript 2 on dimension 3 is outside 1:1"),
        "{err}"
    );
    let err = eval_err("sub2ind([3, 3], [1, 2], [1; 2])");
    assert!(
        err.contains("subscript arrays must be the same size"),
        "{err}"
    );
    let err = eval_err("sub2ind([3, 3], ones(1, 2), ones(1, 3))");
    assert!(
        err.contains("subscript arrays must be the same size"),
        "{err}"
    );
    let err = eval_err("sub2ind([3, 3], [], 0)");
    assert!(
        err.contains("index must be a positive integer, got 0"),
        "{err}"
    );
    let err = eval_err("sub2ind([3, 3], 1+j, 1)");
    assert!(
        err.contains("index must be a real positive integer"),
        "{err}"
    );
    let err = eval_err("ind2sub(3)");
    assert!(err.contains("ind2sub"), "{err}");
    let err = eval_err("sub2ind([2, 2, 2, 2], 1, 1, 1, 1)");
    assert!(err.contains("rustlab arrays are at most rank 3"), "{err}");
}

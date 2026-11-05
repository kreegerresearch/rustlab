//! Logical arrays: comparisons, `logical` / `islogical`, and mask indexing.

use crate::eval::value::Value;
use crate::testutil::{eval, eval_err, re};

fn val<'a>(ev: &'a crate::eval::Evaluator, name: &str) -> &'a Value {
    ev.get(name).unwrap_or_else(|| panic!("{name} missing"))
}

#[test]
fn literal_is_a_logical_array_not_numeric() {
    let ev = eval("L = [true, false, true]");
    match val(&ev, "L") {
        Value::Logical(a) => {
            assert_eq!((a.rows(), a.cols()), (1, 3));
            assert_eq!(a.to_flags(), vec![true, false, true]);
        }
        other => panic!("expected logical, got {other:?}"),
    }
    assert!(matches!(
        val(&eval("a = islogical([true, false])"), "a"),
        Value::Bool(true)
    ));
    assert!(matches!(
        val(&eval("a = islogical([1, 0])"), "a"),
        Value::Bool(false)
    ));
    assert_eq!(
        match val(&eval("a = class([true, false])"), "a") {
            Value::Str(s) => s.as_str(),
            other => panic!("{other:?}"),
        },
        "logical"
    );
}

#[test]
fn scalar_comparison_stays_bool_and_islogical() {
    assert!(matches!(val(&eval("a = 1 == 1"), "a"), Value::Bool(true)));
    assert!(matches!(
        val(&eval("a = islogical(1 == 1)"), "a"),
        Value::Bool(true)
    ));
    assert!(matches!(
        val(&eval("a = islogical(true)"), "a"),
        Value::Bool(true)
    ));
    assert_eq!(
        match val(&eval("a = class(1 == 1)"), "a") {
            Value::Str(s) => s.as_str(),
            other => panic!("{other:?}"),
        },
        "bool"
    );
    assert!(matches!(
        val(&eval("a = isa(true, \"logical\")"), "a"),
        Value::Bool(true)
    ));
    assert!(matches!(
        val(&eval("a = isa([true, false], \"bool\")"), "a"),
        Value::Bool(false)
    ));
}

#[test]
fn array_comparison_is_logical_and_sums() {
    let ev = eval("L = [1, 2, 3] == [1, 0, 3]");
    match val(&ev, "L") {
        Value::Logical(a) => assert_eq!(a.to_flags(), vec![true, false, true]),
        other => panic!("{other:?}"),
    }
    assert_eq!(re(&eval("a = sum([1, 2, 3] == [1, 0, 3])"), "a"), 2.0);
    assert_eq!(re(&eval("a = sum(sum([1, 2; 3, 4] > 2))"), "a"), 2.0);
    assert!(matches!(
        val(&eval("a = islogical([1, 2] > [0, 9])"), "a"),
        Value::Bool(true)
    ));
    assert_eq!(
        match val(&eval("a = class([1, 2] > 0)"), "a") {
            Value::Str(s) => s.as_str(),
            other => panic!("{other:?}"),
        },
        "logical"
    );
}

#[test]
fn mask_indexing_and_assign() {
    assert_eq!(
        re(&eval("v = [1, 2, 3, 4, 5]\ns = sum(v(v > 2))"), "s"),
        12.0
    );
    assert_eq!(re(&eval("M = [1, 2; 3, 4]\ns = sum(M(M > 2))"), "s"), 7.0);
    assert_eq!(
        re(&eval("v = [1, 2, 3, 4, 5]\nv(v > 3) = 0\ns = sum(v)"), "s"),
        6.0
    );
    let ev = eval("M = [1, 2; 3, 4]\nr = M(M(:, 1) > 1, :)");
    let s: f64 = match val(&ev, "r") {
        Value::Vector(v) => v.iter().map(|c| c.re).sum::<f64>(),
        Value::Matrix(m) => m.iter().map(|c| c.re).sum::<f64>(),
        other => panic!("{other:?}"),
    };
    assert_eq!(s, 7.0);
}

#[test]
fn find_on_a_logical_matches_nonzero_positions() {
    assert_eq!(
        crate::testutil::vec_re(&eval("a = find([1, 0, 3] != 0)"), "a"),
        crate::testutil::vec_re(&eval("a = find([1, 0, 3])"), "a")
    );
    let ev = eval("M = [1, 0; 0, 1]\na = find(M == 1)");
    let idx = crate::testutil::vec_re(&ev, "a");
    assert_eq!(idx, vec![1.0, 4.0]);
}

#[test]
fn empty_scalar_and_shape_mismatch() {
    let ev = eval("a = zeros(1, 0) == zeros(1, 0)");
    match val(&ev, "a") {
        Value::Logical(a) => {
            assert_eq!((a.rows(), a.cols()), (1, 0));
            assert!(a.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(re(&eval("a = sum(zeros(1, 0) == zeros(1, 0))"), "a"), 0.0);
    let err = eval_err("a = [1, 2] == [1, 2, 3]");
    assert!(err.contains("mismatch"), "{err}");
    let err = eval_err("v = [1, 2, 3]\nx = v([true, false])");
    assert!(err.contains("logical"), "{err}");
    let err = eval_err("M = [1, 2; 3, 4]\nx = M([true, false; false, true; true, false])");
    assert!(err.contains("logical") || err.contains("element"), "{err}");
}

#[test]
fn logical_conversion_and_constructors() {
    assert!(matches!(
        val(&eval("a = logical(0)"), "a"),
        Value::Bool(false)
    ));
    assert!(matches!(
        val(&eval("a = logical(-0)"), "a"),
        Value::Bool(false)
    ));
    assert!(matches!(
        val(&eval("a = logical(0.5)"), "a"),
        Value::Bool(true)
    ));
    assert!(matches!(
        val(&eval("a = logical(NaN)"), "a"),
        Value::Bool(true)
    ));
    let err = eval_err("a = logical(1 + 2j)");
    assert!(err.contains("complex"), "{err}");
    let ev = eval("a = true(2, 3)");
    match val(&ev, "a") {
        Value::Logical(a) => {
            assert_eq!((a.rows(), a.cols()), (2, 3));
            assert_eq!(a.count_true(), 6);
        }
        other => panic!("{other:?}"),
    }
    let ev = eval("a = false(1, 4)");
    match val(&ev, "a") {
        Value::Logical(a) => {
            assert_eq!((a.rows(), a.cols()), (1, 4));
            assert_eq!(a.count_true(), 0);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn elementwise_and_or_xor_and_not() {
    let ev = eval("a = and([true, false, true], [false, false, true])");
    match val(&ev, "a") {
        Value::Logical(a) => assert_eq!(a.to_flags(), vec![false, false, true]),
        other => panic!("{other:?}"),
    }
    let ev = eval("a = or([true, false], [false, false])");
    match val(&ev, "a") {
        Value::Logical(a) => assert_eq!(a.to_flags(), vec![true, false]),
        other => panic!("{other:?}"),
    }
    let ev = eval("a = xor([true, false, true], [true, true, false])");
    match val(&ev, "a") {
        Value::Logical(a) => assert_eq!(a.to_flags(), vec![false, true, true]),
        other => panic!("{other:?}"),
    }
    let ev = eval("a = ![true, false, false]");
    match val(&ev, "a") {
        Value::Logical(a) => assert_eq!(a.to_flags(), vec![false, true, true]),
        other => panic!("{other:?}"),
    }
    assert!(matches!(val(&eval("a = !0"), "a"), Value::Bool(true)));
    assert!(matches!(val(&eval("a = !NaN"), "a"), Value::Bool(false)));
}

#[test]
fn double_and_arithmetic_promote() {
    assert_eq!(re(&eval("a = double(true)"), "a"), 1.0);
    let ev = eval("a = double([true, false, true])");
    assert!(matches!(val(&ev, "a"), Value::Vector(_)));
    assert_eq!(crate::testutil::vec_re(&ev, "a"), vec![1.0, 0.0, 1.0]);
    assert_eq!(re(&eval("a = sum([true, false, true] + 0)"), "a"), 2.0);
    assert!(matches!(
        val(&eval("a = all([true, true])"), "a"),
        Value::Bool(true)
    ));
    assert!(matches!(
        val(&eval("a = any([false, false])"), "a"),
        Value::Bool(false)
    ));
    assert!(matches!(
        val(&eval("a = all(false(1, 0))"), "a"),
        Value::Bool(true)
    ));
}

#[test]
fn table_column_stays_logical() {
    let ev = eval("flag = [true, false, true]\nT = table(flag)\na = islogical(T.flag)");
    assert!(matches!(val(&ev, "a"), Value::Bool(true)));
    match val(&ev, "flag") {
        Value::Logical(_) => {}
        other => panic!("column came back as {other:?}"),
    }
}

#[test]
fn display_uses_ones_and_zeros() {
    let ev = eval("L = [true, false, true]");
    let text = val(&ev, "L").to_string();
    assert!(text.contains("logical"), "{text}");
    assert!(text.contains('1') && text.contains('0'), "{text}");
    assert!(!text.contains("1.000000"), "{text}");
}

fn assert_logical_cell(src: &str, name: &str, bit: bool) {
    match val(&eval(src), name) {
        Value::Logical(a) => {
            assert_eq!((a.rows(), a.cols()), (1, 1), "{src}");
            assert_eq!(a.to_flags(), vec![bit], "{src}");
        }
        other => panic!("{src}: expected a 1×1 logical, got {other:?}"),
    }
}

fn class_of(src: &str) -> String {
    match val(&eval(src), "a") {
        Value::Str(s) => s.clone(),
        other => panic!("{src}: {other:?}"),
    }
}

/// One linear or two-dimensional index of a logical array keeps a 1×1
/// logical. It does not collapse to a scalar bool.
#[test]
fn single_index_keeps_a_logical_array() {
    assert_logical_cell("L = [true, false, true]\na = L(1)", "a", true);
    assert_logical_cell("L = [true, false, true]\na = L(2)", "a", false);
    assert_logical_cell("L = [true, false, true]\na = L(end)", "a", true);
    assert_logical_cell("L = [true, false, true]\na = L(1:1)", "a", true);
    assert_logical_cell("L = [true, false, true]\na = L([1])", "a", true);
    assert_eq!(
        class_of("L = [true, false, true]\na = class(L(1))"),
        "logical"
    );
    assert!(matches!(
        val(&eval("L = [true, false, true]\na = islogical(L(1))"), "a"),
        Value::Bool(true)
    ));
    // Column-major: [true, false; false, true] is true, false, false, true.
    assert_logical_cell("L = [true, false; false, true]\na = L(1)", "a", true);
    assert_logical_cell("L = [true, false; false, true]\na = L(2)", "a", false);
    assert_logical_cell("L = [true, false; false, true]\na = L(3)", "a", false);
    assert_logical_cell("L = [true, false; false, true]\na = L(4)", "a", true);
    assert_logical_cell("L = [true, false; false, true]\na = L(1, 2)", "a", false);
    assert_logical_cell("L = [true, false; false, true]\na = L(end, end)", "a", true);
    // The 1×1 result can itself be indexed. A scalar bool cannot.
    assert_logical_cell("L = [true, false, true]\nx = L(2)\na = x(1)", "a", false);
    assert_eq!(
        re(&eval("L = [true, false, true]\na = double(L(1))"), "a"),
        1.0
    );
    assert_eq!(
        re(&eval("L = [true, false, true]\na = double(L(2))"), "a"),
        0.0
    );
    assert_eq!(
        re(&eval("L = [true, false, true]\na = sum(L(1))"), "a"),
        1.0
    );
}

/// `true` selects the first element. `false` selects nothing.
/// A one-element logical is the same subscript (`A(mask(k))`).
/// A same-length mask with one true stays a length-1 vector.
#[test]
fn scalar_logical_subscript_selects_first_or_nothing() {
    assert!(matches!(
        val(&eval("v = [10, 20, 30, 40]\na = v(true)"), "a"),
        Value::Scalar(x) if *x == 10.0
    ));
    let ev = eval("v = [10, 20, 30, 40]\na = v(false)");
    match val(&ev, "a") {
        Value::Vector(v) => assert_eq!(v.len(), 0),
        other => panic!("v(false) should be an empty vector, got {other:?}"),
    }
    assert!(matches!(
        val(&eval("v = [10, 20, 30, 40]\na = v(true(1, 1))"), "a"),
        Value::Scalar(x) if *x == 10.0
    ));
    assert!(matches!(
        val(&eval("v = [10, 20, 30, 40]\na = v(logical(1))"), "a"),
        Value::Scalar(x) if *x == 10.0
    ));
    let ev = eval("v = [10, 20, 30, 40]\na = v(false(1, 1))");
    match val(&ev, "a") {
        Value::Vector(v) => assert_eq!(v.len(), 0),
        other => panic!("v(false(1, 1)) should be empty, got {other:?}"),
    }
    assert!(matches!(
        val(
            &eval("L = [true, false, true]\nv = [10, 20, 30, 40]\na = v(L(1))"),
            "a"
        ),
        Value::Scalar(x) if *x == 10.0
    ));
    let ev = eval("L = [true, false, true]\nv = [10, 20, 30, 40]\na = v(L(2))");
    match val(&ev, "a") {
        Value::Vector(v) => assert_eq!(v.len(), 0),
        other => panic!("v(L(2)) should be empty, got {other:?}"),
    }
    assert!(matches!(
        val(&eval("M = [1, 2; 3, 4]\na = M(true)"), "a"),
        Value::Scalar(x) if *x == 1.0
    ));
    let ev = eval("M = [1, 2; 3, 4]\na = M(false)");
    match val(&ev, "a") {
        Value::Vector(v) => assert_eq!(v.len(), 0),
        other => panic!("M(false) should be empty, got {other:?}"),
    }
    assert_eq!(
        crate::testutil::vec_re(&eval("M = [1, 2; 3, 4]\na = M(true, :)"), "a"),
        vec![1.0, 2.0]
    );
    let ev = eval("M = [1, 2; 3, 4]\na = M(false, :)");
    match val(&ev, "a") {
        Value::Matrix(m) => assert_eq!((m.nrows(), m.ncols()), (0, 2)),
        other => panic!("M(false, :) should be 0×2, got {other:?}"),
    }
    // One true in a same-length mask stays a vector, not a scalar and not empty.
    let ev = eval("v = [10, 20, 30, 40]\na = v([false, true, false, false])");
    match val(&ev, "a") {
        Value::Vector(v) => {
            assert_eq!(v.len(), 1);
            assert!((v[0].re - 20.0).abs() < 1e-12);
        }
        other => panic!("one-true mask should stay a vector, got {other:?}"),
    }
    let err = eval_err("v = [1, 2, 3]\nx = v([true, false])");
    assert!(err.contains("logical"), "{err}");
    let err = eval_err("v = zeros(1, 0)\na = v(true)");
    assert!(err.contains("bounds") || err.contains("index"), "{err}");
    let ev = eval("v = zeros(1, 0)\na = v(false)");
    match val(&ev, "a") {
        Value::Vector(v) => assert_eq!(v.len(), 0),
        other => panic!("empty(false) should be empty, got {other:?}"),
    }
    assert_logical_cell("L = [true, false, true]\na = L(true)", "a", true);
    let ev = eval("L = [true, false, true]\na = L(false)");
    match val(&ev, "a") {
        Value::Logical(a) => {
            assert_eq!((a.rows(), a.cols()), (1, 0));
            assert!(a.is_empty());
        }
        other => panic!("L(false) should be an empty logical, got {other:?}"),
    }
    let ev = eval("v = [10, 20, 30]\nv(true) = 7");
    assert!(matches!(val(&ev, "v"), Value::Vector(v) if (v[0].re - 7.0).abs() < 1e-12));
    assert_eq!(
        re(&eval("v = [10, 20, 30]\nv(false) = 9\na = sum(v)"), "a"),
        60.0
    );
    assert_eq!(
        re(&eval("M = [1, 2; 3, 4]\nM(true) = 9\na = M(1)"), "a"),
        9.0
    );
    assert_eq!(
        re(&eval("M = [1, 2; 3, 4]\nM(false) = 8\na = sum(sum(M))"), "a"),
        10.0
    );
}

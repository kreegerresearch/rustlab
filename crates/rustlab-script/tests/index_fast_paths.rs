//! Indexing shortcuts for real-stored arrays (one element, one column, a
//! position list, a one-column write) give what complex storage gives, and
//! a subscript that uses `end` after a nested index sees the outer `end`.

use rustlab_script::eval::shared::set_real_storage;
use rustlab_script::{Evaluator, Value};

fn run(src: &str) -> Result<Evaluator, String> {
    let src = format!("{src}\n");
    let tokens = rustlab_script::lexer::tokenize(&src).unwrap();
    let stmts = rustlab_script::parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    ev.run(&stmts).map(|()| ev).map_err(|e| e.to_string())
}

/// Shape and real parts of a numeric value.
fn shape_and_values(v: &Value) -> (Vec<usize>, Vec<f64>) {
    match v {
        Value::Scalar(x) => (vec![], vec![*x]),
        Value::Vector(a) => (vec![a.len()], a.iter().map(|z| z.re).collect()),
        Value::Matrix(m) => (vec![m.nrows(), m.ncols()], m.iter().map(|z| z.re).collect()),
        other => panic!("not numeric: {other:?}"),
    }
}

const SETUP: &str = "M = reshape(1:12, 3, 4); v = 10:10:60; c = (1:4)'; P = [3 1 2 6 6 4];
                     b = [1 2]; L = M > 5;";

/// Each read gives the same variant, shape, values, or error message with
/// real and with complex storage.
#[test]
fn reads_match_complex_storage() {
    let reads = [
        "M(2, 3)",
        "M(:, 2)",
        "M(3, 4)",
        "c(:, 1)",
        "c(4, 1)",
        "v(P)",
        "c(P(1:3))",
        "M(P)",
        "M(0, 1)",
        "M(1.5, 1)",
        "M(4, 1)",
        "M(1, 5)",
        "M(:, 0)",
        "M(:, 2.5)",
        "M(:, 9)",
        "M(NaN, 1)",
        "v([1 0 2])",
        "v([1 NaN])",
        "v([1 7])",
        "v([2.5 1])",
        "v([-1 2])",
        "c([1 9])",
        "M([1 13])",
    ];
    for r in reads {
        let src = format!("{SETUP}\nx = {r};");
        set_real_storage(Some(true));
        let real = run(&src);
        set_real_storage(Some(false));
        let complex = run(&src);
        set_real_storage(None);
        match (real, complex) {
            (Ok(a), Ok(b)) => {
                let (x, y) = (a.get("x").unwrap(), b.get("x").unwrap());
                assert_eq!(
                    std::mem::discriminant(x),
                    std::mem::discriminant(y),
                    "{r}: {x:?} vs {y:?}"
                );
                assert_eq!(shape_and_values(x), shape_and_values(y), "{r}");
            }
            (Err(a), Err(b)) => assert_eq!(a, b, "{r}"),
            (a, b) => panic!("{r}: real {:?} vs complex {:?}", a.is_ok(), b.is_ok()),
        }
    }
}

#[test]
fn element_and_column_reads() {
    let ev = run(&format!(
        "{SETUP}\ne = M(2, 3); k = M(:, 2); g = v(P); h = c(P(1:3));"
    ))
    .unwrap();
    assert!(matches!(ev.get("e"), Some(Value::Scalar(x)) if *x == 8.0));
    assert_eq!(
        shape_and_values(ev.get("k").unwrap()),
        (vec![3, 1], vec![4.0, 5.0, 6.0])
    );
    assert_eq!(
        shape_and_values(ev.get("g").unwrap()),
        (vec![6], vec![30.0, 10.0, 20.0, 60.0, 60.0, 40.0])
    );
    assert_eq!(
        shape_and_values(ev.get("h").unwrap()),
        (vec![3, 1], vec![3.0, 1.0, 2.0])
    );
}

/// One-column writes from a column, a row, a scalar, and a logical column.
#[test]
fn one_column_writes() {
    for real in [true, false] {
        set_real_storage(Some(real));
        let ev = run("A = zeros(3, 4); A(:, 2) = [7; 8; 9]; A(:, 4) = [1 2 3];
                      A(:, 1) = 5; A(:, 3) = [4; 0; 6] > 3;")
        .unwrap();
        set_real_storage(None);
        assert_eq!(
            shape_and_values(ev.get("A").unwrap()),
            (
                vec![3, 4],
                vec![5.0, 7.0, 1.0, 1.0, 5.0, 8.0, 0.0, 2.0, 5.0, 9.0, 1.0, 3.0]
            ),
            "real storage {real}"
        );
    }
    assert!(run("A = zeros(3, 4); A(:, 2) = [1 2];").is_err());
}

/// `end` after a nested index in the same subscript is the outer array's
/// `end` (MATLAB). Before 0.3.91 the nested read removed it: "undefined
/// variable 'end'".
#[test]
fn end_after_a_nested_index() {
    let ev = run(&format!(
        "{SETUP}\nx = v(b(1) + end - 6); y = M(b(2), end); z = M(end, b(1));
         w = v(end - b(1)); v(b(1) + end) = 99;"
    ))
    .unwrap();
    let s = |n: &str| match ev.get(n) {
        Some(Value::Scalar(x)) => *x,
        other => panic!("{n}: {other:?}"),
    };
    assert_eq!((s("x"), s("y"), s("z"), s("w")), (10.0, 11.0, 3.0, 50.0));
    assert_eq!(
        shape_and_values(ev.get("v").unwrap()).1,
        vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 99.0]
    );
}

/// Same-shape real matrices combine element by element.
#[test]
fn same_shape_columns_are_elementwise() {
    let ev = run("a = [1; 2; 3]; b = [4; 5; 6]; s = a + b; p = a .* b; q = a .^ b;").unwrap();
    assert_eq!(
        shape_and_values(ev.get("s").unwrap()),
        (vec![3, 1], vec![5.0, 7.0, 9.0])
    );
    assert_eq!(
        shape_and_values(ev.get("p").unwrap()).1,
        vec![4.0, 10.0, 18.0]
    );
    assert_eq!(
        shape_and_values(ev.get("q").unwrap()).1,
        vec![1.0, 32.0, 729.0]
    );
}

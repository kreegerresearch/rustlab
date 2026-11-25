//! `size`, `numel`, `length`, `ndims`, and `isempty` on empty values,
//! strings, cells, structs, and function handles.
//!
//! rustlab has one string type for `"..."` and `'...'`, and it behaves as a
//! MATLAB char row vector (`s(3)` is a character, `length("abc")` is 3). So
//! a string is `1×N` with `N` characters, and the empty string is `0×0`
//! like MATLAB `''`. MATLAB's string scalar `""` is `1×1`; rustlab has no
//! string scalar. `{...}` is a cell of strings: `{}` is `0×0`, `{"a","b"}`
//! is `1×2`. A zero-length numeric vector (`[]`, `1:0`) is `1×0`.

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
        Some(Value::Matrix(m)) => m.iter().map(|c| c.re).collect(),
        other => panic!("{name}: expected a number or array, got {other:?}"),
    }
}

fn truth(ev: &Evaluator, name: &str) -> bool {
    match ev.get(name) {
        Some(Value::Bool(b)) => *b,
        other => panic!("{name}: expected a bool, got {other:?}"),
    }
}

/// `size(x)`, `numel(x)`, `length(x)`, `ndims(x)`, `isempty(x)`.
fn shape_of(setup: &str, expr: &str) -> (Vec<f64>, f64, f64, f64, bool) {
    let ev = eval(&format!(
        "{setup}\ns = size({expr});\nn = numel({expr});\nl = length({expr});\nd = ndims({expr});\ne = isempty({expr});"
    ));
    (
        reals(&ev, "s"),
        reals(&ev, "n")[0],
        reals(&ev, "l")[0],
        reals(&ev, "d")[0],
        truth(&ev, "e"),
    )
}

#[test]
fn empty_cell_is_0_by_0() {
    assert_eq!(shape_of("", "{}"), (vec![0.0, 0.0], 0.0, 0.0, 2.0, true));
    let ev = eval("c = {};\na = size(c, 1);\nb = size(c, 2);\n[r, k] = size(c);");
    assert_eq!(reals(&ev, "a"), [0.0]);
    assert_eq!(reals(&ev, "b"), [0.0]);
    assert_eq!(reals(&ev, "r"), [0.0]);
    assert_eq!(reals(&ev, "k"), [0.0]);
}

#[test]
fn nonempty_cell_is_a_row() {
    assert_eq!(
        shape_of("", "{\"a\", \"bc\", \"\"}"),
        (vec![1.0, 3.0], 3.0, 3.0, 2.0, false)
    );
}

#[test]
fn empty_string_is_0_by_0_for_both_quote_styles() {
    for lit in ["\"\"", "''"] {
        assert_eq!(
            shape_of("", lit),
            (vec![0.0, 0.0], 0.0, 0.0, 2.0, true),
            "{lit}"
        );
        let ev = eval(&format!(
            "x = {lit};\na = size(x, 1);\nb = size(x, 2);\n[r, k] = size(x);"
        ));
        assert_eq!(reals(&ev, "a"), [0.0], "{lit}");
        assert_eq!(reals(&ev, "b"), [0.0], "{lit}");
        assert_eq!(reals(&ev, "r"), [0.0], "{lit}");
        assert_eq!(reals(&ev, "k"), [0.0], "{lit}");
    }
}

#[test]
fn string_is_a_char_row() {
    for lit in ["\"hello\"", "'hello'"] {
        assert_eq!(
            shape_of("", lit),
            (vec![1.0, 5.0], 5.0, 5.0, 2.0, false),
            "{lit}"
        );
    }
    let ev = eval("[r, k] = size(\"hello\");\nc = size('hello', 2);");
    assert_eq!(reals(&ev, "r"), [1.0]);
    assert_eq!(reals(&ev, "k"), [5.0]);
    assert_eq!(reals(&ev, "c"), [5.0]);
}

#[test]
fn string_length_counts_characters_not_bytes() {
    // `s(k)` indexes characters, so the shape does too.
    assert_eq!(
        shape_of("", "\"héllo\""),
        (vec![1.0, 5.0], 5.0, 5.0, 2.0, false)
    );
    let ev = eval("s = \"héllo\";\nlast = s(length(s));");
    assert!(matches!(ev.get("last"), Some(Value::Str(c)) if c == "o"));
}

#[test]
fn struct_and_function_handles_are_1_by_1() {
    let one = (vec![1.0, 1.0], 1.0, 1.0, 2.0, false);
    assert_eq!(shape_of("st = struct(\"a\", 1);", "st"), one);
    assert_eq!(shape_of("", "@sin"), one);
    assert_eq!(shape_of("f = @(x) x + 1;", "f"), one);
}

#[test]
fn size_dim_past_the_last_dimension_is_1() {
    let ev = eval(
        "a = size(zeros(2, 3), 3);\nb = size(zeros(0, 3), 7);\nc = size(zeros3(2, 3, 4), 4);\nd = size({}, 3);\ne = size(\"ab\", 3);",
    );
    for name in ["a", "b", "c", "d", "e"] {
        assert_eq!(reals(&ev, name), [1.0], "{name}");
    }
}

#[test]
fn size_dim_must_be_a_positive_integer() {
    assert!(eval_err("size(zeros(2, 3), 0)").contains("size"));
    assert!(eval_err("size(zeros(2, 3), 1.5)").contains("size"));
    assert!(eval_err("size(zeros(2, 3), -1)").contains("size"));
}

#[test]
fn multi_output_size_folds_and_pads_like_matlab() {
    let ev = eval(
        "[r, c] = size(zeros(2, 3));\n\
         [r3, c3, p3] = size(zeros(2, 3));\n\
         [tr, tc] = size(zeros3(2, 3, 4));\n\
         [a, b, c4, d4] = size(zeros3(2, 3, 4));\n\
         [er, ec] = size(zeros(0, 3));",
    );
    assert_eq!(reals(&ev, "r"), [2.0]);
    assert_eq!(reals(&ev, "c"), [3.0]);
    assert_eq!(reals(&ev, "r3"), [2.0]);
    assert_eq!(reals(&ev, "c3"), [3.0]);
    assert_eq!(reals(&ev, "p3"), [1.0]);
    // Fewer outputs than dimensions: the last output is the product of the rest.
    assert_eq!(reals(&ev, "tr"), [2.0]);
    assert_eq!(reals(&ev, "tc"), [12.0]);
    assert_eq!(reals(&ev, "a"), [2.0]);
    assert_eq!(reals(&ev, "b"), [3.0]);
    assert_eq!(reals(&ev, "c4"), [4.0]);
    assert_eq!(reals(&ev, "d4"), [1.0]);
    assert_eq!(reals(&ev, "er"), [0.0]);
    assert_eq!(reals(&ev, "ec"), [3.0]);
}

#[test]
fn empty_table_has_length_0() {
    let (size, numel, length, ndims, empty) = shape_of("T = table(zeros(0, 1), zeros(0, 1));", "T");
    assert_eq!(size, [0.0, 2.0]);
    assert_eq!((numel, length, ndims, empty), (0.0, 0.0, 2.0, true));
}

#[test]
fn zero_length_vector_is_1_by_0() {
    for expr in ["[]", "1:0"] {
        assert_eq!(
            shape_of("", expr),
            (vec![1.0, 0.0], 0.0, 0.0, 2.0, true),
            "{expr}"
        );
    }
}

#[test]
fn the_five_queries_agree() {
    // numel = prod(size); isempty = (numel == 0); length = 0 if empty else
    // max(size); ndims = number of size entries.
    let setup = "S = sparse([1, 2], [1, 3], [1, 2], 2, 4);\nst = struct(\"a\", 1);\nT = table(zeros(0, 1), zeros(0, 1));";
    for expr in [
        "{}",
        "{\"a\"}",
        "\"\"",
        "''",
        "\"héllo\"",
        "[]",
        "zeros(0, 3)",
        "zeros(3, 0)",
        "zeros(2, 5)",
        "true(0, 2)",
        "int32(zeros(0, 2))",
        "S",
        "S(:, [])",
        "zeros3(2, 0, 3)",
        "zeros3(2, 3, 4)",
        "st",
        "@sin",
        "T",
        "5",
        "true",
    ] {
        let (size, numel, length, ndims, empty) = shape_of(setup, expr);
        let prod: f64 = size.iter().product();
        assert_eq!(numel, prod, "numel {expr}");
        assert_eq!(empty, numel == 0.0, "isempty {expr}");
        let max = size.iter().cloned().fold(0.0, f64::max);
        assert_eq!(length, if empty { 0.0 } else { max }, "length {expr}");
        assert_eq!(ndims, size.len() as f64, "ndims {expr}");
    }
}

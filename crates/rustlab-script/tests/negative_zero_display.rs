//! Negative zero in displayed values (rustlab_em lesson-review item #4).
//!
//! MATLAB stores IEEE `-0` but its value display (`x = -0`, `disp`, matrix
//! and complex output) prints `0`. Explicit printf-style formats are C
//! conversions and keep the sign: `sprintf('%g', -0)` is `-0` in MATLAB.
//! Only an exact zero loses its sign; a tiny negative such as `-1e-17` still
//! prints `-0.000000` in a matrix, as MATLAB prints `-0.0000`.

use rustlab_script::eval::output;
use rustlab_script::{Evaluator, Value};

fn run(src: &str) -> Result<Evaluator, String> {
    let src = format!("{src}\n");
    let tokens = rustlab_script::lexer::tokenize(&src).map_err(|e| e.to_string())?;
    let stmts = rustlab_script::parser::parse(tokens).map_err(|e| e.to_string())?;
    let mut ev = Evaluator::new();
    ev.run(&stmts).map_err(|e| e.to_string())?;
    Ok(ev)
}

fn printed(src: &str) -> String {
    output::start_capture();
    let r = run(src);
    let out = output::stop_capture();
    r.unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"));
    out
}

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name) {
        Some(Value::Scalar(x)) => *x,
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

#[test]
fn auto_display_of_negative_zero_is_zero() {
    assert_eq!(printed("x = -0"), "x = 0\n");
    assert_eq!(printed("y = 0 * -1"), "y = 0\n");
}

#[test]
fn disp_and_print_show_zero() {
    assert_eq!(printed("disp(-0)"), "0\n");
    assert_eq!(printed("print(-0)"), "0\n");
    assert_eq!(printed("print(-0, 1)"), "0 1\n");
}

#[test]
fn stored_value_keeps_its_sign() {
    let ev = run("x = -0; a = 1 / x; b = 1 / -0; c = 1 / (0 * -1);").unwrap();
    let x = scalar(&ev, "x");
    assert_eq!(x, 0.0);
    assert!(x.is_sign_negative(), "x must still be -0");
    assert_eq!(scalar(&ev, "a"), f64::NEG_INFINITY);
    assert_eq!(scalar(&ev, "b"), f64::NEG_INFINITY);
    assert_eq!(scalar(&ev, "c"), f64::NEG_INFINITY);
}

#[test]
fn vectors_and_matrices_show_zero() {
    assert_eq!(
        printed("v = [-0, 1, -0]"),
        "v = [1×3]  0.000000  1.000000  0.000000\n"
    );
    assert_eq!(
        printed("M = [-0, 1; 2, -0]"),
        "M = Matrix(2x2)\n  [0.000000, 1.000000]\n  [2.000000, 0.000000]\n"
    );
    assert_eq!(printed("disp([-0, 1])"), "[1×2]  0.000000  1.000000\n");
}

#[test]
fn tiny_negative_values_keep_their_sign() {
    // Not a zero: MATLAB prints `-0.0000` here too.
    assert_eq!(
        printed("v = [-1e-17, 1]"),
        "v = [1×2]  -0.000000  1.000000\n"
    );
}

#[test]
fn complex_parts_show_zero() {
    // conj(1+0j) stores an imaginary -0; it printed as `1+-0j`.
    assert_eq!(printed("q = conj(1 + 0j)"), "q = 1+0j\n");
    assert_eq!(printed("disp(conj(1 + 0j))"), "1+0j\n");
    // Real part -0.
    assert_eq!(printed("z = -(0 - 1j)"), "z = 0+1j\n");
    let ev = run("z = -(0 - 1j); r = 1 / real(z);").unwrap();
    assert_eq!(scalar(&ev, "r"), f64::NEG_INFINITY);
    // Both parts -0.
    assert_eq!(printed("w = conj(-(0 + 0j))"), "w = 0+0j\n");
    // Complex matrix cells.
    assert_eq!(
        printed("C = [conj(1 + 2j); -(0 - 1j)]"),
        "C = Matrix(2x1)\n  [1.000000-2.000000j]\n  [0.000000+1.000000j]\n"
    );
}

#[test]
fn tensor3_pages_show_zero() {
    assert_eq!(
        printed("T = -zeros3(1, 1, 2)"),
        "T = tensor3 [1×1×2]\n  (:, :, 1) =\n    [0.000000]\n  (:, :, 2) =\n    [0.000000]\n"
    );
}

#[test]
fn struct_and_table_show_zero() {
    assert_eq!(printed("s.a = -0;\ndisp(s)"), "struct {a: 0}\n");
    let out = printed("T = table([-0; 1])");
    assert!(!out.contains("-0"), "table printed a signed zero:\n{out}");
}

/// Output after the `format NAME` line, without that command's own echo.
fn printed_in_format(name: &str, src: &str) -> String {
    let out = printed(&format!("format {name};\n{src}"));
    out.lines()
        .filter(|l| !l.starts_with("format:"))
        .map(|l| format!("{l}\n"))
        .collect()
}

#[test]
fn number_formats_show_zero_except_hex() {
    assert_eq!(
        printed_in_format("long", "x = -0"),
        "x = 0.000000000000000\n"
    );
    assert_eq!(printed_in_format("commas", "x = -0"), "x = 0\n");
    assert_eq!(
        printed_in_format("commas", "v = [-0, 1]"),
        "v = [1×2]  0.000000  1.000000\n"
    );
    // `format hex` shows the stored bits, as MATLAB's does.
    assert_eq!(printed_in_format("hex", "x = -0"), "x = 8000000000000000\n");
}

#[test]
fn printf_formats_stay_c_exact() {
    // MATLAB: sprintf('%g', -0) is '-0'; %f / %e follow C.
    let ev = run(r#"s = sprintf("%g|%f|%e|%.2f|%5.1f", -0, -0, -0, -0, -0);"#).unwrap();
    match ev.get("s") {
        Some(Value::Str(s)) => assert_eq!(s, "-0|-0.000000|-0.000000e+00|-0.00| -0.0"),
        other => panic!("{other:?}"),
    }
    // Integer conversions have no signed zero.
    let ev = run(r#"s = sprintf("%d|%i|%u", -0, -0, -0);"#).unwrap();
    match ev.get("s") {
        Some(Value::Str(s)) => assert_eq!(s, "0|0|0"),
        other => panic!("{other:?}"),
    }
    assert_eq!(printed(r#"fprintf("%g\n", -0)"#), "-0\n");
    // `+ 0` clears the sign, the same idiom as in MATLAB.
    assert_eq!(printed(r#"fprintf("%g\n", -0 + 0)"#), "0\n");
}

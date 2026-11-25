//! Reducing `parmap`: `parmap(f, xs, "sum")` and `parsum(f, xs)`.
//!
//! The sum is a fixed pairwise tree over trial indices, so the bits do not
//! depend on thread timing or the number of worker threads. The tests pin
//! that order against an explicit script-side tree built from
//! `parmap(f, xs)` rows (same per-task seeds, so the same summands).

use rustlab_script::{Evaluator, Value};

fn eval(src: &str) -> Result<Evaluator, rustlab_script::ScriptError> {
    let mut ev = Evaluator::new();
    let tokens = rustlab_script::lexer::tokenize(&format!("{src}\n"))?;
    let stmts = rustlab_script::parser::parse(tokens)?;
    ev.run(&stmts)?;
    Ok(ev)
}

fn run(src: &str) -> Evaluator {
    eval(src).unwrap_or_else(|e| panic!("script failed: {e}"))
}

fn run_err(src: &str) -> String {
    match eval(src) {
        Ok(_) => panic!("expected an error from: {src}"),
        Err(e) => e.to_string(),
    }
}

/// Shape plus every element as (re, im) bit patterns, row-major.
fn bits(v: &Value) -> (Vec<usize>, Vec<(u64, u64)>) {
    match v {
        Value::Scalar(x) => (vec![], vec![(x.to_bits(), 0)]),
        Value::Complex(c) => (vec![], vec![(c.re.to_bits(), c.im.to_bits())]),
        Value::Vector(a) => (
            vec![a.len()],
            a.iter()
                .map(|c| (c.re.to_bits(), c.im.to_bits()))
                .collect(),
        ),
        Value::Matrix(m) => (
            m.shape().to_vec(),
            m.iter()
                .map(|c| (c.re.to_bits(), c.im.to_bits()))
                .collect(),
        ),
        other => panic!("unexpected {}", other.type_name()),
    }
}

fn reals(v: &Value) -> Vec<f64> {
    match v {
        Value::Scalar(x) => vec![*x],
        Value::Vector(a) => a.iter().map(|c| c.re).collect(),
        Value::Matrix(m) => m.iter().map(|c| c.re).collect(),
        other => panic!("unexpected {}", other.type_name()),
    }
}

fn get<'a>(ev: &'a Evaluator, name: &str) -> &'a Value {
    ev.get(name).unwrap_or_else(|| panic!("{name} not set"))
}

#[test]
fn scalar_sum_both_spellings() {
    let ev = run("a = parsum(@(k) k^2, 1:10); b = parmap(@(k) k^2, 1:10, \"sum\");");
    assert!(matches!(get(&ev, "a"), Value::Scalar(x) if *x == 385.0));
    assert!(matches!(get(&ev, "b"), Value::Scalar(x) if *x == 385.0));
}

#[test]
fn complex_scalar_sum() {
    let ev = run("z = parsum(@(k) k + 1j * k, 1:4);");
    match get(&ev, "z") {
        Value::Complex(c) => assert_eq!((c.re, c.im), (10.0, 10.0)),
        other => panic!("expected complex, got {other:?}"),
    }
}

#[test]
fn empty_input_sums_to_zero() {
    let ev = run("z = parsum(@(k) k, []);");
    assert!(matches!(get(&ev, "z"), Value::Scalar(x) if *x == 0.0));
}

#[test]
fn integer_vectors_equal_sum_of_parmap_exactly() {
    let ev = run("f = @(k) k * (1:1000) + mod(k, 3);\n\
         a = parsum(f, 1:8);\n\
         b = sum(parmap(f, 1:8));");
    assert_eq!(reals(get(&ev, "a")), reals(get(&ev, "b")));
    assert!(matches!(get(&ev, "a"), Value::Vector(v) if v.len() == 1000));
}

#[test]
fn random_vectors_match_sum_of_parmap_to_rounding() {
    let ev = run("seed(7);\n\
         f = @(k) randn(1, 5000)(1, :);\n\
         a = parsum(f, 1:8);\n\
         b = sum(parmap(f, 1:8));");
    let (a, b) = (reals(get(&ev, "a")), reals(get(&ev, "b")));
    assert_eq!(a.len(), 5000);
    for (x, y) in a.iter().zip(&b) {
        assert!((x - y).abs() <= 1e-12 * (1.0 + y.abs()), "{x} vs {y}");
    }
}

#[test]
fn matrix_results_keep_their_shape() {
    let ev = run("seed(3);\n\
         f = @(k) k * ones(3, 4) + randn(3, 4);\n\
         a = parsum(f, 1:6);\n\
         P = parmap(f, 1:6);\n\
         b = zeros(3, 4);\n\
         for k = 1:6\n b = b + P(:, :, k);\n end");
    let (sa, _) = bits(get(&ev, "a"));
    assert_eq!(sa, vec![3, 4]);
    let (a, b) = (reals(get(&ev, "a")), reals(get(&ev, "b")));
    for (x, y) in a.iter().zip(&b) {
        assert!((x - y).abs() <= 1e-12 * (1.0 + y.abs()), "{x} vs {y}");
    }
}

#[test]
fn column_results_stay_columns() {
    let ev = run("a = parsum(@(k) k * ones(5, 1), 1:4);");
    match get(&ev, "a") {
        Value::Matrix(m) => {
            assert_eq!(m.shape(), &[5, 1]);
            assert!(m.iter().all(|c| c.re == 10.0));
        }
        other => panic!("expected 5×1 matrix, got {other:?}"),
    }
}

/// The documented order: `S(lo, hi) = S(lo, mid) + S(mid, hi)` with
/// `mid = lo + ceil(n / 2)`, earlier trials on the left.
#[test]
fn reduction_order_is_the_documented_tree() {
    let ev = run("seed(11);\n\
         f = @(k) randn(1, 777)(1, :) * 10^mod(k, 5);\n\
         M = parmap(f, 1:5);\n\
         ref5 = ((M(1, :) + M(2, :)) + M(3, :)) + (M(4, :) + M(5, :));\n\
         s5 = parsum(f, 1:5);\n\
         N = parmap(f, 1:8);\n\
         ref8 = ((N(1, :) + N(2, :)) + (N(3, :) + N(4, :))) + ((N(5, :) + N(6, :)) + (N(7, :) + N(8, :)));\n\
         s8 = parmap(f, 1:8, \"sum\");\n\
         ref3 = (N(1, :) + N(2, :)) + N(3, :);\n\
         s3 = parsum(f, 1:3);");
    for (s, r) in [("s5", "ref5"), ("s8", "ref8"), ("s3", "ref3")] {
        assert_eq!(bits(get(&ev, s)).1, bits(get(&ev, r)).1, "{s} vs {r}");
    }
}

#[test]
fn deterministic_across_runs_and_worker_counts() {
    // Long enough rows to take the element-parallel add path.
    let src = "seed(5);\n\
         a = parsum(@(k) randn(1, 140000)(1, :) * k, 1:13);\n\
         s = parsum(@(k) sin(k) * 1e-3 + k, 1:37);\n\
         m = parsum(@(k) randn(17, 9) / k, 1:11);";
    let reference = {
        let ev = run(src);
        ["a", "s", "m"].map(|n| bits(get(&ev, n)))
    };
    for threads in [1, 2, 3, 4, 7] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        for _ in 0..2 {
            let got = pool.install(|| {
                let ev = run(src);
                ["a", "s", "m"].map(|n| bits(get(&ev, n)))
            });
            assert_eq!(got, reference, "{threads} threads");
        }
    }
}

#[test]
fn vector_length_mismatch_errors() {
    let err = run_err("y = parsum(@(k) ones(1, k + 1)(1, :), 1:3);");
    assert!(err.contains("parsum"), "msg: {err}");
    assert!(err.contains("trial 2"), "msg: {err}");
    assert!(
        err.contains("length 2") && err.contains("length 3"),
        "msg: {err}"
    );
}

#[test]
fn matrix_shape_mismatch_errors() {
    let err = run_err("y = parmap(@(k) ones(2, k + 1), 1:3, \"sum\");");
    assert!(err.contains("parmap"), "msg: {err}");
    assert!(err.contains("trial 2"), "msg: {err}");
    assert!(err.contains("2×2") && err.contains("2×3"), "msg: {err}");
}

#[test]
fn scalar_then_vector_mismatch_errors() {
    let err = run_err("y = parsum(@(k) ones(1, k)(1, :), 1:2);");
    assert!(err.contains("trial 2"), "msg: {err}");
    assert!(err.contains("same shape"), "msg: {err}");
}

#[test]
fn unsupported_result_type_errors() {
    let err = run_err("y = parsum(@(k) \"text\", 1:2);");
    assert!(err.contains("parsum") && err.contains("trial 1"), "msg: {err}");
}

#[test]
fn unknown_reduction_errors() {
    let err = run_err("y = parmap(@(k) k, 1:3, \"max\");");
    assert!(err.contains("\"sum\""), "msg: {err}");
}

#[test]
fn lambda_error_names_the_trial() {
    let err = run_err("y = parsum(@(k) error(\"boom\") * (k == 3) + k, 1:5);");
    assert!(err.contains("boom"), "msg: {err}");
}

#[test]
fn first_failing_trial_is_reported() {
    let err = run_err(
        "function r = g(k)\n if k >= 3\n error(sprintf(\"bad %d\", k));\n end\n r = k;\nend\n\
         y = parsum(@g, 1:8);",
    );
    assert!(err.contains("trial 3") && err.contains("bad 3"), "msg: {err}");
}

#[test]
fn impure_builtins_are_refused() {
    let err = run_err("y = parsum(@(k) clf(), 1:2);");
    assert!(err.contains("clf"), "msg: {err}");
}

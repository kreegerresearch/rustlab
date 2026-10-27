//! Lambda capture: free variables only, snapshotted at creation.
//!
//! Before this change `Expr::Lambda` did `captured_env: self.env.clone()`
//! (`eval/mod.rs`). A 2000×2000 complex matrix is 64_000_000 bytes, and
//! each new lambda cloned every previous lambda as well. Measured on a
//! debug build (numeric payload, nested captures included):
//!
//! | scenario | before |
//! |---|---|
//! | one lambda beside `zeros(2000,2000)` that only closes over scalar `a` | 64_000_088 bytes, 10 keys, the matrix included |
//! | `make = @(g) @(x) x+g+a` then `inner = make(10)`, same matrix | each closure 64_000_088 / 64_000_096 bytes; create 0.206 s |
//! | five rebinds `f = @(x) f(x)+1` beside that matrix | 384_000_520 bytes, 6 matrix copies, create 1.689 s |
//! | eight lambdas kept beside `zeros(128,128)` (262_144 bytes) | copies 1,2,4,…,128; total 66_867_120 bytes |
//!
//! Eight of those 128×128 closures project to about 15.2 GiB of matrix
//! payload at 2000×2000 (255 copies × 64_000_000). The 20-rebind
//! overwrite of the 2000×2000 matrix was not timed to completion: the
//! debug process was still cloning at ~2.5 GB RSS after several
//! minutes. The assertions below are the after state (debug build,
//! numeric payload). An unreferenced matrix contributes 0 copies.
//!
//! | scenario | after |
//! |---|---|
//! | one lambda beside `zeros(2000,2000)` closing over scalar `a` | 88 bytes, user keys `{a}`, 0 matrix copies |
//! | `make = @(g) @(x) x+g+a` then `inner = make(10)`, same matrix | 88 / 96 bytes, 0 copies, create 67 µs |
//! | five rebinds `f = @(x) f(x)+1` beside that matrix | 480 bytes, 0 copies, create 80 µs |
//! | twenty overwrites `f = @(x) x+1` beside that matrix | 80 bytes, 0 copies, create 148 µs |
//! | eight lambdas kept beside `zeros(128,128)` | 640 bytes total, 0 copies, create 777 µs |
//! | four lambdas kept beside `zeros(2000,2000)` | 320 bytes total, create 79 µs |

use std::collections::BTreeSet;
use std::time::Instant;

use rustlab_script::{Evaluator, Value};

const CONSTS: &[&str] = &["i", "j", "pi", "e", "Inf", "NaN", "true", "false", "stdin"];

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

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name).unwrap() {
        Value::Scalar(n) => *n,
        other => panic!("{name}: expected scalar, got {}", other.type_name()),
    }
}

fn lambda<'a>(ev: &'a Evaluator, name: &str) -> &'a Value {
    let v = ev.get(name).unwrap();
    assert!(
        matches!(v, Value::Lambda { .. }),
        "{name}: expected lambda, got {}",
        v.type_name()
    );
    v
}

fn owned_numeric_bytes(v: &Value) -> u64 {
    match v {
        Value::Scalar(_) | Value::Bool(_) => 8,
        Value::Complex(_) => 16,
        Value::Vector(x) => (x.len() * 16) as u64,
        Value::Matrix(m) => (m.len() * 16) as u64,
        Value::Tensor3(t) => (t.len() * 16) as u64,
        Value::Lambda { captured_env, .. } => captured_env.values().map(owned_numeric_bytes).sum(),
        Value::Struct(fields) => fields.values().map(owned_numeric_bytes).sum(),
        Value::Tuple(items) => items.iter().map(owned_numeric_bytes).sum(),
        _ => 0,
    }
}

fn matrix_copies(v: &Value) -> usize {
    match v {
        Value::Matrix(_) => 1,
        Value::Lambda { captured_env, .. } => captured_env.values().map(matrix_copies).sum(),
        Value::Struct(fields) => fields.values().map(matrix_copies).sum(),
        Value::Tuple(items) => items.iter().map(matrix_copies).sum(),
        _ => 0,
    }
}

fn capture_keys(v: &Value) -> BTreeSet<String> {
    match v {
        Value::Lambda { captured_env, .. } => captured_env.keys().cloned().collect(),
        other => panic!("expected lambda, got {}", other.type_name()),
    }
}

/// Names other than the built-in constants that are always snapshotted.
fn user_keys(v: &Value) -> BTreeSet<String> {
    capture_keys(v)
        .into_iter()
        .filter(|k| !CONSTS.contains(&k.as_str()))
        .collect()
}

fn expect_users(v: &Value, keys: &[&str]) {
    let got = user_keys(v);
    let want: BTreeSet<String> = keys.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(got, want, "captured user names");
    for c in CONSTS {
        assert!(
            capture_keys(v).contains(*c),
            "built-in constant {c} missing from capture"
        );
    }
}

fn report(label: &str, v: &Value) {
    println!(
        "BENCH {label}: keys={} user={:?} numeric_bytes={} matrix_copies={}",
        capture_keys(v).len(),
        user_keys(v),
        owned_numeric_bytes(v),
        matrix_copies(v)
    );
}

// ── size: the bug ────────────────────────────────────────────────────

#[test]
fn unreferenced_2000_matrix_is_not_captured() {
    let ev = run("big = zeros(2000, 2000); a = 5; f = @(x) x + a; a = 99; y = f(1);");
    let f = lambda(&ev, "f");
    report("single_2000_ref_a", f);
    expect_users(f, &["a"]);
    assert_eq!(matrix_copies(f), 0);
    // Constants plus the scalar `a`. A 2000×2000 matrix is 64_000_000.
    assert!(
        owned_numeric_bytes(f) < 200,
        "capture still holds numeric bulk"
    );
    assert!((scalar(&ev, "y") - 6.0).abs() < 1e-12);
}

#[test]
fn overwrite_loop_does_not_stack_the_matrix() {
    let src = r#"
        big = zeros(2000, 2000);
        tic;
        for k = 1:20
            f = @(x) x + 1;
        end
        create_s = toc;
        tic;
        s = 0;
        for k = 1:1000
            s = s + f(k);
        end
        call_s = toc;
    "#;
    let ev = run(src);
    let f = lambda(&ev, "f");
    report("overwrite_20_of_2000", f);
    expect_users(f, &[]);
    assert_eq!(matrix_copies(f), 0);
    println!(
        "BENCH overwrite_20_of_2000: create_s={} call_1000_s={}",
        scalar(&ev, "create_s"),
        scalar(&ev, "call_s")
    );
    // sum_{k=1}^{1000} (k+1) = 500500 + 1000
    assert!((scalar(&ev, "s") - 501_500.0).abs() < 1e-6);
}

#[test]
fn nested_rebind_chain_does_not_copy_the_matrix() {
    let src = r#"
        big = zeros(2000, 2000);
        f = @(x) x;
        tic;
        for k = 1:5
            f = @(x) f(x) + 1;
        end
        create_s = toc;
        y = f(10);
    "#;
    let ev = run(src);
    let f = lambda(&ev, "f");
    report("chain_5_of_2000", f);
    // The previous lambda is referenced. The matrix and the loop index are not.
    expect_users(f, &["f"]);
    assert_eq!(matrix_copies(f), 0);
    assert!(owned_numeric_bytes(f) < 2_000);
    println!(
        "BENCH chain_5_of_2000: create_s={}",
        scalar(&ev, "create_s")
    );
    assert!((scalar(&ev, "y") - 15.0).abs() < 1e-9);
}

#[test]
fn alongside_lambdas_do_not_capture_each_other_or_the_matrix() {
    let mut src = String::from("big = zeros(128, 128);\n");
    for i in 0..8 {
        src.push_str(&format!("f{i} = @(x) x + {i};\n"));
    }
    let t0 = Instant::now();
    let ev = run(&src);
    let create = t0.elapsed();
    let mut total = 0u64;
    for i in 0..8 {
        let f = lambda(&ev, &format!("f{i}"));
        expect_users(f, &[]);
        assert_eq!(matrix_copies(f), 0);
        total += owned_numeric_bytes(f);
    }
    println!("BENCH alongside_128: create={create:?} total_numeric_bytes={total}");
    // Eight copies of the constants, not 255 copies of the matrix.
    assert!(total < 2_000, "total capture {total} bytes");
}

#[test]
fn lambda_returning_lambda_drops_the_unreferenced_matrix() {
    let src = r#"
        big = zeros(2000, 2000);
        a = 3;
        tic;
        make = @(g) @(x) x + g + a;
        inner = make(10);
        create_s = toc;
        y = inner(1);
    "#;
    let ev = run(src);
    report("maker_2000", lambda(&ev, "make"));
    report("inner_2000", lambda(&ev, "inner"));
    expect_users(lambda(&ev, "make"), &["a"]);
    expect_users(lambda(&ev, "inner"), &["a", "g"]);
    assert_eq!(matrix_copies(lambda(&ev, "make")), 0);
    assert_eq!(matrix_copies(lambda(&ev, "inner")), 0);
    println!("BENCH returning: create_s={}", scalar(&ev, "create_s"));
    assert!((scalar(&ev, "y") - 14.0).abs() < 1e-9);
}

#[test]
fn four_kept_lambdas_beside_a_2000_matrix_stay_small() {
    let mut ev = Evaluator::new();
    let tokens = rustlab_script::lexer::tokenize("big = zeros(2000, 2000);\n").unwrap();
    let stmts = rustlab_script::parser::parse(tokens).unwrap();
    ev.run(&stmts).unwrap();
    let t0 = Instant::now();
    for i in 0..4 {
        let src = format!("f{i} = @(x) x + 1;\n");
        let tokens = rustlab_script::lexer::tokenize(&src).unwrap();
        let stmts = rustlab_script::parser::parse(tokens).unwrap();
        ev.run(&stmts).unwrap();
    }
    let create = t0.elapsed();
    let mut total = 0u64;
    for i in 0..4 {
        let f = lambda(&ev, &format!("f{i}"));
        expect_users(f, &[]);
        assert_eq!(matrix_copies(f), 0);
        total += owned_numeric_bytes(f);
    }
    println!("BENCH kept_2000: create={create:?} total_numeric_bytes={total}");
    assert!(total < 1_000);
}

// ── semantics ────────────────────────────────────────────────────────

#[test]
fn snapshot_ignores_later_writes_and_unrelated_names() {
    let ev = run("a = 5; big = zeros(4, 4); f = @(x) x + a; a = 99; later = 7; y = f(1);");
    assert!((scalar(&ev, "y") - 6.0).abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &["a"]);
}

#[test]
fn builtin_call_and_shadowed_vector_still_resolve() {
    let ev = run("f = @(x) sin(x); y = f(pi / 2);");
    assert!((scalar(&ev, "y") - 1.0).abs() < 1e-10);
    expect_users(lambda(&ev, "f"), &[]);

    let ev = run("sin = [10, 20, 30]; f = @(i) sin(i); sin = 0; y = f(2);");
    assert!((scalar(&ev, "y") - 20.0).abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &["sin"]);
}

#[test]
fn user_function_called_from_a_lambda_still_sees_pi() {
    let src = r#"
        function y = addpi(x)
            y = x + pi;
        end
        big = zeros(4, 4);
        f = @(x) addpi(x);
        y = f(1);
    "#;
    let ev = run(src);
    assert!((scalar(&ev, "y") - (1.0 + std::f64::consts::PI)).abs() < 1e-12);
    // `addpi` lives in the function table, not the capture.
    expect_users(lambda(&ev, "f"), &[]);
}

#[test]
fn shadowed_pi_at_creation_reaches_a_user_function() {
    let src = r#"
        pi = 3;
        function y = addpi(x)
            y = x + pi;
        end
        f = @(x) addpi(x);
        pi = 100;
        y = f(1);
    "#;
    let ev = run(src);
    assert!((scalar(&ev, "y") - 4.0).abs() < 1e-12);
    match lambda(&ev, "f") {
        Value::Lambda { captured_env, .. } => match captured_env.get("pi") {
            Some(Value::Scalar(n)) => assert!((*n - 3.0).abs() < 1e-12),
            other => panic!("captured pi: {other:?}"),
        },
        _ => unreachable!(),
    }
}

#[test]
fn end_is_bound_at_the_index_not_captured() {
    let ev = run("v = 1:5; f = @(x) v(end); y = f(0);");
    assert!((scalar(&ev, "y") - 5.0).abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &["v"]);

    let ev = run("M = [1, 2; 3, 4]; f = @(i) M(i, end); y = f(1); z = f(2);");
    assert!((scalar(&ev, "y") - 2.0).abs() < 1e-12);
    assert!((scalar(&ev, "z") - 4.0).abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &["M"]);
}

#[test]
fn loop_index_is_snapshotted_per_lambda() {
    let ev = run("k = 1; s.a = @(x) x + k; k = 2; s.b = @(x) x + k; fa = s.a; fb = s.b; ya = fa(0); yb = fb(0);");
    assert!((scalar(&ev, "ya") - 1.0).abs() < 1e-12);
    assert!((scalar(&ev, "yb") - 2.0).abs() < 1e-12);
}

#[test]
fn handle_to_a_lambda_and_to_a_builtin() {
    // `(@sq)(x)` is an index on the handle expression, so the call goes
    // through a name.
    let ev = run("sq = @(x) x^2; f = @(x) sq(x); h = @sq; g = @(x) h(x); y = f(5); z = g(3);");
    assert!((scalar(&ev, "y") - 25.0).abs() < 1e-12);
    assert!((scalar(&ev, "z") - 9.0).abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &["sq"]);
    // `@sq` while `sq` holds a lambda clones that lambda into `h`.
    expect_users(lambda(&ev, "g"), &["h"]);
    assert_eq!(matrix_copies(lambda(&ev, "g")), 0);

    let ev = run("h = @sin; f = @(x) h(x); y = f(pi / 2);");
    assert!((scalar(&ev, "y") - 1.0).abs() < 1e-10);
}

#[test]
fn handle_calls_the_function_not_a_shadowing_variable() {
    // `@sin` is the builtin even when a variable named sin exists.
    // Indexing that vector would reject 0; sin(0) is 0.
    let ev = run("sin = [1, 2, 3]; h = @sin; y = h(0);");
    assert!(matches!(ev.get("h").unwrap(), Value::FuncHandle(_)));
    assert!(scalar(&ev, "y").abs() < 1e-12);

    let ev = run(
        "function y = sq(x)\n  y = x * x;\nend\nsq = [10, 20, 30]; h = @sq; y = h(4); z = sq(1);",
    );
    assert!((scalar(&ev, "y") - 16.0).abs() < 1e-12, "handle must call the user function");
    assert!((scalar(&ev, "z") - 10.0).abs() < 1e-12, "a direct call still indexes the variable");

    // `@f` while f holds a lambda copies that lambda. Later writes to f
    // do not change the handle.
    let ev = run("f = @(x) x + 1; g = @f; f = @(x) x + 100; y = g(3);");
    assert!(matches!(ev.get("g").unwrap(), Value::Lambda { .. }));
    assert!((scalar(&ev, "y") - 4.0).abs() < 1e-12);

    // A lambda that calls the handle does not copy the shadowing value.
    let ev = run("h = @sin; sin = [10, 20, 30]; big = zeros(8, 8); f = @(x) h(x); y = f(0);");
    assert!(scalar(&ev, "y").abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &["h"]);
    assert_eq!(matrix_copies(lambda(&ev, "f")), 0);

    let ev = run("h = @sin; sin = @(x) x + 100; big = zeros(8, 8); f = @(x) h(x); y = f(0);");
    assert!(scalar(&ev, "y").abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &["h"]);
    assert_eq!(matrix_copies(lambda(&ev, "f")), 0);

    let ev = run("sin = [1, 2, 3]; h = @sin; r = arrayfun(h, [0]);");
    match ev.get("r").unwrap() {
        Value::Vector(v) => assert!(v[0].re.abs() < 1e-12),
        other => panic!("arrayfun: {}", other.type_name()),
    }

    // `@feval` is dynamic: the lambda that calls it keeps the workspace.
    let ev = run(
        "h = @feval; big = zeros(4, 4); sq = @(x) x^2; f = @(name, v) h(name, v); y = f(\"sq\", 5);",
    );
    assert!((scalar(&ev, "y") - 25.0).abs() < 1e-12);
    let f = lambda(&ev, "f");
    assert!(user_keys(f).contains("big"));
    assert!(user_keys(f).contains("sq"));
    assert_eq!(matrix_copies(f), 1);
}

#[test]
fn feval_literal_captures_only_that_name() {
    let ev = run("big = zeros(8, 8); sq = @(x) x^2; f = @(n) feval(\"sq\", n); y = f(5);");
    assert!((scalar(&ev, "y") - 25.0).abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &["sq"]);
    assert_eq!(matrix_copies(lambda(&ev, "f")), 0);
}

#[test]
fn feval_of_a_computed_name_still_snapshots_the_workspace() {
    let ev =
        run("big = zeros(8, 8); sq = @(x) x^2; f = @(name, v) feval(name, v); y = f(\"sq\", 5);");
    assert!((scalar(&ev, "y") - 25.0).abs() < 1e-12);
    let f = lambda(&ev, "f");
    assert!(
        user_keys(f).contains("big"),
        "dynamic feval must keep the workspace"
    );
    assert!(user_keys(f).contains("sq"));
    assert_eq!(matrix_copies(f), 1);
}

#[test]
fn lambda_made_inside_a_function_keeps_the_local_not_the_temp() {
    let src = r#"
        function f = make()
            big = zeros(4, 4);
            a = 7;
            f = @(x) x + a;
        end
        f = make();
        y = f(1);
    "#;
    let ev = run(src);
    assert!((scalar(&ev, "y") - 8.0).abs() < 1e-12);
    let f = lambda(&ev, "f");
    assert_eq!(user_keys(f), BTreeSet::from(["a".to_string()]));
    // A user function seeds every built-in constant from its caller (it
    // left out true/false before 0.3.19). The lambda snapshots that
    // environment, so all nine are present — same as a full clone of the
    // function workspace.
    for c in ["i", "j", "pi", "e", "Inf", "NaN", "true", "false", "stdin"] {
        assert!(capture_keys(f).contains(c), "{c} missing");
    }
    assert_eq!(matrix_copies(f), 0);
}

#[test]
fn parameter_shadows_an_outer_of_the_same_name() {
    let ev = run("a = 5; f = @(a) a + 1; y = f(10);");
    assert!((scalar(&ev, "y") - 11.0).abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &[]);
}

#[test]
fn nested_parameter_does_not_capture_the_outer_shadowed_name() {
    let ev = run("a = 5; f = @(x) @(a) a + x; g = f(3); y = g(10);");
    assert!((scalar(&ev, "y") - 13.0).abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &[]);
    expect_users(lambda(&ev, "g"), &["x"]);
}

#[test]
fn self_reference_sees_the_previous_lambda_not_itself() {
    // The snapshot is taken before the assignment completes, so the new
    // `f` closes over the old one. `f(1)` calls that old lambda.
    let ev = run("f = @(n) 0; f = @(n) f(n); y = f(1);");
    assert!((scalar(&ev, "y") - 0.0).abs() < 1e-12);
}

#[test]
fn arrayfun_and_parmap_see_the_capture() {
    let ev = run("a = 3; r = arrayfun(@(x) x + a, [1, 2, 3]);");
    match ev.get("r").unwrap() {
        Value::Vector(v) => {
            assert!((v[0].re - 4.0).abs() < 1e-12);
            assert!((v[2].re - 6.0).abs() < 1e-12);
        }
        other => panic!("arrayfun: {other:?}"),
    }
    let ev = run("a = 3; r = parmap(@(x) x + a, [1, 2, 3]);");
    match ev.get("r").unwrap() {
        Value::Vector(v) => {
            assert!((v[0].re - 4.0).abs() < 1e-12);
            assert!((v[1].re - 5.0).abs() < 1e-12);
            assert!((v[2].re - 6.0).abs() < 1e-12);
        }
        other => panic!("parmap: {other:?}"),
    }
}

#[test]
fn missing_name_is_still_an_error_at_the_call() {
    let err = match eval("f = @(x) x + missing; y = f(1);") {
        Err(e) => e,
        Ok(_) => panic!("expected a missing-name error"),
    };
    let msg = err.to_string();
    assert!(msg.contains("missing"), "{msg}");
}

#[test]
fn display_does_not_dump_the_capture() {
    let ev = run("a = 5; f = @(x, y) x + y + a;");
    assert_eq!(format!("{}", lambda(&ev, "f")), "@(x, y) <expr>");
}

#[test]
fn field_capture_and_short_circuit_names() {
    let ev = run("s.gain = 4; f = @(x) x + s.gain; y = f(1);");
    assert!((scalar(&ev, "y") - 5.0).abs() < 1e-12);
    expect_users(lambda(&ev, "f"), &["s"]);

    let ev = run("a = 1; b = 2; f = @(x) (x > 0) && (a > 0) || (b < 0); y = f(1); z = f(0);");
    assert!(scalar_or_bool(&ev, "y"), "y should be truthy");
    assert!(!scalar_or_bool(&ev, "z"), "z should be false");
    expect_users(lambda(&ev, "f"), &["a", "b"]);
}

fn scalar_or_bool(ev: &Evaluator, name: &str) -> bool {
    match ev.get(name).unwrap() {
        Value::Bool(b) => *b,
        Value::Scalar(n) => *n != 0.0,
        other => panic!("{name}: {other:?}"),
    }
}

//! `rect_mask` / `disk_mask` / `polygon_mask` with `"coverage", K`: the
//! fraction of each meshgrid cell inside the shape, from K×K sub-samples
//! (`rustlab_em` `dev/rustlab/requests/lesson-review-findings-2026-07-09.md`,
//! item #6). Without the option the masks are binary and unchanged.

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

fn matrix(ev: &Evaluator, name: &str) -> Vec<Vec<f64>> {
    match ev.get(name) {
        Some(Value::Matrix(m)) => m
            .rows()
            .into_iter()
            .map(|r| r.iter().map(|c| c.re).collect())
            .collect(),
        other => panic!("{name}: expected a matrix, got {other:?}"),
    }
}

const GRID: &str = "h = 0.05; x = (-30:30) * h; [X, Y] = meshgrid(x, x);";
const TRI: &str = "V = [-0.9, -0.7; 1.1, -0.4; 0.2, 1.05];";

#[test]
fn k1_is_the_binary_mask_bit_for_bit() {
    let ev = eval(&format!(
        "{GRID} {TRI}
         d0 = disk_mask(X, Y, 0.1, -0.05, 0.77);
         d1 = disk_mask(X, Y, 0.1, -0.05, 0.77, \"coverage\", 1);
         r0 = rect_mask(X, Y, -0.5, -0.3, 0.9, 0.7);
         r1 = rect_mask(X, Y, -0.5, -0.3, 0.9, 0.7, \"coverage\", 1);
         p0 = polygon_mask(X, Y, V);
         p1 = polygon_mask(X, Y, V, \"coverage\", 1);"
    ));
    for (a, b) in [("d0", "d1"), ("r0", "r1"), ("p0", "p1")] {
        let (ma, mb) = (matrix(&ev, a), matrix(&ev, b));
        for (ra, rb) in ma.iter().zip(&mb) {
            for (x, y) in ra.iter().zip(rb) {
                assert_eq!(x.to_bits(), y.to_bits(), "{a} vs {b}");
            }
        }
    }
}

#[test]
fn full_inside_is_one_and_outside_is_zero() {
    let ev = eval(&format!(
        "{GRID} {TRI}
         D = disk_mask(X, Y, 0, 0, 1, \"coverage\", 8);
         R = rect_mask(X, Y, -1, -0.5, 1.5, 1, \"coverage\", 8);
         P = polygon_mask(X, Y, V, \"coverage\", 8);
         dc = D(31, 31); dk = D(1, 1);
         rc = R(31, 21); rk = R(61, 61);
         pc = P(31, 31); pk = P(61, 1);
         lo = min([min(min(D)), min(min(R)), min(min(P))]);
         hi = max([max(max(D)), max(max(R)), max(max(P))]);
         frac = sum(sum(and(D > 0, D < 1)));"
    ));
    for (n, want) in [("dc", 1.0), ("rc", 1.0), ("pc", 1.0), ("dk", 0.0), ("rk", 0.0), ("pk", 0.0)] {
        assert_eq!(scalar(&ev, n), want, "{n}");
    }
    assert_eq!(scalar(&ev, "lo"), 0.0);
    assert_eq!(scalar(&ev, "hi"), 1.0);
    assert!(scalar(&ev, "frac") > 0.0, "no fractional boundary cells");
}

#[test]
fn area_is_within_h_over_k() {
    for k in [1, 4, 16] {
        let ev = eval(&format!(
            "{GRID} {TRI}
             K = {k};
             ed = abs(sum(sum(disk_mask(X, Y, 0.03, -0.02, 1, \"coverage\", K))) * h^2 - pi);
             er = abs(sum(sum(rect_mask(X, Y, -0.83, -0.41, 1.37, 0.92, \"coverage\", K))) * h^2 - 1.37 * 0.92);"
        ));
        let h = 0.05;
        let bd = 2.0 * std::f64::consts::PI * h / k as f64;
        let br = 2.0 * (1.37 + 0.92) * h / k as f64;
        assert!(scalar(&ev, "ed") <= bd, "disk K={k}: {} > {bd}", scalar(&ev, "ed"));
        assert!(scalar(&ev, "er") <= br, "rect K={k}: {} > {br}", scalar(&ev, "er"));
    }
}

#[test]
fn map_is_symmetric() {
    let ev = eval(
        "x = ((0:39) - 19.5) * 0.07; [X, Y] = meshgrid(x, x);
         D = disk_mask(X, Y, 0, 0, 0.9, \"coverage\", 6);
         s = max(max(abs(D - D'))) + max(max(abs(D - fliplr(D)))) + max(max(abs(D - flipud(D))));",
    );
    assert_eq!(scalar(&ev, "s"), 0.0);
}

#[test]
fn option_name_is_case_insensitive() {
    let ev = eval(&format!(
        "{GRID}
         a = disk_mask(X, Y, 0, 0, 1, \"coverage\", 4);
         b = disk_mask(X, Y, 0, 0, 1, \"Coverage\", 4);
         d = max(max(abs(a - b)));"
    ));
    assert_eq!(scalar(&ev, "d"), 0.0);
}

#[test]
fn invalid_k_errors() {
    for bad in ["0", "-2", "2.5", "NaN", "Inf", "\"four\"", "[2, 3]", "1025"] {
        let msg = eval_err(&format!("{GRID} D = disk_mask(X, Y, 0, 0, 1, \"coverage\", {bad});"));
        assert!(
            msg.contains("disk_mask") && msg.contains("coverage"),
            "K={bad}: {msg}"
        );
    }
    let msg = eval_err(&format!("{GRID} R = rect_mask(X, Y, 0, 0, 1, 1, \"coverage\", 0);"));
    assert!(msg.contains("rect_mask") && msg.contains("coverage"), "{msg}");
    let msg = eval_err(&format!("{GRID} {TRI} P = polygon_mask(X, Y, V, \"coverage\", 0);"));
    assert!(msg.contains("polygon_mask") && msg.contains("coverage"), "{msg}");
}

#[test]
fn bad_option_forms_error() {
    // Unknown name, missing value, a bare trailing number.
    let msg = eval_err(&format!("{GRID} D = disk_mask(X, Y, 0, 0, 1, \"area\", 4);"));
    assert!(msg.contains("coverage"), "{msg}");
    let msg = eval_err(&format!("{GRID} D = disk_mask(X, Y, 0, 0, 1, \"coverage\");"));
    assert!(msg.contains("disk_mask"), "{msg}");
    let msg = eval_err(&format!("{GRID} D = disk_mask(X, Y, 0, 0, 1, 4);"));
    assert!(msg.contains("disk_mask"), "{msg}");
}

#[test]
fn coverage_needs_meshgrid_coordinates() {
    let msg = eval_err(
        "X = [0, 1; 2, 3]; Y = [0, 0; 1, 1];
         D = disk_mask(X, Y, 0, 0, 1, \"coverage\", 2);",
    );
    assert!(msg.contains("meshgrid"), "{msg}");
    // The binary mask still takes arbitrary point coordinates.
    eval("X = [0, 1; 2, 3]; Y = [0, 0; 1, 1]; D = disk_mask(X, Y, 0, 0, 1);");
}

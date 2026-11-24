//! `pcg` and `ichol` at the script level: MATLAB argument forms, return
//! shapes, flags, messages, and options (dev/plans/sparse_spd_solve.md,
//! Phase 3).

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

fn eval(src: &str) -> Evaluator {
    run(src).unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"))
}

fn eval_err(src: &str) -> String {
    match run(src) {
        Err(e) => e,
        Ok(_) => panic!("expected an error\n{src}"),
    }
}

/// Run `src` and return what it printed to stdout.
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
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

/// SPD 3-D Poisson operator on an `n³` grid and a right-hand side.
fn poisson3(n: usize) -> String {
    format!(
        "n = {n}; N = n ^ 3; A = -1 * laplacian_3d(n, n, n);
         b = ones(N, 1); b(1:2:N) = 2;"
    )
}

#[test]
fn pcg_returns_a_column_and_all_five_outputs() {
    let ev = eval(&format!(
        "{}
         [x, flag, relres, iter, resvec] = pcg(A, b, 1e-10, 500);
         sx = size(x); sr = size(resvec);
         ok_shape = sx(1) == N && sx(2) == 1 && sr(1) == iter + 1 && sr(2) == 1;
         true_rel = norm(b - A * x) / norm(b);
         r1 = resvec(1) - norm(b);
         d = norm(x - A \\ b) / norm(A \\ b);",
        poisson3(8)
    ));
    assert_eq!(scalar(&ev, "ok_shape"), 1.0);
    assert_eq!(scalar(&ev, "flag"), 0.0);
    let relres = scalar(&ev, "relres");
    assert!(relres <= 1e-10);
    assert!((scalar(&ev, "true_rel") - relres).abs() < 1e-14);
    assert_eq!(scalar(&ev, "r1"), 0.0);
    assert!(scalar(&ev, "d") < 1e-8);
    assert!(scalar(&ev, "iter") > 5.0);
}

#[test]
fn pcg_defaults_are_tol_1e_6_and_maxit_min_n_20() {
    let ev = eval(&format!(
        "{}
         [x, flag, relres, iter, resvec] = pcg(A, b);
         nr = numel(resvec);
         [x2, flag2, relres2, iter2] = pcg(A, b, [], 400);",
        poisson3(10)
    ));
    // 1000 unknowns: 20 iterations is not enough for 1e-6.
    assert_eq!(scalar(&ev, "flag"), 1.0);
    assert_eq!(scalar(&ev, "nr"), 21.0);
    assert!(scalar(&ev, "relres") > 1e-6);
    assert_eq!(scalar(&ev, "flag2"), 0.0);
    let r2 = scalar(&ev, "relres2");
    assert!(r2 <= 1e-6 && r2 > 1e-9, "default tol is 1e-6, got {r2}");
    // A 5-unknown system: maxit defaults to n.
    let ev = eval(
        "A = -1 * laplacian_1d(5); b = ones(5, 1);
         [x, flag, relres, iter] = pcg(A, b);",
    );
    assert_eq!(scalar(&ev, "flag"), 0.0);
    assert!(scalar(&ev, "iter") <= 5.0);
}

#[test]
fn pcg_preconditioners_matrix_pair_and_handles() {
    let ev = eval(&format!(
        "{}
         [x0, f0, r0, it0] = pcg(A, b, 1e-10, 500);
         M = spdiags(4 * ones(N, 1), 0, N, N) + speye(N);
         [x1, f1, r1, it1] = pcg(A, b, 1e-10, 500, M);
         L = ichol(A);
         [x2, f2, r2, it2] = pcg(A, b, 1e-10, 500, L, L');
         [x3, f3, r3, it3] = pcg(@(v) A * v, b, 1e-10, 500, L, L');
         [x4, f4, r4, it4] = pcg(A, b, 1e-10, 500, @(r) r / 7);
         [x5, f5, r5, it5] = pcg(A, b, 1e-10, 500, @(r) L \\ r, @(r) L' \\ r);
         [x6, f6, r6, it6] = pcg(A, b, 1e-10, 500, L * L');
         e3 = norm(x3 - x2) / norm(x2);
         e5 = norm(x5 - x2) / norm(x2);
         e6 = norm(x6 - x2) / norm(x2);",
        poisson3(10)
    ));
    for k in 0..7 {
        assert_eq!(scalar(&ev, &format!("f{k}")), 0.0, "f{k}");
        assert!(scalar(&ev, &format!("r{k}")) <= 1e-10, "r{k}");
    }
    let it0 = scalar(&ev, "it0");
    // Constant diagonal: Jacobi is a scaling of no preconditioner.
    assert!((scalar(&ev, "it1") - it0).abs() <= 1.0);
    assert!((scalar(&ev, "it4") - it0).abs() <= 1.0);
    let it2 = scalar(&ev, "it2");
    assert!(it2 < 0.7 * it0, "ichol {it2} vs none {it0}");
    assert_eq!(scalar(&ev, "it3"), it2);
    assert_eq!(scalar(&ev, "it5"), it2);
    assert_eq!(scalar(&ev, "it6"), it2);
    assert!(scalar(&ev, "e3") < 1e-12);
    assert!(scalar(&ev, "e5") < 1e-10);
    assert!(scalar(&ev, "e6") < 1e-10);
}

#[test]
fn pcg_initial_guess_and_empty_placeholders() {
    let ev = eval(&format!(
        "{}
         xs = A \\ b;
         [x, flag, relres, iter, resvec] = pcg(A, b, 1e-8, 200, [], [], xs);
         nr = numel(resvec);
         [y, fy, ry, iy] = pcg(A, b, 1e-8, 200, [], [], 0.5 * xs);
         [z, fz, rz, iz] = pcg(A, b, 1e-8, 200);",
        poisson3(6)
    ));
    assert_eq!(scalar(&ev, "flag"), 0.0);
    assert_eq!(scalar(&ev, "iter"), 0.0);
    assert_eq!(scalar(&ev, "nr"), 1.0);
    assert_eq!(scalar(&ev, "fy"), 0.0);
    assert!(scalar(&ev, "iy") <= scalar(&ev, "iz"));
}

#[test]
fn pcg_zero_rhs() {
    let ev = eval(
        "A = -1 * laplacian_2d(5, 5);
         [x, flag, relres, iter, resvec] = pcg(A, zeros(25, 1), 1e-6, 10, [], [], ones(25, 1));
         s = size(x); nx = norm(x);",
    );
    assert_eq!(scalar(&ev, "flag"), 0.0);
    assert_eq!(scalar(&ev, "relres"), 0.0);
    assert_eq!(scalar(&ev, "iter"), 0.0);
    assert_eq!(scalar(&ev, "resvec"), 0.0);
    assert_eq!(scalar(&ev, "nx"), 0.0);
}

#[test]
fn pcg_flags_2_and_4() {
    let ev = eval(
        "A = -1 * laplacian_2d(5, 5); b = ones(25, 1);
         M = speye(25); M(3, 3) = 0;
         [x, f2, r2, i2, rv2] = pcg(A, b, 1e-6, 10, M);
         n2 = numel(rv2);
         [y, f4, r4, i4, rv4] = pcg(sparse([1, 0; 0, -1]), [1; 1]);
         n4 = numel(rv4);",
    );
    assert_eq!(scalar(&ev, "f2"), 2.0);
    assert_eq!(scalar(&ev, "i2"), 0.0);
    assert_eq!(scalar(&ev, "n2"), 1.0);
    assert_eq!(scalar(&ev, "f4"), 4.0);
    assert_eq!(scalar(&ev, "n4"), 1.0);
}

#[test]
fn pcg_dense_matrix_and_complex_hermitian() {
    let ev = eval(
        "A = [4, 1, 0; 1, 3, 1; 0, 1, 2]; b = [1; 2; 3];
         [x, flag] = pcg(A, b, 1e-12, 10);
         e = norm(x - A \\ b);
         H = sparse([3, -1+0.5j, 0; -1-0.5j, 3, -1+0.5j; 0, -1-0.5j, 3]);
         c = [1; 1j; 2];
         [z, fz, rz] = pcg(H, c, 1e-12, 10);
         ez = norm(z - full(H) \\ c);",
    );
    assert_eq!(scalar(&ev, "flag"), 0.0);
    assert!(scalar(&ev, "e") < 1e-12);
    assert_eq!(scalar(&ev, "fz"), 0.0);
    assert!(scalar(&ev, "ez") < 1e-12);
    assert!(matches!(ev.get("z"), Some(Value::Matrix(_))));
}

#[test]
fn pcg_prints_a_message_only_without_flag() {
    let setup = "A = -1 * laplacian_2d(10, 10); b = ones(100, 1);";
    let one = printed(&format!("{setup} x = pcg(A, b, 1e-8, 200);"));
    assert!(
        one.contains("pcg converged at iteration") && one.contains("relative residual"),
        "{one:?}"
    );
    let bare = printed(&format!("{setup} pcg(A, b, 1e-8, 200);"));
    assert!(bare.contains("pcg converged at iteration"), "{bare:?}");
    let two = printed(&format!("{setup} [x, flag] = pcg(A, b, 1e-8, 200);"));
    assert!(!two.contains("pcg"), "{two:?}");
    let zero = printed(&format!("{setup} x = pcg(A, zeros(100, 1));"));
    assert!(
        zero.contains("right hand side vector is all zero"),
        "{zero:?}"
    );
    // Non-convergence is a warning on stderr, not stdout.
    let fail = printed(&format!("{setup} x = pcg(A, b, 1e-8, 2);"));
    assert!(!fail.contains("pcg"), "{fail:?}");
}

#[test]
fn pcg_argument_errors() {
    let setup = "A = -1 * laplacian_2d(4, 4); b = ones(16, 1);";
    assert!(eval_err(&format!("{setup} pcg(A, ones(15, 1));")).contains("pcg"));
    assert!(eval_err(&format!("{setup} pcg(A(1:15, :), b);")).contains("square"));
    assert!(eval_err(&format!("{setup} pcg(A, b, 1e-6, 10, speye(3));")).contains("M1"));
    assert!(eval_err(&format!("{setup} pcg(A, b, 1e-6, 10, [], [], ones(3, 1));")).contains("x0"));
    assert!(eval_err(&format!("{setup} pcg(A, b, 1e-6, 10, [], [], [], 4);")).contains("pcg"));
    assert!(eval_err(&format!("{setup} pcg(A, b, 1e-6, -1.5j);")).contains("maxit"));
    assert!(eval_err(&format!("{setup} pcg(@(v) v(1:3), b);")).contains("length"));
    assert!(eval_err(&format!("{setup} pcg(A, ones(1, 16));")).contains("column"));
}

#[test]
fn pcg_through_feval_and_a_handle() {
    let ev = eval(
        "A = -1 * laplacian_2d(6, 6); b = ones(36, 1);
         h = @pcg;
         x = h(A, b, 1e-10, 100);
         y = feval(\"pcg\", A, b, 1e-10, 100);
         [z, flag] = pcg(A, b, 1e-10, 100);
         d = norm(x - y) + norm(x - z);",
    );
    assert_eq!(scalar(&ev, "flag"), 0.0);
    assert_eq!(scalar(&ev, "d"), 0.0);
}

// ─── ichol ────────────────────────────────────────────────────────────────

#[test]
fn ichol_nofill_is_lower_with_the_pattern_of_a() {
    let ev = eval(
        "n = 6; N = n * n; A = -1 * laplacian_2d(n, n);
         L = ichol(A);
         sp = issparse(L);
         [I, J] = find(L);
         lower = all(I >= J);
         [IA, JA] = find(A);
         k = IA >= JA;
         same_count = numel(I) == sum(k);
         D = full(L * L') - full(A);
         idx = (JA(k) - 1) * N + IA(k);
         err = max(abs(D(idx)));",
    );
    assert_eq!(scalar(&ev, "sp"), 1.0);
    assert_eq!(scalar(&ev, "lower"), 1.0);
    assert_eq!(scalar(&ev, "same_count"), 1.0);
    assert!(scalar(&ev, "err") < 1e-12);
}

#[test]
fn ichol_options() {
    let ev = eval(
        "A = -1 * laplacian_2d(6, 6); N = 36;
         L = ichol(A, struct(\"type\", \"ict\", \"droptol\", 0));
         R = chol(full(A));
         e_full = max(max(abs(full(L) - R')));
         L3 = ichol(A, struct(\"type\", \"ict\", \"droptol\", 0.1));
         fewer = nnz(L3) < nnz(L);
         Lm = ichol(A, struct(\"michol\", \"on\"));
         e_rows = max(abs(full(Lm * Lm') * ones(N, 1) - full(A) * ones(N, 1)));
         Ld = ichol(A, struct(\"diagcomp\", 0.5));
         Ls = ichol(A + 0.5 * spdiags(4 * ones(N, 1), 0, N, N));
         e_shift = max(max(abs(full(Ld - Ls))));
         U = ichol(A, struct(\"shape\", \"upper\"));
         L0 = ichol(A, struct(\"shape\", \"lower\", \"type\", \"nofill\", \"michol\", \"off\"));
         e_upper = max(max(abs(full(U - L0'))));",
    );
    assert!(scalar(&ev, "e_full") < 1e-12);
    assert_eq!(scalar(&ev, "fewer"), 1.0);
    assert!(scalar(&ev, "e_rows") < 1e-12);
    assert!(scalar(&ev, "e_shift") < 1e-14);
    assert_eq!(scalar(&ev, "e_upper"), 0.0);
}

#[test]
fn ichol_errors() {
    let setup = "A = -1 * laplacian_2d(4, 4);";
    assert!(eval_err(&format!("{setup} ichol(full(A));")).contains("sparse"));
    assert!(eval_err(&format!("{setup} ichol(A(1:3, :));")).contains("square"));
    assert!(eval_err("ichol(sparse([1, 2; 2, 1]));").contains("nonpositive pivot"));
    assert!(eval_err(&format!("{setup} ichol(A, struct(\"typo\", 1));")).contains("typo"));
    assert!(eval_err(&format!("{setup} ichol(A, struct(\"type\", \"ilu\"));")).contains("type"));
    assert!(eval_err(&format!("{setup} ichol(A, struct(\"droptol\", -1));")).contains("droptol"));
    assert!(eval_err(&format!("{setup} ichol(A, struct(\"diagcomp\", -1));")).contains("diagcomp"));
    assert!(eval_err(&format!("{setup} ichol(A, 3);")).contains("struct"));
    assert!(
        eval_err("H = sparse([3, 1j; -1j, 3]); ichol(H, struct(\"michol\", \"on\"));")
            .contains("michol")
    );
    // A diagonal shift rescues the indefinite case.
    let ev = eval("L = ichol(sparse([1, 2; 2, 1]), struct(\"diagcomp\", 4)); ok = issparse(L);");
    assert_eq!(scalar(&ev, "ok"), 1.0);
}

#[test]
fn ichol_complex_hermitian_preconditions_pcg() {
    let ev = eval(
        "n = 60; e = ones(n, 1);
         H = spdiags(3 * e, 0, n, n) + spdiags((-1 + 0.5j) * e, -1, n, n) + spdiags((-1 - 0.5j) * e, 1, n, n);
         L = ichol(H);
         c = (1:n)' + 1j;
         [z, flag, relres, iter] = pcg(H, c, 1e-12, 100, L, L');
         ez = norm(z - H \\ c) / norm(z);",
    );
    assert_eq!(scalar(&ev, "flag"), 0.0);
    // A tridiagonal has no fill: IC(0) is exact, one iteration.
    assert!(scalar(&ev, "iter") <= 2.0);
    assert!(scalar(&ev, "ez") < 1e-12);
}

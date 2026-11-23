//! Sparse `\` and `spsolve` on `pin_dirichlet` operators: the row-singleton
//! Cholesky fast path (dev/plans/sparse_spd_solve.md, Phase 1).
//!
//! Every answer is checked against the residual and against the natural-
//! order sparse LU, which is what `\` ran on these matrices in 0.3.84.

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
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

/// Lesson 18's concentric-sphere operator on an `n³` grid.
fn sphere(n: usize) -> String {
    format!(
        "n = {n}; c = (n + 1) / 2; bc = (n - 1) / 2 - 0.5; ac = bc / 2;
         R2 = zeros3(n, n, n);
         [Xg, Yg] = meshgrid((1:n) - c, (1:n) - c);
         for kk = 1:n
           R2(:, :, kk) = Xg .^ 2 + Yg .^ 2 + (kk - c) ^ 2;
         end
         inner = 1.0 * (R2 <= ac ^ 2); outer = 1.0 * (R2 >= bc ^ 2);
         er = ones3(n, n, n); er(:, :, 1:3) = 4.4;
         A = -1 * laplacian_eps_3d(er, 1, 1, 1);
         b = zeros(n ^ 3, 1);
         [A, b] = pin_dirichlet(A, b, inner, 1.0);
         [A, b] = pin_dirichlet(A, b, outer, 0.0);
         x_ref = spsolve(A, b, \"lu\", \"identity\"); x_ref = x_ref(:);"
    )
}

#[test]
fn backslash_on_a_pinned_operator_matches_lu() {
    let ev = eval(&format!(
        "{}
         x = A \\ b; x = x(:);
         res = norm(A * x - b) / norm(b);
         dif = norm(x - x_ref) / norm(x_ref);
         pin = max(abs(x .* inner(:) - inner(:)));",
        sphere(13)
    ));
    assert!(
        scalar(&ev, "res") < 1e-12,
        "residual {}",
        scalar(&ev, "res")
    );
    assert!(scalar(&ev, "dif") < 1e-12, "vs LU {}", scalar(&ev, "dif"));
    assert_eq!(
        scalar(&ev, "pin"),
        0.0,
        "pinned nodes must come back exactly"
    );
}

#[test]
fn spsolve_auto_and_cholesky_modes_match_lu() {
    let ev = eval(&format!(
        "{}
         xa = spsolve(A, b); xa = xa(:);
         xc = spsolve(A, b, \"cholesky\"); xc = xc(:);
         xn = spsolve(A, b, \"cholesky\", \"nd\"); xn = xn(:);
         xi = spsolve(A, b, \"auto\", \"identity\"); xi = xi(:);
         da = norm(xa - x_ref) / norm(x_ref);
         dc = norm(xc - x_ref) / norm(x_ref);
         dn = norm(xn - x_ref) / norm(x_ref);
         di = norm(xi - x_ref) / norm(x_ref);",
        sphere(11)
    ));
    for v in ["da", "dc", "dn", "di"] {
        assert!(scalar(&ev, v) < 1e-12, "{v} = {}", scalar(&ev, v));
    }
}

#[test]
fn backslash_with_several_right_hand_sides() {
    let ev = eval(&format!(
        "{}
         B = [b, 2 * b, b + 0.5 * inner(:)];
         X = A \\ B;
         r = norm(full(A) * X - B) / norm(B);
         d1 = norm(X(:, 1) - x_ref) / norm(x_ref);
         d2 = norm(X(:, 2) - 2 * x_ref) / norm(x_ref);",
        sphere(9)
    ));
    assert!(scalar(&ev, "r") < 1e-12, "residual {}", scalar(&ev, "r"));
    assert!(scalar(&ev, "d1") < 1e-12);
    assert!(scalar(&ev, "d2") < 1e-12);
}

#[test]
fn backslash_with_a_complex_right_hand_side() {
    let ev = eval(&format!(
        "{}
         bz = b + 1j * (0.25 * inner(:) + 0.01);
         xr = A \\ real(bz); xr = xr(:); xi = A \\ imag(bz); xi = xi(:);
         z = A \\ bz; z = z(:);
         d = norm(z - (xr + 1j * xi)) / norm(z);
         r = norm(A * z - bz) / norm(bz);",
        sphere(9)
    ));
    assert!(scalar(&ev, "d") < 1e-12, "d = {}", scalar(&ev, "d"));
    assert!(scalar(&ev, "r") < 1e-12, "r = {}", scalar(&ev, "r"));
}

#[test]
fn indefinite_free_block_falls_back_to_lu() {
    // Free block [1 2; 2 1] is symmetric but indefinite; row 3 is pinned.
    let ev = eval(
        "A = sparse([1, 2, 1; 2, 1, 0; 0, 0, 3]); b = [1; 2; 3];
         x = A \\ b; r = norm(full(A) * x(:) - b);
         y = spsolve(A, b); y = y(:); s = norm(full(A) * y - b);",
    );
    assert!(scalar(&ev, "r") < 1e-12);
    assert!(scalar(&ev, "s") < 1e-12);
    let e = eval_err(
        "A = sparse([1, 2, 1; 2, 1, 0; 0, 0, 3]); x = spsolve(A, [1; 2; 3], \"cholesky\");",
    );
    assert!(e.contains("not Hermitian positive definite"), "{e}");
}

#[test]
fn cholesky_mode_rejects_a_non_hermitian_matrix() {
    // 0.3.84 read only the upper triangle and returned a wrong answer here.
    let e = eval_err(
        "A = sparse([4, 1, 0; 0, 3, 1; 0, 2, 5]); x = spsolve(A, [1; 2; 3], \"cholesky\");",
    );
    assert!(e.contains("not Hermitian"), "{e}");
    let ev = eval(
        "A = sparse([4, 1, 0; 0, 3, 1; 0, 2, 5]); b = [1; 2; 3];
         x = spsolve(A, b); r = norm(full(A) * x(:) - b);",
    );
    assert!(scalar(&ev, "r") < 1e-12);
}

#[test]
fn nd_ordering_is_accepted_everywhere() {
    let ev = eval(
        "L = -1 * laplacian_2d(12, 12); b = ones(144, 1);
         x1 = spsolve(L, b, \"lu\", \"nd\"); x1 = x1(:);
         F = chol(L, \"nd\"); x2 = solve(F, b); x2 = x2(:);
         G = lu(L, \"nd\"); x3 = solve(G, b); x3 = x3(:);
         x0 = L \\ b; x0 = x0(:);
         d = max([norm(x1 - x0), norm(x2 - x0), norm(x3 - x0)]) / norm(x0);",
    );
    assert!(scalar(&ev, "d") < 1e-12);
    let e = eval_err("x = spsolve(speye(3), [1; 2; 3], \"auto\", \"metis\");");
    assert!(e.contains("\"nd\""), "{e}");
}

#[test]
fn symmetric_pinning_still_takes_cholesky() {
    // Pinned rows AND columns: SPD from the start, as before.
    let ev = eval(
        "n = 10; A = -1 * laplacian_2d(n, n); b = ones(n * n, 1);
         x = A \\ b; x = x(:); y = spsolve(A, b, \"lu\", \"identity\"); y = y(:);
         d = norm(x - y) / norm(y);",
    );
    assert!(scalar(&ev, "d") < 1e-12);
}

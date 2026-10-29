//! Regression tests for core math builtins.
//!
//! Each case is a closed form, a small integer identity, or a MATLAB/Octave
//! convention called out in the assertion. Reference vectors are literals.

use crate::eval::value::Value;
use crate::{lexer, parser, Evaluator};
use num_complex::Complex64;

fn eval(src: &str) -> Evaluator {
    let src = if src.ends_with('\n') {
        src.to_string()
    } else {
        format!("{src}\n")
    };
    let tokens = lexer::tokenize(&src).unwrap();
    let stmts = parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    for stmt in &stmts {
        ev.exec_stmt(stmt).unwrap();
    }
    ev
}

fn eval_err(src: &str) -> String {
    let src = if src.ends_with('\n') {
        src.to_string()
    } else {
        format!("{src}\n")
    };
    let tokens = lexer::tokenize(&src).unwrap();
    let stmts = parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    for stmt in &stmts {
        if let Err(e) = ev.exec_stmt(stmt) {
            return e.to_string();
        }
    }
    panic!("expected an error from: {src}");
}

fn num(ev: &Evaluator, name: &str) -> Complex64 {
    match ev.get(name).unwrap() {
        Value::Scalar(x) => Complex64::new(*x, 0.0),
        Value::Complex(c) => *c,
        Value::Vector(v) if v.len() == 1 => v[0],
        Value::Matrix(m) if m.nrows() == 1 && m.ncols() == 1 => m[[0, 0]],
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

fn re(ev: &Evaluator, name: &str) -> f64 {
    let z = num(ev, name);
    assert!(z.im.abs() < 1e-9, "{name} imag {}", z.im);
    z.re
}

fn vec_c(ev: &Evaluator, name: &str) -> Vec<Complex64> {
    match ev.get(name).unwrap() {
        Value::Vector(v) => v.iter().copied().collect(),
        Value::Matrix(m) if m.nrows() == 1 || m.ncols() == 1 => m.iter().copied().collect(),
        other => panic!("{name}: expected a vector, got {other:?}"),
    }
}

fn mat(ev: &Evaluator, name: &str) -> Vec<Vec<Complex64>> {
    match ev.get(name).unwrap() {
        Value::Matrix(m) => (0..m.nrows())
            .map(|r| (0..m.ncols()).map(|c| m[[r, c]]).collect())
            .collect(),
        other => panic!("{name}: expected a matrix, got {other:?}"),
    }
}

fn assert_c(got: Complex64, want: Complex64, tol: f64) {
    let d = (got - want).norm();
    assert!(d <= tol, "got {got}, want {want}, |diff| {d:e}");
}

fn assert_re(ev: &Evaluator, name: &str, want: f64, tol: f64) {
    let g = re(ev, name);
    assert!((g - want).abs() <= tol, "{name} = {g}, want {want}");
}

// ── inv, det, linsolve ──────────────────────────────────────────────────────

#[test]
fn det_of_integer_matrices() {
    assert_re(&eval("d = det([1, 2; 3, 4])"), "d", -2.0, 1e-12);
    assert_re(
        &eval("d = det([1, 2, 3; 0, 4, 5; 0, 0, 6])"),
        "d",
        24.0,
        1e-12,
    );
    assert_re(&eval("d = det([0, 1; 1, 0])"), "d", -1.0, 1e-12);
    // 3-cycle (123): one even? The permutation matrix of a transposition of
    // rows 1 and 3 of I is odd. [[0,0,1],[1,0,0],[0,1,0]] is the cycle (1 3 2),
    // a transposition of the identity's columns... det = sign of the perm
    // sending e1→e2, e2→e3, e3→e1, which is a 3-cycle (even). det = +1.
    assert_re(
        &eval("d = det([0, 0, 1; 1, 0, 0; 0, 1, 0])"),
        "d",
        1.0,
        1e-12,
    );
    assert_re(&eval("d = det(eye(4))"), "d", 1.0, 1e-12);
}

#[test]
fn det_2x2_hilbert_and_complex() {
    // det [[1, 1/2],[1/2, 1/3]] = 1/12.
    assert_re(&eval("d = det([1, 0.5; 0.5, 1/3])"), "d", 1.0 / 12.0, 1e-12);
    // det [[1, i],[i, 1]] = 1 - i² = 2.
    assert_re(&eval("d = det([1, j; j, 1])"), "d", 2.0, 1e-12);
}

#[test]
fn det_does_not_skip_a_tiny_pivot() {
    // Column 1 is 1e-20. Skipping elimination (the old 1e-14 cutoff) left
    // the product of the diagonals at 1e-40. The determinant is -1e-20.
    let d = num(&eval("d = det([1e-20, 1; 1e-20, 1e-20])"), "d");
    assert_c(d, Complex64::new(-1e-20, 0.0), 1e-28);
}

#[test]
fn det_edges() {
    assert_re(&eval("d = det(7)"), "d", 7.0, 0.0);
    assert!(eval_err("d = det([1, 2, 3; 4, 5, 6])").contains("square"));
    assert!(eval_err("d = det([1, NaN; 0, 1])").contains("finite"));
    assert!(eval_err("d = det(Inf)").contains("finite"));
}

#[test]
fn inv_times_a_is_identity() {
    let ev = eval(
        "A = [1, 2; 3, 4]\nB = inv(A)\nR = A * B\n\
         s = inv(1e-16 * eye(3)) * (1e-16 * eye(3))",
    );
    let r = mat(&ev, "R");
    assert_c(r[0][0], Complex64::new(1.0, 0.0), 1e-10);
    assert_c(r[0][1], Complex64::new(0.0, 0.0), 1e-10);
    assert_c(r[1][0], Complex64::new(0.0, 0.0), 1e-10);
    assert_c(r[1][1], Complex64::new(1.0, 0.0), 1e-10);
    // A well-conditioned matrix scaled below the old 1e-14 cutoff.
    let s = mat(&ev, "s");
    for i in 0..3 {
        for j in 0..3 {
            let want = if i == j { 1.0 } else { 0.0 };
            assert_c(s[i][j], Complex64::new(want, 0.0), 1e-8);
        }
    }
}

#[test]
fn inv_known_2x2_and_complex() {
    // inv([1,2;3,4]) = [-2, 1; 1.5, -0.5]
    let b = mat(&eval("B = inv([1, 2; 3, 4])"), "B");
    assert_c(b[0][0], Complex64::new(-2.0, 0.0), 1e-12);
    assert_c(b[0][1], Complex64::new(1.0, 0.0), 1e-12);
    assert_c(b[1][0], Complex64::new(1.5, 0.0), 1e-12);
    assert_c(b[1][1], Complex64::new(-0.5, 0.0), 1e-12);
    // 1/(1+i) = (1-i)/2
    assert_c(
        num(&eval("z = inv(1+j)"), "z"),
        Complex64::new(0.5, -0.5),
        1e-12,
    );
    assert_re(&eval("z = inv(4)"), "z", 0.25, 1e-15);
}

#[test]
fn inv_rejects_singular_and_nonfinite() {
    assert!(eval_err("x = inv([1, 2; 2, 4])").contains("singular"));
    assert!(eval_err("x = inv(0)").contains("singular"));
    assert!(eval_err("x = inv([1, 2, 3; 4, 5, 6])").contains("square"));
    assert!(eval_err("x = inv([NaN, 0; 0, 1])").contains("finite"));
    assert!(eval_err("x = inv(Inf)").contains("finite"));
}

#[test]
fn hilbert5_inverse_residual_is_small() {
    // H_ij = 1/(i+j-1), 1-based. cond(H_5) is about 4.8e5.
    let ev = eval(
        "n = 5\nH = zeros(n, n)\n\
         for i = 1:n\n  for j = 1:n\n    H(i, j) = 1 / (i + j - 1)\n  end\nend\n\
         R = H * inv(H)\nerr = norm(R - eye(n))",
    );
    let err = re(&ev, "err");
    assert!(err < 1e-8, "Hilbert-5 residual {err:e}");
}

#[test]
fn linsolve_known_system_and_complex() {
    // [2, 1; 1, 3] [1; 3] = [5; 10]
    let x = vec_c(&eval("x = linsolve([2, 1; 1, 3], [5; 10])"), "x");
    assert_c(x[0], Complex64::new(1.0, 0.0), 1e-12);
    assert_c(x[1], Complex64::new(3.0, 0.0), 1e-12);
    // (1+i) x = 2i → x = 1+i. A 1×1 matrix, not a length-1 vector.
    assert_c(
        num(&eval("x = linsolve(eye(1) * (1+j), [2j])"), "x"),
        Complex64::new(1.0, 1.0),
        1e-12,
    );
    assert!(eval_err("x = linsolve([1, 2; 2, 4], [1; 2])").contains("singular"));
    assert!(eval_err("x = linsolve([1, 2, 3; 4, 5, 6], [1; 2])").contains("square"));
}

#[test]
fn sparse_chol_and_lu_solve_a_scaled_system() {
    // [[2, -1], [-1, 2]] [1; 0] = [2; -1], so solving for [1; 0] gives [2/3; 1/3].
    let ev = eval(
        "A = sparse([1, 2, 1, 2], [1, 2, 2, 1], [2, 2, -1, -1], 2, 2)\n\
         x = solve(chol(A), [1; 0])\n\
         y = solve(lu(A), [1; 0])\n\
         S = sparse([1, 2], [1, 2], [1e-14, 1e-14], 2, 2)\n\
         z = solve(chol(S), [1e-14; 0])",
    );
    for name in ["x", "y"] {
        let v = vec_c(&ev, name);
        assert_c(v[0], Complex64::new(2.0 / 3.0, 0.0), 1e-10);
        assert_c(v[1], Complex64::new(1.0 / 3.0, 0.0), 1e-10);
    }
    // 1e-14 * I is above the sparse drop tolerance (1e-15) and used to be
    // rejected by Cholesky's absolute pivot floor of 1e-12.
    let z = vec_c(&ev, "z");
    assert_c(z[0], Complex64::new(1.0, 0.0), 1e-8);
    assert_c(z[1], Complex64::new(0.0, 0.0), 1e-8);
    assert!(eval_err("A = sparse([1, 2], [1, 2], [1, -1], 2, 2)\nF = chol(A)").contains("chol"));
}

// ── svd, rank, pinv, norm, cond ─────────────────────────────────────────────

#[test]
fn svd_of_a_diagonal_and_a_rank_one_matrix() {
    let s = vec_c(&eval("s = svd([3, 0, 0; 0, 1, 0; 0, 0, 2])"), "s");
    assert_c(s[0], Complex64::new(3.0, 0.0), 1e-10);
    assert_c(s[1], Complex64::new(2.0, 0.0), 1e-10);
    assert_c(s[2], Complex64::new(1.0, 0.0), 1e-10);

    let ev = eval("A = [0, 2; 0, 0]\n[U, s, V] = svd(A)\nR = U * diag(s) * V'\nerr = norm(R - A)");
    let s = vec_c(&ev, "s");
    assert_c(s[0], Complex64::new(2.0, 0.0), 1e-10);
    assert!(s[1].re.abs() < 1e-10 && s[1].im.abs() < 1e-10);
    assert!(re(&ev, "err") < 1e-10);
}

#[test]
fn svd_reconstructs_wide_tall_and_complex() {
    let ev = eval(
        "W = [1, 2, 3, 4; 0, 1, 0, 1]\n\
         [Uw, sw, Vw] = svd(W, \"econ\")\n\
         ew = norm(Uw * diag(sw) * Vw' - W)\n\
         T = [1, 0; 2, 1; 3, 0; 4, 1]\n\
         [Ut, st, Vt] = svd(T)\n\
         et = norm(Ut(:, 1:2) * diag(st) * Vt(:, 1:2)' - T)\n\
         C = [1, j; -j, 2]\n\
         [Uc, sc, Vc] = svd(C)\n\
         ec = norm(Uc * diag(sc) * Vc' - C)",
    );
    assert!(re(&ev, "ew") < 1e-9, "wide {}", re(&ev, "ew"));
    assert!(re(&ev, "et") < 1e-9, "tall {}", re(&ev, "et"));
    assert!(re(&ev, "ec") < 1e-9, "complex {}", re(&ev, "ec"));
}

#[test]
fn rank_uses_the_matlab_tolerance() {
    assert_re(&eval("r = rank(eye(4))"), "r", 4.0, 0.0);
    assert_re(&eval("r = rank([1, 2; 2, 4])"), "r", 1.0, 0.0);
    assert_re(&eval("r = rank([0, 0; 0, 0])"), "r", 0.0, 0.0);
    // σ = 1e-10 is above max(size)*eps*σmax ≈ 4e-16 and below the old
    // sqrt(eps) cutoff ≈ 3e-8, which reported rank 1.
    assert_re(&eval("r = rank([1, 0; 0, 1e-10])"), "r", 2.0, 0.0);
    assert_re(&eval("r = rank([1e-20, 0, 0])"), "r", 1.0, 0.0);
    assert_re(&eval("r = rank(0)"), "r", 0.0, 0.0);
    assert!(eval_err("r = rank([NaN, 0; 0, 1])").contains("finite"));
}

#[test]
fn pinv_moore_penrose() {
    // pinv of a column [1;2;3] is the row [1, 2, 3] / 14.
    let ev = eval(
        "A = [1; 2; 3]\nB = pinv(A)\n\
         Z = pinv([1, 0; 0, 0])\n\
         P = pinv([1, 2; 3, 4])\nR = [1, 2; 3, 4] * P * [1, 2; 3, 4]",
    );
    let b = mat(&ev, "B");
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].len(), 3);
    assert_c(b[0][0], Complex64::new(1.0 / 14.0, 0.0), 1e-12);
    assert_c(b[0][1], Complex64::new(2.0 / 14.0, 0.0), 1e-12);
    assert_c(b[0][2], Complex64::new(3.0 / 14.0, 0.0), 1e-12);
    let z = mat(&ev, "Z");
    assert_c(z[0][0], Complex64::new(1.0, 0.0), 1e-10);
    assert!(z[0][1].norm() < 1e-10 && z[1][0].norm() < 1e-10 && z[1][1].norm() < 1e-10);
    let a = mat(&eval("A = [1, 2; 3, 4]"), "A");
    let r = mat(&ev, "R");
    for i in 0..2 {
        for j in 0..2 {
            assert_c(r[i][j], a[i][j], 1e-8);
        }
    }
    assert_re(&eval("p = pinv(0)"), "p", 0.0, 0.0);
}

#[test]
fn norm_known_values() {
    assert_re(&eval("n = norm([3, 4])"), "n", 5.0, 1e-12);
    assert_re(&eval("n = norm([3, -4], 1)"), "n", 7.0, 1e-12);
    assert_re(&eval("n = norm([3, -4], Inf)"), "n", 4.0, 1e-12);
    // 1-norm is the max column sum: |1|+|-3| = 4, |-2|+|4| = 6.
    assert_re(&eval("n = norm([1, -2; -3, 4], 1)"), "n", 6.0, 1e-12);
    // Inf-norm is the max row sum: 3 and 7.
    assert_re(&eval("n = norm([1, -2; -3, 4], Inf)"), "n", 7.0, 1e-12);
    // Default matrix norm is the 2-norm, the same value as norm(A, 2).
    assert_re(
        &eval("n = norm([1, -2; -3, 4])"),
        "n",
        5.464985704219043,
        1e-9,
    );
    assert_re(
        &eval("n = norm([1, -2; -3, 4], \"fro\")"),
        "n",
        30.0_f64.sqrt(),
        1e-12,
    );
    // Spectral norm of a diagonal is the largest |entry|.
    assert_re(&eval("n = norm([3, 0; 0, -4], 2)"), "n", 4.0, 1e-9);
    assert_re(&eval("n = norm(0)"), "n", 0.0, 0.0);
    assert_re(&eval("n = norm(3+4j)"), "n", 5.0, 1e-12);
}

#[test]
fn sparse_spectral_norm_matches_dense() {
    let ev = eval(
        "S = sparse([1, 2], [1, 2], [3, -4], 2, 2)\n\
         a = norm(S)\nb = norm(S, 2)\nc = norm(full(S), 2)\nd = norm(S, \"fro\")",
    );
    assert_re(&ev, "a", (9.0_f64 + 16.0).sqrt(), 1e-12);
    assert!((re(&ev, "b") - re(&ev, "c")).abs() < 1e-9);
    assert!((re(&ev, "b") - 4.0).abs() < 1e-8);
    assert_re(&ev, "d", 5.0, 1e-12);
    assert!(eval_err("S = speye(2)\nn = norm(S, 3)").contains("p-norm"));
}

#[test]
fn cond_of_a_diagonal_and_a_singular_matrix() {
    // diag(1, 100) has 2-norm condition number 100.
    assert_re(&eval("c = cond([1, 0; 0, 100])"), "c", 100.0, 1e-6);
    assert_re(&eval("c = cond([1, 0; 0, 100], 1)"), "c", 100.0, 1e-8);
    assert_re(&eval("c = cond([1, 0; 0, 100], Inf)"), "c", 100.0, 1e-8);
    assert!(re(&eval("c = cond([1, 2; 2, 4])"), "c").is_infinite());
    assert_re(&eval("c = cond(5)"), "c", 1.0, 1e-12);
    assert!(re(&eval("c = cond(0)"), "c").is_infinite());
    assert!(eval_err("c = cond([1, 2, 3], 1)").contains("square"));
}

// ── eig, eigs, expm, kron, diag, trace ──────────────────────────────────────

#[test]
fn eig_closed_forms() {
    // Rotation by a right angle: eigenvalues ±i, residual small.
    let ev = eval(
        "A = [0, -1; 1, 0]\n[V, D] = eig(A)\n\
         lam = [D(1,1), D(2,2)]\nerr = norm(A*V - V*D)\n\
         J = [2, 1; 0, 2]\ne = eig(J)\n\
         H = [2, 1; 1, 2]\n[U, S] = eig(H)\nherr = norm(H*U - U*S)",
    );
    let mut lams = vec_c(&ev, "lam");
    lams.sort_by(|a, b| a.im.partial_cmp(&b.im).unwrap());
    assert_c(lams[0], Complex64::new(0.0, -1.0), 1e-10);
    assert_c(lams[1], Complex64::new(0.0, 1.0), 1e-10);
    assert!(re(&ev, "err") < 1e-10, "residual {}", re(&ev, "err"));
    let e = vec_c(&ev, "e");
    assert_eq!(e.len(), 2);
    assert_c(e[0], Complex64::new(2.0, 0.0), 1e-10);
    assert_c(e[1], Complex64::new(2.0, 0.0), 1e-10);
    // Hermitian [2, 1; 1, 2] has eigenvalues 1 and 3, ascending.
    let s = mat(&ev, "S");
    assert!((s[0][0].re - 1.0).abs() < 1e-10);
    assert!((s[1][1].re - 3.0).abs() < 1e-10);
    assert!(re(&ev, "herr") < 1e-10);
    assert!(eval_err("e = eig([1, 2, 3; 4, 5, 6])").contains("square"));
    assert!(
        eval_err("e = eig([1, NaN; 0, 1])").contains("NaN")
            || eval_err("e = eig([1, NaN; 0, 1])").contains("finite")
            || eval_err("e = eig([1, NaN; 0, 1])").contains("Inf")
    );
}

#[test]
fn eigs_of_a_diagonal_sparse_matrix() {
    let ev = eval(
        "A = sparse([1, 2, 3], [1, 2, 3], [1, 4, 9], 3, 3)\n\
         [V, D] = eigs(A, 2, \"lm\")\n\
         s = real(D(1)) + real(D(2))\n\
         r0 = norm(A * V(:, 1)' - D(1) * V(:, 1)')\n\
         r1 = norm(A * V(:, 2)' - D(2) * V(:, 2)')",
    );
    assert_re(&ev, "s", 13.0, 1e-8);
    assert!(re(&ev, "r0") < 1e-8, "{}", re(&ev, "r0"));
    assert!(re(&ev, "r1") < 1e-8, "{}", re(&ev, "r1"));
}

#[test]
fn expm_closed_forms() {
    let ev = eval(
        "R = expm([0, -1; 1, 0])\n\
         N = expm([0, 1; 0, 0])\n\
         J = expm([2, 1; 0, 2])\n\
         Z = expm(zeros(3, 3))\n\
         Big = expm(10 * [0, -1; 1, 0])",
    );
    let r = mat(&ev, "R");
    assert_c(r[0][0], Complex64::new(1.0_f64.cos(), 0.0), 1e-12);
    assert_c(r[0][1], Complex64::new(-1.0_f64.sin(), 0.0), 1e-12);
    assert_c(r[1][0], Complex64::new(1.0_f64.sin(), 0.0), 1e-12);
    assert_c(r[1][1], Complex64::new(1.0_f64.cos(), 0.0), 1e-12);
    let n = mat(&ev, "N");
    assert_c(n[0][0], Complex64::new(1.0, 0.0), 1e-12);
    assert_c(n[0][1], Complex64::new(1.0, 0.0), 1e-12);
    assert_c(n[1][1], Complex64::new(1.0, 0.0), 1e-12);
    // expm(2 I + N) = e^2 (I + N)
    let e2 = 2.0_f64.exp();
    let j = mat(&ev, "J");
    assert_c(j[0][0], Complex64::new(e2, 0.0), 1e-10);
    assert_c(j[0][1], Complex64::new(e2, 0.0), 1e-10);
    assert_c(j[1][0], Complex64::new(0.0, 0.0), 1e-10);
    assert_c(j[1][1], Complex64::new(e2, 0.0), 1e-10);
    let z = mat(&ev, "Z");
    for i in 0..3 {
        for k in 0..3 {
            let want = if i == k { 1.0 } else { 0.0 };
            assert_c(z[i][k], Complex64::new(want, 0.0), 1e-12);
        }
    }
    let big = mat(&ev, "Big");
    assert_c(big[0][0], Complex64::new(10.0_f64.cos(), 0.0), 1e-10);
    assert_c(big[0][1], Complex64::new(-10.0_f64.sin(), 0.0), 1e-10);
    assert_c(big[1][0], Complex64::new(10.0_f64.sin(), 0.0), 1e-10);
    assert!(eval_err("y = expm([1, 2, 3; 4, 5, 6])").contains("square"));
    assert!(eval_err("y = expm([NaN, 0; 0, 0])").contains("finite"));
}

#[test]
fn expm_of_the_moler_van_loan_matrix() {
    // A = [-49, 24; -64, 31] has eigenvalues -1 and -17.
    // P = [1, 3; 2, 4], expm(A) = P diag(e^{-1}, e^{-17}) P^{-1}.
    let ev = eval("E = expm([-49, 24; -64, 31])");
    let e = mat(&ev, "E");
    let p = [[1.0, 3.0], [2.0, 4.0]];
    let pinv = [[-2.0, 1.5], [1.0, -0.5]]; // det P = -2
    let d0 = (-1.0_f64).exp();
    let d1 = (-17.0_f64).exp();
    let diag = [d0, d1];
    for i in 0..2 {
        for j in 0..2 {
            let mut want = 0.0;
            for k in 0..2 {
                want += p[i][k] * diag[k] * pinv[k][j];
            }
            assert_c(e[i][j], Complex64::new(want, 0.0), 1e-10);
        }
    }
}

#[test]
fn kron_diag_trace_known_values() {
    // kron([1, 2; 3, 4], [0, 5; 6, 7]) block (i,j) = a_ij * B.
    let k = mat(&eval("K = kron([1, 2; 3, 4], [0, 5; 6, 7])"), "K");
    let b = [[0.0, 5.0], [6.0, 7.0]];
    let a = [[1.0, 2.0], [3.0, 4.0]];
    for i in 0..2 {
        for j in 0..2 {
            for r in 0..2 {
                for c in 0..2 {
                    assert_c(
                        k[i * 2 + r][j * 2 + c],
                        Complex64::new(a[i][j] * b[r][c], 0.0),
                        1e-12,
                    );
                }
            }
        }
    }
    // A row and a column is an outer product.
    let o = mat(&eval("O = kron([1, 2, 3], [10; 20])"), "O");
    assert_eq!(o.len(), 2);
    assert_eq!(o[0].len(), 3);
    assert_c(o[1][2], Complex64::new(60.0, 0.0), 1e-12);

    let d = mat(&eval("D = diag([1, 2, 3], 1)"), "D");
    assert_eq!(d.len(), 4);
    assert_c(d[0][1], Complex64::new(1.0, 0.0), 0.0);
    assert_c(d[1][2], Complex64::new(2.0, 0.0), 0.0);
    assert_c(d[2][3], Complex64::new(3.0, 0.0), 0.0);
    assert_c(d[0][0], Complex64::new(0.0, 0.0), 0.0);
    let back = vec_c(&eval("v = diag([1, 2; 3, 4])"), "v");
    assert_c(back[0], Complex64::new(1.0, 0.0), 0.0);
    assert_c(back[1], Complex64::new(4.0, 0.0), 0.0);

    assert_re(&eval("t = trace([1, 2; 3, 4])"), "t", 5.0, 1e-15);
    assert_c(
        num(&eval("t = trace([j, 0; 0, 2j])"), "t"),
        Complex64::new(0.0, 3.0),
        1e-12,
    );
    // Rectangular trace is the main diagonal (MATLAB errors; recorded as a divergence).
    assert_re(&eval("t = trace([1, 2, 3; 4, 5, 6])"), "t", 6.0, 1e-15);
}

// ── roots, fft, conv, filter, freqz, stats, sort ────────────────────────────

#[test]
fn roots_of_known_polynomials() {
    // (x-1)(x-2)(x-3) = x^3 - 6x^2 + 11x - 6
    let mut r = vec_c(&eval("r = roots([1, -6, 11, -6])"), "r");
    r.sort_by(|a, b| a.re.partial_cmp(&b.re).unwrap());
    assert_c(r[0], Complex64::new(1.0, 0.0), 1e-8);
    assert_c(r[1], Complex64::new(2.0, 0.0), 1e-8);
    assert_c(r[2], Complex64::new(3.0, 0.0), 1e-8);
    // (x-1)^2
    let r = vec_c(&eval("r = roots([1, -2, 1])"), "r");
    assert_eq!(r.len(), 2);
    assert_c(r[0], Complex64::new(1.0, 0.0), 1e-8);
    assert_c(r[1], Complex64::new(1.0, 0.0), 1e-8);
    // x^2 + 1 = 0 → ±i
    let mut r = vec_c(&eval("r = roots([1, 0, 1])"), "r");
    r.sort_by(|a, b| a.im.partial_cmp(&b.im).unwrap());
    assert_c(r[0], Complex64::new(0.0, -1.0), 1e-8);
    assert_c(r[1], Complex64::new(0.0, 1.0), 1e-8);
    assert!(vec_c(&eval("r = roots([0, 0, 5])"), "r").is_empty());
}

#[test]
fn fft_ifft_script_matches_the_dft() {
    let y = vec_c(&eval("y = fft([1, 2, 3, 4])"), "y");
    assert_c(y[0], Complex64::new(10.0, 0.0), 1e-10);
    assert_c(y[1], Complex64::new(-2.0, 2.0), 1e-10);
    assert_c(y[2], Complex64::new(-2.0, 0.0), 1e-10);
    assert_c(y[3], Complex64::new(-2.0, -2.0), 1e-10);
    let ev = eval(
        "x = [0.2, -1, 3, 0.5, 4, -2, 1]\n\
         e = norm(ifft(fft(x)) - x)\n\
         f = norm(ifft(fft([1, 2, 3, 4, 5, 6])) - [1, 2, 3, 4, 5, 6])",
    );
    assert!(re(&ev, "e") < 1e-10);
    assert!(re(&ev, "f") < 1e-10);
}

#[test]
fn convolve_and_complex_arithmetic() {
    let y = vec_c(&eval("y = convolve([1, 2, 3], [4, 5])"), "y");
    for (g, w) in y.iter().zip([4.0, 13.0, 22.0, 15.0]) {
        assert_c(*g, Complex64::new(w, 0.0), 1e-12);
    }
    // (1+2i)(3+4i) = -5+10i. (1+i)/(1-i) = i.
    assert_c(
        num(&eval("z = (1+2j) * (3+4j)"), "z"),
        Complex64::new(-5.0, 10.0),
        1e-12,
    );
    assert_c(
        num(&eval("z = (1+j) / (1-j)"), "z"),
        Complex64::new(0.0, 1.0),
        1e-12,
    );
    assert_c(
        num(&eval("z = sqrt(-4)"), "z"),
        Complex64::new(0.0, 2.0),
        1e-12,
    );
    let pi = std::f64::consts::PI;
    assert_c(
        num(&eval("z = log(-1)"), "z"),
        Complex64::new(0.0, pi),
        1e-12,
    );
    assert_c(
        num(&eval("z = log10(-1)"), "z"),
        Complex64::new(0.0, pi / 10.0_f64.ln()),
        1e-12,
    );
    assert_c(
        num(&eval("z = log2(-1)"), "z"),
        Complex64::new(0.0, pi / 2.0_f64.ln()),
        1e-12,
    );
    assert_re(&eval("z = log10(1000)"), "z", 3.0, 1e-12);
}

#[test]
fn filter_recurrence_and_butterworth_pair() {
    // y[n] = x[n] + 0.5 y[n-1]
    let y = vec_c(&eval("y = filter([1], [1, -0.5], [1, 0, 0, 0])"), "y");
    for (g, w) in y.iter().zip([1.0, 0.5, 0.25, 0.125]) {
        assert_c(*g, Complex64::new(w, 0.0), 1e-12);
    }
    // a(1) normalisation: filter([2], [2], x) is the identity.
    let y = vec_c(&eval("y = filter([2, 0], [2], [3, 4])"), "y");
    assert_c(y[0], Complex64::new(3.0, 0.0), 1e-12);
    assert_c(y[1], Complex64::new(4.0, 0.0), 1e-12);

    let ev = eval(
        "[b, a] = butterworth_lowpass(4, 1000, 8000)\n\
         dc = sum(b) / sum(a)\n\
         [bh, ah] = butterworth_highpass(3, 1000, 8000)\n\
         hdc = sum(bh) / sum(ah)\n\
         y = filtfilt(b, a, ones(1, 48))\n\
         drift = max(abs(y - 1))",
    );
    assert_re(&ev, "dc", 1.0, 1e-8);
    assert!(
        re(&ev, "hdc").abs() < 1e-8,
        "highpass DC {}",
        re(&ev, "hdc")
    );
    assert!(
        re(&ev, "drift") < 1e-6,
        "filtfilt drift {}",
        re(&ev, "drift")
    );
    assert_eq!(vec_c(&ev, "b").len(), 5);
    assert_eq!(vec_c(&ev, "a").len(), 5);
    assert!((vec_c(&ev, "a")[0].re - 1.0).abs() < 1e-12);
    // One output is still the numerator alone.
    assert_eq!(
        vec_c(&eval("b = butterworth_lowpass(2, 1000, 8000)"), "b").len(),
        3
    );
    assert!(eval_err("y = filter([1+j], [1], [1])").contains("real"));
}

#[test]
fn freqz_delay_and_lowpass_dc() {
    let ev = eval(
        "H = freqz([0, 1], 5, 1000)\n\
         f2 = H(1, 3)\n\
         h2 = H(2, 3)\n\
         h = fir_lowpass(31, 1000, 8000, \"hann\")\n\
         dc = sum(h)\n\
         w = window(\"hann\", 4)",
    );
    assert_re(&ev, "f2", 200.0, 1e-9);
    // H(fs/5) of a one-sample delay is exp(-j 2π/5). Bin 3 (1-based) is k=2,
    // f = 200 Hz, phase = -2π * 0.2.
    let ang = -2.0 * std::f64::consts::PI * 0.2;
    assert_c(num(&ev, "h2"), Complex64::new(ang.cos(), ang.sin()), 1e-9);
    assert_re(&ev, "dc", 1.0, 1e-12);
    let w = vec_c(&ev, "w");
    for (g, want) in w.iter().zip([0.0, 0.75, 0.75, 0.0]) {
        assert_c(*g, Complex64::new(want, 0.0), 1e-12);
    }
}

#[test]
fn stats_match_the_definitions() {
    assert_re(&eval("m = mean([1, 2, 3, 4])"), "m", 2.5, 1e-15);
    // Sample variance of 1..4 is 5/3; std is its square root.
    assert_re(&eval("v = var([1, 2, 3, 4])"), "v", 5.0 / 3.0, 1e-12);
    assert_re(
        &eval("s = std([1, 2, 3, 4])"),
        "s",
        (5.0_f64 / 3.0).sqrt(),
        1e-12,
    );
    assert_re(&eval("s = std([5])"), "s", 0.0, 0.0);
    assert_re(&eval("m = median([3, 1, 2])"), "m", 2.0, 1e-15);
    assert_re(&eval("m = median([4, 1, 3, 2])"), "m", 2.5, 1e-15);
    assert!(re(&eval("m = median([1, NaN, 3])"), "m").is_nan());
    // Column means of [1, 2; 3, 4] are [2, 3].
    let m = mat(&eval("M = mean([1, 2; 3, 4])"), "M");
    assert_c(m[0][0], Complex64::new(2.0, 0.0), 1e-15);
    assert_c(m[0][1], Complex64::new(3.0, 0.0), 1e-15);
    // Complex mean.
    assert_c(
        num(&eval("m = mean([1+j, 3+3j])"), "m"),
        Complex64::new(2.0, 2.0),
        1e-12,
    );
    // cumsum
    let c = vec_c(&eval("c = cumsum([1, 2, 3, 4])"), "c");
    for (g, w) in c.iter().zip([1.0, 3.0, 6.0, 10.0]) {
        assert_c(*g, Complex64::new(w, 0.0), 1e-15);
    }
    let cm = mat(&eval("C = cumsum([1, 2; 3, 4])"), "C");
    assert_c(cm[0][0], Complex64::new(1.0, 0.0), 1e-15);
    assert_c(cm[1][0], Complex64::new(4.0, 0.0), 1e-15);
    assert_c(cm[0][1], Complex64::new(2.0, 0.0), 1e-15);
    assert_c(cm[1][1], Complex64::new(6.0, 0.0), 1e-15);
}

#[test]
fn sort_order_and_indices() {
    let ev = eval("[s, i] = sort([3, 1, 2])\n[d, j] = sort([3, 1, 2], \"descend\")");
    let s = vec_c(&ev, "s");
    let idx = vec_c(&ev, "i");
    assert_c(s[0], Complex64::new(1.0, 0.0), 0.0);
    assert_c(s[1], Complex64::new(2.0, 0.0), 0.0);
    assert_c(s[2], Complex64::new(3.0, 0.0), 0.0);
    assert_c(idx[0], Complex64::new(2.0, 0.0), 0.0);
    assert_c(idx[1], Complex64::new(3.0, 0.0), 0.0);
    assert_c(idx[2], Complex64::new(1.0, 0.0), 0.0);
    let d = vec_c(&ev, "d");
    assert_c(d[0], Complex64::new(3.0, 0.0), 0.0);
    // Complex values sort by real part, not by magnitude (a recorded divergence).
    let z = vec_c(&eval("z = sort([1+5j, -1, 0.5])"), "z");
    assert!((z[0].re - -1.0).abs() < 1e-15);
    assert!((z[1].re - 0.5).abs() < 1e-15);
    assert!((z[2].re - 1.0).abs() < 1e-15 && (z[2].im - 5.0).abs() < 1e-15);
    let infs = vec_c(&eval("z = sort([Inf, 1, -Inf])"), "z");
    assert!(infs[0].re.is_infinite() && infs[0].re.is_sign_negative());
    assert!((infs[1].re - 1.0).abs() < 1e-15);
    assert!(infs[2].re.is_infinite() && infs[2].re.is_sign_positive());
}

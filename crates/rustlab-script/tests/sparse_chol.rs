//! MATLAB-style sparse `chol` outputs and `decomposition`
//! (dev/plans/sparse_spd_solve.md, Phase 4).
//!
//! `R = chol(S)`, `[R, p] = chol(S)`, `[R, p, P] = chol(S)` with
//! `"matrix"` / `"vector"` and `"upper"` / `"lower"`, the triangular `\`
//! that `R \ b` must take, and `dS = decomposition(S, "chol"); x = dS \ b`.

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

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name) {
        Some(Value::Scalar(x)) => *x,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

fn is_sparse(ev: &Evaluator, name: &str) -> bool {
    matches!(ev.get(name), Some(Value::SparseMatrix(_)))
}

fn text(ev: &Evaluator, name: &str) -> String {
    match ev.get(name) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("{name}: expected a string, got {other:?}"),
    }
}

/// SPD 2-D operator, `N = n²` unknowns, and a right-hand side.
const GRID: &str = "n = 7; N = n * n; S = -1 * laplacian_2d(n, n); b = (1:N)' / N;";

/// `S` with one diagonal entry made negative (symmetric, indefinite).
const INDEFINITE: &str = "n = 5; N = n * n; S = -1 * laplacian_2d(n, n); S(13, 13) = -2;";

/// Complex Hermitian positive definite.
const HERMITIAN: &str = "n = 5; N = n * n; L = -1 * laplacian_2d(n, n);
     U = spdiags(0.3j * ones(N, 1), 1, N, N);
     S = L + U + U';";

// ─── R = chol(S) ──────────────────────────────────────────────────────────

#[test]
fn one_output_is_the_sparse_upper_factor() {
    let ev = eval(&format!(
        "{GRID}
         R = chol(S);
         sz = size(R); sq = sz(1) == N && sz(2) == N;
         [I, J] = find(R); upper = all(I <= J);
         e = norm(full(R' * R - S)) / norm(full(S));
         d = norm(full(R) - chol(full(S)));"
    ));
    assert!(is_sparse(&ev, "R"));
    assert_eq!(scalar(&ev, "sq"), 1.0);
    assert_eq!(scalar(&ev, "upper"), 1.0);
    assert!(scalar(&ev, "e") < 1e-14);
    assert!(
        scalar(&ev, "d") < 1e-12,
        "same as the dense natural-order factor"
    );
}

#[test]
fn one_output_is_unpermuted_and_two_outputs_report_zero() {
    let ev = eval(&format!(
        "{GRID}
         R1 = chol(S);
         [R2, p] = chol(S);
         d = norm(full(R1 - R2));"
    ));
    assert_eq!(scalar(&ev, "p"), 0.0);
    assert_eq!(scalar(&ev, "d"), 0.0);
}

#[test]
fn one_output_errors_when_not_positive_definite() {
    let e = eval_err(&format!("{INDEFINITE} R = chol(S);"));
    assert!(e.contains("positive definite"), "{e}");
}

// ─── [R, p] = chol(S) ─────────────────────────────────────────────────────

#[test]
fn flag_form_returns_the_partial_factor_without_error() {
    let ev = eval(&format!(
        "{INDEFINITE}
         [R, p] = chol(S);
         q = p - 1;
         sz = size(R); rows = sz(1); cols = sz(2);
         K = R' * R - S;
         top = norm(full(K(1:q, :)));
         left = norm(full(K(:, 1:q)));
         [I, J] = find(R); upper = all(I <= J);"
    ));
    let p = scalar(&ev, "p");
    assert_eq!(p, 13.0, "first pivot that fails in natural order");
    assert!(is_sparse(&ev, "R"));
    // MATLAB: R is q×n and R'*R agrees with S on the L-shaped region.
    assert_eq!(scalar(&ev, "rows"), 12.0);
    assert_eq!(scalar(&ev, "cols"), 25.0);
    assert!(scalar(&ev, "top") < 1e-13);
    assert!(scalar(&ev, "left") < 1e-13);
    assert_eq!(scalar(&ev, "upper"), 1.0);
}

#[test]
fn flag_form_on_a_singular_matrix() {
    let ev = eval(
        "S = sparse([1, 0, 0; 0, 0, 0; 0, 0, 2]);
         [R, p] = chol(S); sz = size(R); r = sz(1); c = sz(2); v = full(R(1, 1));",
    );
    assert_eq!(scalar(&ev, "p"), 2.0);
    assert_eq!(scalar(&ev, "r"), 1.0);
    assert_eq!(scalar(&ev, "c"), 3.0);
    assert_eq!(scalar(&ev, "v"), 1.0);
    assert!(eval_err("S = sparse([1, 0; 0, 0]); R = chol(S);").contains("positive definite"));
}

// ─── [R, p, P] = chol(S) ──────────────────────────────────────────────────

#[test]
fn three_outputs_reconstruct_the_permuted_matrix() {
    let ev = eval(&format!(
        "{GRID}
         [R, p, P] = chol(S);
         e = norm(full(R' * R - P' * S * P)) / norm(full(S));
         perm_ok = norm(full(P' * P - speye(N))) == 0 && nnz(P) == N;
         [I, J] = find(R); upper = all(I <= J);"
    ));
    assert_eq!(scalar(&ev, "p"), 0.0);
    assert!(is_sparse(&ev, "P"));
    assert!(is_sparse(&ev, "R"));
    assert!(scalar(&ev, "e") < 1e-14);
    assert_eq!(scalar(&ev, "perm_ok"), 1.0);
    assert_eq!(scalar(&ev, "upper"), 1.0);
}

#[test]
fn the_permutation_reduces_fill_on_a_3d_grid() {
    let ev = eval(
        "S = -1 * laplacian_3d(9, 9, 9);
         R0 = chol(S); [R, p, P] = chol(S);
         nat = nnz(R0); red = nnz(R);",
    );
    assert!(scalar(&ev, "red") < 0.8 * scalar(&ev, "nat"));
}

#[test]
fn vector_form_matches_the_matrix_form() {
    let ev = eval(&format!(
        "{GRID}
         [R, p, P] = chol(S);
         [Rv, pv, q] = chol(S, \"vector\");
         [Rm, pm, Pm] = chol(S, \"matrix\");
         sq = size(q); row = sq(1) == 1 && sq(2) == N;
         Pq = sparse(q, 1:N, ones(1, N), N, N);
         same_p = norm(full(Pq - P)) == 0 && norm(full(Pm - P)) == 0;
         same_r = norm(full(Rv - R)) == 0;
         e = norm(full(Rv' * Rv - S(q, q))) / norm(full(S));"
    ));
    assert_eq!(scalar(&ev, "row"), 1.0, "q is a 1×n row, as in MATLAB");
    assert_eq!(scalar(&ev, "same_p"), 1.0);
    assert_eq!(scalar(&ev, "same_r"), 1.0);
    assert_eq!(scalar(&ev, "pv"), 0.0);
    assert!(scalar(&ev, "e") < 1e-14);
}

#[test]
fn three_outputs_on_an_indefinite_matrix() {
    let ev = eval(&format!(
        "{INDEFINITE}
         [R, p, P] = chol(S);
         q = p - 1;
         K = R' * R - P' * S * P;
         sz = size(R);
         top = norm(full(K(1:q, :))); left = norm(full(K(:, 1:q)));"
    ));
    let p = scalar(&ev, "p");
    assert!(p >= 1.0);
    assert!(scalar(&ev, "top") < 1e-13);
    assert!(scalar(&ev, "left") < 1e-13);
}

#[test]
fn explicit_ordering_selects_the_permutation() {
    let ev = eval(&format!(
        "{GRID}
         [Rn, pn, Pn] = chol(S, \"natural\");
         nat = norm(full(Pn - speye(N))) == 0;
         [Rd, pd, Pd] = chol(S, \"nd\");
         ed = norm(full(Rd' * Rd - Pd' * S * Pd)) / norm(full(S));
         [Ra, pa, qa] = chol(S, \"amd\", \"vector\");
         ea = norm(full(Ra' * Ra - S(qa, qa))) / norm(full(S));"
    ));
    assert_eq!(scalar(&ev, "nat"), 1.0);
    assert!(scalar(&ev, "ed") < 1e-14);
    assert!(scalar(&ev, "ea") < 1e-14);
    let e = eval_err(&format!("{GRID} R = chol(S, \"nd\");"));
    assert!(e.contains("[R, p, P]"), "{e}");
    let e = eval_err(&format!("{GRID} [R, p, P] = chol(S, \"banana\");"));
    assert!(e.contains("banana"), "{e}");
}

// ─── "lower" / "upper" ────────────────────────────────────────────────────

#[test]
fn lower_returns_the_transpose() {
    let ev = eval(&format!(
        "{GRID}
         L = chol(S, \"lower\"); R = chol(S, \"upper\");
         [I, J] = find(L); lower = all(I >= J);
         e = norm(full(L * L' - S)) / norm(full(S));
         t = norm(full(L - R'));
         [L3, p, P] = chol(S, \"lower\");
         e3 = norm(full(L3 * L3' - P' * S * P)) / norm(full(S));
         [L4, p4, q] = chol(S, \"vector\", \"lower\");
         [L5, p5, q5] = chol(S, \"lower\", \"vector\");
         e4 = norm(full(L4 * L4' - S(q, q))) / norm(full(S));
         same = norm(full(L4 - L5)) == 0 && norm(q - q5) == 0;"
    ));
    assert!(is_sparse(&ev, "L"));
    assert_eq!(scalar(&ev, "lower"), 1.0);
    assert!(scalar(&ev, "e") < 1e-14);
    assert_eq!(scalar(&ev, "t"), 0.0);
    assert!(scalar(&ev, "e3") < 1e-14);
    assert!(scalar(&ev, "e4") < 1e-14);
    assert_eq!(scalar(&ev, "same"), 1.0);
}

#[test]
fn lower_flag_form_is_n_by_q() {
    let ev = eval(&format!(
        "{INDEFINITE}
         [L, p] = chol(S, \"lower\"); sz = size(L); r = sz(1); c = sz(2);
         q = p - 1; K = L * L' - S; top = norm(full(K(1:q, :)));"
    ));
    assert_eq!(scalar(&ev, "p"), 13.0);
    assert_eq!(scalar(&ev, "r"), 25.0);
    assert_eq!(scalar(&ev, "c"), 12.0);
    assert!(scalar(&ev, "top") < 1e-13);
}

#[test]
fn only_the_selected_triangle_is_read() {
    // MATLAB reads the upper triangle by default and assumes symmetry.
    let ev = eval(
        "S = sparse([4, 1, 0; 100, 3, 1; 7, -50, 5]);
         H = [4, 1, 0; 1, 3, 1; 0, 1, 5];
         R = chol(S); eu = norm(full(R' * R) - H);
         [R3, p, P] = chol(S); e3 = norm(full(R3' * R3 - P' * sparse(H) * P));
         T = sparse([4, 100, 7; 1, 3, -50; 0, 1, 5]);
         L = chol(T, \"lower\"); el = norm(full(L * L') - H);",
    );
    assert!(scalar(&ev, "eu") < 1e-14);
    assert!(scalar(&ev, "e3") < 1e-14);
    assert!(scalar(&ev, "el") < 1e-14);
}

// ─── Complex, small, and degenerate inputs ────────────────────────────────

#[test]
fn complex_hermitian_positive_definite() {
    let ev = eval(&format!(
        "{HERMITIAN}
         R = chol(S); e = norm(full(R' * R - S)) / norm(full(S));
         d = norm(full(R) - chol(full(S)));
         [R3, p, P] = chol(S); e3 = norm(full(R3' * R3 - P' * S * P)) / norm(full(S));
         L = chol(S, \"lower\"); el = norm(full(L * L' - S)) / norm(full(S));"
    ));
    assert!(scalar(&ev, "e") < 1e-14);
    assert!(scalar(&ev, "d") < 1e-12);
    assert!(scalar(&ev, "e3") < 1e-14);
    assert!(scalar(&ev, "el") < 1e-14);
    assert_eq!(scalar(&ev, "p"), 0.0);
}

#[test]
fn one_by_one_diagonal_and_empty() {
    let ev = eval(
        "R1 = chol(sparse(9)); v1 = full(R1(1, 1));
         D = chol(4 * speye(4)); dd = norm(full(D - 2 * speye(4)));
         [R0, p0, P0] = chol(spzeros(0, 0)); s0 = size(R0); n0 = s0(1) + s0(2);
         [Rn, pn] = chol(sparse(-1)); sn = size(Rn); rn = sn(1); cn = sn(2);",
    );
    assert!(is_sparse(&ev, "R1"));
    assert_eq!(scalar(&ev, "v1"), 3.0);
    assert_eq!(scalar(&ev, "dd"), 0.0);
    assert_eq!(scalar(&ev, "p0"), 0.0);
    assert_eq!(scalar(&ev, "n0"), 0.0);
    assert_eq!(scalar(&ev, "pn"), 1.0);
    assert_eq!(scalar(&ev, "rn"), 0.0);
    assert_eq!(scalar(&ev, "cn"), 1.0);
}

#[test]
fn bad_inputs_are_errors() {
    assert!(eval_err("R = chol(sparse([1, 2, 3; 4, 5, 6]));").contains("square"));
    assert!(eval_err("R = chol(sparse([1, NaN; NaN, 4]));").contains("finite"));
    assert!(eval_err("R = chol(speye(3), \"sideways\");").contains("sideways"));
    assert!(eval_err("[R, p, P, Q] = chol(speye(3));").contains("at most"));
}

// ─── Dense chol is unchanged ──────────────────────────────────────────────

#[test]
fn dense_chol_is_unchanged() {
    let ev = eval(
        "A = [4, 2; 2, 3]; R = chol(A); e = norm(R' * R - A);
         [R2, p] = chol([4, 0; 0, -1]); L = chol(A, \"lower\"); el = norm(L * L' - A);",
    );
    assert!(!is_sparse(&ev, "R"));
    assert!(scalar(&ev, "e") < 1e-14);
    assert!(scalar(&ev, "el") < 1e-14);
    assert_eq!(scalar(&ev, "p"), 2.0);
    let e = eval_err("[R, p, P] = chol([4, 2; 2, 3]);");
    assert!(e.contains("sparse"), "{e}");
}

// ─── Using R: triangular \ and the MATLAB solve idiom ─────────────────────

#[test]
fn matlab_solve_idiom_matches_backslash() {
    let ev = eval(&format!(
        "{GRID}
         [R, p, P] = chol(S);
         x = P * (R \\ (R' \\ (P' * b)));
         sx = size(x); col = sx(1) == N && sx(2) == 1;
         y = S \\ b;
         d = norm(x - y) / norm(y);
         R0 = chol(S); z = R0 \\ (R0' \\ b); dz = norm(z - y) / norm(y);"
    ));
    assert_eq!(scalar(&ev, "col"), 1.0);
    assert!(scalar(&ev, "d") < 1e-12);
    assert!(scalar(&ev, "dz") < 1e-12);
}

#[test]
fn sparse_triangular_backslash() {
    let ev = eval(
        "U = sparse([2, 1, 0; 0, 4, -1; 0, 0, 5]); b = [1; 2; 3];
         x = U \\ b; ex = norm(full(U) * x - b);
         y = U' \\ b; ey = norm(full(U') * y - b);
         z = U \\ [1j; 2; 3]; ez = norm(full(U) * z - [1j; 2; 3]);
         W = U \\ [b, 2 * b]; ew = norm(full(U) * W - [b, 2 * b]);",
    );
    for name in ["ex", "ey", "ez", "ew"] {
        assert!(scalar(&ev, name) < 1e-14, "{name}");
    }
}

// ─── Migration: solve(F, b) on a chol result ──────────────────────────────

#[test]
fn solve_on_a_chol_result_names_the_replacements() {
    let e = eval_err(&format!("{GRID} F = chol(S); x = solve(F, b);"));
    assert!(e.contains("decomposition"), "{e}");
    assert!(e.contains("S \\ b") || e.contains("A \\ b"), "{e}");
    assert!(e.contains("[R, p, P]"), "{e}");
    // lu(S) handles keep working with solve.
    let ev = eval(&format!(
        "{GRID} F = lu(S); x = solve(F, b); d = norm(x - S \\ b) / norm(S \\ b);"
    ));
    assert!(scalar(&ev, "d") < 1e-12);
}

// ─── decomposition ────────────────────────────────────────────────────────

#[test]
fn decomposition_chol_reuses_one_factor() {
    let ev = eval(&format!(
        "{GRID}
         dS = decomposition(S, \"chol\");
         x1 = dS \\ b; x2 = dS \\ (2 * b);
         X = dS \\ [b, 3 * b];
         y = S \\ b;
         d1 = norm(x1 - y) / norm(y); d2 = norm(x2 - 2 * y) / norm(y);
         dX = norm(X - [y, 3 * y]) / norm(y);
         sx = size(x1); col = sx(1) == N && sx(2) == 1;
         t = dS.Type; ms = dS.MatrixSize; msz = ms(1) == N && ms(2) == N;
         c = class(dS);
         z = dS \\ (1j * b); dz = norm(z - 1j * y) / norm(y);
         xs = solve(dS, b); ds = norm(xs - y) / norm(y);
         r = b' / dS; dr = norm(r - y') / norm(y);"
    ));
    for name in ["d1", "d2", "dX", "dz", "ds", "dr"] {
        assert!(scalar(&ev, name) < 1e-12, "{name}");
    }
    assert_eq!(scalar(&ev, "col"), 1.0);
    assert_eq!(scalar(&ev, "msz"), 1.0);
    assert_eq!(text(&ev, "t"), "chol");
    assert_eq!(text(&ev, "c"), "decomposition");
}

#[test]
fn decomposition_types_and_auto() {
    let ev = eval(
        "n = 8; A = -1 * laplacian_2d(n, n); b = ones(n * n, 1);
         G = ones(n, n); G(2:n-1, 2:n-1) = 0;
         [Ap, bp] = pin_dirichlet(A, b, G, 1.0);
         d1 = decomposition(A); t1 = d1.Type;
         d2 = decomposition(Ap); t2 = d2.Type;
         x2 = d2 \\ bp; e2 = norm(x2 - Ap \\ bp) / norm(Ap \\ bp);
         d3 = decomposition(Ap, \"lu\"); t3 = d3.Type;
         x3 = d3 \\ bp; e3 = norm(x3 - Ap \\ bp) / norm(Ap \\ bp);
         d4 = decomposition(sparse([2, 1; 0, 4])); t4 = d4.Type;
         d5 = decomposition(4 * speye(3)); t5 = d5.Type;
         d6 = decomposition(A, \"chol\", \"lower\"); t6 = d6.Type;
         x6 = d6 \\ b; e6 = norm(x6 - A \\ b) / norm(A \\ b);",
    );
    assert_eq!(text(&ev, "t1"), "chol");
    assert_eq!(text(&ev, "t2"), "chol");
    assert_eq!(text(&ev, "t3"), "lu");
    assert_eq!(text(&ev, "t4"), "triangular");
    assert_eq!(text(&ev, "t5"), "diagonal");
    assert_eq!(text(&ev, "t6"), "chol");
    assert!(scalar(&ev, "e2") < 1e-12);
    assert!(scalar(&ev, "e3") < 1e-12);
    assert!(scalar(&ev, "e6") < 1e-12);
}

#[test]
fn decomposition_chol_errors_when_not_positive_definite() {
    let e = eval_err(&format!("{INDEFINITE} dS = decomposition(S, \"chol\");"));
    assert!(e.contains("positive definite"), "{e}");
    let e = eval_err("dS = decomposition(speye(3), \"banana\");");
    assert!(e.contains("banana"), "{e}");
    let e = eval_err("dS = decomposition(speye(3)); x = dS \\ [1; 2];");
    assert!(e.contains("3"), "{e}");
}

#[test]
fn decomposition_of_a_dense_matrix() {
    let ev = eval(
        "A = [4, 1, 0; 1, 3, 1; 0, 1, 5]; b = [1; 2; 3]; y = A \\ b;
         d1 = decomposition(A); t1 = d1.Type; e1 = norm(d1 \\ b - y);
         d2 = decomposition(A, \"lu\"); e2 = norm(d2 \\ b - y);
         d3 = decomposition(A, \"chol\"); e3 = norm(d3 \\ b - y);
         M = [1, 2; 3, 4; 5, 7]; c = [1; 2; 4];
         d4 = decomposition(M); t4 = d4.Type; e4 = norm(d4 \\ c - M \\ c);
         d5 = decomposition([2, 1; 1j, 3]); t5 = d5.Type;
         e5 = norm(d5 \\ [1; 1] - [2, 1; 1j, 3] \\ [1; 1]);",
    );
    assert_eq!(text(&ev, "t1"), "chol");
    assert_eq!(text(&ev, "t4"), "qr");
    assert_eq!(text(&ev, "t5"), "lu");
    for name in ["e1", "e2", "e3", "e4", "e5"] {
        assert!(scalar(&ev, name) < 1e-12, "{name}");
    }
    let e = eval_err("dA = decomposition([1, 2; 2, 1], \"chol\");");
    assert!(e.contains("positive definite"), "{e}");
}

// ─── Sparse × sparse stays sparse (MATLAB) ────────────────────────────────

#[test]
fn sparse_times_sparse_is_sparse() {
    let ev = eval(&format!(
        "{GRID}
         [R, p, P] = chol(S);
         A = R' * R; B = P' * S * P; C = S * S; D = S' * S';
         e = norm(full(C) - full(S) * full(S)) + norm(full(D) - full(S)' * full(S)');
         Z = speye(3) * sparse([0, 1, 0; 0, 0, 0; 0, 0, 0]) * speye(3)';
         nz = nnz(Z); K = sparse([1, 1], [1, 2], [1, -1], 1, 2) * sparse([1, 2], [1, 1], [1, 1], 2, 1); k = nnz(K);"
    ));
    for name in ["A", "B", "C", "D", "Z"] {
        assert!(is_sparse(&ev, name), "{name} should be sparse");
    }
    assert!(scalar(&ev, "e") < 1e-12);
    assert_eq!(scalar(&ev, "nz"), 1.0);
    // Exact cancellation leaves no stored entry.
    assert_eq!(scalar(&ev, "k"), 0.0);
    assert!(eval_err("X = speye(3) * speye(4);").contains("dimension"));
}

// ─── Fused A' \ b and A.' \ b for triangular sparse A ─────────────────────

/// Complex upper `R`, complex lower `L`, complex diagonal `D`, and a
/// complex two-column right-hand side.
const TRI: &str = "n = 6; N = n * n; L0 = -1 * laplacian_2d(n, n);
     U = spdiags(0.3j * ones(N, 1), 1, N, N);
     H = L0 + U + U';
     R = chol(H); L = chol(H, \"lower\");
     Rr = chol(L0); Lr = chol(L0, \"lower\");
     D = spdiags((1:N)' + 2j * ones(N, 1), 0, N, N);
     y = (1:N)' / N + 0.5j * ones(N, 1);
     Y = [y, 2 * y - 1j];";

#[test]
fn transposed_triangular_solves_match_dense() {
    let ev = eval(&format!(
        "{TRI}
         e1 = norm(R' \\ y - full(R)' \\ y);
         e2 = norm(R.' \\ y - full(R).' \\ y);
         e3 = norm(L' \\ y - full(L)' \\ y);
         e4 = norm(L.' \\ y - full(L).' \\ y);
         e5 = norm(D' \\ y - full(D)' \\ y);
         e6 = norm(D.' \\ y - full(D).' \\ y);
         e7 = norm(Rr' \\ y - full(Rr)' \\ y) + norm(Rr.' \\ y - full(Rr).' \\ y);
         e8 = norm(Lr' \\ y - full(Lr)' \\ y) + norm(Lr.' \\ y - full(Lr).' \\ y);
         e9 = norm(R' \\ Y - full(R)' \\ Y) + norm(L.' \\ Y - full(L).' \\ Y);
         conj_differs = norm(R' \\ y - R.' \\ y) > 1e-3 && norm(D' \\ y - D.' \\ y) > 1e-3;
         s = size(R' \\ y); col = s(1) == N && s(2) == 1;
         s2 = size(R' \\ Y); two = s2(1) == N && s2(2) == 2;"
    ));
    for name in ["e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "e9"] {
        assert!(scalar(&ev, name) < 1e-12, "{name} = {}", scalar(&ev, name));
    }
    assert_eq!(scalar(&ev, "conj_differs"), 1.0);
    assert_eq!(scalar(&ev, "col"), 1.0);
    assert_eq!(scalar(&ev, "two"), 1.0);
}

#[test]
fn transposed_solve_matches_the_materialized_transpose_bit_for_bit() {
    let ev = eval(&format!(
        "{TRI}
         Rt = R'; Rn = R.'; Lt = L'; Ln = L.';
         d = norm(R' \\ y - Rt \\ y) + norm(R.' \\ y - Rn \\ y)
           + norm(L' \\ y - Lt \\ y) + norm(L.' \\ y - Ln \\ y);
         dr = norm(Rr' \\ real(y) - Rr.' \\ real(y));"
    ));
    assert_eq!(scalar(&ev, "d"), 0.0);
    assert_eq!(scalar(&ev, "dr"), 0.0, "real: ' and .' agree");
}

#[test]
fn transposed_solve_falls_back_for_other_matrices() {
    let ev = eval(
        "A = sparse([4, 1, 0; 2, 3, 1; 0, 2, 5]); b = [1; 2; 3];
         e = norm(A' \\ b - full(A)' \\ b) + norm(A.' \\ b - full(A).' \\ b);
         S = -1 * laplacian_2d(5, 5); c = ones(25, 1);
         es = norm(S' \\ c - S \\ c);
         M = sparse([2, 1; 0, 4]); f = (2 * M)' \\ [1; 1]; ef = norm(f - full(2 * M)' \\ [1; 1]);",
    );
    assert!(scalar(&ev, "e") < 1e-12);
    assert!(scalar(&ev, "es") < 1e-12);
    assert!(scalar(&ev, "ef") < 1e-14);
    // Shape errors read the same as with an explicit transpose.
    let e1 = eval_err("U = sparse([2, 1; 0, 4]); x = U' \\ [1; 2; 3];");
    let e2 = eval_err("U = sparse([2, 1; 0, 4]); Ut = U'; x = Ut \\ [1; 2; 3];");
    assert_eq!(e1, e2);
    // A zero on the diagonal keeps the non-fused path's behaviour.
    let z1 =
        run("U = sparse([0, 1; 0, 4]); x = U' \\ [1; 2];").map(|ev| format!("{:?}", ev.get("x")));
    let z2 = run("U = sparse([0, 1; 0, 4]); Ut = U'; x = Ut \\ [1; 2];")
        .map(|ev| format!("{:?}", ev.get("x")));
    assert_eq!(z1, z2);
}

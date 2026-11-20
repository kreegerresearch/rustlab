//! `S(rows, cols)` on a sparse matrix returns a sparse matrix.
//!
//! Index extraction used to densify the whole operator first; at 41³ that
//! asked for ~76 GB (`rustlab_em`
//! `dev/rustlab/requests/column-reshape-and-sparse-submatrix.md` §2). The
//! submatrix is gathered from the stored entries, so symmetric Dirichlet
//! elimination (`A_ff V_f = −A_fp V_p`) plus Cholesky works at that size.

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

fn sparse_shape(ev: &Evaluator, name: &str) -> (usize, usize, usize) {
    match ev.get(name) {
        Some(Value::SparseMatrix(s)) => (s.rows, s.cols, s.entries.len()),
        other => panic!("{name}: expected a sparse matrix, got {other:?}"),
    }
}

/// Every selection form must equal the dense result element for element.
#[test]
fn submatrix_matches_dense_indexing() {
    let ev = eval(
        "seed(7); S = sprand(9, 8, 0.4) + 1j * sprand(9, 8, 0.3); D = full(S);
         r = [7, 2, 2, 9, 1]; c = [8, 3, 1, 3];
         A = S(r, c);           eA = max(max(abs(full(A) - D(r, c))));
         B = S(:, c);           eB = max(max(abs(full(B) - D(:, c))));
         C = S(r, :);           eC = max(max(abs(full(C) - D(r, :))));
         E = S(2:5, end-2:end); eE = max(max(abs(full(E) - D(2:5, end-2:end))));
         m = [true, false, true, true, false, false, true, false, true];
         F = S(m, 2:3);         eF = max(max(abs(full(F) - D(m, 2:3))));
         G = S(:, :);           eG = max(max(abs(full(G) - D)));",
    );
    for (name, shape) in [
        ("A", (5, 4)),
        ("B", (9, 4)),
        ("C", (5, 8)),
        ("E", (4, 3)),
        ("F", (5, 2)),
        ("G", (9, 8)),
    ] {
        let (r, c, _) = sparse_shape(&ev, name);
        assert_eq!((r, c), shape, "{name}");
        assert_eq!(scalar(&ev, &format!("e{name}")), 0.0, "{name} values");
    }
}

#[test]
fn issparse_and_nnz_of_a_submatrix() {
    let ev = eval(
        "L = laplacian_2d(4, 4); k = [1, 2, 5, 6];
         A = L(k, k); s = issparse(A); n = nnz(A);
         Z = L([1, 2], [11, 16]); z = nnz(Z); zs = issparse(Z);",
    );
    assert_eq!(scalar(&ev, "s"), 1.0);
    // 2×2 block of grid nodes: 4 diagonals + 4 links stored both ways.
    assert_eq!(scalar(&ev, "n"), 12.0);
    assert_eq!(scalar(&ev, "zs"), 1.0);
    assert_eq!(scalar(&ev, "z"), 0.0);
}

#[test]
fn single_element_and_single_line_forms_are_dense() {
    let ev = eval(
        "S = sparse([1, 2, 3], [1, 2, 3], [4, 5, 6], 3, 3);
         a = S(2, 2); r = S(2, :); c = S(:, 3); nr = numel(r); r2 = r(2);",
    );
    assert_eq!(scalar(&ev, "a"), 5.0);
    assert!(
        matches!(ev.get("r"), Some(Value::Vector(_))),
        "{:?}",
        ev.get("r")
    );
    // A single column is a dense N×1 column, as `M(:, k)` on a dense matrix.
    assert!(
        matches!(ev.get("c"), Some(Value::Matrix(m)) if m.dim() == (3, 1)),
        "{:?}",
        ev.get("c")
    );
    assert_eq!(scalar(&ev, "nr"), 3.0);
    assert_eq!(scalar(&ev, "r2"), 5.0);
}

/// Single-line reads gathered from the stored entries match the dense read,
/// including repeated, unordered, and logical subscripts and complex values.
#[test]
fn single_line_forms_match_dense_indexing() {
    let ev = eval(
        "seed(11); S = sprand(9, 8, 0.4) + 1j * sprand(9, 8, 0.3); D = full(S);
         r = [7, 2, 2, 9, 1]; c = [8, 3, 1, 3];
         m = [true, false, true, true, false, false, true, false, true];
         e1 = max(abs(S(:, 3) - D(:, 3)));
         e2 = max(abs(S(4, :) - D(4, :)));
         e3 = max(abs(S(r, 5) - D(r, 5)));
         e4 = max(abs(S(6, c) - D(6, c)));
         e5 = max(abs(S(m, 2) - D(m, 2)));
         e6 = max(abs(S(2:5, end) - D(2:5, end)));
         s1 = size(S(:, 3)); s2 = size(S(4, :)); s3 = size(S(r, 5)); s4 = size(S(6, c));",
    );
    for name in ["e1", "e2", "e3", "e4", "e5", "e6"] {
        assert_eq!(scalar(&ev, name), 0.0, "{name}");
    }
    let size = |name: &str| match ev.get(name) {
        Some(v) => v
            .to_cvector()
            .unwrap()
            .iter()
            .map(|z| z.re)
            .collect::<Vec<_>>(),
        None => panic!("{name} missing"),
    };
    assert_eq!(size("s1"), vec![9.0, 1.0]);
    assert_eq!(size("s2"), vec![1.0, 8.0]);
    assert_eq!(size("s3"), vec![5.0, 1.0]);
    assert_eq!(size("s4"), vec![1.0, 4.0]);
}

/// `S(:, k)` and `S(k, :)` on the 41³ operator. Densifying the whole matrix
/// first asked for ~76 GB (`rustlab_em`, 0.3.77); the line is gathered from
/// the stored entries instead.
#[test]
fn large_3d_single_column_and_row_do_not_densify() {
    let ev = eval(
        "n = 41; N = n * n * n; L = -1 * laplacian_eps_3d(ones3(n, n, n), 1, 1, 1);
         k = ijk2k(21, 21, 21, n, n);
         c = L(:, k); r = L(k, :);
         nc = sum(c != 0); nr = sum(r != 0);
         dc = c(k); dr = r(k); tc = sum(c); tr = sum(r);
         nb = [k - 1, k + 1, k - n, k + n, k - n * n, k + n * n];
         ec = max(abs(c(nb) + 1)); er = max(abs(r(nb) + 1));",
    );
    assert!(
        matches!(ev.get("c"), Some(Value::Matrix(m)) if m.dim() == (68921, 1)),
        "column shape"
    );
    assert!(
        matches!(ev.get("r"), Some(Value::Vector(v)) if v.len() == 68921),
        "row shape"
    );
    for (name, want) in [
        ("nc", 7.0),
        ("nr", 7.0),
        ("dc", 6.0),
        ("dr", 6.0),
        ("tc", 0.0),
        ("tr", 0.0),
        ("ec", 0.0),
        ("er", 0.0),
    ] {
        assert_eq!(scalar(&ev, name), want, "{name}");
    }
}

#[test]
fn out_of_range_subscript_is_an_error() {
    let e = eval_err("S = speye(3); A = S([1, 4], [1, 2]);");
    assert!(e.contains("out of bounds") || e.contains("exceeds"), "{e}");
}

/// Symmetric elimination on a pinned capacitor: the free-free block is SPD,
/// so Cholesky applies, and the potential equals the `pin_dirichlet` + LU
/// result.
#[test]
fn symmetric_elimination_matches_pin_dirichlet() {
    let ev = eval(
        "n = 15; N = n * n;
         M = zeros(n, n); M(6:10, 6:10) = 1; G = ones(n, n); G(2:n-1, 2:n-1) = 0;
         P = M + G; pr = find(P(:)' > 0); fr = find(P(:)' == 0);
         Vp = M(:)'; Vp = Vp(pr);
         L = -1 * laplacian_2d(n, n);
         Aff = L(fr, fr); Afp = L(fr, pr);
         rhs = -1 * (Afp * Vp');
         Vf = spsolve(Aff, rhs, \"cholesky\");
         V = zeros(1, N); V(pr) = Vp; V(fr) = Vf;
         [A, b] = pin_dirichlet(L, zeros(N, 1), P, Vp);
         W = spsolve(A, b);
         D = V - W; err = max(abs(D(:)));
         sym = nnz(Aff - Aff');",
    );
    assert_eq!(scalar(&ev, "sym"), 0.0, "free-free block is symmetric");
    let err = scalar(&ev, "err");
    assert!(err < 1e-12, "elimination vs pin_dirichlet: {err:e}");
}

/// The request's size: 41³ with ~30k free nodes. Densifying would need
/// 76 GB; the sparse gather is a few MB.
#[test]
fn large_3d_submatrix_stays_sparse() {
    let ev = eval(
        "n = 41; L = -1 * laplacian_eps_3d(ones3(n, n, n), 1, 1, 1);
         fr = 1:2:(n * n * n);
         Aff = L(fr, fr); s = issparse(Aff); k = nnz(Aff);",
    );
    let (r, c, nnz) = sparse_shape(&ev, "Aff");
    assert_eq!((r, c), (34461, 34461));
    assert_eq!(scalar(&ev, "s"), 1.0);
    // Odd linear indices only: every neighbour offset (1, 41, 1681) is odd,
    // so each kept row keeps just its diagonal.
    assert_eq!(nnz, 34461);
    assert_eq!(scalar(&ev, "k"), 34461.0);
}

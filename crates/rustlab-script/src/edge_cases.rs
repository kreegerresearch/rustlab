//! Edge cases for the math shipped after 0.3.24.
//!
//! Expected values are closed forms, a published formula (Fritsch–Carlson,
//! modified Akima, the `xcorr` sum), or the MATLAB rule named in the
//! assertion. Nothing here is a number copied out of a previous run.

use num_complex::Complex64;

use crate::eval::value::Value;
use crate::testutil::{
    assert_c, check_err, check_re, close, eval, mat_re, matrix_dim, re, vec_c, vec_re,
};

fn approx_c(got: &[Complex64], want: &[(f64, f64)], tol: f64) {
    assert_eq!(
        got.len(),
        want.len(),
        "length {} vs {}",
        got.len(),
        want.len()
    );
    for (i, (g, (wr, wi))) in got.iter().zip(want).enumerate() {
        let d = (*g - Complex64::new(*wr, *wi)).norm();
        assert!(
            d <= tol,
            "index {i}: got {g}, want ({wr}, {wi}), |diff| {d:e}"
        );
    }
}

fn real_nan(ev: &crate::eval::Evaluator, name: &str) {
    match ev.get(name).unwrap() {
        Value::Scalar(x) => {
            assert!(x.is_nan(), "{name} = {x}, want a real NaN");
        }
        other => panic!("{name}: expected a real scalar NaN, got {other:?}"),
    }
}

// ── constructors: one size is square; empty and illegal sizes ──────────────

#[test]
fn square_constructors_and_illegal_sizes() {
    let ev = eval(
        "Z = zeros(3);
         R = zeros(1, 4);
         C = zeros(2, 0);
         E = zeros(0);
         O = ones(2);
         I = zeros(2, \"int32\");",
    );
    assert_eq!(matrix_dim(&ev, "Z"), (3, 3));
    assert_eq!(matrix_dim(&ev, "R"), (1, 4));
    assert_eq!(matrix_dim(&ev, "C"), (2, 0));
    assert_eq!(matrix_dim(&ev, "E"), (0, 0));
    assert_eq!(matrix_dim(&ev, "O"), (2, 2));
    assert!(mat_re(&ev, "O").iter().flatten().all(|v| *v == 1.0));
    match ev.get("I").unwrap() {
        Value::IntArray {
            rows, cols, data, ..
        } => {
            assert_eq!((*rows, *cols), (2, 2));
            assert!(data.iter().all(|v| *v == 0));
        }
        other => panic!("integer zeros: {other:?}"),
    }
    // rand() stays a scalar. A single size is square, and the draws sit in [0, 1).
    let ev = eval("seed(1); s = rand(); M = rand(2); G = randn(1, 3);");
    assert!(matches!(ev.get("s").unwrap(), Value::Scalar(_)));
    assert_eq!(matrix_dim(&ev, "M"), (2, 2));
    for row in mat_re(&ev, "M") {
        for v in row {
            assert!((0.0..1.0).contains(&v), "rand {v}");
        }
    }
    assert_eq!(matrix_dim(&ev, "G"), (1, 3));
    check_err(&[
        ("z = zeros(-1);", "non-negative"),
        ("z = ones(1.5);", "integer"),
        ("z = rand(-1);", "non-negative"),
        ("z = randn(2.2);", "integer"),
        ("z = zeros(0, -1);", "non-negative"),
    ]);
}

#[test]
fn randi_square_shape_and_closed_range() {
    // Every draw is an integer in the closed interval. lo > hi is an error.
    let ev = eval("seed(7); s = randi(5); M = randi([2, 4], 3); R = randi([0, 0], 1, 4);");
    let s = re(&ev, "s");
    assert_eq!(s, s.round());
    assert!((1.0..=5.0).contains(&s), "{s}");
    assert_eq!(matrix_dim(&ev, "M"), (3, 3));
    for row in mat_re(&ev, "M") {
        for v in row {
            assert_eq!(v, v.round());
            assert!((2.0..=4.0).contains(&v), "{v}");
        }
    }
    assert_eq!(matrix_dim(&ev, "R"), (1, 4));
    assert!(mat_re(&ev, "R").iter().flatten().all(|v| *v == 0.0));
    check_err(&[
        ("z = randi(0);", "lo"),
        ("z = randi([5, 1]);", "lo"),
        ("z = randi(3, -1);", "non-negative"),
    ]);
}

// ── std, var, median, sort, diag, trace ─────────────────────────────────────

#[test]
fn stats_shapes_and_published_values() {
    // Sample std of each column of [1, 2, 3; 4, 5, 6].
    // Mean 2.5, sum of squared deviations 4.5, divisor n−1 = 1.
    let cols = mat_re(&eval("S = std([1, 2, 3; 4, 5, 6]);"), "S");
    assert_eq!(cols.len(), 1);
    assert_eq!(cols[0].len(), 3);
    for v in &cols[0] {
        close(*v, 4.5_f64.sqrt());
    }
    // A 1×n matrix is one series, so the median is a scalar.
    // Even length: average of the two middle order statistics, (2+3)/2.
    check_re(&[
        ("m = median([1, 3, 2, 100]);", "m", 2.5, 0.0),
        ("v = var([1, 2, 3, 4], 1);", "v", 1.25, 1e-12),
        ("s = std(7);", "s", 0.0, 0.0),
        ("s = std([Inf]);", "s", 0.0, 0.0),
    ]);
    // std([1, 2, 3, 4], 1): mean 2.5, population variance
    // ((1.5²+0.5²+0.5²+1.5²)/4) = (2.25+0.25+0.25+2.25)/4 = 5/4 = 1.25.
    let ev = eval("m = median([1, NaN]);");
    real_nan(&ev, "m");
    let ev = eval("s = std([1, Inf]);");
    real_nan(&ev, "s");
    // Empty reductions are NaN, including a 0×0 matrix.
    real_nan(&eval("m = median(zeros(0));"), "m");
    real_nan(&eval("s = std(zeros(0));"), "s");
    // An empty matrix with columns still has one result per column.
    let empty_cols = mat_re(&eval("S = std(zeros(0, 3));"), "S");
    assert_eq!(empty_cols.len(), 1);
    assert_eq!(empty_cols[0].len(), 3);
    assert!(empty_cols[0].iter().all(|v| v.is_nan()));
    check_err(&[
        ("m = median([1, 2+j]);", "complex"),
        ("s = std([1, 2, 3], Inf);", "weight"),
        ("s = std([1, 2], [1, NaN]);", "finite"),
    ]);
}

#[test]
fn sort_edges_keep_order_and_rank_nonfinite() {
    // Empty stays empty. A scalar is already sorted; its index is 1.
    let ev = eval("e = sort([]); [s, i] = sort(5);");
    assert!(vec_re(&ev, "e").is_empty());
    close(re(&ev, "s"), 5.0);
    close(re(&ev, "i"), 1.0);
    // −Inf, then the finite values, then +Inf, then NaN. Ties keep the
    // earlier index: the first 2 stays ahead of the second.
    let ev = eval("[s, i] = sort([2, Inf, 2, -Inf, NaN]);");
    let s = vec_c(&ev, "s");
    assert!(s[0].re.is_infinite() && s[0].re.is_sign_negative());
    assert_eq!(s[1].re, 2.0);
    assert_eq!(s[2].re, 2.0);
    assert!(s[3].re.is_infinite() && s[3].re.is_sign_positive());
    assert!(s[4].re.is_nan());
    let idx = vec_re(&ev, "i");
    assert_eq!(idx[1], 1.0);
    assert_eq!(idx[2], 3.0);
    // A column sorts along its length. Dim 2 of a column is one observation
    // per row, so the column is unchanged and every index is 1.
    let ev = eval("c = sort([3; 1; 2]); [u, j] = sort([3; 1; 2], 2);");
    assert_eq!(matrix_dim(&ev, "c"), (3, 1));
    assert_eq!(vec_re(&ev, "c"), vec![1.0, 2.0, 3.0]);
    assert_eq!(vec_re(&ev, "u"), vec![3.0, 1.0, 2.0]);
    assert!(vec_re(&ev, "j").iter().all(|v| *v == 1.0));
    check_err(&[("s = sort([1, 2], 3);", "dim")]);
}

#[test]
fn diag_and_trace_edges() {
    // diag of no elements is 0×0. An offset on an empty vector is |k|×|k|.
    assert_eq!(matrix_dim(&eval("D = diag([]);"), "D"), (0, 0));
    let off = mat_re(&eval("D = diag([], 2);"), "D");
    assert_eq!(off.len(), 2);
    assert!(off.iter().flatten().all(|v| *v == 0.0));
    // A column builds a diagonal. k = 1 puts the entries on the superdiagonal.
    let d = mat_re(&eval("D = diag([7; 8], 1);"), "D");
    assert_eq!(d.len(), 3);
    close(d[0][1], 7.0);
    close(d[1][2], 8.0);
    close(d[0][0], 0.0);
    // Extracting past the corner is an empty vector.
    assert!(vec_re(&eval("d = diag(eye(2), 5);"), "d").is_empty());
    // trace of a square, of a scalar, of 0×0 (empty sum is 0), and of a
    // complex diagonal. A rectangle is an error.
    check_re(&[
        ("t = trace(eye(1));", "t", 1.0, 0.0),
        ("t = trace(5);", "t", 5.0, 0.0),
        ("t = trace(zeros(0));", "t", 0.0, 0.0),
        ("t = trace([1+j, 0; 0, 2-j]);", "t", 3.0, 1e-12),
    ]);
    check_err(&[
        // A vector is not a matrix. A column matrix is a rectangle.
        ("t = trace([1, 2, 3]);", "matrix"),
        ("t = trace([1; 2; 3]);", "square"),
        ("t = trace(zeros(2, 3));", "square"),
    ]);
}

// ── .\ and scalar-numerator / ───────────────────────────────────────────────

#[test]
fn elementwise_left_division_edges() {
    // A .\ B = B ./ A, including a zero denominator and a complex quotient.
    // 2 / (1+i) = 1−i.
    let ev = eval(
        "a = [1, 2] .\\ [2, 8];
         z = (1+j) .\\ 2;
         row = [1, 2] .\\ [2, 4; 6, 8];
         col = [1; 2] .\\ [2, 4; 6, 8];
         inf = 0 .\\ 1;
         zero = 1 .\\ 0;",
    );
    assert_eq!(vec_re(&ev, "a"), vec![2.0, 4.0]);
    assert_c(
        match ev.get("z").unwrap() {
            Value::Complex(z) => *z,
            Value::Scalar(x) => Complex64::new(*x, 0.0),
            other => panic!("{other:?}"),
        },
        Complex64::new(1.0, -1.0),
        1e-12,
    );
    let row = mat_re(&ev, "row");
    close(row[0][0], 2.0);
    close(row[0][1], 2.0);
    close(row[1][0], 6.0);
    close(row[1][1], 4.0);
    let col = mat_re(&ev, "col");
    close(col[0][0], 2.0);
    close(col[0][1], 4.0);
    close(col[1][0], 3.0);
    close(col[1][1], 4.0);
    assert!(re(&ev, "inf").is_infinite());
    close(re(&ev, "zero"), 0.0);
}

#[test]
fn right_division_edges() {
    // 2 / [2] solves X * 2 = 2. A 1×1 numerator against two columns does not.
    check_re(&[("x = 2 / [2];", "x", 1.0, 1e-12)]);
    check_err(&[
        ("M = eye(2); y = 2 / M;", "rows"),
        ("a = [1, 2]; b = [3, 4]; c = a / b;", "./"),
    ]);
    // An explicit one-row right-hand side stays a row, so two columns of a
    // 1×2 system. The vector form is a column and does not match that row.
    let ev = eval(
        "A = [1, 0; 0, 2];
         row = zeros(1, 2);
         row(1) = 4; row(2) = 6;
         X = row / A;",
    );
    let x = vec_re(&ev, "X");
    close(x[0], 4.0);
    close(x[1], 3.0);
    check_err(&[("A = [1, 2]; b = [3, 4]; x = A \\ b;", "rows")]);
}

// ── qr, backslash, chol, lu ─────────────────────────────────────────────────

#[test]
fn factor_edges() {
    // 1×1 QR has a nonnegative R. 5 = Q * R with Q = 1.
    let ev = eval("[Q, R] = qr(5); p = Q * R;");
    close(re(&ev, "Q"), 1.0);
    close(re(&ev, "R"), 5.0);
    close(re(&ev, "p"), 5.0);
    // A wide 1×n vector is one row. Economy Q is 1×1.
    let ev = eval("[Q, R] = qr([3, 4], 0);");
    close(re(&ev, "Q"), 1.0);
    let r = vec_re(&ev, "R");
    close(r[0].hypot(r[1]), 5.0);
    // Empty product: 0 rows, 2 columns, one right-hand side → 2×1.
    let ev = eval("x = zeros(0, 2) \\ zeros(0, 1);");
    assert_eq!(matrix_dim(&ev, "x"), (2, 1));
    assert!(mat_re(&ev, "x").iter().flatten().all(|v| *v == 0.0));
    // Overdetermined, full column rank: the least-squares solution of
    // [1; 1] x = [1; 3] is x = 2.
    let ev = eval("x = [1; 1] \\ [1; 3];");
    close(re(&ev, "x"), 2.0);
    // Hermitian but not positive definite stays on LU. [-1, 0; 0, 2] \\ [1; 4].
    let ev = eval("x = [-1, 0; 0, 2] \\ [1; 4];");
    assert_eq!(vec_re(&ev, "x"), vec![-1.0, 2.0]);
    // Hilbert-5 is SPD and ill-conditioned (cond ≈ 4.8e5). Solving H*(1) recovers 1.
    let ev = eval(
        "n = 5;
         H = zeros(n);
         for i = 1:n
           for j = 1:n
             H(i, j) = 1 / (i + j - 1);
           end
         end
         b = H * ones(n, 1);
         x = H \\ b;
         err = norm(x - ones(n, 1));",
    );
    let err = re(&ev, "err");
    assert!(err < 1e-8, "Hilbert-5 solution error {err:e}");
    check_err(&[
        ("[Q, R] = qr([1, NaN; 0, 1]);", "finite"),
        ("x = [1, 0; 0, 1] \\ [1, NaN];", "finite"),
        ("x = [1, Inf; 0, 1] \\ [1; 2];", "finite"),
    ]);
}

#[test]
fn chol_and_lu_edges() {
    // 1×1: R = 2 because 4 = 2*2. 0×0 factors with p = 0.
    check_re(&[("R = chol(4);", "R", 2.0, 1e-12)]);
    let ev = eval("[R, p] = chol(zeros(0));");
    close(re(&ev, "p"), 0.0);
    assert_eq!(matrix_dim(&ev, "R"), (0, 0));
    // A non-real diagonal is not a positive pivot. The real part alone
    // would not restore A = R'*R.
    let ev = eval("[R, p] = chol([1+j]);");
    close(re(&ev, "p"), 1.0);
    assert_eq!(matrix_dim(&ev, "R"), (0, 0));
    // The failure is the second pivot: the leading 1×1 is the square root of 4.
    // The leading 1×1 factor packs as a scalar, the same as chol(4).
    let ev = eval("A = [4, 0; 0, -1]; [R, p] = chol(A);");
    close(re(&ev, "p"), 2.0);
    close(re(&ev, "R"), 2.0);
    // 1×1 LU. A tall rectangle still satisfies P*A = L*U.
    let ev = eval(
        "[L, U, p] = lu(4, \"vector\");
         A = [1, 2, 3; 4, 5, 6];
         [Lt, Ut, Pt] = lu(A);
         D = Pt * A - Lt * Ut;",
    );
    close(re(&ev, "L"), 1.0);
    close(re(&ev, "U"), 4.0);
    close(vec_re(&ev, "p")[0], 1.0);
    for row in mat_re(&ev, "D") {
        for v in row {
            close(v, 0.0);
        }
    }
    check_err(&[
        ("R = chol([1+j]);", "positive"),
        ("R = chol([1, 2, 3; 4, 5, 6]);", "square"),
        ("R = chol([1, NaN; 0, 1]);", "finite"),
        ("[L, U] = lu([1, NaN; 0, 1]);", "finite"),
    ]);
}

// ── polyfit and interp1 ─────────────────────────────────────────────────────

#[test]
fn polyfit_degree_zero_duplicates_and_centering() {
    // Degree 0 is the mean. Residual of [1, 2, 3] about 2 is sqrt(2).
    // df = 3 − 1.
    let ev = eval(
        "[p, S] = polyfit([1, 2, 3], [1, 2, 3], 0);
         df = S.df; nr = S.normr;",
    );
    close(re(&ev, "p"), 2.0);
    close(re(&ev, "df"), 2.0);
    close(re(&ev, "nr"), 2.0_f64.sqrt());
    // Duplicate x is a least-squares column, not an error. Degree 0 ignores x.
    let ev = eval("[p, S] = polyfit([1, 1, 1], [2, 4, 6], 0); nr = S.normr;");
    close(re(&ev, "p"), 4.0);
    close(re(&ev, "nr"), (4.0_f64 + 0.0 + 4.0).sqrt());
    // Three outputs center. x = 1, 2, 3 has mean 2 and sample std 1, and
    // y = x² = (z+2)² = z² + 4z + 4.
    let ev = eval(
        "[p, S, mu] = polyfit([1, 2, 3], [1, 4, 9], 2);
         yhat = polyval(p, [1, 2, 3], S, mu);",
    );
    let mu = vec_re(&ev, "mu");
    close(mu[0], 2.0);
    close(mu[1], 1.0);
    let p = vec_re(&ev, "p");
    close(p[0], 1.0);
    close(p[1], 4.0);
    close(p[2], 4.0);
    let yhat = vec_re(&ev, "yhat");
    close(yhat[0], 1.0);
    close(yhat[1], 4.0);
    close(yhat[2], 9.0);
    // A constant x has no scale. A negative degree is not a size.
    check_err(&[
        (
            "[p, S, mu] = polyfit([2, 2, 2], [1, 2, 3], 1);",
            "standard deviation",
        ),
        ("p = polyfit([1, 2], [1, 2], -1);", "non-negative"),
        ("p = polyfit([1, 1+j], [1, 2], 1);", "real"),
    ]);
}

#[test]
fn interp1_edges_match_the_formulas() {
    // Duplicates and a non-monotonic interior are rejected. One sample is not
    // an interval. A NaN query stays a real NaN even when a fill is set.
    // Inf is outside the data: the fill replaces it, and linear extrap of
    // y = x runs away to +Inf.
    check_err(&[
        ("yi = interp1([0, 1, 1], [0, 1, 2], 0.5);", "monotonic"),
        ("yi = interp1([0, 2, 1], [0, 1, 2], 0.5);", "monotonic"),
        ("yi = interp1([1], [1], 1);", "two samples"),
        ("yi = interp1([], [], 0);", "empty"),
    ]);
    let ev = eval(
        "         n = interp1([0, 1], [0, 1], NaN);
         nf = interp1([0, 1], [0, 1], NaN, \"linear\", 0);
         bare = interp1([0, 1], [0, 1], Inf);
         f = interp1([0, 1], [0, 1], Inf, \"linear\", 0);
         e = interp1([0, 1], [0, 1], Inf, \"linear\", \"extrap\");
         near = interp1([0, 1], [0, 1], Inf, \"nearest\", \"extrap\");
         q = interp1([0, 1], [0, 1], [0.25, NaN]);",
    );
    real_nan(&ev, "n");
    real_nan(&ev, "nf");
    real_nan(&ev, "bare");
    close(re(&ev, "f"), 0.0);
    close(re(&ev, "near"), 1.0);
    assert!(re(&ev, "e").is_infinite() && re(&ev, "e").is_sign_positive());
    let q = vec_re(&ev, "q");
    close(q[0], 0.25);
    assert!(q[1].is_nan());
    // Fritsch–Carlson on a strictly increasing sample stays inside each span.
    let ev = eval(
        "x = [0, 1, 2, 3];
         y = [0, 1, 4, 9];
         q = interp1(x, y, [0.5, 1.5, 2.5], \"pchip\");",
    );
    let q = vec_re(&ev, "q");
    let spans = [(0.0, 1.0), (1.0, 4.0), (4.0, 9.0)];
    for (v, (lo, hi)) in q.iter().zip(spans) {
        assert!(
            *v >= lo - 1e-9 && *v <= hi + 1e-9,
            "{v} not in [{lo}, {hi}]"
        );
    }
    // Modified Akima on a flat run followed by a unit jump.
    // Secants δ = [0, 0, 1]. The endpoint extension gives slope 0 at x = 2
    // and 1.3 at x = 3. The Hermite value at 2.5 is 0.3375.
    check_re(&[(
        "m = interp1([0, 1, 2, 3], [0, 0, 0, 1], 2.5, \"makima\");",
        "m",
        0.3375,
        1e-12,
    )]);
    // A column of samples interpolates to a scalar, and the query shape is kept.
    let ev = eval(
        "y = [0; 1; 0];
         s = interp1([0, 1, 2], y, 0.5);
         M = interp1([0, 1, 2], y, [0, 1; 2, 0.5]);",
    );
    close(re(&ev, "s"), 0.5);
    assert_eq!(matrix_dim(&ev, "M"), (2, 2));
    let m = mat_re(&ev, "M");
    close(m[0][0], 0.0);
    close(m[0][1], 1.0);
    close(m[1][0], 0.0);
    close(m[1][1], 0.5);
}

// ── sparse exact zeros, and xcorr ──────────────────────────────────────────

#[test]
fn sparse_keeps_tiny_values_and_signed_zero_is_empty() {
    // −0 is an exact zero. A tiny real part, a tiny imaginary part, and Inf
    // stay stored. Setting an entry to 0 removes it. Two stored values that
    // cancel to an exact zero are not stored.
    let ev = eval(
        "S = sparse([1, 2, 3, 4], [1, 1, 1, 1], [1e-18, -0, 1e-8j, Inf], 4, 1);
         k = nnz(S);
         a = S(1, 1);
         b = S(2, 1);
         c = S(3, 1);
         d = S(4, 1);
         S(1, 1) = 0;
         k2 = nnz(S);
         Z = sparsevec([1, 1], [1e-16, -1e-16], 3);
         kz = nnz(Z);",
    );
    close(re(&ev, "k"), 3.0);
    close(re(&ev, "a"), 1e-18);
    close(re(&ev, "b"), 0.0);
    let c = match ev.get("c").unwrap() {
        Value::Complex(z) => *z,
        other => panic!("tiny imag entry: {other:?}"),
    };
    // 1e-8 is above the 1e-12 imaginary flush on sparse indexing, and
    // below any old structural-zero cutoff, so the stored part round-trips.
    assert_c(c, Complex64::new(0.0, 1e-8), 1e-20);
    assert!(re(&ev, "d").is_infinite());
    close(re(&ev, "k2"), 2.0);
    close(re(&ev, "kz"), 0.0);
}

#[test]
fn xcorr_unequal_lengths_maxlag_and_scales() {
    // x = [1, 2, 3], y = [4, 5]. Positive lag m is sum_n x[n+m] conj(y[n]).
    // N = 3. Lags −2..2: [0, 5, 14, 23, 12].
    let raw = [
        (0.0, 0.0),
        (5.0, 0.0),
        (14.0, 0.0),
        (23.0, 0.0),
        (12.0, 0.0),
    ];
    let ev = eval(
        "r = xcorr([1, 2, 3], [4, 5]);
         b = xcorr([1, 2, 3], [4, 5], \"biased\");
         u = xcorr([1, 2, 3], [4, 5], \"unbiased\");
         c = xcorr([1, 2, 3], [4, 5], \"coeff\");
         n = xcorr([1, 2, 3], [4, 5], \"normalized\");",
    );
    approx_c(&vec_c(&ev, "r"), &raw, 1e-9);
    let n = 3.0;
    let biased: Vec<(f64, f64)> = raw.iter().map(|(re, _)| (re / n, 0.0)).collect();
    approx_c(&vec_c(&ev, "b"), &biased, 1e-9);
    // Unbiased divides by N−|m|: 1, 2, 3, 2, 1.
    let den = [1.0, 2.0, 3.0, 2.0, 1.0];
    let unbiased: Vec<(f64, f64)> = raw
        .iter()
        .zip(den)
        .map(|((re, _), d)| (re / d, 0.0))
        .collect();
    approx_c(&vec_c(&ev, "u"), &unbiased, 1e-9);
    let scale = (14.0_f64 * 41.0).sqrt();
    let coeff: Vec<(f64, f64)> = raw.iter().map(|(re, _)| (re / scale, 0.0)).collect();
    approx_c(&vec_c(&ev, "c"), &coeff, 1e-9);
    approx_c(&vec_c(&ev, "n"), &coeff, 1e-9);

    // maxlag 0 is the zero lag. maxlag 1 of the autocorrelation is even.
    // maxlag past N−1 pads with zeros. |m| = N makes the unbiased divisor 0.
    let ev = eval(
        "z = xcorr([1, 2, 3], 0);
         a = xcorr([1, 2, 3], 1);
         w = xcorr([1, 2, 3], 4);
         u = xcorr([1, 2, 3], 3, \"unbiased\");
         s = xcorr(3);
         p = xcorr(1+j, 1-j);",
    );
    close(re(&ev, "z"), 14.0);
    assert_eq!(vec_re(&ev, "a"), vec![8.0, 14.0, 8.0]);
    assert_eq!(
        vec_re(&ev, "w"),
        vec![0.0, 0.0, 3.0, 8.0, 14.0, 8.0, 3.0, 0.0, 0.0]
    );
    let u = vec_re(&ev, "u");
    assert!(u[0].is_nan() && u[6].is_nan(), "ends {u:?}");
    close(u[1], 3.0);
    close(u[3], 14.0 / 3.0);
    close(re(&ev, "s"), 9.0);
    // A scalar is a length-1 sequence, so one lag comes back as a vector.
    let p = vec_c(&ev, "p");
    assert_eq!(p.len(), 1);
    assert_c(p[0], Complex64::new(0.0, 2.0), 1e-12);
    // xcov removes the mean first. [1, 2, 3] → [−1, 0, 1], lags −2..2.
    assert_eq!(
        vec_re(&eval("v = xcov([1, 2, 3]);"), "v"),
        vec![-1.0, 0.0, 2.0, 0.0, -1.0]
    );
    // A column's lags are a column. A row matrix stays one row.
    let ev = eval("[r, lags] = xcorr([1; 2; 3], 1);");
    assert_eq!(matrix_dim(&ev, "r"), (3, 1));
    assert_eq!(matrix_dim(&ev, "lags"), (3, 1));
    assert_eq!(vec_re(&ev, "lags"), vec![-1.0, 0.0, 1.0]);
    check_err(&[
        ("r = xcorr([]);", "empty"),
        ("r = xcorr([1, 2], -1);", "non-negative"),
        ("r = xcorr([1, 2], 1.5);", "integer"),
        ("r = xcorr([1, 2], Inf);", "integer"),
        ("r = xcorr([1, 2], \"nope\");", "scale"),
        ("r = xcorr([1, 2; 3, 4], [1; 2]);", "one signal"),
    ]);
}

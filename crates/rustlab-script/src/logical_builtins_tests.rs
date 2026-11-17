//! Builtins that take a logical array.
//!
//! MATLAB treats a logical as 0/1 doubles in numeric functions, keeps the
//! logical class through shape functions (`reshape`, `repmat`, `fliplr`,
//! `sort`, `min` / `max`, `diag`, `cat`), and accepts it in plots.

use crate::eval::value::Value;
use crate::testutil::eval;

/// `m < 0` is `[0 1; 0 0]`. `v` is a 1-D vector comparison, `r` a `1×3`
/// matrix comparison, `c` a column.
const SETUP: &str = "m = [1, -1; 0, 2]; L = m < 0; v = [1, -2, 3] > 0; \
                     r = (zeros(1, 3) + [1, -2, 3]) > 0; c = [1; -2; 3] > 0;\n";

fn get(src: &str, name: &str) -> Value {
    eval(&format!("{SETUP}{src}"))
        .get(name)
        .unwrap_or_else(|| panic!("{name} missing"))
        .clone()
}

/// `(rows, cols, real parts)` of a numeric value, column-major. A vector is a row.
fn numeric(v: &Value, what: &str) -> (usize, usize, Vec<f64>) {
    match v {
        Value::Scalar(x) => (1, 1, vec![*x]),
        Value::Bool(b) => (1, 1, vec![f64::from(u8::from(*b))]),
        Value::Vector(x) => (1, x.len(), x.iter().map(|z| z.re).collect()),
        Value::Matrix(m) => {
            let (r, c) = m.dim();
            let mut out = Vec::with_capacity(r * c);
            for j in 0..c {
                for i in 0..r {
                    out.push(m[[i, j]].re);
                }
            }
            (r, c, out)
        }
        Value::Logical(a) => {
            let mut out = Vec::with_capacity(a.len());
            for j in 0..a.cols() {
                for i in 0..a.rows() {
                    out.push(f64::from(u8::from(a.get(i, j).unwrap())));
                }
            }
            (a.rows(), a.cols(), out)
        }
        other => panic!("{what}: not numeric: {other:?}"),
    }
}

fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x.is_nan() && y.is_nan()) || (x - y).abs() < 1e-12)
}

/// Each `f(X)` with `X` a logical must equal `f(double(X))`: same shape,
/// same values. Before the fix every one of these errored.
#[test]
fn numeric_builtins_treat_a_logical_as_zero_one_doubles() {
    let cases = [
        "mean(L)",
        "mean(v)",
        "mean(L, 2)",
        "median(L)",
        "median(v)",
        "std(L)",
        "std(v)",
        "var(L)",
        "var(v)",
        "prod(L)",
        "prod(c)",
        "cumsum(L)",
        "cumsum(v)",
        "abs(L)",
        "sqrt(L)",
        "round(L)",
        "floor(L)",
        "ceil(L)",
        "exp(L)",
        "real(L)",
        "imag(L)",
        "norm(v)",
        "norm(L)",
        "diff(v)",
        "diff(L)",
        "trapz(v)",
        "cumtrapz(v)",
        "nnz(L)",
        "mod(L, 2)",
        "kron(L, L)",
        "dot(v, v)",
        "mean(mean(L))",
        "max(L, 0.5)",
        "min(v, 0.5)",
        "inv(L + eye(2))",
        "trace(L)",
        "convolve(v, v)",
    ];
    for case in cases {
        let arg = ["L", "v", "c"].into_iter().fold(case.to_string(), |s, n| {
            s.replace(&format!("({n}"), &format!("(double({n})"))
                .replace(&format!(", {n})"), &format!(", double({n}))"))
                .replace(&format!(", {n},"), &format!(", double({n}),"))
                .replace(&format!("({n} "), &format!("(double({n}) "))
        });
        let got = get(&format!("a = {case};"), "a");
        let want = get(&format!("a = {arg};"), "a");
        let (gr, gc, gv) = numeric(&got, case);
        let (wr, wc, wv) = numeric(&want, &arg);
        assert_eq!((gr, gc), (wr, wc), "{case} vs {arg}: shape");
        assert!(same(&gv, &wv), "{case} = {gv:?}, {arg} = {wv:?}");
    }
}

#[test]
fn reported_cases_from_rustlab_llm() {
    let a = get("a = mean(m < 0);", "a");
    assert_eq!(numeric(&a, "mean"), (1, 2, vec![0.0, 0.5]));
    eval(&format!("{SETUP}heatmap(m < 0);"));
    eval(&format!("{SETUP}imagesc(m < 0);"));
}

#[test]
fn reductions_return_double_values() {
    for (src, want) in [
        ("a = mean(v);", vec![2.0 / 3.0]),
        ("a = median(L);", vec![0.0, 0.5]),
        ("a = prod(v);", vec![0.0]),
        ("a = cumsum(v);", vec![1.0, 1.0, 2.0]),
        ("a = var(v);", vec![1.0 / 3.0]),
    ] {
        let a = get(src, "a");
        assert!(
            !matches!(a, Value::Logical(_) | Value::Bool(_)),
            "{src}: {a:?}"
        );
        assert!(same(&numeric(&a, src).2, &want), "{src}: {a:?}");
    }
}

fn assert_logical(src: &str, rows: usize, cols: usize, col_major: &[f64]) {
    match get(src, "a") {
        Value::Logical(a) => {
            assert_eq!((a.rows(), a.cols()), (rows, cols), "{src}");
            let (_, _, vals) = numeric(&Value::Logical(a), src);
            assert_eq!(vals, col_major, "{src}");
        }
        other => panic!("{src}: expected logical, got {other:?}"),
    }
}

/// Shape functions keep the logical class, as in MATLAB.
#[test]
fn shape_functions_keep_the_logical_class() {
    assert_logical("a = reshape(L, 1, 4);", 1, 4, &[0.0, 0.0, 1.0, 0.0]);
    assert_logical("a = reshape(m > 0, 1, 4);", 1, 4, &[1.0, 0.0, 0.0, 1.0]);
    assert_logical("a = reshape(v, 3, 1);", 3, 1, &[1.0, 0.0, 1.0]);
    assert_logical(
        "a = repmat(L, 1, 2);",
        2,
        4,
        &[0., 0., 1., 0., 0., 0., 1., 0.],
    );
    assert_logical("a = repmat(v, 2, 1);", 2, 3, &[1., 1., 0., 0., 1., 1.]);
    assert_logical("a = fliplr(L);", 2, 2, &[1.0, 0.0, 0.0, 0.0]);
    assert_logical("a = fliplr(v);", 1, 3, &[1.0, 0.0, 1.0]);
    assert_logical("a = sort(v);", 1, 3, &[0.0, 1.0, 1.0]);
    assert_logical("a = diag(v);", 3, 3, &[1., 0., 0., 0., 0., 0., 0., 0., 1.]);
    assert_logical("a = max(L);", 1, 2, &[0.0, 1.0]);
    assert_logical("a = min(L, [], 2);", 2, 1, &[0.0, 0.0]);
    assert_logical("a = cat(2, L, L);", 2, 4, &[0., 0., 1., 0., 0., 0., 1., 0.]);
    assert_logical("a = horzcat(v, v);", 1, 6, &[1., 0., 1., 1., 0., 1.]);
    assert_logical(
        "a = vertcat(L, L);",
        4,
        2,
        &[0., 0., 0., 0., 1., 0., 1., 0.],
    );
    assert_logical("a = squeeze(L);", 2, 2, &[0.0, 0.0, 1.0, 0.0]);
    assert_logical("a = L';", 2, 2, &[0.0, 1.0, 0.0, 0.0]);
    assert_logical("a = transpose(L);", 2, 2, &[0.0, 1.0, 0.0, 0.0]);
    assert!(matches!(get("a = max(v);", "a"), Value::Bool(true)));
    let ev = eval(&format!("{SETUP}[s, i] = sort(v); [mx, k] = max(L);"));
    assert!(matches!(ev.get("s"), Some(Value::Logical(_))));
    assert!(matches!(ev.get("mx"), Some(Value::Logical(_))));
    assert_eq!(numeric(ev.get("i").unwrap(), "i").2, vec![2.0, 1.0, 3.0]);
    assert_eq!(numeric(ev.get("k").unwrap(), "k").2, vec![1.0, 1.0]);
}

/// A logical mixed with a double operand gives a double, as in MATLAB.
#[test]
fn mixed_logical_and_double_shape_calls_give_double() {
    for src in [
        "a = horzcat(L, m);",
        "a = cat(1, L, m);",
        "a = max(L, 0.5);",
    ] {
        let a = get(src, "a");
        assert!(matches!(a, Value::Matrix(_)), "{src}: {a:?}");
    }
    match get("a = cat(3, L, L);", "a") {
        Value::Tensor3(t) => assert_eq!(t.dim(), (2, 2, 2)),
        other => panic!("cat(3): {other:?}"),
    }
}

#[test]
fn size_queries_accept_a_logical() {
    assert_eq!(numeric(&get("a = size(L);", "a"), "size").2, vec![2.0, 2.0]);
    assert_eq!(numeric(&get("a = numel(v);", "a"), "numel").2, vec![3.0]);
    assert_eq!(numeric(&get("a = length(c);", "a"), "length").2, vec![3.0]);
    assert_eq!(numeric(&get("a = ndims(L);", "a"), "ndims").2, vec![2.0]);
}

/// `double` of a logical has the shape `double` of the same numeric array
/// has: a `1×n` logical gives a `1×n` matrix, not a 1-D vector.
#[test]
fn double_of_a_logical_keeps_its_shape() {
    for (src, rows, cols) in [
        ("a = double(v);", 1, 3),
        ("a = double(r);", 1, 3),
        ("a = double(c);", 3, 1),
        ("a = double(L);", 2, 2),
        ("a = double(reshape(L, 1, 4));", 1, 4),
    ] {
        match get(src, "a") {
            Value::Matrix(mm) => assert_eq!(mm.dim(), (rows, cols), "{src}"),
            other => panic!("{src}: expected a {rows}×{cols} matrix, got {other:?}"),
        }
    }
    assert!(matches!(get("a = double(L(1, 2));", "a"), Value::Scalar(x) if x == 1.0));
    let want = get("a = double(zeros(1, 3) + [1, 0, 1]);", "a");
    let got = get("a = double(r);", "a");
    assert_eq!(numeric(&got, "double(r)"), numeric(&want, "double(row)"));
    assert_eq!(
        std::mem::discriminant(&got),
        std::mem::discriminant(&want),
        "double of a 1×n logical and of a 1×n double differ: {got:?} vs {want:?}"
    );
}

#[test]
fn plots_accept_a_logical() {
    for src in [
        "heatmap(m < 0);",
        "imagesc(m < 0);",
        "plot(v);",
        "plot(1:3, v);",
        "stem(v);",
        "bar(v);",
        "surf(double(m) .* (m > 0) + L);",
        "surf(L);",
        "contour(L);",
    ] {
        eval(&format!("{SETUP}{src}"));
    }
}

/// Bool and logical builtins keep seeing the raw logical.
#[test]
fn logical_aware_builtins_are_unchanged() {
    assert!(matches!(get("a = islogical(L);", "a"), Value::Bool(true)));
    assert!(matches!(get("a = class(v);", "a"), Value::Str(s) if s == "logical"));
    assert!(matches!(get("a = and(L, L);", "a"), Value::Logical(_)));
    assert!(matches!(get("a = logical(L);", "a"), Value::Logical(_)));
    assert!(matches!(
        get("a = feval(\"islogical\", L);", "a"),
        Value::Bool(true)
    ));
    assert_logical("a = any(L);", 1, 2, &[0.0, 1.0]);
    assert_logical("a = all(L);", 1, 2, &[0.0, 0.0]);
    assert_logical("a = any(m);", 1, 2, &[1.0, 1.0]);
    assert_logical("a = all(m);", 1, 2, &[0.0, 1.0]);
    assert!(matches!(get("a = all(v);", "a"), Value::Bool(false)));
    assert_eq!(numeric(&get("a = sum(L);", "a"), "sum").2, vec![0.0, 1.0]);
    assert_eq!(numeric(&get("a = find(L);", "a"), "find").2, vec![3.0]);
}

/// A scalar `true` / `false` is 1 / 0 in a numeric builtin, as in MATLAB.
#[test]
fn numeric_builtins_accept_a_scalar_bool() {
    for (src, want) in [
        ("a = mean(true);", 1.0),
        ("a = abs(true);", 1.0),
        ("a = sqrt(false);", 0.0),
        ("a = exp(false);", 1.0),
        ("a = prod([true, true]);", 1.0),
    ] {
        let a = get(src, "a");
        assert!(matches!(a, Value::Scalar(x) if x == want), "{src}: {a:?}");
    }
    assert!(matches!(
        get("a = max(true, false);", "a"),
        Value::Bool(true)
    ));
    assert!(matches!(get("a = sprintf(\"%d\", 1 == 1);", "a"), Value::Str(s) if s == "1"));
}

/// Arithmetic: a logical row next to a 1-D vector is a vector, so
/// `x .* (x > 0)` keeps the type of `x`. Otherwise it is a `1×n` matrix and
/// broadcasts against a column.
#[test]
fn arithmetic_shape_of_a_logical_row() {
    let ev = eval("x = [1, -2, 3]; y = x .* (x > 0);");
    assert!(matches!(ev.get("y"), Some(Value::Vector(_))));
    assert_eq!(crate::testutil::vec_re(&ev, "y"), vec![1.0, 0.0, 3.0]);
    match get("a = [1; 2; 3] + (r + 0);", "a") {
        Value::Matrix(mm) => assert_eq!(mm.dim(), (3, 3)),
        other => panic!("{other:?}"),
    }
    match get("a = r + 0;", "a") {
        Value::Matrix(mm) => assert_eq!(mm.dim(), (1, 3)),
        other => panic!("{other:?}"),
    }
}

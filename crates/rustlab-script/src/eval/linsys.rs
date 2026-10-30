//! `A\b`, `B/A`, and the dense factorizations behind `qr` and `lu`.
//!
//! A square system uses LU with partial pivoting, including a zero pivot
//! (`Inf` when the residual is nonzero). A rectangular system uses
//! column-pivoted Householder QR and a basic solution (free variables are 0).
//! A vector on the right is a column. An explicit one-row matrix stays a row.

#[cfg(test)]
use std::cell::Cell;

use ndarray::Array2;
use rustlab_core::factor::{chol, lu, qr};
use rustlab_core::C64;

use super::shared::Shared;
use super::value::Value;
use crate::ast::BinOp;
use crate::error::ScriptError;

const ILL_RCOND: f64 = 1e-12;
/// Rectangular sparse `\` densifies only up to this many entries.
const RECT_SPARSE_CELLS: u64 = 2_000_000;

#[cfg(test)]
thread_local! {
    static SINGULAR_WARNS: Cell<u32> = const { Cell::new(0) };
}

/// How many singular warnings the current thread has printed since the
/// last take. Tests use this; the text still goes to stderr.
#[cfg(test)]
pub(crate) fn take_singular_warns() -> u32 {
    SINGULAR_WARNS.with(|c| c.replace(0))
}

pub(crate) fn mldivide(a: Value, b: Value) -> Result<Value, String> {
    if let Some(s) = scalar_like(&a) {
        if !s.re.is_finite() || !s.im.is_finite() {
            return Err("mldivide: entries must be finite".to_string());
        }
        if s.norm() == 0.0 {
            warn_singular();
        }
        let denom = if s.im == 0.0 {
            Value::Scalar(s.re)
        } else {
            Value::Complex(s)
        };
        return Value::binop(BinOp::Div, b, denom);
    }
    if let Value::SparseMatrix(sm) = &a {
        if sm.rows == sm.cols {
            return super::builtins::sparse_square_mldivide(sm, &b);
        }
        let cells = (sm.rows as u64).saturating_mul(sm.cols as u64);
        if cells > RECT_SPARSE_CELLS {
            return Err(format!(
                "mldivide: rectangular sparse A is {}×{} ({} entries); the dense limit is {RECT_SPARSE_CELLS} entries",
                sm.rows, sm.cols, cells
            ));
        }
    }
    let a_real = value_is_real(&a);
    let b_real = value_is_real(&b);
    let b_cols = rhs_columns(&b)?;
    let a = value_to_matrix(a, "mldivide")?;
    solve_ab(&a, &b_cols, a_real && b_real)
}

/// `B/A`, the solution of `X * A = B`. A scalar denominator stays
/// element-wise and does not come through here. A scalar numerator does.
/// The reduction is `(A' \ B')'`: conjugate transpose, not `.'`.
pub(crate) fn mrdivide(b: Value, a: Value) -> Result<Value, String> {
    let bt = b.transpose()?;
    let at = a.transpose()?;
    let x = mldivide(at, bt)?;
    x.transpose()
}

fn solve_ab(a: &Array2<C64>, b: &Array2<C64>, real: bool) -> Result<Value, String> {
    let (m, n) = a.dim();
    let (mb, _) = b.dim();
    if m != mb {
        return Err(format!(
            "mldivide: A is {m}×{n} but b has {mb} rows\n  hint: a vector is a column; an explicit row has one row"
        ));
    }
    if !entries_finite(a) || !entries_finite(b) {
        return Err("mldivide: entries must be finite".to_string());
    }
    if m == 0 || n == 0 {
        return Ok(pack(&Array2::zeros((n, b.ncols())), real));
    }
    if m == n {
        if let Some(upper) = triangle_kind(a) {
            let (x, singular) = triangular_solve(a, b, upper);
            if singular {
                warn_singular();
            }
            return Ok(pack(&x, real));
        }
        if is_hermitian(a) {
            if let Ok(f) = chol::upper(a) {
                if f.p == 0 {
                    return Ok(pack(&chol::solve_upper(&f.factor, b), real));
                }
            }
        }
        let f = lu::lu(a);
        if f.singular {
            warn_singular();
        } else if f.rcond < ILL_RCOND {
            warn_rcond(f.rcond);
        }
        return Ok(pack(&lu::solve(&f, b), real));
    }
    let f = qr::qr(a, true);
    let (x, rank, rcond) = qr::least_squares(&f, b);
    let full = rank >= m.min(n);
    if !full {
        // A square singular system already warned above.
        if m != n {
            warn_singular();
        }
    } else if rcond < ILL_RCOND {
        warn_rcond(rcond);
    }
    Ok(pack(&x, real))
}

fn warn_singular() {
    #[cfg(test)]
    SINGULAR_WARNS.with(|c| c.set(c.get().saturating_add(1)));
    eprintln!("warning: matrix is singular to working precision");
}

/// `Some(true)` when every entry below the diagonal is zero (upper,
/// including diagonal). `Some(false)` when every entry above the diagonal
/// is zero. A matrix that is both is reported as upper.
fn triangle_kind(a: &Array2<C64>) -> Option<bool> {
    let n = a.nrows();
    let mut upper = true;
    let mut lower = true;
    for i in 0..n {
        for j in 0..n {
            if a[[i, j]].norm() == 0.0 {
                continue;
            }
            if i > j {
                upper = false;
            }
            if i < j {
                lower = false;
            }
            if !upper && !lower {
                return None;
            }
        }
    }
    if upper {
        Some(true)
    } else if lower {
        Some(false)
    } else {
        None
    }
}

fn is_hermitian(a: &Array2<C64>) -> bool {
    let n = a.nrows();
    for i in 0..n {
        if a[[i, i]].im != 0.0 {
            return false;
        }
        for j in (i + 1)..n {
            let z = a[[i, j]];
            let w = a[[j, i]];
            if z.re != w.re || z.im != -w.im {
                return false;
            }
        }
    }
    true
}

/// Forward substitution when `upper` is false, back substitution when it
/// is true. A zero diagonal sets that unknown to `Inf` or `NaN` and reports
/// that a pivot vanished. A structural zero does not multiply `Inf` into `NaN`.
fn triangular_solve(a: &Array2<C64>, b: &Array2<C64>, upper: bool) -> (Array2<C64>, bool) {
    let n = a.nrows();
    let p = b.ncols();
    let mut x = Array2::<C64>::zeros((n, p));
    let mut singular = false;
    let idxs: Vec<usize> = if upper { (0..n).rev().collect() } else { (0..n).collect() };
    for col in 0..p {
        for &i in &idxs {
            let mut s = b[[i, col]];
            let js: Vec<usize> = if upper { ((i + 1)..n).collect() } else { (0..i).collect() };
            for j in js {
                let u = a[[i, j]];
                if u.norm() == 0.0 {
                    continue;
                }
                s -= u * x[[j, col]];
            }
            let diag = a[[i, i]];
            if diag.norm() == 0.0 || !diag.re.is_finite() || !diag.im.is_finite() {
                singular = true;
                x[[i, col]] = if s.im == 0.0 {
                    if s.re == 0.0 {
                        C64::new(f64::NAN, 0.0)
                    } else {
                        C64::new(s.re.signum() * f64::INFINITY, 0.0)
                    }
                } else {
                    s / diag
                };
            } else {
                x[[i, col]] = s / diag;
            }
        }
    }
    (x, singular)
}

fn warn_rcond(rcond: f64) {
    eprintln!("warning: matrix is close to singular or badly scaled (rcond = {rcond:.3e})");
}

fn entries_finite(a: &Array2<C64>) -> bool {
    a.iter().all(|z| z.re.is_finite() && z.im.is_finite())
}

fn scalar_like(v: &Value) -> Option<C64> {
    match v {
        Value::Scalar(x) => Some(C64::new(*x, 0.0)),
        Value::Complex(z) => Some(*z),
        Value::Bool(b) => Some(C64::new(if *b { 1.0 } else { 0.0 }, 0.0)),
        Value::Vector(a) if a.len() == 1 => Some(a[0]),
        Value::Matrix(m) if m.nrows() == 1 && m.ncols() == 1 => Some(m[[0, 0]]),
        _ => None,
    }
}

fn value_is_real(v: &Value) -> bool {
    match v {
        Value::Scalar(_) | Value::Bool(_) => true,
        Value::Complex(z) => z.im == 0.0,
        Value::Vector(a) => Shared::is_real(a) || a.iter().all(|z| z.im == 0.0),
        Value::Matrix(m) => Shared::is_real(m) || m.iter().all(|z| z.im == 0.0),
        Value::SparseVector(sv) => sv.entries.iter().all(|(_, z)| z.im == 0.0),
        Value::SparseMatrix(sm) => sm.entries.iter().all(|(_, _, z)| z.im == 0.0),
        _ => false,
    }
}

/// Right-hand side of `\`. A vector is an `n×1` column read from its
/// storage. A real vector is copied once into that column and is not widened.
/// An explicit matrix keeps its shape, so a `1×n` row stays one row.
fn rhs_columns(b: &Value) -> Result<Array2<C64>, String> {
    match b {
        Value::Vector(v) => Ok(vector_as_column(v)),
        other => value_to_matrix(other.clone(), "mldivide"),
    }
}

fn vector_as_column(v: &Shared<ndarray::Array1<C64>>) -> Array2<C64> {
    if let Some(re) = Shared::real_slice(v) {
        let mut col = Array2::zeros((re.len(), 1));
        for (i, &x) in re.iter().enumerate() {
            col[[i, 0]] = C64::new(x, 0.0);
        }
        return col;
    }
    let n = Shared::len(v);
    Array2::from_shape_fn((n, 1), |(i, _)| v[i])
}

fn value_to_matrix(v: Value, name: &str) -> Result<Array2<C64>, String> {
    match v {
        Value::Matrix(m) => Ok((*m).clone()),
        Value::Vector(v) => {
            let n = v.len();
            Ok(Array2::from_shape_fn((1, n), |(_, j)| v[j]))
        }
        Value::Scalar(x) => Ok(Array2::from_elem((1, 1), C64::new(x, 0.0))),
        Value::Complex(z) => Ok(Array2::from_elem((1, 1), z)),
        Value::Bool(b) => Ok(Array2::from_elem((1, 1), C64::new(if b { 1.0 } else { 0.0 }, 0.0))),
        Value::SparseMatrix(sm) => Ok(sm.to_dense()),
        Value::SparseVector(sv) => {
            let d = sv.to_dense();
            let n = d.len();
            Ok(Array2::from_shape_fn((n, 1), |(i, _)| d[i]))
        }
        other => Err(format!(
            "{name}: expected a numeric array, got {}",
            other.type_name()
        )),
    }
}

fn pack(x: &Array2<C64>, real: bool) -> Value {
    let (m, n) = x.dim();
    if m == 1 && n == 1 {
        let z = x[[0, 0]];
        if real || z.im.abs() <= 1e-12 * (1.0 + z.re.abs()) {
            return Value::Scalar(z.re);
        }
        return Value::Complex(z);
    }
    if m == 1 {
        if real {
            return Value::real_vector(ndarray::Array1::from_iter(x.iter().map(|z| z.re)));
        }
        return Value::Vector(ndarray::Array1::from_iter(x.iter().copied()).into());
    }
    if real {
        return Value::real_matrix(Array2::from_shape_fn((m, n), |(i, j)| x[[i, j]].re));
    }
    Value::Matrix(x.clone().into())
}

fn as_matrix(v: &Value, name: &str) -> Result<Array2<C64>, ScriptError> {
    value_to_matrix(v.clone(), name).map_err(ScriptError::type_err)
}

pub(crate) fn builtin_qr(args: Vec<Value>, nargout: usize) -> Result<Value, ScriptError> {
    if args.is_empty() || args.len() > 2 {
        return Err(ScriptError::type_err(format!(
            "qr: expected 1 or 2 arguments, got {}",
            args.len()
        )));
    }
    if nargout > 3 {
        return Err(ScriptError::type_err(
            "qr: returns at most [Q, R, p]".to_string(),
        ));
    }
    let opt = if args.len() == 2 { Some(qr_option(&args[1])?) } else { None };
    // "vector" and "matrix" only choose the third output. One and two
    // outputs stay the unpivoted factor; those flags would be ignored.
    if nargout < 3 && matches!(opt, Some(QrOption::Vector | QrOption::Matrix)) {
        return Err(ScriptError::type_err(
            "qr: \"vector\" and \"matrix\" select the third output; use [Q, R, p] = qr(A) or qr(A, \"matrix\")".to_string(),
        ));
    }
    let economy = matches!(opt, Some(QrOption::Economy));
    let a = as_matrix(&args[0], "qr")?;
    if !entries_finite(&a) {
        return Err(ScriptError::type_err("qr: entries must be finite".to_string()));
    }
    let real = value_is_real(&args[0]);
    // Pivot only when the caller asks for the permutation. Two outputs
    // stay `qr(a, false)`, the same unpivoted factor as before.
    let f = qr::qr(&a, nargout == 3);
    let (q, r) = qr::explicit(&f, economy);
    if nargout <= 1 {
        return Ok(pack(&r, real));
    }
    if nargout == 2 {
        return Ok(Value::Tuple(vec![pack(&q, real), pack(&r, real)]));
    }
    let third = if matches!(opt, Some(QrOption::Matrix)) {
        column_permutation_matrix(&f.piv)
    } else {
        permutation_vector(&f.piv)
    };
    Ok(Value::Tuple(vec![pack(&q, real), pack(&r, real), third]))
}

enum QrOption {
    Economy,
    Vector,
    Matrix,
}

fn qr_option(v: &Value) -> Result<QrOption, ScriptError> {
    match v {
        Value::Scalar(x) if *x == 0.0 => Ok(QrOption::Economy),
        Value::Str(s) => match s.to_ascii_lowercase().as_str() {
            "econ" | "economy" | "0" => Ok(QrOption::Economy),
            "vector" => Ok(QrOption::Vector),
            "matrix" => Ok(QrOption::Matrix),
            other => Err(ScriptError::type_err(format!(
                "qr: second argument must be 0, \"econ\", \"vector\", or \"matrix\", got \"{other}\""
            ))),
        },
        other => Err(ScriptError::type_err(format!(
            "qr: second argument must be 0, \"econ\", \"vector\", or \"matrix\", got {}",
            other.type_name()
        ))),
    }
}

/// 1-based column order. `p(j)` is the original column now at position `j`,
/// so `A(:, p) = Q R`. A length-1 result stays a vector, not a scalar.
fn permutation_vector(piv: &[usize]) -> Value {
    Value::real_vector(ndarray::Array1::from_iter(piv.iter().map(|&j| (j + 1) as f64)))
}

/// `A * P = Q R`. Column `j` of `P` has a one in row `piv[j]`.
fn column_permutation_matrix(piv: &[usize]) -> Value {
    let n = piv.len();
    let mut p = ndarray::Array2::<f64>::zeros((n, n));
    for (j, &src) in piv.iter().enumerate() {
        p[[src, j]] = 1.0;
    }
    Value::real_matrix(p)
}

/// Dense `[L, U] = lu(A)` or `[L, U, P] = lu(A)`. `P*A = L*U`.
/// Two outputs absorb the permutation into `L`, so `L*U = A`.
pub(crate) fn builtin_lu_dense(args: &[Value], nargout: usize) -> Result<Value, ScriptError> {
    if args.is_empty() || args.len() > 2 {
        return Err(ScriptError::type_err(format!(
            "lu: expected 1 or 2 arguments, got {}",
            args.len()
        )));
    }
    let vector_perm = if args.len() == 2 {
        match &args[1] {
            Value::Str(s) if s.eq_ignore_ascii_case("vector") => true,
            Value::Str(s) => {
                return Err(ScriptError::type_err(format!(
                    "lu: dense second argument must be \"vector\", got \"{s}\". Ordering is only for a sparse factor"
                )));
            }
            other => {
                return Err(ScriptError::type_err(format!(
                    "lu: dense second argument must be \"vector\", got {}",
                    other.type_name()
                )));
            }
        }
    } else {
        false
    };
    if vector_perm && nargout != 3 {
        return Err(ScriptError::type_err(
            "lu: \"vector\" selects the third output; use [L, U, p] = lu(A, \"vector\")".to_string(),
        ));
    }
    if nargout <= 1 {
        return Err(ScriptError::type_err(
            "lu: a dense matrix returns [L, U] or [L, U, P]. A factor handle is only for a sparse matrix; use lu(sparse(A)) for that".to_string(),
        ));
    }
    if nargout > 3 {
        return Err(ScriptError::type_err(
            "lu: returns at most [L, U, P]".to_string(),
        ));
    }
    let a = as_matrix(&args[0], "lu")?;
    if !entries_finite(&a) {
        return Err(ScriptError::type_err("lu: entries must be finite".to_string()));
    }
    let real = value_is_real(&args[0]);
    let f = lu::lu(&a);
    let l = if nargout == 2 {
        permuted_l(&f)
    } else {
        f.l.clone()
    };
    let u = pack(&f.u, real);
    let l = pack(&l, real);
    if nargout == 2 {
        return Ok(Value::Tuple(vec![l, u]));
    }
    let p = if vector_perm {
        one_based_perm(&f.perm)
    } else {
        pack(&lu::permutation_matrix(&f.perm), true)
    };
    Ok(Value::Tuple(vec![l, u, p]))
}

fn one_based_perm(perm: &[usize]) -> Value {
    Value::real_vector(ndarray::Array1::from_iter(perm.iter().map(|&i| (i + 1) as f64)))
}

/// Dense `R = chol(A)`, `[R, p] = chol(A)`, and `"lower"`.
pub(crate) fn builtin_chol_dense(args: &[Value], nargout: usize) -> Result<Value, ScriptError> {
    if args.is_empty() || args.len() > 2 {
        return Err(ScriptError::type_err(format!(
            "chol: expected 1 or 2 arguments, got {}",
            args.len()
        )));
    }
    if nargout > 2 {
        return Err(ScriptError::type_err(
            "chol: returns at most [R, p]".to_string(),
        ));
    }
    let lower = if args.len() == 2 {
        match &args[1] {
            Value::Str(s) if s.eq_ignore_ascii_case("lower") => true,
            Value::Str(s) if s.eq_ignore_ascii_case("upper") => false,
            Value::Str(s) => {
                return Err(ScriptError::type_err(format!(
                    "chol: dense second argument must be \"lower\" or \"upper\", got \"{s}\""
                )));
            }
            other => {
                return Err(ScriptError::type_err(format!(
                    "chol: dense second argument must be \"lower\" or \"upper\", got {}",
                    other.type_name()
                )));
            }
        }
    } else {
        false
    };
    let a = as_matrix(&args[0], "chol")?;
    if !entries_finite(&a) {
        return Err(ScriptError::type_err("chol: entries must be finite".to_string()));
    }
    let real = value_is_real(&args[0]);
    let f = if lower { chol::lower(&a) } else { chol::upper(&a) };
    let f = f.map_err(ScriptError::type_err)?;
    if nargout <= 1 && f.p != 0 {
        return Err(ScriptError::type_err(format!(
            "chol: matrix is not positive definite (failed at pivot {})",
            f.p
        )));
    }
    let factor = pack(&f.factor, real && f.factor.iter().all(|z| z.im == 0.0));
    if nargout <= 1 {
        return Ok(factor);
    }
    Ok(Value::Tuple(vec![factor, Value::Scalar(f.p as f64)]))
}

/// `L2[perm[i], :] = L[i, :]`, so `L2 * U = A`.
fn permuted_l(f: &lu::Lu) -> Array2<C64> {
    let (m, k) = f.l.dim();
    let mut l2 = Array2::zeros((m, k));
    for i in 0..m {
        let dest = f.perm[i];
        for j in 0..k {
            l2[[dest, j]] = f.l[[i, j]];
        }
    }
    l2
}

pub(crate) fn builtin_polyfit(args: Vec<Value>, nargout: usize) -> Result<Value, ScriptError> {
    if args.len() != 3 {
        return Err(ScriptError::type_err(format!(
            "polyfit: expected (x, y, n), got {} arguments",
            args.len()
        )));
    }
    if nargout > 3 {
        return Err(ScriptError::type_err(
            "polyfit: returns at most [p, S, mu]".to_string(),
        ));
    }
    let n = args[2].to_usize().map_err(ScriptError::type_err)?;
    let x_raw = real_samples(&args[0], "polyfit x")?;
    let y = series_matrix(x_raw.len(), &args[1], "polyfit")?;
    let m = x_raw.len();
    let cols = n + 1;
    if (m as u64).saturating_mul(cols as u64) > 100_000_000 {
        return Err(ScriptError::type_err(format!(
            "polyfit: Vandermonde would have {} entries; the limit is 100000000",
            (m as u64).saturating_mul(cols as u64)
        )));
    }
    // Centering only when mu is requested. One- and two-output fits use x as given.
    let (x, mu) = if nargout >= 3 {
        let (mean, std) = mean_std(&x_raw)?;
        let scaled: Vec<f64> = x_raw.iter().map(|t| (t - mean) / std).collect();
        (scaled, Some((mean, std)))
    } else {
        (x_raw, None)
    };
    let mut v = Array2::zeros((m, cols));
    for i in 0..m {
        for j in 0..cols {
            let power = (n - j) as i32;
            v[[i, j]] = ipow_c(x[i], power);
        }
    }
    for z in y.iter() {
        if !z.re.is_finite() || !z.im.is_finite() {
            return Err(ScriptError::type_err(
                "polyfit: y must be finite".to_string(),
            ));
        }
    }
    let real = y.iter().all(|z| z.im == 0.0);
    let f = qr::qr(&v, true);
    let (coeff, rank, rcond) = qr::least_squares(&f, &y);
    if rank < m.min(cols) {
        warn_singular();
    } else if rcond < ILL_RCOND {
        warn_rcond(rcond);
    }
    // One row of coefficients per column of y. A single series is a row vector.
    let rows = coeff.ncols();
    let flipped = Array2::from_shape_fn((rows, coeff.nrows()), |(i, j)| coeff[[j, i]]);
    let packed = pack(&flipped, real);
    let p = if y.ncols() == 1 { as_row(packed) } else { packed };
    if nargout <= 1 {
        return Ok(p);
    }
    let s = fit_struct(&v, &y, &coeff, &f, m, cols);
    if nargout == 2 {
        return Ok(Value::Tuple(vec![p, s]));
    }
    let (mean, std) = mu.expect("three outputs center x");
    let mu_v = Value::real_vector(ndarray::Array1::from_vec(vec![mean, std]));
    Ok(Value::Tuple(vec![p, s, mu_v]))
}

fn mean_std(x: &[f64]) -> Result<(f64, f64), ScriptError> {
    if x.len() < 2 {
        return Err(ScriptError::type_err(
            "polyfit: centering needs at least two x values".to_string(),
        ));
    }
    let mean = x.iter().sum::<f64>() / x.len() as f64;
    let mut acc = 0.0;
    for t in x {
        let d = t - mean;
        acc += d * d;
    }
    let std = (acc / (x.len() - 1) as f64).sqrt();
    if !(std > 0.0) || !std.is_finite() {
        return Err(ScriptError::type_err(
            "polyfit: x has zero standard deviation".to_string(),
        ));
    }
    Ok((mean, std))
}

/// `S.R` is the triangular factor of this fit's QR. `S.df` is
/// `length(x) - (degree + 1)`. `S.normr` is the residual norm of each column.
fn fit_struct(
    v: &Array2<C64>,
    y: &Array2<C64>,
    coeff: &Array2<C64>,
    f: &qr::Qr,
    m: usize,
    ncoeff: usize,
) -> Value {
    let r = qr::r_factor(f);
    let real_r = r.iter().all(|z| z.im == 0.0);
    let mut norms = Vec::with_capacity(y.ncols());
    for s in 0..y.ncols() {
        let mut ss = 0.0;
        for i in 0..y.nrows() {
            let mut pred = C64::new(0.0, 0.0);
            for j in 0..v.ncols() {
                pred += v[[i, j]] * coeff[[j, s]];
            }
            ss += (y[[i, s]] - pred).norm_sqr();
        }
        norms.push(ss.sqrt());
    }
    let normr = if norms.len() == 1 {
        Value::Scalar(norms[0])
    } else {
        Value::real_vector(ndarray::Array1::from_vec(norms))
    };
    let mut fields = std::collections::HashMap::new();
    fields.insert("R".to_string(), pack(&r, real_r));
    fields.insert("df".to_string(), Value::Scalar(m as f64 - ncoeff as f64));
    fields.insert("normr".to_string(), normr);
    Value::Struct(fields)
}

pub(crate) fn builtin_polyval(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() == 4 {
        if !matches!(&args[2], Value::Struct(_)) {
            return Err(ScriptError::type_err(
                "polyval: the third argument must be the struct from polyfit".to_string(),
            ));
        }
        let (mean, std) = mu_pair(&args[3])?;
        let coeffs = coeff_row(&args[0])?;
        let scaled = scale_by_mu(&args[1], mean, std)?;
        return horner(&coeffs, &scaled);
    }
    if args.len() != 2 {
        return Err(ScriptError::type_err(format!(
            "polyval: expected (p, x) or (p, x, S, mu), got {} arguments",
            args.len()
        )));
    }
    let coeffs = coeff_row(&args[0])?;
    horner(&coeffs, &args[1])
}

fn mu_pair(v: &Value) -> Result<(f64, f64), ScriptError> {
    let xs: Vec<f64> = match v {
        Value::Vector(a) if a.len() == 2 => a.iter().map(|z| z.re).collect(),
        Value::Matrix(m) if m.len() == 2 => m.iter().map(|z| z.re).collect(),
        _ => {
            return Err(ScriptError::type_err(
                "polyval: mu must be [mean, std]".to_string(),
            ))
        }
    };
    if !xs[0].is_finite() || !(xs[1] > 0.0) || !xs[1].is_finite() {
        return Err(ScriptError::type_err(
            "polyval: mu must be a finite mean and a positive standard deviation".to_string(),
        ));
    }
    Ok((xs[0], xs[1]))
}

fn scale_by_mu(x: &Value, mean: f64, std: f64) -> Result<Value, ScriptError> {
    let scale = |z: C64| -> Result<C64, ScriptError> {
        if z.im.abs() > 1e-12 {
            return Err(ScriptError::type_err(
                "polyval: x must be real when mu is given".to_string(),
            ));
        }
        Ok(C64::new((z.re - mean) / std, 0.0))
    };
    match x {
        Value::Scalar(s) => Ok(Value::Scalar(scale(C64::new(*s, 0.0))?.re)),
        Value::Complex(z) => Ok(Value::Scalar(scale(*z)?.re)),
        Value::Vector(v) => {
            let mut out = Vec::with_capacity(v.len());
            for z in v.iter() {
                out.push(scale(*z)?.re);
            }
            Ok(Value::real_vector(ndarray::Array1::from_vec(out)))
        }
        Value::Matrix(m) => {
            let mut data = Vec::with_capacity(m.len());
            for z in m.iter() {
                data.push(scale(*z)?.re);
            }
            Ok(Value::real_matrix(
                Array2::from_shape_vec(m.dim(), data).expect("same shape"),
            ))
        }
        other => Err(ScriptError::type_err(format!(
            "polyval: x must be numeric, got {}",
            other.type_name()
        ))),
    }
}

fn as_row(v: Value) -> Value {
    match v {
        Value::Matrix(m) if m.nrows() == 1 => {
            Value::Vector(ndarray::Array1::from_iter(m.iter().copied()).into())
        }
        other => other,
    }
}

fn coeff_row(v: &Value) -> Result<Vec<C64>, ScriptError> {
    let row = match v {
        Value::Vector(a) => a.iter().copied().collect(),
        Value::Matrix(m) if m.nrows() == 1 || m.ncols() == 1 => m.iter().copied().collect(),
        Value::Scalar(x) => vec![C64::new(*x, 0.0)],
        Value::Complex(z) => vec![*z],
        other => {
            return Err(ScriptError::type_err(format!(
                "polyval: p must be a coefficient vector, got {}",
                other.type_name()
            )))
        }
    };
    if row.is_empty() {
        return Err(ScriptError::type_err("polyval: p is empty".to_string()));
    }
    if row.iter().any(|z| !z.re.is_finite() || !z.im.is_finite()) {
        return Err(ScriptError::type_err(
            "polyval: coefficients must be finite".to_string(),
        ));
    }
    Ok(row)
}

fn horner(coeffs: &[C64], x: &Value) -> Result<Value, ScriptError> {
    let eval_one = |z: C64| -> Result<C64, ScriptError> {
        if !z.re.is_finite() || !z.im.is_finite() {
            return Err(ScriptError::type_err(
                "polyval: x must be finite".to_string(),
            ));
        }
        let mut acc = coeffs[0];
        for c in coeffs.iter().skip(1) {
            acc = acc * z + *c;
        }
        Ok(acc)
    };
    match x {
        Value::Scalar(s) => {
            let z = eval_one(C64::new(*s, 0.0))?;
            Ok(pack_scalar(z))
        }
        Value::Complex(z) => Ok(pack_scalar(eval_one(*z)?)),
        Value::Vector(v) => {
            let mut out = Vec::with_capacity(v.len());
            for z in v.iter() {
                out.push(eval_one(*z)?);
            }
            let real = out.iter().all(|z| z.im == 0.0);
            if real {
                Ok(Value::real_vector(ndarray::Array1::from_iter(
                    out.into_iter().map(|z| z.re),
                )))
            } else {
                Ok(Value::Vector(ndarray::Array1::from_vec(out).into()))
            }
        }
        Value::Matrix(m) => {
            let mut data = Vec::with_capacity(m.len());
            for z in m.iter() {
                data.push(eval_one(*z)?);
            }
            let real = data.iter().all(|z| z.im == 0.0);
            let (rows, cols) = m.dim();
            if real {
                Ok(Value::real_matrix(
                    Array2::from_shape_vec((rows, cols), data.into_iter().map(|z| z.re).collect())
                        .expect("same shape"),
                ))
            } else {
                Ok(Value::Matrix(
                    Array2::from_shape_vec((rows, cols), data)
                        .expect("same shape")
                        .into(),
                ))
            }
        }
        other => Err(ScriptError::type_err(format!(
            "polyval: x must be numeric, got {}",
            other.type_name()
        ))),
    }
}

fn pack_scalar(z: C64) -> Value {
    if z.im.abs() <= 1e-12 * (1.0 + z.re.abs()) {
        Value::Scalar(z.re)
    } else {
        Value::Complex(z)
    }
}

fn ipow_c(base: f64, exp: i32) -> C64 {
    C64::new(base.powi(exp), 0.0)
}

/// Columns of `y` are the series. A vector, a row, or a single column is one series.
pub(super) fn series_matrix(n: usize, y: &Value, name: &str) -> Result<Array2<C64>, ScriptError> {
    match y {
        Value::Vector(v) => {
            if v.len() != n {
                return Err(ScriptError::type_err(format!(
                    "{name}: x and y must have the same length ({n} vs {})",
                    v.len()
                )));
            }
            Ok(Array2::from_shape_fn((n, 1), |(i, _)| v[i]))
        }
        Value::Matrix(m) => {
            if m.nrows() == n {
                Ok((**m).clone())
            } else if m.ncols() == n && m.nrows() == 1 {
                Ok(Array2::from_shape_fn((n, 1), |(i, _)| m[[0, i]]))
            } else {
                Err(ScriptError::type_err(format!(
                    "{name}: y has shape {}×{}, expected {} rows (one per sample) or a vector of length {n}",
                    m.nrows(),
                    m.ncols(),
                    n
                )))
            }
        }
        Value::Scalar(s) if n == 1 => Ok(Array2::from_elem((1, 1), C64::new(*s, 0.0))),
        Value::Complex(z) if n == 1 => Ok(Array2::from_elem((1, 1), *z)),
        other => Err(ScriptError::type_err(format!(
            "{name}: y must be a vector or matrix, got {}",
            other.type_name()
        ))),
    }
}

pub(super) fn real_samples(v: &Value, name: &str) -> Result<Vec<f64>, ScriptError> {
    let vals: Vec<C64> = match v {
        Value::Vector(a) => a.iter().copied().collect(),
        Value::Matrix(m) if m.nrows() == 1 || m.ncols() == 1 => m.iter().copied().collect(),
        Value::Scalar(x) => vec![C64::new(*x, 0.0)],
        other => {
            return Err(ScriptError::type_err(format!(
                "{name} must be a real vector, got {}",
                other.type_name()
            )))
        }
    };
    if vals.is_empty() {
        return Err(ScriptError::type_err(format!("{name} is empty")));
    }
    let mut out = Vec::with_capacity(vals.len());
    for z in vals {
        if z.im.abs() > 1e-12 || !z.re.is_finite() {
            return Err(ScriptError::type_err(format!(
                "{name} must be real and finite"
            )));
        }
        out.push(z.re);
    }
    Ok(out)
}

pub(crate) fn builtin_null(args: Vec<Value>) -> Result<Value, ScriptError> {
    null_or_orth(&args, true)
}

pub(crate) fn builtin_orth(args: Vec<Value>) -> Result<Value, ScriptError> {
    null_or_orth(&args, false)
}

fn null_or_orth(args: &[Value], nullspace: bool) -> Result<Value, ScriptError> {
    let name = if nullspace { "null" } else { "orth" };
    if args.len() != 1 {
        return Err(ScriptError::type_err(format!(
            "{name}: expected 1 argument, got {}",
            args.len()
        )));
    }
    let a = as_matrix(&args[0], name)?;
    let (m, n) = a.dim();
    let (u, sigma, v) = super::builtins::svd_full(&a)?;
    let smax = sigma.first().copied().unwrap_or(0.0);
    let tol = (m.max(n) as f64) * f64::EPSILON * smax;
    let real = value_is_real(&args[0]);
    if nullspace {
        let cols: Vec<usize> = (0..n.min(sigma.len()))
            .filter(|&i| sigma[i] <= tol)
            .collect();
        // Columns of V past the singular values are also in the nullspace
        // when n > m (they were completed as an orthonormal basis).
        let mut idx = cols;
        if n > sigma.len() {
            idx.extend(sigma.len()..n);
        }
        Ok(pack(&take_columns(&v, &idx), real))
    } else {
        let cols: Vec<usize> = (0..m.min(sigma.len()))
            .filter(|&i| sigma[i] > tol)
            .collect();
        Ok(pack(&take_columns(&u, &cols), real))
    }
}

fn take_columns(a: &Array2<C64>, cols: &[usize]) -> Array2<C64> {
    let m = a.nrows();
    Array2::from_shape_fn((m, cols.len()), |(i, j)| a[[i, cols[j]]])
}

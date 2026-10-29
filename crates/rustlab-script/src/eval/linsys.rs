//! `A\b`, `B/A`, and the dense factorizations behind `qr` and `lu`.
//!
//! A square nonsingular system uses LU with partial pivoting. A rectangular
//! system, and a square system whose pivot is numerically zero, uses
//! column-pivoted Householder QR. A singular or ill-conditioned matrix
//! prints a warning and still returns a basic solution (free variables are 0).

use ndarray::Array2;
use rustlab_core::factor::{lu, qr};
use rustlab_core::C64;

use super::value::Value;
use crate::ast::BinOp;
use crate::error::ScriptError;

const ILL_RCOND: f64 = 1e-12;

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
    let a_real = value_is_real(&a);
    let b_real = value_is_real(&b);
    let a = value_to_matrix(a, "mldivide")?;
    let b = value_to_matrix(b, "mldivide")?;
    solve_ab(&a, &b, a_real && b_real)
}

/// `B/A`, the solution of `X * A = B`. Scalar `/` stays element-wise and
/// does not come through here.
pub(crate) fn mrdivide(b: Value, a: Value) -> Result<Value, String> {
    let bt = b.non_conj_transpose()?;
    let at = a.non_conj_transpose()?;
    let x = mldivide(at, bt)?;
    x.non_conj_transpose()
}

fn solve_ab(a: &Array2<C64>, b: &Array2<C64>, real: bool) -> Result<Value, String> {
    let (m, n) = a.dim();
    let (mb, _) = b.dim();
    if m != mb {
        return Err(format!(
            "mldivide: A is {m}×{n} but b has {mb} rows\n  hint: a vector is a row; pass a column as b'"
        ));
    }
    if !entries_finite(a) || !entries_finite(b) {
        return Err("mldivide: entries must be finite".to_string());
    }
    if m == 0 || n == 0 {
        return Ok(pack(&Array2::zeros((n, b.ncols())), real));
    }
    if m == n {
        let f = lu::lu(a);
        if !f.singular {
            if f.rcond < ILL_RCOND {
                warn_rcond(f.rcond);
            }
            return Ok(pack(&lu::solve(&f, b), real));
        }
        warn_singular();
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
    eprintln!("warning: matrix is singular to working precision");
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
        Value::Vector(a) => a.iter().all(|z| z.im == 0.0),
        Value::Matrix(m) => m.iter().all(|z| z.im == 0.0),
        Value::SparseVector(sv) => sv.to_dense().iter().all(|z| z.im == 0.0),
        Value::SparseMatrix(sm) => sm.to_dense().iter().all(|z| z.im == 0.0),
        _ => false,
    }
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
    if args.len() != 1 {
        return Err(ScriptError::type_err(
            "lu: a dense matrix takes one argument; ordering is only for a sparse factor".to_string(),
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
    let p = pack(&lu::permutation_matrix(&f.perm), true);
    Ok(Value::Tuple(vec![l, u, p]))
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

pub(crate) fn builtin_polyfit(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() != 3 {
        return Err(ScriptError::type_err(format!(
            "polyfit: expected (x, y, n), got {} arguments",
            args.len()
        )));
    }
    let n = args[2].to_usize().map_err(ScriptError::type_err)?;
    let x = real_samples(&args[0], "polyfit x")?;
    let y = series_matrix(x.len(), &args[1], "polyfit")?;
    let m = x.len();
    let cols = n + 1;
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
    let cols = coeff.nrows();
    let flipped = Array2::from_shape_fn((rows, cols), |(i, j)| coeff[[j, i]]);
    let packed = pack(&flipped, real);
    if y.ncols() == 1 {
        Ok(as_row(packed))
    } else {
        Ok(packed)
    }
}

pub(crate) fn builtin_polyval(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() != 2 {
        return Err(ScriptError::type_err(format!(
            "polyval: expected (p, x), got {} arguments",
            args.len()
        )));
    }
    let coeffs = coeff_row(&args[0])?;
    horner(&coeffs, &args[1])
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

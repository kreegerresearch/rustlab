//! `interp1(x, y, xi, method)`.
//!
//! `x` is strictly monotonic. Outside `[min(x), max(x)]` the result is NaN
//! unless the call passes `"extrap"` or a numeric fill. `spline` is the
//! not-a-knot cubic. `pchip` is Fritsch–Carlson. `makima` is modified Akima.

use ndarray::Array2;
use rustlab_core::C64;

use super::linsys::{real_samples, series_matrix};
use super::value::Value;
use crate::error::ScriptError;

#[derive(Clone, Copy, PartialEq)]
enum Method {
    Linear,
    Nearest,
    Previous,
    Next,
    Spline,
    Pchip,
    Makima,
}

#[derive(Clone, Copy)]
enum Outside {
    Nan,
    Extrap,
    Fill(f64),
}

pub(crate) fn builtin_interp1(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() < 3 || args.len() > 5 {
        return Err(ScriptError::type_err(format!(
            "interp1: expected (x, y, xi [, method [, \"extrap\" or fill]]), got {} arguments",
            args.len()
        )));
    }
    let (method, outside) = method_and_outside(&args[3..])?;
    let mut x = real_samples(&args[0], "interp1 x")?;
    if x.len() < 2 {
        return Err(ScriptError::type_err(
            "interp1: x needs at least two samples".to_string(),
        ));
    }
    let mut y = series_matrix(x.len(), &args[1], "interp1")?;
    let decreasing = x[0] > x[x.len() - 1];
    if decreasing {
        x.reverse();
        y = reverse_rows(&y);
    }
    if !strictly_increasing(&x) {
        return Err(ScriptError::type_err(
            "interp1: x must be strictly monotonic".to_string(),
        ));
    }
    let xi = query_values(&args[2])?;
    let ms = match method {
        Method::Spline => Some(spline_seconds(&x, &y)?),
        Method::Pchip => Some(hermite_slopes(&x, &y, pchip_slopes)?),
        Method::Makima => Some(hermite_slopes(&x, &y, makima_slopes)?),
        _ => None,
    };
    let mut cols = Vec::with_capacity(y.ncols());
    for c in 0..y.ncols() {
        let col: Vec<C64> = (0..y.nrows()).map(|i| y[[i, c]]).collect();
        let m = ms.as_ref().map(|all| all[c].as_slice());
        let mut out = Vec::with_capacity(xi.len());
        for &q in &xi {
            out.push(eval_one(&x, &col, q, method, outside, m));
        }
        cols.push(out);
    }
    pack_queries(&args[2], &cols)
}

fn method_and_outside(rest: &[Value]) -> Result<(Method, Outside), ScriptError> {
    match rest {
        [] => Ok((Method::Linear, Outside::Nan)),
        [a] => {
            if is_extrap(a) {
                Ok((Method::Linear, Outside::Extrap))
            } else if let Some(fill) = real_fill(a) {
                Ok((Method::Linear, Outside::Fill(fill)))
            } else {
                Ok((parse_method(a)?, Outside::Nan))
            }
        }
        [a, b] => {
            if is_extrap(b) {
                Ok((parse_method(a)?, Outside::Extrap))
            } else if let Some(fill) = real_fill(b) {
                Ok((parse_method(a)?, Outside::Fill(fill)))
            } else {
                Err(ScriptError::type_err(
                    "interp1: the fifth argument must be \"extrap\" or a number".to_string(),
                ))
            }
        }
        _ => Err(ScriptError::type_err(
            "interp1: too many arguments".to_string(),
        )),
    }
}

fn real_fill(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar(x) if x.is_finite() => Some(*x),
        Value::Complex(z) if z.im == 0.0 && z.re.is_finite() => Some(z.re),
        _ => None,
    }
}

fn is_extrap(v: &Value) -> bool {
    matches!(v, Value::Str(s) if s.eq_ignore_ascii_case("extrap"))
}

fn parse_method(v: &Value) -> Result<Method, ScriptError> {
    let Value::Str(s) = v else {
        return Err(ScriptError::type_err(
            "interp1: method must be \"linear\", \"nearest\", \"previous\", \"next\", \"spline\", \"pchip\", or \"makima\""
                .to_string(),
        ));
    };
    match s.to_ascii_lowercase().as_str() {
        "linear" => Ok(Method::Linear),
        "nearest" => Ok(Method::Nearest),
        "previous" => Ok(Method::Previous),
        "next" => Ok(Method::Next),
        "spline" => Ok(Method::Spline),
        "pchip" => Ok(Method::Pchip),
        "makima" => Ok(Method::Makima),
        other => Err(ScriptError::type_err(format!(
            "interp1: unknown method \"{other}\""
        ))),
    }
}

fn strictly_increasing(x: &[f64]) -> bool {
    x.windows(2).all(|w| w[1] > w[0] && w[0].is_finite() && w[1].is_finite())
}

fn reverse_rows(y: &Array2<C64>) -> Array2<C64> {
    let (m, n) = y.dim();
    Array2::from_shape_fn((m, n), |(i, j)| y[[m - 1 - i, j]])
}

fn query_values(v: &Value) -> Result<Vec<f64>, ScriptError> {
    let vals: Vec<C64> = match v {
        Value::Scalar(x) => vec![C64::new(*x, 0.0)],
        Value::Complex(z) => vec![*z],
        Value::Vector(a) => a.iter().copied().collect(),
        Value::Matrix(m) => m.iter().copied().collect(),
        other => {
            return Err(ScriptError::type_err(format!(
                "interp1: xi must be numeric, got {}",
                other.type_name()
            )))
        }
    };
    let mut out = Vec::with_capacity(vals.len());
    for z in vals {
        if z.im.abs() > 1e-12 {
            return Err(ScriptError::type_err(
                "interp1: xi must be real".to_string(),
            ));
        }
        out.push(z.re);
    }
    Ok(out)
}

/// Interval `i` with `x[i] <= xi`, or `None` when `xi` is outside.
fn bracket(x: &[f64], xi: f64) -> Option<usize> {
    if !xi.is_finite() || xi < x[0] || xi > x[x.len() - 1] {
        return None;
    }
    if xi == x[x.len() - 1] {
        return Some(x.len() - 2);
    }
    let mut lo = 0usize;
    let mut hi = x.len() - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if x[mid] <= xi {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(lo)
}

fn eval_one(
    x: &[f64],
    y: &[C64],
    xi: f64,
    method: Method,
    outside: Outside,
    seconds: Option<&[C64]>,
) -> C64 {
    if !xi.is_finite() {
        return C64::new(f64::NAN, f64::NAN);
    }
    let n = x.len();
    let inside = bracket(x, xi);
    if inside.is_none() {
        match outside {
            Outside::Nan => return C64::new(f64::NAN, 0.0),
            Outside::Fill(v) => return C64::new(v, 0.0),
            Outside::Extrap => {}
        }
    }
    match method {
        Method::Nearest => {
            let i = match inside {
                Some(i) => {
                    let d0 = xi - x[i];
                    let d1 = x[i + 1] - xi;
                    // A tie goes to the sample with the larger x.
                    if d0 < d1 { i } else { i + 1 }
                }
                None if xi < x[0] => 0,
                None => n - 1,
            };
            y[i]
        }
        Method::Previous => {
            let i = match inside {
                Some(_) if xi == x[n - 1] => n - 1,
                Some(i) => i,
                None if xi < x[0] => 0,
                None => n - 1,
            };
            y[i]
        }
        Method::Next => {
            let i = match inside {
                Some(i) if (xi - x[i]).abs() <= 1e-15 * (1.0 + x[i].abs()) => i,
                Some(i) => i + 1,
                None if xi < x[0] => 0,
                None => n - 1,
            };
            y[i]
        }
        Method::Linear | Method::Spline => {
            let i = match inside {
                Some(i) => i,
                None if xi < x[0] => 0,
                None => n - 2,
            };
            let h = x[i + 1] - x[i];
            let b = (xi - x[i]) / h;
            let a = 1.0 - b;
            let mut s = y[i] * a + y[i + 1] * b;
            if method == Method::Spline {
                let m = seconds.expect("spline seconds");
                let a3 = a * a * a - a;
                let b3 = b * b * b - b;
                let scale = h * h / 6.0;
                s += (m[i] * a3 + m[i + 1] * b3) * scale;
            }
            s
        }
        Method::Pchip | Method::Makima => {
            let i = match inside {
                Some(i) => i,
                None if xi < x[0] => 0,
                None => n - 2,
            };
            let d = seconds.expect("hermite slopes");
            hermite(x, y, d, xi, i)
        }
    }
}

fn hermite(x: &[f64], y: &[C64], d: &[C64], xi: f64, i: usize) -> C64 {
    let h = x[i + 1] - x[i];
    let t = (xi - x[i]) / h;
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    y[i] * h00 + d[i] * (h * h10) + y[i + 1] * h01 + d[i + 1] * (h * h11)
}

fn hermite_slopes(
    x: &[f64],
    y: &Array2<C64>,
    slopes: fn(&[f64], &[f64]) -> Vec<f64>,
) -> Result<Vec<Vec<C64>>, ScriptError> {
    let mut out = Vec::with_capacity(y.ncols());
    for c in 0..y.ncols() {
        let re: Vec<f64> = (0..y.nrows()).map(|i| y[[i, c]].re).collect();
        let im: Vec<f64> = (0..y.nrows()).map(|i| y[[i, c]].im).collect();
        let sr = slopes(x, &re);
        let si = if im.iter().all(|v| *v == 0.0) {
            vec![0.0; sr.len()]
        } else {
            slopes(x, &im)
        };
        out.push(sr.into_iter().zip(si).map(|(r, i)| C64::new(r, i)).collect());
    }
    Ok(out)
}

/// Fritsch–Carlson slopes. An interior slope is 0 when the adjacent
/// secants change sign. End slopes use the three-point formula, clamped.
fn pchip_slopes(x: &[f64], y: &[f64]) -> Vec<f64> {
    let n = x.len();
    let h: Vec<f64> = (0..n - 1).map(|i| x[i + 1] - x[i]).collect();
    let delta: Vec<f64> = (0..n - 1).map(|i| (y[i + 1] - y[i]) / h[i]).collect();
    if n == 2 {
        return vec![delta[0], delta[0]];
    }
    let mut d = vec![0.0; n];
    for k in 1..n - 1 {
        let d0 = delta[k - 1];
        let d1 = delta[k];
        if d0 == 0.0 || d1 == 0.0 || d0.signum() != d1.signum() {
            d[k] = 0.0;
        } else {
            let w1 = 2.0 * h[k] + h[k - 1];
            let w2 = h[k] + 2.0 * h[k - 1];
            d[k] = (w1 + w2) / (w1 / d0 + w2 / d1);
        }
    }
    d[0] = fritsch_end(h[0], h[1], delta[0], delta[1]);
    d[n - 1] = fritsch_end(h[n - 2], h[n - 3], delta[n - 2], delta[n - 3]);
    d
}

fn fritsch_end(h0: f64, h1: f64, delta0: f64, delta1: f64) -> f64 {
    let d = ((2.0 * h0 + h1) * delta0 - h0 * delta1) / (h0 + h1);
    if d == 0.0 || delta0 == 0.0 || d.signum() != delta0.signum() {
        0.0
    } else if delta0.signum() != delta1.signum() && d.abs() > 3.0 * delta0.abs() {
        3.0 * delta0
    } else {
        d
    }
}

/// Modified Akima slopes. Weights use both the difference and the sum of
/// neighboring secants, so a flat run next to a jump does not follow
/// ordinary Akima.
fn makima_slopes(x: &[f64], y: &[f64]) -> Vec<f64> {
    let n = x.len();
    let h: Vec<f64> = (0..n - 1).map(|i| x[i + 1] - x[i]).collect();
    let delta: Vec<f64> = (0..n - 1).map(|i| (y[i + 1] - y[i]) / h[i]).collect();
    if n == 2 {
        return vec![delta[0], delta[0]];
    }
    let l = delta.len();
    let mut ext = vec![0.0; l + 4];
    for i in 0..l {
        ext[i + 2] = delta[i];
    }
    ext[1] = 2.0 * delta[0] - delta[1];
    ext[0] = 2.0 * ext[1] - delta[0];
    ext[l + 2] = 2.0 * delta[l - 1] - delta[l - 2];
    ext[l + 3] = 2.0 * ext[l + 2] - delta[l - 1];
    let mut d = vec![0.0; n];
    for i in 0..n {
        let dm2 = ext[i];
        let dm1 = ext[i + 1];
        let di = ext[i + 2];
        let dp = ext[i + 3];
        let w1 = (dp - di).abs() + 0.5 * (dp + di).abs();
        let w2 = (dm1 - dm2).abs() + 0.5 * (dm1 + dm2).abs();
        d[i] = if w1 + w2 == 0.0 {
            0.0
        } else {
            (w1 * dm1 + w2 * di) / (w1 + w2)
        };
    }
    d
}

/// Second derivatives at the knots, one vector per column. Real and
/// imaginary parts are independent not-a-knot systems.
fn spline_seconds(x: &[f64], y: &Array2<C64>) -> Result<Vec<Vec<C64>>, ScriptError> {
    let mut out = Vec::with_capacity(y.ncols());
    for c in 0..y.ncols() {
        let re: Vec<f64> = (0..y.nrows()).map(|i| y[[i, c]].re).collect();
        let im: Vec<f64> = (0..y.nrows()).map(|i| y[[i, c]].im).collect();
        let mr = seconds_of(x, &re)?;
        let mi = if im.iter().all(|v| *v == 0.0) {
            vec![0.0; mr.len()]
        } else {
            seconds_of(x, &im)?
        };
        out.push(
            mr.into_iter()
                .zip(mi)
                .map(|(r, i)| C64::new(r, i))
                .collect(),
        );
    }
    Ok(out)
}

fn seconds_of(x: &[f64], y: &[f64]) -> Result<Vec<f64>, ScriptError> {
    let n = x.len();
    if n == 2 {
        return Ok(vec![0.0, 0.0]);
    }
    if n == 3 {
        return Ok(quadratic_m(x, y));
    }
    let h: Vec<f64> = (0..n - 1).map(|i| x[i + 1] - x[i]).collect();
    let mut a = vec![vec![0.0; n]; n];
    let mut rhs = vec![0.0; n];
    // Not-a-knot at the first interior knot.
    a[0][0] = h[1];
    a[0][1] = -(h[0] + h[1]);
    a[0][2] = h[0];
    for i in 1..n - 1 {
        a[i][i - 1] = h[i - 1];
        a[i][i] = 2.0 * (h[i - 1] + h[i]);
        a[i][i + 1] = h[i];
        let d0 = (y[i] - y[i - 1]) / h[i - 1];
        let d1 = (y[i + 1] - y[i]) / h[i];
        rhs[i] = 6.0 * (d1 - d0);
    }
    // Not-a-knot at the last interior knot.
    a[n - 1][n - 3] = h[n - 2];
    a[n - 1][n - 2] = -(h[n - 3] + h[n - 2]);
    a[n - 1][n - 1] = h[n - 3];
    solve_ge(&mut a, &mut rhs)
}

fn quadratic_m(x: &[f64], y: &[f64]) -> Vec<f64> {
    let t1 = x[1] - x[0];
    let t2 = x[2] - x[0];
    let c = ((y[2] - y[0]) / t2 - (y[1] - y[0]) / t1) / (t2 - t1);
    vec![2.0 * c, 2.0 * c, 2.0 * c]
}

fn solve_ge(a: &mut [Vec<f64>], b: &mut [f64]) -> Result<Vec<f64>, ScriptError> {
    let n = b.len();
    for k in 0..n {
        let mut piv = k;
        let mut best = a[k][k].abs();
        for i in (k + 1)..n {
            let v = a[i][k].abs();
            if v > best {
                best = v;
                piv = i;
            }
        }
        if best <= f64::EPSILON * n.max(1) as f64 {
            return Err(ScriptError::type_err(
                "interp1: spline system is singular".to_string(),
            ));
        }
        if piv != k {
            a.swap(k, piv);
            b.swap(k, piv);
        }
        let diag = a[k][k];
        for i in (k + 1)..n {
            let lik = a[i][k] / diag;
            for j in k..n {
                let prod = lik * a[k][j];
                a[i][j] -= prod;
            }
            b[i] -= lik * b[k];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in (i + 1)..n {
            s -= a[i][j] * x[j];
        }
        x[i] = s / a[i][i];
    }
    Ok(x)
}

fn pack_queries(xi_value: &Value, cols: &[Vec<C64>]) -> Result<Value, ScriptError> {
    let p = cols.len();
    let real = cols.iter().all(|c| c.iter().all(|z| z.im == 0.0));
    if p == 1 {
        return Ok(match xi_value {
            Value::Scalar(_) | Value::Complex(_) => {
                let z = cols[0][0];
                if real || z.im == 0.0 {
                    Value::Scalar(z.re)
                } else {
                    Value::Complex(z)
                }
            }
            Value::Vector(_) => row_or_complex(&cols[0], real),
            Value::Matrix(m) => {
                if real {
                    Value::real_matrix(Array2::from_shape_fn(m.dim(), |(i, j)| {
                        cols[0][i * m.ncols() + j].re
                    }))
                } else {
                    Value::Matrix(
                        Array2::from_shape_fn(m.dim(), |(i, j)| cols[0][i * m.ncols() + j]).into(),
                    )
                }
            }
            _ => unreachable!("query_values accepted this"),
        });
    }
    // Several columns of y: one row per query, one column per series.
    let nxi = cols[0].len();
    if real {
        Ok(Value::real_matrix(Array2::from_shape_fn((nxi, p), |(i, j)| {
            cols[j][i].re
        })))
    } else {
        Ok(Value::Matrix(
            Array2::from_shape_fn((nxi, p), |(i, j)| cols[j][i]).into(),
        ))
    }
}

fn row_or_complex(col: &[C64], real: bool) -> Value {
    if real {
        Value::real_vector(ndarray::Array1::from_iter(col.iter().map(|z| z.re)))
    } else {
        Value::Vector(ndarray::Array1::from_vec(col.to_vec()).into())
    }
}

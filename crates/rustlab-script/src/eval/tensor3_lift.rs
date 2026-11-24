//! Element-wise operations on `Tensor3` through the matrix code paths.
//!
//! An `m × n × p` tensor is laid out as the `m × (n·p)` matrix whose column
//! `k·n + j` is page `k`, column `j`. Its column-major order is the tensor's
//! column-major order (`T(:)`), so linear indices and `[row, col] = find`
//! agree with MATLAB, where `col` runs over the trailing dimensions merged.
//! An element-wise builtin runs on that matrix and the result is mapped back
//! to the tensor's shape. There is no rank-3 logical: a logical result comes
//! back as 0/1 doubles.

use super::shared::Shared;
use super::value::Value;
use ndarray::{Array2, Array3};
use rustlab_core::CTensor3;

fn dims(t: &Shared<CTensor3>) -> [usize; 3] {
    let s = Shared::shape(t);
    [s[0], s[1], s[2]]
}

/// The `m × (n·p)` matrix view of a tensor, copied. Real storage stays real.
pub(crate) fn to_columns(t: &Shared<CTensor3>) -> Value {
    let [m, n, p] = dims(t);
    if let Some(r) = Shared::as_real(t) {
        let mut out = Array2::<f64>::zeros((m, n * p));
        for i in 0..m {
            for k in 0..p {
                for j in 0..n {
                    out[[i, k * n + j]] = r[[i, j, k]];
                }
            }
        }
        return Value::real_matrix(out);
    }
    let mut out = Array2::zeros((m, n * p));
    for i in 0..m {
        for k in 0..p {
            for j in 0..n {
                out[[i, k * n + j]] = t[[i, j, k]];
            }
        }
    }
    Value::Matrix(out.into())
}

/// Map a result of the matrix path back to an `m × n × p` tensor.
pub(crate) fn from_columns(name: &str, v: Value, shape: [usize; 3]) -> Result<Value, String> {
    let [m, n, p] = shape;
    let np = n * p;
    let bad = |what: &str| {
        Err(format!(
            "{name}: unexpected {what} result for a {m}x{n}x{p} Tensor3"
        ))
    };
    match v {
        Value::Matrix(mat) => {
            if Shared::shape(&mat) != [m, np] {
                return bad("matrix");
            }
            if let Some(r) = Shared::as_real(&mat) {
                let out = Array3::from_shape_fn((m, n, p), |(i, j, k)| r[[i, k * n + j]]);
                return Ok(Value::real_tensor3(out));
            }
            let out = Array3::from_shape_fn((m, n, p), |(i, j, k)| mat[[i, k * n + j]]);
            Ok(Value::Tensor3(out.into()))
        }
        Value::Logical(a) => {
            if (a.rows(), a.cols()) != (m, np) {
                return bad("logical");
            }
            let out = Array3::from_shape_fn((m, n, p), |(i, j, k)| {
                f64::from(u8::from(a.get(i, k * n + j) == Some(true)))
            });
            Ok(Value::real_tensor3(out))
        }
        Value::Vector(vec) if m == 1 && Shared::len(&vec) == np => {
            if let Some(r) = Shared::real_slice(&vec) {
                let out = Array3::from_shape_fn((1, n, p), |(_, j, k)| r[k * n + j]);
                return Ok(Value::real_tensor3(out));
            }
            let out = Array3::from_shape_fn((1, n, p), |(_, j, k)| vec[k * n + j]);
            Ok(Value::Tensor3(out.into()))
        }
        Value::Scalar(x) if m * np == 1 => Ok(Value::real_tensor3(Array3::from_elem((1, 1, 1), x))),
        Value::Bool(b) if m * np == 1 => Ok(Value::real_tensor3(Array3::from_elem(
            (1, 1, 1),
            f64::from(u8::from(b)),
        ))),
        Value::Complex(c) if m * np == 1 => {
            Ok(Value::Tensor3(Array3::from_elem((1, 1, 1), c).into()))
        }
        other => bad(other.type_name()),
    }
}

/// Run `f` with every `Tensor3` argument replaced by its column matrix, then
/// map the result back. `None` when no argument is a tensor. Tensors must
/// share one shape; other arguments must be scalars (or strings, for option
/// flags). A matrix or vector next to a tensor is an error, as for `+`.
pub(crate) fn lift<E>(
    name: &str,
    args: &[Value],
    wrap: fn(String) -> E,
    f: impl FnOnce(Vec<Value>) -> Result<Value, E>,
) -> Option<Result<Value, E>> {
    let shape = args.iter().find_map(|a| match a {
        Value::Tensor3(t) => Some(dims(t)),
        _ => None,
    })?;
    let run = || -> Result<Value, E> {
        let mut conv = Vec::with_capacity(args.len());
        for a in args {
            match a {
                Value::Tensor3(t) => {
                    let d = dims(t);
                    if d != shape {
                        return Err(wrap(format!(
                            "{name}: Tensor3 shape mismatch: {}x{}x{} vs {}x{}x{}",
                            shape[0], shape[1], shape[2], d[0], d[1], d[2]
                        )));
                    }
                    conv.push(to_columns(t));
                }
                Value::Scalar(_)
                | Value::Complex(_)
                | Value::Bool(_)
                | Value::Int { .. }
                | Value::Str(_) => conv.push(a.clone()),
                other => {
                    return Err(wrap(format!(
                        "{name}: a Tensor3 pairs only with a scalar or a Tensor3 of the same size, got {}",
                        other.type_name()
                    )));
                }
            }
        }
        let out = f(conv)?;
        from_columns(name, out, shape).map_err(wrap)
    };
    Some(run())
}

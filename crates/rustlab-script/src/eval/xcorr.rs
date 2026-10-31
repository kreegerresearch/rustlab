//! `xcorr` and `xcov`. The sums live in `rustlab-dsp`; this module reads
//! script values without widening real storage and packs the lag axis.

use super::shared::Shared;
use super::value::Value;
use crate::error::ScriptError;
use ndarray::{Array1, Array2};
use num_complex::Complex;
use rustlab_core::{CMatrix, CVector, C64};
use rustlab_dsp::xcorr::lag_count;
use rustlab_dsp::{xcorr_c64, xcorr_f64, XcorrMethod, XcorrScale};

pub(crate) fn builtin_xcorr(args: Vec<Value>, nargout: usize) -> Result<Value, ScriptError> {
    correlate("xcorr", args, nargout, false)
}

pub(crate) fn builtin_xcov(args: Vec<Value>, nargout: usize) -> Result<Value, ScriptError> {
    correlate("xcov", args, nargout, true)
}

#[derive(Clone, Copy)]
enum Orient {
    /// A `Value::Vector`, which prints as a row.
    Vector,
    /// An explicit one-row matrix.
    Row,
    /// A column, or a matrix of column signals.
    Column,
}

#[derive(Clone)]
enum Samples {
    Real(Vec<f64>),
    Complex(Vec<C64>),
}

impl Samples {
    fn len(&self) -> usize {
        match self {
            Samples::Real(v) => v.len(),
            Samples::Complex(v) => v.len(),
        }
    }

    fn is_real(&self) -> bool {
        matches!(self, Samples::Real(_))
    }
}

struct Signal {
    columns: Vec<Samples>,
    orient: Orient,
    /// More than one column, each longer than one sample.
    multi: bool,
    rows: usize,
    cols: usize,
}

fn correlate(
    name: &str,
    mut args: Vec<Value>,
    nargout: usize,
    demean: bool,
) -> Result<Value, ScriptError> {
    if nargout > 2 {
        return Err(ScriptError::type_err(format!(
            "{name}: returns at most [r, lags]"
        )));
    }
    if args.is_empty() || args.len() > 4 {
        return Err(ScriptError::type_err(format!(
            "{name}: expected a sequence and optional maxlag and scale"
        )));
    }
    let scale = match args.last() {
        Some(Value::Str(s)) => {
            let scale = parse_scale(name, s)?;
            args.pop();
            scale
        }
        _ => XcorrScale::None,
    };
    let (x, y, maxlag_arg) = match args.len() {
        1 => (signal(name, &args[0])?, None, None),
        2 => {
            if is_maxlag(&args[1]) {
                (signal(name, &args[0])?, None, Some(maxlag(name, &args[1])?))
            } else {
                (signal(name, &args[0])?, Some(signal(name, &args[1])?), None)
            }
        }
        3 => (
            signal(name, &args[0])?,
            Some(signal(name, &args[1])?),
            Some(maxlag(name, &args[2])?),
        ),
        _ => {
            return Err(ScriptError::type_err(format!(
                "{name}: expected a sequence and optional maxlag and scale"
            )))
        }
    };
    if x.multi && scale != XcorrScale::None {
        // MATLAB's column-pair form accepts only the raw sum. "biased",
        // "unbiased", "normalized", and "coeff" are the two-sequence scales.
        return Err(ScriptError::type_err(format!(
            "{name}: scale must be \"none\" for a matrix with several columns"
        )));
    }
    let x_empty = x.columns.is_empty() || x.columns.iter().all(|c| c.len() == 0);
    if x_empty {
        if y.as_ref()
            .is_some_and(|y| y.columns.iter().any(|c| c.len() != 0))
        {
            return Err(ScriptError::type_err(format!("{name}: input is empty")));
        }
        // No samples: the correlation has no lags. `[]` is an empty row.
        // A 0×N matrix is 0×N² (one empty pair per column pair).
        return empty_autocorr(name, &x, nargout);
    }
    if let Some(y) = &y {
        if x.multi || y.multi {
            return Err(ScriptError::type_err(format!(
                "{name}: a second sequence must be one signal; a matrix is one signal per column"
            )));
        }
        if y.columns.is_empty() || y.columns[0].len() == 0 {
            return Err(ScriptError::type_err(format!("{name}: input is empty")));
        }
    }
    let n = match &y {
        Some(y) => x.columns[0].len().max(y.columns[0].len()),
        None => x.columns[0].len(),
    };
    let maxlag = maxlag_arg.unwrap_or(n - 1);
    // `2*maxlag+1` is what used to overflow `Vec` capacity. Reject it
    // before any correlation, including the lag axis collected below.
    let n_lags = lag_count(name, maxlag, rustlab_core::ELEM_F64).map_err(|e| {
        ScriptError::type_err(match e {
            rustlab_core::CoreError::InvalidParameter(msg) => {
                // The math layer says "xcorr"; this call might be xcov.
                if name == "xcorr" {
                    msg
                } else {
                    msg.replacen("xcorr", name, 1)
                }
            }
            other => other.to_string(),
        })
    })?;
    let orient = x.orient_tag();
    let y_real = match &y {
        Some(y) => y.columns.iter().all(Samples::is_real),
        None => true,
    };
    let real = x.columns.iter().all(Samples::is_real) && y_real;

    let r = if let Some(y) = y {
        let mut a = x.columns.into_iter().next().unwrap();
        let mut b = y.columns.into_iter().next().unwrap();
        if demean {
            a = demean_samples(a);
            b = demean_samples(b);
        }
        one_pair(name, a, b, maxlag, scale)?
    } else if x.multi {
        let mut cols = x.columns;
        if demean {
            cols = cols.into_iter().map(demean_samples).collect();
        }
        column_pairs(name, &cols, maxlag, n_lags, scale, real)?
    } else {
        let mut a = x.columns.into_iter().next().unwrap();
        if demean {
            a = demean_samples(a);
        }
        let b = clone_samples(&a);
        one_pair(name, a, b, maxlag, scale)?
    };

    let lags = (0..n_lags)
        .map(|k| k as f64 - maxlag as f64)
        .collect::<Vec<_>>();
    let r = pack(r, orient);
    if nargout == 2 {
        let lags_v = pack_real(&lags, orient);
        Ok(Value::Tuple(vec![r, lags_v]))
    } else {
        Ok(r)
    }
}

impl Signal {
    fn orient_tag(&self) -> Orient {
        self.orient
    }
}

/// Autocorrelation of an input with no samples. The result has no lags.
/// A vector stays an empty row. A 0×N matrix is 0×N². An M×0 matrix is
/// (2M−1)×0, the column-pair width with no columns.
fn empty_autocorr(name: &str, sig: &Signal, nargout: usize) -> Result<Value, ScriptError> {
    let r = match sig.orient {
        Orient::Vector => Value::real_vector(Array1::zeros(0)),
        _ => {
            let (h, w) = empty_matrix_hw(name, sig.rows, sig.cols)?;
            Value::real_matrix(Array2::zeros((h, w)))
        }
    };
    if nargout == 2 {
        let lags = match sig.orient {
            Orient::Vector => Value::real_vector(Array1::zeros(0)),
            Orient::Row => Value::real_matrix(Array2::zeros((1, 0))),
            Orient::Column => {
                let h = if sig.rows == 0 { 0 } else { sig.rows * 2 - 1 };
                Value::real_matrix(Array2::zeros((h, 1)))
            }
        };
        Ok(Value::Tuple(vec![r, lags]))
    } else {
        Ok(r)
    }
}

fn empty_matrix_hw(name: &str, rows: usize, cols: usize) -> Result<(usize, usize), ScriptError> {
    if rows == 0 && cols == 0 {
        return Ok((0, 0));
    }
    if cols == 0 {
        let h = rows
            .checked_mul(2)
            .and_then(|h| h.checked_sub(1))
            .ok_or_else(|| {
                ScriptError::type_err(rustlab_core::variable_too_large(name, &[rows]))
            })?;
        return Ok((h, 0));
    }
    let width = if cols == 1 {
        1
    } else {
        cols.checked_mul(cols).ok_or_else(|| {
            ScriptError::type_err(rustlab_core::variable_too_large(name, &[cols, cols]))
        })?
    };
    // Height is 0, so the element count is 0, but the width still has to
    // be a legal dimension (a 0×N² matrix of a very wide input).
    if width > rustlab_core::MAX_ELEMENTS as usize {
        return Err(ScriptError::type_err(rustlab_core::variable_too_large(
            name,
            &[0, width],
        )));
    }
    Ok((0, width))
}

fn one_pair(
    name: &str,
    x: Samples,
    y: Samples,
    maxlag: usize,
    scale: XcorrScale,
) -> Result<Pair, ScriptError> {
    match (x, y) {
        (Samples::Real(x), Samples::Real(y)) => Ok(Pair::Real(
            xcorr_f64(&x, &y, maxlag, scale, XcorrMethod::Auto).map_err(|e| corr_err(name, e))?,
        )),
        (x, y) => {
            let x = to_c64(x);
            let y = to_c64(y);
            Ok(Pair::Complex(
                xcorr_c64(&x, &y, maxlag, scale, XcorrMethod::Auto)
                    .map_err(|e| corr_err(name, e))?,
            ))
        }
    }
}

fn corr_err(name: &str, err: rustlab_core::CoreError) -> ScriptError {
    let msg = match err {
        rustlab_core::CoreError::InvalidParameter(msg) => {
            if name == "xcorr" {
                msg
            } else {
                msg.replacen("xcorr", name, 1)
            }
        }
        other => other.to_string(),
    };
    ScriptError::type_err(msg)
}

fn column_pairs(
    name: &str,
    cols: &[Samples],
    maxlag: usize,
    rows: usize,
    scale: XcorrScale,
    real: bool,
) -> Result<Pair, ScriptError> {
    let n = cols.len();
    let width = n
        .checked_mul(n)
        .ok_or_else(|| ScriptError::type_err(rustlab_core::variable_too_large(name, &[n, n])))?;
    let elem = if real {
        rustlab_core::ELEM_F64
    } else {
        rustlab_core::ELEM_C64
    };
    rustlab_core::checked_elems(&[rows, width], elem).map_err(|_| {
        ScriptError::type_err(rustlab_core::variable_too_large(name, &[rows, width]))
    })?;
    if real {
        let cols: Vec<&[f64]> = cols
            .iter()
            .map(|c| match c {
                Samples::Real(v) => v.as_slice(),
                Samples::Complex(_) => unreachable!("real flag"),
            })
            .collect();
        let mut data = vec![0.0; rows * n * n];
        for i in 0..n {
            for j in 0..n {
                let col = xcorr_f64(cols[i], cols[j], maxlag, scale, XcorrMethod::Auto)
                    .map_err(|e| corr_err(name, e))?;
                let out_c = i * n + j;
                for (r, v) in col.into_iter().enumerate() {
                    data[r * n * n + out_c] = v;
                }
            }
        }
        Ok(Pair::MatrixReal {
            data,
            rows,
            cols: n * n,
        })
    } else {
        let cols: Vec<Vec<C64>> = cols.iter().cloned().map(to_c64).collect();
        let mut data = vec![Complex::new(0.0, 0.0); rows * n * n];
        for i in 0..n {
            for j in 0..n {
                let col = xcorr_c64(&cols[i], &cols[j], maxlag, scale, XcorrMethod::Auto)
                    .map_err(|e| corr_err(name, e))?;
                let out_c = i * n + j;
                for (r, v) in col.into_iter().enumerate() {
                    data[r * n * n + out_c] = v;
                }
            }
        }
        Ok(Pair::MatrixComplex {
            data,
            rows,
            cols: width,
        })
    }
}

enum Pair {
    Real(Vec<f64>),
    Complex(Vec<C64>),
    MatrixReal {
        data: Vec<f64>,
        rows: usize,
        cols: usize,
    },
    MatrixComplex {
        data: Vec<C64>,
        rows: usize,
        cols: usize,
    },
}

fn pack(pair: Pair, orient: Orient) -> Value {
    match pair {
        Pair::Real(v) => pack_real(&v, orient),
        Pair::Complex(v) => pack_complex(&v, orient),
        Pair::MatrixReal { data, rows, cols } => {
            Value::real_matrix(Array2::from_shape_vec((rows, cols), data).expect("shape"))
        }
        Pair::MatrixComplex { data, rows, cols } => Value::Matrix(
            Array2::from_shape_vec((rows, cols), data)
                .expect("shape")
                .into(),
        ),
    }
}

fn pack_real(v: &[f64], orient: Orient) -> Value {
    match orient {
        Orient::Vector => Value::real_vector(Array1::from_vec(v.to_vec())),
        Orient::Row => {
            Value::real_matrix(Array2::from_shape_vec((1, v.len()), v.to_vec()).expect("row"))
        }
        Orient::Column => {
            Value::real_matrix(Array2::from_shape_vec((v.len(), 1), v.to_vec()).expect("column"))
        }
    }
}

fn pack_complex(v: &[C64], orient: Orient) -> Value {
    match orient {
        Orient::Vector => Value::Vector(Array1::from_vec(v.to_vec()).into()),
        Orient::Row => Value::Matrix(
            Array2::from_shape_vec((1, v.len()), v.to_vec())
                .expect("row")
                .into(),
        ),
        Orient::Column => Value::Matrix(
            Array2::from_shape_vec((v.len(), 1), v.to_vec())
                .expect("column")
                .into(),
        ),
    }
}

fn demean_samples(s: Samples) -> Samples {
    match s {
        Samples::Real(v) => {
            if v.is_empty() {
                return Samples::Real(v);
            }
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            Samples::Real(v.into_iter().map(|x| x - mean).collect())
        }
        Samples::Complex(v) => {
            if v.is_empty() {
                return Samples::Complex(v);
            }
            let mean = v.iter().copied().sum::<C64>() / (v.len() as f64);
            Samples::Complex(v.into_iter().map(|x| x - mean).collect())
        }
    }
}

fn clone_samples(s: &Samples) -> Samples {
    match s {
        Samples::Real(v) => Samples::Real(v.clone()),
        Samples::Complex(v) => Samples::Complex(v.clone()),
    }
}

fn to_c64(s: Samples) -> Vec<C64> {
    match s {
        Samples::Complex(v) => v,
        Samples::Real(v) => v.into_iter().map(|x| Complex::new(x, 0.0)).collect(),
    }
}

fn is_maxlag(v: &Value) -> bool {
    matches!(v, Value::Scalar(_))
}

fn maxlag(name: &str, v: &Value) -> Result<usize, ScriptError> {
    let Value::Scalar(x) = v else {
        return Err(ScriptError::type_err(format!(
            "{name}: maxlag must be a non-negative integer"
        )));
    };
    if *x < 0.0 || !x.is_finite() || x.fract() != 0.0 {
        return Err(ScriptError::type_err(format!(
            "{name}: maxlag must be a non-negative integer"
        )));
    }
    Ok(*x as usize)
}

fn parse_scale(name: &str, s: &str) -> Result<XcorrScale, ScriptError> {
    match s.to_ascii_lowercase().as_str() {
        "none" => Ok(XcorrScale::None),
        "biased" => Ok(XcorrScale::Biased),
        "unbiased" => Ok(XcorrScale::Unbiased),
        "normalized" | "coeff" => Ok(XcorrScale::Coeff),
        other => Err(ScriptError::type_err(format!(
            "{name}: unknown scale \"{other}\" (expected \"none\", \"biased\", \"unbiased\", \"normalized\", or \"coeff\")"
        ))),
    }
}

fn signal(name: &str, v: &Value) -> Result<Signal, ScriptError> {
    match v {
        Value::Scalar(x) => Ok(Signal {
            columns: vec![Samples::Real(vec![*x])],
            orient: Orient::Vector,
            multi: false,
            rows: 1,
            cols: 1,
        }),
        Value::Complex(c) => Ok(Signal {
            columns: vec![Samples::Complex(vec![*c])],
            orient: Orient::Vector,
            multi: false,
            rows: 1,
            cols: 1,
        }),
        Value::Vector(v) => Ok(Signal {
            columns: vec![vector_samples(v)],
            orient: Orient::Vector,
            multi: false,
            rows: 1,
            cols: Shared::len(v),
        }),
        Value::Matrix(m) => matrix_signal(m),
        other => Err(ScriptError::type_err(format!(
            "{name}: expected a vector or matrix, got {}",
            other.type_name()
        ))),
    }
}

fn vector_samples(v: &Shared<CVector>) -> Samples {
    if Shared::is_real(v) {
        let data = if let Some(s) = Shared::real_slice(v) {
            s.to_vec()
        } else {
            Shared::as_real(v).expect("real").iter().copied().collect()
        };
        Samples::Real(data)
    } else {
        let a: &CVector = v;
        Samples::Complex(a.iter().copied().collect())
    }
}

fn matrix_signal(m: &Shared<CMatrix>) -> Result<Signal, ScriptError> {
    let shape = Shared::shape(m);
    let (rows, cols) = (shape[0], shape[1]);
    if rows == 0 || cols == 0 {
        let columns = if cols == 0 {
            Vec::new()
        } else {
            vec![Samples::Real(Vec::new()); cols]
        };
        // A 1×0 matrix is an empty row. Every other empty shape is a
        // column or a column-pair matrix, so the lag axis is a column.
        let orient = if rows == 1 && cols == 0 {
            Orient::Row
        } else {
            Orient::Column
        };
        return Ok(Signal {
            columns,
            orient,
            multi: false,
            rows,
            cols,
        });
    }
    if Shared::is_real(m) {
        let a = Shared::as_real(m).expect("real");
        if rows <= 1 || cols <= 1 {
            let data: Vec<f64> = if rows <= 1 {
                (0..cols).map(|j| a[[0, j]]).collect()
            } else {
                (0..rows).map(|i| a[[i, 0]]).collect()
            };
            let orient = if cols <= 1 {
                Orient::Column
            } else {
                Orient::Row
            };
            return Ok(Signal {
                columns: vec![Samples::Real(data)],
                orient,
                multi: false,
                rows,
                cols,
            });
        }
        let mut columns = Vec::with_capacity(cols);
        for j in 0..cols {
            columns.push(Samples::Real((0..rows).map(|i| a[[i, j]]).collect()));
        }
        Ok(Signal {
            columns,
            orient: Orient::Column,
            multi: true,
            rows,
            cols,
        })
    } else {
        let a: &CMatrix = m;
        if rows <= 1 || cols <= 1 {
            let data: Vec<C64> = if rows <= 1 {
                (0..cols).map(|j| a[[0, j]]).collect()
            } else {
                (0..rows).map(|i| a[[i, 0]]).collect()
            };
            let orient = if cols <= 1 {
                Orient::Column
            } else {
                Orient::Row
            };
            return Ok(Signal {
                columns: vec![Samples::Complex(data)],
                orient,
                multi: false,
                rows,
                cols,
            });
        }
        let mut columns = Vec::with_capacity(cols);
        for j in 0..cols {
            columns.push(Samples::Complex((0..rows).map(|i| a[[i, j]]).collect()));
        }
        Ok(Signal {
            columns,
            orient: Orient::Column,
            multi: true,
            rows,
            cols,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{column_pairs, empty_matrix_hw, Samples};
    use rustlab_dsp::XcorrScale;

    #[test]
    fn wide_empty_pair_width_is_an_error() {
        // 2^40 columns, no stored samples. The pair matrix would be
        // 0 × 2^80, which does not fit in `usize`.
        let err = empty_matrix_hw("xcorr", 0, 1usize << 40).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("maximum possible variable size"), "{msg}");
        assert!(msg.contains("xcorr"), "{msg}");
    }

    #[test]
    fn column_pair_product_over_the_cap_is_an_error() {
        let cols = vec![Samples::Real(vec![1.0]); 4];
        match column_pairs("xcorr", &cols, 0, usize::MAX / 2, XcorrScale::None, true) {
            Err(err) => assert!(
                err.to_string().contains("maximum possible variable size"),
                "{err}"
            ),
            Ok(_) => panic!("column-pair product over the cap should error"),
        }
    }
}

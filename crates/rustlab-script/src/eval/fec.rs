//! `poly2trellis`, `istrellis`, and `vitdec`. The trellis and the decoder
//! live in `rustlab_dsp::viterbi`; this module converts MATLAB's trellis
//! struct and the arguments.

use super::shared::Shared;
use super::value::Value;
use crate::error::ScriptError;
use ndarray::{Array1, Array2};
use rustlab_dsp::error::DspError;
use rustlab_dsp::viterbi::{self, DecType, OpMode, Trellis};
use std::collections::HashMap;

fn err(name: &str, e: DspError) -> ScriptError {
    match e {
        DspError::InvalidParameter(m) => ScriptError::runtime(format!("{name}: {m}")),
        other => ScriptError::runtime(format!("{name}: {other}")),
    }
}

/// Real entries of a scalar, vector, or matrix in MATLAB (column-major)
/// order, plus its (rows, cols).
fn real_entries(
    name: &str,
    what: &str,
    v: &Value,
) -> Result<(Vec<f64>, usize, usize), ScriptError> {
    let not_real = || ScriptError::runtime(format!("{name}: {what} must be real"));
    match v {
        Value::Scalar(x) => Ok((vec![*x], 1, 1)),
        Value::Bool(b) => Ok((vec![f64::from(u8::from(*b))], 1, 1)),
        Value::Vector(a) => {
            let d: Vec<f64> = match Shared::real_slice(a) {
                Some(d) => d.to_vec(),
                None => {
                    if a.iter().any(|z| z.im != 0.0) {
                        return Err(not_real());
                    }
                    a.iter().map(|z| z.re).collect()
                }
            };
            let n = d.len();
            Ok((d, 1, n))
        }
        Value::Matrix(m) => {
            let s = Shared::shape(m);
            let (r, c) = (s[0], s[1]);
            let row_major: Vec<f64> = match Shared::real_slice(m) {
                Some(d) => d.to_vec(),
                None => {
                    if m.iter().any(|z| z.im != 0.0) {
                        return Err(not_real());
                    }
                    m.iter().map(|z| z.re).collect()
                }
            };
            let mut col_major = Vec::with_capacity(r * c);
            for j in 0..c {
                for i in 0..r {
                    col_major.push(row_major[i * c + j]);
                }
            }
            Ok((col_major, r, c))
        }
        other => Err(ScriptError::type_err(format!(
            "{name}: {what} must be numeric, got {}",
            other.type_name()
        ))),
    }
}

fn whole(name: &str, what: &str, x: f64) -> Result<usize, ScriptError> {
    if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x < 1e15 {
        Ok(x as usize)
    } else {
        Err(ScriptError::runtime(format!(
            "{name}: {what} must be a nonnegative integer, got {x}"
        )))
    }
}

fn real_matrix(rows: usize, cols: usize, f: impl Fn(usize, usize) -> f64) -> Value {
    let mut d = Vec::with_capacity(rows * cols);
    for i in 0..rows {
        for j in 0..cols {
            d.push(f(i, j));
        }
    }
    Value::real_matrix(Array2::from_shape_vec((rows, cols), d).expect("shape"))
}

fn trellis_value(t: &Trellis) -> Value {
    let nsym = 1usize << t.k;
    let mut m = HashMap::new();
    m.insert("numInputSymbols".to_string(), Value::Scalar(nsym as f64));
    m.insert(
        "numOutputSymbols".to_string(),
        Value::Scalar((1u64 << t.n) as f64),
    );
    m.insert("numStates".to_string(), Value::Scalar(t.num_states as f64));
    m.insert(
        "nextStates".to_string(),
        real_matrix(t.num_states, nsym, |s, x| t.next[s * nsym + x] as f64),
    );
    m.insert(
        "outputs".to_string(),
        real_matrix(t.num_states, nsym, |s, x| {
            viterbi::octal_digits(t.out[s * nsym + x] as u64)
        }),
    );
    Value::Struct(m)
}

/// `poly2trellis(K, codegen[, feedback])`.
pub(crate) fn builtin_poly2trellis(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "poly2trellis";
    if !(2..=3).contains(&args.len()) {
        return Err(ScriptError::runtime(format!(
            "{NAME}: expected (K, codegen) or (K, codegen, feedback), got {} arguments",
            args.len()
        )));
    }
    let (kv, _, _) = real_entries(NAME, "K", &args[0])?;
    let constraint = kv
        .iter()
        .map(|&x| whole(NAME, "a constraint length", x))
        .collect::<Result<Vec<_>, _>>()?;
    let k = constraint.len();
    let (g, gr, gc) = real_entries(NAME, "codegen", &args[1])?;
    // One input: any vector of generators. Otherwise one row per input.
    let (rows, cols) = if k == 1 { (1, g.len()) } else { (gr, gc) };
    if rows != k {
        return Err(ScriptError::runtime(format!(
            "{NAME}: codegen must be {k}×n for {k} constraint lengths, got {gr}×{gc}"
        )));
    }
    let mut codegen = vec![Vec::with_capacity(cols); rows];
    for j in 0..cols {
        for (i, row) in codegen.iter_mut().enumerate() {
            let x = if k == 1 { g[j] } else { g[j * rows + i] };
            row.push(viterbi::octal_value(x).map_err(|e| err(NAME, e))?);
        }
    }
    let feedback = match args.get(2) {
        None => None,
        Some(f) => {
            let (fv, _, _) = real_entries(NAME, "feedback", f)?;
            Some(
                fv.iter()
                    .map(|&x| viterbi::octal_value(x).map_err(|e| err(NAME, e)))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
    };
    let t = viterbi::poly2trellis(&constraint, &codegen, feedback.as_deref())
        .map_err(|e| err(NAME, e))?;
    Ok(trellis_value(&t))
}

/// A trellis struct as the core type.
fn trellis_of(name: &str, v: &Value) -> Result<Trellis, ScriptError> {
    let bad =
        |why: String| ScriptError::runtime(format!("{name}: not a valid trellis struct: {why}"));
    let Value::Struct(m) = v else {
        return Err(bad(format!("got {}", v.type_name())));
    };
    let field = |f: &str| m.get(f).ok_or_else(|| bad(format!("no field {f}")));
    let count = |f: &str| -> Result<usize, ScriptError> {
        let (x, _, _) = real_entries(name, f, field(f)?)?;
        match x[..] {
            [c] if c >= 1.0 && c.fract() == 0.0 && (c as u64).is_power_of_two() => Ok(c as usize),
            _ => Err(bad(format!("{f} must be a power of 2"))),
        }
    };
    let nin = count("numInputSymbols")?;
    let nout = count("numOutputSymbols")?;
    let states = count("numStates")?;
    if nin > 1 << 16 || nout > 1 << 16 || states > 1 << 24 {
        return Err(bad("too many symbols or states".to_string()));
    }
    let table = |f: &str| -> Result<Vec<usize>, ScriptError> {
        let (x, r, c) = real_entries(name, f, field(f)?)?;
        if (r, c) != (states, nin) {
            return Err(bad(format!("{f} must be {states}×{nin}, got {r}×{c}")));
        }
        // Column-major from `real_entries`; the core wants row-major.
        let mut out = vec![0; states * nin];
        for j in 0..nin {
            for i in 0..states {
                let v = x[j * states + i];
                if !(v >= 0.0 && v.fract() == 0.0 && v < 1e18) {
                    return Err(bad(format!("{f} entries must be nonnegative integers")));
                }
                out[i * nin + j] = v as usize;
            }
        }
        Ok(out)
    };
    let next = table("nextStates")?;
    let out = table("outputs")?
        .into_iter()
        .map(|o| viterbi::octal_value(o as f64).map(|v| v as usize))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| bad("outputs must be written in octal".to_string()))?;
    let t = Trellis {
        k: nin.trailing_zeros() as usize,
        n: nout.trailing_zeros() as usize,
        num_states: states,
        next,
        out,
    };
    t.validate().map_err(|e| bad(e.to_string()))?;
    Ok(t)
}

/// `istrellis(s)`: whether `s` is a valid trellis struct.
pub(crate) fn builtin_istrellis(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() != 1 {
        return Err(ScriptError::runtime(format!(
            "istrellis: expected 1 argument, got {}",
            args.len()
        )));
    }
    Ok(Value::Bool(trellis_of("istrellis", &args[0]).is_ok()))
}

/// `vitdec(code, trellis, tblen, opmode, dectype[, nsdec])`.
pub(crate) fn builtin_vitdec(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "vitdec";
    if !(5..=6).contains(&args.len()) {
        return Err(ScriptError::runtime(format!(
            "{NAME}: expected (code, trellis, tblen, opmode, dectype[, nsdec]), got {} arguments",
            args.len()
        )));
    }
    let (code, rows, cols) = real_entries(NAME, "code", &args[0])?;
    if rows != 1 && cols != 1 {
        return Err(ScriptError::runtime(format!(
            "{NAME}: code must be a vector, got {rows}×{cols}"
        )));
    }
    let column = cols == 1 && rows > 1;
    let t = trellis_of(NAME, &args[1])?;
    let tblen = match args[2] {
        Value::Scalar(x) if x >= 1.0 && x.fract() == 0.0 && x.is_finite() => x as usize,
        ref other => {
            return Err(ScriptError::runtime(format!(
                "{NAME}: tblen must be a positive integer, got {other}"
            )))
        }
    };
    let word = |i: usize, what: &str| -> Result<String, ScriptError> {
        args[i]
            .to_str()
            .map(|s| s.to_ascii_lowercase())
            .map_err(|_| ScriptError::runtime(format!("{NAME}: {what} must be a string")))
    };
    let mode = match word(3, "opmode")?.as_str() {
        "trunc" => OpMode::Trunc,
        "term" => OpMode::Term,
        "cont" => OpMode::Cont,
        other => {
            return Err(ScriptError::runtime(format!(
                "{NAME}: opmode must be \"trunc\", \"term\", or \"cont\", got \"{other}\""
            )))
        }
    };
    let dec = match word(4, "dectype")?.as_str() {
        "hard" => DecType::Hard,
        "unquant" => DecType::Unquant,
        "soft" => {
            let nsdec = match args.get(5) {
                Some(Value::Scalar(x)) if *x >= 1.0 && *x <= 16.0 && x.fract() == 0.0 => *x as u32,
                Some(other) => {
                    return Err(ScriptError::runtime(format!(
                        "{NAME}: nsdec must be an integer from 1 to 16, got {other}"
                    )))
                }
                None => {
                    return Err(ScriptError::runtime(format!(
                        "{NAME}: \"soft\" needs nsdec, the number of soft-decision bits"
                    )))
                }
            };
            DecType::Soft { nsdec }
        }
        other => {
            return Err(ScriptError::runtime(format!(
                "{NAME}: dectype must be \"hard\", \"soft\", or \"unquant\", got \"{other}\""
            )))
        }
    };
    if args.len() == 6 && !matches!(dec, DecType::Soft { .. }) {
        return Err(ScriptError::runtime(format!(
            "{NAME}: nsdec is only used with \"soft\""
        )));
    }
    let bits = viterbi::vitdec(&code, &t, tblen, mode, dec).map_err(|e| err(NAME, e))?;
    let data: Vec<f64> = bits.into_iter().map(f64::from).collect();
    let n = data.len();
    Ok(if column {
        Value::real_matrix(Array2::from_shape_vec((n, 1), data).expect("shape"))
    } else {
        Value::real_vector(Array1::from_vec(data))
    })
}

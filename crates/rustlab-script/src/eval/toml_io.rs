use super::value::Value;
use ndarray::{Array1, Array3};
use num_complex::Complex;
use std::collections::HashMap;
use toml::Value as Toml;

/// Reserved field names used to encode a Tensor3 inside a TOML table.
/// Prefixed with double underscores to avoid collision with user field names.
const TENSOR3_SHAPE_KEY: &str = "__tensor3_shape";
const TENSOR3_DATA_KEY: &str = "__tensor3_data";

// ─── Save ────────────────────────────────────────────────────────────────────

pub fn save_toml(path: &str, value: &Value) -> Result<(), String> {
    let table = match value {
        Value::Struct(_) => value_to_toml(value)?,
        other => {
            return Err(format!(
                "save: TOML requires a struct at the top level, got {}",
                other.type_name()
            ))
        }
    };
    let text = toml::to_string_pretty(&table)
        .map_err(|e| format!("save: TOML serialization failed: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("save: {e}"))
}

// ─── Load ────────────────────────────────────────────────────────────────────

pub fn load_toml(path: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("load: {e}"))?;
    let table: Toml = text
        .parse::<Toml>()
        .map_err(|e| format!("load: TOML parse error: {e}"))?;
    Ok(toml_to_value(table))
}

// ─── Value → toml::Value ─────────────────────────────────────────────────────

fn value_to_toml(v: &Value) -> Result<Toml, String> {
    match v {
        Value::Scalar(f) => {
            // Write as integer if it's a whole number in i64 range
            if f.fract() == 0.0 && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 {
                Ok(Toml::Integer(*f as i64))
            } else {
                Ok(Toml::Float(*f))
            }
        }
        Value::Bool(b) => Ok(Toml::Boolean(*b)),
        Value::Str(s) => Ok(Toml::String(s.clone())),
        Value::Vector(cv) => {
            let mut arr = Vec::with_capacity(cv.len());
            for &c in cv.iter() {
                if c.im.abs() > 1e-15 {
                    return Err("save: cannot serialize complex values to TOML".into());
                }
                arr.push(scalar_to_toml(c.re));
            }
            Ok(Toml::Array(arr))
        }
        Value::Matrix(cm) => {
            let (rows, cols) = cm.dim();
            let mut arr = Vec::with_capacity(rows);
            for r in 0..rows {
                let mut row = Vec::with_capacity(cols);
                for c in 0..cols {
                    let v = cm[[r, c]];
                    if v.im.abs() > 1e-15 {
                        return Err("save: cannot serialize complex values to TOML".into());
                    }
                    row.push(scalar_to_toml(v.re));
                }
                arr.push(Toml::Array(row));
            }
            Ok(Toml::Array(arr))
        }
        Value::Tensor3(t) => {
            // Encode as a TOML table with flat column-major data plus shape metadata.
            // On load, the table is recognised by its two reserved keys and
            // reconstructed as a Tensor3.
            let s = t.shape();
            let (m, n, p) = (s[0], s[1], s[2]);
            let mut flat = Vec::with_capacity(m * n * p);
            for k in 0..p {
                for j in 0..n {
                    for i in 0..m {
                        let v = t[[i, j, k]];
                        if v.im.abs() > 1e-15 {
                            return Err(
                                "save: cannot serialize complex tensor3 values to TOML".into(),
                            );
                        }
                        flat.push(scalar_to_toml(v.re));
                    }
                }
            }
            let mut table = toml::map::Map::new();
            table.insert(
                TENSOR3_SHAPE_KEY.to_string(),
                Toml::Array(vec![
                    Toml::Integer(m as i64),
                    Toml::Integer(n as i64),
                    Toml::Integer(p as i64),
                ]),
            );
            table.insert(TENSOR3_DATA_KEY.to_string(), Toml::Array(flat));
            Ok(Toml::Table(table))
        }
        Value::Struct(fields) => {
            let mut table = toml::map::Map::new();
            // Sort keys for deterministic output
            let mut keys: Vec<&String> = fields.keys().collect();
            keys.sort();
            for key in keys {
                let val = &fields[key];
                table.insert(key.clone(), value_to_toml(val)?);
            }
            Ok(Toml::Table(table))
        }
        Value::Tuple(items) => {
            let mut arr = Vec::with_capacity(items.len());
            for item in items {
                arr.push(value_to_toml(item)?);
            }
            Ok(Toml::Array(arr))
        }
        other => Err(format!(
            "save: cannot serialize {} to TOML",
            other.type_name()
        )),
    }
}

fn scalar_to_toml(f: f64) -> Toml {
    if f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
        Toml::Integer(f as i64)
    } else {
        Toml::Float(f)
    }
}

// ─── toml::Value → Value ─────────────────────────────────────────────────────

fn toml_to_value(tv: Toml) -> Value {
    match tv {
        Toml::Table(mut map) => {
            // Tensor3 encoding: if the table has our reserved keys, rebuild the tensor.
            if map.contains_key(TENSOR3_SHAPE_KEY) && map.contains_key(TENSOR3_DATA_KEY) {
                if let Some(t) = tensor3_from_table(&mut map) {
                    return t;
                }
            }
            let fields: HashMap<String, Value> = map
                .into_iter()
                .map(|(k, v)| (k, toml_to_value(v)))
                .collect();
            Value::Struct(fields)
        }
        Toml::String(s) => Value::Str(s),
        Toml::Integer(i) => Value::Scalar(i as f64),
        Toml::Float(f) => Value::Scalar(f),
        Toml::Boolean(b) => Value::Bool(b),
        Toml::Datetime(dt) => Value::Str(dt.to_string()),
        Toml::Array(arr) => array_to_value(arr),
    }
}

/// Attempt to reconstruct a Tensor3 from a table that carries our reserved keys.
/// Returns `Some` on success; returns `None` if the payload is malformed so the
/// caller falls back to treating it as a plain struct.
fn tensor3_from_table(map: &mut toml::map::Map<String, Toml>) -> Option<Value> {
    let shape = match map.remove(TENSOR3_SHAPE_KEY)? {
        Toml::Array(arr) if arr.len() == 3 => arr,
        _ => return None,
    };
    let data = match map.remove(TENSOR3_DATA_KEY)? {
        Toml::Array(arr) => arr,
        _ => return None,
    };
    let m = toml_num_as_usize(&shape[0])?;
    let n = toml_num_as_usize(&shape[1])?;
    let p = toml_num_as_usize(&shape[2])?;
    if data.len() != m * n * p {
        return None;
    }
    let flat: Vec<Complex<f64>> = data
        .iter()
        .map(|v| match v {
            Toml::Integer(i) => Some(Complex::new(*i as f64, 0.0)),
            Toml::Float(f) => Some(Complex::new(*f, 0.0)),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    // Fill column-major (matches the save walk)
    let mut t = Array3::<Complex<f64>>::zeros((m, n, p));
    for k in 0..p {
        for j in 0..n {
            for i in 0..m {
                t[[i, j, k]] = flat[k * m * n + j * m + i];
            }
        }
    }
    Some(Value::Tensor3(t))
}

fn toml_num_as_usize(v: &Toml) -> Option<usize> {
    match v {
        Toml::Integer(i) if *i >= 0 => Some(*i as usize),
        Toml::Float(f) if *f >= 0.0 && f.fract() == 0.0 => Some(*f as usize),
        _ => None,
    }
}

fn array_to_value(arr: Vec<Toml>) -> Value {
    if arr.is_empty() {
        return Value::Vector(Array1::zeros(0));
    }

    // Check if all elements are numeric (Integer or Float)
    let all_numeric = arr
        .iter()
        .all(|v| matches!(v, Toml::Integer(_) | Toml::Float(_)));
    if all_numeric {
        let cvec: Array1<Complex<f64>> = Array1::from_vec(
            arr.into_iter()
                .map(|v| {
                    let f = match v {
                        Toml::Integer(i) => i as f64,
                        Toml::Float(f) => f,
                        _ => unreachable!(),
                    };
                    Complex::new(f, 0.0)
                })
                .collect(),
        );
        return Value::Vector(cvec);
    }

    // Array of equal-length, all-numeric sub-arrays → Matrix (one per row).
    // This mirrors how value_to_toml serializes a Matrix; without it a saved
    // Matrix would round-trip back as a Tuple of row vectors.
    let row_lens: Option<Vec<usize>> = arr
        .iter()
        .map(|v| match v {
            Toml::Array(row)
                if !row.is_empty()
                    && row
                        .iter()
                        .all(|e| matches!(e, Toml::Integer(_) | Toml::Float(_))) =>
            {
                Some(row.len())
            }
            _ => None,
        })
        .collect();
    if let Some(lens) = row_lens {
        if lens.iter().all(|&l| l == lens[0]) {
            let (nrows, ncols) = (arr.len(), lens[0]);
            let mut data = Vec::with_capacity(nrows * ncols);
            for v in &arr {
                if let Toml::Array(row) = v {
                    for e in row {
                        let f = match e {
                            Toml::Integer(i) => *i as f64,
                            Toml::Float(f) => *f,
                            _ => unreachable!(),
                        };
                        data.push(Complex::new(f, 0.0));
                    }
                }
            }
            if let Ok(mat) = ndarray::Array2::from_shape_vec((nrows, ncols), data) {
                return Value::Matrix(mat);
            }
        }
    }

    // Mixed or non-numeric array → Tuple
    Value::Tuple(arr.into_iter().map(toml_to_value).collect())
}

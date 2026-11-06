//! `ind2sub` and `sub2ind`.
//!
//! The arithmetic is [`rustlab_core::subscript`]. This module adds the
//! script shapes: nargout folding, rank at most 3, and one-element
//! subscript broadcast. A linear index or a subscript outside the array
//! is an error (MATLAB `ind2sub` would invent a page past the last size).

use ndarray::{Array1, Array2, Array3};
use rustlab_core::subscript::{self, SubscriptError};

use super::shared::Shared;
use super::value::Value;
use crate::error::ScriptError;

const MAX_RANK: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Size,
    Index,
}

#[derive(Clone, PartialEq, Eq)]
enum IdxShape {
    Scalar,
    Vector(usize),
    Matrix(usize, usize),
    Tensor([usize; 3]),
}

struct Numbers {
    shape: IdxShape,
    values: Vec<u64>,
}

pub fn builtin_ind2sub(args: Vec<Value>, nargout: usize) -> Result<Value, ScriptError> {
    if args.len() != 2 {
        return Err(ScriptError::arg_count("ind2sub".into(), 2, args.len()));
    }
    let nout = if nargout == 0 { 1 } else { nargout };
    if nout > MAX_RANK {
        return Err(rank_err("ind2sub"));
    }
    let sizes = parse_sizes("ind2sub", &args[0])?;
    let eff = effective_sizes("ind2sub", &sizes, nout)?;
    let idx = read_indices("ind2sub", &args[1])?;
    let mut cols = vec![Vec::with_capacity(idx.values.len()); nout];
    for &k in &idx.values {
        let subs = subscript::ind2sub(&eff, k).map_err(|e| map_err("ind2sub", e))?;
        for (col, s) in cols.iter_mut().zip(subs) {
            col.push(s as f64);
        }
    }
    if nout == 1 {
        return pack("ind2sub", &idx.shape, cols.pop().unwrap_or_default());
    }
    let mut out = Vec::with_capacity(nout);
    for col in cols {
        out.push(pack("ind2sub", &idx.shape, col)?);
    }
    Ok(Value::Tuple(out))
}

pub fn builtin_sub2ind(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() < 2 {
        return Err(ScriptError::arg_count_range(
            "sub2ind".into(),
            2,
            MAX_RANK + 1,
            args.len(),
        ));
    }
    if args.len() > MAX_RANK + 1 {
        return Err(rank_err("sub2ind"));
    }
    let sizes = parse_sizes("sub2ind", &args[0])?;
    let mut parts = Vec::with_capacity(args.len() - 1);
    for arg in &args[1..] {
        parts.push(read_indices("sub2ind", arg)?);
    }
    let aligned = align_sizes("sub2ind", &sizes, parts.len())?;
    let shape = common_shape("sub2ind", &parts)?;
    for (dim, (sz, part)) in aligned.iter().zip(&parts).enumerate() {
        for &sub in &part.values {
            if sub < 1 || sub > *sz {
                return Err(ScriptError::type_err(format!(
                    "sub2ind: subscript {sub} on dimension {} is outside 1:{sz}",
                    dim + 1
                )));
            }
        }
    }
    // A length of 1 is a broadcast. Anything else, including empty, sets the
    // output length. Every non-broadcast array has that same length.
    let n = parts
        .iter()
        .map(|part| part.values.len())
        .find(|len| *len != 1)
        .unwrap_or(1);
    if n == 0 {
        return pack("sub2ind", &shape, Vec::new());
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mut subs = Vec::with_capacity(aligned.len());
        for part in &parts {
            let v = if part.values.len() == 1 {
                part.values[0]
            } else {
                part.values[i]
            };
            subs.push(v);
        }
        let k = subscript::sub2ind(&aligned, &subs).map_err(|e| map_err("sub2ind", e))?;
        out.push(k as f64);
    }
    pack("sub2ind", &shape, out)
}

fn rank_err(name: &str) -> ScriptError {
    ScriptError::type_err(format!("{name}: rustlab arrays are at most rank 3"))
}

fn map_err(name: &str, err: SubscriptError) -> ScriptError {
    ScriptError::type_err(format!("{name}: {err}"))
}

/// Collapse trailing sizes into the last requested subscript, or pad with 1.
fn effective_sizes(name: &str, sizes: &[u64], nout: usize) -> Result<Vec<u64>, ScriptError> {
    if nout == sizes.len() {
        return Ok(sizes.to_vec());
    }
    if nout < sizes.len() {
        let mut out = sizes[..nout].to_vec();
        let mut prod = 1u64;
        for &sz in &sizes[nout - 1..] {
            prod = prod
                .checked_mul(sz)
                .ok_or_else(|| map_err(name, SubscriptError::SizeOverflow))?;
        }
        out[nout - 1] = prod;
        return Ok(out);
    }
    let mut out = sizes.to_vec();
    out.resize(nout, 1);
    Ok(out)
}

/// Exact length, drop trailing sizes that are all 1, or pad with 1s.
fn align_sizes(name: &str, sizes: &[u64], nsubs: usize) -> Result<Vec<u64>, ScriptError> {
    if nsubs == sizes.len() {
        return Ok(sizes.to_vec());
    }
    if nsubs < sizes.len() {
        if sizes[nsubs..].iter().all(|&s| s == 1) {
            return Ok(sizes[..nsubs].to_vec());
        }
        return Err(ScriptError::type_err(format!(
            "{name}: expected {} subscript arrays for a size of length {}, got {nsubs}",
            sizes.len(),
            sizes.len()
        )));
    }
    let mut out = sizes.to_vec();
    out.resize(nsubs, 1);
    Ok(out)
}

/// One-element arrays are repeated. Every other array must share one shape.
/// The result uses that shape, or the first non-scalar shape when every
/// subscript is a single element.
fn common_shape(name: &str, parts: &[Numbers]) -> Result<IdxShape, ScriptError> {
    let mut concrete: Option<&Numbers> = None;
    for part in parts {
        if part.values.len() == 1 {
            continue;
        }
        match concrete {
            None => concrete = Some(part),
            Some(prev) if prev.shape == part.shape => {}
            Some(_) => {
                return Err(ScriptError::type_err(format!(
                    "{name}: subscript arrays must be the same size"
                )));
            }
        }
    }
    if let Some(found) = concrete {
        return Ok(found.shape.clone());
    }
    for part in parts {
        if !matches!(part.shape, IdxShape::Scalar) {
            return Ok(part.shape.clone());
        }
    }
    Ok(IdxShape::Scalar)
}

fn parse_sizes(name: &str, v: &Value) -> Result<Vec<u64>, ScriptError> {
    if let Value::Matrix(m) = v {
        let (rows, cols) = shape2(m);
        if rows > 1 && cols > 1 {
            return Err(ScriptError::type_err(format!(
                "{name}: size must be a scalar or a vector"
            )));
        }
    }
    if let Value::IntArray { rows, cols, .. } = v {
        if *rows > 1 && *cols > 1 {
            return Err(ScriptError::type_err(format!(
                "{name}: size must be a scalar or a vector"
            )));
        }
    }
    if matches!(v, Value::Tensor3(_)) {
        return Err(ScriptError::type_err(format!(
            "{name}: size must be a scalar or a vector, got tensor3"
        )));
    }
    let (_shape, dims) = read_numbers(name, v, Role::Size)?;
    if dims.is_empty() {
        return Err(ScriptError::type_err(format!(
            "{name}: size must not be empty"
        )));
    }
    if dims.len() > MAX_RANK {
        return Err(rank_err(name));
    }
    Ok(dims)
}

fn read_indices(name: &str, v: &Value) -> Result<Numbers, ScriptError> {
    let (shape, values) = read_numbers(name, v, Role::Index)?;
    Ok(Numbers { shape, values })
}

fn read_numbers(name: &str, v: &Value, role: Role) -> Result<(IdxShape, Vec<u64>), ScriptError> {
    let mut values = Vec::new();
    let shape = match v {
        Value::Scalar(x) => {
            values.push(convert(name, role, *x, 0.0)?);
            IdxShape::Scalar
        }
        Value::Complex(z) => {
            values.push(convert(name, role, z.re, z.im)?);
            IdxShape::Scalar
        }
        Value::Int { data, .. } => {
            values.push(convert_i128(name, role, *data)?);
            IdxShape::Scalar
        }
        Value::IntArray {
            data, rows, cols, ..
        } => {
            for &x in data {
                values.push(convert_i128(name, role, x)?);
            }
            IdxShape::Matrix(*rows, *cols)
        }
        Value::Vector(a) => {
            let n = Shared::len(a);
            extend_shared_1(name, role, a, &mut values)?;
            IdxShape::Vector(n)
        }
        Value::Matrix(m) => {
            let (rows, cols) = shape2(m);
            extend_shared_2(name, role, m, &mut values)?;
            IdxShape::Matrix(rows, cols)
        }
        Value::Tensor3(t) => {
            let dims = shape3(t);
            extend_shared_3(name, role, t, &mut values)?;
            IdxShape::Tensor(dims)
        }
        other => {
            return Err(ScriptError::type_err(match role {
                Role::Size => format!(
                    "{name}: size must be a scalar or a vector, got {}",
                    other.type_name()
                ),
                Role::Index => format!(
                    "{name}: index must be a numeric array, got {}",
                    other.type_name()
                ),
            }));
        }
    };
    Ok((shape, values))
}

fn extend_shared_1(
    name: &str,
    role: Role,
    a: &Shared<ndarray::Array1<rustlab_core::C64>>,
    out: &mut Vec<u64>,
) -> Result<(), ScriptError> {
    if let Some(real) = Shared::as_real(a) {
        for &x in real.iter() {
            out.push(convert(name, role, x, 0.0)?);
        }
    } else {
        for z in a.iter() {
            out.push(convert(name, role, z.re, z.im)?);
        }
    }
    Ok(())
}

fn extend_shared_2(
    name: &str,
    role: Role,
    a: &Shared<ndarray::Array2<rustlab_core::C64>>,
    out: &mut Vec<u64>,
) -> Result<(), ScriptError> {
    if let Some(real) = Shared::as_real(a) {
        for &x in real.iter() {
            out.push(convert(name, role, x, 0.0)?);
        }
    } else {
        for z in a.iter() {
            out.push(convert(name, role, z.re, z.im)?);
        }
    }
    Ok(())
}

fn extend_shared_3(
    name: &str,
    role: Role,
    a: &Shared<ndarray::Array3<rustlab_core::C64>>,
    out: &mut Vec<u64>,
) -> Result<(), ScriptError> {
    if let Some(real) = Shared::as_real(a) {
        for &x in real.iter() {
            out.push(convert(name, role, x, 0.0)?);
        }
    } else {
        for z in a.iter() {
            out.push(convert(name, role, z.re, z.im)?);
        }
    }
    Ok(())
}

fn shape2(m: &Shared<ndarray::Array2<rustlab_core::C64>>) -> (usize, usize) {
    let shp = Shared::shape(m);
    (shp[0], shp[1])
}

fn shape3(t: &Shared<ndarray::Array3<rustlab_core::C64>>) -> [usize; 3] {
    let shp = Shared::shape(t);
    [shp[0], shp[1], shp[2]]
}

fn convert(name: &str, role: Role, re: f64, im: f64) -> Result<u64, ScriptError> {
    if !im.is_finite() || im.abs() >= 1e-10 {
        let shown = if im < 0.0 {
            format!("{re}{im}j")
        } else {
            format!("{re}+{im}j")
        };
        return Err(ScriptError::type_err(match role {
            Role::Size => format!("{name}: size must be a non-negative integer, got {shown}"),
            Role::Index => format!("{name}: index must be a real positive integer, got {shown}"),
        }));
    }
    match role {
        Role::Size => {
            let n = Value::count_from_f64(re).map_err(|_| {
                ScriptError::type_err(format!(
                    "{name}: size must be a non-negative integer, got {re}"
                ))
            })?;
            Ok(n as u64)
        }
        Role::Index => {
            let n = Value::count_from_f64(re).map_err(|_| {
                ScriptError::type_err(format!(
                    "{name}: index must be a positive integer, got {re}"
                ))
            })?;
            if n == 0 {
                return Err(ScriptError::type_err(format!(
                    "{name}: index must be a positive integer, got 0"
                )));
            }
            Ok(n as u64)
        }
    }
}

fn convert_i128(name: &str, role: Role, v: i128) -> Result<u64, ScriptError> {
    let ok = match role {
        Role::Size => v >= 0,
        Role::Index => v >= 1,
    };
    if !ok || v > u64::MAX as i128 {
        return Err(ScriptError::type_err(match role {
            Role::Size => format!("{name}: size must be a non-negative integer, got {v}"),
            Role::Index => format!("{name}: index must be a positive integer, got {v}"),
        }));
    }
    Ok(v as u64)
}

fn pack(name: &str, shape: &IdxShape, data: Vec<f64>) -> Result<Value, ScriptError> {
    match *shape {
        IdxShape::Scalar => data
            .first()
            .copied()
            .map(Value::Scalar)
            .ok_or_else(|| ScriptError::type_err(format!("{name}: empty scalar"))),
        IdxShape::Vector(_) => Ok(Value::real_vector(Array1::from_vec(data))),
        IdxShape::Matrix(rows, cols) => Array2::from_shape_vec((rows, cols), data)
            .map(Value::real_matrix)
            .map_err(|_| ScriptError::type_err(format!("{name}: internal shape mismatch"))),
        IdxShape::Tensor(dims) => Array3::from_shape_vec(dims, data)
            .map(Value::real_tensor3)
            .map_err(|_| ScriptError::type_err(format!("{name}: internal shape mismatch"))),
    }
}

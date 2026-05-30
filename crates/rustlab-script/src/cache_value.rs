//! Phase 3d: cache-side encoding of `Value`.
//!
//! Two operations the dispatcher needs:
//!
//! 1. **Fingerprint** a function's argument list into a stable
//!    `[u8; 32]` used as the `input_hash` column of `cache_entries`.
//!    NaN-bearing or non-cacheable inputs short-circuit the whole
//!    list to `None` and the call bypasses the cache.
//!
//! 2. **Serialize / deserialize** a function's result `Value` into a
//!    blob the SQLite store can hold and replay later. Variants that
//!    hold non-deterministic interior state (`Arc<Mutex<...>>` filter
//!    history, live figures, audio streams, sparse decompositions,
//!    captured-env lambdas) deliberately fail with `None` so the
//!    `put` step skips them — better a cache miss than a stale value.
//!
//! The wire format is a compact tagged binary stream. Each value
//! starts with a one-byte type tag, then a variant-specific payload
//! in little-endian. The tags are an explicit numeric enum so the
//! variant order in `Value` can shift without silently invalidating
//! every cached blob; bumping a tag is a wire-incompatible change
//! and requires a `WIRE_VERSION` bump.

use crate::eval::Value;
use ndarray::{Array1, Array2};
use num_complex::Complex;
use rustlab_core::{CMatrix, CVector, Fingerprint, SparseMat, SparseVec, C64};
use std::collections::BTreeMap;

/// Wire-format version. Bump on any layout change to a `Tag`; the
/// dispatcher refuses to deserialize blobs that don't carry the
/// current version, so the cache silently treats them as misses
/// rather than corrupting results.
///
/// History:
/// - **v1** — initial format. Single-output user functions stored
///   their result as a bare `Value` (e.g. `Scalar(5.0)`).
/// - **v2** — every user-function result is wrapped in a
///   `Value::Tuple` representing the canonical full output set
///   (one element per declared return var, `Value::None` for
///   unassigned slots). The dispatcher shapes the user-facing
///   return on retrieval based on the caller's `nargout`. Old
///   v1 blobs are silently re-computed.
const WIRE_VERSION: u8 = 2;

/// One-byte type tags. Add new tags at the end; never recycle a tag
/// that has ever been written, because old blobs may still reference it.
mod tag {
    pub const NONE: u8 = 0x00;
    pub const SCALAR: u8 = 0x01;
    pub const COMPLEX: u8 = 0x02;
    pub const BOOL: u8 = 0x03;
    pub const STR: u8 = 0x04;
    pub const VECTOR: u8 = 0x05;
    pub const MATRIX: u8 = 0x06;
    pub const TUPLE: u8 = 0x07;
    pub const STRUCT: u8 = 0x08;
    pub const STRING_ARRAY: u8 = 0x09;
    pub const SPARSE_VECTOR: u8 = 0x0A;
    pub const SPARSE_MATRIX: u8 = 0x0B;
    pub const FUNC_HANDLE: u8 = 0x0C;
}

const ARGS_TAG: &[u8] = b"rustlab-cache/args/v1\0";

// ── fingerprint ──────────────────────────────────────────────────────

/// Fingerprint a single value into the `input_hash` byte stream.
/// Returns `false` if the value is uncacheable (NaN somewhere inside
/// or a non-cacheable variant). The dispatcher reads this as
/// "bypass the cache for this call."
pub fn feed_value(hasher: &mut blake3::Hasher, v: &Value) -> bool {
    match v {
        Value::Scalar(n) => n.feed(hasher),
        Value::Complex(c) => c.feed(hasher),
        Value::Vector(v) => v.feed(hasher),
        Value::Matrix(m) => m.feed(hasher),
        Value::Bool(b) => b.feed(hasher),
        Value::Str(s) => s.feed(hasher),
        Value::SparseVector(sv) => sv.feed(hasher),
        Value::SparseMatrix(sm) => sm.feed(hasher),
        Value::StringArray(items) => {
            // Length-prefixed list of length-prefixed strings.
            hasher.update(&[tag::STRING_ARRAY]);
            hasher.update(&(items.len() as u64).to_le_bytes());
            for s in items {
                if !s.feed(hasher) {
                    return false;
                }
            }
            true
        }
        Value::Tuple(items) => {
            hasher.update(&[tag::TUPLE]);
            hasher.update(&(items.len() as u64).to_le_bytes());
            for v in items {
                if !feed_value(hasher, v) {
                    return false;
                }
            }
            true
        }
        Value::Struct(map) => {
            // Sorted key iteration so identical structs hash to the
            // same value regardless of HashMap's internal order.
            hasher.update(&[tag::STRUCT]);
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            hasher.update(&(sorted.len() as u64).to_le_bytes());
            for (k, v) in sorted {
                if !k.as_str().feed(hasher) {
                    return false;
                }
                if !feed_value(hasher, v) {
                    return false;
                }
            }
            true
        }
        Value::FuncHandle(name) => {
            hasher.update(&[tag::FUNC_HANDLE]);
            name.as_str().feed(hasher)
        }
        Value::None => {
            hasher.update(&[tag::NONE]);
            true
        }
        // Everything else is uncacheable as an argument either
        // because of interior mutability (FirState, DspStreamState,
        // LiveFigure), externally-bound resources (AudioIn/Out),
        // or because the captured environment of a Lambda is too
        // open-ended to fingerprint reliably.
        Value::Tensor3(_)
        | Value::QFmt(_)
        | Value::All
        | Value::TransferFn { .. }
        | Value::StateSpace { .. }
        | Value::Lambda { .. }
        | Value::FirState(_)
        | Value::DspStreamState(_)
        | Value::AudioIn { .. }
        | Value::AudioOut { .. }
        | Value::LiveFigure(_)
        | Value::SparseFactor(_) => false,
    }
}

/// Fingerprint a function's full argument list. The `ARGS_TAG`
/// prefix domain-separates this from any other use of `feed_value`
/// so a single argument can never collide with a one-element call.
pub fn fingerprint_args(args: &[Value]) -> Option<[u8; 32]> {
    let mut h = blake3::Hasher::new();
    h.update(ARGS_TAG);
    h.update(&(args.len() as u64).to_le_bytes());
    for arg in args {
        if !feed_value(&mut h, arg) {
            return None;
        }
    }
    Some(*h.finalize().as_bytes())
}

// ── serialize ────────────────────────────────────────────────────────

/// Encode `v` into the cache wire format. Returns `None` for
/// non-cacheable variants — the dispatcher treats this as "miss but
/// don't store."
pub fn serialize_value(v: &Value) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    out.push(WIRE_VERSION);
    encode_value(&mut out, v)?;
    Some(out)
}

/// Decode a blob produced by [`serialize_value`]. Returns `None` for
/// version-mismatched, truncated, or malformed blobs — the
/// dispatcher treats this as a cache miss without surfacing the
/// failure to the user (`serialization_skips` counter records it).
pub fn deserialize_value(bytes: &[u8]) -> Option<Value> {
    let mut cur = Cursor::new(bytes);
    let v = cur.take_u8()?;
    if v != WIRE_VERSION {
        return None;
    }
    decode_value(&mut cur)
}

fn encode_value(out: &mut Vec<u8>, v: &Value) -> Option<()> {
    match v {
        Value::None => out.push(tag::NONE),
        Value::Scalar(n) => {
            out.push(tag::SCALAR);
            out.extend_from_slice(&n.to_le_bytes());
        }
        Value::Complex(c) => {
            out.push(tag::COMPLEX);
            out.extend_from_slice(&c.re.to_le_bytes());
            out.extend_from_slice(&c.im.to_le_bytes());
        }
        Value::Bool(b) => {
            out.push(tag::BOOL);
            out.push(*b as u8);
        }
        Value::Str(s) => {
            out.push(tag::STR);
            encode_str(out, s);
        }
        Value::Vector(vec) => {
            out.push(tag::VECTOR);
            out.extend_from_slice(&(vec.len() as u64).to_le_bytes());
            for c in vec.iter() {
                out.extend_from_slice(&c.re.to_le_bytes());
                out.extend_from_slice(&c.im.to_le_bytes());
            }
        }
        Value::Matrix(m) => {
            out.push(tag::MATRIX);
            out.extend_from_slice(&(m.nrows() as u64).to_le_bytes());
            out.extend_from_slice(&(m.ncols() as u64).to_le_bytes());
            // ndarray::iter walks logical row-major regardless of
            // memory layout — matches the Fingerprint impl and so
            // the wire form is stable across transposed views.
            for c in m.iter() {
                out.extend_from_slice(&c.re.to_le_bytes());
                out.extend_from_slice(&c.im.to_le_bytes());
            }
        }
        Value::Tuple(items) => {
            out.push(tag::TUPLE);
            out.extend_from_slice(&(items.len() as u64).to_le_bytes());
            for v in items {
                encode_value(out, v)?;
            }
        }
        Value::Struct(map) => {
            out.push(tag::STRUCT);
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            out.extend_from_slice(&(sorted.len() as u64).to_le_bytes());
            for (k, v) in sorted {
                encode_str(out, k);
                encode_value(out, v)?;
            }
        }
        Value::StringArray(items) => {
            out.push(tag::STRING_ARRAY);
            out.extend_from_slice(&(items.len() as u64).to_le_bytes());
            for s in items {
                encode_str(out, s);
            }
        }
        Value::SparseVector(sv) => {
            out.push(tag::SPARSE_VECTOR);
            out.extend_from_slice(&(sv.len as u64).to_le_bytes());
            out.extend_from_slice(&(sv.entries.len() as u64).to_le_bytes());
            for (i, c) in &sv.entries {
                out.extend_from_slice(&(*i as u64).to_le_bytes());
                out.extend_from_slice(&c.re.to_le_bytes());
                out.extend_from_slice(&c.im.to_le_bytes());
            }
        }
        Value::SparseMatrix(sm) => {
            out.push(tag::SPARSE_MATRIX);
            out.extend_from_slice(&(sm.rows as u64).to_le_bytes());
            out.extend_from_slice(&(sm.cols as u64).to_le_bytes());
            out.extend_from_slice(&(sm.entries.len() as u64).to_le_bytes());
            for (r, c, v) in &sm.entries {
                out.extend_from_slice(&(*r as u64).to_le_bytes());
                out.extend_from_slice(&(*c as u64).to_le_bytes());
                out.extend_from_slice(&v.re.to_le_bytes());
                out.extend_from_slice(&v.im.to_le_bytes());
            }
        }
        Value::FuncHandle(name) => {
            out.push(tag::FUNC_HANDLE);
            encode_str(out, name);
        }
        // Everything else is non-cacheable on the result side too.
        Value::Tensor3(_)
        | Value::QFmt(_)
        | Value::All
        | Value::TransferFn { .. }
        | Value::StateSpace { .. }
        | Value::Lambda { .. }
        | Value::FirState(_)
        | Value::DspStreamState(_)
        | Value::AudioIn { .. }
        | Value::AudioOut { .. }
        | Value::LiveFigure(_)
        | Value::SparseFactor(_) => return None,
    }
    Some(())
}

fn decode_value(cur: &mut Cursor) -> Option<Value> {
    let t = cur.take_u8()?;
    match t {
        tag::NONE => Some(Value::None),
        tag::SCALAR => Some(Value::Scalar(cur.take_f64()?)),
        tag::COMPLEX => {
            let re = cur.take_f64()?;
            let im = cur.take_f64()?;
            Some(Value::Complex(Complex::new(re, im)))
        }
        tag::BOOL => Some(Value::Bool(cur.take_u8()? != 0)),
        tag::STR => Some(Value::Str(cur.take_str()?)),
        tag::VECTOR => {
            let n = cur.take_u64()? as usize;
            let mut buf: Vec<C64> = Vec::with_capacity(n);
            for _ in 0..n {
                let re = cur.take_f64()?;
                let im = cur.take_f64()?;
                buf.push(Complex::new(re, im));
            }
            Some(Value::Vector(Array1::from(buf) as CVector))
        }
        tag::MATRIX => {
            let nrows = cur.take_u64()? as usize;
            let ncols = cur.take_u64()? as usize;
            let total = nrows.checked_mul(ncols)?;
            let mut buf: Vec<C64> = Vec::with_capacity(total);
            for _ in 0..total {
                let re = cur.take_f64()?;
                let im = cur.take_f64()?;
                buf.push(Complex::new(re, im));
            }
            let arr = Array2::from_shape_vec((nrows, ncols), buf).ok()?;
            Some(Value::Matrix(arr as CMatrix))
        }
        tag::TUPLE => {
            let n = cur.take_u64()? as usize;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(decode_value(cur)?);
            }
            Some(Value::Tuple(items))
        }
        tag::STRUCT => {
            let n = cur.take_u64()? as usize;
            let mut map = std::collections::HashMap::with_capacity(n);
            for _ in 0..n {
                let k = cur.take_str()?;
                let v = decode_value(cur)?;
                map.insert(k, v);
            }
            Some(Value::Struct(map))
        }
        tag::STRING_ARRAY => {
            let n = cur.take_u64()? as usize;
            let mut items: Vec<String> = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(cur.take_str()?);
            }
            Some(Value::StringArray(items))
        }
        tag::SPARSE_VECTOR => {
            let len = cur.take_u64()? as usize;
            let nnz = cur.take_u64()? as usize;
            let mut entries: Vec<(usize, C64)> = Vec::with_capacity(nnz);
            for _ in 0..nnz {
                let idx = cur.take_u64()? as usize;
                let re = cur.take_f64()?;
                let im = cur.take_f64()?;
                entries.push((idx, Complex::new(re, im)));
            }
            Some(Value::SparseVector(SparseVec { len, entries }))
        }
        tag::SPARSE_MATRIX => {
            let rows = cur.take_u64()? as usize;
            let cols = cur.take_u64()? as usize;
            let nnz = cur.take_u64()? as usize;
            let mut entries: Vec<(usize, usize, C64)> = Vec::with_capacity(nnz);
            for _ in 0..nnz {
                let r = cur.take_u64()? as usize;
                let c = cur.take_u64()? as usize;
                let re = cur.take_f64()?;
                let im = cur.take_f64()?;
                entries.push((r, c, Complex::new(re, im)));
            }
            Some(Value::SparseMatrix(SparseMat {
                rows,
                cols,
                entries,
                ordering_hint: None,
            }))
        }
        tag::FUNC_HANDLE => Some(Value::FuncHandle(cur.take_str()?)),
        _ => None,
    }
}

fn encode_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u64).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// Tiny cursor over a `&[u8]` — every read is bounds-checked and
/// returns `None` on truncation. Kept private so we don't expose
/// the wire format as an API.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.pos.checked_add(n)? > self.buf.len() {
            return None;
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }

    fn take_u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn take_u64(&mut self) -> Option<u64> {
        let bytes = self.take(8)?;
        Some(u64::from_le_bytes(bytes.try_into().ok()?))
    }

    fn take_f64(&mut self) -> Option<f64> {
        let bytes = self.take(8)?;
        Some(f64::from_le_bytes(bytes.try_into().ok()?))
    }

    fn take_str(&mut self) -> Option<String> {
        let len = self.take_u64()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    fn rt(v: &Value) -> Option<Value> {
        let bytes = serialize_value(v)?;
        deserialize_value(&bytes)
    }

    fn val_eq(a: &Value, b: &Value) -> bool {
        // Custom equality because Value doesn't impl PartialEq for
        // every variant. We only check the ones we serialize.
        match (a, b) {
            (Value::None, Value::None) => true,
            (Value::Scalar(x), Value::Scalar(y)) => x == y,
            (Value::Complex(x), Value::Complex(y)) => x == y,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Str(x), Value::Str(y)) => x == y,
            (Value::Vector(x), Value::Vector(y)) => x == y,
            (Value::Matrix(x), Value::Matrix(y)) => x == y,
            (Value::Tuple(x), Value::Tuple(y)) => {
                x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| val_eq(a, b))
            }
            (Value::Struct(x), Value::Struct(y)) => {
                x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| val_eq(v, w)))
            }
            (Value::StringArray(x), Value::StringArray(y)) => x == y,
            (Value::SparseVector(x), Value::SparseVector(y)) => x == y,
            (Value::SparseMatrix(x), Value::SparseMatrix(y)) => {
                // Ordering hint is metadata; we drop it on
                // round-trip.
                x.rows == y.rows && x.cols == y.cols && x.entries == y.entries
            }
            (Value::FuncHandle(x), Value::FuncHandle(y)) => x == y,
            _ => false,
        }
    }

    #[test]
    fn round_trip_none_and_scalars() {
        for v in [
            Value::None,
            Value::Scalar(0.0),
            Value::Scalar(-1.5),
            Value::Scalar(f64::INFINITY),
            Value::Bool(true),
            Value::Bool(false),
            Value::Str("hello".into()),
            Value::Str(String::new()),
        ] {
            let back = rt(&v).expect("rt");
            assert!(val_eq(&v, &back), "round-trip failed for {v:?}");
        }
    }

    #[test]
    fn round_trip_complex_and_vector() {
        let v = Value::Complex(Complex::new(1.5, -2.0));
        assert!(val_eq(&v, &rt(&v).unwrap()));

        let arr = Array1::from(vec![
            Complex::new(1.0, 0.0),
            Complex::new(0.0, 1.0),
            Complex::new(-1.0, 2.0),
        ]);
        let v = Value::Vector(arr);
        assert!(val_eq(&v, &rt(&v).unwrap()));
    }

    #[test]
    fn round_trip_matrix_preserves_shape() {
        let m: CMatrix = array![
            [Complex::new(1.0, 0.0), Complex::new(2.0, 1.0)],
            [Complex::new(3.0, -1.0), Complex::new(4.0, 0.5)],
        ];
        let v = Value::Matrix(m);
        let back = rt(&v).unwrap();
        if let Value::Matrix(b) = &back {
            assert_eq!(b.shape(), &[2, 2]);
        } else {
            panic!("expected Matrix");
        }
        assert!(val_eq(&v, &back));
    }

    #[test]
    fn round_trip_tuple_and_struct() {
        let v = Value::Tuple(vec![Value::Scalar(1.0), Value::Bool(true)]);
        assert!(val_eq(&v, &rt(&v).unwrap()));

        let mut m = std::collections::HashMap::new();
        m.insert("x".to_string(), Value::Scalar(3.14));
        m.insert("name".to_string(), Value::Str("circle".into()));
        let v = Value::Struct(m);
        assert!(val_eq(&v, &rt(&v).unwrap()));
    }

    #[test]
    fn round_trip_string_array_and_func_handle() {
        let v = Value::StringArray(vec!["a".into(), "bc".into(), String::new()]);
        assert!(val_eq(&v, &rt(&v).unwrap()));

        let v = Value::FuncHandle("expensive".into());
        assert!(val_eq(&v, &rt(&v).unwrap()));
    }

    #[test]
    fn round_trip_sparse_vector_and_matrix() {
        let sv = SparseVec::new(
            10,
            vec![
                (0, Complex::new(1.0, 0.0)),
                (3, Complex::new(0.0, 2.0)),
                (7, Complex::new(-1.0, 0.5)),
            ],
        );
        let v = Value::SparseVector(sv);
        assert!(val_eq(&v, &rt(&v).unwrap()));

        let sm = SparseMat::new(
            4,
            4,
            vec![
                (0, 0, Complex::new(1.0, 0.0)),
                (1, 2, Complex::new(2.0, 0.0)),
                (3, 3, Complex::new(-1.0, 0.0)),
            ],
        );
        let v = Value::SparseMatrix(sm);
        assert!(val_eq(&v, &rt(&v).unwrap()));
    }

    #[test]
    fn non_cacheable_values_serialize_to_none() {
        let v = Value::Tensor3(ndarray::Array3::from_elem(
            (2, 2, 2),
            Complex::new(0.0, 0.0),
        ));
        assert!(serialize_value(&v).is_none());

        let v = Value::Lambda {
            params: vec!["x".into()],
            body: Box::new(crate::ast::Expr::Var("x".into())),
            captured_env: std::collections::HashMap::new(),
        };
        assert!(serialize_value(&v).is_none());
    }

    #[test]
    fn version_mismatch_fails_deserialize() {
        let mut bytes = serialize_value(&Value::Scalar(1.0)).unwrap();
        bytes[0] = 99; // wrong wire version
        assert!(deserialize_value(&bytes).is_none());
    }

    #[test]
    fn truncated_blob_fails_deserialize() {
        let bytes = serialize_value(&Value::Scalar(1.0)).unwrap();
        // Drop the last byte → reader runs off the end of the f64.
        let truncated = &bytes[..bytes.len() - 1];
        assert!(deserialize_value(truncated).is_none());
    }

    // ── fingerprint ──────────────────────────────────────────────

    #[test]
    fn fingerprint_args_stable_for_same_inputs() {
        let a = [Value::Scalar(1.0), Value::Scalar(2.0)];
        let b = [Value::Scalar(1.0), Value::Scalar(2.0)];
        assert_eq!(fingerprint_args(&a), fingerprint_args(&b));
    }

    #[test]
    fn fingerprint_args_distinguishes_values() {
        let a = [Value::Scalar(1.0)];
        let b = [Value::Scalar(2.0)];
        assert_ne!(fingerprint_args(&a), fingerprint_args(&b));
    }

    #[test]
    fn fingerprint_args_distinguishes_arity() {
        let a = [Value::Scalar(1.0)];
        let b = [Value::Scalar(1.0), Value::Scalar(0.0)];
        assert_ne!(fingerprint_args(&a), fingerprint_args(&b));
    }

    #[test]
    fn nan_arg_returns_none() {
        let a = [Value::Scalar(f64::NAN)];
        assert!(fingerprint_args(&a).is_none());
    }

    #[test]
    fn non_cacheable_arg_returns_none() {
        let a = [Value::Lambda {
            params: vec!["x".into()],
            body: Box::new(crate::ast::Expr::Var("x".into())),
            captured_env: std::collections::HashMap::new(),
        }];
        assert!(fingerprint_args(&a).is_none());
    }

    #[test]
    fn struct_field_order_does_not_affect_fingerprint() {
        let mut m1 = std::collections::HashMap::new();
        m1.insert("a".to_string(), Value::Scalar(1.0));
        m1.insert("b".to_string(), Value::Scalar(2.0));
        let mut m2 = std::collections::HashMap::new();
        m2.insert("b".to_string(), Value::Scalar(2.0));
        m2.insert("a".to_string(), Value::Scalar(1.0));
        let a = [Value::Struct(m1)];
        let b = [Value::Struct(m2)];
        assert_eq!(fingerprint_args(&a), fingerprint_args(&b));
    }
}

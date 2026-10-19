//! Fast paths for elementwise binary operators on dense arrays.
//!
//! [`Value::binop`] takes its operands by value, so `theta - 0.001 * g`
//! used to copy `theta` and `g` out of the workspace, allocate a temporary
//! for `0.001 * g`, and allocate again for the result. Here an operand is
//! either a borrowed workspace value or an owned temporary. The common
//! array cases (array ∘ scalar, and two arrays of the same shape) are
//! computed in one pass, written into an owned temporary of the right shape
//! when there is one, and otherwise into a single new array. Everything
//! else, including arrays that are not in standard (row-major contiguous)
//! layout, falls back to [`Value::binop`] unchanged.
//!
//! Results are bit-identical to [`Value::binop`]: the same complex
//! operation is applied per element, and the "real inputs give real
//! outputs" rule (zero the imaginary part when both inputs are real) is
//! applied under exactly the conditions `binop` applies it.
//!
//! Code size: every kernel is a closure over one chunk of a slice, passed as
//! `&dyn Fn` to one of two non-generic drivers ([`drive`], [`drive_new`]).
//! The drivers run the closure on the whole slice for small arrays and on
//! rayon chunks for large ones, so rayon's job machinery is instantiated
//! once per driver rather than once per operator and operand kind, and
//! there is no separate sequential copy of each kernel. The dynamic call
//! happens once per chunk, never per element.

use super::shared::Shared;
use super::value::Value;
use crate::ast::BinOp;
use rayon::prelude::*;
use rustlab_core::C64;
use std::mem::MaybeUninit;

/// Arrays at least this long are processed in parallel chunks. Each element
/// is computed independently, so the split never changes a result. Below
/// this the thread hand-off costs more than it saves; it also keeps the
/// per-window arrays inside a `parmap` task (≈ 66 k elements for the GPT
/// workload) on their own worker thread.
pub(crate) const PAR_MIN_LEN: usize = 1 << 17;
pub(crate) const PAR_CHUNK: usize = 1 << 14;

/// A binary-operator operand: borrowed from the workspace, or an owned
/// temporary that may be reused for the result.
pub(crate) enum Operand<'a> {
    Borrowed(&'a Value),
    Owned(Value),
}

impl Operand<'_> {
    pub(crate) fn get(&self) -> &Value {
        match self {
            Operand::Borrowed(v) => v,
            Operand::Owned(v) => v,
        }
    }

    fn into_owned(self) -> Value {
        match self {
            Operand::Borrowed(v) => v.clone(),
            Operand::Owned(v) => v,
        }
    }
}

/// `l op r`, taking the fast path when one applies.
pub(crate) fn binop(op: BinOp, l: Operand, r: Operand) -> Result<Value, String> {
    use BinOp::*;
    match (l.get(), r.get()) {
        // Array ∘ scalar and scalar ∘ array.
        (Value::Vector(_) | Value::Matrix(_), Value::Scalar(_) | Value::Complex(_))
            if is_scalar_op(op) && is_contiguous(l.get()) =>
        {
            let s = scalar_of(r.get());
            Ok(with_op!(op, scalar, f => map_array(l, move |a| f(a, s))))
        }
        (Value::Scalar(_) | Value::Complex(_), Value::Vector(_) | Value::Matrix(_))
            if is_scalar_op(op) && is_contiguous(r.get()) =>
        {
            let s = scalar_of(l.get());
            Ok(with_op!(op, scalar, f => map_array(r, move |a| f(s, a))))
        }
        // Two vectors of the same length. `binop` zeroes the imaginary part
        // of real inputs for `.*`, `./`, `.^` only.
        (Value::Vector(a), Value::Vector(b))
            if a.len() == b.len()
                && matches!(op, Add | Sub | ElemMul | ElemDiv | ElemPow)
                && is_contiguous(l.get())
                && is_contiguous(r.get()) =>
        {
            let real_rule = matches!(op, ElemMul | ElemDiv | ElemPow);
            Ok(zip_arrays(op, l, r, real_rule))
        }
        // Two matrices of the same shape (no broadcasting). `binop` goes
        // through `elementwise_with_broadcast`, which zeroes the imaginary
        // part of real inputs for every elementwise op.
        (Value::Matrix(a), Value::Matrix(b))
            if a.dim() == b.dim()
                && matches!(op, Add | Sub | ElemMul | ElemDiv | ElemPow)
                && is_contiguous(l.get())
                && is_contiguous(r.get()) =>
        {
            Ok(zip_arrays(op, l, r, true))
        }
        // Matrix product: same check and kernel as `binop`, without copying
        // the operands first. A dimension mismatch takes the fallback so the
        // error message is `binop`'s.
        (Value::Matrix(a), Value::Matrix(b)) if op == Mul && a.ncols() == b.nrows() => Ok(
            Value::Matrix(rustlab_core::matmul::matmul(&a.view(), &b.view()).into()),
        ),
        _ => Value::binop(op, l.into_owned(), r.into_owned()),
    }
}

fn scalar_of(v: &Value) -> C64 {
    match v {
        Value::Scalar(x) => C64::new(*x, 0.0),
        Value::Complex(c) => *c,
        _ => unreachable!("checked by the caller"),
    }
}

/// Whether `op` is elementwise between an array and a scalar: the scalar
/// broadcast arms of `binop`, which apply the complex operation directly
/// with no real-output rule.
fn is_scalar_op(op: BinOp) -> bool {
    use BinOp::*;
    matches!(op, Add | Sub | Mul | ElemMul | Div | ElemDiv | Pow | ElemPow)
}

/// A dense array whose elements are one row-major contiguous slice, so a
/// kernel can work on `&[C64]` and the result keeps the same shape.
fn is_contiguous(v: &Value) -> bool {
    match v {
        Value::Vector(a) => a.as_slice().is_some(),
        Value::Matrix(m) => m.as_slice().is_some(),
        _ => false,
    }
}

fn data(v: &Value) -> &[C64] {
    match v {
        Value::Vector(a) => a.as_slice(),
        Value::Matrix(m) => m.as_slice(),
        _ => None,
    }
    .expect("checked contiguous by the caller")
}

/// An owned temporary whose buffer nothing else refers to, so the result
/// can be written into it. A shared one (a copy-on-write clone of a variable)
/// is read like a borrowed operand: writing would copy it first.
fn unique(v: &Value) -> bool {
    match v {
        Value::Vector(a) => Shared::is_unique(a),
        Value::Matrix(m) => Shared::is_unique(m),
        _ => false,
    }
}

fn data_mut(v: &mut Value) -> &mut [C64] {
    match v {
        Value::Vector(a) => Shared::make_mut(a).as_slice_mut(),
        Value::Matrix(m) => Shared::make_mut(m).as_slice_mut(),
        _ => None,
    }
    .expect("checked contiguous by the caller")
}

/// A value of `like`'s kind and shape holding `data` (row-major).
fn shaped_like(like: &Value, data: Vec<C64>) -> Value {
    match like {
        Value::Vector(_) => Value::Vector(ndarray::Array1::from_vec(data).into()),
        Value::Matrix(m) => Value::Matrix(
            ndarray::Array2::from_shape_vec(m.dim(), data).expect("same element count").into(),
        ),
        _ => unreachable!("checked by the caller"),
    }
}

/// Run `$body` with a local `f: impl Fn(C64, C64) -> C64` bound to the
/// operator, one monomorphised copy per operator, so the per-element call
/// is inlined (a function pointer here blocked inlining and vectorisation).
/// `scalar` accepts `*` / `/` / `^` as the elementwise forms; `elem` only
/// the dotted ones (the caller has already excluded the rest).
macro_rules! with_op {
    ($op:expr, scalar, $f:ident => $body:expr) => {{
        use BinOp::*;
        match $op {
            Add => { let $f = |x: C64, y: C64| x + y; $body }
            Sub => { let $f = |x: C64, y: C64| x - y; $body }
            Mul | ElemMul => { let $f = |x: C64, y: C64| x * y; $body }
            Div | ElemDiv => { let $f = Value::div_c64; $body }
            Pow | ElemPow => { let $f = |x: C64, y: C64| Value::pow_c64(x, y); $body }
            _ => unreachable!("checked by the caller"),
        }
    }};
    ($op:expr, elem, $f:ident => $body:expr) => {{
        use BinOp::*;
        match $op {
            Add => { let $f = |x: C64, y: C64| x + y; $body }
            Sub => { let $f = |x: C64, y: C64| x - y; $body }
            ElemMul => { let $f = |x: C64, y: C64| x * y; $body }
            ElemDiv => { let $f = Value::div_c64; $body }
            ElemPow => { let $f = |x: C64, y: C64| Value::pow_c64(x, y); $body }
            _ => unreachable!("checked by the caller"),
        }
    }};
}
use with_op;

// ── Drivers ─────────────────────────────────────────────────────────────────

/// Flags gathered while combining two arrays, to apply the real-output rule
/// after the fact without a separate scan of the inputs.
#[derive(Clone, Copy)]
struct RealFlags {
    /// Every input element on both sides had `|im| < ε` (the rule's test).
    inputs_real: bool,
    /// Some output element has an imaginary part that zeroing would change
    /// (any bit pattern other than `+0.0`).
    output_needs_zeroing: bool,
}

const NO_FLAGS: RealFlags = RealFlags { inputs_real: true, output_needs_zeroing: false };

fn combine(f: RealFlags, g: RealFlags) -> RealFlags {
    RealFlags {
        inputs_real: f.inputs_real && g.inputs_real,
        output_needs_zeroing: f.output_needs_zeroing || g.output_needs_zeroing,
    }
}

#[inline(always)]
fn track(flags: &mut RealFlags, x: C64, y: C64, z: C64) {
    flags.inputs_real &= x.im.abs() < f64::EPSILON && y.im.abs() < f64::EPSILON;
    flags.output_needs_zeroing |= z.im.to_bits() != 0;
}

/// A kernel over one chunk: `(offset of the chunk in the whole slice,
/// chunk)`, returning the flags it gathered.
type ChunkFn<'a, T> = dyn Fn(usize, &mut [T]) -> RealFlags + Sync + 'a;

/// Run `k` over `xs`: whole, or in parallel chunks when long.
fn drive(xs: &mut [C64], k: &ChunkFn<C64>) -> RealFlags {
    if xs.len() < PAR_MIN_LEN {
        return k(0, xs);
    }
    xs.par_chunks_mut(PAR_CHUNK)
        .enumerate()
        .map(|(i, c)| k(i * PAR_CHUNK, c))
        .reduce(|| NO_FLAGS, combine)
}

/// A new length-`n` buffer filled by `k`, which must initialise every
/// element of each chunk it is given.
fn drive_new(n: usize, k: &ChunkFn<MaybeUninit<C64>>) -> (Vec<C64>, RealFlags) {
    drive_new_sized(n, PAR_MIN_LEN, PAR_CHUNK, k)
}

/// [`drive_new`] with its own parallel threshold and chunk length, for
/// kernels much costlier per element than arithmetic.
fn drive_new_sized(n: usize, min_len: usize, chunk: usize, k: &ChunkFn<MaybeUninit<C64>>) -> (Vec<C64>, RealFlags) {
    let mut out: Vec<C64> = Vec::with_capacity(n);
    let dst = &mut out.spare_capacity_mut()[..n];
    let flags = if n < min_len {
        k(0, dst)
    } else {
        dst.par_chunks_mut(chunk)
            .enumerate()
            .map(|(i, c)| k(i * chunk, c))
            .reduce(|| NO_FLAGS, combine)
    };
    // SAFETY: the chunks cover `0..n` exactly once and `k` initialises every
    // element of each chunk it is given.
    unsafe { out.set_len(n) };
    (out, flags)
}

// ── Kernels ─────────────────────────────────────────────────────────────────

/// Apply `f` to every element of an array operand, in place when it is an
/// owned temporary.
fn map_array(a: Operand, f: impl Fn(C64) -> C64 + Sync) -> Value {
    match a {
        Operand::Owned(mut v) if unique(&v) => {
            drive(data_mut(&mut v), &|_, c: &mut [C64]| {
                for x in c {
                    *x = f(*x);
                }
                NO_FLAGS
            });
            v
        }
        a => {
            let v = a.get();
            let src = data(v);
            let (out, _) = drive_new(src.len(), &|o, c: &mut [MaybeUninit<C64>]| {
                for (d, &x) in c.iter_mut().zip(&src[o..]) {
                    d.write(f(x));
                }
                NO_FLAGS
            });
            shaped_like(v, out)
        }
    }
}

/// Combine two same-shape arrays elementwise. Writes into whichever operand
/// is an owned temporary, else allocates once. With `real_rule`, zeroes the
/// imaginary parts of the result when both inputs were real.
fn zip_arrays(op: BinOp, l: Operand, r: Operand, real_rule: bool) -> Value {
    with_op!(op, elem, f => zip_arrays_with(f, l, r, real_rule))
}

fn zip_arrays_with(f: impl Fn(C64, C64) -> C64 + Sync, l: Operand, r: Operand, real_rule: bool) -> Value {
    let (mut out, flags) = match (l, r) {
        (Operand::Owned(mut a), r) if unique(&a) => {
            let b = data(r.get());
            let flags = drive(data_mut(&mut a), &|o, c: &mut [C64]| {
                let mut fl = NO_FLAGS;
                for (x, &y) in c.iter_mut().zip(&b[o..]) {
                    let z = f(*x, y);
                    track(&mut fl, *x, y, z);
                    *x = z;
                }
                fl
            });
            (a, flags)
        }
        (l, Operand::Owned(mut b)) if unique(&b) => {
            let a = data(l.get());
            let flags = drive(data_mut(&mut b), &|o, c: &mut [C64]| {
                let mut fl = NO_FLAGS;
                for (y, &x) in c.iter_mut().zip(&a[o..]) {
                    let z = f(x, *y);
                    track(&mut fl, x, *y, z);
                    *y = z;
                }
                fl
            });
            (b, flags)
        }
        (l, r) => {
            let (a, b) = (data(l.get()), data(r.get()));
            let (out, flags) = drive_new(a.len(), &|o, c: &mut [MaybeUninit<C64>]| {
                let mut fl = NO_FLAGS;
                for ((d, &x), &y) in c.iter_mut().zip(&a[o..]).zip(&b[o..]) {
                    let z = f(x, y);
                    track(&mut fl, x, y, z);
                    d.write(z);
                }
                fl
            });
            (shaped_like(l.get(), out), flags)
        }
    };
    // The real-output rule: `binop` zeroes the imaginary parts whenever both
    // inputs were real. Skipped when every imaginary part is already `+0.0`.
    if real_rule && flags.inputs_real && flags.output_needs_zeroing {
        drive(data_mut(&mut out), &|_, c: &mut [C64]| {
            for z in c {
                z.im = 0.0;
            }
            NO_FLAGS
        });
    }
    out
}

// ── Copies and reductions used by indexing and `sum` ───────────────────────

/// `m[r0..r0+nr, c0..c0+nc]` copied in parallel, or `None` when the block
/// is small or `m` is not in standard (row-major) layout.
pub(crate) fn par_block_copy(
    m: &rustlab_core::CMatrix,
    (r0, nr): (usize, usize),
    (c0, nc): (usize, usize),
) -> Option<rustlab_core::CMatrix> {
    let n = nr * nc;
    if n < PAR_MIN_LEN || nc == 0 {
        return None;
    }
    let src = m.as_slice()?;
    let stride = m.ncols();
    let (out, _) = drive_new(n, &|o, dst: &mut [MaybeUninit<C64>]| {
        // Copy the chunk row segment by row segment.
        let end = o + dst.len();
        let mut k = o;
        while k < end {
            let (i, j) = (k / nc, k % nc);
            let len = (nc - j).min(end - k);
            let from = (r0 + i) * stride + c0 + j;
            for (d, &x) in dst[k - o..k - o + len].iter_mut().zip(&src[from..from + len]) {
                d.write(x);
            }
            k += len;
        }
        NO_FLAGS
    });
    Some(ndarray::Array2::from_shape_vec((nr, nc), out).expect("nr × nc elements"))
}

/// Elements from which a transcendental map (`tanh`, `exp`, `gelu`, …)
/// runs in parallel, and its chunk length: at several ns per element these
/// pay for the threads far sooner than `+` does.
pub(crate) const PAR_MAP_MIN_LEN: usize = 1 << 14;
const PAR_MAP_CHUNK: usize = 1 << 12;

/// `f` of every element of `src`, in parallel from [`PAR_MAP_MIN_LEN`].
pub(crate) fn par_map(src: &[C64], f: &(dyn Fn(C64) -> C64 + Sync)) -> Vec<C64> {
    drive_new_sized(src.len(), PAR_MAP_MIN_LEN, PAR_MAP_CHUNK, &|o, c: &mut [MaybeUninit<C64>]| {
        for (d, &x) in c.iter_mut().zip(&src[o..]) {
            d.write(f(x));
        }
        NO_FLAGS
    })
    .0
}

/// A new length-`n` buffer filled by `k(offset, chunk)`, which must write
/// every element of each chunk it is given; in parallel chunks when long.
pub(crate) fn par_fill(n: usize, k: &(dyn Fn(usize, &mut [MaybeUninit<C64>]) + Sync)) -> Vec<C64> {
    drive_new(n, &|o, c: &mut [MaybeUninit<C64>]| {
        k(o, c);
        NO_FLAGS
    })
    .0
}

/// The transpose of the row-major `rows × cols` slice `src` (conjugated
/// when `conj`), as a row-major `cols × rows` buffer: element `j·rows + i`
/// is `src[i·cols + j]`. Written in 32 × 32 tiles so reads and writes stay
/// in cache, in parallel chunks when large.
pub(crate) fn par_transpose(src: &[C64], rows: usize, cols: usize, conj: bool) -> Vec<C64> {
    let n = rows * cols;
    assert_eq!(src.len(), n);
    if n == 0 {
        return Vec::new();
    }
    let k = |o: usize, dst: &mut [MaybeUninit<C64>]| {
        if conj {
            transpose_chunk::<true>(src, rows, cols, o, dst)
        } else {
            transpose_chunk::<false>(src, rows, cols, o, dst)
        }
    };
    par_fill(n, &k)
}

/// Output positions `o..o + dst.len()` of `par_transpose`: the whole
/// output rows in tiles, a partial first or last row element by element.
fn transpose_chunk<const CONJ: bool>(src: &[C64], rows: usize, cols: usize, o: usize, dst: &mut [MaybeUninit<C64>]) {
    const TILE: usize = 32;
    let f = |x: C64| if CONJ { x.conj() } else { x };
    let end = o + dst.len();
    // One output row's positions lo..hi (0-based within row j).
    let mut part = |j: usize, lo: usize, hi: usize| {
        for i in lo..hi {
            dst[j * rows + i - o].write(f(src[i * cols + j]));
        }
    };
    let (j_first, j_last) = (o / rows, (end - 1) / rows);
    if j_first == j_last {
        return part(j_first, o - j_first * rows, end - j_first * rows);
    }
    // Whole rows j_lo..j_hi.
    let mut j_lo = j_first;
    if o % rows != 0 {
        part(j_first, o % rows, rows);
        j_lo += 1;
    }
    let j_hi = end / rows;
    if end % rows != 0 {
        part(j_last, 0, end % rows);
    }
    // One side of each tile is strided. A stride with many factors of two
    // maps the tile's rows to a few cache sets and thrashes (129 × 512 read
    // across its 512 columns ran 1.5× slower than written across its 129
    // rows), so stride the side with fewer; on a tie, write contiguously.
    let write_runs = cols.trailing_zeros() <= rows.trailing_zeros();
    for j0 in (j_lo..j_hi).step_by(TILE) {
        let j1 = (j0 + TILE).min(j_hi);
        for i0 in (0..rows).step_by(TILE) {
            let i1 = (i0 + TILE).min(rows);
            if write_runs {
                for j in j0..j1 {
                    let out = &mut dst[j * rows + i0 - o..j * rows + i1 - o];
                    for (d, i) in out.iter_mut().zip(i0..i1) {
                        d.write(f(src[i * cols + j]));
                    }
                }
            } else {
                for i in i0..i1 {
                    let row = &src[i * cols + j0..i * cols + j1];
                    for (j, &x) in (j0..j1).zip(row) {
                        dst[j * rows + i - o].write(f(x));
                    }
                }
            }
        }
    }
}

/// `v[start..start+n]` copied in parallel, or `None` when short.
pub(crate) fn par_vector_copy(v: &rustlab_core::CVector, start: usize, n: usize) -> Option<rustlab_core::CVector> {
    if n < PAR_MIN_LEN {
        return None;
    }
    let src = &v.as_slice()?[start..start + n];
    let (out, _) = drive_new(n, &|o, dst: &mut [MaybeUninit<C64>]| {
        for (d, &x) in dst.iter_mut().zip(&src[o..]) {
            d.write(x);
        }
        NO_FLAGS
    });
    Some(ndarray::Array1::from_vec(out))
}

/// Column sums of a row-major matrix in parallel over column chunks, each
/// column added top to bottom from zero (the sequential order), or `None`
/// when small or not row-major.
pub(crate) fn par_column_sums(m: &rustlab_core::CMatrix) -> Option<rustlab_core::CVector> {
    if m.len() < PAR_MIN_LEN {
        return None;
    }
    let src = m.as_slice()?;
    let (nrows, ncols) = m.dim();
    let mut acc = vec![C64::new(0.0, 0.0); ncols];
    drive(&mut acc, &|c0, a: &mut [C64]| {
        for i in 0..nrows {
            let row = &src[i * ncols + c0..i * ncols + c0 + a.len()];
            for (x, &y) in a.iter_mut().zip(row) {
                *x += y;
            }
        }
        NO_FLAGS
    });
    Some(ndarray::Array1::from_vec(acc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::{Array1, Array2};

    fn vec_of(xs: &[(f64, f64)]) -> Value {
        Value::Vector(Array1::from_iter(xs.iter().map(|&(r, i)| C64::new(r, i))).into())
    }

    fn mat_of(rows: usize, cols: usize, xs: &[(f64, f64)]) -> Value {
        Value::Matrix(
            Array2::from_shape_vec((rows, cols), xs.iter().map(|&(r, i)| C64::new(r, i)).collect())
                .unwrap().into(),
        )
    }

    /// Exact bit-level equality, so `-0.0` vs `+0.0` and NaN payload
    /// differences count.
    fn bits(v: &Value) -> Vec<(u64, u64)> {
        let elems: Vec<C64> = match v {
            Value::Vector(a) => a.iter().copied().collect(),
            Value::Matrix(m) => m.iter().copied().collect(),
            Value::Scalar(x) => vec![C64::new(*x, 0.0)],
            Value::Complex(c) => vec![*c],
            other => panic!("unexpected {other:?}"),
        };
        elems.iter().map(|c| (c.re.to_bits(), c.im.to_bits())).collect()
    }

    fn same_kind(a: &Value, b: &Value) -> bool {
        std::mem::discriminant(a) == std::mem::discriminant(b)
    }

    /// A copy of `v` with its own buffer, so an owned operand made from it
    /// is unique and gets written in place. (`v.clone()` shares the buffer.)
    fn fresh(v: &Value) -> Value {
        match v {
            Value::Vector(a) => Value::Vector(Shared::new((**a).clone())),
            Value::Matrix(m) => Value::Matrix(Shared::new((**m).clone())),
            other => other.clone(),
        }
    }

    /// Every operand mix (borrowed, owned and unique, owned but sharing the
    /// buffer of `l` / `r`, on each side) must give exactly what
    /// `Value::binop` gives, and must leave `l` and `r` unchanged.
    fn side(v: &Value, k: usize) -> Operand<'_> {
        match k {
            0 => Operand::Borrowed(v),
            1 => Operand::Owned(fresh(v)),
            _ => Operand::Owned(v.clone()),
        }
    }

    fn check(op: BinOp, l: &Value, r: &Value) {
        let want = Value::binop(op, fresh(l), fresh(r));
        let (l_bits, r_bits) = (bits(l), bits(r));
        for (i, j) in (0..3).flat_map(|i| (0..3).map(move |j| (i, j))) {
            let got = binop(op, side(l, i), side(r, j));
            assert_eq!(bits(l), l_bits, "{op:?} wrote into a shared left operand");
            assert_eq!(bits(r), r_bits, "{op:?} wrote into a shared right operand");
            match (&want, &got) {
                (Ok(w), Ok(g)) => {
                    assert!(same_kind(w, g), "{op:?}: kind {w:?} vs {g:?}");
                    assert_eq!(bits(w), bits(g), "{op:?} on {l:?}, {r:?}");
                }
                (Err(w), Err(g)) => assert_eq!(w, g),
                _ => panic!("{op:?}: {want:?} vs {got:?}"),
            }
        }
    }

    const OPS: [BinOp; 10] = [
        BinOp::Add,
        BinOp::Sub,
        BinOp::Mul,
        BinOp::Div,
        BinOp::Pow,
        BinOp::ElemMul,
        BinOp::ElemDiv,
        BinOp::ElemPow,
        BinOp::Lt,
        BinOp::Eq,
    ];

    #[test]
    fn matches_binop_bit_for_bit() {
        let real_v = vec_of(&[(1.5, 0.0), (-2.0, 0.0), (0.0, 0.0), (-0.0, 0.0), (3.0, -0.0)]);
        let real_w = vec_of(&[(-4.0, 0.0), (0.5, 0.0), (-1.0, 0.0), (2.0, 0.0), (-3.0, 0.0)]);
        let cplx_v = vec_of(&[(1.0, 2.0), (-2.0, 0.5), (0.0, -1.0), (3.0, 0.0), (1e-17, 1e-17)]);
        let special = vec_of(&[(f64::INFINITY, 0.0), (f64::NAN, 0.0), (-1.0, 0.0), (0.0, 0.0), (1e308, 0.0)]);
        let tiny_im = vec_of(&[(1.0, 1e-18), (2.0, -1e-18), (3.0, 0.0), (4.0, 0.0), (5.0, 0.0)]);
        let real_m = mat_of(2, 3, &[(1.0, 0.0), (-2.0, 0.0), (3.0, 0.0), (-0.5, 0.0), (0.0, 0.0), (7.0, 0.0)]);
        let real_n = mat_of(2, 3, &[(-1.0, 0.0), (4.0, 0.0), (-3.0, 0.0), (2.0, 0.0), (-5.0, 0.0), (0.25, 0.0)]);
        let cplx_m = mat_of(2, 3, &[(1.0, 1.0), (0.0, -2.0), (3.0, 0.0), (-1.0, 0.5), (2.0, 2.0), (0.0, 0.0)]);
        let row_m = mat_of(1, 3, &[(1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]);
        let scalars = [
            Value::Scalar(0.001),
            Value::Scalar(-2.0),
            Value::Scalar(0.0),
            Value::Scalar(2.0),
            Value::Scalar(0.5),
            Value::Complex(C64::new(1.0, -1.0)),
        ];
        let arrays = [&real_v, &real_w, &cplx_v, &special, &tiny_im, &real_m, &real_n, &cplx_m, &row_m];
        for op in OPS {
            for a in arrays {
                for b in arrays {
                    check(op, a, b);
                }
                for s in &scalars {
                    check(op, a, s);
                    check(op, s, a);
                }
            }
        }
    }

    /// Arrays above `PAR_MIN_LEN` take the parallel kernels; they must agree
    /// with `binop` bit for bit too, including the real-output fix-up pass
    /// (negative reals make `.*` produce `-0.0` imaginary parts).
    #[test]
    fn parallel_kernels_match_binop_bit_for_bit() {
        let n = PAR_MIN_LEN + PAR_CHUNK / 2 + 3; // a ragged last chunk
        let wave = |k: usize, phase: f64| ((k as f64) * 0.37 + phase).sin() * 3.0;
        let real_a: Vec<(f64, f64)> = (0..n).map(|k| (wave(k, 0.0), 0.0)).collect();
        let real_b: Vec<(f64, f64)> = (0..n).map(|k| (wave(k, 1.0) + 0.1, 0.0)).collect();
        let mut cplx: Vec<(f64, f64)> = (0..n).map(|k| (wave(k, 2.0), wave(k, 3.0))).collect();
        cplx[17] = (f64::INFINITY, 0.0);
        let va = vec_of(&real_a);
        let vb = vec_of(&real_b);
        let vc = vec_of(&cplx);
        let rows = 3;
        let cols = n / rows;
        let ma = mat_of(rows, cols, &real_a[..rows * cols]);
        let mc = mat_of(rows, cols, &cplx[..rows * cols]);
        for op in [BinOp::Add, BinOp::Sub, BinOp::ElemMul, BinOp::ElemDiv, BinOp::ElemPow] {
            check(op, &va, &vb);
            check(op, &va, &vc);
            check(op, &ma, &mc);
            check(op, &ma, &ma);
        }
        for op in [BinOp::Mul, BinOp::Div, BinOp::Sub, BinOp::ElemPow] {
            check(op, &va, &Value::Scalar(-0.5));
            check(op, &Value::Scalar(2.0), &vc);
            check(op, &mc, &Value::Complex(C64::new(0.5, -2.0)));
        }
    }

    #[test]
    fn parallel_copies_and_column_sums_match_sequential() {
        let (rows, cols) = (7, PAR_MIN_LEN / 5 + 11);
        let m = Array2::from_shape_fn((rows, cols), |(i, j)| {
            C64::new((i * 31 + j) as f64 * 0.25 - 3.0, ((j % 7) as f64) - 3.0)
        });
        for &(r0, nr, c0, nc) in &[(0, rows, 0, cols), (1, 5, 3, cols - 10), (0, rows, 1, cols - 2)] {
            let got = par_block_copy(&m, (r0, nr), (c0, nc)).expect("large block takes the parallel path");
            assert_eq!(got, m.slice(ndarray::s![r0..r0 + nr, c0..c0 + nc]).to_owned());
        }
        assert!(par_block_copy(&m, (0, 1), (0, 10)).is_none(), "small blocks stay sequential");

        let v = Array1::from_iter((0..PAR_MIN_LEN + 999).map(|k| C64::new(k as f64, -(k as f64))));
        let got = par_vector_copy(&v, 5, PAR_MIN_LEN + 3).unwrap();
        assert_eq!(got, v.slice(ndarray::s![5..5 + PAR_MIN_LEN + 3]).to_owned());

        let sums = par_column_sums(&m).unwrap();
        for j in [0, 1, cols / 2, cols - 1] {
            let want: C64 = m.column(j).iter().copied().sum();
            assert_eq!(sums[j].re.to_bits(), want.re.to_bits());
            assert_eq!(sums[j].im.to_bits(), want.im.to_bits());
        }
    }

    /// A matrix not in row-major contiguous layout takes `binop` unchanged.
    #[test]
    fn non_contiguous_operands_fall_back_to_binop() {
        use ndarray::ShapeBuilder;
        let data: Vec<C64> = (0..6).map(|k| C64::new(k as f64 - 2.5, 0.0)).collect();
        let fortran = Value::Matrix(Array2::from_shape_vec((2, 3).f(), data).unwrap().into());
        assert!(!is_contiguous(&fortran));
        let row = mat_of(2, 3, &[(1.0, 0.0), (-2.0, 0.0), (3.0, 0.0), (0.5, 0.0), (-1.0, 0.0), (2.0, 0.0)]);
        for op in OPS {
            check(op, &fortran, &row);
            check(op, &row, &fortran);
            check(op, &fortran, &Value::Scalar(-1.5));
            check(op, &Value::Scalar(2.0), &fortran);
        }
    }

    #[test]
    fn owned_left_operand_is_reused() {
        let v = vec_of(&[(1.0, 0.0), (2.0, 0.0)]);
        let ptr = match &v {
            Value::Vector(a) => a.as_ptr(),
            _ => unreachable!(),
        };
        let out = binop(BinOp::Mul, Operand::Owned(v), Operand::Owned(Value::Scalar(3.0))).unwrap();
        match out {
            Value::Vector(a) => assert_eq!(a.as_ptr(), ptr, "result should reuse the temporary"),
            _ => panic!(),
        }
    }

    #[test]
    fn shared_owned_operand_is_not_written() {
        let v = vec_of(&[(1.0, 0.0), (2.0, 0.0)]);
        let out = binop(BinOp::Mul, Operand::Owned(v.clone()), Operand::Owned(Value::Scalar(3.0))).unwrap();
        assert_eq!(bits(&v), bits(&vec_of(&[(1.0, 0.0), (2.0, 0.0)])));
        assert_eq!(bits(&out), bits(&vec_of(&[(3.0, 0.0), (6.0, 0.0)])));
    }

    #[test]
    fn par_transpose_matches_the_definition() {
        // Serial, and parallel with output rows split across chunks.
        for (rows, cols) in [(1, 1), (3, 5), (33, 65), (7, PAR_MIN_LEN / 7 + 3), (PAR_MIN_LEN / 5 + 1, 5)] {
            let src: Vec<C64> = (0..rows * cols).map(|k| C64::new(k as f64, 0.5 - k as f64)).collect();
            for conj in [false, true] {
                let got = par_transpose(&src, rows, cols, conj);
                assert_eq!(got.len(), rows * cols);
                for i in 0..rows {
                    for j in 0..cols {
                        let x = src[i * cols + j];
                        let want = if conj { x.conj() } else { x };
                        assert_eq!(got[j * rows + i], want, "{rows}×{cols} ({i}, {j}) conj={conj}");
                    }
                }
            }
        }
    }
}

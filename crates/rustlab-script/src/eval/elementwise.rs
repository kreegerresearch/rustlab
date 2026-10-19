//! Fast paths for elementwise binary operators on dense arrays.
//!
//! [`Value::binop`] takes its operands by value, so `theta - 0.001 * g`
//! used to copy `theta` and `g` out of the workspace, allocate a temporary
//! for `0.001 * g`, and allocate again for the result. Here an operand is
//! either a borrowed workspace value or an owned temporary. The common
//! array cases (array ∘ scalar, and two arrays of the same shape) are
//! computed in one pass, written into an owned temporary of the right shape
//! when there is one, and otherwise into a single new array. Everything
//! else falls back to [`Value::binop`] unchanged.
//!
//! Results are bit-identical to [`Value::binop`]: the same complex
//! operation is applied per element, and the "real inputs give real
//! outputs" rule (zero the imaginary part when both inputs are real) is
//! applied under exactly the conditions `binop` applies it.

use super::value::Value;
use crate::ast::BinOp;
use ndarray::{Array, ArrayBase, Data, DataMut, Dimension, Zip};
use rayon::prelude::*;
use rustlab_core::C64;

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
            if is_scalar_op(op) =>
        {
            let s = scalar_of(r.get());
            Ok(with_op!(op, scalar, f => map_array(l, move |a| f(a, s))))
        }
        (Value::Scalar(_) | Value::Complex(_), Value::Vector(_) | Value::Matrix(_))
            if is_scalar_op(op) =>
        {
            let s = scalar_of(l.get());
            Ok(with_op!(op, scalar, f => map_array(r, move |a| f(s, a))))
        }
        // Two vectors of the same length. `binop` zeroes the imaginary part
        // of real inputs for `.*`, `./`, `.^` only.
        (Value::Vector(a), Value::Vector(b))
            if a.len() == b.len() && matches!(op, Add | Sub | ElemMul | ElemDiv | ElemPow) =>
        {
            let real_rule = matches!(op, ElemMul | ElemDiv | ElemPow);
            Ok(zip_arrays(op, l, r, real_rule))
        }
        // Two matrices of the same shape (no broadcasting). `binop` goes
        // through `elementwise_with_broadcast`, which zeroes the imaginary
        // part of real inputs for every elementwise op.
        (Value::Matrix(a), Value::Matrix(b))
            if a.dim() == b.dim() && matches!(op, Add | Sub | ElemMul | ElemDiv | ElemPow) =>
        {
            Ok(zip_arrays(op, l, r, true))
        }
        // Matrix product: same check and kernel as `binop`, without copying
        // the operands first. A dimension mismatch takes the fallback so the
        // error message is `binop`'s.
        (Value::Matrix(a), Value::Matrix(b)) if op == Mul && a.ncols() == b.nrows() => Ok(
            Value::Matrix(rustlab_core::matmul::matmul(&a.view(), &b.view())),
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
            Div | ElemDiv => { let $f = |x: C64, y: C64| x / y; $body }
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
            ElemDiv => { let $f = |x: C64, y: C64| x / y; $body }
            ElemPow => { let $f = |x: C64, y: C64| Value::pow_c64(x, y); $body }
            _ => unreachable!("checked by the caller"),
        }
    }};
}
use with_op;

/// Apply `f` to every element of an array operand, in place when it is an
/// owned temporary.
fn map_array(a: Operand, f: impl Fn(C64) -> C64 + Sync) -> Value {
    match a {
        Operand::Owned(Value::Vector(mut v)) => {
            map_inplace(&mut v, f);
            Value::Vector(v)
        }
        Operand::Owned(Value::Matrix(mut m)) => {
            map_inplace(&mut m, f);
            Value::Matrix(m)
        }
        Operand::Borrowed(Value::Vector(v)) => Value::Vector(map_new(v, f)),
        Operand::Borrowed(Value::Matrix(m)) => Value::Matrix(map_new(m, f)),
        _ => unreachable!("checked by the caller"),
    }
}

fn map_inplace<D: Dimension>(a: &mut Array<C64, D>, f: impl Fn(C64) -> C64 + Sync) {
    match a.as_slice_mut() {
        Some(xs) if xs.len() >= PAR_MIN_LEN => {
            xs.par_chunks_mut(PAR_CHUNK).for_each(|c| c.iter_mut().for_each(|x| *x = f(*x)))
        }
        _ => a.mapv_inplace(f),
    }
}

fn map_new<D: Dimension>(a: &Array<C64, D>, f: impl Fn(C64) -> C64 + Sync) -> Array<C64, D> {
    match a.as_slice() {
        Some(xs) if xs.len() >= PAR_MIN_LEN => {
            let out = fill_par(xs.len(), |range, dst| {
                for (d, &x) in dst.iter_mut().zip(&xs[range]) {
                    d.write(f(x));
                }
            });
            Array::from_shape_vec(a.raw_dim(), out).expect("same length and C order as the input")
        }
        _ => a.mapv(f),
    }
}

/// Build a length-`n` vector in parallel: `write(range, dst)` must
/// initialise every element of `dst`, which is the output for `range`.
fn fill_par(n: usize, write: impl Fn(std::ops::Range<usize>, &mut [std::mem::MaybeUninit<C64>]) + Sync) -> Vec<C64> {
    let mut out: Vec<C64> = Vec::with_capacity(n);
    out.spare_capacity_mut()[..n]
        .par_chunks_mut(PAR_CHUNK)
        .enumerate()
        .for_each(|(k, dst)| {
            let start = k * PAR_CHUNK;
            write(start..start + dst.len(), dst)
        });
    // SAFETY: the chunks cover `0..n` exactly once and `write` initialises
    // every element of each chunk it is given.
    unsafe { out.set_len(n) };
    out
}

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
    let out = fill_par(n, |range, dst| {
        // Copy the chunk row segment by row segment.
        let mut k = range.start;
        while k < range.end {
            let (i, j) = (k / nc, k % nc);
            let len = (nc - j).min(range.end - k);
            let from = (r0 + i) * stride + c0 + j;
            for (d, &x) in dst[k - range.start..k - range.start + len].iter_mut().zip(&src[from..from + len]) {
                d.write(x);
            }
            k += len;
        }
    });
    Some(ndarray::Array2::from_shape_vec((nr, nc), out).expect("nr × nc elements"))
}

/// `v[start..start+n]` copied in parallel, or `None` when short.
pub(crate) fn par_vector_copy(v: &rustlab_core::CVector, start: usize, n: usize) -> Option<rustlab_core::CVector> {
    if n < PAR_MIN_LEN {
        return None;
    }
    let src = &v.as_slice()?[start..start + n];
    let out = fill_par(n, |range, dst| {
        for (d, &x) in dst.iter_mut().zip(&src[range]) {
            d.write(x);
        }
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
    acc.par_chunks_mut(PAR_CHUNK).enumerate().for_each(|(k, a)| {
        let c0 = k * PAR_CHUNK;
        for i in 0..nrows {
            let row = &src[i * ncols + c0..i * ncols + c0 + a.len()];
            for (x, &y) in a.iter_mut().zip(row) {
                *x += y;
            }
        }
    });
    Some(ndarray::Array1::from_vec(acc))
}

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

/// Combine two same-shape arrays elementwise. Writes into whichever operand
/// is an owned temporary, else allocates once. With `real_rule`, zeroes the
/// imaginary parts of the result when both inputs were real.
fn zip_arrays(op: BinOp, l: Operand, r: Operand, real_rule: bool) -> Value {
    with_op!(op, elem, f => zip_arrays_with(f, l, r, real_rule))
}

fn zip_arrays_with(f: impl Fn(C64, C64) -> C64 + Copy + Sync, l: Operand, r: Operand, real_rule: bool) -> Value {
    match (l, r) {
        (Operand::Owned(Value::Vector(mut a)), r) => {
            let b = vector_ref(r.get());
            let flags = zip_into_left(&mut a, b, f);
            Value::Vector(finish(a, flags, real_rule))
        }
        (l, Operand::Owned(Value::Vector(mut b))) => {
            let a = vector_ref(l.get());
            let flags = zip_into_right(a, &mut b, f);
            Value::Vector(finish(b, flags, real_rule))
        }
        (Operand::Owned(Value::Matrix(mut a)), r) => {
            let b = matrix_ref(r.get());
            let flags = zip_into_left(&mut a, b, f);
            Value::Matrix(finish(a, flags, real_rule))
        }
        (l, Operand::Owned(Value::Matrix(mut b))) => {
            let a = matrix_ref(l.get());
            let flags = zip_into_right(a, &mut b, f);
            Value::Matrix(finish(b, flags, real_rule))
        }
        (l, r) => match (l.get(), r.get()) {
            (Value::Vector(a), Value::Vector(b)) => {
                let (out, flags) = zip_new(a, b, f);
                Value::Vector(finish(out, flags, real_rule))
            }
            (Value::Matrix(a), Value::Matrix(b)) => {
                let (out, flags) = zip_new(a, b, f);
                Value::Matrix(finish(out, flags, real_rule))
            }
            _ => unreachable!("checked by the caller"),
        },
    }
}

fn vector_ref(v: &Value) -> &rustlab_core::CVector {
    match v {
        Value::Vector(v) => v,
        _ => unreachable!("checked by the caller"),
    }
}

fn matrix_ref(v: &Value) -> &rustlab_core::CMatrix {
    match v {
        Value::Matrix(m) => m,
        _ => unreachable!("checked by the caller"),
    }
}

const fn new_flags() -> RealFlags {
    RealFlags { inputs_real: true, output_needs_zeroing: false }
}

#[inline(always)]
fn track(flags: &mut RealFlags, x: C64, y: C64, z: C64) {
    flags.inputs_real &= x.im.abs() < f64::EPSILON && y.im.abs() < f64::EPSILON;
    flags.output_needs_zeroing |= z.im.to_bits() != 0;
}

fn combine(f: &RealFlags, g: RealFlags) -> RealFlags {
    RealFlags {
        inputs_real: f.inputs_real && g.inputs_real,
        output_needs_zeroing: f.output_needs_zeroing || g.output_needs_zeroing,
    }
}

/// `a ← f(a, b)`, returning the real-output flags.
fn zip_into_left<S, T, D>(a: &mut ArrayBase<S, D>, b: &ArrayBase<T, D>, f: impl Fn(C64, C64) -> C64 + Sync) -> RealFlags
where
    S: DataMut<Elem = C64>,
    T: Data<Elem = C64>,
    D: Dimension,
{
    if a.len() >= PAR_MIN_LEN {
        if let (Some(xs), Some(ys)) = (a.as_slice_mut(), b.as_slice()) {
            return xs
                .par_chunks_mut(PAR_CHUNK)
                .zip(ys.par_chunks(PAR_CHUNK))
                .map(|(xc, yc)| {
                    let mut flags = new_flags();
                    for (x, &y) in xc.iter_mut().zip(yc) {
                        let z = f(*x, y);
                        track(&mut flags, *x, y, z);
                        *x = z;
                    }
                    flags
                })
                .reduce(new_flags, |p, q| combine(&p, q));
        }
    }
    let mut flags = new_flags();
    Zip::from(a).and(b).for_each(|x, &y| {
        let z = f(*x, y);
        track(&mut flags, *x, y, z);
        *x = z;
    });
    flags
}

/// `b ← f(a, b)`, returning the real-output flags.
fn zip_into_right<S, T, D>(a: &ArrayBase<S, D>, b: &mut ArrayBase<T, D>, f: impl Fn(C64, C64) -> C64 + Sync) -> RealFlags
where
    S: Data<Elem = C64>,
    T: DataMut<Elem = C64>,
    D: Dimension,
{
    zip_into_left(b, a, move |y, x| f(x, y))
}

fn zip_new<D: Dimension>(
    a: &Array<C64, D>,
    b: &Array<C64, D>,
    f: impl Fn(C64, C64) -> C64 + Sync,
) -> (Array<C64, D>, RealFlags) {
    if a.len() >= PAR_MIN_LEN {
        if let (Some(xs), Some(ys)) = (a.as_slice(), b.as_slice()) {
            let n = xs.len();
            let mut out: Vec<C64> = Vec::with_capacity(n);
            let flags = out.spare_capacity_mut()[..n]
                .par_chunks_mut(PAR_CHUNK)
                .enumerate()
                .map(|(k, dst)| {
                    let start = k * PAR_CHUNK;
                    let mut flags = new_flags();
                    for ((d, &x), &y) in dst.iter_mut().zip(&xs[start..]).zip(&ys[start..]) {
                        let z = f(x, y);
                        track(&mut flags, x, y, z);
                        d.write(z);
                    }
                    flags
                })
                .reduce(new_flags, |p, q| combine(&p, q));
            // SAFETY: the chunks cover `0..n` once and each loop writes every
            // element of its chunk (`dst` is no longer than the inputs' tail).
            unsafe { out.set_len(n) };
            let out = Array::from_shape_vec(a.raw_dim(), out).expect("same length and C order as the input");
            return (out, flags);
        }
    }
    let mut flags = new_flags();
    let out = Zip::from(a).and(b).map_collect(|&x, &y| {
        let z = f(x, y);
        track(&mut flags, x, y, z);
        z
    });
    (out, flags)
}

/// Apply the real-output rule: zero the imaginary parts when both inputs
/// were real, which `binop` does unconditionally in that case. Skipped when
/// every imaginary part is already `+0.0`, since zeroing would not change it.
fn finish<D: Dimension>(mut out: ndarray::Array<C64, D>, flags: RealFlags, real_rule: bool) -> ndarray::Array<C64, D> {
    if real_rule && flags.inputs_real && flags.output_needs_zeroing {
        match out.as_slice_mut() {
            Some(xs) if xs.len() >= PAR_MIN_LEN => {
                xs.par_chunks_mut(PAR_CHUNK).for_each(|c| c.iter_mut().for_each(|z| z.im = 0.0))
            }
            _ => out.iter_mut().for_each(|c| c.im = 0.0),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::{Array1, Array2};

    fn vec_of(xs: &[(f64, f64)]) -> Value {
        Value::Vector(Array1::from_iter(xs.iter().map(|&(r, i)| C64::new(r, i))))
    }

    fn mat_of(rows: usize, cols: usize, xs: &[(f64, f64)]) -> Value {
        Value::Matrix(
            Array2::from_shape_vec((rows, cols), xs.iter().map(|&(r, i)| C64::new(r, i)).collect())
                .unwrap(),
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

    /// Every operand mix (borrowed / owned on each side) must give exactly
    /// what `Value::binop` gives.
    fn check(op: BinOp, l: &Value, r: &Value) {
        let want = Value::binop(op, l.clone(), r.clone());
        let combos: [(Operand, Operand); 4] = [
            (Operand::Borrowed(l), Operand::Borrowed(r)),
            (Operand::Owned(l.clone()), Operand::Borrowed(r)),
            (Operand::Borrowed(l), Operand::Owned(r.clone())),
            (Operand::Owned(l.clone()), Operand::Owned(r.clone())),
        ];
        for (a, b) in combos {
            let got = binop(op, a, b);
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
}

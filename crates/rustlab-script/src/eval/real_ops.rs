//! Binary operators on real-stored arrays.
//!
//! When every array operand is real-stored (`Shared::as_real`) and every
//! scalar operand is real, the operators here compute in `f64` and return
//! real storage, half the bytes of the complex path and with no widening of
//! the inputs. Each result is bit for bit the real part of what the complex
//! path computes, and the complex path's imaginary part is zero in exactly
//! these cases. Anything else returns `None`, and the caller takes the
//! complex path:
//!
//! - a complex scalar, or a complex-stored operand;
//! - `array .^ scalar` and `scalar .^ array` whose result is complex (a
//!   negative base with a non-integer exponent). Between two arrays, `.^`
//!   keeps the real part, as the complex path's real-output rule does;
//! - shapes that do not broadcast, so the error message stays the complex
//!   path's.
//!
//! Kernels are small loops over contiguous rows, parallel (rayon) from
//! `PAR_MIN_LEN` elements, like `elementwise.rs`. Results do not depend on
//! the split.

use super::elementwise::{Operand, PAR_CHUNK, PAR_MIN_LEN};
use super::shared::Shared;
use super::value::Value;
use crate::ast::BinOp;
use ndarray::{Array1, Array2};
use rayon::prelude::*;
use rustlab_core::C64;

/// A real operand: an array as row-major `rows × cols` data (a vector is a
/// 1 × n row), or a scalar.
#[derive(Clone, Copy)]
enum R<'a> {
    Vec(&'a [f64]),
    Mat(&'a [f64], usize, usize),
    Scalar(f64),
}

impl<'a> R<'a> {
    fn of(v: &'a Value) -> Option<R<'a>> {
        match v {
            Value::Scalar(x) => Some(R::Scalar(*x)),
            Value::Vector(a) => Shared::real_slice(a).map(R::Vec),
            Value::Matrix(m) => {
                let s = Shared::shape(m);
                Shared::real_slice(m).map(|d| R::Mat(d, s[0], s[1]))
            }
            _ => None,
        }
    }

    /// `(data, rows, cols)` as a matrix; a vector is one row.
    fn grid(self) -> Option<(&'a [f64], usize, usize)> {
        match self {
            R::Vec(d) => Some((d, 1, d.len())),
            R::Mat(d, r, c) => Some((d, r, c)),
            R::Scalar(_) => None,
        }
    }
}

/// The real part of the complex path's operator on real inputs.
macro_rules! with_rop {
    ($op:expr, $f:ident => $body:expr) => {{
        use BinOp::*;
        match $op {
            Add => { let $f = |x: f64, y: f64| x + y; $body }
            Sub => { let $f = |x: f64, y: f64| x - y; $body }
            Mul | ElemMul => { let $f = |x: f64, y: f64| x * y; $body }
            Div | ElemDiv => { let $f = |x: f64, y: f64| x / y; $body }
            Pow | ElemPow => { let $f = pow_re; $body }
            _ => unreachable!("checked by the caller"),
        }
    }};
}

/// `l op r` in real arithmetic, writing into an owned real temporary when
/// one has the result's shape, or the operands back when this module does
/// not cover the case (see the module notes).
pub(crate) fn binop<'a>(op: BinOp, l: Operand<'a>, r: Operand<'a>) -> Result<Value, (Operand<'a>, Operand<'a>)> {
    let (l, r) = match in_place(op, l, r) {
        Ok(v) => return Ok(v),
        Err(lr) => lr,
    };
    match binop_ref(op, &l, &r) {
        Some(v) => Ok(v),
        None => Err((l, r)),
    }
}

/// The real data of an owned array nothing else refers to.
fn owned_real<'o>(o: &'o mut Operand<'_>) -> Option<&'o mut [f64]> {
    match o {
        Operand::Owned(Value::Vector(a)) => Shared::get_mut_real(a)?.as_slice_mut(),
        Operand::Owned(Value::Matrix(m)) => Shared::get_mut_real(m)?.as_slice_mut(),
        _ => None,
    }
}

fn into_value(o: Operand) -> Value {
    match o {
        Operand::Owned(v) => v,
        Operand::Borrowed(_) => unreachable!("only owned operands are written in place"),
    }
}

/// Array ∘ scalar and same-shape array ∘ array where an operand is an owned,
/// unique, real-stored temporary: the result overwrites it. The same
/// operations and the same bits as [`binop_ref`].
fn in_place<'a>(op: BinOp, mut l: Operand<'a>, mut r: Operand<'a>) -> Result<Value, (Operand<'a>, Operand<'a>)> {
    use BinOp::*;
    let shape = |v: &Value| match v {
        Value::Vector(a) if Shared::is_real(a) => Some((false, Shared::shape(a).to_vec())),
        Value::Matrix(m) if Shared::is_real(m) => Some((true, Shared::shape(m).to_vec())),
        _ => None,
    };
    let scalar = |v: &Value| match v {
        Value::Scalar(x) => Some(*x),
        _ => None,
    };
    let (ls, rs) = (shape(l.get()), shape(r.get()));
    // Array ∘ scalar.
    let arr_scalar = match (&ls, scalar(r.get()), scalar(l.get()), &rs) {
        (Some(_), Some(s), _, _) => Some((s, false)),
        (_, _, Some(s), Some(_)) => Some((s, true)),
        _ => None,
    };
    if let Some((s, scalar_left)) = arr_scalar {
        // `scalar / array` is right-division, not a scale.
        if scalar_left && op == BinOp::Div {
            return Err((l, r));
        }
        let arr = if scalar_left { &mut r } else { &mut l };
        let Some(d) = owned_real(arr) else { return Err((l, r)) };
        match op {
            Add | Sub | Mul | ElemMul | Div | ElemDiv => {}
            Pow | ElemPow if pow_is_real(d, s, scalar_left) => {}
            _ => return Err((l, r)),
        }
        with_rop!(op, f => {
            if scalar_left { update(d, |x| f(s, x)) } else { update(d, |x| f(x, s)) }
        });
        return Ok(into_value(if scalar_left { r } else { l }));
    }
    // Same shape, same kind (vector ∘ vector or matrix ∘ matrix).
    let (Some((lm, lsh)), Some((rm, rsh))) = (ls, rs) else { return Err((l, r)) };
    let ok_op = matches!(op, Add | Sub | ElemMul | ElemDiv | ElemPow) || (lm && op == Pow);
    if lm != rm || lsh != rsh || !ok_op {
        return Err((l, r));
    }
    let n: usize = lsh.iter().product();
    let full = |v: &Value| real_data(v).is_some_and(|d| d.len() == n);
    if !full(l.get()) || !full(r.get()) {
        return Err((l, r));
    }
    if owned_real(&mut l).is_some() {
        let y = real_data(r.get()).expect("checked");
        let x = owned_real(&mut l).expect("checked");
        with_rop!(op, f => zip_update(x, y, |a, b| f(a, b)));
        return Ok(into_value(l));
    }
    if owned_real(&mut r).is_some() {
        let x = real_data(l.get()).expect("checked");
        let y = owned_real(&mut r).expect("checked");
        with_rop!(op, f => zip_update(y, x, |b, a| f(a, b)));
        return Ok(into_value(r));
    }
    Err((l, r))
}

fn real_data(v: &Value) -> Option<&[f64]> {
    match v {
        Value::Vector(a) => Shared::real_slice(a),
        Value::Matrix(m) => Shared::real_slice(m),
        _ => None,
    }
}

// ── Drivers ─────────────────────────────────────────────────────────────────
//
// As in `elementwise.rs`: every parallel loop here goes through one of these
// two non-generic drivers with a `&dyn Fn` over one chunk, so rayon is
// instantiated once per driver, not once per operator and kernel (a closure
// handed straight to rayon costs 10–20 KB of binary). The dynamic call
// happens once per chunk, never per element.

/// `k(offset, chunk)` over `xs`: whole below `min_len`, else in parallel
/// chunks of `chunk` elements.
fn drive_f64(xs: &mut [f64], min_len: usize, chunk: usize, k: &(dyn Fn(usize, &mut [f64]) + Sync)) {
    if xs.len() < min_len {
        return k(0, xs);
    }
    xs.par_chunks_mut(chunk).enumerate().for_each(|(i, c)| k(i * chunk, c));
}

/// A new length-`n` buffer, each chunk filled once by `k`, which returns
/// how many of its slots it wrote. Published only when every chunk reports
/// its full length (no zero-fill, and never an uninitialised read).
pub(crate) fn fill_f64(
    n: usize,
    min_len: usize,
    chunk: usize,
    k: &(dyn Fn(usize, &mut [std::mem::MaybeUninit<f64>]) -> usize + Sync),
) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }
    let dst = &mut out.spare_capacity_mut()[..n];
    let run = |o: usize, c: &mut [std::mem::MaybeUninit<f64>]| {
        let want = c.len();
        let got = k(o, c);
        assert_eq!(got, want, "real kernel filled {got} of {want} slots");
    };
    if n < min_len {
        run(0, dst);
    } else {
        dst.par_chunks_mut(chunk).enumerate().for_each(|(i, c)| run(i * chunk, c));
    }
    // SAFETY: the chunks partition `0..n`, and each asserted above that its
    // kernel wrote every one of its slots (counted per write). `f64` is
    // `Copy`, so a panic before this point drops a len-0 Vec.
    unsafe { out.set_len(n) };
    out
}

/// Writes `f(src[o + k])` into each slot of `dst`, returning the count.
#[inline(always)]
fn fill_map(dst: &mut [std::mem::MaybeUninit<f64>], src: &[f64], f: impl Fn(f64) -> f64) -> usize {
    dst.iter_mut().zip(src).map(|(d, &x)| { d.write(f(x)); }).count()
}

/// `x = f(x)` for every element, in parallel from `PAR_MIN_LEN`.
fn update(xs: &mut [f64], f: impl Fn(f64) -> f64 + Sync) {
    drive_f64(xs, PAR_MIN_LEN, PAR_CHUNK, &|_, c| {
        for x in c {
            *x = f(*x);
        }
    });
}

/// `x = f(x, y)` elementwise, in parallel from `PAR_MIN_LEN`.
fn zip_update(xs: &mut [f64], ys: &[f64], f: impl Fn(f64, f64) -> f64 + Sync) {
    drive_f64(xs, PAR_MIN_LEN, PAR_CHUNK, &|o, c| {
        for (x, &y) in c.iter_mut().zip(&ys[o..]) {
            *x = f(*x, y);
        }
    });
}

/// `l op r` in real arithmetic from borrowed operands, or `None` when this
/// module does not cover the case (see the module notes).
fn binop_ref(op: BinOp, l: &Operand, r: &Operand) -> Option<Value> {
    use BinOp::*;
    let (a, b) = (R::of(l.get())?, R::of(r.get())?);
    if matches!((a, b), (R::Scalar(_), R::Scalar(_))) {
        return None;
    }
    if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) {
        return compare(op, a, b);
    }
    match (a, b) {
        (R::Scalar(s), arr) | (arr, R::Scalar(s)) => {
            let scalar_left = matches!(a, R::Scalar(_));
            let data = match arr {
                R::Vec(d) | R::Mat(d, _, _) => d,
                R::Scalar(_) => unreachable!("one side is an array"),
            };
            if scalar_left && op == Div {
                return None;
            }
            match op {
                Add | Sub | Mul | ElemMul | Div | ElemDiv => {}
                Pow | ElemPow if pow_is_real(data, s, scalar_left) => {}
                _ => return None,
            }
            let out = with_rop!(op, f => {
                if scalar_left { map(data, |x| f(s, x)) } else { map(data, |x| f(x, s)) }
            });
            Some(shaped(arr, out))
        }
        (R::Vec(x), R::Vec(y)) => {
            if x.len() != y.len() || !matches!(op, Add | Sub | ElemMul | ElemDiv | ElemPow) {
                return None;
            }
            let out = with_rop!(op, f => zip(x, y, f));
            Some(Value::real_vector(Array1::from_vec(out)))
        }
        // A real n×1 column and a same-length vector are element-wise.
        // Broadcasting them as n×1 against 1×n would build an n×n matrix.
        (R::Mat(d, r, 1), R::Vec(v)) if r == v.len() && is_column_elem(op) => {
            let out = with_rop!(op, f => zip(d, v, f));
            Some(Value::real_vector(Array1::from_vec(out)))
        }
        (R::Vec(v), R::Mat(d, r, 1)) if r == v.len() && is_column_elem(op) => {
            let out = with_rop!(op, f => zip(v, d, f));
            Some(Value::real_vector(Array1::from_vec(out)))
        }
        _ => {
            let (x, xr, xc) = a.grid()?;
            let (y, yr, yc) = b.grid()?;
            match op {
                Mul => product(a, b),
                Add | Sub | ElemMul | ElemDiv | ElemPow | Pow => {
                    // Matrix ∘ matrix `^` is elementwise in the complex path
                    // too; vector ∘ vector `^` was excluded above.
                    let (rows, cols) = (bdim(xr, yr)?, bdim(xc, yc)?);
                    let out = with_rop!(op, f => broadcast((x, xr, xc), (y, yr, yc), rows, cols, f));
                    Some(Value::real_matrix(Array2::from_shape_vec((rows, cols), out).ok()?))
                }
                _ => None,
            }
        }
    }
}

/// Whether `x .^ s` (or `s .^ x` when `scalar_left`) is real for every
/// element: an integer exponent, or a base that is `>= 0` (NaN is not, as
/// in `Value::pow_c64`).
fn pow_is_real(data: &[f64], s: f64, scalar_left: bool) -> bool {
    let int = |e: f64| e.fract() == 0.0 && e.abs() <= i32::MAX as f64;
    if scalar_left {
        s >= 0.0 || data.iter().all(|&e| int(e))
    } else {
        int(s) || data.iter().all(|&x| x >= 0.0)
    }
}

/// `Re(Value::pow_c64(x + 0i, y + 0i))`.
fn pow_re(x: f64, y: f64) -> f64 {
    if y.fract() == 0.0 && y.abs() <= i32::MAX as f64 {
        x.powi(y as i32)
    } else if x >= 0.0 {
        x.powf(y)
    } else {
        Value::pow_c64(C64::new(x, 0.0), C64::new(y, 0.0)).re
    }
}

fn is_column_elem(op: BinOp) -> bool {
    matches!(op, BinOp::Add | BinOp::Sub | BinOp::ElemMul | BinOp::ElemDiv | BinOp::ElemPow | BinOp::Pow)
}

/// Broadcast extent of one dimension (equal, or one side 1).
fn bdim(a: usize, b: usize) -> Option<usize> {
    if a == b {
        Some(a)
    } else if a == 1 {
        Some(b)
    } else if b == 1 {
        Some(a)
    } else {
        None
    }
}

fn shaped(like: R, data: Vec<f64>) -> Value {
    match like {
        R::Vec(_) => Value::real_vector(Array1::from_vec(data)),
        R::Mat(_, r, c) => Value::real_matrix(Array2::from_shape_vec((r, c), data).expect("same element count")),
        R::Scalar(_) => unreachable!("an array"),
    }
}

// ── Kernels ─────────────────────────────────────────────────────────────────

/// Rows of output per parallel task, chosen so a task has about
/// `PAR_MIN_LEN / 8` elements.
fn rows_per_task(cols: usize) -> usize {
    (PAR_MIN_LEN / 8 / cols.max(1)).max(1)
}

/// `f` of every element, in parallel from `min_len` elements (in chunks of
/// a quarter of that).
pub(crate) fn map_with(src: &[f64], min_len: usize, f: impl Fn(f64) -> f64 + Sync) -> Vec<f64> {
    fill_f64(src.len(), min_len, (min_len / 4).max(1), &|o, d| fill_map(d, &src[o..], &f))
}

/// The transpose of the row-major `rows × cols` slice: element `j·rows + i`
/// is `src[i·cols + j]`. In 32 × 32 tiles, parallel over blocks of output
/// rows when large; a copy, so the split never matters. One side of a tile
/// is strided, and a stride with many factors of two maps the tile to a few
/// cache sets, so the strided side is the one with fewer (as in
/// `elementwise::par_transpose`).
pub(crate) fn transpose_f64(src: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    use std::mem::MaybeUninit;
    const TILE: usize = 32;
    assert_eq!(src.len(), rows * cols);
    let n = rows * cols;
    let mut out: Vec<f64> = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }
    let write_runs = cols.trailing_zeros() <= rows.trailing_zeros();
    // One block of TILE output rows (source columns j0..j1), written once
    // per slot into spare capacity. Returns the slots written: the tiles
    // partition the block, so a full block is `(j1 - j0) · rows`.
    let block = |j0: usize, dst: &mut [MaybeUninit<f64>]| -> usize {
        let j1 = (j0 + TILE).min(cols);
        let mut written = 0;
        for i0 in (0..rows).step_by(TILE) {
            let i1 = (i0 + TILE).min(rows);
            if write_runs {
                for j in j0..j1 {
                    let row = &mut dst[(j - j0) * rows + i0..(j - j0) * rows + i1];
                    for (d, i) in row.iter_mut().zip(i0..i1) {
                        d.write(src[i * cols + j]);
                    }
                    written += i1 - i0;
                }
            } else {
                for i in i0..i1 {
                    let line = &src[i * cols + j0..i * cols + j1];
                    for (k, &x) in line.iter().enumerate() {
                        dst[k * rows + i].write(x);
                    }
                    written += line.len();
                }
            }
        }
        written
    };
    let dst = &mut out.spare_capacity_mut()[..n];
    let check = |got: usize, want: usize| assert_eq!(got, want, "transpose filled {got} of {want} slots");
    if n < PAR_MIN_LEN {
        for (b, d) in dst.chunks_mut(TILE * rows).enumerate() {
            let want = d.len();
            check(block(b * TILE, d), want);
        }
    } else {
        dst.par_chunks_mut(TILE * rows).enumerate().for_each(|(b, d)| {
            let want = d.len();
            check(block(b * TILE, d), want);
        });
    }
    // SAFETY: the blocks partition `0..n` and each asserted that it wrote
    // every one of its slots (distinct (i, j) pairs, one write each).
    // `f64` is `Copy`, so a panic drops a len-0 Vec.
    unsafe { out.set_len(n) };
    out
}

fn map(src: &[f64], f: impl Fn(f64) -> f64 + Sync) -> Vec<f64> {
    fill_f64(src.len(), PAR_MIN_LEN, PAR_CHUNK, &|o, d| fill_map(d, &src[o..], &f))
}

fn zip(x: &[f64], y: &[f64], f: impl Fn(f64, f64) -> f64 + Sync) -> Vec<f64> {
    fill_f64(x.len(), PAR_MIN_LEN, PAR_CHUNK, &|o, d| {
        d.iter_mut().zip(&x[o..]).zip(&y[o..]).map(|((s, &a), &b)| { s.write(f(a, b)); }).count()
    })
}

/// `f(x[i', j'], y[i'', j''])` over the broadcast `rows × cols` grid, where a
/// singleton dimension of either operand is repeated. Each output row is
/// one contiguous loop: a full row of each operand, or one element of a
/// column (`k × 1`) operand.
fn broadcast(
    (x, xr, xc): (&[f64], usize, usize),
    (y, yr, yc): (&[f64], usize, usize),
    rows: usize,
    cols: usize,
    f: impl Fn(f64, f64) -> f64 + Sync,
) -> Vec<f64> {
    let n = rows * cols;
    if n == 0 {
        return Vec::new();
    }
    let row = |i: usize, dst: &mut [std::mem::MaybeUninit<f64>]| -> usize {
        let xa = &x[if xr == 1 { 0 } else { i } * xc..][..xc];
        let ya = &y[if yr == 1 { 0 } else { i } * yc..][..yc];
        match (xc == cols, yc == cols) {
            (true, true) => dst.iter_mut().zip(xa).zip(ya).map(|((d, &a), &b)| { d.write(f(a, b)); }).count(),
            (true, false) => {
                let b = ya[0];
                dst.iter_mut().zip(xa).map(|(d, &a)| { d.write(f(a, b)); }).count()
            }
            (false, true) => {
                let a = xa[0];
                dst.iter_mut().zip(ya).map(|(d, &b)| { d.write(f(a, b)); }).count()
            }
            (false, false) => {
                let v = f(xa[0], ya[0]);
                dst.iter_mut().map(|d| { d.write(v); }).count()
            }
        }
    };
    // Chunks are whole rows, so a chunk's offset gives its first row.
    let per = rows_per_task(cols);
    fill_f64(n, PAR_MIN_LEN, per * cols, &|o, d| {
        d.chunks_mut(cols).enumerate().map(|(k, dst)| row(o / cols + k, dst)).sum()
    })
}

/// `*` between real arrays: matrix × matrix, vector × matrix (a row times
/// a matrix; 1 × 1 collapses to a scalar, as in the complex path), and an
/// `m × 1` matrix times a vector (outer product). `None` on a dimension
/// mismatch, so the complex path reports it.
fn product(a: R, b: R) -> Option<Value> {
    fn view(d: &[f64], r: usize, c: usize) -> Option<ndarray::ArrayView2<'_, f64>> {
        ndarray::ArrayView2::from_shape((r, c), d).ok()
    }
    match (a, b) {
        (R::Mat(x, xr, xc), R::Mat(y, yr, yc)) if xc == yr => {
            Some(Value::real_matrix(rustlab_core::matmul::matmul_real(&view(x, xr, xc)?, &view(y, yr, yc)?)))
        }
        (R::Vec(x), R::Mat(y, yr, yc)) if x.len() == yr => {
            let p = rustlab_core::matmul::matmul_real(&view(x, 1, x.len())?, &view(y, yr, yc)?);
            if yc == 1 {
                return Some(Value::Scalar(p[[0, 0]]));
            }
            Some(Value::real_vector(p.index_axis_move(ndarray::Axis(0), 0)))
        }
        (R::Mat(x, xr, 1), R::Vec(y)) => {
            // Outer product: `m[[r, 0]] * v[c]` per element, as the complex
            // path computes it.
            let mut out = Vec::with_capacity(xr * y.len());
            for &p in &x[..xr] {
                out.extend(y.iter().map(|&q| p * q));
            }
            Some(Value::real_matrix(Array2::from_shape_vec((xr, y.len()), out).ok()?))
        }
        _ => None,
    }
}

/// Comparison operators with a real array operand: 0/1 results of the
/// complex path's shapes (vector ∘ vector of equal length, matrix ∘ matrix
/// of equal shape, a vector against a 1 × n matrix, and either against a
/// scalar).
fn compare(op: BinOp, a: R, b: R) -> Option<Value> {
    use BinOp::*;
    let cmp = move |x: f64, y: f64| -> f64 {
        let t = match op {
            Eq => x == y,
            Ne => x != y,
            Lt => x < y,
            Le => x <= y,
            Gt => x > y,
            Ge => x >= y,
            _ => unreachable!("comparison"),
        };
        if t { 1.0 } else { 0.0 }
    };
    match (a, b) {
        (arr, R::Scalar(s)) => {
            let d = match arr {
                R::Vec(d) | R::Mat(d, _, _) => d,
                R::Scalar(_) => return None,
            };
            Some(shaped(arr, map(d, |x| cmp(x, s))))
        }
        (R::Scalar(s), arr) => {
            let d = match arr {
                R::Vec(d) | R::Mat(d, _, _) => d,
                R::Scalar(_) => return None,
            };
            Some(shaped(arr, map(d, |x| cmp(s, x))))
        }
        (R::Vec(x), R::Vec(y)) if x.len() == y.len() => Some(Value::real_vector(Array1::from_vec(zip(x, y, cmp)))),
        (R::Mat(x, r, c), R::Mat(y, r2, c2)) if (r, c) == (r2, c2) => {
            Some(Value::real_matrix(Array2::from_shape_vec((r, c), zip(x, y, cmp)).ok()?))
        }
        (R::Vec(x), R::Mat(y, 1, c)) | (R::Mat(x, 1, c), R::Vec(y)) if x.len() == c && y.len() == c => {
            Some(Value::real_matrix(Array2::from_shape_vec((1, c), zip(x, y, cmp)).ok()?))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::shared::set_real_storage;

    fn rv(xs: &[f64]) -> Value {
        Value::Vector(Shared::from_real(Array1::from_vec(xs.to_vec())))
    }

    fn rm(r: usize, c: usize, xs: &[f64]) -> Value {
        Value::Matrix(Shared::from_real(Array2::from_shape_vec((r, c), xs.to_vec()).unwrap()))
    }

    /// The same value complex-stored.
    fn widened(v: &Value) -> Value {
        match v {
            Value::Vector(a) => Value::Vector(Shared::new((**a).clone())),
            Value::Matrix(m) => Value::Matrix(Shared::new((**m).clone())),
            other => other.clone(),
        }
    }

    fn same(x: &Value, y: &Value) -> bool {
        let bits = |z: &C64| (z.re.to_bits(), z.im.to_bits(), z.re.is_nan());
        let eq = |a: &[C64], b: &[C64]| {
            a.len() == b.len()
                && a.iter().zip(b).all(|(p, q)| {
                    (p.re.is_nan() && q.re.is_nan() || bits(p).0 == bits(q).0) && p.im == q.im
                })
        };
        match (x, y) {
            (Value::Vector(a), Value::Vector(b)) => eq(a.as_slice().unwrap(), b.as_slice().unwrap()),
            (Value::Matrix(a), Value::Matrix(b)) => {
                a.dim() == b.dim() && eq(a.as_standard_layout().as_slice().unwrap(), b.as_standard_layout().as_slice().unwrap())
            }
            (Value::Scalar(a), Value::Scalar(b)) => a.to_bits() == b.to_bits() || a.is_nan() && b.is_nan(),
            _ => false,
        }
    }

    /// Every operator and shape against the complex path on the same
    /// numbers: real result, equal to the complex result's real part, and
    /// the complex result's imaginary part is zero.
    #[test]
    fn matches_the_complex_path() {
        use BinOp::*;
        set_real_storage(Some(true));
        let xs = [1.5, -2.0, 0.0, -0.0, 3.25, f64::INFINITY, -7.0, 0.5, f64::NAN, 2.0, -1.0, 4.0];
        let ys = [2.0, 0.5, -3.0, 0.0, -0.0, 1.0, 2.5, f64::NEG_INFINITY, 3.0, -0.5, 7.0, 0.25];
        let operands = [
            rv(&xs[..6]),
            rv(&ys[..6]),
            rm(2, 3, &xs[..6]),
            rm(2, 3, &ys[..6]),
            rm(1, 3, &ys[..3]),
            rm(2, 1, &ys[4..6]),
            rm(3, 2, &ys[6..12]),
            rm(1, 6, &ys[..6]),
            rm(6, 1, &xs[6..12]),
            Value::Scalar(2.0),
            Value::Scalar(-0.5),
            Value::Scalar(0.0),
            Value::Scalar(3.0),
        ];
        let ops = [Add, Sub, Mul, ElemMul, Div, ElemDiv, Pow, ElemPow, Eq, Ne, Lt, Le, Gt, Ge];
        let mut covered = 0;
        for op in ops {
            for l in &operands {
                for r in &operands {
                    let Some(got) = binop_ref(op, &Operand::Borrowed(l), &Operand::Borrowed(r)) else {
                        continue;
                    };
                    covered += 1;
                    let want = Value::binop(op, widened(l), widened(r))
                        .unwrap_or_else(|e| panic!("{op:?}: real path accepted what the complex path rejects: {e}"));
                    assert!(same(&got, &want), "{op:?} {l:?} {r:?}: {got:?} vs {want:?}");
                    match &got {
                        Value::Vector(a) => assert!(Shared::is_real(a)),
                        Value::Matrix(m) => assert!(Shared::is_real(m)),
                        _ => {}
                    }
                }
            }
        }
        assert!(covered > 400, "only {covered} cases took the real path");
        set_real_storage(None);
    }

    #[test]
    fn complex_cases_are_left_to_the_complex_path() {
        use BinOp::*;
        let neg = rv(&[-8.0, 8.0]);
        let third = Value::Scalar(1.0 / 3.0);
        assert!(binop_ref(ElemPow, &Operand::Borrowed(&neg), &Operand::Borrowed(&third)).is_none());
        let half = rv(&[0.5, 2.0]);
        let m2 = Value::Scalar(-2.0);
        assert!(binop_ref(ElemPow, &Operand::Borrowed(&m2), &Operand::Borrowed(&half)).is_none());
        let cplx = Value::Complex(C64::new(0.0, 1.0));
        assert!(binop_ref(Mul, &Operand::Borrowed(&neg), &Operand::Borrowed(&cplx)).is_none());
        let stored = Value::Vector(Shared::new(Array1::from_vec(vec![C64::new(1.0, 0.0)])));
        assert!(binop_ref(Add, &Operand::Borrowed(&stored), &Operand::Borrowed(&neg)).is_none());
        let a = rm(2, 3, &[1.0; 6]);
        let b = rm(2, 2, &[1.0; 4]);
        assert!(binop_ref(Mul, &Operand::Borrowed(&a), &Operand::Borrowed(&b)).is_none());
        assert!(binop_ref(Add, &Operand::Borrowed(&a), &Operand::Borrowed(&b)).is_none());
        set_real_storage(None);
    }

    #[test]
    fn owned_temporaries_are_overwritten_with_the_same_result() {
        use BinOp::*;
        set_real_storage(Some(true));
        let xs: Vec<f64> = (0..PAR_MIN_LEN + 13).map(|i| (i as f64 * 0.37).sin() * 4.0).collect();
        let ys: Vec<f64> = (0..PAR_MIN_LEN + 13).map(|i| (i as f64 * 0.11).cos() + 2.0).collect();
        for n in [5, PAR_MIN_LEN + 13] {
            let (x, y) = (rv(&xs[..n]), rv(&ys[..n]));
            let ptr = |v: &Value| match v {
                Value::Vector(a) => Shared::real_slice(a).unwrap().as_ptr(),
                _ => unreachable!(),
            };
            for op in [Add, Sub, Mul, ElemMul, Div, ElemDiv, Pow, ElemPow] {
                for s in [Value::Scalar(1.5), Value::Scalar(-2.0), Value::Scalar(3.0)] {
                    for left in [false, true] {
                        let (a, b) = if left { (s.clone(), y.clone()) } else { (y.clone(), s.clone()) };
                        let want = binop_ref(op, &Operand::Borrowed(&a), &Operand::Borrowed(&b));
                        let owned = rv(&ys[..n]);
                        let p = ptr(&owned);
                        let (l, r) = if left { (Operand::Borrowed(&s), Operand::Owned(owned)) } else { (Operand::Owned(owned), Operand::Borrowed(&s)) };
                        match (binop(op, l, r), want) {
                            (Ok(got), Some(want)) => {
                                assert!(same(&got, &want), "{op:?} {s:?} left={left}");
                                assert_eq!(ptr(&got), p, "{op:?}: not written in place");
                            }
                            (Err(_), None) => {}
                            (got, want) => panic!("{op:?} {s:?}: {:?} vs {want:?}", got.is_ok()),
                        }
                    }
                }
            }
            for op in [Add, Sub, ElemMul, ElemDiv, ElemPow] {
                let want = binop_ref(op, &Operand::Borrowed(&x), &Operand::Borrowed(&y)).unwrap();
                let owned = rv(&xs[..n]);
                let p = ptr(&owned);
                let got = binop(op, Operand::Owned(owned), Operand::Borrowed(&y)).ok().unwrap();
                assert!(same(&got, &want) && ptr(&got) == p, "{op:?} left");
                let owned = rv(&ys[..n]);
                let p = ptr(&owned);
                let got = binop(op, Operand::Borrowed(&x), Operand::Owned(owned)).ok().unwrap();
                assert!(same(&got, &want) && ptr(&got) == p, "{op:?} right");
            }
        }
    }

    #[test]
    fn transpose_matches_the_definition() {
        for (r, c) in [(1, 1), (3, 5), (32, 32), (33, 31), (129, 512), (512, 129), (600, 300)] {
            let src: Vec<f64> = (0..r * c).map(|k| k as f64 * 0.5 - 7.0).collect();
            let t = transpose_f64(&src, r, c);
            for i in 0..r {
                for j in 0..c {
                    assert_eq!(t[j * r + i], src[i * c + j], "{r}x{c} at ({i}, {j})");
                }
            }
        }
    }

    #[test]
    fn large_operands_match_the_sequential_result() {
        use BinOp::*;
        set_real_storage(Some(true));
        let n = PAR_MIN_LEN + 77;
        let xs: Vec<f64> = (0..n).map(|i| (i as f64).sin() * 3.0).collect();
        let ys: Vec<f64> = (0..n).map(|i| (i as f64 * 0.7).cos() + 1.5).collect();
        let (x, y) = (rv(&xs), rv(&ys));
        for op in [Add, Sub, ElemMul, ElemDiv, ElemPow] {
            let got = binop_ref(op, &Operand::Borrowed(&x), &Operand::Borrowed(&y)).unwrap();
            let want = Value::binop(op, widened(&x), widened(&y)).unwrap();
            assert!(same(&got, &want), "{op:?}");
        }
        let m = rm(n / 7, 7, &xs[..n / 7 * 7]);
        let row = rm(1, 7, &ys[..7]);
        let col = rm(n / 7, 1, &ys[..n / 7]);
        for (l, r) in [(&m, &row), (&m, &col), (&col, &row)] {
            let got = binop_ref(ElemMul, &Operand::Borrowed(l), &Operand::Borrowed(r)).unwrap();
            let want = Value::binop(ElemMul, widened(l), widened(r)).unwrap();
            assert!(same(&got, &want));
        }
    }
}

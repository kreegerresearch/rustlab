//! Indexing reads of real-stored arrays, and index values read without
//! widening them.
//!
//! [`Value::real_index`] is the real twin of `index_1d` / `index_2d` for a
//! real-stored vector or matrix: the same positions, the same result shapes
//! (a scalar, a vector, or a matrix), and `f64` storage for the result. It
//! returns `None` for anything it does not cover and for any invalid index,
//! so the complex path runs and reports the error in its own words.
//!
//! [`Elems`] reads an index vector or matrix as it is stored. Its rules are
//! the complex path's exactly: a run `a:b` needs exact integers and a zero
//! imaginary part, a logical mask needs every entry 0 or 1 with
//! `|im| <= 1e-12`, and a position list uses the real parts.

use super::shared::Shared;
use super::value::Value;
use ndarray::{Array1, Array2};
use rustlab_core::{CMatrix, CVector, C64};

/// The elements of an index vector or matrix, in row-major order.
#[derive(Clone, Copy)]
pub(crate) enum Elems<'a> {
    Re(&'a [f64]),
    C(&'a [C64]),
}

impl<'a> Elems<'a> {
    /// `None` for a layout that is not one row-major slice.
    pub(crate) fn of_vector(v: &'a Shared<CVector>) -> Option<Elems<'a>> {
        if Shared::is_real(v) {
            return Shared::real_slice(v).map(Elems::Re);
        }
        v.as_slice().map(Elems::C)
    }

    pub(crate) fn of_matrix(m: &'a Shared<CMatrix>) -> Option<Elems<'a>> {
        if Shared::is_real(m) {
            return Shared::real_slice(m).map(Elems::Re);
        }
        m.as_slice().map(Elems::C)
    }

    pub(crate) fn len(self) -> usize {
        match self {
            Elems::Re(d) => d.len(),
            Elems::C(d) => d.len(),
        }
    }

    #[inline]
    fn get(self, k: usize) -> (f64, f64) {
        match self {
            Elems::Re(d) => (d[k], 0.0),
            Elems::C(d) => (d[k].re, d[k].im),
        }
    }

    /// `Some((start, len))` (0-based) when the entries are the consecutive
    /// positive integers `a, a+1, …` (`Value::consecutive_positions`).
    pub(crate) fn run(self) -> Option<(usize, usize)> {
        if self.len() == 0 {
            return None;
        }
        let (base, im) = self.get(0);
        if im != 0.0 || base < 1.0 || base.fract() != 0.0 || !base.is_finite() {
            return None;
        }
        (0..self.len())
            .all(|k| self.get(k) == (base + k as f64, 0.0))
            .then_some((base as usize - 1, self.len()))
    }

    /// The 0-based positions of the ones when this is a 0/1 mask of length
    /// `dim_len` (`Value::as_logical_mask`).
    pub(crate) fn mask(self, dim_len: usize) -> Option<Vec<usize>> {
        if self.len() != dim_len {
            return None;
        }
        let ok = (0..self.len()).all(|k| {
            let (re, im) = self.get(k);
            im.abs() <= 1e-12 && (re == 0.0 || re == 1.0)
        });
        ok.then(|| (0..self.len()).filter(|&k| self.get(k).0 == 1.0).collect())
    }

    /// The real parts as 0-based positions below `dim_len`, or the first
    /// entry that is not one.
    pub(crate) fn list(self, dim_len: usize) -> Result<Vec<usize>, f64> {
        (0..self.len())
            .map(|k| {
                let re = self.get(k).0;
                match Value::one_based_to_zero_pub(re) {
                    Ok(i) if i < dim_len => Ok(i),
                    _ => Err(re),
                }
            })
            .collect()
    }

    /// Positions in a dimension of `dim_len`: a run, a mask, or a list, in
    /// that order (`Value::resolve_index_dim`).
    pub(crate) fn positions(self, dim_len: usize) -> Option<Vec<usize>> {
        if let Some((start, n)) = self.run() {
            if start + n <= dim_len {
                return Some((start..start + n).collect());
            }
        }
        if let Some(p) = self.mask(dim_len) {
            return Some(p);
        }
        self.list(dim_len).ok()
    }
}

impl Value {
    /// `self(indices)` for a real-stored vector or matrix, as real storage;
    /// `None` when the complex path should handle it (see the module notes).
    /// Integer indices have already been widened to scalars.
    pub(crate) fn real_index(&self, indices: &[Value]) -> Option<Value> {
        match (self, indices) {
            (Value::Matrix(m), [ri, ci]) => {
                let d = Shared::real_slice(m)?;
                let s = Shared::shape(m);
                let (nr, nc) = (s[0], s[1]);
                let rows = Value::resolve_index_dim(ri, nr).ok()?;
                let cols = Value::resolve_index_dim(ci, nc).ok()?;
                let at = |r: usize, c: usize| d[r * nc + c];
                if rows.len() == 1 && cols.len() == 1 {
                    return Some(Value::Scalar(at(rows[0], cols[0])));
                }
                let col_run = Value::run_of(&cols);
                let gather_row = |out: &mut Vec<f64>, r: usize| match col_run {
                    Some((c0, n)) => out.extend_from_slice(&d[r * nc + c0..r * nc + c0 + n]),
                    None => out.extend(cols.iter().map(|&c| at(r, c))),
                };
                // Whole rows `r0..r0+k` (`M(a:b, :)`): one contiguous copy.
                if let (Some((r0, k)), Some((0, w))) = (Value::run_of(&rows), col_run) {
                    if w == nc && rows.len() > 1 {
                        let out = d[r0 * nc..(r0 + k) * nc].to_vec();
                        return Some(Value::real_matrix(Array2::from_shape_vec((k, nc), out).ok()?));
                    }
                }
                if rows.len() == 1 {
                    let mut out = Vec::with_capacity(cols.len());
                    gather_row(&mut out, rows[0]);
                    return Some(Value::real_vector(Array1::from_vec(out)));
                }
                if cols.len() == 1 {
                    let c = cols[0];
                    return Some(Value::real_vector(rows.iter().map(|&r| at(r, c)).collect()));
                }
                let n = rows.len() * cols.len();
                if let (true, Some((c0, w))) = (n >= super::elementwise::PAR_MIN_LEN, col_run) {
                    // A large block (`M(:, a:b)`): row segments copied in
                    // parallel.
                    use rayon::prelude::*;
                    let mut out = vec![0.0; n];
                    out.par_chunks_mut(w).zip(rows.par_iter()).for_each(|(dst, &r)| {
                        dst.copy_from_slice(&d[r * nc + c0..r * nc + c0 + w]);
                    });
                    return Some(Value::real_matrix(Array2::from_shape_vec((rows.len(), w), out).ok()?));
                }
                let mut out = Vec::with_capacity(n);
                for &r in &rows {
                    gather_row(&mut out, r);
                }
                Some(Value::real_matrix(Array2::from_shape_vec((rows.len(), cols.len()), out).ok()?))
            }
            // `v(i, 1)` / `v(1, j)`: one dimension of a vector is 1.
            (Value::Vector(_), [ri, ci]) => match (ri, ci) {
                (_, Value::Scalar(c)) if (*c as usize) == 1 => self.real_index(std::slice::from_ref(ri)),
                (Value::Scalar(r), _) if (*r as usize) == 1 => self.real_index(std::slice::from_ref(ci)),
                _ => None,
            },
            (Value::Vector(v), [i]) => {
                let d = Shared::real_slice(v)?;
                match i {
                    Value::All => Some(Value::Vector(v.clone())),
                    Value::Scalar(n) => {
                        let k = Value::one_based_to_zero_pub(*n).ok()?;
                        d.get(k).map(|&x| Value::Scalar(x))
                    }
                    Value::Vector(iv) => {
                        let e = Elems::of_vector(iv)?;
                        if let Some((s, n)) = e.run() {
                            if s + n <= d.len() {
                                return Some(Value::real_vector(Array1::from(d[s..s + n].to_vec())));
                            }
                        }
                        let p = match e.mask(d.len()) {
                            Some(p) => p,
                            None => e.list(d.len()).ok()?,
                        };
                        Some(Value::real_vector(p.iter().map(|&k| d[k]).collect()))
                    }
                    _ => None,
                }
            }
            (Value::Matrix(m), [i]) => {
                // Linear indexing, column-major: position p is row p % nr,
                // column p / nr.
                let d = Shared::real_slice(m)?;
                let s = Shared::shape(m);
                let (nr, nc) = (s[0], s[1]);
                let total = nr * nc;
                let at = |p: usize| d[(p % nr) * nc + p / nr];
                let p = match i {
                    Value::Scalar(k) => {
                        let p = Value::one_based_to_zero_pub(*k).ok()?;
                        return (p < total).then(|| Value::Scalar(at(p)));
                    }
                    Value::All => (0..total).collect(),
                    Value::Vector(iv) => {
                        let e = Elems::of_vector(iv)?;
                        match e.mask(total) {
                            Some(p) => p,
                            None => e.list(total).ok()?,
                        }
                    }
                    Value::Matrix(im) => {
                        let e = Elems::of_matrix(im)?;
                        let is = Shared::shape(im);
                        let logical = (0..e.len()).all(|k| {
                            let (re, imv) = e.get(k);
                            imv.abs() <= 1e-12 && (re == 0.0 || re == 1.0)
                        });
                        if is == [nr, nc] && logical {
                            // Same-shape mask, picked column-major.
                            let mut p = Vec::new();
                            for c in 0..nc {
                                for r in 0..nr {
                                    if e.get(r * nc + c).0 == 1.0 {
                                        p.push(c * nr + r);
                                    }
                                }
                            }
                            p
                        } else if is[0] == 1 || is[1] == 1 {
                            e.list(total).ok()?
                        } else {
                            return None;
                        }
                    }
                    _ => return None,
                };
                Some(Value::real_vector(p.iter().map(|&k| at(k)).collect()))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::shared::set_real_storage;

    fn real_m(r: usize, c: usize) -> Value {
        Value::real_matrix(Array2::from_shape_fn((r, c), |(i, j)| (i * 10 + j) as f64 + 0.5))
    }

    fn widened(v: &Value) -> Value {
        match v {
            Value::Vector(a) => Value::Vector(Shared::new((**a).clone())),
            Value::Matrix(m) => Value::Matrix(Shared::new((**m).clone())),
            other => other.clone(),
        }
    }

    fn rv(xs: &[f64]) -> Value {
        Value::real_vector(Array1::from(xs.to_vec()))
    }

    /// Every index form against the complex path on the same numbers.
    #[test]
    fn reads_match_the_complex_path() {
        set_real_storage(Some(true));
        let m = real_m(4, 5);
        let v = rv(&[1.5, -2.0, 3.0, 4.25, -5.0, 6.0]);
        let idx = [
            Value::All,
            Value::Scalar(1.0),
            Value::Scalar(3.0),
            Value::Scalar(4.0),
            rv(&[2.0, 3.0, 4.0]),
            rv(&[4.0, 1.0, 2.0]),
            rv(&[1.0, 0.0, 1.0, 1.0]),
            rv(&[1.0, 0.0, 1.0, 1.0, 0.0, 1.0]),
            Value::Vector(Shared::new([2.0, 3.0].iter().map(|&x| C64::new(x, 0.0)).collect())),
            Value::real_matrix(Array2::from_shape_vec((1, 2), vec![5.0, 7.0]).unwrap()),
            Value::real_matrix(Array2::from_shape_fn((4, 5), |(i, j)| ((i + j) % 2) as f64)),
            rv(&[0.0, 2.0]),
            rv(&[2.5]),
            Value::Scalar(99.0),
        ];
        let mut covered = 0;
        for target in [&m, &v] {
            let mut forms: Vec<Vec<Value>> = idx.iter().map(|i| vec![i.clone()]).collect();
            for a in &idx {
                for b in &idx {
                    forms.push(vec![a.clone(), b.clone()]);
                }
            }
            for f in forms {
                let want = widened(target).index_ref(f.clone());
                if let Some(got) = target.real_index(&f) {
                    covered += 1;
                    let want = want.unwrap_or_else(|e| panic!("{f:?}: complex path errors ({e}) but real path answered"));
                    let same = match (&got, &want) {
                        (Value::Scalar(a), Value::Scalar(b)) => a.to_bits() == b.to_bits(),
                        (Value::Vector(a), Value::Vector(b)) => **a == **b,
                        (Value::Matrix(a), Value::Matrix(b)) => **a == **b,
                        _ => false,
                    };
                    assert!(same, "{f:?}: {got:?} vs {want:?}");
                    match &got {
                        Value::Vector(a) => assert!(Shared::is_real(a), "{f:?}"),
                        Value::Matrix(a) => assert!(Shared::is_real(a), "{f:?}"),
                        Value::Scalar(_) => {}
                        other => panic!("{f:?}: unexpected {other:?}"),
                    }
                }
            }
        }
        assert!(covered >= 80, "only {covered} reads took the real path");
        set_real_storage(None);
    }
}

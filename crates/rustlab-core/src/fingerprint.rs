//! Stable byte-level fingerprint for values held by the persistent
//! function-result cache.
//!
//! The cache keys on `(entry_id, input_hash)` where `input_hash` is a
//! BLAKE3 of the function's arguments in canonical byte form. To stay
//! correct across machines and rustlab versions, the encoding here is:
//!
//! - **Host-independent.** All integers and floats are written
//!   little-endian. Field order is fixed.
//! - **Domain-separated.** Each type prepends a one-byte tag so that
//!   different types with coincident byte content (e.g. a bool `true`
//!   and an `i64 = 1`) don't collide.
//! - **Length-prefixed.** Variable-length data (strings, slices, the
//!   sparse `entries` vec) writes its length first so that
//!   `("ab", "c")` and `("a", "bc")` hash differently.
//! - **NaN-rejecting.** Any `f64::NAN` in the value flips the trait's
//!   return to `false`/`None`; the cache layer reads this as
//!   "uncacheable, just run the function." IEEE NaN ≠ NaN means we
//!   couldn't honour a cache hit anyway.
//!
//! Use [`Fingerprint::fingerprint`] for a one-shot 32-byte hash;
//! implementers provide [`Fingerprint::feed`] so composite types can
//! stream their components into a shared hasher without intermediate
//! allocations.

use crate::types::{CMatrix, CVector, RMatrix, RVector, SparseMat, SparseVec, C64};

/// Domain-separator tags. Bumping the layout for a type means bumping
/// its tag too (or the schema-version path in `rustlab-cache`).
mod tag {
    pub const F64: u8 = 0x01;
    pub const I64: u8 = 0x02;
    pub const U64: u8 = 0x03;
    pub const BOOL: u8 = 0x04;
    pub const STR: u8 = 0x05;
    pub const BYTES: u8 = 0x06;
    pub const C64: u8 = 0x07;
    pub const TUPLE: u8 = 0x08;
    pub const OPTION_NONE: u8 = 0x09;
    pub const OPTION_SOME: u8 = 0x0A;
    pub const SLICE: u8 = 0x0B;

    pub const RVECTOR: u8 = 0x20;
    pub const CVECTOR: u8 = 0x21;
    pub const RMATRIX: u8 = 0x22;
    pub const CMATRIX: u8 = 0x23;
    pub const SPARSE_VEC: u8 = 0x30;
    pub const SPARSE_MAT: u8 = 0x31;
}

/// A value that can be fed into a BLAKE3 hasher to produce a stable
/// content fingerprint for cache keying.
pub trait Fingerprint {
    /// Stream this value's canonical bytes into `hasher`. Returns
    /// `false` if the value contains data that disqualifies it from
    /// caching (the only current case is `f64::NAN`).
    ///
    /// Implementers MUST:
    ///
    /// 1. Write a unique domain-separator tag byte first.
    /// 2. Length-prefix any variable-length payload with a `u64` LE.
    /// 3. Write all integers / floats little-endian.
    /// 4. Propagate `false` from nested `feed` calls — the rule is
    ///    "any NaN anywhere makes the whole value uncacheable."
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool;

    /// Convenience: produce the full 32-byte BLAKE3 hash. Returns
    /// `None` if [`feed`](Fingerprint::feed) reported the value as
    /// uncacheable.
    fn fingerprint(&self) -> Option<[u8; 32]> {
        let mut hasher = blake3::Hasher::new();
        if self.feed(&mut hasher) {
            Some(*hasher.finalize().as_bytes())
        } else {
            None
        }
    }
}

// ── primitive impls ──────────────────────────────────────────────────

impl Fingerprint for f64 {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        if self.is_nan() {
            return false;
        }
        hasher.update(&[tag::F64]);
        // Normalize the bit pattern of -0.0 to +0.0 so that
        // `0.0 == -0.0` (per IEEE) also hashes the same. Without this
        // step the two negate-equal zeroes would key differently.
        let value = if *self == 0.0 { 0.0 } else { *self };
        hasher.update(&value.to_le_bytes());
        true
    }
}

impl Fingerprint for i64 {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::I64]);
        hasher.update(&self.to_le_bytes());
        true
    }
}

impl Fingerprint for u64 {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::U64]);
        hasher.update(&self.to_le_bytes());
        true
    }
}

impl Fingerprint for bool {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::BOOL, *self as u8]);
        true
    }
}

impl Fingerprint for str {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::STR]);
        hasher.update(&(self.len() as u64).to_le_bytes());
        hasher.update(self.as_bytes());
        true
    }
}

impl Fingerprint for String {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        self.as_str().feed(hasher)
    }
}

impl Fingerprint for [u8] {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::BYTES]);
        hasher.update(&(self.len() as u64).to_le_bytes());
        hasher.update(self);
        true
    }
}

impl Fingerprint for C64 {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        if self.re.is_nan() || self.im.is_nan() {
            return false;
        }
        hasher.update(&[tag::C64]);
        let re = if self.re == 0.0 { 0.0 } else { self.re };
        let im = if self.im == 0.0 { 0.0 } else { self.im };
        hasher.update(&re.to_le_bytes());
        hasher.update(&im.to_le_bytes());
        true
    }
}

impl<T: Fingerprint + ?Sized> Fingerprint for &T {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        (*self).feed(hasher)
    }
}

impl<T: Fingerprint> Fingerprint for Option<T> {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        match self {
            None => {
                hasher.update(&[tag::OPTION_NONE]);
                true
            }
            Some(v) => {
                hasher.update(&[tag::OPTION_SOME]);
                v.feed(hasher)
            }
        }
    }
}

impl<T: Fingerprint> Fingerprint for [T] {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::SLICE]);
        hasher.update(&(self.len() as u64).to_le_bytes());
        for item in self {
            if !item.feed(hasher) {
                return false;
            }
        }
        true
    }
}

impl<T: Fingerprint> Fingerprint for Vec<T> {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        self.as_slice().feed(hasher)
    }
}

impl<A: Fingerprint, B: Fingerprint> Fingerprint for (A, B) {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::TUPLE]);
        self.0.feed(hasher) && self.1.feed(hasher)
    }
}

impl<A: Fingerprint, B: Fingerprint, C: Fingerprint> Fingerprint for (A, B, C) {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::TUPLE]);
        self.0.feed(hasher) && self.1.feed(hasher) && self.2.feed(hasher)
    }
}

// ── matrix / vector impls ────────────────────────────────────────────

/// Helper: feed a 2-D shape `(rows, cols)` as two LE u64s.
fn feed_shape2(hasher: &mut blake3::Hasher, rows: usize, cols: usize) {
    hasher.update(&(rows as u64).to_le_bytes());
    hasher.update(&(cols as u64).to_le_bytes());
}

impl Fingerprint for RVector {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::RVECTOR]);
        hasher.update(&(self.len() as u64).to_le_bytes());
        for x in self.iter() {
            if !x.feed(hasher) {
                return false;
            }
        }
        true
    }
}

impl Fingerprint for CVector {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::CVECTOR]);
        hasher.update(&(self.len() as u64).to_le_bytes());
        for x in self.iter() {
            if !x.feed(hasher) {
                return false;
            }
        }
        true
    }
}

impl Fingerprint for RMatrix {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::RMATRIX]);
        feed_shape2(hasher, self.nrows(), self.ncols());
        // `iter()` walks in logical (row-major) order regardless of
        // the array's actual memory layout, so this stays canonical
        // even for transposed / sliced views that share storage.
        for x in self.iter() {
            if !x.feed(hasher) {
                return false;
            }
        }
        true
    }
}

impl Fingerprint for CMatrix {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::CMATRIX]);
        feed_shape2(hasher, self.nrows(), self.ncols());
        for x in self.iter() {
            if !x.feed(hasher) {
                return false;
            }
        }
        true
    }
}

impl Fingerprint for SparseVec {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::SPARSE_VEC]);
        hasher.update(&(self.len as u64).to_le_bytes());
        hasher.update(&(self.entries.len() as u64).to_le_bytes());
        for (idx, val) in &self.entries {
            hasher.update(&(*idx as u64).to_le_bytes());
            if !val.feed(hasher) {
                return false;
            }
        }
        true
    }
}

impl Fingerprint for SparseMat {
    fn feed(&self, hasher: &mut blake3::Hasher) -> bool {
        hasher.update(&[tag::SPARSE_MAT]);
        hasher.update(&(self.rows as u64).to_le_bytes());
        hasher.update(&(self.cols as u64).to_le_bytes());
        hasher.update(&(self.entries.len() as u64).to_le_bytes());
        // SparseMat::new sorts entries row-major, so iterating in
        // storage order is canonical. `ordering_hint` is deliberately
        // omitted — it's solver metadata, not part of the value.
        for (r, c, val) in &self.entries {
            hasher.update(&(*r as u64).to_le_bytes());
            hasher.update(&(*c as u64).to_le_bytes());
            if !val.feed(hasher) {
                return false;
            }
        }
        true
    }
}

// Note on coverage: `RVector` / `CVector` / `RMatrix` / `CMatrix` are
// type *aliases* for `Array1<f64>` / `Array1<C64>` / `Array2<f64>` /
// `Array2<C64>`, so the impls above already cover the bare ndarray
// types. No additional `impl Fingerprint for Array{1,2}<…>` blocks are
// needed (they'd be duplicate impls).

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;
    use num_complex::Complex;

    fn fp<T: Fingerprint + ?Sized>(v: &T) -> Option<[u8; 32]> {
        v.fingerprint()
    }

    #[test]
    fn primitives_distinct_values_hash_differently() {
        assert_ne!(fp(&1.0f64).unwrap(), fp(&2.0f64).unwrap());
        assert_ne!(fp(&1i64).unwrap(), fp(&2i64).unwrap());
        assert_ne!(fp(&true).unwrap(), fp(&false).unwrap());
        assert_ne!(fp("hello").unwrap(), fp("world").unwrap());
    }

    #[test]
    fn primitives_stable_for_identical_input() {
        assert_eq!(fp(&1.0f64).unwrap(), fp(&1.0f64).unwrap());
        assert_eq!(fp("abc").unwrap(), fp("abc").unwrap());
        assert_eq!(fp(&true).unwrap(), fp(&true).unwrap());
    }

    #[test]
    fn negative_and_positive_zero_collide() {
        // 0.0 == -0.0 in IEEE; cache should agree.
        assert_eq!(fp(&0.0f64).unwrap(), fp(&(-0.0f64)).unwrap());
    }

    #[test]
    fn nan_makes_value_uncacheable() {
        assert!(fp(&f64::NAN).is_none());
        let c_nan = Complex::new(1.0, f64::NAN);
        assert!(fp(&c_nan).is_none());
    }

    #[test]
    fn nan_in_matrix_propagates_to_none() {
        let mut m: RMatrix = ndarray::Array2::from_elem((2, 2), 1.0);
        m[[1, 1]] = f64::NAN;
        assert!(fp(&m).is_none());
    }

    #[test]
    fn types_dont_collide_via_byte_content() {
        // bool true and i64=1 and f64=1.0 would all be "ones" without
        // domain separator tags. They must hash distinctly.
        let a = fp(&true).unwrap();
        let b = fp(&1i64).unwrap();
        let c = fp(&1.0f64).unwrap();
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    #[test]
    fn str_length_prefix_prevents_concat_ambiguity() {
        let ab_c = fp(&("ab", "c")).unwrap();
        let a_bc = fp(&("a", "bc")).unwrap();
        assert_ne!(
            ab_c, a_bc,
            "length-prefixed strings must hash differently when split point differs"
        );
    }

    #[test]
    fn matrix_shape_change_changes_hash() {
        let row: RMatrix = array![[1.0, 2.0, 3.0]];
        let col: RMatrix = array![[1.0], [2.0], [3.0]];
        assert_ne!(fp(&row).unwrap(), fp(&col).unwrap());
    }

    #[test]
    fn matrix_content_change_changes_hash() {
        let a: RMatrix = array![[1.0, 2.0], [3.0, 4.0]];
        let b: RMatrix = array![[1.0, 2.0], [3.0, 5.0]];
        assert_ne!(fp(&a).unwrap(), fp(&b).unwrap());
    }

    #[test]
    fn cmatrix_stable_and_distinct() {
        let a: CMatrix = array![[Complex::new(1.0, 0.5), Complex::new(2.0, -1.0)]];
        let b: CMatrix = array![[Complex::new(1.0, 0.5), Complex::new(2.0, -1.0)]];
        let c: CMatrix = array![[Complex::new(1.0, 0.5), Complex::new(2.0, 1.0)]];
        assert_eq!(fp(&a).unwrap(), fp(&b).unwrap());
        assert_ne!(fp(&a).unwrap(), fp(&c).unwrap());
    }

    #[test]
    fn sparse_mat_entry_change_changes_hash() {
        let m1 = SparseMat::new(3, 3, vec![(0, 0, Complex::new(1.0, 0.0))]);
        let m2 = SparseMat::new(3, 3, vec![(0, 0, Complex::new(2.0, 0.0))]);
        let m3 = SparseMat::new(3, 3, vec![(0, 1, Complex::new(1.0, 0.0))]);
        let m4 = SparseMat::new(4, 3, vec![(0, 0, Complex::new(1.0, 0.0))]);
        assert_ne!(fp(&m1).unwrap(), fp(&m2).unwrap(), "value differs");
        assert_ne!(fp(&m1).unwrap(), fp(&m3).unwrap(), "position differs");
        assert_ne!(fp(&m1).unwrap(), fp(&m4).unwrap(), "shape differs");
    }

    #[test]
    fn sparse_mat_ordering_hint_does_not_affect_hash() {
        // ordering_hint is solver metadata, not part of the value.
        let a = SparseMat::new(2, 2, vec![(0, 0, Complex::new(1.0, 0.0))]);
        let mut b = a.clone();
        b.ordering_hint = Some(crate::OrderingHint::Identity);
        assert_eq!(fp(&a).unwrap(), fp(&b).unwrap());
    }

    #[test]
    fn sparse_vec_stable_and_distinct() {
        let a = SparseVec::new(10, vec![(0, Complex::new(1.0, 0.0))]);
        let b = SparseVec::new(10, vec![(0, Complex::new(1.0, 0.0))]);
        let c = SparseVec::new(10, vec![(1, Complex::new(1.0, 0.0))]);
        assert_eq!(fp(&a).unwrap(), fp(&b).unwrap());
        assert_ne!(fp(&a).unwrap(), fp(&c).unwrap());
    }

    #[test]
    fn slice_length_prefix_prevents_collision() {
        // [[1,2], [3]] vs [[1], [2,3]] — both flatten to "1,2,3" but
        // the length prefix on the outer slice and on each inner slice
        // makes them hash differently.
        let a: Vec<Vec<i64>> = vec![vec![1, 2], vec![3]];
        let b: Vec<Vec<i64>> = vec![vec![1], vec![2, 3]];
        let fa = {
            let mut h = blake3::Hasher::new();
            a.as_slice().feed(&mut h);
            *h.finalize().as_bytes()
        };
        let fb = {
            let mut h = blake3::Hasher::new();
            b.as_slice().feed(&mut h);
            *h.finalize().as_bytes()
        };
        assert_ne!(fa, fb);
    }

    #[test]
    fn option_none_some_distinct() {
        let none: Option<i64> = None;
        let some: Option<i64> = Some(0);
        assert_ne!(fp(&none).unwrap(), fp(&some).unwrap());
    }
}

//! Logical (boolean) arrays.
//!
//! A [`LogicalArray`] is a row-major grid of `true` / `false`, one byte per
//! element (`0` or `1`). Comparisons and masks use this type instead of a
//! numeric 0/1 array. MATLAB stores `logical` the same way (one byte, not a
//! bit). A packed bit vector would shrink a mask, and a pure scan of packed
//! bits can be quicker, but indexing a mask is a gather: the byte form is
//! the one measured below.
//!
//! # Storage measurement
//!
//! `byte_mask_scan_matches_a_bit_pack` walks 1_000_000 flags (about one
//! third set) and keeps the best of five. Release build on this host:
//!
//! | walk | byte (`u8`) | packed (`u64` words) |
//! |---|---|---|
//! | count of trues | 364 µs | 92 µs |
//! | list of true positions | 1.10 ms | 695 µs |
//!
//! The bit pack wins a sequential count by about 4× and a position list by
//! about 1.6× (0.4 ms on a million elements). Mask indexing still uses
//! bytes: each element is one load, `get` does not shift, and MATLAB stores
//! `logical` as one byte. A second packed representation is not worth that
//! gap.

use crate::size::{checked_elems, ELEM_U8};
use crate::CoreError;
use std::fmt;

/// Row-major logical array, shape `rows × cols`.
///
/// Element `(r, c)` lives at `data[r * cols + c]`. A stored byte is `0`
/// (`false`) or `1` (`true`); [`LogicalArray::get`] treats any other nonzero
/// byte as `true` so a hand-built buffer still reads as a mask.
///
/// A vector is one row (`rows == 1`) or one column (`cols == 1`).
#[derive(Clone, PartialEq, Eq)]
pub struct LogicalArray {
    data: Vec<u8>,
    rows: usize,
    cols: usize,
}

impl fmt::Debug for LogicalArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LogicalArray({}x{})", self.rows, self.cols)?;
        for r in 0..self.rows.min(4) {
            write!(f, " [")?;
            for c in 0..self.cols.min(8) {
                if c > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", u8::from(self.get(r, c).unwrap_or(false)))?;
            }
            if self.cols > 8 {
                write!(f, " ...")?;
            }
            write!(f, "]")?;
        }
        if self.rows > 4 {
            write!(f, " ...")?;
        }
        Ok(())
    }
}

impl fmt::Display for LogicalArray {
    /// `logical(RxC)` plus one `0`/`1` row per row.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let row = LogicalArray::row(vec![true, false]).unwrap();
    /// assert_eq!(row.to_string(), "logical(1x2)\n  1 0");
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "logical({}x{})", self.rows, self.cols)?;
        for r in 0..self.rows {
            write!(f, "\n  ")?;
            for c in 0..self.cols {
                if c > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", u8::from(self.get(r, c).unwrap_or(false)))?;
            }
        }
        Ok(())
    }
}

impl LogicalArray {
    /// `rows × cols` array from bytes already in row-major order.
    ///
    /// Nonzero bytes are stored as `1`. The element count must equal
    /// `rows * cols`, and that product must pass [`checked_elems`].
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let m = LogicalArray::from_bytes(vec![1, 0, 0, 2], 2, 2).unwrap();
    /// assert!(m.get(0, 0).unwrap());
    /// assert!(m.get(1, 1).unwrap());
    /// assert!(!m.get(0, 1).unwrap());
    /// ```
    pub fn from_bytes(data: Vec<u8>, rows: usize, cols: usize) -> Result<Self, CoreError> {
        let n = checked_elems(&[rows, cols], ELEM_U8)?;
        if data.len() != n {
            return Err(CoreError::InvalidParameter(format!(
                "logical array has {} elements, shape is {rows}x{cols}",
                data.len()
            )));
        }
        let data = data.into_iter().map(|b| u8::from(b != 0)).collect();
        Ok(Self { data, rows, cols })
    }

    /// [`from_bytes`](Self::from_bytes) from `bool` flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let row = LogicalArray::from_flags(vec![true, false, true], 1, 3).unwrap();
    /// assert_eq!(row.true_positions_column_major(), vec![0, 2]);
    /// ```
    pub fn from_flags(flags: Vec<bool>, rows: usize, cols: usize) -> Result<Self, CoreError> {
        Self::from_bytes(flags.into_iter().map(u8::from).collect(), rows, cols)
    }

    /// Every element `value`. `0 × 0` is the empty logical matrix.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let t = LogicalArray::filled(2, 3, true).unwrap();
    /// assert_eq!(t.count_true(), 6);
    /// let empty = LogicalArray::filled(1, 0, false).unwrap();
    /// assert!(empty.is_empty());
    /// ```
    pub fn filled(rows: usize, cols: usize, value: bool) -> Result<Self, CoreError> {
        let n = checked_elems(&[rows, cols], ELEM_U8)?;
        Ok(Self {
            data: vec![u8::from(value); n],
            rows,
            cols,
        })
    }

    /// One row. An empty list is `1 × 0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let row = LogicalArray::row(vec![true, false]).unwrap();
    /// assert_eq!((row.rows(), row.cols()), (1, 2));
    /// assert_eq!(row.as_slice(), &[1, 0]);
    /// ```
    pub fn row(flags: Vec<bool>) -> Result<Self, CoreError> {
        let n = flags.len();
        Self::from_flags(flags, 1, n)
    }

    /// Number of rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let m = LogicalArray::filled(2, 3, false).unwrap();
    /// assert_eq!(m.rows(), 2);
    /// assert_eq!(m.cols(), 3);
    /// assert_eq!(m.len(), 6);
    /// ```
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of columns.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let row = LogicalArray::row(vec![true, false, true]).unwrap();
    /// assert_eq!(row.cols(), 3);
    /// assert_eq!(row.len(), 3);
    /// assert!(!row.is_empty());
    /// assert_eq!(row.as_slice(), &[1, 0, 1]);
    /// ```
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// `rows * cols`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// assert_eq!(LogicalArray::filled(2, 2, true).unwrap().len(), 4);
    /// ```
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// No elements (`0 × n`, `n × 0`, or `0 × 0`).
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// assert!(LogicalArray::filled(0, 3, false).unwrap().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Row-major bytes, `0` or `1`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// assert_eq!(LogicalArray::row(vec![false, true]).unwrap().as_slice(), &[0, 1]);
    /// ```
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Element `(r, c)`, or `None` when the index is outside the array.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let m = LogicalArray::from_flags(vec![true, false, false, true], 2, 2).unwrap();
    /// assert_eq!(m.get(1, 1), Some(true));
    /// assert_eq!(m.get(2, 0), None);
    /// assert_eq!(m.get_column_major(3), Some(true));
    /// assert_eq!(m.to_flags(), vec![true, false, false, true]);
    /// ```
    pub fn get(&self, r: usize, c: usize) -> Option<bool> {
        if r >= self.rows || c >= self.cols {
            return None;
        }
        Some(self.data[r * self.cols + c] != 0)
    }

    /// Column-major linear element `k` (`k = c * rows + r`), or `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let m = LogicalArray::from_flags(vec![true, false, false, true], 2, 2).unwrap();
    /// assert_eq!(m.get_column_major(0), Some(true));
    /// assert_eq!(m.get_column_major(1), Some(false));
    /// assert_eq!(m.count_true(), 2);
    /// assert_eq!(m.to_flags(), vec![true, false, false, true]);
    /// ```
    pub fn get_column_major(&self, k: usize) -> Option<bool> {
        if self.rows == 0 || k >= self.len() {
            return None;
        }
        let c = k / self.rows;
        let r = k % self.rows;
        self.get(r, c)
    }

    /// How many elements are `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// assert_eq!(LogicalArray::row(vec![true, false, true]).unwrap().count_true(), 2);
    /// ```
    pub fn count_true(&self) -> usize {
        self.data.iter().filter(|&&b| b != 0).count()
    }

    /// `true` / `false` in row-major order.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// assert_eq!(
    ///     LogicalArray::row(vec![true, false]).unwrap().to_flags(),
    ///     vec![true, false]
    /// );
    /// ```
    pub fn to_flags(&self) -> Vec<bool> {
        self.data.iter().map(|&b| b != 0).collect()
    }

    /// 0-based indices of `true` elements in storage (row-major) order.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let row = LogicalArray::row(vec![false, true, true]).unwrap();
    /// assert_eq!(row.true_indices_storage_order(), vec![1, 2]);
    /// assert_eq!(row.count_true(), 2);
    /// ```
    pub fn true_indices_storage_order(&self) -> Vec<usize> {
        self.data
            .iter()
            .enumerate()
            .filter(|(_, &b)| b != 0)
            .map(|(i, _)| i)
            .collect()
    }

    /// 0-based column-major linear indices of `true` elements.
    ///
    /// Column `c`, row `r` is index `c * rows + r`, the same walk as
    /// `M(mask)` on a numeric matrix. A single row is the same order as
    /// storage.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// // [1 0; 0 1] — trues at (0,0) and (1,1).
    /// let m = LogicalArray::from_flags(vec![true, false, false, true], 2, 2).unwrap();
    /// assert_eq!(m.true_positions_column_major(), vec![0, 3]);
    /// ```
    pub fn true_positions_column_major(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for c in 0..self.cols {
            for r in 0..self.rows {
                if self.data[r * self.cols + c] != 0 {
                    out.push(c * self.rows + r);
                }
            }
        }
        out
    }

    /// Positions of `true` along one dimension.
    ///
    /// The array must be a vector (`rows <= 1` or `cols <= 1`) whose length
    /// equals `dim_len`. A wider logical is not a subscript list.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let mask = LogicalArray::row(vec![true, false, true]).unwrap();
    /// assert_eq!(mask.dimension_mask(3).unwrap(), vec![0, 2]);
    /// assert!(mask.dimension_mask(2).is_err());
    /// ```
    pub fn dimension_mask(&self, dim_len: usize) -> Result<Vec<usize>, CoreError> {
        if self.rows > 1 && self.cols > 1 {
            return Err(CoreError::InvalidParameter(
                "logical index must be a vector".to_string(),
            ));
        }
        if self.len() != dim_len {
            return Err(CoreError::InvalidParameter(format!(
                "logical index length {} does not match size {dim_len}",
                self.len()
            )));
        }
        Ok(self.true_indices_storage_order())
    }

    /// Column-major true positions when this mask has exactly `total` elements.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let mask = LogicalArray::from_flags(vec![true, false, false, true], 2, 2).unwrap();
    /// assert_eq!(mask.linear_mask(4).unwrap(), vec![0, 3]);
    /// assert!(mask.linear_mask(3).is_err());
    /// ```
    pub fn linear_mask(&self, total: usize) -> Result<Vec<usize>, CoreError> {
        if self.len() != total {
            return Err(CoreError::InvalidParameter(format!(
                "logical index must have {total} elements to index an array of {total} elements, got {}",
                self.len()
            )));
        }
        Ok(self.true_positions_column_major())
    }

    /// Element-wise `not`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let a = LogicalArray::row(vec![true, false]).unwrap();
    /// let b = LogicalArray::row(vec![false, false]).unwrap();
    /// assert_eq!(a.not().to_flags(), vec![false, true]);
    /// assert_eq!(a.or(&b).unwrap().to_flags(), vec![true, false]);
    /// assert_eq!(a.and(&b).unwrap().count_true(), 0);
    /// assert_eq!(a.xor(&a).unwrap().count_true(), 0);
    /// assert!(a.and(&LogicalArray::filled(2, 2, true).unwrap()).is_err());
    /// ```
    pub fn not(&self) -> Self {
        Self {
            data: self.data.iter().map(|&b| u8::from(b == 0)).collect(),
            rows: self.rows,
            cols: self.cols,
        }
    }

    /// Element-wise `and`. Shapes must match.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let a = LogicalArray::row(vec![true, false, true]).unwrap();
    /// let b = LogicalArray::row(vec![true, true, false]).unwrap();
    /// assert_eq!(a.and(&b).unwrap().to_flags(), vec![true, false, false]);
    /// assert_eq!(a.or(&b).unwrap().to_flags(), vec![true, true, true]);
    /// assert_eq!(a.xor(&b).unwrap().to_flags(), vec![false, true, true]);
    /// ```
    pub fn and(&self, other: &Self) -> Result<Self, CoreError> {
        self.zip(other, |a, b| a != 0 && b != 0)
    }

    /// Element-wise `or`. Shapes must match.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let a = LogicalArray::row(vec![false, true]).unwrap();
    /// let b = LogicalArray::row(vec![false, false]).unwrap();
    /// assert_eq!(a.or(&b).unwrap().to_flags(), vec![false, true]);
    /// ```
    pub fn or(&self, other: &Self) -> Result<Self, CoreError> {
        self.zip(other, |a, b| a != 0 || b != 0)
    }

    /// Element-wise `xor`. Shapes must match.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let a = LogicalArray::row(vec![true, false]).unwrap();
    /// let b = LogicalArray::row(vec![true, true]).unwrap();
    /// assert_eq!(a.xor(&b).unwrap().to_flags(), vec![false, true]);
    /// ```
    pub fn xor(&self, other: &Self) -> Result<Self, CoreError> {
        self.zip(other, |a, b| (a != 0) != (b != 0))
    }

    /// Swap rows and columns. Conjugation does not apply to logicals.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// // rows [1 0 0; 1 1 0]
    /// let m = LogicalArray::from_flags(vec![true, false, false, true, true, false], 2, 3).unwrap();
    /// let t = m.transpose();
    /// assert_eq!((t.rows(), t.cols()), (3, 2));
    /// assert_eq!(t.get(0, 1), Some(true));
    /// assert_eq!(m.column_counts(), vec![2, 1, 0]);
    /// assert_eq!(m.row_counts(), vec![1, 2]);
    /// ```
    pub fn transpose(&self) -> Self {
        let mut data = vec![0u8; self.len()];
        for r in 0..self.rows {
            for c in 0..self.cols {
                data[c * self.rows + r] = self.data[r * self.cols + c];
            }
        }
        Self {
            data,
            rows: self.cols,
            cols: self.rows,
        }
    }

    /// Number of `true` elements in each column.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let m = LogicalArray::from_flags(vec![true, false, true, false], 2, 2).unwrap();
    /// assert_eq!(m.column_counts(), vec![2, 0]);
    /// assert_eq!(m.row_counts(), vec![1, 1]);
    /// ```
    pub fn column_counts(&self) -> Vec<u64> {
        let mut out = vec![0u64; self.cols];
        for r in 0..self.rows {
            for c in 0..self.cols {
                if self.data[r * self.cols + c] != 0 {
                    out[c] += 1;
                }
            }
        }
        out
    }

    /// Number of `true` elements in each row.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustlab_core::LogicalArray;
    /// let m = LogicalArray::from_flags(vec![true, true, false, false], 2, 2).unwrap();
    /// assert_eq!(m.row_counts(), vec![2, 0]);
    /// ```
    pub fn row_counts(&self) -> Vec<u64> {
        let mut out = vec![0u64; self.rows];
        for r in 0..self.rows {
            let mut n = 0u64;
            for c in 0..self.cols {
                if self.data[r * self.cols + c] != 0 {
                    n += 1;
                }
            }
            out[r] = n;
        }
        out
    }

    fn zip(&self, other: &Self, f: impl Fn(u8, u8) -> bool) -> Result<Self, CoreError> {
        if self.rows != other.rows || self.cols != other.cols {
            return Err(CoreError::InvalidParameter(format!(
                "logical shape mismatch: {}x{} vs {}x{}",
                self.rows, self.cols, other.rows, other.cols
            )));
        }
        let data = self
            .data
            .iter()
            .zip(&other.data)
            .map(|(&a, &b)| u8::from(f(a, b)))
            .collect();
        Ok(Self {
            data,
            rows: self.rows,
            cols: self.cols,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_a_row_and_rejects_a_bad_length() {
        let row = LogicalArray::row(vec![true, false, true]).unwrap();
        assert_eq!((row.rows(), row.cols(), row.len()), (1, 3, 3));
        assert!(row.get(0, 0).unwrap());
        assert!(!row.get(0, 1).unwrap());
        assert!(row.get(0, 3).is_none());
        assert!(LogicalArray::from_flags(vec![true], 2, 2).is_err());
    }

    #[test]
    fn empty_shapes_have_no_elements() {
        for (r, c) in [(1, 0), (0, 1), (0, 0)] {
            let a = LogicalArray::filled(r, c, true).unwrap();
            assert!(a.is_empty(), "{r}x{c}");
            assert_eq!(a.count_true(), 0);
            assert!(a.true_positions_column_major().is_empty());
        }
    }

    #[test]
    fn column_major_positions_walk_down_columns() {
        // 1 0 1
        // 0 1 0
        let m =
            LogicalArray::from_flags(vec![true, false, true, false, true, false], 2, 3).unwrap();
        // Column-major: (0,0) → 0, (1,1) → 3, (0,2) → 4.
        assert_eq!(m.true_positions_column_major(), vec![0, 3, 4]);
        assert_eq!(m.column_counts(), vec![1, 1, 1]);
        assert_eq!(m.row_counts(), vec![2, 1]);
    }

    #[test]
    fn elementwise_logic_and_shape_errors() {
        let a = LogicalArray::row(vec![true, false, true]).unwrap();
        let b = LogicalArray::row(vec![false, false, true]).unwrap();
        assert_eq!(a.and(&b).unwrap().to_flags(), vec![false, false, true]);
        assert_eq!(a.or(&b).unwrap().to_flags(), vec![true, false, true]);
        assert_eq!(a.xor(&b).unwrap().to_flags(), vec![true, false, false]);
        assert_eq!(a.not().to_flags(), vec![false, true, false]);
        let col = LogicalArray::from_flags(vec![true, false], 2, 1).unwrap();
        assert!(a.and(&col).is_err());
        assert_eq!(a.transpose().rows(), 3);
        assert_eq!(a.transpose().cols(), 1);
    }

    #[test]
    fn dimension_mask_rejects_a_matrix_and_a_short_vector() {
        let m = LogicalArray::filled(2, 2, true).unwrap();
        assert!(m.dimension_mask(4).is_err());
        let row = LogicalArray::row(vec![true, false]).unwrap();
        assert!(row.dimension_mask(3).is_err());
        assert_eq!(row.dimension_mask(2).unwrap(), vec![0]);
        assert!(row.linear_mask(3).is_err());
        assert_eq!(row.linear_mask(2).unwrap(), vec![0]);
    }

    /// Count and gather of 1_000_000 flags. The numbers in the module docs
    /// come from this test; re-run it if the representation changes.
    #[test]
    fn byte_mask_scan_matches_a_bit_pack() {
        const N: usize = 1_000_000;
        let flags: Vec<bool> = (0..N).map(|i| i % 3 == 0).collect();
        let expect = flags.iter().filter(|&&b| b).count();
        let arr = LogicalArray::from_flags(flags, 1, N).unwrap();
        let packed = pack_bits(arr.as_slice());

        let mut best_byte_count = std::time::Duration::from_secs(60);
        let mut best_bit_count = best_byte_count;
        let mut best_byte_pos = best_byte_count;
        let mut best_bit_pos = best_byte_count;
        for _ in 0..5 {
            let t = std::time::Instant::now();
            let c = arr.as_slice().iter().filter(|&&b| b != 0).count();
            best_byte_count = best_byte_count.min(t.elapsed());
            assert_eq!(c, expect);

            let t = std::time::Instant::now();
            let c = count_bits(&packed, N);
            best_bit_count = best_bit_count.min(t.elapsed());
            assert_eq!(c, expect);

            let t = std::time::Instant::now();
            let p = arr.true_indices_storage_order();
            best_byte_pos = best_byte_pos.min(t.elapsed());
            assert_eq!(p.len(), expect);

            let t = std::time::Instant::now();
            let p = positions_bits(&packed, N);
            best_bit_pos = best_bit_pos.min(t.elapsed());
            assert_eq!(p.len(), expect);
            assert_eq!(p[0], 0);
            assert_eq!(*p.last().unwrap(), N - 1);
        }
        assert_eq!(std::mem::size_of_val(&arr.as_slice()[0]), 1);
        eprintln!(
            "logical storage: count byte {best_byte_count:?} bit {best_bit_count:?}; \
             positions byte {best_byte_pos:?} bit {best_bit_pos:?}"
        );
    }

    fn pack_bits(data: &[u8]) -> Vec<u64> {
        let mut words = vec![0u64; data.len().div_ceil(64)];
        for (i, &b) in data.iter().enumerate() {
            if b != 0 {
                words[i / 64] |= 1u64 << (i % 64);
            }
        }
        words
    }

    fn count_bits(words: &[u64], n: usize) -> usize {
        let mut c = 0usize;
        for (w, word) in words.iter().enumerate() {
            let mut word = *word;
            while word != 0 {
                let i = w * 64 + word.trailing_zeros() as usize;
                if i < n {
                    c += 1;
                }
                word &= word - 1;
            }
        }
        c
    }

    fn positions_bits(words: &[u64], n: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for (w, word) in words.iter().enumerate() {
            let mut word = *word;
            while word != 0 {
                let i = w * 64 + word.trailing_zeros() as usize;
                if i < n {
                    out.push(i);
                }
                word &= word - 1;
            }
        }
        out
    }
}

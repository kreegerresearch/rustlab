//! Column-major, 1-based linear indexes.
//!
//! For sizes `[d1, d2, …]` (the first dimension varies fastest, the way
//! `reshape` and `M(k)` walk an array) the linear index of subscripts
//! `(s1, s2, …)` is
//!
//! ```text
//! k = 1 + (s1 - 1) + (s2 - 1)·d1 + (s3 - 1)·d1·d2 + …
//! ```
//!
//! [`ind2sub`] and [`sub2ind`] are inverses for every index in `1..=numel`.
//! An index outside that range is an error. This module does not fold
//! trailing dimensions or broadcast subscript arrays; the script builtins do.

use thiserror::Error;

/// A size or a subscript the conversion cannot accept.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SubscriptError {
    /// `sizes` was empty.
    #[error("size must not be empty")]
    EmptySize,
    /// A product of sizes, or a linear index, does not fit in `u64`.
    #[error("size product overflows")]
    SizeOverflow,
    /// `index` was not in `1..=numel`.
    #[error("linear index {index} is outside 1:{numel}")]
    LinearOutOfRange { index: u64, numel: u64 },
    /// A subscript was not in `1..=size` on that dimension (`dim` is 1-based).
    #[error("subscript {sub} on dimension {dim} is outside 1:{size}")]
    SubscriptOutOfRange { dim: usize, sub: u64, size: u64 },
    /// `sub2ind` was given a different number of subscripts than sizes.
    #[error("expected {sizes} subscripts, got {subs}")]
    RankMismatch { sizes: usize, subs: usize },
}

/// Product of `sizes`. An empty list is an error. A zero size is a product of 0.
pub fn numel(sizes: &[u64]) -> Result<u64, SubscriptError> {
    if sizes.is_empty() {
        return Err(SubscriptError::EmptySize);
    }
    let mut n: u64 = 1;
    for &sz in sizes {
        n = n.checked_mul(sz).ok_or(SubscriptError::SizeOverflow)?;
    }
    Ok(n)
}

/// Subscripts of one 1-based column-major linear index.
///
/// `sizes` is `[d1, d2, …]` with `d1` the fastest dimension (rows). `index`
/// must be in `1..=numel(sizes)`. The last subscript is not wrapped, so an
/// index past the array is [`SubscriptError::LinearOutOfRange`] rather than
/// a coordinate in an extra page.
///
/// # Examples
///
/// The MathWorks 3×3 example. Linear indexes 3, 4, 5, 6 are rows
/// 3, 1, 2, 3 and columns 1, 2, 2, 2.
///
/// ```
/// use rustlab_core::subscript::ind2sub;
///
/// assert_eq!(ind2sub(&[3, 3], 3).unwrap(), vec![3, 1]);
/// assert_eq!(ind2sub(&[3, 3], 4).unwrap(), vec![1, 2]);
/// assert_eq!(ind2sub(&[3, 3], 5).unwrap(), vec![2, 2]);
/// assert_eq!(ind2sub(&[3, 3], 6).unwrap(), vec![3, 2]);
/// ```
pub fn ind2sub(sizes: &[u64], index: u64) -> Result<Vec<u64>, SubscriptError> {
    let n = numel(sizes)?;
    if index < 1 || index > n {
        return Err(SubscriptError::LinearOutOfRange { index, numel: n });
    }
    let mut k = index - 1;
    let mut out = Vec::with_capacity(sizes.len());
    for (i, &sz) in sizes.iter().enumerate() {
        if i + 1 == sizes.len() {
            out.push(k + 1);
        } else {
            // Every size is at least 1: a 0 would make `numel` 0, and the
            // index check above would already have failed.
            out.push((k % sz) + 1);
            k /= sz;
        }
    }
    Ok(out)
}

/// 1-based column-major linear index of one subscript tuple.
///
/// `subs` must have the same length as `sizes`. Each subscript is in
/// `1..=sizes[dim]`.
///
/// # Examples
///
/// ```
/// use rustlab_core::subscript::sub2ind;
///
/// assert_eq!(sub2ind(&[3, 3], &[2, 2]).unwrap(), 5);
/// assert_eq!(sub2ind(&[3, 3], &[1, 2]).unwrap(), 4);
/// assert_eq!(sub2ind(&[10, 10, 10], &[10, 10, 10]).unwrap(), 1000);
/// ```
pub fn sub2ind(sizes: &[u64], subs: &[u64]) -> Result<u64, SubscriptError> {
    if sizes.is_empty() {
        return Err(SubscriptError::EmptySize);
    }
    if sizes.len() != subs.len() {
        return Err(SubscriptError::RankMismatch {
            sizes: sizes.len(),
            subs: subs.len(),
        });
    }
    let mut acc: u64 = 1;
    let mut stride: u64 = 1;
    for (dim, (&sz, &sub)) in sizes.iter().zip(subs).enumerate() {
        if sub < 1 || sub > sz {
            return Err(SubscriptError::SubscriptOutOfRange {
                dim: dim + 1,
                sub,
                size: sz,
            });
        }
        let term = (sub - 1)
            .checked_mul(stride)
            .ok_or(SubscriptError::SizeOverflow)?;
        acc = acc.checked_add(term).ok_or(SubscriptError::SizeOverflow)?;
        stride = stride.checked_mul(sz).ok_or(SubscriptError::SizeOverflow)?;
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mathworks_3_by_3() {
        let rows = [3, 1, 2, 3];
        let cols = [1, 2, 2, 2];
        for (k, (&r, &c)) in [3u64, 4, 5, 6].iter().zip(rows.iter().zip(cols.iter())) {
            assert_eq!(ind2sub(&[3, 3], *k).unwrap(), vec![r, c]);
            assert_eq!(sub2ind(&[3, 3], &[r, c]).unwrap(), *k);
        }
        assert_eq!(sub2ind(&[3, 3], &[1, 2]).unwrap(), 4);
        assert_eq!(sub2ind(&[3, 3], &[2, 2]).unwrap(), 5);
        assert_eq!(sub2ind(&[3, 3], &[3, 2]).unwrap(), 6);
        assert_eq!(sub2ind(&[3, 3], &[1, 3]).unwrap(), 7);
    }

    #[test]
    fn cube_2_2_2_and_corners() {
        let rows = [1, 2, 1, 2, 1, 2, 1, 2];
        let cols = [1, 1, 2, 2, 1, 1, 2, 2];
        let pages = [1, 1, 1, 1, 2, 2, 2, 2];
        for k in 1..=8 {
            let got = ind2sub(&[2, 2, 2], k).unwrap();
            assert_eq!(
                got,
                vec![
                    rows[k as usize - 1],
                    cols[k as usize - 1],
                    pages[k as usize - 1]
                ]
            );
            assert_eq!(sub2ind(&[2, 2, 2], &got).unwrap(), k);
        }
        assert_eq!(sub2ind(&[10, 10, 10], &[1, 1, 1]).unwrap(), 1);
        assert_eq!(sub2ind(&[10, 10, 10], &[10, 10, 10]).unwrap(), 1000);
        assert_eq!(sub2ind(&[2, 3, 4], &[2, 3, 4]).unwrap(), 24);
    }

    #[test]
    fn round_trip_every_index() {
        let sz = [2u64, 3, 4];
        let n = numel(&sz).unwrap();
        assert_eq!(n, 24);
        for k in 1..=n {
            let subs = ind2sub(&sz, k).unwrap();
            assert_eq!(sub2ind(&sz, &subs).unwrap(), k);
        }
    }

    #[test]
    fn scalar_size_and_empty_dimension() {
        assert_eq!(numel(&[5]).unwrap(), 5);
        assert_eq!(ind2sub(&[5], 3).unwrap(), vec![3]);
        assert_eq!(sub2ind(&[5], &[3]).unwrap(), 3);
        assert_eq!(numel(&[0, 3]).unwrap(), 0);
        assert_eq!(
            ind2sub(&[0, 3], 1).unwrap_err(),
            SubscriptError::LinearOutOfRange { index: 1, numel: 0 }
        );
        assert!(matches!(
            sub2ind(&[0, 3], &[1, 1]).unwrap_err(),
            SubscriptError::SubscriptOutOfRange {
                dim: 1,
                sub: 1,
                size: 0
            }
        ));
    }

    #[test]
    fn rejects_empty_size_rank_and_range() {
        assert_eq!(numel(&[]).unwrap_err(), SubscriptError::EmptySize);
        assert_eq!(
            ind2sub(&[3, 3], 0).unwrap_err(),
            SubscriptError::LinearOutOfRange { index: 0, numel: 9 }
        );
        assert_eq!(
            ind2sub(&[3, 3], 10).unwrap_err(),
            SubscriptError::LinearOutOfRange {
                index: 10,
                numel: 9
            }
        );
        assert!(matches!(
            sub2ind(&[3, 3], &[4, 1]).unwrap_err(),
            SubscriptError::SubscriptOutOfRange {
                dim: 1,
                sub: 4,
                size: 3
            }
        ));
        assert_eq!(
            sub2ind(&[3, 3], &[1]).unwrap_err(),
            SubscriptError::RankMismatch { sizes: 2, subs: 1 }
        );
    }
}

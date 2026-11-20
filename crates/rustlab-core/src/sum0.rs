//! Floating-point sums that start from `+0.0`.
//!
//! `Iterator::sum::<f64>()` folds from `-0.0`, the IEEE additive identity, so
//! an empty sum (or one of only `-0.0` terms) is `-0.0` and a norm built on it
//! prints `-0`. MATLAB sums start from `+0`. For any other input the two give
//! the same bits: `-0.0 + x` and `+0.0 + x` differ only when `x` is `-0.0`.

use std::borrow::Borrow;

/// `sum0()` on any iterator of `f64` or `&f64`.
pub trait Sum0: Iterator + Sized
where
    Self::Item: Borrow<f64>,
{
    /// Left-to-right sum from `+0.0`.
    ///
    /// ```
    /// use rustlab_core::Sum0;
    /// let empty: [f64; 0] = [];
    /// assert_eq!(empty.iter().sum0().to_bits(), 0.0f64.to_bits());
    /// assert_eq!([1.5, 2.0].iter().sum0(), 3.5);
    /// ```
    fn sum0(self) -> f64 {
        self.fold(0.0, |acc, x| acc + *x.borrow())
    }
}

impl<I> Sum0 for I
where
    I: Iterator,
    I::Item: Borrow<f64>,
{
}

#[cfg(test)]
mod tests {
    use super::Sum0;

    #[test]
    fn empty_and_negative_zero_sums_are_plus_zero() {
        let empty: Vec<f64> = Vec::new();
        assert_eq!(empty.iter().sum0().to_bits(), 0.0f64.to_bits());
        assert_eq!([-0.0f64, -0.0].iter().sum0().to_bits(), 0.0f64.to_bits());
        assert_eq!(empty.iter().sum::<f64>().to_bits(), (-0.0f64).to_bits());
    }

    #[test]
    fn nonzero_sums_match_the_std_sum_bit_for_bit() {
        let cases: [&[f64]; 5] = [
            &[1.0, -1.0],
            &[0.1, 0.2, 0.3],
            &[-0.0, 2.5],
            &[1e308, 1e308, -1e308],
            &[f64::NAN, 1.0],
        ];
        for c in cases {
            let a = c.iter().sum0();
            let b = c.iter().sum::<f64>();
            assert!(
                a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()),
                "{c:?}"
            );
        }
    }
}

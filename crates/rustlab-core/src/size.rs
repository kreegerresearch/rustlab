//! Checked sizes for array allocation.
//!
//! A dimension product that overflows, a count above MATLAB's maximum
//! variable size (`2^48 − 1` elements), or a byte length that overflows or
//! does not fit in `isize`, is rejected here. Nothing is allocated.

use crate::CoreError;

/// MATLAB's maximum number of elements in one array (`2^48 − 1`).
pub const MAX_ELEMENTS: u64 = (1 << 48) - 1;

/// Stored width of an `f64` element.
pub const ELEM_F64: usize = 8;

/// Stored width of a `Complex<f64>` element.
pub const ELEM_C64: usize = 16;

/// Stored width of the current integer arrays (`i128` backing, every class).
pub const ELEM_I128: usize = 16;

/// `2^64` as `f64`. The first magnitude that does not fit in `u64`.
///
/// `usize::MAX as f64` is this same value, so `n > usize::MAX as f64`
/// accepts `n == 2^64`, and `as usize` then saturates.
pub const F64_TWO_POW_64: f64 = 18446744073709551616.0;

/// Element count of an array with axis lengths `dims` and `elem_size` bytes
/// per element, or [`CoreError::VariableTooLarge`].
///
/// `elem_size` is the width actually stored: 8 for `f64`, 16 for `C64`,
/// 16 for an `i128` integer array (an `int8` is not one byte in memory).
/// The fold is `checked_mul` in `u128`, so a wrapped `usize` product is an
/// error instead of a short buffer.
pub fn checked_elems(dims: &[usize], elem_size: usize) -> Result<usize, CoreError> {
    let mut count: u128 = 1;
    for &d in dims {
        count = count
            .checked_mul(d as u128)
            .ok_or(CoreError::VariableTooLarge)?;
    }
    if count > MAX_ELEMENTS as u128 {
        return Err(CoreError::VariableTooLarge);
    }
    let bytes = count
        .checked_mul(elem_size as u128)
        .ok_or(CoreError::VariableTooLarge)?;
    if bytes > isize::MAX as u128 {
        return Err(CoreError::VariableTooLarge);
    }
    Ok(count as usize)
}

/// `A x B x C` for the script-facing size error. One axis stays a single number.
pub fn shape_text(dims: &[usize]) -> String {
    if dims.is_empty() {
        "0".to_string()
    } else {
        dims.iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("x")
    }
}

/// `'<name>: requested AxB array exceeds the maximum possible variable size'`.
pub fn variable_too_large(name: &str, dims: &[usize]) -> String {
    format!(
        "{name}: requested {} array exceeds the maximum possible variable size",
        shape_text(dims)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_of_2_pow_32_errors_without_allocating() {
        // 2^32 × 2^32 × 16 bytes overflows the element cap. The function
        // only multiplies integers; it does not reserve a buffer.
        let err = checked_elems(&[1 << 32, 1 << 32], ELEM_I128).unwrap_err();
        assert!(matches!(err, CoreError::VariableTooLarge));
    }

    #[test]
    fn count_under_the_element_cap_is_a_legal_size() {
        // 2^32 i128 elements are 64 GiB, and 2^32 f64 elements are 32 GiB.
        // Both are under 2^48 − 1 and under isize::MAX, so the count is
        // legal. This multiplies integers; it does not reserve a buffer.
        assert_eq!(checked_elems(&[1 << 32, 1], ELEM_I128).unwrap(), 1 << 32);
        assert_eq!(checked_elems(&[1 << 32], ELEM_F64).unwrap(), 1 << 32);
    }

    #[test]
    fn byte_length_past_isize_max_errors() {
        // The element cap is not the only byte check. A wide element can
        // still make the allocation larger than isize::MAX.
        let wide = 1usize << 16;
        assert!(checked_elems(&[MAX_ELEMENTS as usize], wide).is_err());
    }

    #[test]
    fn modest_shapes_pass() {
        assert_eq!(checked_elems(&[2, 3], ELEM_F64).unwrap(), 6);
        assert_eq!(checked_elems(&[0, 5], ELEM_C64).unwrap(), 0);
        assert_eq!(checked_elems(&[1, 4096], ELEM_F64).unwrap(), 4096);
    }

    #[test]
    fn two_pow_64_is_the_f64_boundary() {
        assert_eq!(F64_TWO_POW_64, usize::MAX as f64);
        assert!(F64_TWO_POW_64 >= 18446744073709551616.0);
    }
}

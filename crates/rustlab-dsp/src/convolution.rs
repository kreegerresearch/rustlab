use crate::fft::{fft_raw, ifft_raw};
use ndarray::Array1;
use num_complex::Complex;
use rustlab_core::{checked_elems, variable_too_large, CVector, CoreError, C64, ELEM_C64};

fn len_ok(name: &str, n: usize) -> Result<(), CoreError> {
    checked_elems(&[n], ELEM_C64)
        .map(|_| ())
        .map_err(|_| CoreError::InvalidParameter(variable_too_large(name, &[n])))
}

/// Compute the linear convolution of two complex signals using a direct O(n·m) algorithm.
///
/// Each output sample is the inner product of `x` with a shifted copy of `h`:
///
/// ```text
/// y[n] = Σ_{k} x[k] · h[n − k]
/// ```
///
/// The output length is `x.len() + h.len() − 1`. Returns an empty array if
/// either input is empty.
///
/// This function is used internally by [`FirFilter::apply`](crate::fir::design::FirFilter).
/// For long signals combined with a short filter kernel, consider
/// [`overlap_add`] instead to reduce computational cost.
pub fn convolve(x: &CVector, h: &CVector) -> Result<CVector, CoreError> {
    let nx = x.len();
    let nh = h.len();

    if nx == 0 || nh == 0 {
        return Ok(Array1::zeros(0));
    }

    let out_len = nx
        .checked_add(nh)
        .and_then(|s| s.checked_sub(1))
        .ok_or_else(|| CoreError::InvalidParameter(variable_too_large("convolve", &[nx, nh])))?;
    len_ok("convolve", out_len)?;
    let mut output: CVector = Array1::zeros(out_len);

    for n in 0..out_len {
        let k_start = if n + 1 >= nh { n + 1 - nh } else { 0 };
        let k_end = n.min(nx - 1);
        for k in k_start..=k_end {
            output[n] = output[n] + x[k] * h[n - k];
        }
    }

    Ok(output)
}

/// Compute the linear convolution of `x` and `h` using the overlap-add method.
///
/// The *overlap-add* algorithm splits the input `x` into non-overlapping blocks
/// of length `block_size`, convolves each block with `h` in the frequency domain
/// (FFT multiply), and sums the resulting overlapping segments. This reduces the
/// complexity from O(n·m) to O(n · m_fft · log m_fft) where m_fft is the next
/// power of two above `block_size + h.len() − 1`.
///
/// The output length is the same as [`convolve`]: `x.len() + h.len() − 1`.
///
/// # Parameters
/// - `x`: input signal (complex-valued).
/// - `h`: filter kernel / impulse response.
/// - `block_size`: number of input samples to process per FFT block.
///
/// # Errors
/// Returns [`CoreError::InvalidParameter`] if `block_size == 0`.
pub fn overlap_add(x: &CVector, h: &CVector, block_size: usize) -> Result<CVector, CoreError> {
    let nx = x.len();
    let nh = h.len();

    if nx == 0 || nh == 0 {
        return Ok(Array1::zeros(0));
    }

    if block_size == 0 {
        return Err(CoreError::InvalidParameter(
            "block_size must be > 0".to_string(),
        ));
    }

    // FFT size must be at least block_size + nh - 1
    let need = block_size
        .checked_add(nh)
        .and_then(|s| s.checked_sub(1))
        .ok_or_else(|| {
            CoreError::InvalidParameter(variable_too_large("overlap_add", &[block_size, nh]))
        })?;
    let fft_size = next_power_of_two(need)
        .map_err(|_| CoreError::InvalidParameter(variable_too_large("overlap_add", &[need])))?;
    len_ok("overlap_add", fft_size)?;

    // Pad h to fft_size and compute its FFT
    let h_padded: Vec<C64> = h
        .iter()
        .copied()
        .chain(std::iter::repeat(Complex::new(0.0, 0.0)))
        .take(fft_size)
        .collect();
    let h_freq = fft_raw(&h_padded);

    let out_len = nx
        .checked_add(nh)
        .and_then(|s| s.checked_sub(1))
        .ok_or_else(|| CoreError::InvalidParameter(variable_too_large("overlap_add", &[nx, nh])))?;
    len_ok("overlap_add", out_len)?;
    let mut output: Vec<C64> = vec![Complex::new(0.0, 0.0); out_len];

    // Process each block; the zero-padded block and frequency-product
    // buffers are reused across iterations instead of reallocated.
    let zero = Complex::new(0.0, 0.0);
    let mut block: Vec<C64> = vec![zero; fft_size];
    let mut product: Vec<C64> = vec![zero; fft_size];
    let mut pos = 0;
    while pos < nx {
        let end = (pos + block_size).min(nx);
        let block_len = end - pos;

        // Refill the zero-padded block of length fft_size
        for i in 0..block_len {
            block[i] = x[pos + i];
        }
        block[block_len..].fill(zero);

        // Frequency-domain multiply
        let block_freq = fft_raw(&block);
        for (p, (a, b)) in product.iter_mut().zip(block_freq.iter().zip(h_freq.iter())) {
            *p = a * b;
        }

        // IFFT back to time domain
        let conv_block = ifft_raw(&product);

        // Overlap-add into output
        let out_start = pos;
        let out_end = (out_start + fft_size).min(out_len);
        for i in 0..(out_end - out_start) {
            output[out_start + i] = output[out_start + i] + conv_block[i];
        }

        pos += block_size;
    }

    Ok(Array1::from_vec(output))
}

/// Next power of two, at least 1, that is a legal complex-array length.
///
/// The old loop (`p <<= 1` while `p < n`) overflowed or spun forever once
/// `n` was past `2^63`. A length that fits in `usize` but not in the
/// element cap is the same class of error as `xcorr`'s capacity overflow.
pub(crate) fn next_power_of_two(n: usize) -> Result<usize, CoreError> {
    let p = if n <= 1 {
        1
    } else {
        n.checked_next_power_of_two()
            .ok_or_else(|| CoreError::InvalidParameter(variable_too_large("fft", &[n])))?
    };
    len_ok("fft", p)?;
    Ok(p)
}

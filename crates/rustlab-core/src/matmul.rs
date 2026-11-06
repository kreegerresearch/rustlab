//! Dense matrix multiply for complex-stored values.
//!
//! Every rustlab matrix is stored as `Complex<f64>`, but most workloads are
//! real. A complex multiply does four times the floating-point work of a
//! real one and moves twice the bytes, so when both operands have no
//! imaginary part the product is formed from their real parts with a real
//! GEMM (ndarray's `f64` `dot`, which is `matrixmultiply::dgemm`) and widened
//! back. The check and the copies are O(n²) against O(n³) work.
//!
//! On a CPU with the Arm Scalable Matrix Extension and `f64` outer products
//! (Apple M4), a real product of at least [`SME_MIN_MACS`] multiply-adds
//! runs on the matrix unit instead (`sme.rs`), reading the real parts
//! straight from the complex storage. Each element is then a fused
//! multiply-add chain in k order. `RUSTLAB_NO_SME=1` turns that path off.
//!
//! The real path sums in a different order from the complex kernel, so
//! results can differ in the last digits. It also differs where the complex
//! kernel manufactured NaN: `(Inf + 0i)·(1 + 0i)` has imaginary part
//! `Inf·0 = NaN` there, while the real product is `Inf` with a zero
//! imaginary part.

mod sme;

use crate::error::CoreError;
use crate::size::{checked_elems, ELEM_C64, ELEM_F64};
use crate::types::{CMatrix, C64};
use ndarray::linalg::general_mat_mul;
use ndarray::{Array2, ArrayView2, ShapeBuilder};

/// True when every element has an imaginary part of exactly zero
/// (`-0.0` counts as zero; NaN does not).
pub fn is_real(a: &ArrayView2<C64>) -> bool {
    a.iter().all(|z| z.im == 0.0)
}

/// `a · b` for complex-stored matrices. Uses a real GEMM when both operands
/// are real, otherwise the complex one. The caller checks that the inner
/// dimensions agree.
pub fn matmul(a: &ArrayView2<C64>, b: &ArrayView2<C64>) -> Result<CMatrix, CoreError> {
    matmul_conj(a, false, b, false)
}

/// `op(a) · op(b)`, where `op` conjugates the elements when its flag is set.
///
/// The operands may have any strides: the evaluator passes `A'` as a
/// transposed view with `conj_a`, so the transpose is never written out.
/// Conjugation does nothing to a real operand. The result is always
/// row-major (`dot` would follow the operands' memory order, and the
/// elementwise fast paths reject a column-major result).
pub fn matmul_conj(
    a: &ArrayView2<C64>,
    conj_a: bool,
    b: &ArrayView2<C64>,
    conj_b: bool,
) -> Result<CMatrix, CoreError> {
    debug_assert_eq!(a.ncols(), b.nrows());
    let dim = (a.nrows(), b.ncols());
    // Once per call, before the result buffer (and the real-part copies).
    checked_elems(&[dim.0, dim.1], ELEM_C64)?;
    if use_sme(dim.0, a.ncols(), dim.1) {
        if let Some(c) = sme::gemm_real_c64(a, b) {
            return Ok(c);
        }
    }
    if let Some(ar) = real_parts(a) {
        if let Some(br) = real_parts(b) {
            let mut c = Array2::<f64>::zeros(dim);
            general_mat_mul(1.0, &ar, &br, 0.0, &mut c);
            return Ok(c.mapv(|x| C64::new(x, 0.0)));
        }
    }
    let conj = |m: &ArrayView2<C64>, on: bool| on.then(|| m.mapv(|z| z.conj()));
    let (ac, bc) = (conj(a, conj_a), conj(b, conj_b));
    let a = ac.as_ref().map_or(a.view(), |m| m.view());
    let b = bc.as_ref().map_or(b.view(), |m| m.view());
    let mut c = Array2::<C64>::zeros(dim);
    general_mat_mul(C64::new(1.0, 0.0), &a, &b, C64::new(0.0, 0.0), &mut c);
    Ok(c)
}

/// `a · b` for real matrices, with the kernel `matmul_conj` uses for real
/// operands (SME when available and large enough, else `matrixmultiply`),
/// so a real-stored product equals a complex-stored one bit for bit. The
/// result is row-major.
pub fn matmul_real(a: &ArrayView2<f64>, b: &ArrayView2<f64>) -> Result<Array2<f64>, CoreError> {
    debug_assert_eq!(a.ncols(), b.nrows());
    let dim = (a.nrows(), b.ncols());
    // Once per call, before the result buffer. The SME tile loop is unchanged.
    checked_elems(&[dim.0, dim.1], ELEM_F64)?;
    if use_sme(dim.0, a.ncols(), dim.1) {
        return Ok(sme::gemm(a, b));
    }
    let mut c = Array2::<f64>::zeros(dim);
    general_mat_mul(1.0, a, b, 0.0, &mut c);
    Ok(c)
}

/// Products with fewer multiply-adds (`m·k·n`) than this stay on
/// `matrixmultiply`: below it, entering and leaving streaming mode and
/// packing cost more than the matrix unit saves. On an Apple M4 Max the two
/// paths cross near 24³; 16³ is 13 % slower on SME and 32³ 28 % faster.
const SME_MIN_MACS: usize = 1 << 14;

/// Whether a real `m × k` by `k × n` product runs on the SME kernel.
fn use_sme(m: usize, k: usize, n: usize) -> bool {
    m.saturating_mul(k).saturating_mul(n) >= SME_MIN_MACS && sme::available()
}

/// The real parts of `a` when every imaginary part is zero (the same test
/// as [`is_real`]), `None` otherwise. One pass over the elements in memory
/// order, so a transposed view costs the same as the matrix itself; the
/// result keeps that memory order.
fn real_parts(a: &ArrayView2<C64>) -> Option<Array2<f64>> {
    let dim = a.dim();
    let layout_c = a.is_standard_layout();
    let layout_f = !layout_c && a.t().is_standard_layout();
    let slice = if layout_c || layout_f {
        a.as_slice_memory_order()
    } else {
        None
    };
    let Some(src) = slice else {
        return is_real(a).then(|| a.mapv(|z| z.re));
    };
    // No early exit, so the loop vectorizes; a complex operand is found
    // after one pass either way.
    let mut nonzero_im = false;
    let re: Vec<f64> = src
        .iter()
        .map(|z| {
            nonzero_im |= z.im != 0.0;
            z.re
        })
        .collect();
    if nonzero_im {
        return None;
    }
    let shape = if layout_c {
        dim.into_shape_with_order()
    } else {
        dim.f()
    };
    Some(Array2::from_shape_vec(shape, re).expect("length matches the shape"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(a: &CMatrix, b: &CMatrix) -> CMatrix {
        let (m, k, n) = (a.nrows(), a.ncols(), b.ncols());
        Array2::from_shape_fn((m, n), |(i, j)| (0..k).map(|p| a[[i, p]] * b[[p, j]]).sum())
    }

    /// Deterministic pseudo-random fill, so the test needs no RNG crate.
    fn filled(m: usize, n: usize, seed: u64, complex: bool) -> CMatrix {
        let mut s = seed;
        let mut next = move || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
        };
        Array2::from_shape_fn((m, n), |_| {
            let re = next();
            let im = if complex { next() } else { 0.0 };
            C64::new(re, im)
        })
    }

    fn assert_close(x: &CMatrix, y: &CMatrix, tol: f64) {
        assert_eq!(x.shape(), y.shape());
        for (p, q) in x.iter().zip(y.iter()) {
            assert!((p - q).norm() <= tol, "{p} vs {q}");
        }
    }

    #[test]
    fn real_path_matches_naive_on_many_shapes() {
        for &(m, k, n) in &[
            (1, 1, 1),
            (1, 7, 1),
            (5, 3, 4),
            (1, 64, 33),
            (65, 17, 1),
            (129, 128, 77),
        ] {
            let a = filled(m, k, 1 + m as u64, false);
            let b = filled(k, n, 2 + n as u64, false);
            let c = matmul(&a.view(), &b.view()).unwrap();
            assert_close(&c, &naive(&a, &b), 1e-12 * k as f64);
            assert!(c.iter().all(|z| z.im == 0.0), "real product must stay real");
        }
    }

    #[test]
    fn complex_and_mixed_operands_use_the_complex_product() {
        let a = filled(6, 5, 3, true);
        let b = filled(5, 4, 4, true);
        let br = filled(5, 4, 5, false);
        assert_close(
            &matmul(&a.view(), &b.view()).unwrap(),
            &naive(&a, &b),
            1e-12,
        );
        assert_close(
            &matmul(&a.view(), &br.view()).unwrap(),
            &naive(&a, &br),
            1e-12,
        );
        assert_close(
            &matmul(&br.t(), &a.t()).unwrap(),
            &naive(&br.t().to_owned(), &a.t().to_owned()),
            1e-12,
        );
    }

    #[test]
    fn transposed_views_work() {
        let a = filled(9, 4, 6, false);
        let b = filled(9, 3, 7, false);
        let c = matmul(&a.t(), &b.view()).unwrap();
        assert_close(&c, &naive(&a.t().to_owned(), &b), 1e-12);
    }

    /// A transposed view must give exactly the product of the transpose
    /// written out (the evaluator passes `A'` as a view), in row-major
    /// layout so later elementwise fast paths still apply.
    #[test]
    fn transposed_views_are_bit_identical_and_row_major() {
        for &(m, k, n) in &[
            (1, 1, 1),
            (3, 5, 2),
            (129, 128, 128),
            (65, 129, 33),
            (512, 129, 77),
        ] {
            for complex in [false, true] {
                let at = filled(k, m, 10 + m as u64, complex); // A = at'
                let b = filled(k, n, 20 + n as u64, complex);
                let bt = filled(n, k, 30 + k as u64, complex); // B = bt'
                let a = filled(m, k, 40 + k as u64, complex);
                let cases = [
                    (
                        matmul(&at.t(), &b.view()).unwrap(),
                        matmul(&at.t().as_standard_layout().view(), &b.view()).unwrap(),
                    ),
                    (
                        matmul(&a.view(), &bt.t()).unwrap(),
                        matmul(&a.view(), &bt.t().as_standard_layout().view()).unwrap(),
                    ),
                    (
                        matmul(&at.t(), &bt.t()).unwrap(),
                        matmul(
                            &at.t().as_standard_layout().view(),
                            &bt.t().as_standard_layout().view(),
                        )
                        .unwrap(),
                    ),
                ];
                for (view, copied) in cases {
                    assert!(
                        view.is_standard_layout(),
                        "{m}x{k}x{n} complex={complex}: not row-major"
                    );
                    assert_eq!(view.shape(), copied.shape());
                    for (p, q) in view.iter().zip(copied.iter()) {
                        assert!(
                            p.re.to_bits() == q.re.to_bits() && p.im.to_bits() == q.im.to_bits(),
                            "{m}x{k}x{n} complex={complex}: {p} vs {q}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn nan_propagates_and_inf_stays_real() {
        let mut a = filled(3, 3, 8, false);
        let b = filled(3, 3, 9, false);
        a[[0, 1]] = C64::new(f64::NAN, 0.0);
        let c = matmul(&a.view(), &b.view()).unwrap();
        assert!(c.row(0).iter().all(|z| z.re.is_nan()));
        assert!(c.row(1).iter().all(|z| z.re.is_finite()));

        let inf = Array2::from_elem((1, 1), C64::new(f64::INFINITY, 0.0));
        let one = Array2::from_elem((1, 1), C64::new(1.0, 0.0));
        let c = matmul(&inf.view(), &one.view()).unwrap();
        assert_eq!(c[[0, 0]], C64::new(f64::INFINITY, 0.0));
    }

    #[test]
    fn negative_zero_imaginary_counts_as_real() {
        let a = Array2::from_elem((2, 2), C64::new(1.0, -0.0));
        assert!(is_real(&a.view()));
        let n = Array2::from_elem((2, 2), C64::new(1.0, f64::NAN));
        assert!(!is_real(&n.view()));
    }
}

//! Dense matrix multiply for complex-stored values.
//!
//! Every rustlab matrix is stored as `Complex<f64>`, but most workloads are
//! real. A complex multiply does four times the floating-point work of a
//! real one and moves twice the bytes, so when both operands have no
//! imaginary part the product is formed from their real parts with a real
//! GEMM (ndarray's `f64` `dot`, which is `matrixmultiply::dgemm`) and widened
//! back. The check and the copies are O(n²) against O(n³) work.
//!
//! The real path sums in a different order from the complex kernel, so
//! results can differ in the last digits. It also differs where the complex
//! kernel manufactured NaN: `(Inf + 0i)·(1 + 0i)` has imaginary part
//! `Inf·0 = NaN` there, while the real product is `Inf` with a zero
//! imaginary part.

use crate::types::{CMatrix, C64};
use ndarray::{Array2, ArrayView2};

/// True when every element has an imaginary part of exactly zero
/// (`-0.0` counts as zero; NaN does not).
pub fn is_real(a: &ArrayView2<C64>) -> bool {
    a.iter().all(|z| z.im == 0.0)
}

/// `a · b` for complex-stored matrices. Uses a real GEMM when both operands
/// are real, otherwise the complex one. The caller checks that the inner
/// dimensions agree.
pub fn matmul(a: &ArrayView2<C64>, b: &ArrayView2<C64>) -> CMatrix {
    debug_assert_eq!(a.ncols(), b.nrows());
    if is_real(a) && is_real(b) {
        let ar: Array2<f64> = a.mapv(|z| z.re);
        let br: Array2<f64> = b.mapv(|z| z.re);
        ar.dot(&br).mapv(|x| C64::new(x, 0.0))
    } else {
        a.dot(b)
    }
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
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
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
        for &(m, k, n) in &[(1, 1, 1), (1, 7, 1), (5, 3, 4), (1, 64, 33), (65, 17, 1), (129, 128, 77)] {
            let a = filled(m, k, 1 + m as u64, false);
            let b = filled(k, n, 2 + n as u64, false);
            let c = matmul(&a.view(), &b.view());
            assert_close(&c, &naive(&a, &b), 1e-12 * k as f64);
            assert!(c.iter().all(|z| z.im == 0.0), "real product must stay real");
        }
    }

    #[test]
    fn complex_and_mixed_operands_use_the_complex_product() {
        let a = filled(6, 5, 3, true);
        let b = filled(5, 4, 4, true);
        let br = filled(5, 4, 5, false);
        assert_close(&matmul(&a.view(), &b.view()), &naive(&a, &b), 1e-12);
        assert_close(&matmul(&a.view(), &br.view()), &naive(&a, &br), 1e-12);
        assert_close(&matmul(&br.t(), &a.t()), &naive(&br.t().to_owned(), &a.t().to_owned()), 1e-12);
    }

    #[test]
    fn transposed_views_work() {
        let a = filled(9, 4, 6, false);
        let b = filled(9, 3, 7, false);
        let c = matmul(&a.t(), &b.view());
        assert_close(&c, &naive(&a.t().to_owned(), &b), 1e-12);
    }

    #[test]
    fn nan_propagates_and_inf_stays_real() {
        let mut a = filled(3, 3, 8, false);
        let b = filled(3, 3, 9, false);
        a[[0, 1]] = C64::new(f64::NAN, 0.0);
        let c = matmul(&a.view(), &b.view());
        assert!(c.row(0).iter().all(|z| z.re.is_nan()));
        assert!(c.row(1).iter().all(|z| z.re.is_finite()));

        let inf = Array2::from_elem((1, 1), C64::new(f64::INFINITY, 0.0));
        let one = Array2::from_elem((1, 1), C64::new(1.0, 0.0));
        let c = matmul(&inf.view(), &one.view());
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

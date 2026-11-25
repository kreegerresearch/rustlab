//! The NEON kernel for products that find every SME slot taken (`slots.rs`).
//!
//! It reads the panels `sme.rs` packed (16 rows of A, 16 columns of B, both
//! k-major) and writes the same padded row-major output. Each element of C
//! starts at +0 and takes one fused multiply-add per k, in k order, which
//! is exactly what the matrix unit's `fmopa` chain does. So a product gives
//! the same bits whichever kernel ran it, and routing by load cannot make a
//! `parmap` result depend on timing.
//!
//! A 16 × 16 block is computed as eight 4 × 8 register tiles: per k, two
//! loads of A (4 values), four loads of B (8 values) and 16 `fmla`s by lane
//! into 16 accumulators, enough independent chains to cover the FMA
//! latency on the four vector pipes of an M4 core.

/// Rows and columns of C per packed panel, as in `sme.rs`.
#[cfg(target_arch = "aarch64")]
const BLOCK: usize = 16;

/// `c[ib·16 + r, jb·16 + s] = Σ_kk pa(ib)[kk, r] · pb(jb)[kk, s]` for every
/// block, with `c` row-major at `ldc` elements per row.
#[cfg(target_arch = "aarch64")]
pub(super) fn kernel(
    pa: &[f64],
    pb: &[f64],
    c: &mut [f64],
    k: usize,
    mb: usize,
    nb: usize,
    ldc: usize,
) {
    assert!(pa.len() >= mb * BLOCK * k && pb.len() >= nb * BLOCK * k);
    assert!(ldc >= nb * BLOCK && c.len() >= (mb * BLOCK - 1) * ldc + nb * BLOCK);
    for jb in 0..nb {
        let panel_b = pb[jb * BLOCK * k..].as_ptr();
        for ib in 0..mb {
            let panel_a = pa[ib * BLOCK * k..].as_ptr();
            for r0 in (0..BLOCK).step_by(4) {
                for c0 in (0..BLOCK).step_by(8) {
                    let at = (ib * BLOCK + r0) * ldc + jb * BLOCK + c0;
                    // SAFETY: the asserts bound the 16k doubles of each
                    // panel and the 4 rows × 8 columns written at `at`
                    // (row ib·16 + r0 + 3 < mb·16, column < nb·16 ≤ ldc).
                    unsafe {
                        tile(
                            panel_a.add(r0),
                            panel_b.add(c0),
                            k,
                            c.as_mut_ptr().add(at),
                            ldc,
                        )
                    }
                }
            }
        }
    }
}

/// One 4 × 8 tile: `a` points at row 0 of the tile inside an A panel,
/// `b` at column 0 inside a B panel, both with 16 doubles per k.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn tile(a: *const f64, b: *const f64, k: usize, c: *mut f64, ldc: usize) {
    use std::arch::aarch64::*;
    // SAFETY: the caller guarantees 16k readable doubles from `a` and `b`
    // (minus the tile offset, which is under 16) and a writable 4 × 8 tile
    // at `c`.
    unsafe {
        let z = vdupq_n_f64(0.0);
        let mut acc = [[z; 4]; 4];
        for kk in 0..k {
            let (pa, pb) = (a.add(kk * BLOCK), b.add(kk * BLOCK));
            let a01 = vld1q_f64(pa);
            let a23 = vld1q_f64(pa.add(2));
            let bv = [
                vld1q_f64(pb),
                vld1q_f64(pb.add(2)),
                vld1q_f64(pb.add(4)),
                vld1q_f64(pb.add(6)),
            ];
            for j in 0..4 {
                acc[0][j] = vfmaq_laneq_f64::<0>(acc[0][j], bv[j], a01);
                acc[1][j] = vfmaq_laneq_f64::<1>(acc[1][j], bv[j], a01);
                acc[2][j] = vfmaq_laneq_f64::<0>(acc[2][j], bv[j], a23);
                acc[3][j] = vfmaq_laneq_f64::<1>(acc[3][j], bv[j], a23);
            }
        }
        for (i, row) in acc.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                vst1q_f64(c.add(i * ldc + 2 * j), *v);
            }
        }
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) fn kernel(_: &[f64], _: &[f64], _: &mut [f64], _: usize, _: usize, _: usize, _: usize) {
    unreachable!("the SME kernel, and so its overflow, is aarch64-only")
}

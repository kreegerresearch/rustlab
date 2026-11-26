//! Softmax kernels on real row-major data, and the `exp` they use.
//!
//! [`exp_f64`] is table driven: `e^x = 2^(k/128) * e^r` with `|r| <=
//! ln2/256`, the 128 values of `2^(j/128)` held as a double plus a
//! relative tail (computed at compile time in double-double arithmetic),
//! and a degree-5 Taylor polynomial for `e^r`. It is within one ulp of the
//! correctly rounded value; against glibc's `exp` about 0.1% of normal
//! results differ, by one ulp (`exp_matches_libm_within_one_ulp`). It uses
//! no fused multiply-add, so it gives the same bits on every platform,
//! which the platform `f64::exp` does not.
//!
//! The slice loop ([`exp_into`]) builds the normal-range scale from bits
//! and runs branch-free, so it vectorizes. Lanes below -746 are 0; lanes
//! outside `[NORMAL_MIN, NORMAL_MAX]` (results near or below the subnormal
//! range, overflow, `NaN`) are redone by [`exp_f64`] after it. The two
//! paths give the same bits.
//!
//! The softmax kernels compute `exp(x - max) / sum` with each sum taken in
//! the same left-to-right order as the scalar loop they replace, so with
//! the same `exp` they give the same bits (`row_kernel_matches_scalar_loop`).
//! Several rows (or, per column, all columns) are summed at once so the
//! dependent adds overlap.

/// `1.5 * 2^52`: adding it rounds to an integer held in the low mantissa bits.
const SHIFT: f64 = 6_755_399_441_055_744.0;
const SHIFT_BITS: u64 = SHIFT.to_bits();
const TABLE_BITS: u32 = 7;
const N: usize = 1 << TABLE_BITS;
const N_F: f64 = N as f64;
/// `N / ln 2`.
const INV_LN2_N: f64 = f64::from_bits(0x3ff7_1547_652b_82fe) * N_F;
/// `ln 2 / N` split so `k * LN2_HI_N` is exact for `|k| < 2^20`.
const LN2_HI_N: f64 = f64::from_bits(0x3fe6_2e42_fee0_0000) / N_F;
const LN2_LO_N: f64 = f64::from_bits(0x3dea_39ef_3579_3c76) / N_F;
const C3: f64 = 1.0 / 6.0;
const C4: f64 = 1.0 / 24.0;
const C5: f64 = 1.0 / 120.0;
/// Below this `exp` rounds to 0 (`ln 2^-1075` is -745.13).
const UNDERFLOW: f64 = -746.0;
/// Inside `[NORMAL_MIN, NORMAL_MAX]` the slice loop's scale is at least
/// `2^-967`, so any part of `scale * tmp` that can change the rounded sum
/// is a normal number, and the loop matches [`exp_f64`] bit for bit.
const NORMAL_MIN: f64 = -670.0;
const NORMAL_MAX: f64 = 709.7;

// Double-double arithmetic for building the table at compile time.
const fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}
const fn quick_two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    (s, b - (s - a))
}
const fn split(a: f64) -> (f64, f64) {
    let c = 134_217_729.0 * a;
    let h = c - (c - a);
    (h, a - h)
}
const fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    (p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
}
const fn dd_add(x: (f64, f64), y: (f64, f64)) -> (f64, f64) {
    let (s, e) = two_sum(x.0, y.0);
    quick_two_sum(s, e + x.1 + y.1)
}
const fn dd_mul(x: (f64, f64), y: (f64, f64)) -> (f64, f64) {
    let (p, e) = two_prod(x.0, y.0);
    quick_two_sum(p, e + x.0 * y.1 + x.1 * y.0)
}
const fn dd_div(x: (f64, f64), b: f64) -> (f64, f64) {
    let q1 = x.0 / b;
    let (p, pe) = two_prod(q1, b);
    let (s, se) = two_sum(x.0, -p);
    quick_two_sum(q1, (s + (se - pe + x.1)) / b)
}

/// `(2^(j/N) rounded, relative tail)`: `2^(j/N) = hi * (1 + tail)` to
/// about 1e-32.
const fn build_table() -> [(f64, f64); N] {
    let ln2 = (
        f64::from_bits(0x3fe6_2e42_fefa_39ef),
        f64::from_bits(0x3c7a_bc9e_3b39_803f),
    );
    let mut t = [(0.0, 0.0); N];
    let mut j = 0;
    while j < N {
        let a = dd_mul(ln2, (j as f64 / N_F, 0.0));
        let mut sum = (1.0, 0.0);
        let mut term = (1.0, 0.0);
        let mut n = 1;
        while n < 40 {
            term = dd_div(dd_mul(term, a), n as f64);
            sum = dd_add(sum, term);
            n += 1;
        }
        t[j] = (sum.0, sum.1 / sum.0);
        j += 1;
    }
    t
}

static TABLE: [(f64, f64); N] = build_table();

/// Reduce `x`: the shifted `k` (`k + SHIFT`, k = round(x * N / ln2)), the
/// table entry for `k mod N`, and `e^r - 1` relative to it (`tmp`), so
/// `e^x = 2^(k div N) * (s + s * tmp)`.
#[inline(always)]
fn reduce(x: f64) -> (u64, f64, f64) {
    let kd = x * INV_LN2_N + SHIFT;
    let kf = kd - SHIFT;
    let r = (x - kf * LN2_HI_N) - kf * LN2_LO_N;
    let ki = kd.to_bits();
    let (s, tail) = TABLE[(ki & (N as u64 - 1)) as usize];
    let r2 = r * r;
    let tmp = tail + r + r2 * (0.5 + r * C3) + (r2 * r2) * (C4 + r * C5);
    (ki, s, tmp)
}

/// `2^m` for an integer `m` in `[-1022, 1023]` held as `m + SHIFT`.
#[inline(always)]
fn pow2_shifted(md: f64) -> f64 {
    f64::from_bits(
        md.to_bits()
            .wrapping_sub(SHIFT_BITS)
            .wrapping_add(1023)
            .wrapping_shl(52),
    )
}

/// `e^x`, within one ulp of the correctly rounded value. `NaN` gives
/// `NaN`, `-Inf` and `x < -745.14` give 0, `+Inf` and `x > 709.78` give
/// `+Inf`.
#[inline]
pub fn exp_f64(x: f64) -> f64 {
    // The clamp keeps k inside the range the two-factor scale can build;
    // results at both clamps are already 0 or Inf. `clamp` keeps NaN.
    let x = x.clamp(-1100.0, 1000.0);
    let (ki, s, tmp) = reduce(x);
    let u = s + s * tmp;
    // 2^(k div N) as two normal factors, so only the last product rounds.
    let k = ((ki.wrapping_sub(SHIFT_BITS) as i64) >> TABLE_BITS) as f64;
    let k1d = k * 0.5 + SHIFT;
    let k2d = (k - (k1d - SHIFT)) + SHIFT;
    (u * pow2_shifted(k1d)) * pow2_shifted(k2d)
}

/// `dst[i] = exp_f64(src[i] - m)`.
pub fn exp_into(dst: &mut [f64], src: &[f64], m: f64) {
    let mut redo = false;
    for (d, &v) in dst.iter_mut().zip(src) {
        let x = v - m;
        let (ki, s, tmp) = reduce(x);
        // Adding (k div N) to the exponent of s: exact in the normal range.
        let scale = f64::from_bits(
            s.to_bits()
                .wrapping_add((ki.wrapping_sub(SHIFT_BITS) & !(N as u64 - 1)) << (52 - TABLE_BITS)),
        );
        let y = scale + scale * tmp;
        let normal = (NORMAL_MIN..=NORMAL_MAX).contains(&x);
        let zero = x < UNDERFLOW;
        redo |= !normal & !zero;
        *d = if zero { 0.0 } else { y };
    }
    if redo {
        for (d, &v) in dst.iter_mut().zip(src) {
            let x = v - m;
            if !(NORMAL_MIN..=NORMAL_MAX).contains(&x) {
                *d = exp_f64(x);
            }
        }
    }
}

/// The value `slice.iter().fold(-Inf, f64::max)` returns, for softmax:
/// `NaN` is skipped, and only the sign of a zero maximum can differ, which
/// leaves every `x - max` unchanged. `v > m` is one instruction per lane
/// where `f64::max` is several on x86.
#[inline]
fn gt_max(m: f64, v: f64) -> f64 {
    if v > m {
        v
    } else {
        m
    }
}

/// Lanes of [`max_of`]: enough independent compares to hide their latency.
const MAX_LANES: usize = 16;

/// Largest element (see [`gt_max`]), [`MAX_LANES`] at a time.
#[inline]
fn max_of(x: &[f64]) -> f64 {
    let mut m = [f64::NEG_INFINITY; MAX_LANES];
    let (chunks, rest) = x.as_chunks::<MAX_LANES>();
    for c in chunks {
        for (ml, &v) in m.iter_mut().zip(c) {
            *ml = gt_max(*ml, v);
        }
    }
    let mut r = f64::NEG_INFINITY;
    for &v in m.iter().chain(rest) {
        r = gt_max(r, v);
    }
    r
}

/// [`exp_into`] that writes the trailing run of exact zeros (`x - m <
/// -746`, the masked keys of a causal attention row) without computing them.
#[inline]
fn exp_masked(dst: &mut [f64], src: &[f64], m: f64) {
    let mut end = src.len();
    while end > 0 && src[end - 1] - m < UNDERFLOW {
        end -= 1;
    }
    exp_into(&mut dst[..end], &src[..end], m);
    dst[end..].fill(0.0);
}

/// Softmax of one slice in place.
pub fn softmax_slice(x: &mut [f64]) {
    let src = x.to_vec();
    exp_masked(x, &src, max_of(&src));
    let mut sum = 0.0_f64;
    for &v in x.iter() {
        sum += v;
    }
    for v in x.iter_mut() {
        *v /= sum;
    }
}

/// Rows summed together so their dependent adds overlap.
const ROW_BLOCK: usize = 8;

/// Softmax of each row of a row-major matrix with `ncols` columns, in place.
pub fn softmax_rows(out: &mut [f64], ncols: usize) {
    if ncols == 0 {
        return;
    }
    debug_assert_eq!(out.len() % ncols, 0);
    let mut src = vec![0.0; ncols];
    for block in out.chunks_mut(ROW_BLOCK * ncols) {
        let nb = block.len() / ncols;
        for row in block.chunks_exact_mut(ncols) {
            src.copy_from_slice(row);
            exp_masked(row, &src, max_of(&src));
        }
        let mut sums = [0.0_f64; ROW_BLOCK];
        if nb == ROW_BLOCK {
            for j in 0..ncols {
                for (b, s) in sums.iter_mut().enumerate() {
                    *s += block[b * ncols + j];
                }
            }
        } else {
            for (row, s) in block.chunks_exact(ncols).zip(sums.iter_mut()) {
                for &v in row {
                    *s += v;
                }
            }
        }
        for (row, &s) in block.chunks_exact_mut(ncols).zip(&sums) {
            for v in row {
                *v /= s;
            }
        }
    }
}

/// Softmax of each column of a row-major matrix with `ncols` columns, in
/// place, in the same per-column order as a loop over one column at a time.
pub fn softmax_cols(out: &mut [f64], ncols: usize) {
    if ncols == 0 {
        return;
    }
    let mut m = vec![f64::NEG_INFINITY; ncols];
    for row in out.chunks_exact(ncols) {
        for (mj, &v) in m.iter_mut().zip(row) {
            *mj = gt_max(*mj, v);
        }
    }
    let mut sums = vec![0.0_f64; ncols];
    let mut x = vec![0.0_f64; ncols];
    for row in out.chunks_exact_mut(ncols) {
        for ((xj, &v), &mj) in x.iter_mut().zip(row.iter()).zip(&m) {
            *xj = v - mj;
        }
        exp_into(row, &x, 0.0);
        for (s, &v) in sums.iter_mut().zip(row.iter()) {
            *s += v;
        }
    }
    for row in out.chunks_exact_mut(ncols) {
        for (v, &s) in row.iter_mut().zip(&sums) {
            *v /= s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ulps(a: f64, b: f64) -> u64 {
        if a == b || (a.is_nan() && b.is_nan()) {
            return 0;
        }
        (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs()
    }

    /// A deterministic spread of inputs: every binade the softmax sees
    /// (`x - max <= 0`), the positive side, and the subnormal window.
    fn sample_inputs() -> Vec<f64> {
        let mut s = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut v = Vec::new();
        for _ in 0..400_000 {
            v.push(-40.0 * next());
        }
        for _ in 0..100_000 {
            v.push(-next());
        }
        for _ in 0..100_000 {
            v.push(1420.0 * next() - 710.0);
        }
        for _ in 0..50_000 {
            v.push(-708.0 - 37.5 * next());
        }
        for _ in 0..50_000 {
            v.push((next() - 0.5) * 1e-6);
        }
        v
    }

    #[test]
    fn exp_matches_libm_within_one_ulp() {
        let xs = sample_inputs();
        let (mut diff, mut worst, mut normal, mut normal_diff) = (0usize, 0u64, 0usize, 0usize);
        for &x in &xs {
            let u = ulps(exp_f64(x), x.exp());
            worst = worst.max(u);
            diff += (u != 0) as usize;
            if (NORMAL_MIN..=NORMAL_MAX).contains(&x) {
                normal += 1;
                normal_diff += (u != 0) as usize;
            }
        }
        println!(
            "exp_f64 vs f64::exp: {normal_diff} of {normal} normal results differ, \
             {diff} of {} overall, worst {worst} ulp",
            xs.len()
        );
        assert!(worst <= 1, "worst {worst} ulp");
        // glibc: about 0.1% of normal results; subnormal ones round twice.
        assert!(normal_diff * 200 < normal, "{normal_diff} differ");
    }

    #[test]
    fn exp_special_values() {
        assert!(exp_f64(f64::NAN).is_nan());
        assert_eq!(exp_f64(f64::NEG_INFINITY), 0.0);
        assert_eq!(exp_f64(f64::INFINITY), f64::INFINITY);
        assert_eq!(exp_f64(-1e9), 0.0);
        assert_eq!(exp_f64(1e9), f64::INFINITY);
        assert_eq!(exp_f64(0.0), 1.0);
        assert_eq!(exp_f64(-0.0), 1.0);
        assert!(ulps(exp_f64(709.0), 709.0_f64.exp()) <= 1);
        assert_eq!(exp_f64(710.0), f64::INFINITY);
        for x in [-745.0, -744.5, -740.0, -720.0, -708.5, -746.0] {
            assert!(ulps(exp_f64(x), x.exp()) <= 1, "{x}");
        }
        assert_eq!(exp_f64(-746.0), 0.0);
        assert_eq!(exp_f64(-745.0), f64::from_bits(1));
    }

    #[test]
    fn slice_loop_matches_scalar_exp_bit_for_bit() {
        let mut xs = sample_inputs();
        xs.extend([
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -0.0,
            -1e9,
            1e9,
            -745.1,
            -708.4,
            709.8,
        ]);
        for m in [0.0, 3.5, -2.0] {
            let mut out = vec![0.0; xs.len()];
            exp_into(&mut out, &xs, m);
            for (&g, &x) in out.iter().zip(&xs) {
                let w = exp_f64(x - m);
                assert!(
                    g.to_bits() == w.to_bits() || (g.is_nan() && w.is_nan()),
                    "{x}"
                );
            }
        }
    }

    /// The scalar loop `softmax` used before, over a given `exp`.
    fn reference(x: &[f64], exp: fn(f64) -> f64) -> Vec<f64> {
        let m = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut out: Vec<f64> = x.iter().map(|&v| exp(v - m)).collect();
        let mut sum = 0.0;
        for &v in &out {
            sum += v;
        }
        for v in &mut out {
            *v /= sum;
        }
        out
    }

    /// Pseudo-random scores in about `[-8, 8)`, every third matrix with the
    /// causal mask of an attention head (`-1e9` above the diagonal).
    fn matrix(nrows: usize, ncols: usize, salt: usize) -> Vec<f64> {
        let mut s = 0x2545_f491_4f6c_dd1d_u64 ^ (salt as u64).wrapping_mul(0x9e37_79b9);
        let mut a: Vec<f64> = (0..nrows * ncols)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                16.0 * ((s >> 11) as f64 / (1u64 << 53) as f64) - 8.0
            })
            .collect();
        if salt % 3 == 1 {
            for i in 0..nrows {
                for j in (i + 1)..ncols {
                    a[i * ncols + j] -= 1e9;
                }
            }
        }
        a
    }

    #[test]
    fn row_kernel_matches_scalar_loop() {
        for (k, (nr, nc)) in [(1, 1), (3, 5), (8, 129), (129, 129), (17, 3), (9, 64)]
            .into_iter()
            .enumerate()
        {
            for salt in [3 * k, 3 * k + 1] {
                let a = matrix(nr, nc, salt);
                let mut out = a.clone();
                softmax_rows(&mut out, nc);
                for i in 0..nr {
                    let row = &a[i * nc..(i + 1) * nc];
                    let want = reference(row, exp_f64);
                    for j in 0..nc {
                        assert_eq!(out[i * nc + j].to_bits(), want[j].to_bits());
                    }
                }
                let mut s = a[..nc].to_vec();
                softmax_slice(&mut s);
                assert_eq!(s, reference(&a[..nc], exp_f64));
            }
        }
    }

    #[test]
    fn col_kernel_matches_scalar_loop() {
        for (nr, nc) in [(1, 1), (5, 3), (129, 129), (3, 17)] {
            let a = matrix(nr, nc, 7);
            let mut out = a.clone();
            softmax_cols(&mut out, nc);
            for j in 0..nc {
                let col: Vec<f64> = (0..nr).map(|i| a[i * nc + j]).collect();
                let want = reference(&col, exp_f64);
                for i in 0..nr {
                    assert_eq!(out[i * nc + j].to_bits(), want[i].to_bits());
                }
            }
        }
    }

    /// Against the libm loop the only difference is `exp`, at most one ulp
    /// per term, so each probability moves by a few 1e-16 at most.
    #[test]
    fn softmax_within_1e_15_of_libm_loop() {
        let mut worst = 0.0_f64;
        for (nr, nc) in [(129, 129), (64, 77), (7, 1000)] {
            for salt in 0..6 {
                let a = matrix(nr, nc, salt);
                let mut out = a.clone();
                softmax_rows(&mut out, nc);
                for i in 0..nr {
                    let want = reference(&a[i * nc..(i + 1) * nc], f64::exp);
                    for j in 0..nc {
                        let (g, w) = (out[i * nc + j], want[j]);
                        if w == 0.0 {
                            assert_eq!(g, 0.0);
                        } else {
                            worst = worst.max(((g - w) / w).abs());
                        }
                    }
                }
            }
        }
        println!("softmax vs libm loop: worst relative difference {worst:e}");
        assert!(worst <= 1e-15, "{worst:e}");
    }

    #[test]
    fn nan_and_inf_rows_follow_the_scalar_loop() {
        let rows: [&[f64]; 4] = [
            &[1.0, f64::NAN, 2.0],
            &[f64::INFINITY, 1.0, 2.0],
            &[f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
            &[f64::NEG_INFINITY, 0.0, 1.0],
        ];
        for row in rows {
            let mut out = row.to_vec();
            softmax_rows(&mut out, 3);
            let want = reference(row, f64::exp);
            for (g, w) in out.iter().zip(&want) {
                assert!(g.to_bits() == w.to_bits() || (g.is_nan() && w.is_nan()));
            }
        }
    }
}

//! Sort order and variance used by `sort`, `std`, and `var`.
//!
//! Real values sort numerically. A slice with any nonzero imaginary part
//! sorts by magnitude, then by phase. NaNs sort above every finite value
//! and above `+Inf`, so an ascending sort leaves them at the end and a
//! descending sort leaves them at the start. Equal keys keep their original
//! order (the index is the tie-break).
//!
//! Variance matches the three weights: `0` divides by `n−1`, `1`
//! divides by `n`, and a weight vector divides by the sum of the weights.
//! An empty slice is NaN. A single observation is 0.

use rustlab_core::C64;

use crate::error::ScriptError;

#[derive(Clone, Debug)]
pub(crate) enum Weight {
    /// Divide by `n − 1`. One observation is 0.
    Sample,
    /// Divide by `n`.
    Population,
    /// Nonnegative weights. Divide by their sum.
    Vector(Vec<f64>),
}

/// `true` when any imaginary part is a nonzero finite or infinite value.
/// A NaN imaginary part is a missing value, not a complex magnitude.
pub(crate) fn any_nonzero_imag(vals: &[C64]) -> bool {
    vals.iter().any(|z| z.im != 0.0 && !z.im.is_nan())
}

fn missing(z: C64) -> bool {
    z.re.is_nan() || z.im.is_nan()
}

/// Ascending order. NaN is greater than every number, including `+Inf`.
fn cmp_ascend(a: C64, b: C64, by_magnitude: bool) -> std::cmp::Ordering {
    match (missing(a), missing(b)) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) if by_magnitude => match a.norm().total_cmp(&b.norm()) {
            std::cmp::Ordering::Equal => a.arg().total_cmp(&b.arg()),
            other => other,
        },
        (false, false) => a.re.total_cmp(&b.re),
    }
}

/// Sorted values and the 0-based source index of each output slot.
/// Equal keys keep input order.
pub(crate) fn sort_indexed(vals: &[C64], descending: bool) -> (Vec<C64>, Vec<usize>) {
    let by_magnitude = any_nonzero_imag(vals);
    let mut order: Vec<usize> = (0..vals.len()).collect();
    order.sort_unstable_by(|&i, &j| {
        let mut ord = cmp_ascend(vals[i], vals[j], by_magnitude);
        if descending {
            ord = ord.reverse();
        }
        if ord == std::cmp::Ordering::Equal {
            i.cmp(&j)
        } else {
            ord
        }
    });
    let sorted = order.iter().map(|&i| vals[i]).collect();
    (sorted, order)
}

/// Same rules as [`variance`], on real storage. Bit-identical to the real
/// part of [`variance`] when every imaginary part is zero.
pub(crate) fn variance_f64(name: &str, vals: &[f64], weight: &Weight) -> Result<f64, ScriptError> {
    let n = vals.len();
    if n == 0 {
        return Ok(f64::NAN);
    }
    if vals.iter().any(|x| x.is_nan()) {
        return Ok(f64::NAN);
    }
    match weight {
        Weight::Sample | Weight::Population => {
            if n == 1 {
                return Ok(0.0);
            }
            let nf = n as f64;
            let mean = vals.iter().sum::<f64>() / nf;
            let ss: f64 = vals
                .iter()
                .map(|x| {
                    let d = x - mean;
                    d * d
                })
                .sum();
            let denom = match weight {
                Weight::Sample => nf - 1.0,
                Weight::Population => nf,
                Weight::Vector(_) => unreachable!(),
            };
            Ok(ss / denom)
        }
        Weight::Vector(w) => weighted_ss(
            name,
            n,
            w,
            |i| vals[i],
            |x, mean| {
                let d = x - mean;
                d * d
            },
        ),
    }
}

fn weighted_ss(
    name: &str,
    n: usize,
    w: &[f64],
    x_at: impl Fn(usize) -> f64,
    dev_sqr: impl Fn(f64, f64) -> f64,
) -> Result<f64, ScriptError> {
    if w.len() != n {
        return Err(ScriptError::type_err(format!(
            "{name}: weight vector has length {} but the axis has length {n}",
            w.len()
        )));
    }
    if w.iter().any(|wi| !wi.is_finite() || *wi < 0.0) {
        return Err(ScriptError::type_err(format!(
            "{name}: weights must be finite and nonnegative"
        )));
    }
    let sw: f64 = w.iter().sum();
    if sw == 0.0 {
        return Ok(f64::NAN);
    }
    let mut acc = 0.0;
    for i in 0..n {
        acc += x_at(i) * w[i];
    }
    let mean = acc / sw;
    let mut ss = 0.0;
    for i in 0..n {
        ss += w[i] * dev_sqr(x_at(i), mean);
    }
    Ok(ss / sw)
}

/// Ascending real order. NaN is greater than every number, including `+Inf`.
fn cmp_real(a: f64, b: f64) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => a.total_cmp(&b),
    }
}

/// Sorted real values. Equal keys, including NaNs, keep input order.
/// This is the one-output path: it does not build an index vector.
/// At 64 elements and above it is a stable 16-bit radix sort on [`rank_key`],
/// which has the same order as [`cmp_real`] and stays within 2× of
/// `sort_unstable` at a million and ten million elements.
pub(crate) fn sort_real_values(vals: &[f64], descending: bool) -> Vec<f64> {
    if vals.len() < 64 {
        let mut sorted = vals.to_vec();
        sorted.sort_by(|a, b| directed_cmp(*a, *b, descending));
        return sorted;
    }
    radix_sort_f64(vals, descending)
}

/// Sorted real values and the 0-based source index of each output slot.
/// Equal keys, including NaNs, keep input order.
pub(crate) fn sort_real(vals: &[f64], descending: bool) -> (Vec<f64>, Vec<usize>) {
    let mut idx: Vec<usize> = (0..vals.len()).collect();
    if vals.len() < 64 {
        idx.sort_by(|&i, &j| directed_cmp(vals[i], vals[j], descending).then(i.cmp(&j)));
    } else {
        let mut keys: Vec<u64> = vals
            .iter()
            .copied()
            .map(|x| rank_key(x, descending))
            .collect();
        radix_sort_usize(&mut idx, &mut keys);
    }
    let sorted = idx.iter().map(|&i| vals[i]).collect();
    (sorted, idx)
}

fn directed_cmp(a: f64, b: f64, descending: bool) -> std::cmp::Ordering {
    let ord = cmp_real(a, b);
    if descending {
        ord.reverse()
    } else {
        ord
    }
}

/// Integer rank with the same order as [`cmp_real`]: every NaN is above
/// `+Inf` and ties with every other NaN. The sign-bit transform orders
/// finite values and infinities, and keeps `-0` below `+0`.
fn rank_key(x: f64, descending: bool) -> u64 {
    if x.is_nan() {
        return if descending { 0 } else { u64::MAX };
    }
    let bits = x.to_bits();
    let ordered = if bits >> 63 == 0 {
        bits | (1u64 << 63)
    } else {
        !bits
    };
    if descending {
        !ordered
    } else {
        ordered
    }
}

fn radix_prefix(keys: &[u64], shift: u32, count: &mut [usize; 256]) {
    count.fill(0);
    for &k in keys {
        count[((k >> shift) & 0xff) as usize] += 1;
    }
    let mut sum = 0usize;
    for c in count.iter_mut() {
        let n = *c;
        *c = sum;
        sum += n;
    }
}

/// Four stable 16-bit passes. Only the values move; the rank is recomputed
/// from each value, which is cheaper than hauling a second key array.
fn radix_sort_f64(vals: &[f64], descending: bool) -> Vec<f64> {
    let n = vals.len();
    let mut cur = vals.to_vec();
    let mut nxt = vec![0.0; n];
    let mut count = vec![0usize; 65536];
    for shift in (0..64).step_by(16) {
        count.fill(0);
        for &v in &cur {
            let k = rank_key(v, descending);
            count[((k >> shift) & 0xffff) as usize] += 1;
        }
        let mut sum = 0usize;
        for c in count.iter_mut() {
            let k = *c;
            *c = sum;
            sum += k;
        }
        for &v in &cur {
            let k = rank_key(v, descending);
            let b = ((k >> shift) & 0xffff) as usize;
            let i = count[b];
            count[b] += 1;
            nxt[i] = v;
        }
        std::mem::swap(&mut cur, &mut nxt);
    }
    cur
}

fn radix_sort_usize(idx: &mut [usize], keys: &mut [u64]) {
    let n = idx.len();
    let mut tmp_i = vec![0usize; n];
    let mut tmp_k = vec![0u64; n];
    let mut count = [0usize; 256];
    for shift in (0..64).step_by(8) {
        radix_prefix(keys, shift, &mut count);
        for i in 0..n {
            let b = ((keys[i] >> shift) & 0xff) as usize;
            let dst = count[b];
            count[b] += 1;
            tmp_k[dst] = keys[i];
            tmp_i[dst] = idx[i];
        }
        idx.copy_from_slice(&tmp_i);
        keys.copy_from_slice(&tmp_k);
    }
}

/// Sample, population, or weighted variance. Empty input and a non-finite
/// weight sum are NaN. One unweighted observation is 0.
pub(crate) fn variance(name: &str, vals: &[C64], weight: &Weight) -> Result<f64, ScriptError> {
    let n = vals.len();
    if n == 0 {
        return Ok(f64::NAN);
    }
    if vals.iter().copied().any(missing) {
        return Ok(f64::NAN);
    }
    match weight {
        Weight::Sample | Weight::Population => {
            if n == 1 {
                return Ok(0.0);
            }
            let nf = n as f64;
            let mean = vals.iter().copied().sum::<C64>() / nf;
            let ss: f64 = vals.iter().map(|z| (*z - mean).norm_sqr()).sum();
            let denom = match weight {
                Weight::Sample => nf - 1.0,
                Weight::Population => nf,
                Weight::Vector(_) => unreachable!(),
            };
            Ok(ss / denom)
        }
        Weight::Vector(w) => {
            // Real and imaginary parts are weighted separately, then combined
            // as Σ w |z − mean|² / Σ w. A missing part is handled above.
            let re: Vec<f64> = vals.iter().map(|z| z.re).collect();
            let im: Vec<f64> = vals.iter().map(|z| z.im).collect();
            let re_ss = weighted_ss(
                name,
                n,
                w,
                |i| re[i],
                |x, mean| {
                    let d = x - mean;
                    d * d
                },
            )?;
            let im_ss = weighted_ss(
                name,
                n,
                w,
                |i| im[i],
                |x, mean| {
                    let d = x - mean;
                    d * d
                },
            )?;
            if re_ss.is_nan() {
                Ok(f64::NAN)
            } else {
                Ok(re_ss + im_ss)
            }
        }
    }
}

pub(crate) fn parse_weight(
    name: &str,
    v: &crate::eval::value::Value,
) -> Result<Weight, ScriptError> {
    use crate::eval::value::Value;
    match v {
        Value::Scalar(x) if *x == 0.0 => Ok(Weight::Sample),
        Value::Scalar(x) if *x == 1.0 => Ok(Weight::Population),
        Value::Scalar(x) => Err(ScriptError::type_err(format!(
            "{name}: weight must be 0, 1, or a nonnegative vector, got {x}"
        ))),
        Value::Vector(a) => weights_from_iter(name, a.iter().copied()),
        Value::Matrix(m) if m.nrows() == 1 || m.ncols() == 1 => {
            weights_from_iter(name, m.iter().copied())
        }
        other => Err(ScriptError::type_err(format!(
            "{name}: weight must be 0, 1, or a vector, got {}",
            other.type_name()
        ))),
    }
}

fn weights_from_iter(name: &str, it: impl Iterator<Item = C64>) -> Result<Weight, ScriptError> {
    let mut w = Vec::new();
    for z in it {
        if z.im != 0.0 || !z.re.is_finite() || z.re < 0.0 {
            return Err(ScriptError::type_err(format!(
                "{name}: weights must be real, finite, and nonnegative"
            )));
        }
        w.push(z.re);
    }
    Ok(Weight::Vector(w))
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex;

    fn c(re: f64, im: f64) -> C64 {
        Complex::new(re, im)
    }

    #[test]
    fn real_sort_puts_nan_last_and_keeps_ties() {
        let v = [c(2.0, 0.0), c(f64::NAN, 0.0), c(2.0, 0.0), c(-1.0, 0.0)];
        let (sorted, idx) = sort_indexed(&v, false);
        assert!(sorted[0].re == -1.0);
        assert!(sorted[1].re == 2.0 && idx[1] == 0);
        assert!(sorted[2].re == 2.0 && idx[2] == 2);
        assert!(sorted[3].re.is_nan());
    }

    #[test]
    fn descend_puts_nan_first() {
        let v = [c(1.0, 0.0), c(f64::NAN, 0.0), c(3.0, 0.0)];
        let (sorted, _) = sort_indexed(&v, true);
        assert!(sorted[0].re.is_nan());
        assert_eq!(sorted[1].re, 3.0);
        assert_eq!(sorted[2].re, 1.0);
    }

    #[test]
    fn complex_sort_is_magnitude_then_phase() {
        // |1+i| = √2, |-2| = 2, |1-i| = √2 with a negative phase.
        let v = [c(1.0, 1.0), c(-2.0, 0.0), c(1.0, -1.0)];
        let (sorted, _) = sort_indexed(&v, false);
        assert_eq!(sorted[0], c(1.0, -1.0));
        assert_eq!(sorted[1], c(1.0, 1.0));
        assert_eq!(sorted[2], c(-2.0, 0.0));
    }

    #[test]
    fn weighted_std_matches_the_published_column() {
        // MATLAB: A = [1 5; 3 7; -9 2], w = [1 1 0.5], first column std ≈ 4.49.
        let col = [c(1.0, 0.0), c(3.0, 0.0), c(-9.0, 0.0)];
        let w = Weight::Vector(vec![1.0, 1.0, 0.5]);
        let s = variance("std", &col, &w).unwrap().sqrt();
        assert!((s - 4.49).abs() < 1e-3, "{s}");
    }

    #[test]
    fn empty_variance_is_nan_and_one_point_is_zero() {
        assert!(variance("std", &[], &Weight::Sample).unwrap().is_nan());
        assert_eq!(
            variance("std", &[c(5.0, 0.0)], &Weight::Sample).unwrap(),
            0.0
        );
        let v = [c(1.0, 0.0), c(2.0, 0.0), c(3.0, 0.0)];
        let sample = variance("std", &v, &Weight::Sample).unwrap();
        let pop = variance("std", &v, &Weight::Population).unwrap();
        assert!((sample - 1.0).abs() < 1e-12);
        assert!((pop - (2.0 / 3.0)).abs() < 1e-12);
    }

    #[test]
    fn real_variance_matches_the_complex_real_part() {
        let xs = [1.0, 2.0, 3.0, 4.0];
        let zs: Vec<C64> = xs.iter().map(|&x| c(x, 0.0)).collect();
        for w in [
            Weight::Sample,
            Weight::Population,
            Weight::Vector(vec![1.0, 1.0, 0.5, 2.0]),
        ] {
            let a = variance_f64("std", &xs, &w).unwrap();
            let b = variance("std", &zs, &w).unwrap();
            assert_eq!(a.to_bits(), b.to_bits(), "{a} vs {b}");
        }
        assert!(variance_f64("std", &[1.0, f64::NAN, 3.0], &Weight::Sample)
            .unwrap()
            .is_nan());
        assert!(
            variance_f64("std", &[1.0, 2.0], &Weight::Vector(vec![0.0, 0.0]))
                .unwrap()
                .is_nan()
        );
    }

    #[test]
    fn radix_sort_matches_stable_comparison() {
        let mut v = vec![
            1.0,
            -0.0,
            0.0,
            f64::NAN,
            2.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1.0,
            f64::NAN,
            -3.0,
            2.0,
        ];
        for i in 0..200 {
            v.push((i % 17) as f64 - 8.0);
            if i % 23 == 0 {
                v.push(f64::NAN);
            }
        }
        for desc in [false, true] {
            let got = sort_real_values(&v, desc);
            let mut want = v.clone();
            want.sort_by(|a, b| directed_cmp(*a, *b, desc));
            assert_eq!(got.len(), want.len());
            for (a, b) in got.iter().zip(want.iter()) {
                assert_eq!(a.to_bits(), b.to_bits(), "desc={desc}");
            }
            let (sorted, idx) = sort_real(&v, desc);
            for (k, &i) in idx.iter().enumerate() {
                assert_eq!(sorted[k].to_bits(), v[i].to_bits());
            }
            for w in idx.windows(2) {
                let c = directed_cmp(v[w[0]], v[w[1]], desc);
                assert_ne!(c, std::cmp::Ordering::Greater);
                if c == std::cmp::Ordering::Equal {
                    assert!(w[0] < w[1]);
                }
            }
        }
    }

    #[test]
    fn sort_real_puts_nan_last_and_keeps_ties() {
        let v = [2.0, f64::NAN, 2.0, f64::NEG_INFINITY, f64::INFINITY];
        let (sorted, idx) = sort_real(&v, false);
        assert!(sorted[0].is_infinite() && sorted[0].is_sign_negative());
        assert_eq!(sorted[1], 2.0);
        assert_eq!(idx[1], 0);
        assert_eq!(sorted[2], 2.0);
        assert_eq!(idx[2], 2);
        assert!(sorted[3].is_infinite() && sorted[3].is_sign_positive());
        assert!(sorted[4].is_nan());
        let (desc, _) = sort_real(&v, true);
        assert!(desc[0].is_nan());
        assert!(desc[1].is_infinite() && desc[1].is_sign_positive());
    }

    /// Release timings for the phase-1 gates. Not part of `cargo test`.
    /// `cargo test -p rustlab-script --release --lib phase1_sort_and_std_timings -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn phase1_sort_and_std_timings() {
        use std::time::Instant;
        for &n in &[1_000_000usize, 10_000_000] {
            let mut data = vec![0.0; n];
            let mut x = 0.123_456_789_f64;
            for d in &mut data {
                x = (x * 1.7 + 0.3).fract();
                *d = x * 2.0 - 1.0;
            }
            let mut unstable = data.clone();
            // Warm the allocator and the caches, then take the best of three.
            unstable.sort_unstable_by(|a, b| a.total_cmp(b));
            let _ = sort_real_values(&data, false);
            let mut unstable_s = f64::MAX;
            let mut stable_s = f64::MAX;
            let mut stable = Vec::new();
            for _ in 0..3 {
                unstable.copy_from_slice(&data);
                let t0 = Instant::now();
                unstable.sort_unstable_by(|a, b| a.total_cmp(b));
                unstable_s = unstable_s.min(t0.elapsed().as_secs_f64());
                let t1 = Instant::now();
                stable = sort_real_values(&data, false);
                stable_s = stable_s.min(t1.elapsed().as_secs_f64());
            }
            let ratio = stable_s / unstable_s;
            eprintln!(
                "sort n={n} unstable={:.1} ms stable={:.1} ms ratio={ratio:.3}",
                unstable_s * 1e3,
                stable_s * 1e3
            );
            assert!(
                (stable[0] - unstable[0]).abs() < 1e-12,
                "stable and unstable disagree on the first finite value"
            );
            assert!(
                ratio <= 2.0,
                "stable sort of {n} is {ratio:.2}× sort_unstable (gate is 2×)"
            );
        }
        let n = 1_000_000usize;
        let mut data = vec![0.0; n];
        let mut x = 0.42_f64;
        for d in &mut data {
            x = (x * 1.3 + 0.17).fract();
            *d = x;
        }
        let complex: Vec<C64> = data.iter().map(|&r| c(r, 0.0)).collect();
        let _ = variance("std", &complex, &Weight::Sample).unwrap();
        let _ = variance_f64("std", &data, &Weight::Sample).unwrap();
        let mut old_s = f64::MAX;
        let mut new_s = f64::MAX;
        let mut old = 0.0;
        let mut new = 0.0;
        for _ in 0..5 {
            let t_old = Instant::now();
            old = variance("std", &complex, &Weight::Sample).unwrap();
            old_s = old_s.min(t_old.elapsed().as_secs_f64());
            let t_new = Instant::now();
            new = variance_f64("std", &data, &Weight::Sample).unwrap();
            new_s = new_s.min(t_new.elapsed().as_secs_f64());
        }
        let ratio = new_s / old_s;
        eprintln!(
            "std n={n} complex={:.2} ms real={:.2} ms ratio={ratio:.3} value={new}",
            old_s * 1e3,
            new_s * 1e3
        );
        assert_eq!(old.to_bits(), new.to_bits());
        assert!(
            ratio <= 1.10,
            "real std of {n} is {ratio:.2}× the complex loop (gate is 1.10×)"
        );
    }
}

//! Min/max envelope reduction for polylines sent to the external viewer.
//!
//! A line of 1e8 samples does not fit in one IPC frame (`MAX_MSG_LEN` is
//! 32 MiB; the msgpack of two `f64` arrays is about 18 bytes per sample).
//! The viewer also cannot zoom a buffer it never received, so the reduction
//! keeps the samples that define the shape on a wide panel: one bucket per
//! horizontal pixel, and both the lowest and highest y in that bucket.

/// Horizontal buckets for a viewer line. 4096 matches a 4K-wide panel, so a
/// narrower window is drawn with more than one bucket per pixel and a modest
/// zoom still follows the original curve.
pub(crate) const VIEWER_ENVELOPE_COLUMNS: usize = 4096;

/// At most a min and a max vertex per column.
pub(crate) const VIEWER_ENVELOPE_MAX_POINTS: usize = VIEWER_ENVELOPE_COLUMNS * 2;

/// Reduce `x`/`y` to about `columns` min/max buckets.
///
/// Samples are grouped in draw order. Each bucket keeps the original points
/// where y is lowest and highest, in index order, so a peak stays at its real
/// x and a monotonic stretch keeps its ends. The first and last sample of
/// each finite run are kept. A series no longer than `2 * columns` samples is
/// returned unchanged.
///
/// Non-finite samples split the line. A run of them becomes a single NaN so
/// the gap survives and a long run of NaNs cannot by itself overflow the
/// frame. When `x` and `y` differ in length, the extra tail is ignored.
pub(crate) fn envelope_decimate(x: &[f64], y: &[f64], columns: usize) -> (Vec<f64>, Vec<f64>) {
    let n = x.len().min(y.len());
    let x = &x[..n];
    let y = &y[..n];
    let max_points = if columns == VIEWER_ENVELOPE_COLUMNS {
        VIEWER_ENVELOPE_MAX_POINTS
    } else {
        columns.saturating_mul(2).max(2)
    };
    if columns == 0 || n <= max_points {
        return (x.to_vec(), y.to_vec());
    }

    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if x[i].is_finite() && y[i].is_finite() {
            let start = i;
            i += 1;
            while i < n && x[i].is_finite() && y[i].is_finite() {
                i += 1;
            }
            runs.push((start, i));
        } else {
            i += 1;
        }
    }
    if runs.is_empty() {
        return (vec![f64::NAN], vec![f64::NAN]);
    }

    let lens: Vec<usize> = runs.iter().map(|(a, b)| b - a).collect();
    let finite: usize = lens.iter().sum();
    // A long run of NaNs must not be copied verbatim just because the finite
    // samples already fit. Collapse those gaps below.
    let budgets = if finite <= max_points {
        lens.clone()
    } else {
        allocate_budgets(&lens, max_points)
    };
    let mut out_x = Vec::with_capacity(max_points + runs.len());
    let mut out_y = Vec::with_capacity(max_points + runs.len());
    let mut emitted = false;
    let mut cursor = 0usize;
    for (run_i, &(start, end)) in runs.iter().enumerate() {
        // Gaps (including a non-finite run) between finite runs become one NaN.
        if emitted && start > cursor {
            out_x.push(f64::NAN);
            out_y.push(f64::NAN);
        }
        let before = out_x.len();
        decimate_run(
            &x[start..end],
            &y[start..end],
            budgets[run_i],
            &mut out_x,
            &mut out_y,
        );
        if out_x.len() > before {
            emitted = true;
        }
        cursor = end;
    }
    (out_x, out_y)
}

/// Largest-remainder split of `max_points` across runs, proportional to length.
/// A positive-length run gets at least one point when there are enough points.
fn allocate_budgets(lens: &[usize], max_points: usize) -> Vec<usize> {
    let n = lens.len();
    let mut budgets = vec![0usize; n];
    if n == 0 || max_points == 0 {
        return budgets;
    }
    let total: usize = lens.iter().sum();
    if total == 0 {
        return budgets;
    }
    let mut remainders = vec![0u128; n];
    let mut used = 0usize;
    for (i, &len) in lens.iter().enumerate() {
        let prod = max_points as u128 * len as u128;
        let add = (prod / total as u128) as usize;
        let add = add.min(len);
        budgets[i] = add;
        used += add;
        remainders[i] = prod % total as u128;
    }
    let mut left = max_points.saturating_sub(used);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        remainders[b]
            .cmp(&remainders[a])
            .then(lens[b].cmp(&lens[a]))
            .then(a.cmp(&b))
    });
    for &i in &order {
        if left == 0 {
            break;
        }
        if budgets[i] < lens[i] {
            budgets[i] += 1;
            left -= 1;
        }
    }
    if n <= max_points {
        for i in 0..n {
            if lens[i] > 0 && budgets[i] == 0 {
                if let Some(j) = (0..n)
                    .filter(|&j| budgets[j] > 1)
                    .max_by_key(|&j| budgets[j])
                {
                    budgets[j] -= 1;
                    budgets[i] = 1;
                }
            }
        }
    }
    budgets
}

fn decimate_run(
    x: &[f64],
    y: &[f64],
    max_points: usize,
    out_x: &mut Vec<f64>,
    out_y: &mut Vec<f64>,
) {
    let n = x.len().min(y.len());
    if n == 0 || max_points == 0 {
        return;
    }
    if n <= max_points {
        out_x.extend_from_slice(&x[..n]);
        out_y.extend_from_slice(&y[..n]);
        return;
    }
    if max_points == 1 {
        let mut best = 0usize;
        let mut best_abs = y[0].abs();
        for (i, sample) in y.iter().enumerate().skip(1) {
            let a = sample.abs();
            if a > best_abs {
                best = i;
                best_abs = a;
            }
        }
        out_x.push(x[best]);
        out_y.push(y[best]);
        return;
    }

    let pairs = (max_points - 2) / 2;
    let mut idx = Vec::with_capacity(max_points);
    idx.push(0);
    if pairs > 0 {
        for b in 0..pairs {
            let start = b * n / pairs;
            let end = if b + 1 == pairs {
                n
            } else {
                (b + 1) * n / pairs
            };
            if start >= end {
                continue;
            }
            let mut min_i = start;
            let mut max_i = start;
            for i in start + 1..end {
                // A new low cannot also be a new high. Ties keep the earlier index.
                if y[i] < y[min_i] {
                    min_i = i;
                } else if y[i] > y[max_i] {
                    max_i = i;
                }
            }
            if min_i <= max_i {
                idx.push(min_i);
                idx.push(max_i);
            } else {
                idx.push(max_i);
                idx.push(min_i);
            }
        }
    }
    idx.push(n - 1);
    idx.sort_unstable();
    idx.dedup();
    for i in idx {
        out_x.push(x[i]);
        out_y.push(y[i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xs(n: usize) -> Vec<f64> {
        (0..n).map(|i| i as f64).collect()
    }

    #[test]
    fn short_series_is_copied() {
        let x = xs(5);
        let y = vec![1.0, 2.0, f64::NAN, 4.0, 5.0];
        let (ox, oy) = envelope_decimate(&x, &y, 4);
        assert_eq!(ox, x);
        assert_eq!(oy.len(), y.len());
        for (a, b) in oy.iter().zip(&y) {
            assert!(a.is_nan() && b.is_nan() || a == b);
        }
    }

    #[test]
    fn exactly_the_budget_is_copied() {
        let n = 8; // 2 * columns
        let x = xs(n);
        let y = xs(n);
        let (ox, oy) = envelope_decimate(&x, &y, 4);
        assert_eq!(ox, x);
        assert_eq!(oy, y);
    }

    #[test]
    fn empty_and_single_and_unequal() {
        let (ox, oy) = envelope_decimate(&[], &[], 4);
        assert!(ox.is_empty() && oy.is_empty());
        let (ox, oy) = envelope_decimate(&[3.0], &[4.0], 4);
        assert_eq!(ox, vec![3.0]);
        assert_eq!(oy, vec![4.0]);
        let (ox, oy) = envelope_decimate(&[0.0, 1.0, 2.0], &[9.0], 4);
        assert_eq!(ox, vec![0.0]);
        assert_eq!(oy, vec![9.0]);
    }

    #[test]
    fn spike_keeps_its_x_and_every_output_is_an_input() {
        let n = 1000;
        let x = xs(n);
        let mut y = vec![0.0; n];
        y[700] = 42.0;
        y[50] = -7.0;
        let (ox, oy) = envelope_decimate(&x, &y, 8);
        assert!(ox.len() <= 16, "len {}", ox.len());
        assert!(oy
            .iter()
            .zip(&ox)
            .any(|(yy, xx)| *yy == 42.0 && *xx == 700.0));
        assert!(oy
            .iter()
            .zip(&ox)
            .any(|(yy, xx)| *yy == -7.0 && *xx == 50.0));
        assert_eq!(ox[0], 0.0);
        assert_eq!(*ox.last().unwrap(), (n - 1) as f64);
        for (xx, yy) in ox.iter().zip(&oy) {
            let i = *xx as usize;
            assert_eq!(y[i], *yy);
        }
        let mut prev = -1.0;
        for xx in &ox {
            assert!(*xx >= prev);
            prev = *xx;
        }
    }

    #[test]
    fn sine_envelope_keeps_amplitude_and_ends() {
        let n = 50_000;
        let x: Vec<f64> = (0..n).map(|i| i as f64 / (n - 1) as f64).collect();
        // Many cycles, so a plain stride would miss the peaks.
        let y: Vec<f64> = x
            .iter()
            .map(|t| (t * 400.0 * std::f64::consts::PI).sin())
            .collect();
        let (ox, oy) = envelope_decimate(&x, &y, 32);
        assert!(ox.len() <= 64, "len {}", ox.len());
        let ymax = oy.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let ymin = oy.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(ymax > 0.99, "ymax {ymax}");
        assert!(ymin < -0.99, "ymin {ymin}");
        assert_eq!(ox[0], 0.0);
        assert!((ox.last().unwrap() - 1.0).abs() < 1e-12);
        // One slow cycle is locally monotonic inside a bucket, so the
        // envelope is a faithful polyline, not a filled band.
        let y_slow: Vec<f64> = x
            .iter()
            .map(|t| (2.0 * std::f64::consts::PI * t).sin())
            .collect();
        let (sx, sy) = envelope_decimate(&x, &y_slow, 32);
        let slow_max = sy.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(slow_max > 0.99, "slow max {slow_max}");
        assert_eq!(sx[0], 0.0);
        assert!((sx.last().unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn nan_gap_is_preserved_between_runs() {
        let n = 500;
        let x = xs(n);
        let mut y: Vec<f64> = (0..n).map(|i| i as f64).collect();
        for v in y.iter_mut().take(300).skip(200) {
            *v = f64::NAN;
        }
        let (ox, oy) = envelope_decimate(&x, &y, 4);
        let gap = oy.iter().position(|v| v.is_nan()).expect("gap");
        assert!(ox[gap].is_nan());
        assert!(oy[..gap].iter().all(|v| v.is_finite() && *v < 200.0));
        assert!(oy[gap + 1..].iter().all(|v| v.is_finite() && *v >= 300.0));
        assert!(oy.iter().filter(|v| v.is_finite()).count() <= 8);
    }

    #[test]
    fn long_nan_run_collapses_to_one_gap() {
        let n = 20_000;
        let x = xs(n);
        let mut y = vec![f64::NAN; n];
        y[0] = 1.0;
        y[1] = 2.0;
        y[2] = 3.0;
        y[n - 3] = 4.0;
        y[n - 2] = 5.0;
        y[n - 1] = 6.0;
        let (ox, oy) = envelope_decimate(&x, &y, 4);
        assert!(oy.len() < 20, "len {}", oy.len());
        assert_eq!(
            oy.iter().filter(|v| v.is_nan()).count(),
            1,
            "one gap, got {oy:?}"
        );
        let finite: Vec<f64> = oy.iter().copied().filter(|v| v.is_finite()).collect();
        assert_eq!(finite, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(ox[0], 0.0);
        assert_eq!(*ox.last().unwrap(), (n - 1) as f64);
    }

    #[test]
    fn monotonic_line_spans_the_domain() {
        let n = 10_000;
        let x = xs(n);
        let y: Vec<f64> = x.iter().map(|v| v * 0.5).collect();
        let (ox, oy) = envelope_decimate(&x, &y, 16);
        assert!(ox.len() <= 32);
        assert_eq!(ox[0], 0.0);
        assert_eq!(*ox.last().unwrap(), (n - 1) as f64);
        assert_eq!(oy[0], 0.0);
        assert_eq!(*oy.last().unwrap(), (n - 1) as f64 * 0.5);
        for w in oy.windows(2) {
            assert!(w[1] >= w[0]);
        }
    }

    /// Local timing for the reported 1e8-point line. Not part of CI.
    #[test]
    #[ignore]
    fn envelope_decimate_1e8_timing() {
        let n = 100_000_000usize;
        eprintln!("allocating {n} points");
        let x: Vec<f64> = (0..n).map(|i| i as f64 / (n - 1) as f64).collect();
        let y: Vec<f64> = x
            .iter()
            .map(|t| (2.0 * std::f64::consts::PI * t).sin())
            .collect();
        let rss = |label: &str| {
            if let Ok(text) = std::fs::read_to_string("/proc/self/status") {
                if let Some(line) = text.lines().find(|l| l.starts_with("VmRSS:")) {
                    eprintln!("{label}: {line}");
                }
            }
        };
        rss("after alloc");
        let start = std::time::Instant::now();
        let (ox, oy) = envelope_decimate(&x, &y, VIEWER_ENVELOPE_COLUMNS);
        let elapsed = start.elapsed();
        rss("after decimate");
        eprintln!(
            "decimate {n} -> {} points in {elapsed:?} (columns {VIEWER_ENVELOPE_COLUMNS})",
            ox.len()
        );
        let ymax = oy.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let ymin = oy.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(ymax > 0.999, "ymax {ymax}");
        assert!(ymin < -0.999, "ymin {ymin}");
        assert_eq!(ox[0], 0.0);
        assert!((ox.last().unwrap() - 1.0).abs() < 1e-9);
        assert!(ox.len() <= VIEWER_ENVELOPE_MAX_POINTS);
        assert_eq!(ox.len(), oy.len());
    }
}

//! Softmax kernels on a 129 x 129 matrix (one attention head's scores)
//! against the scalar libm loop `softmax` used before 0.3.91. Best of 200.
//!
//! cargo run --release -p rustlab-core --example bench_softmax
use rustlab_core::softmax::{exp_into, softmax_rows};
use std::hint::black_box;
use std::time::Instant;

fn libm_rows(x: &mut [f64], n: usize) {
    for row in x.chunks_exact_mut(n) {
        let m = row.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut s = 0.0;
        for v in row.iter_mut() {
            *v = (*v - m).exp();
            s += *v;
        }
        for v in row.iter_mut() {
            *v /= s;
        }
    }
}

fn time(label: &str, src: &[f64], f: &dyn Fn(&mut [f64])) {
    let mut buf = src.to_vec();
    let mut best = f64::INFINITY;
    for _ in 0..200 {
        buf.copy_from_slice(src);
        let t = Instant::now();
        f(black_box(&mut buf));
        best = best.min(t.elapsed().as_secs_f64());
        black_box(&buf);
    }
    println!("{label:<28} {:8.2} us", best * 1e6);
}

fn main() {
    let n = 129;
    let scores: Vec<f64> = (0..n * n)
        .map(|i| ((i * 37 % 211) as f64 - 105.0) * 0.07)
        .collect();
    let mut causal = scores.clone();
    for i in 0..n {
        for j in (i + 1)..n {
            causal[i * n + j] -= 1e9;
        }
    }
    time("libm loop", &scores, &|b| libm_rows(b, n));
    time("softmax_rows", &scores, &|b| softmax_rows(b, n));
    time("libm loop, causal mask", &causal, &|b| libm_rows(b, n));
    time("softmax_rows, causal mask", &causal, &|b| {
        softmax_rows(b, n)
    });
    let src = scores.clone();
    time("exp_into (16641 values)", &scores, &|b| {
        exp_into(b, &src, 0.0)
    });
    time("f64::exp (16641 values)", &scores, &|b| {
        for v in b.iter_mut() {
            *v = v.exp();
        }
    });
}

//! Vector-calculus kernel benchmarks. Run via:
//!
//!   cargo run --release --example bench_vector_calc -p rustlab-dsp
//!
//! Reports wall-clock times for `gradient_2d`, `divergence_2d`, `curl_2d`,
//! and `divergence_3d` at a range of grid sizes. Phase 3 of
//! `dev/plans/em_performance.md` rewrote these kernels to use slice
//! iteration, fused single-sweep `divergence`/`curl`, and rayon outer-axis
//! parallelism above a threshold.
//!
//! For a comparison against the pre-Phase-3 baseline, run this same
//! example on commit `ddb78f8` (last commit before Phase 3) and diff
//! the output by hand.

use num_complex::Complex;
use rustlab_core::{CMatrix, CTensor3};
use rustlab_dsp::vector_calc::{curl_2d, divergence_2d, divergence_3d, gradient_2d};
use std::time::Instant;

fn fill_2d(ny: usize, nx: usize, seed: u64) -> CMatrix {
    // Deterministic LCG so repeated runs measure the same workload.
    let mut state = seed.wrapping_add(1);
    CMatrix::from_shape_fn((ny, nx), |_| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        Complex::new(((state >> 33) as f64) / (u32::MAX as f64) - 0.5, 0.0)
    })
}

fn fill_3d(m: usize, n: usize, p: usize, seed: u64) -> CTensor3 {
    let mut state = seed.wrapping_add(1);
    CTensor3::from_shape_fn((m, n, p), |_| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        Complex::new(((state >> 33) as f64) / (u32::MAX as f64) - 0.5, 0.0)
    })
}

fn time_ms<F: FnOnce()>(f: F) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    println!("Vector-calculus kernel benchmarks (release build)");
    println!();
    println!("== 2-D ==");
    println!(
        "{:>10} {:>14} {:>14} {:>14}",
        "N×N", "gradient (ms)", "divergence (ms)", "curl (ms)"
    );
    for &n in &[50_usize, 100, 200, 400, 800] {
        let f = fill_2d(n, n, 1);
        let fx = fill_2d(n, n, 2);
        let fy = fill_2d(n, n, 3);
        let dx = 0.01;
        let dy = 0.01;

        // Warm-up
        let _ = gradient_2d(&f, dx, dy).unwrap();
        let _ = divergence_2d(&fx, &fy, dx, dy).unwrap();
        let _ = curl_2d(&fx, &fy, dx, dy).unwrap();

        // Best of 3
        let runs = 3;
        let mut t_grad = f64::INFINITY;
        let mut t_div = f64::INFINITY;
        let mut t_curl = f64::INFINITY;
        for _ in 0..runs {
            t_grad = t_grad.min(time_ms(|| {
                let _ = gradient_2d(&f, dx, dy).unwrap();
            }));
            t_div = t_div.min(time_ms(|| {
                let _ = divergence_2d(&fx, &fy, dx, dy).unwrap();
            }));
            t_curl = t_curl.min(time_ms(|| {
                let _ = curl_2d(&fx, &fy, dx, dy).unwrap();
            }));
        }
        println!("{:>10} {:>14.3} {:>14.3} {:>14.3}", n, t_grad, t_div, t_curl);
    }

    println!();
    println!("== 3-D divergence ==");
    println!("{:>14} {:>16}", "shape", "divergence_3d (ms)");
    for &n in &[20_usize, 40, 60, 80] {
        let fx = fill_3d(n, n, n, 1);
        let fy = fill_3d(n, n, n, 2);
        let fz = fill_3d(n, n, n, 3);
        let _ = divergence_3d(&fx, &fy, &fz, 0.01, 0.01, 0.01).unwrap();
        let runs = 3;
        let mut t = f64::INFINITY;
        for _ in 0..runs {
            t = t.min(time_ms(|| {
                let _ = divergence_3d(&fx, &fy, &fz, 0.01, 0.01, 0.01).unwrap();
            }));
        }
        println!("{:>5}×{:>3}×{:>3} {:>16.3}", n, n, n, t);
    }
}

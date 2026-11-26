//! Summed matrix-multiply throughput when several threads multiply at once,
//! for each SME slot cap. The workload of `rustlab_llm`'s
//! `par.matmul_129x128x512_B8`: every worker runs `--products` products of
//! the same shape, and the wall time of the slowest worker counts.
//!
//! ```sh
//! cargo run --release -p rustlab-core --example bench_matmul_scaling
//! cargo run --release -p rustlab-core --example bench_matmul_scaling -- \
//!     --shape 129,128,512 --workers 1,2,4,8,14 --slots 0,1,2,3,4 --reps 7
//! ```
//!
//! `--slots 0` is no cap (every product on SME, the 0.3.90 behaviour).
//! `--overflow N` sets the largest product (in multiply-adds) that may move
//! to NEON. Without SME (x86, Linux on Arm) the cap does nothing and every
//! row of one worker count is the same kernel.

use ndarray::Array2;
use rustlab_core::matmul;
use std::sync::Barrier;
use std::time::Instant;

fn list(v: &str) -> Vec<usize> {
    v.split(',')
        .map(|x| {
            x.trim()
                .parse()
                .expect("a comma-separated list of integers")
        })
        .collect()
}

fn filled(m: usize, n: usize, seed: u64) -> Array2<f64> {
    let mut s = seed;
    Array2::from_shape_fn((m, n), |_| {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((s >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    })
}

fn main() {
    let mut shape = vec![129, 128, 512];
    let mut workers = vec![1, 2, 4, 8, 14];
    let mut slot_caps: Option<Vec<usize>> = None;
    let (mut products, mut reps) = (20, 7);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--shape" => shape = list(&val()),
            "--workers" => workers = list(&val()),
            "--slots" => slot_caps = Some(list(&val())),
            "--products" => products = val().parse().expect("--products N"),
            "--reps" => reps = val().parse().expect("--reps N"),
            "--overflow" => matmul::set_sme_overflow_macs(val().parse().expect("--overflow N")),
            other => panic!("unknown option {other}"),
        }
    }
    assert_eq!(shape.len(), 3, "--shape m,k,n");
    let (m, k, n) = (shape[0], shape[1], shape[2]);
    let default_cap = matmul::sme_slots();
    let caps = slot_caps.unwrap_or_else(|| {
        let mut c = vec![0, 1, 2, 3, 4];
        if !c.contains(&default_cap) {
            c.push(default_cap);
        }
        c
    });
    let a = filled(m, k, 1);
    let b = filled(k, n, 2);
    let want = matmul::matmul_real(&a.view(), &b.view()).unwrap();
    let gflop = 2.0 * (m * k * n) as f64 / 1e9;

    println!(
        "{m}x{k}x{n}, {products} products per worker, median of {reps}; SME {}; default slots {default_cap}; overflow up to {} MACs",
        if matmul::sme_available() { "on" } else { "off" },
        matmul::sme_overflow_macs()
    );
    println!(
        "{:>5} {:>7} {:>12} {:>10} {:>8}",
        "slots", "workers", "us/product", "GFLOP/s", "NEON %"
    );
    for &cap in &caps {
        matmul::set_sme_slots(cap);
        for &w in &workers {
            let (s0, n0) = matmul::sme_route_counts();
            let mut times = Vec::with_capacity(reps);
            for rep in 0..=reps {
                let barrier = Barrier::new(w + 1);
                let t = std::thread::scope(|s| {
                    for _ in 0..w {
                        s.spawn(|| {
                            barrier.wait();
                            let mut acc = 0.0;
                            for _ in 0..products {
                                let c = matmul::matmul_real(&a.view(), &b.view()).unwrap();
                                acc += c[[0, 0]];
                            }
                            std::hint::black_box(acc);
                        });
                    }
                    barrier.wait();
                    Instant::now()
                });
                // The first round warms the per-thread scratch buffers.
                if rep > 0 {
                    times.push(t.elapsed().as_secs_f64());
                }
            }
            times.sort_by(f64::total_cmp);
            let t = times[times.len() / 2];
            let (s1, n1) = matmul::sme_route_counts();
            let routed = (s1 - s0) + (n1 - n0);
            let neon = if routed > 0 {
                format!("{:.0}", 100.0 * (n1 - n0) as f64 / routed as f64)
            } else {
                "-".into()
            };
            println!(
                "{cap:>5} {w:>7} {:>12.1} {:>10.0} {neon:>8}",
                1e6 * t / products as f64,
                (w * products) as f64 * gflop / t
            );
        }
    }
    matmul::set_sme_slots(default_cap);
    // A last check that every route gave the same bits.
    let got = matmul::matmul_real(&a.view(), &b.view()).unwrap();
    assert!(got
        .iter()
        .zip(want.iter())
        .all(|(p, q)| p.to_bits() == q.to_bits()));
}

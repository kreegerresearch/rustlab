//! Laplacian builder build-time benchmarks. Run via:
//!
//!   cargo run --release --example bench_laplacian_build -p rustlab-dsp
//!
//! Reports wall-clock times for `laplacian_1d`, `laplacian_2d_bc`,
//! `laplacian_3d`, and `laplacian_eps_3d` (complex- and real-stored ε)
//! at a range of grid sizes. Phase 4 of
//! `dev/plans/em_performance.md` rewrote the builders to emit entries
//! in row-major-then-column-major sorted order and call
//! `SparseMat::from_sorted_entries`, skipping the HashMap dedupe + full
//! sort that `SparseMat::new` does.

use ndarray::Array3;
use num_complex::Complex;
use rustlab_dsp::laplacian::{
    laplacian_1d, laplacian_2d_bc, laplacian_3d, laplacian_eps_3d, laplacian_eps_3d_real,
    BoundaryCondition,
};
use std::time::Instant;

fn time_ms<F: FnOnce()>(f: F) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    println!("Laplacian build benchmarks (release build)");
    println!();
    println!("== 1-D ==");
    println!("{:>10} {:>14}", "n", "build (ms)");
    for &n in &[1_000_usize, 10_000, 100_000, 1_000_000] {
        let _ = laplacian_1d(n, 0.01, BoundaryCondition::Dirichlet).unwrap();
        let runs = 3;
        let mut t = f64::INFINITY;
        for _ in 0..runs {
            t = t.min(time_ms(|| {
                let _ = laplacian_1d(n, 0.01, BoundaryCondition::Dirichlet).unwrap();
            }));
        }
        println!("{:>10} {:>14.3}", n, t);
    }

    println!();
    println!("== 2-D ==");
    println!("{:>10} {:>14}", "N×N", "build (ms)");
    for &n in &[50_usize, 100, 200, 400, 800] {
        let _ = laplacian_2d_bc(n, n, 0.01, 0.01, BoundaryCondition::Dirichlet).unwrap();
        let runs = 3;
        let mut t = f64::INFINITY;
        for _ in 0..runs {
            t = t.min(time_ms(|| {
                let _ =
                    laplacian_2d_bc(n, n, 0.01, 0.01, BoundaryCondition::Dirichlet).unwrap();
            }));
        }
        println!("{:>10} {:>14.3}", n, t);
    }

    println!();
    println!("== 3-D ==");
    println!(
        "{:>14} {:>14} {:>16} {:>16}",
        "N×N×N", "build (ms)", "eps cplx (ms)", "eps real (ms)"
    );
    for &n in &[20_usize, 40, 60, 80, 100] {
        let bc = BoundaryCondition::Dirichlet;
        let best = |f: &dyn Fn()| {
            f();
            (0..3).map(|_| time_ms(f)).fold(f64::INFINITY, f64::min)
        };
        let t = best(&|| {
            let _ = laplacian_3d(n, n, n, 0.01, 0.01, 0.01, bc).unwrap();
        });
        // Two-layer stack, the Lesson 15 shape: ε = 25 below, 4 above.
        let eps_real =
            Array3::from_shape_fn((n, n, n), |(_, _, k)| if k < n / 2 { 25.0 } else { 4.0 });
        let eps_cplx = eps_real.mapv(|v| Complex::new(v, 0.0));
        let te = best(&|| {
            let _ = laplacian_eps_3d(&eps_cplx, 0.01, 0.01, 0.01, bc).unwrap();
        });
        let tr = best(&|| {
            let _ = laplacian_eps_3d_real(&eps_real, 0.01, 0.01, 0.01, bc).unwrap();
        });
        println!(
            "{:>5}×{:>3}×{:>3} {:>14.3} {:>16.3} {:>16.3}",
            n, n, n, t, te, tr
        );
    }
}

//! Serial 3-D FDTD step throughput. Run via:
//!
//!   cargo run --release --example bench_fdtd -p rustlab-dsp
//!
//! 60×60×20 vacuum grid (the patch-lesson mesh of `dev/plans/fdtd3d.md`),
//! one soft source and one probe, best of five runs of 2000 steps.
//! Reports cell updates per second. Phase 6 threads this kernel; this is
//! the serial baseline.

use rustlab_dsp::fdtd::{Component, FdtdSim, SourceKind, Waveform, DEFAULT_COURANT};
use std::time::Instant;

fn main() {
    let (nx, ny, nz) = (60, 60, 20);
    let steps = 2000;
    let mut best = f64::INFINITY;
    for _ in 0..5 {
        let mut sim = FdtdSim::new(nx, ny, nz, 1e-3, 1e-3, 0.8e-3, DEFAULT_COURANT).unwrap();
        let tau = 20.0 * sim.dt();
        sim.add_source(
            Component::Ez,
            &[(ny / 2, nx / 2, nz / 2)],
            SourceKind::Soft,
            Waveform::Pulse {
                t0: 4.0 * tau,
                tau,
                f0: 0.0,
            },
        )
        .unwrap();
        sim.add_probe(Component::Ez, ny / 2, nx / 2 + 6, nz / 2)
            .unwrap();
        let start = Instant::now();
        let report = sim.run(steps, &mut || false).unwrap();
        let secs = start.elapsed().as_secs_f64();
        assert_eq!(report.completed, steps);
        best = best.min(secs);
    }
    let cells = (nx * ny * nz) as f64;
    println!(
        "{nx}×{ny}×{nz}, {steps} steps: best {:.3} s, {:.1} Mcell/s, {:.2} ms/step",
        best,
        cells * steps as f64 / best / 1e6,
        best / steps as f64 * 1e3
    );
    println!(
        "8000 steps (Phase 6 gate workload) ≈ {:.1} s serial",
        best / steps as f64 * 8000.0
    );
}

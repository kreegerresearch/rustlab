//! Serial 3-D FDTD step throughput. Run via:
//!
//!   cargo run --release --example bench_fdtd -p rustlab-dsp
//!
//! 60×60×20 grid (the patch-lesson mesh of `dev/plans/fdtd3d.md`), one soft
//! source and one probe, best of five runs of 2000 steps, for each media
//! configuration the kernel distinguishes:
//!
//! - vacuum: no coefficient arrays (the Phase 1 kernel);
//! - PEC: a ground face and a patch, so `Cb` arrays (lossless path);
//! - dielectric: a 4-cell `eps_r = 4.4` substrate (lossless path);
//! - dielectric + mu: the substrate plus `mu_r = 2` (adds `Db` arrays);
//! - lossy: the substrate with `sigma > 0` (adds `Ca` arrays).
//!
//! Reports cell updates per second. Phase 6 threads this kernel; this is the
//! serial baseline.

use rustlab_dsp::fdtd::{Axis, CellMap, Component, FdtdSim, SourceKind, Waveform, DEFAULT_COURANT};
use std::time::Instant;

const NX: usize = 60;
const NY: usize = 60;
const NZ: usize = 20;
const NSUB: usize = 4;

fn substrate(inside: f64, outside: f64) -> CellMap {
    let mut v = vec![outside; NX * NY * NZ];
    for (c, x) in v.iter_mut().enumerate() {
        if c / (NX * NY) < NSUB {
            *x = inside;
        }
    }
    CellMap::Cells(v)
}

fn build(case: &str) -> FdtdSim {
    let mut sim = FdtdSim::new(NX, NY, NZ, 1e-3, 1e-3, 0.8e-3, DEFAULT_COURANT).unwrap();
    match case {
        "vacuum" => {}
        "PEC" => {
            sim.add_pec_face(Axis::Z, 1, &vec![true; NX * NY]).unwrap();
            let patch: Vec<bool> = (0..NX * NY)
                .map(|c| (20..40).contains(&(c / NX)) && (15..45).contains(&(c % NX)))
                .collect();
            sim.add_pec_face(Axis::Z, NSUB + 1, &patch).unwrap();
        }
        "dielectric" => sim
            .set_media(
                substrate(4.4, 1.0),
                CellMap::Uniform(1.0),
                CellMap::Uniform(0.0),
            )
            .unwrap(),
        "dielectric + mu" => sim
            .set_media(
                substrate(4.4, 1.0),
                substrate(2.0, 1.0),
                CellMap::Uniform(0.0),
            )
            .unwrap(),
        "lossy" => sim
            .set_media(
                substrate(4.4, 1.0),
                CellMap::Uniform(1.0),
                substrate(0.01, 0.0),
            )
            .unwrap(),
        _ => unreachable!(),
    }
    let tau = 20.0 * sim.dt();
    sim.add_source(
        Component::Ez,
        &[(NY / 2, NX / 2, NZ / 2)],
        SourceKind::Soft,
        Waveform::Pulse {
            t0: 4.0 * tau,
            tau,
            f0: 0.0,
        },
    )
    .unwrap();
    sim.add_probe(Component::Ez, NY / 2, NX / 2 + 6, NZ / 2)
        .unwrap();
    sim
}

fn main() {
    let steps = 2000;
    let cells = (NX * NY * NZ) as f64;
    let mut vacuum = 0.0;
    for case in ["vacuum", "PEC", "dielectric", "dielectric + mu", "lossy"] {
        let mut best = f64::INFINITY;
        for _ in 0..5 {
            let mut sim = build(case);
            let start = Instant::now();
            let report = sim.run(steps, &mut || false).unwrap();
            let secs = start.elapsed().as_secs_f64();
            assert_eq!(report.completed, steps);
            best = best.min(secs);
        }
        let rate = cells * steps as f64 / best / 1e6;
        if case == "vacuum" {
            vacuum = rate;
        }
        println!(
            "{case:>16}: {NX}×{NY}×{NZ}, {steps} steps: best {best:.3} s, {rate:.1} Mcell/s \
             ({:.0}% of vacuum), {:.2} ms/step",
            100.0 * rate / vacuum,
            best / steps as f64 * 1e3
        );
    }
}

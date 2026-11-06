//! Serial 3-D FDTD step throughput, with and without the CPML absorber.
//!
//!   cargo run --release --example bench_fdtd -p rustlab-dsp
//!   cargo run --release --example bench_fdtd -p rustlab-dsp -- gate
//!
//! The first form times a 60×60×20 grid (the patch-lesson mesh of
//! `dev/plans/fdtd3d.md`), one soft source and one probe, best of five
//! runs of 2000 steps. Each media configuration the kernel distinguishes
//! is timed twice: CPML off (the Phase 2 kernel; the slab correction is
//! not called) and CPML on (`set_cpml(8)`, full auxiliaries on all six
//! faces). Cell updates count every cell, slab included. Percentages are
//! against the vacuum run with CPML off.
//!
//! - vacuum: no coefficient arrays;
//! - PEC: a ground face and a patch, so `Cb` arrays (lossless path);
//! - dielectric: a 4-cell `eps_r = 4.4` substrate (lossless path);
//! - dielectric + mu: the substrate plus `mu_r = 2` (adds `Db` arrays);
//! - lossy: the substrate with `sigma > 0` (adds `Ca` arrays).
//!
//! `gate` times one 8000-step vacuum run with CPML on all six faces and
//! exits 1 if that run takes 60 seconds or more.

use rustlab_dsp::fdtd::{
    Axis, CellMap, Component, CpmlParams, FdtdSim, SourceKind, Waveform, DEFAULT_COURANT,
};
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

fn build(case: &str, cpml: bool) -> FdtdSim {
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
    if cpml {
        sim.set_cpml(8, CpmlParams::default()).unwrap();
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

fn best_secs(case: &str, cpml: bool, steps: usize, repeats: usize) -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..repeats {
        let mut sim = build(case, cpml);
        let start = Instant::now();
        let report = sim.run(steps, &mut || false).unwrap();
        let secs = start.elapsed().as_secs_f64();
        assert_eq!(report.completed, steps);
        best = best.min(secs);
    }
    best
}

fn main() {
    let gate = std::env::args().any(|a| a == "gate");
    let cells = (NX * NY * NZ) as f64;
    if gate {
        let steps = 8000;
        let secs = best_secs("vacuum", true, steps, 1);
        let rate = cells * steps as f64 / secs / 1e6;
        println!(
            "gate: vacuum, CPML on six faces, {NX}×{NY}×{NZ}, {steps} steps: {secs:.3} s, \
             {rate:.1} Mcell/s"
        );
        if secs >= 60.0 {
            std::process::exit(1);
        }
        return;
    }

    let steps = 2000;
    let mut vacuum_off = 0.0;
    for case in ["vacuum", "PEC", "dielectric", "dielectric + mu", "lossy"] {
        let off = best_secs(case, false, steps, 5);
        let on = best_secs(case, true, steps, 5);
        let off_rate = cells * steps as f64 / off / 1e6;
        let on_rate = cells * steps as f64 / on / 1e6;
        if case == "vacuum" {
            vacuum_off = off_rate;
        }
        println!(
            "{case:>16}: CPML off: {off_rate:.1} Mcell/s ({:.0}% of vacuum off)  \
             CPML on: {on_rate:.1} Mcell/s ({:.0}% of vacuum off)",
            100.0 * off_rate / vacuum_off,
            100.0 * on_rate / vacuum_off
        );
    }
}

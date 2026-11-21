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
//! exits 1 if that run takes 60 seconds or more. That path is unchanged.
//!
//! After the 60×60×20 table, a 40³ grid with CPML 8 times the stepper
//! with the near-to-far transform off, at one frequency, and at eight.
//! One frequency must stay within 10% of the off time in the same run.
//! The 60×60×20 mesh has no room in z for CPML 8 plus the Huygens margin.

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

/// Reference patch mesh, 2000 steps, four near-to-far frequencies.
/// Prints the serial rate, the shared-pool rate, and the pool rate with
/// the transform off, so the speedup and the transform's share are both
/// on one line.
fn bench_patch() {
    let l = 0.0294216_f64;
    let w = 0.0380100_f64;
    let dx = 1e-3;
    let dz = 0.4e-3;
    let nsub = 4usize;
    let npml = 8usize;
    let margin = 2usize;
    let ngx = 2 * ((l / 2.0 + 16e-3) / dx).round() as usize;
    let ngy = 2 * ((w / 2.0 + 16e-3) / dx).round() as usize;
    let nax = ((10e-3_f64) / dx).round() as usize;
    let naz = ((20e-3_f64) / dz).round() as usize;
    let nx = ngx + 2 * (npml + margin + nax);
    let ny = ngy + 2 * (npml + margin + nax);
    let nz = nsub + 2 * (npml + margin + naz);
    let jg0 = npml + margin + nax;
    let ig0 = jg0;
    let kg = npml + margin + naz;
    let cells = (nx * ny * nz) as f64;
    let steps = 2000usize;
    let build = |threads: u32, ntff: bool| {
        let mut sim = FdtdSim::new(nx, ny, nz, dx, dx, dz, DEFAULT_COURANT).unwrap();
        sim.set_threads(threads).unwrap();
        let mut eps = vec![1.0; nx * ny * nz];
        for k in kg..kg + nsub {
            for i in ig0..ig0 + ngy {
                for j in jg0..jg0 + ngx {
                    eps[(k * ny + i) * nx + j] = 4.4;
                }
            }
        }
        sim.set_media(
            CellMap::Cells(eps),
            CellMap::Uniform(1.0),
            CellMap::Uniform(0.0),
        )
        .unwrap();
        sim.set_cpml(npml, CpmlParams::default()).unwrap();
        let mut ground = vec![false; ny * nx];
        for i in ig0..=ig0 + ngy {
            for j in jg0..=jg0 + ngx {
                ground[i * nx + j] = true;
            }
        }
        sim.add_pec_face(Axis::Z, kg, &ground).unwrap();
        let n_l = (28e-3 / dx).round() as usize;
        let n_w = (38e-3 / dx).round() as usize;
        let j0 = nx / 2 - n_l / 2;
        let i0 = ny / 2 - n_w / 2;
        let mut patch = vec![false; ny * nx];
        for i in i0..=i0 + n_w {
            for j in j0..=j0 + n_l {
                patch[i * nx + j] = true;
            }
        }
        sim.add_pec_face(Axis::Z, kg + nsub, &patch).unwrap();
        let f0 = 2.4e9;
        let tau = 1.0 / (2.0 * f0);
        sim.add_port(
            Component::Ez,
            &[
                (ny / 2, j0, kg),
                (ny / 2, j0, kg + 1),
                (ny / 2, j0, kg + 2),
                (ny / 2, j0, kg + 3),
            ],
            50.0,
            Waveform::Pulse {
                t0: 4.0 * tau,
                tau,
                f0,
            },
        )
        .unwrap();
        if ntff {
            let freqs: Vec<f64> = (0..4).map(|q| 2.40e9 + 0.02e9 * q as f64).collect();
            sim.set_ntff(&freqs, margin).unwrap();
        }
        sim
    };
    let time = |threads: u32, ntff: bool| {
        let mut sim = build(threads, ntff);
        let start = Instant::now();
        let report = sim.run(steps, &mut || false).unwrap();
        let secs = start.elapsed().as_secs_f64();
        assert_eq!(report.completed, steps);
        secs
    };
    let serial = time(1, true);
    let parallel = time(0, true);
    let bare = time(0, false);
    let rate = |secs: f64| cells * steps as f64 / secs / 1e6;
    let long_steps = 12000usize;
    let mut sim = build(0, true);
    let start = Instant::now();
    let report = sim.run(long_steps, &mut || false).unwrap();
    let long = start.elapsed().as_secs_f64();
    assert_eq!(report.completed, long_steps);
    println!(
        "patch parallel {long_steps} steps: {long:.1} s, {:.1} Mcell/s",
        cells * long_steps as f64 / long / 1e6
    );
    println!(
        "patch {nx}×{ny}×{nz} {steps} steps: serial {:.1} Mcell/s ({serial:.1} s), \
         pool {:.1} Mcell/s ({parallel:.1} s), speedup {:.2}×, \
         pool without NTFF {:.1} Mcell/s, NTFF share {:.0}%",
        rate(serial),
        rate(parallel),
        serial / parallel,
        rate(bare),
        100.0 * (1.0 - bare / parallel)
    );
}

fn main() {
    if std::env::args().any(|a| a == "patch") {
        bench_patch();
        return;
    }
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
    bench_ntff();
}

/// 40³ with CPML 8. The 60×60×20 mesh has no room in z for that absorber
/// plus the Huygens margin. One frequency must stay within 10% of the
/// same grid with the transform off.
fn bench_ntff() {
    use rustlab_dsp::fdtd::C0;
    const N: usize = 40;
    const STEPS: usize = 2000;
    let cells = (N * N * N) as f64;
    let rate = |secs: f64| cells * STEPS as f64 / secs / 1e6;
    let time = |nf: usize| {
        let mut best = f64::INFINITY;
        for _ in 0..5 {
            let mut sim = FdtdSim::new(N, N, N, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
            sim.set_cpml(8, CpmlParams::default()).unwrap();
            let tau = 20.0 * sim.dt();
            sim.add_source(
                Component::Ez,
                &[(N / 2, N / 2, N / 2)],
                SourceKind::Soft,
                Waveform::Pulse {
                    t0: 4.0 * tau,
                    tau,
                    f0: 0.0,
                },
            )
            .unwrap();
            if nf > 0 {
                let freqs: Vec<f64> = (0..nf)
                    .map(|q| C0 / (1e-3 * (20.0 + 5.0 * q as f64)))
                    .collect();
                sim.set_ntff(&freqs, 2).unwrap();
            }
            let start = Instant::now();
            let report = sim.run(STEPS, &mut || false).unwrap();
            let secs = start.elapsed().as_secs_f64();
            assert_eq!(report.completed, STEPS);
            best = best.min(secs);
        }
        best
    };
    let off = time(0);
    let one = time(1);
    let eight = time(8);
    let off_r = rate(off);
    let one_r = rate(one);
    let eight_r = rate(eight);
    println!(
        "ntff 40³ CPML 8: off {off_r:.1} Mcell/s, 1 freq {one_r:.1} ({:.1}% of off), \
         8 freq {eight_r:.1} ({:.1}% of off)",
        100.0 * one_r / off_r,
        100.0 * eight_r / off_r
    );
    if one > off * 1.10 {
        eprintln!(
            "ntff gate: 1 frequency took {one:.3} s, NTFF-off took {off:.3} s (more than 10% slower)"
        );
        std::process::exit(1);
    }
}

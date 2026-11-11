//! Parallel stepping matches the serial curl bit for bit.
//!
//! The threshold is thread-local. Each test that forces the parallel driver
//! sets it on the thread that calls `run`, including inside `ThreadPool::install`.

use super::*;
use rayon::ThreadPoolBuilder;
use rustlab_core::C64;

fn never() -> impl FnMut() -> bool {
    || false
}

fn bits(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

fn cbits(a: &[C64], b: &[C64]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.re.to_bits() == y.re.to_bits() && x.im.to_bits() == y.im.to_bits())
}

fn same(a: &FdtdSim, b: &FdtdSim) {
    assert!(bits(&a.ex, &b.ex), "ex");
    assert!(bits(&a.ey, &b.ey), "ey");
    assert!(bits(&a.ez, &b.ez), "ez");
    assert!(bits(&a.hx, &b.hx), "hx");
    assert!(bits(&a.hy, &b.hy), "hy");
    assert!(bits(&a.hz, &b.hz), "hz");
    for (n, (x, y)) in a.absorber.q_h.iter().zip(b.absorber.q_h.iter()).enumerate() {
        assert!(bits(x, y), "q_h {n}");
    }
    for (n, (x, y)) in a.absorber.q_e.iter().zip(b.absorber.q_e.iter()).enumerate() {
        assert!(bits(x, y), "q_e {n}");
    }
    assert!(bits(&a.record, &b.record), "probes");
    assert!(bits(&a.port_v, &b.port_v), "port V");
    assert!(bits(&a.port_i, &b.port_i), "port I");
    assert_eq!(a.steps, b.steps);
    match (a.ntff_snapshot(), b.ntff_snapshot()) {
        (None, None) => {}
        (Some((ae, ah, ad)), Some((be, bh, bd))) => {
            assert!(cbits(&ae, &be), "NTFF E");
            assert!(cbits(&ah, &bh), "NTFF H");
            assert!(bits(&ad, &bd), "NTFF decay");
        }
        _ => panic!("NTFF armed on one grid only"),
    }
}

fn pulse(dt: f64) -> Waveform {
    Waveform::Pulse {
        t0: 20.0 * dt,
        tau: 8.0 * dt,
        f0: 0.0,
    }
}

fn vacuum() -> FdtdSim {
    FdtdSim::new(16, 14, 12, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap()
}

fn lossy() -> FdtdSim {
    let mut s = FdtdSim::new(16, 14, 12, 1e-3, 1.2e-3, 0.8e-3, DEFAULT_COURANT).unwrap();
    let n = s.n_cells();
    let eps = vec![2.0; n];
    let mu = vec![1.5; n];
    let mut sig = vec![0.0; n];
    for v in sig.iter_mut().take(n / 2) {
        *v = 0.02;
    }
    s.set_media(CellMap::Cells(eps), CellMap::Cells(mu), CellMap::Cells(sig))
        .unwrap();
    s
}

fn pec() -> FdtdSim {
    let mut s = vacuum();
    let mut vol = vec![false; s.n_cells()];
    vol[s.idx(4, 4, 4)] = true;
    s.add_pec_volume(&vol).unwrap();
    let mut face = vec![false; s.ny() * s.nx()];
    face[3 * s.nx() + 3] = true;
    face[3 * s.nx() + 4] = true;
    face[4 * s.nx() + 3] = true;
    face[4 * s.nx() + 4] = true;
    s.add_pec_face(Axis::Z, 6, &face).unwrap();
    s
}

fn cpml() -> FdtdSim {
    let mut s = FdtdSim::new(20, 18, 16, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    s.set_cpml(4, CpmlParams::default()).unwrap();
    s
}

fn sourced() -> FdtdSim {
    let mut s = cpml();
    let dt = s.dt();
    s.add_source(Component::Ez, &[(8, 8, 8)], SourceKind::Soft, pulse(dt))
        .unwrap();
    s.add_source(Component::Ex, &[(9, 8, 8)], SourceKind::Hard, pulse(dt))
        .unwrap();
    s.add_probe(Component::Ez, 8, 10, 8).unwrap();
    s.add_probe(Component::Hx, 8, 8, 8).unwrap();
    s
}

fn ported() -> FdtdSim {
    let mut s = cpml();
    let dt = s.dt();
    s.add_port(Component::Ez, &[(8, 8, 6), (8, 8, 7)], 50.0, pulse(dt))
        .unwrap();
    s
}

fn radiated() -> FdtdSim {
    let mut s = ported();
    let f = C0 / 0.04;
    s.set_ntff(&[f, 1.1 * f, 0.9 * f], 1).unwrap();
    s
}

fn run_pair(mut serial: FdtdSim, mut parallel: FdtdSim, steps: usize) {
    serial.set_threads(1).unwrap();
    serial.run(steps, &mut never()).unwrap();
    yee::set_par_cells_threshold(0);
    // Two rows per task. Chunk 0 is only the first two rows, so a bad
    // offset on any later chunk changes a field the compare reads.
    yee::set_rows_per_task(2);
    yee::reset_par_hits();
    yee::reset_par_chunks();
    parallel.run(steps, &mut never()).unwrap();
    assert!(
        yee::par_hits() > 0,
        "parallel driver did not run (hits {})",
        yee::par_hits()
    );
    let rows = parallel.ny() * parallel.nz();
    let per_update = rows.div_ceil(2);
    let min_chunks = (per_update * 2 * steps) as u64;
    assert!(
        yee::par_chunks() >= min_chunks,
        "{} rows, {} chunks after {steps} steps (need at least {min_chunks})",
        rows,
        yee::par_chunks()
    );
    same(&serial, &parallel);
    if parallel.ntff_freqs().is_empty() {
        return;
    }
    let th = [0.3, 1.2, 2.4];
    let ph = [0.0, 1.0, 2.0];
    let fs = serial.far_field(&th, &ph, None, RecordDecay::Off).unwrap();
    let fp = parallel
        .far_field(&th, &ph, None, RecordDecay::Off)
        .unwrap();
    assert!(cbits(&fs.e_theta, &fp.e_theta), "Etheta");
    assert!(cbits(&fs.e_phi, &fp.e_phi), "Ephi");
    assert!(bits(&fs.prad, &fp.prad), "prad");
}

fn with_pool(n: usize, f: impl FnOnce() + Send) {
    let pool = ThreadPoolBuilder::new().num_threads(n).build().unwrap();
    pool.install(f);
}

#[test]
fn parallel_step_matches_serial_bitwise() {
    let fixtures: Vec<(&str, fn() -> FdtdSim)> = vec![
        ("vacuum", vacuum),
        ("lossy", lossy),
        ("pec", pec),
        ("cpml", cpml),
        ("sources", sourced),
        ("port", ported),
        ("ntff", radiated),
    ];
    for (name, build) in fixtures {
        for n in 1..=4 {
            with_pool(n, || {
                let a = build();
                let b = a.clone();
                run_pair(a, b, 30);
            });
            eprintln!("{name} threads {n} ok");
        }
    }
    // A floating patch is the capstone geometry, on a coarse grid.
    for n in 1..=4 {
        with_pool(n, || {
            let spec = patch_tests::PatchSpec::tiny();
            let a = patch_tests::build(&spec).sim;
            let b = a.clone();
            run_pair(a, b, 20);
        });
    }
}

#[test]
fn parallel_split_run_identity() {
    with_pool(3, || {
        yee::set_par_cells_threshold(0);
        yee::set_rows_per_task(2);
        let mut one = sourced();
        one.run(150, &mut never()).unwrap();
        let mut split = sourced();
        split.run(70, &mut never()).unwrap();
        split.run(80, &mut never()).unwrap();
        same(&one, &split);
    });
}

#[test]
fn small_grid_stays_serial() {
    yee::set_par_cells_threshold(32_768);
    yee::reset_par_hits();
    let mut s = FdtdSim::new(8, 8, 8, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    s.run(5, &mut never()).unwrap();
    assert_eq!(yee::par_hits(), 0, "a 512-cell grid entered rayon");
}

#[test]
fn threads_one_forces_serial() {
    yee::set_par_cells_threshold(0);
    yee::reset_par_hits();
    let mut s = vacuum();
    s.set_threads(1).unwrap();
    s.run(5, &mut never()).unwrap();
    assert_eq!(yee::par_hits(), 0, "threads = 1 entered rayon");
    assert_eq!(s.threads(), 1);
    let err = FdtdSim::new(4, 4, 4, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT)
        .unwrap()
        .set_threads(2)
        .unwrap_err()
        .to_string();
    assert!(err.contains("0") && err.contains("1"), "{err}");
}

/// The H and E CPML corrections have to take the same row split as the curl.
/// Two steps are four corrections (H and E, twice).
#[test]
fn cpml_correction_runs_on_the_pool() {
    with_pool(4, || {
        yee::set_par_cells_threshold(0);
        yee::set_rows_per_task(2);
        yee::reset_cpml_hits();
        yee::reset_par_chunks();
        let mut s = cpml();
        s.run(2, &mut never()).unwrap();
        assert!(
            yee::cpml_hits() >= 4,
            "CPML correction stayed serial (hits {})",
            yee::cpml_hits()
        );
        let per = (s.ny() * s.nz()).div_ceil(2) as u64;
        // Two steps, four parallel regions each step (H curl, H correction,
        // E curl, E correction), two rows per task.
        assert!(
            yee::par_chunks() >= per * 8,
            "CPML row split was not exercised ({} chunks, need {})",
            yee::par_chunks(),
            per * 8
        );
    });
}

/// 60×60×20 is the Phase 3 gate (72,000 cells). The pool made this mesh
/// about twice as slow as the 0.3.63 serial step, so the default threshold
/// has to leave it on the serial loop.
#[test]
fn parent_gate_mesh_stays_serial() {
    yee::set_par_cells_threshold(yee::default_par_cells());
    yee::set_rows_per_task(0);
    yee::reset_par_hits();
    let mut s = FdtdSim::new(60, 60, 20, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    s.set_cpml(8, CpmlParams::default()).unwrap();
    s.run(1, &mut never()).unwrap();
    assert_eq!(
        yee::par_hits(),
        0,
        "60×60×20 entered the pool at the default threshold"
    );
}

/// The lesson mesh is 285,120 cells. It has to stay on the pool, so the
/// threshold that keeps 72,000 serial cannot sit above this grid either.
#[test]
fn lesson_sized_mesh_uses_the_pool() {
    yee::set_par_cells_threshold(yee::default_par_cells());
    yee::set_rows_per_task(0);
    yee::reset_par_hits();
    let mut s = FdtdSim::new(64, 64, 64, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    s.run(1, &mut never()).unwrap();
    assert!(
        yee::par_hits() > 0,
        "64³ (262144 cells) stayed serial at the default threshold"
    );
}

#[test]
fn parallel_run_stops_between_steps() {
    with_pool(2, || {
        yee::set_par_cells_threshold(0);
        let mut s = sourced();
        let mut seen = 0;
        s.run(100, &mut || {
            let stop = seen >= 5;
            seen += 1;
            stop
        })
        .unwrap();
        assert_eq!(s.steps(), 5);
        let mut serial = sourced();
        serial.set_threads(1).unwrap();
        serial.run(5, &mut never()).unwrap();
        same(&s, &serial);
    });
}

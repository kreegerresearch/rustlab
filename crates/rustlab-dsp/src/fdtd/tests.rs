use super::*;
use std::f64::consts::PI;

fn never() -> impl FnMut() -> bool {
    || false
}

/// Peak frequency of the Hann-windowed DTFT of `x` in `[f_lo, f_hi]`: a grid
/// at 1/16 of a bin, then golden-section refinement.
fn peak_freq(x: &[f64], dt: f64, f_lo: f64, f_hi: f64) -> f64 {
    let n = x.len();
    let mean = x.iter().sum::<f64>() / n as f64;
    let w: Vec<f64> = (0..n)
        .map(|m| {
            let hann = 0.5 - 0.5 * (2.0 * PI * m as f64 / (n - 1) as f64).cos();
            hann * (x[m] - mean)
        })
        .collect();
    let mag = |f: f64| {
        let (mut re, mut im) = (0.0, 0.0);
        for (m, v) in w.iter().enumerate() {
            let ph = 2.0 * PI * f * m as f64 * dt;
            re += v * ph.cos();
            im -= v * ph.sin();
        }
        re * re + im * im
    };
    let bin = 1.0 / (n as f64 * dt);
    let step = bin / 16.0;
    let mut best = (f_lo, mag(f_lo));
    let mut f = f_lo;
    while f <= f_hi {
        let m = mag(f);
        if m > best.1 {
            best = (f, m);
        }
        f += step;
    }
    let (mut a, mut b) = (best.0 - step, best.0 + step);
    let g = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..60 {
        let c = b - g * (b - a);
        let d = a + g * (b - a);
        if mag(c) > mag(d) {
            b = d;
        } else {
            a = c;
        }
    }
    0.5 * (a + b)
}

/// Yee dispersion: `ω` with `(2/(c dt))² sin²(ω dt/2) = Σ (2/Δ)² sin²(kΔ/2)`,
/// `c = c0 / index`.
fn yee_freq(sim: &FdtdSim, index: f64, kx: f64, ky: f64, kz: f64) -> f64 {
    let (dx, dy, dz) = sim.spacing();
    let dt = sim.dt();
    let s = |k: f64, d: f64| (2.0 / d * (k * d / 2.0).sin()).powi(2);
    let rhs = s(kx, dx) + s(ky, dy) + s(kz, dz);
    let arg = C0 / index * dt / 2.0 * rhs.sqrt();
    2.0 * arg.asin() / dt / (2.0 * PI)
}

struct ModeCheck {
    measured: f64,
    yee: f64,
    analytic: f64,
    bin: f64,
}

/// Ring a mode with a modulated soft source at the box centre (for that
/// component's stagger) and measure the probe peak.
fn ring_mode(comp: Component, m: [usize; 3]) -> ModeCheck {
    ring_mode_in(comp, m, 1.0, |_| {})
}

/// [`ring_mode`] in a medium of refractive `index`, set up by `media`.
fn ring_mode_in(
    comp: Component,
    m: [usize; 3],
    index: f64,
    media: impl FnOnce(&mut FdtdSim),
) -> ModeCheck {
    let (nx, ny, nz) = (8, 6, 4);
    let (dx, dy, dz) = (1.0e-3, 1.2e-3, 0.9e-3);
    let mut sim = FdtdSim::new(nx, ny, nz, dx, dy, dz, DEFAULT_COURANT).unwrap();
    media(&mut sim);
    let (lx, ly, lz) = (nx as f64 * dx, ny as f64 * dy, nz as f64 * dz);
    let (kx, ky, kz) = (
        m[0] as f64 * PI / lx,
        m[1] as f64 * PI / ly,
        m[2] as f64 * PI / lz,
    );
    let analytic = C0 / index / (2.0 * PI) * (kx * kx + ky * ky + kz * kz).sqrt();
    let tau = 2.0 / analytic;
    let edge = (ny / 2, nx / 2, nz / 2);
    sim.add_source(
        comp,
        &[edge],
        SourceKind::Soft,
        Waveform::Pulse {
            t0: 4.0 * tau,
            tau,
            f0: analytic,
        },
    )
    .unwrap();
    sim.add_probe(comp, edge.0, edge.1, edge.2).unwrap();
    let nsteps = 20_000;
    sim.run(nsteps, &mut never()).unwrap();
    let skip = (8.0 * tau / sim.dt()).ceil() as usize + 50;
    let x = &sim.record()[skip..];
    let bin = 1.0 / (x.len() as f64 * sim.dt());
    let measured = peak_freq(x, sim.dt(), 0.95 * analytic, 1.05 * analytic);
    ModeCheck {
        measured,
        yee: yee_freq(&sim, index, kx, ky, kz),
        analytic,
        bin,
    }
}

fn assert_mode(name: &str, r: ModeCheck) {
    let err_bins = (r.measured - r.yee).abs() / r.bin;
    eprintln!(
        "{name}: measured {:.6e} Hz, Yee {:.6e} Hz, analytic {:.6e} Hz, bin {:.3e} Hz, |meas-Yee| = {err_bins:.4} bins",
        r.measured, r.yee, r.analytic, r.bin
    );
    assert!(
        err_bins < 0.01,
        "{name}: measured {} Hz vs Yee {} Hz ({err_bins} bins of {} Hz)",
        r.measured,
        r.yee,
        r.bin
    );
    let rel = (r.measured - r.analytic).abs() / r.analytic;
    assert!(
        rel < 0.02,
        "{name}: {rel} relative to analytic {}",
        r.analytic
    );
    // On this coarse grid the dispersion shift is several bins, so matching
    // Yee to a tenth of a bin is not the continuum frequency in disguise.
    assert!(
        (r.yee - r.analytic).abs() > 3.0 * r.bin,
        "{name}: Yee shift {} Hz is not resolved (bin {} Hz)",
        r.yee - r.analytic,
        r.bin
    );
}

#[test]
fn cavity_tm110_matches_yee_dispersion() {
    assert_mode("ez (1,1,0)", ring_mode(Component::Ez, [1, 1, 0]));
}

#[test]
fn cavity_ex_mode_011_matches_yee_dispersion() {
    assert_mode("ex (0,1,1)", ring_mode(Component::Ex, [0, 1, 1]));
}

#[test]
fn cavity_ey_mode_101_matches_yee_dispersion() {
    assert_mode("ey (1,0,1)", ring_mode(Component::Ey, [1, 0, 1]));
}

#[test]
fn courant_above_one_is_rejected() {
    let e = FdtdSim::new(4, 4, 4, 1e-3, 1e-3, 1e-3, 1.5).unwrap_err();
    assert!(e.to_string().contains("courant must be in (0, 1]"), "{e}");
    assert!(FdtdSim::new(4, 4, 4, 1e-3, 1e-3, 1e-3, 0.0).is_err());
    assert!(FdtdSim::new(4, 4, 4, 1e-3, 1e-3, 1e-3, f64::NAN).is_err());
    let s = FdtdSim::new(4, 4, 4, 1e-3, 1e-3, 1e-3, 1.0).unwrap();
    assert_eq!(s.courant(), 1.0);
    let lim = 1.0 / (C0 * (3.0f64).sqrt() / 1e-3);
    assert!((s.dt() - lim).abs() <= 1e-15 * lim);
}

#[test]
fn bad_grid_is_rejected() {
    assert!(FdtdSim::new(1, 4, 4, 1e-3, 1e-3, 1e-3, 0.99).is_err());
    assert!(FdtdSim::new(4, 4, 4, -1e-3, 1e-3, 1e-3, 0.99).is_err());
    assert!(FdtdSim::new(4, 4, 4, 1e-3, f64::INFINITY, 1e-3, 0.99).is_err());
    assert!(FdtdSim::new(usize::MAX / 2, 4, 4, 1e-3, 1e-3, 1e-3, 0.99).is_err());
}

fn small_sim() -> FdtdSim {
    let mut sim = FdtdSim::new(10, 9, 8, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    let tau = 15.0 * sim.dt();
    sim.add_source(
        Component::Ez,
        &[(4, 5, 3), (4, 5, 4)],
        SourceKind::Soft,
        Waveform::Pulse {
            t0: 4.0 * tau,
            tau,
            f0: 0.0,
        },
    )
    .unwrap();
    sim.add_source(
        Component::Ex,
        &[(2, 2, 2)],
        SourceKind::Hard,
        Waveform::Samples(vec![1.0, 2.0, 3.0]),
    )
    .unwrap();
    sim.add_probe(Component::Ez, 4, 7, 3).unwrap();
    sim.add_probe(Component::Hy, 4, 6, 3).unwrap();
    sim
}

#[test]
fn repeat_run_is_bit_identical() {
    let mut a = small_sim();
    let mut b = small_sim();
    a.run(120, &mut never()).unwrap();
    b.run(70, &mut never()).unwrap();
    b.run(50, &mut never()).unwrap();
    assert_eq!(a.steps(), 120);
    assert_eq!(a.field_bits(), b.field_bits());
    let ra: Vec<u64> = a.record().iter().map(|x| x.to_bits()).collect();
    let rb: Vec<u64> = b.record().iter().map(|x| x.to_bits()).collect();
    assert_eq!(ra, rb);
    assert_eq!(a.record().len(), 120 * 2);
    assert!(a.record().iter().any(|&v| v != 0.0));
}

#[test]
fn stop_callback_ends_run_at_a_step_boundary() {
    let mut sim = small_sim();
    let mut calls = 0;
    let mut stop = || {
        calls += 1;
        calls > 2
    };
    let r = sim.run(50, &mut stop).unwrap();
    assert_eq!(
        r,
        RunReport {
            completed: 2,
            stopped: true
        }
    );
    assert!(sim.interrupted());
    assert_eq!(sim.steps(), 2);
    assert_eq!(sim.record().len(), 2 * sim.n_probes());

    let mut full = small_sim();
    full.run(2, &mut never()).unwrap();
    assert_eq!(sim.field_bits(), full.field_bits());

    let r = sim.run(3, &mut never()).unwrap();
    assert_eq!(
        r,
        RunReport {
            completed: 3,
            stopped: false
        }
    );
    assert!(!sim.interrupted());
    assert_eq!(sim.steps(), 5);
}

#[test]
fn stop_before_first_step_runs_nothing() {
    let mut sim = small_sim();
    let r = sim.run(10, &mut || true).unwrap();
    assert_eq!(
        r,
        RunReport {
            completed: 0,
            stopped: true
        }
    );
    assert_eq!(sim.steps(), 0);
    assert!(sim.record().is_empty());
}

#[test]
fn vacuum_run_stays_bounded() {
    let mut sim = FdtdSim::new(12, 12, 12, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    let tau = 10.0 * sim.dt();
    sim.add_source(
        Component::Ez,
        &[(6, 6, 6)],
        SourceKind::Soft,
        Waveform::Pulse {
            t0: 40.0 * sim.dt(),
            tau,
            f0: 30e9,
        },
    )
    .unwrap();
    sim.run(200, &mut never()).unwrap();
    let e0 = sim.energy();
    assert!(e0 > 0.0);
    sim.run(1800, &mut never()).unwrap();
    let e1 = sim.energy();
    assert!(
        e1.is_finite() && e1 < 2.0 * e0 && e1 > 0.5 * e0,
        "{e0} -> {e1}"
    );
}

#[test]
fn hard_source_overwrites_then_releases() {
    let mut sim = FdtdSim::new(6, 6, 6, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    sim.add_source(
        Component::Ez,
        &[(3, 3, 3)],
        SourceKind::Hard,
        Waveform::Samples(vec![2.0e-3, 4.0e-3]),
    )
    .unwrap();
    sim.add_probe(Component::Ez, 3, 3, 3).unwrap();
    sim.run(3, &mut never()).unwrap();
    let r = sim.record();
    assert_eq!(r[0], -2.0);
    assert_eq!(r[1], -4.0);
    assert_ne!(r[2], 0.0);
    assert_ne!(r[2], -4.0);
}

#[test]
fn soft_source_on_a_stack_splits_the_voltage() {
    let mk = |edges: &[(usize, usize, usize)]| {
        let mut sim = FdtdSim::new(6, 6, 6, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
        sim.add_source(
            Component::Ez,
            edges,
            SourceKind::Soft,
            Waveform::Samples(vec![1.0]),
        )
        .unwrap();
        sim.run(1, &mut never()).unwrap();
        sim
    };
    let one = mk(&[(3, 3, 2)]);
    let two = mk(&[(3, 3, 2), (3, 3, 3)]);
    let v1 = -one.value_at(Component::Ez, 3, 3, 2) * 1e-3;
    let v2 = -(two.value_at(Component::Ez, 3, 3, 2) + two.value_at(Component::Ez, 3, 3, 3)) * 1e-3;
    assert!(v1 > 0.0);
    assert!((v1 - v2).abs() <= 1e-15 * v1);
}

#[test]
fn sources_and_probes_are_validated() {
    let mut sim = FdtdSim::new(5, 5, 5, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    let w = || Waveform::Samples(vec![1.0]);
    assert!(sim
        .add_source(Component::Hz, &[(2, 2, 2)], SourceKind::Soft, w())
        .is_err());
    assert!(sim
        .add_source(Component::Ez, &[(0, 2, 2)], SourceKind::Soft, w())
        .is_err());
    assert!(sim
        .add_source(Component::Ez, &[(2, 0, 2)], SourceKind::Soft, w())
        .is_err());
    assert!(sim
        .add_source(Component::Ez, &[(2, 2, 0)], SourceKind::Soft, w())
        .is_ok());
    assert!(sim
        .add_source(Component::Ex, &[(2, 0, 2)], SourceKind::Soft, w())
        .is_ok());
    assert!(sim
        .add_source(Component::Ex, &[(2, 2, 5)], SourceKind::Soft, w())
        .is_err());
    assert!(sim
        .add_source(Component::Ez, &[(2, 2, 0)], SourceKind::Hard, w())
        .is_err());
    assert!(sim.add_probe(Component::Hx, 4, 4, 4).is_ok());
    assert!(sim.add_probe(Component::Hx, 5, 4, 4).is_err());
    sim.run(1, &mut never()).unwrap();
    assert!(sim.add_probe(Component::Ez, 2, 2, 2).is_err());
    assert!(sim
        .add_source(Component::Ez, &[(2, 2, 2)], SourceKind::Soft, w())
        .is_err());
}

#[test]
fn probe_components_follow_the_half_step() {
    // An E probe at the source edge sees the source on the same step; an H
    // probe next to it sees the first change one step later (H leads E by a
    // half step and is updated before the source is applied).
    let mut sim = FdtdSim::new(6, 6, 6, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    sim.add_source(
        Component::Ez,
        &[(3, 3, 3)],
        SourceKind::Soft,
        Waveform::Samples(vec![1.0]),
    )
    .unwrap();
    sim.add_probe(Component::Ez, 3, 3, 3).unwrap();
    sim.add_probe(Component::Hy, 3, 3, 3).unwrap();
    sim.run(2, &mut never()).unwrap();
    let r = sim.record();
    assert_ne!(r[0], 0.0);
    assert_eq!(r[1], 0.0);
    assert_ne!(r[3], 0.0);
}

#[test]
fn slices_have_the_tensor3_shapes() {
    let mut sim = FdtdSim::new(5, 4, 3, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    sim.add_source(
        Component::Ez,
        &[(2, 3, 1)],
        SourceKind::Hard,
        Waveform::Samples(vec![-1e-3]),
    )
    .unwrap();
    sim.run(1, &mut never()).unwrap();
    let (r, c, z) = sim.slice(Component::Ez, Axis::Z, 1).unwrap();
    assert_eq!((r, c), (4, 5));
    assert_eq!(z[2 * 5 + 3], 1.0);
    let (r, c, y) = sim.slice(Component::Ez, Axis::Y, 2).unwrap();
    assert_eq!((r, c), (5, 3));
    assert_eq!(y[3 * 3 + 1], 1.0);
    let (r, c, x) = sim.slice(Component::Ez, Axis::X, 3).unwrap();
    assert_eq!((r, c), (4, 3));
    assert_eq!(x[2 * 3 + 1], 1.0);
    assert!(sim.slice(Component::Ez, Axis::Z, 3).is_err());
}

#[test]
fn gaussian_pulse_values() {
    assert_eq!(gaussian_pulse(1.0, 1.0, 0.5, 0.0), 1.0);
    let v = gaussian_pulse(1.5, 1.0, 0.5, 0.0);
    assert!((v - (-1.0f64).exp()).abs() < 1e-15);
    let m = gaussian_pulse(1.25, 1.0, 0.5, 1.0);
    assert!(m.abs() < 1e-15);
    assert_eq!(gaussian_pulse(1.0, 1.0, 0.5, 3.0), 1.0);
}

#[test]
fn vacuum_constants() {
    assert!((EPS0 - 8.8541878128e-12).abs() < 1e-21);
    assert!((1.0 / (MU0 * EPS0).sqrt() - C0).abs() < 1e-6);
}

// ---------------------------------------------------------------- Phase 2

fn fnv(bits: impl Iterator<Item = u64>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bits {
        for byte in b.to_le_bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// Arithmetic-only waveforms (no libm), so the checksum is the same on every
/// IEEE platform.
fn golden_sim() -> FdtdSim {
    let mut sim = FdtdSim::new(10, 9, 8, 1e-3, 1.1e-3, 0.9e-3, DEFAULT_COURANT).unwrap();
    let soft: Vec<f64> = (0..60)
        .map(|s| {
            let x = s as f64 / 30.0 - 1.0;
            (1.0 - x * x) * x * 1e-3
        })
        .collect();
    sim.add_source(
        Component::Ez,
        &[(4, 5, 3), (4, 5, 4)],
        SourceKind::Soft,
        Waveform::Samples(soft),
    )
    .unwrap();
    sim.add_source(
        Component::Ex,
        &[(2, 2, 2)],
        SourceKind::Hard,
        Waveform::Samples(vec![1.0, 2.0, 3.0]),
    )
    .unwrap();
    sim.add_probe(Component::Ez, 4, 7, 3).unwrap();
    sim.add_probe(Component::Hy, 4, 6, 3).unwrap();
    sim
}

fn golden_hash(mut sim: FdtdSim) -> u64 {
    sim.run(300, &mut never()).unwrap();
    fnv(sim
        .field_bits()
        .into_iter()
        .chain(sim.record().iter().map(|x| x.to_bits())))
}

/// FNV-1a of the fields and record of `golden_sim` after 300 steps, computed
/// with the Phase 1 (0.3.47) kernel before media existed.
const PHASE1_GOLDEN: u64 = 0x0d04_6f37_beda_4513;

#[test]
fn vacuum_matches_phase1_bit_for_bit() {
    assert_eq!(golden_hash(golden_sim()), PHASE1_GOLDEN);
}

#[test]
fn unit_media_and_wall_pec_match_vacuum_bit_for_bit() {
    let mut a = golden_sim();
    a.set_media(
        CellMap::Uniform(1.0),
        CellMap::Cells(vec![1.0; a.n_cells()]),
        CellMap::Uniform(0.0),
    )
    .unwrap();
    assert!(a.baked.cb.is_none() && a.baked.db.is_none());
    assert_eq!(golden_hash(a), PHASE1_GOLDEN);

    // PEC on the outer walls only: the lossless kernel runs with Cb = 1 on
    // every updated edge.
    let mut b = golden_sim();
    b.add_pec_face(Axis::Z, 0, &[true; 9 * 10]).unwrap();
    b.add_pec_face(Axis::X, 10, &[true; 9 * 8]).unwrap();
    assert!(b.baked.cb.is_some());
    assert_eq!(golden_hash(b), PHASE1_GOLDEN);
}

#[test]
fn dielectric_cavity_shifts_by_inverse_sqrt_eps() {
    let vac = ring_mode(Component::Ez, [1, 1, 0]);
    let diel = ring_mode_in(Component::Ez, [1, 1, 0], 2.0, |s| {
        s.set_media(
            CellMap::Uniform(4.0),
            CellMap::Uniform(1.0),
            CellMap::Uniform(0.0),
        )
        .unwrap()
    });
    let ratio = diel.measured / vac.measured;
    eprintln!("eps_r = 4: f / f_vacuum = {ratio}");
    assert!((ratio - 0.5).abs() < 0.01 * 0.5, "ratio {ratio}");
    assert_mode("ez (1,1,0), eps_r = 4", diel);
}

#[test]
fn magnetic_cavity_shifts_by_inverse_sqrt_eps_mu() {
    let m = ring_mode_in(Component::Ey, [1, 0, 1], 2.0, |s| {
        let n = s.n_cells();
        s.set_media(
            CellMap::Uniform(2.0),
            CellMap::Cells(vec![2.0; n]),
            CellMap::Uniform(0.0),
        )
        .unwrap()
    });
    assert_mode("ey (1,0,1), eps_r = mu_r = 2", m);
}

#[test]
fn edges_take_the_arithmetic_mean_of_their_cells() {
    let (nx, ny, nz) = (4, 4, 4);
    let mut sim = FdtdSim::new(nx, ny, nz, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    let layer = |inside: f64| {
        let mut v = vec![1.0; nx * ny * nz];
        for (c, x) in v.iter_mut().enumerate() {
            if c / (nx * ny) < 2 {
                *x = inside;
            }
        }
        CellMap::Cells(v)
    };
    sim.set_media(layer(4.0), layer(2.0), CellMap::Uniform(0.0))
        .unwrap();
    let idx = |i, j, k| sim.idx(i, j, k);
    let cb = sim.baked.cb.as_ref().unwrap();
    let db = sim.baked.db.as_ref().unwrap();
    // Ex at the interface page shares two eps 4 cells and two eps 1 cells:
    // arithmetic 2.5 (harmonic would be 1.6).
    assert_eq!(cb[0][idx(2, 1, 2)], 1.0 / 2.5);
    assert_eq!(cb[0][idx(2, 1, 1)], 1.0 / 4.0);
    assert_eq!(cb[1][idx(1, 2, 2)], 1.0 / 2.5);
    // Ez lies inside one page.
    assert_eq!(cb[2][idx(2, 2, 1)], 1.0 / 4.0);
    assert_eq!(cb[2][idx(2, 2, 2)], 1.0);
    // Hz sits on a z face between pages 1 and 2; Hx, Hy inside a page.
    assert_eq!(db[2][idx(1, 1, 2)], 1.0 / 1.5);
    assert_eq!(db[0][idx(1, 1, 1)], 0.5);
    assert_eq!(db[1][idx(1, 1, 3)], 1.0);
    // A min-wall edge has one cell.
    assert_eq!(cb[2][idx(0, 0, 0)], 1.0 / 4.0);
}

#[test]
fn media_sets_dt_from_the_fastest_speed() {
    let mk = || FdtdSim::new(6, 5, 4, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    let vac = mk().dt();
    let mut s = mk();
    s.set_media(
        CellMap::Uniform(4.0),
        CellMap::Uniform(1.0),
        CellMap::Uniform(0.0),
    )
    .unwrap();
    assert_eq!(s.dt(), vac);
    let mut s = mk();
    let mut eps = vec![1.0; s.n_cells()];
    eps[7] = 0.25;
    s.set_media(
        CellMap::Cells(eps),
        CellMap::Uniform(1.0),
        CellMap::Uniform(0.0),
    )
    .unwrap();
    assert!((s.dt() - vac / 2.0).abs() <= 1e-15 * vac);
    let mut s = mk();
    s.set_media(
        CellMap::Uniform(0.5),
        CellMap::Uniform(0.5),
        CellMap::Uniform(0.0),
    )
    .unwrap();
    assert!((s.dt() - vac / 2.0).abs() <= 1e-15 * vac);
}

#[test]
fn soft_source_scales_with_the_edge_permittivity() {
    let first = |eps: f64| {
        let mut sim = FdtdSim::new(6, 6, 6, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
        sim.set_media(
            CellMap::Uniform(eps),
            CellMap::Uniform(1.0),
            CellMap::Uniform(0.0),
        )
        .unwrap();
        sim.add_source(
            Component::Ez,
            &[(3, 3, 3)],
            SourceKind::Soft,
            Waveform::Samples(vec![1.0]),
        )
        .unwrap();
        sim.run(1, &mut never()).unwrap();
        sim.value_at(Component::Ez, 3, 3, 3)
    };
    let (v1, v4) = (first(1.0), first(4.0));
    assert!(v1 < 0.0);
    assert!((v4 - v1 / 4.0).abs() <= 1e-15 * v1.abs());
}

#[test]
fn lossy_medium_decays() {
    let run = |sigma: f64| {
        let mut sim = FdtdSim::new(10, 10, 10, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
        sim.set_media(
            CellMap::Uniform(2.0),
            CellMap::Uniform(1.0),
            CellMap::Uniform(sigma),
        )
        .unwrap();
        sim.add_source(
            Component::Ez,
            &[(5, 5, 5)],
            SourceKind::Soft,
            Waveform::Pulse {
                t0: 40.0 * sim.dt(),
                tau: 10.0 * sim.dt(),
                f0: 20e9,
            },
        )
        .unwrap();
        sim.run(200, &mut never()).unwrap();
        let e0 = sim.energy();
        sim.run(1800, &mut never()).unwrap();
        (e0, sim.energy())
    };
    let (l0, l1) = run(0.0);
    assert!(l1 > 0.5 * l0 && l1 < 2.0 * l0, "{l0} -> {l1}");
    // eps/sigma = 2 eps0 / 0.5 ≈ 35 ps ≈ 18 steps of 1.9 ps.
    let (e0, e1) = run(0.5);
    assert!(e1.is_finite() && e1 < 1e-6 * e0, "{e0} -> {e1}");
    assert!(e0 < l0);
}

fn pec_probe_sim() -> FdtdSim {
    let mut sim = FdtdSim::new(8, 8, 8, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    sim.add_source(
        Component::Ez,
        &[(4, 4, 1)],
        SourceKind::Soft,
        Waveform::Pulse {
            t0: 30.0 * sim.dt(),
            tau: 8.0 * sim.dt(),
            f0: 0.0,
        },
    )
    .unwrap();
    sim
}

#[test]
fn pec_face_zeros_the_tangential_e_on_that_face() {
    let mut sim = pec_probe_sim();
    // Metal plane at z = 4 mm over the nodes i, j in 2..=5.
    let mut nodes = vec![false; 8 * 8];
    for i in 2..=5 {
        for j in 2..=5 {
            nodes[i * 8 + j] = true;
        }
    }
    sim.add_pec_face(Axis::Z, 4, &nodes).unwrap();
    let on_ex = sim.add_probe(Component::Ex, 3, 3, 4).unwrap();
    let on_ey = sim.add_probe(Component::Ey, 3, 3, 4).unwrap();
    let below = sim.add_probe(Component::Ex, 3, 3, 3).unwrap();
    let rim = sim.add_probe(Component::Ex, 3, 5, 4).unwrap(); // node j = 6 is open
    let normal = sim.add_probe(Component::Ez, 3, 3, 4).unwrap();
    sim.run(300, &mut never()).unwrap();
    let np = sim.n_probes();
    let col = |p: usize| {
        sim.record()
            .iter()
            .skip(p)
            .step_by(np)
            .copied()
            .collect::<Vec<_>>()
    };
    assert!(col(on_ex).iter().all(|&v| v == 0.0));
    assert!(col(on_ey).iter().all(|&v| v == 0.0));
    for p in [below, rim, normal] {
        assert!(col(p).iter().any(|&v| v != 0.0), "probe {p} stayed 0");
    }
    assert!(sim.is_pec_edge(Component::Ex, 3, 4, 4));
    assert!(!sim.is_pec_edge(Component::Ex, 3, 5, 4));
    assert!(!sim.is_pec_edge(Component::Ez, 3, 3, 4));
}

#[test]
fn pec_volume_holds_every_edge_of_the_cell() {
    let mut sim = pec_probe_sim();
    let mut cells = vec![false; sim.n_cells()];
    let (i, j, k) = (3, 3, 4);
    cells[sim.idx(i, j, k)] = true;
    sim.add_pec_volume(&cells).unwrap();
    let mut ids = Vec::new();
    for (a, b) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        ids.push(sim.add_probe(Component::Ex, i + a, j, k + b).unwrap());
        ids.push(sim.add_probe(Component::Ey, i, j + a, k + b).unwrap());
        ids.push(sim.add_probe(Component::Ez, i + a, j + b, k).unwrap());
    }
    let outside = sim.add_probe(Component::Ez, i + 2, j, k).unwrap();
    sim.run(300, &mut never()).unwrap();
    let np = sim.n_probes();
    for (row, chunk) in sim.record().chunks(np).enumerate() {
        for &p in &ids {
            assert_eq!(chunk[p], 0.0, "step {row} probe {p}");
        }
    }
    assert!(sim.record().chunks(np).any(|r| r[outside] != 0.0));
}

#[test]
fn interior_pec_box_rings_at_the_yee_frequency() {
    // Six PEC faces cut an 8 × 6 × 4 cavity out of a larger grid. It must ring
    // like the 8 × 6 × 4 grid with PEC outer walls.
    let (nx, ny, nz) = (12, 10, 8);
    let (dx, dy, dz) = (1.0e-3, 1.2e-3, 0.9e-3);
    let (j0, i0, k0) = (2, 2, 2);
    let (j1, i1, k1) = (j0 + 8, i0 + 6, k0 + 4);
    let mut sim = FdtdSim::new(nx, ny, nz, dx, dy, dz, DEFAULT_COURANT).unwrap();
    let face = |rows: usize, cols: usize, r: (usize, usize), c: (usize, usize)| {
        let mut m = vec![false; rows * cols];
        for a in r.0..=r.1 {
            for b in c.0..=c.1 {
                m[a * cols + b] = true;
            }
        }
        m
    };
    for k in [k0, k1] {
        sim.add_pec_face(Axis::Z, k, &face(ny, nx, (i0, i1), (j0, j1)))
            .unwrap();
    }
    for i in [i0, i1] {
        sim.add_pec_face(Axis::Y, i, &face(nx, nz, (j0, j1), (k0, k1)))
            .unwrap();
    }
    for j in [j0, j1] {
        sim.add_pec_face(Axis::X, j, &face(ny, nz, (i0, i1), (k0, k1)))
            .unwrap();
    }
    let (lx, ly) = (8.0 * dx, 6.0 * dy);
    let (kx, ky) = (PI / lx, PI / ly);
    let analytic = C0 / (2.0 * PI) * (kx * kx + ky * ky).sqrt();
    let tau = 2.0 / analytic;
    let edge = (i0 + 3, j0 + 4, k0 + 2);
    sim.add_source(
        Component::Ez,
        &[edge],
        SourceKind::Soft,
        Waveform::Pulse {
            t0: 4.0 * tau,
            tau,
            f0: analytic,
        },
    )
    .unwrap();
    sim.add_probe(Component::Ez, edge.0, edge.1, edge.2)
        .unwrap();
    let outside = sim.add_probe(Component::Ez, 1, 1, 1).unwrap();
    sim.run(20_000, &mut never()).unwrap();
    let rec: Vec<f64> = sim.record().iter().step_by(2).copied().collect();
    assert!(sim
        .record()
        .iter()
        .skip(outside)
        .step_by(2)
        .all(|&v| v == 0.0));
    let skip = (8.0 * tau / sim.dt()).ceil() as usize + 50;
    let x = &rec[skip..];
    assert_mode(
        "interior PEC box ez (1,1,0)",
        ModeCheck {
            measured: peak_freq(x, sim.dt(), 0.95 * analytic, 1.05 * analytic),
            yee: yee_freq(&sim, 1.0, kx, ky, 0.0),
            analytic,
            bin: 1.0 / (x.len() as f64 * sim.dt()),
        },
    );
}

#[test]
fn media_and_pec_are_validated() {
    let mk = || FdtdSim::new(5, 4, 3, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    let u = CellMap::Uniform;
    let mut s = mk();
    let n = s.n_cells();
    for (eps, mu, sig, msg) in [
        (u(0.0), u(1.0), u(0.0), "eps_r must be finite and > 0"),
        (u(1.0), u(-1.0), u(0.0), "mu_r must be finite and > 0"),
        (u(1.0), u(1.0), u(-1.0), "sigma must be finite and >= 0"),
        (u(f64::NAN), u(1.0), u(0.0), "eps_r must be finite"),
        (u(1.0), u(1.0), u(f64::INFINITY), "sigma must be finite"),
        (
            CellMap::Cells(vec![1.0; n - 1]),
            u(1.0),
            u(0.0),
            "eps_r has 59 cells, the grid has 60",
        ),
    ] {
        let e = s.set_media(eps, mu, sig).unwrap_err().to_string();
        assert!(e.contains(msg), "{e}");
    }
    assert!(s.add_pec_volume(&vec![false; n + 1]).is_err());
    assert!(s.add_pec_face(Axis::Z, 4, &[true; 20]).is_err());
    assert!(s.add_pec_face(Axis::Z, 3, &[true; 19]).is_err());
    assert!(s.add_pec_face(Axis::Z, 3, &[true; 20]).is_ok());
    assert!(s.add_pec_face(Axis::Y, 4, &[true; 15]).is_ok());
    assert!(s.add_pec_face(Axis::X, 5, &[true; 12]).is_ok());

    // PEC versus sources, in both orders.
    let w = || Waveform::Samples(vec![1.0]);
    let mut s = mk();
    s.add_source(Component::Ex, &[(1, 1, 1)], SourceKind::Soft, w())
        .unwrap();
    let mut nodes = vec![false; 20];
    nodes[5 + 1] = true;
    nodes[5 + 2] = true;
    let e = s.add_pec_face(Axis::Z, 1, &nodes).unwrap_err().to_string();
    assert!(e.contains("existing ex source"), "{e}");
    assert!(!s.is_pec_edge(Component::Ex, 1, 1, 1));
    nodes[5 + 1] = false;
    s.add_pec_face(Axis::Z, 1, &nodes).unwrap();
    let mut s = mk();
    s.add_pec_face(Axis::Z, 1, &{
        let mut m = vec![false; 20];
        m[6] = true;
        m[7] = true;
        m
    })
    .unwrap();
    assert!(s
        .add_source(Component::Ex, &[(1, 1, 1)], SourceKind::Soft, w())
        .is_err());

    // Setup only before the first step.
    let mut s = mk();
    s.run(1, &mut never()).unwrap();
    let e = s.set_media(u(2.0), u(1.0), u(0.0)).unwrap_err().to_string();
    assert!(e.contains("before the first step"), "{e}");
    assert!(s.add_pec_volume(&vec![true; n]).is_err());
    assert!(s.add_pec_face(Axis::Z, 1, &[true; 20]).is_err());
}

#[test]
fn bytes_count_the_coefficients() {
    let mut s = FdtdSim::new(5, 4, 3, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    let n = s.n_cells();
    assert_eq!(s.bytes(), 6 * n * 8);
    s.add_pec_volume(&vec![false; n]).unwrap();
    assert_eq!(s.bytes(), 6 * n * 8);
    s.set_media(
        CellMap::Cells(vec![2.0; n]),
        CellMap::Uniform(1.0),
        CellMap::Uniform(0.1),
    )
    .unwrap();
    // eps map + Cb + Ca
    assert_eq!(s.bytes(), 6 * n * 8 + n * 8 + 6 * n * 8);
}

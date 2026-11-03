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

/// CPML fully off takes the 0.3.50 Yee kernel. `PHASE1_GOLDEN` is that
/// kernel's FNV-1a on `golden_sim` (field bits and the probe record).
#[test]
fn cpml_off_matches_v0_3_50_kernel_bit_for_bit() {
    assert_eq!(golden_hash(golden_sim()), PHASE1_GOLDEN);

    let mut s = golden_sim();
    s.set_cpml(0, CpmlParams::default()).unwrap();
    assert_eq!(golden_hash(s), PHASE1_GOLDEN);

    let mut s = golden_sim();
    s.set_cpml(3, CpmlParams::default()).unwrap();
    for face in Face::ALL {
        s.set_cpml_face(face, 0).unwrap();
    }
    assert_eq!(s.cpml_faces(), [0; 6]);
    assert_eq!(golden_hash(s), PHASE1_GOLDEN);
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
    let with_media = s.bytes();
    s.set_cpml(1, CpmlParams::default()).unwrap();
    // full q (12 · ncell) plus kinv/b/a on both E and H samples
    assert_eq!(s.bytes(), with_media + 12 * n * 8 + 2 * 3 * (5 + 4 + 3) * 8);
    assert!(s.absorber.is_full());
    s.set_cpml(0, CpmlParams::default()).unwrap();
    assert_eq!(s.bytes(), with_media);
    assert!(!s.absorber.is_full());
}

// ---------------------------------------------------------------- Phase 3 (CPML)

/// Sample `s` (1-based) drives step `s`:
/// `w(s·dt) = −((t − t0)/τ)·exp(−((t − t0)/τ)²)` with `t0 = 4τ`.
fn dgauss(nsteps: usize, dt: f64, tau_steps: f64) -> Vec<f64> {
    let tau = tau_steps * dt;
    let t0 = 4.0 * tau;
    (1..=nsteps)
        .map(|s| {
            let u = (s as f64 * dt - t0) / tau;
            -u * (-u * u).exp()
        })
        .collect()
}

fn coef_at(rho: f64, delta: f64, dt: f64, p: &CpmlParams) -> (f64, f64, f64) {
    let sigma_max = p.sigma_factor * (p.order + 1.0) * EPS0 * C0 / delta;
    let rho_m = rho.powf(p.order);
    let sigma = sigma_max * rho_m;
    let kappa = 1.0 + (p.kappa_max - 1.0) * rho_m;
    let alpha = p.alpha_max * (1.0 - rho).powf(p.alpha_order);
    let b = (-(sigma / kappa + alpha) * dt / EPS0).exp();
    let a = if sigma == 0.0 {
        0.0
    } else {
        sigma / (kappa * (sigma + kappa * alpha)) * (b - 1.0)
    };
    (1.0 / kappa, b, a)
}

fn rel_close(got: f64, exp: f64) {
    let scale = got.abs().max(exp.abs()).max(1.0);
    assert!(
        (got - exp).abs() <= 4.0 * f64::EPSILON * scale,
        "{got} vs {exp}"
    );
}

#[test]
fn cpml_profile_samples_match_the_formulas() {
    let params = CpmlParams {
        sigma_factor: 1.0,
        kappa_max: 2.0,
        alpha_max: 0.05,
        order: 3.0,
        alpha_order: 1.0,
    };
    let delta = 1e-3;
    let dt = cfl_dt(delta, delta, delta, DEFAULT_COURANT);
    let n = 40;
    let n_lo = 8;
    let n_hi = 8;
    let e = cpml::Profile::bake(n, n_lo, n_hi, delta, dt, &params, false);
    let h = cpml::Profile::bake(n, n_lo, n_hi, delta, dt, &params, true);

    let rho_e = |m: usize| -> Option<f64> {
        if m < n_lo {
            Some((n_lo - m) as f64 / n_lo as f64)
        } else if m > n - n_hi {
            Some((m - (n - n_hi)) as f64 / n_hi as f64)
        } else {
            None
        }
    };
    let rho_h = |m: usize| -> Option<f64> {
        if m < n_lo {
            Some((n_lo as f64 - m as f64 - 0.5) / n_lo as f64)
        } else if m >= n - n_hi {
            Some((m as f64 + 0.5 - (n - n_hi) as f64) / n_hi as f64)
        } else {
            None
        }
    };
    let check = |profile: &cpml::Profile, half: bool| {
        for m in 0..n {
            let (k, b, a) = (profile.kinv[m], profile.b[m], profile.a[m]);
            assert!(a <= 0.0 && b > 0.0 && b <= 1.0, "m={m} a={a} b={b}");
            match if half { rho_h(m) } else { rho_e(m) } {
                None => {
                    assert_eq!(k.to_bits(), 1.0f64.to_bits(), "m={m}");
                    assert_eq!(b.to_bits(), 1.0f64.to_bits(), "m={m}");
                    assert_eq!(a.to_bits(), 0.0f64.to_bits(), "m={m}");
                }
                Some(rho) => {
                    let (ek, eb, ea) = coef_at(rho, delta, dt, &params);
                    rel_close(k, ek);
                    rel_close(b, eb);
                    rel_close(a, ea);
                }
            }
        }
    };
    check(&e, false);
    check(&h, true);
    // Worked rows, N = 40, n = 8 (section 3.2).
    for (m, rho) in [
        (0, 1.0),
        (1, 7.0 / 8.0),
        (7, 1.0 / 8.0),
        (33, 1.0 / 8.0),
        (39, 7.0 / 8.0),
    ] {
        let (ek, eb, ea) = coef_at(rho, delta, dt, &params);
        rel_close(e.kinv[m], ek);
        rel_close(e.b[m], eb);
        rel_close(e.a[m], ea);
    }
    assert_eq!(e.kinv[8].to_bits(), 1.0f64.to_bits());
    assert_eq!(e.a[32].to_bits(), 0.0f64.to_bits());
    for (m, rho) in [
        (0, 7.5 / 8.0),
        (7, 0.5 / 8.0),
        (32, 0.5 / 8.0),
        (39, 7.5 / 8.0),
    ] {
        let (ek, eb, ea) = coef_at(rho, delta, dt, &params);
        rel_close(h.kinv[m], ek);
        rel_close(h.b[m], eb);
        rel_close(h.a[m], ea);
    }
    assert_eq!(h.b[8].to_bits(), 1.0f64.to_bits());
    assert_eq!(h.a[31].to_bits(), 0.0f64.to_bits());

    // `n_hi = 0` never selects the hi side (`len - 0 = len`).
    let e0 = cpml::Profile::bake(12, 4, 0, delta, dt, &params, false);
    let h0 = cpml::Profile::bake(12, 4, 0, delta, dt, &params, true);
    for m in 4..12 {
        assert_eq!(e0.a[m].to_bits(), 0.0f64.to_bits(), "E m={m}");
        assert_eq!(h0.kinv[m].to_bits(), 1.0f64.to_bits(), "H m={m}");
    }
    assert!(e0.a[0] < 0.0 && h0.a[3] < 0.0);
}

#[test]
fn cpml_sigma_zero_matches_phase1_bit_for_bit() {
    let mut s = golden_sim();
    s.set_cpml(
        3,
        CpmlParams {
            sigma_factor: 0.0,
            ..CpmlParams::default()
        },
    )
    .unwrap();
    assert_eq!(golden_hash(s), PHASE1_GOLDEN);

    // α without σ is still the identity: `a` is the literal `+0.0`.
    let mut s = golden_sim();
    s.set_cpml(
        3,
        CpmlParams {
            sigma_factor: 0.0,
            alpha_max: 0.05,
            ..CpmlParams::default()
        },
    )
    .unwrap();
    assert_eq!(golden_hash(s), PHASE1_GOLDEN);

    let lossy = |cpml: bool| {
        let mut s = golden_sim();
        let n = s.n_cells();
        s.set_media(
            CellMap::Uniform(1.5),
            CellMap::Uniform(2.0),
            CellMap::Uniform(0.02),
        )
        .unwrap();
        let mut cells = vec![false; n];
        // Away from the golden Ez stack at (4, 5, 3..4) and the Ex source at (2, 2, 2).
        cells[(5 * 9 + 6) * 10 + 7] = true;
        s.add_pec_volume(&cells).unwrap();
        if cpml {
            s.set_cpml(
                3,
                CpmlParams {
                    sigma_factor: 0.0,
                    ..CpmlParams::default()
                },
            )
            .unwrap();
        }
        s
    };
    assert_eq!(golden_hash(lossy(true)), golden_hash(lossy(false)));
}

#[test]
fn cpml_defaults_change_the_fields() {
    let mut plain = golden_sim();
    let mut absorbed = golden_sim();
    absorbed.set_cpml(3, CpmlParams::default()).unwrap();
    plain.run(300, &mut never()).unwrap();
    absorbed.run(300, &mut never()).unwrap();
    let hash = fnv(absorbed
        .field_bits()
        .into_iter()
        .chain(absorbed.record().iter().map(|x| x.to_bits())));
    assert_ne!(hash, PHASE1_GOLDEN);
    // The golden current is a polynomial, not zero-mean, so it leaves charge.
    // An α = 0 CPML turns that into a slow late-time growth and the field
    // energy ends higher than the PEC box (about 5× at step 300). T4, with a
    // zero-mean derivative-of-Gaussian, is the absorption check.
    assert_ne!(absorbed.energy().to_bits(), plain.energy().to_bits());
}

#[test]
fn cpml_box_drains_while_pec_box_keeps_energy() {
    let run = |cpml: bool| {
        let mut s = FdtdSim::new(20, 20, 20, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
        let w = dgauss(300, s.dt(), 6.0);
        s.add_source(
            Component::Ez,
            &[(10, 10, 10)],
            SourceKind::Soft,
            Waveform::Samples(w),
        )
        .unwrap();
        if cpml {
            s.set_cpml(6, CpmlParams::default()).unwrap();
        }
        let mut energy = Vec::with_capacity(300);
        for _ in 0..300 {
            s.run(1, &mut never()).unwrap();
            energy.push(s.energy());
        }
        energy
    };
    let pec = run(false);
    // Step 8τ + 10. `energy()` is Σ E² + H² at the end of the leapfrog step,
    // not the conserved quadratic form, so one sample swings by about 10%
    // while the mode beats. The late-time mean does not drift.
    let anchor_at = 8 * 6 + 10 - 1;
    let late = &pec[anchor_at..];
    let late_mean = late.iter().sum::<f64>() / late.len() as f64;
    let anchor = pec[anchor_at];
    assert!(
        (late_mean - anchor).abs() / anchor < 0.01,
        "late mean {late_mean} vs anchor {anchor}"
    );
    let swing = late
        .iter()
        .map(|e| (e - late_mean).abs())
        .fold(0.0_f64, f64::max)
        / late_mean;
    assert!(swing < 0.12, "energy swing {swing}");
    let cpml = run(true);
    let peak = cpml.iter().copied().fold(0.0_f64, f64::max);
    let ratio = cpml[299] / peak;
    println!("cpml energy(300)/peak = {ratio:.3e}");
    assert!(ratio < 1e-4, "energy ratio {ratio}");
}

/// The reflection is `test − reference`, where `reference` is the same source
/// and probe in a PEC box large enough that no wall echo reaches the probe
/// inside the window. The interior update is the same arithmetic in both
/// grids (same `dx`, so the same `dt`), so the difference is exactly what the
/// absorber sends back, plus whatever comes back from the PEC wall behind it.
///
/// Sizing, cubic cells, Courant 0.99 (1.7495 steps/cell), `t0 = 4τ`:
/// `W ≥ 8τ + 1.75·(N_t + p) + 10` holds the source plus the test-grid echo.
/// The reference echo's leading edge (`t0 − 3τ + 1.75·(L − p)`) must arrive
/// after `W`. Here `N_t = 28`, `n = 8`, `p = 3`, `τ = 8`, `W = 128`, `L = 76`
/// (leading edge ≈ step 135.7).
fn reflection_db(
    n_t: usize,
    npml: usize,
    src: (usize, usize, usize),
    probe_off: usize,
    tau_steps: f64,
    w: usize,
    reference_l: usize,
    alpha_max: f64,
) -> (f64, f64, f64) {
    let dx = 1e-3;
    let dt = cfl_dt(dx, dx, dx, DEFAULT_COURANT);
    let wave = dgauss(w, dt, tau_steps);
    let series = |n: usize, origin: (usize, usize, usize), cpml: bool| {
        let mut s = FdtdSim::new(n, n, n, dx, dx, dx, DEFAULT_COURANT).unwrap();
        s.add_source(
            Component::Ez,
            &[origin],
            SourceKind::Soft,
            Waveform::Samples(wave.clone()),
        )
        .unwrap();
        let probe = (origin.0, origin.1 + probe_off, origin.2);
        s.add_probe(Component::Ez, probe.0, probe.1, probe.2)
            .unwrap();
        if cpml {
            s.set_cpml(
                npml,
                CpmlParams {
                    alpha_max,
                    ..CpmlParams::default()
                },
            )
            .unwrap();
        }
        s.run(w, &mut never()).unwrap();
        s.record().to_vec()
    };
    let test = series(n_t, src, true);
    let reference = series(
        reference_l,
        (reference_l / 2, reference_l / 2, reference_l / 2),
        false,
    );
    metric(&test, &reference, tau_steps, probe_off)
}

fn metric(test: &[f64], reference: &[f64], tau_steps: f64, probe_off: usize) -> (f64, f64, f64) {
    let peak = reference.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
    let err = test
        .iter()
        .zip(reference)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f64, f64::max);
    let r = 20.0 * (err / peak).log10();
    let late_from = (8.0 * tau_steps + 1.75 * probe_off as f64 + 5.0).ceil() as usize;
    let late_peak = reference[late_from - 1..]
        .iter()
        .map(|v| v.abs())
        .fold(0.0_f64, f64::max);
    let late_err = test[late_from - 1..]
        .iter()
        .zip(&reference[late_from - 1..])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f64, f64::max);
    let late = 20.0 * (late_err / late_peak.max(peak * 1e-30)).log10();
    // The brief's late ratio uses the late reference peak as the denominator.
    // After the direct pulse that peak is the quiet tail, so the ratio can
    // read positive even when the residual is far below the incident. The
    // third number is the same late error against the incident peak.
    let late_vs_peak = 20.0 * (late_err / peak).log10();
    (r, late, late_vs_peak)
}

#[test]
fn cpml_reflection_below_40_db() {
    let (r, late, late_vs_peak) = reflection_db(28, 8, (14, 14, 14), 3, 8.0, 128, 76, 0.0);
    println!(
        "debug CPML reflection {r:.2} dB, late {late:.2} dB, late vs incident {late_vs_peak:.2} dB"
    );
    assert!(r < -40.0, "reflection {r} dB");

    // The same 28³ box with PEC walls is visible to the metric.
    let dx = 1e-3;
    let dt = cfl_dt(dx, dx, dx, DEFAULT_COURANT);
    let w = 128;
    let wave = dgauss(w, dt, 8.0);
    let pec = {
        let mut s = FdtdSim::new(28, 28, 28, dx, dx, dx, DEFAULT_COURANT).unwrap();
        s.add_source(
            Component::Ez,
            &[(14, 14, 14)],
            SourceKind::Soft,
            Waveform::Samples(wave.clone()),
        )
        .unwrap();
        s.add_probe(Component::Ez, 14, 17, 14).unwrap();
        s.run(w, &mut never()).unwrap();
        s.record().to_vec()
    };
    let reference = {
        let mut s = FdtdSim::new(76, 76, 76, dx, dx, dx, DEFAULT_COURANT).unwrap();
        s.add_source(
            Component::Ez,
            &[(38, 38, 38)],
            SourceKind::Soft,
            Waveform::Samples(wave),
        )
        .unwrap();
        s.add_probe(Component::Ez, 38, 41, 38).unwrap();
        s.run(w, &mut never()).unwrap();
        s.record().to_vec()
    };
    let (pec_r, _, _) = metric(&pec, &reference, 8.0, 3);
    println!("debug PEC reflection {pec_r:.2} dB");
    assert!(pec_r > -20.0, "PEC reflection {pec_r} dB should be obvious");
}

#[test]
#[ignore = "release-size reference run: cargo test --release -p rustlab-dsp -- --ignored cpml_reflection"]
fn cpml_reflection_hello_geometry() {
    let (r, late, late_vs_peak) = reflection_db(40, 8, (20, 20, 20), 6, 20.0, 250, 144, 0.0);
    println!(
        "release CPML alpha=0 reflection {r:.2} dB, late {late:.2} dB, late vs incident {late_vs_peak:.2} dB"
    );
    assert!(r < -40.0, "reflection {r} dB");
    let (ra, late_a, late_a_vs_peak) = reflection_db(40, 8, (20, 20, 20), 6, 20.0, 250, 144, 0.033);
    println!(
        "release CPML alpha=0.033 reflection {ra:.2} dB, late {late_a:.2} dB, late vs incident {late_a_vs_peak:.2} dB"
    );
    assert!(ra < -40.0, "alpha reflection {ra} dB");
}

#[test]
fn cpml_split_run_is_bit_identical() {
    let build = || {
        let mut sim = FdtdSim::new(24, 22, 20, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
        sim.set_cpml(6, CpmlParams::default()).unwrap();
        let tau = 15.0 * sim.dt();
        sim.add_source(
            Component::Ez,
            &[(10, 12, 8), (10, 12, 9)],
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
            &[(8, 8, 8)],
            SourceKind::Hard,
            Waveform::Samples(vec![1.0, 2.0, 3.0]),
        )
        .unwrap();
        sim.add_probe(Component::Ez, 10, 14, 8).unwrap();
        sim.add_probe(Component::Hy, 10, 13, 8).unwrap();
        sim
    };
    let mut a = build();
    let mut b = build();
    a.run(150, &mut never()).unwrap();
    b.run(70, &mut never()).unwrap();
    b.run(80, &mut never()).unwrap();
    assert_eq!(a.field_bits(), b.field_bits());
    assert_eq!(
        a.record().iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
        b.record().iter().map(|x| x.to_bits()).collect::<Vec<_>>()
    );
    let mut c = build();
    c.run(150, &mut never()).unwrap();
    assert_eq!(a.field_bits(), c.field_bits());
}

#[test]
fn cpml_each_face_absorbs_on_its_own_side() {
    // nx, ny, nz all different so an axis swap cannot hide. Probes sit one
    // cell inside each face's interface (section 8.1, T7).
    let nx = 22;
    let ny = 20;
    let nz = 18;
    let n = 4;
    let w = 100;
    let probes = [
        (Component::Ez, 10, 5, 9),   // xmin
        (Component::Ez, 10, 17, 9),  // xmax, interface node 18
        (Component::Ez, 5, 11, 9),   // ymin
        (Component::Ez, 15, 11, 9),  // ymax, interface node 16
        (Component::Ex, 10, 11, 5),  // zmin
        (Component::Ex, 10, 11, 13), // zmax, interface node 14
    ];
    let run = |off: Option<Face>| -> Vec<Vec<f64>> {
        let mut s = FdtdSim::new(nx, ny, nz, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
        s.set_cpml(n, CpmlParams::default()).unwrap();
        if let Some(face) = off {
            s.set_cpml_face(face, 0).unwrap();
        }
        let wave = dgauss(w, s.dt(), 6.0);
        for comp in [Component::Ez, Component::Ex] {
            s.add_source(
                comp,
                &[(10, 11, 9)],
                SourceKind::Soft,
                Waveform::Samples(wave.clone()),
            )
            .unwrap();
        }
        for &(c, i, j, k) in &probes {
            s.add_probe(c, i, j, k).unwrap();
        }
        s.run(w, &mut never()).unwrap();
        (0..6)
            .map(|p| s.record().iter().skip(p).step_by(6).copied().collect())
            .collect()
    };
    let reference = run(None);
    let faces = Face::ALL;
    for (f, face) in faces.iter().enumerate() {
        let got = run(Some(*face));
        let mut first = [0usize; 6];
        let mut peak_f = 0.0;
        let mut diff_f = 0.0;
        for g in 0..6 {
            let peak = reference[g].iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
            let diff: Vec<f64> = got[g]
                .iter()
                .zip(&reference[g])
                .map(|(a, b)| a - b)
                .collect();
            let max_diff = diff.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
            first[g] = diff
                .iter()
                .position(|d| d.abs() > 1e-3 * peak)
                .map(|i| i + 1)
                .unwrap_or(w + 1);
            if g == f {
                peak_f = peak;
                diff_f = max_diff;
            }
        }
        println!(
            "face {} off, first step above 1e-3 of the reference peak: {:?}",
            face.name(),
            first
        );
        for g in 0..6 {
            if g != f {
                assert!(
                    first[f] < first[g],
                    "{} reflected at step {} before its own probe (step {})",
                    faces[g].name(),
                    first[g],
                    first[f]
                );
            }
        }
        assert!(
            diff_f > 0.03 * peak_f,
            "{} echo {diff_f} vs 0.03·{peak_f}",
            face.name()
        );
    }
}

#[test]
fn cpml_then_media_equals_media_then_cpml() {
    // `eps_r = 2` does not move `dt`: Phase 2 caps the step at the vacuum
    // CFL, and only `min(eps)·min(mu) < 1` speeds the wave up. A stale-`dt`
    // rebake (P6) is invisible unless the medium actually changes `dt`, so
    // this uses `eps_r = 0.25`.
    let setup = |cpml_first: bool| {
        let mut s = FdtdSim::new(12, 10, 10, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
        let vacuum_dt = s.dt();
        let wave = dgauss(100, vacuum_dt, 6.0);
        s.add_source(
            Component::Ez,
            &[(5, 6, 5)],
            SourceKind::Soft,
            Waveform::Samples(wave),
        )
        .unwrap();
        let media = |s: &mut FdtdSim| {
            s.set_media(
                CellMap::Uniform(0.25),
                CellMap::Uniform(1.0),
                CellMap::Uniform(0.0),
            )
            .unwrap();
        };
        let cpml = |s: &mut FdtdSim| s.set_cpml(3, CpmlParams::default()).unwrap();
        if cpml_first {
            cpml(&mut s);
            media(&mut s);
        } else {
            media(&mut s);
            cpml(&mut s);
        }
        assert!(s.dt() < vacuum_dt, "dt {} vs vacuum {vacuum_dt}", s.dt());
        s.run(100, &mut never()).unwrap();
        s.field_bits()
    };
    assert_eq!(setup(true), setup(false));
}

#[test]
fn cpml_setup_rules() {
    // z is the short axis, so `n = 3` fails there and nowhere else (3+3 < 8).
    let mk = || FdtdSim::new(8, 8, 5, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    let mut s = mk();
    s.run(1, &mut never()).unwrap();
    let e = s
        .set_cpml(1, CpmlParams::default())
        .unwrap_err()
        .to_string();
    assert!(e.contains("before the first step"), "{e}");

    let mut s = mk();
    let bytes = s.bytes();
    let e = s
        .set_cpml(3, CpmlParams::default())
        .unwrap_err()
        .to_string();
    // smallest axis is z = 5; 3 + 3 is not fewer than 5
    assert!(e.contains("leaves no interior on z"), "{e}");
    assert_eq!(s.cpml_faces(), [0; 6]);
    assert_eq!(s.bytes(), bytes);

    let e = s.set_cpml_face(Face::ZMin, 0).unwrap_err().to_string();
    assert!(e.contains("before turning a face"), "{e}");
    assert_eq!(s.bytes(), bytes);

    s.set_cpml(2, CpmlParams::default()).unwrap();
    assert_eq!(s.cpml_faces(), [2; 6]);
    let e = s.set_cpml_face(Face::XMax, 1).unwrap_err().to_string();
    assert!(e.contains("0 (off) or the absorber thickness 2"), "{e}");
    assert_eq!(s.cpml_faces(), [2; 6]);

    for (params, msg) in [
        (
            CpmlParams {
                sigma_factor: -1.0,
                ..CpmlParams::default()
            },
            "sigma_factor must be finite and >= 0",
        ),
        (
            CpmlParams {
                kappa_max: 0.5,
                ..CpmlParams::default()
            },
            "kappa_max must be finite and >= 1",
        ),
        (
            CpmlParams {
                alpha_max: f64::NAN,
                ..CpmlParams::default()
            },
            "alpha_max must be finite and >= 0",
        ),
        (
            CpmlParams {
                order: -1.0,
                ..CpmlParams::default()
            },
            "order must be finite and >= 0",
        ),
        (
            CpmlParams {
                alpha_order: f64::INFINITY,
                ..CpmlParams::default()
            },
            "alpha_order must be finite and >= 0",
        ),
    ] {
        let before = s.cpml_faces();
        let bytes = s.bytes();
        let e = s.set_cpml(2, params).unwrap_err().to_string();
        assert!(e.contains(msg), "{e}");
        assert_eq!(s.cpml_faces(), before);
        assert_eq!(s.bytes(), bytes);
    }

    s.set_cpml_face(Face::ZMin, 0).unwrap();
    assert_eq!(s.cpml_faces(), [2, 2, 2, 2, 0, 2]);
    s.set_cpml_face(Face::ZMin, 2).unwrap();
    assert_eq!(s.cpml_faces(), [2; 6]);
    let on = s.bytes();
    s.set_cpml(0, CpmlParams::default()).unwrap();
    assert_eq!(s.cpml_faces(), [0; 6]);
    assert_eq!(s.bytes(), bytes);
    assert!(on > bytes);

    // Fresh 5×4×3 grid: the documented byte delta, with no media.
    let mut t = FdtdSim::new(5, 4, 3, 1e-3, 1e-3, 1e-3, DEFAULT_COURANT).unwrap();
    let n = t.n_cells();
    let base = t.bytes();
    t.set_cpml(1, CpmlParams::default()).unwrap();
    assert_eq!(t.bytes(), base + 12 * n * 8 + 2 * 3 * (5 + 4 + 3) * 8);
}

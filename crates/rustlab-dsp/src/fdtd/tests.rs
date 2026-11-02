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

/// Yee dispersion: `ω` with `(2/(c dt))² sin²(ω dt/2) = Σ (2/Δ)² sin²(kΔ/2)`.
fn yee_freq(sim: &FdtdSim, kx: f64, ky: f64, kz: f64) -> f64 {
    let (dx, dy, dz) = sim.spacing();
    let dt = sim.dt();
    let s = |k: f64, d: f64| (2.0 / d * (k * d / 2.0).sin()).powi(2);
    let rhs = s(kx, dx) + s(ky, dy) + s(kz, dz);
    let arg = C0 * dt / 2.0 * rhs.sqrt();
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
    let (nx, ny, nz) = (8, 6, 4);
    let (dx, dy, dz) = (1.0e-3, 1.2e-3, 0.9e-3);
    let mut sim = FdtdSim::new(nx, ny, nz, dx, dy, dz, DEFAULT_COURANT).unwrap();
    let (lx, ly, lz) = (nx as f64 * dx, ny as f64 * dy, nz as f64 * dz);
    let (kx, ky, kz) = (
        m[0] as f64 * PI / lx,
        m[1] as f64 * PI / ly,
        m[2] as f64 * PI / lz,
    );
    let analytic = C0 / (2.0 * PI) * (kx * kx + ky * ky + kz * kz).sqrt();
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
        yee: yee_freq(&sim, kx, ky, kz),
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

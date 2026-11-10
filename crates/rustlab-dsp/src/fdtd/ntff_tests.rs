//! Independent checks for the near-to-far transform. The numbers are the
//! gates in `dev/plans/fdtd3d_phase5_impl.md`. G8 is the corrected reading
//! (peak directivity, and D ∝ sin²θ), not a pointwise comparison with 1.5.

use super::*;
use num_complex::Complex;
use rustlab_core::C64;
use std::f64::consts::PI;
use std::sync::OnceLock;

fn never() -> impl FnMut() -> bool {
    || false
}

fn dgauss(dt: f64, n: usize, fc: f64) -> Vec<f64> {
    let tau = 2.0f64.sqrt() / (2.0 * PI * fc);
    let t0 = 5.0 * tau;
    (1..=n)
        .map(|s| {
            let u = (s as f64 * dt - t0) / tau;
            -u * (-u * u).exp()
        })
        .collect()
}

fn moment(g: &[f64], dt: f64, f: f64, area: f64) -> C64 {
    let w = 2.0 * PI * f;
    let mut acc = C64::new(0.0, 0.0);
    for (n, &v) in g.iter().enumerate() {
        let t = (n as f64 + 0.5) * dt;
        acc += Complex::from_polar(v * dt, -w * t);
    }
    acc * area
}

fn grid(n: usize, pml: usize) -> FdtdSim {
    let d = 1.0e-3;
    let mut s = FdtdSim::new(n, n, n, d, d, d, 0.99).unwrap();
    s.set_cpml(pml, CpmlParams::default()).unwrap();
    s
}

struct ZDipole {
    sim: FdtdSim,
    g: Vec<f64>,
    /// λ/Δ = 40, 30, 20.
    freqs: [f64; 3],
}

fn z_dipole() -> &'static ZDipole {
    static RUN: OnceLock<ZDipole> = OnceLock::new();
    RUN.get_or_init(|| {
        let mut sim = grid(32, 6);
        let fc = C0 / 0.020;
        let g = dgauss(sim.dt(), 700, fc);
        sim.add_source(
            Component::Ez,
            &[(16, 16, 16)],
            SourceKind::Soft,
            Waveform::Samples(g.clone()),
        )
        .unwrap();
        let freqs = [C0 / 0.040, C0 / 0.030, C0 / 0.020];
        sim.set_ntff(&freqs, 2).unwrap();
        sim.run(700, &mut never()).unwrap();
        ZDipole { sim, g, freqs }
    })
}

fn idx_of(ratio: f64) -> usize {
    match ratio as i32 {
        40 => 0,
        30 => 1,
        20 => 2,
        _ => panic!("ratio {ratio}"),
    }
}

fn at(ff: &FarField, q: usize, a: usize, b: usize) -> (C64, C64) {
    let np = ff.phi.len();
    let slot = (q * ff.theta.len() + a) * np + b;
    (ff.e_theta[slot], ff.e_phi[slot])
}

fn fan_z(k: f64, eta: f64, il: C64, theta: f64) -> C64 {
    Complex::new(0.0, eta * k / (4.0 * PI)) * il * theta.sin()
}

/// θ = 90° and an elevation cut, origin on the Ez edge.
fn cuts(run: &ZDipole) -> FarField {
    let theta: Vec<f64> = (0..=36).map(|a| PI * a as f64 / 36.0).collect();
    let phi: Vec<f64> = (0..72).map(|b| 2.0 * PI * b as f64 / 72.0).collect();
    let origin = [16.0e-3, 16.0e-3, 16.5e-3];
    run.sim
        .far_field(&theta, &phi, Some(origin), RecordDecay::Pulse)
        .unwrap()
}

fn cuts_cached() -> &'static FarField {
    static FF: OnceLock<FarField> = OnceLock::new();
    FF.get_or_init(|| cuts(z_dipole()))
}

#[test]
fn g1_amplitude_and_phase_at_the_source() {
    let run = z_dipole();
    let ff = cuts_cached();
    assert_eq!(run.sim.ntff_sample_count(), Some(3264));
    let a90 = ff
        .theta
        .iter()
        .position(|t| (*t - PI / 2.0).abs() < 1e-12)
        .unwrap();
    for (ratio, amp, phase) in [(30.0, 0.01, 0.02), (20.0, 0.02, 0.03)] {
        let q = idx_of(ratio);
        let il = moment(&run.g, run.sim.dt(), run.freqs[q], 1.0e-6);
        let analytic = fan_z(ff.k[q], ff.eta, il, PI / 2.0);
        let (ft, _) = at(ff, q, a90, 0);
        let rel = ft / analytic;
        assert!(
            (rel.norm() - 1.0).abs() <= amp,
            "λ/Δ={ratio}: |F/Fan| = {}, limit {amp}",
            rel.norm()
        );
        assert!(
            rel.arg().abs() <= phase,
            "λ/Δ={ratio}: arg = {}, limit {phase}",
            rel.arg()
        );
    }
}

#[test]
fn g2_z_dipole_is_e_theta() {
    let ff = cuts_cached();
    let a90 = ff
        .theta
        .iter()
        .position(|t| (*t - PI / 2.0).abs() < 1e-12)
        .unwrap();
    for (ratio, lim) in [(30.0, 2.0e-5), (20.0, 2.0e-4)] {
        let q = idx_of(ratio);
        let mut worst = 0.0f64;
        for b in 0..ff.phi.len() {
            let (ft, fp) = at(ff, q, a90, b);
            worst = worst.max(fp.norm() / ft.norm());
        }
        assert!(
            worst <= lim,
            "λ/Δ={ratio}: max |Fφ|/|Fθ| = {worst}, limit {lim}"
        );
    }
}

#[test]
fn g3_magnitude_shape_and_azimuth_ripple() {
    let run = z_dipole();
    let ff = cuts_cached();
    let q = idx_of(30.0);
    let il = moment(&run.g, run.sim.dt(), run.freqs[q], 1.0e-6);
    let mut shape = 0.0f64;
    for a in 1..36 {
        let th = ff.theta[a];
        let (ft, _) = at(ff, q, a, 0);
        let scale = fan_z(ff.k[q], ff.eta, il, PI / 2.0).norm() * th.sin();
        shape = shape.max((ft.norm() / scale - 1.0).abs());
    }
    assert!(shape <= 0.01, "shape error {shape}");
    let a90 = 18;
    let mut lo = f64::INFINITY;
    let mut hi = 0.0f64;
    for b in 0..ff.phi.len() {
        let m = at(ff, q, a90, b).0.norm();
        lo = lo.min(m);
        hi = hi.max(m);
    }
    let ripple = hi / lo - 1.0;
    assert!(ripple <= 5.0e-3, "azimuth ripple {ripple}");
}

#[test]
fn g6_flux_matches_hertzian_power() {
    let run = z_dipole();
    let ff = cuts_cached();
    for (ratio, lim) in [(30.0, 0.01), (20.0, 0.02)] {
        let q = idx_of(ratio);
        let il = moment(&run.g, run.sim.dt(), run.freqs[q], 1.0e-6);
        let pan = ff.eta * ff.k[q].powi(2) * il.norm_sqr() / (12.0 * PI);
        let rel = (ff.prad[q] / pan - 1.0).abs();
        assert!(rel <= lim, "λ/Δ={ratio}: |P/Pan − 1| = {rel}");
    }
}

#[test]
fn g8_peak_directivity_and_sin2_shape() {
    let ff = cuts_cached();
    for ratio in [30.0, 40.0] {
        let q = idx_of(ratio);
        let mut peak = 0.0f64;
        let mut d_at = vec![0.0; ff.theta.len()];
        for a in 0..ff.theta.len() {
            let (ft, fp) = at(ff, q, a, 0);
            let d = directive_gain(ft, fp, ff.eta, ff.prad[q]);
            d_at[a] = d;
            peak = peak.max(d);
        }
        let perr = (peak / 1.5 - 1.0).abs();
        assert!(
            perr <= 0.01,
            "λ/Δ={ratio}: |Dpeak/1.5 − 1| = {perr}, Dpeak = {peak}"
        );
        let mut ang = 0.0f64;
        for a in 1..36 {
            let expect = peak * ff.theta[a].sin().powi(2);
            ang = ang.max((d_at[a] / expect - 1.0).abs());
        }
        assert!(
            ang <= 0.01,
            "λ/Δ={ratio}: angular |D/(Dpeak sin²θ) − 1| = {ang}"
        );
    }
}

#[test]
fn g7_pattern_integral_matches_flux() {
    let run = z_dipole();
    let nt = 61;
    let np = 120;
    let theta: Vec<f64> = (0..nt).map(|a| PI * a as f64 / (nt - 1) as f64).collect();
    let phi: Vec<f64> = (0..np).map(|b| 2.0 * PI * b as f64 / np as f64).collect();
    let ff = run
        .sim
        .far_field(&theta, &phi, None, RecordDecay::Pulse)
        .unwrap();
    let dth = PI / 60.0;
    let dph = 2.0 * PI / 120.0;
    let q = idx_of(30.0);
    let mut pat = 0.0f64;
    for a in 0..nt {
        let w = if a == 0 || a + 1 == nt { 0.5 } else { 1.0 };
        for b in 0..np {
            let (ft, fp) = at(&ff, q, a, b);
            let u = (ft.norm_sqr() + fp.norm_sqr()) / (2.0 * ff.eta);
            pat += u * theta[a].sin() * w * dth * dph;
        }
    }
    let rel = (pat / ff.prad[q] - 1.0).abs();
    assert!(rel <= 0.005, "|Ppat/Pflux − 1| = {rel}");
}

#[test]
fn g4_x_dipole_polarization() {
    let mut sim = grid(32, 6);
    let fc = C0 / 0.020;
    let g = dgauss(sim.dt(), 700, fc);
    sim.add_source(
        Component::Ex,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(g.clone()),
    )
    .unwrap();
    let f = C0 / 0.030;
    sim.set_ntff(&[f], 2).unwrap();
    sim.run(700, &mut never()).unwrap();
    let theta: Vec<f64> = (0..=18).map(|a| PI * a as f64 / 18.0).collect();
    let phi: Vec<f64> = (0..24).map(|b| 2.0 * PI * b as f64 / 24.0).collect();
    let origin = [16.5e-3, 16.0e-3, 16.0e-3];
    let ff = sim
        .far_field(&theta, &phi, Some(origin), RecordDecay::Pulse)
        .unwrap();
    let il = moment(&g, sim.dt(), f, 1.0e-6);
    let f0 = Complex::new(0.0, ff.eta * ff.k[0] / (4.0 * PI)) * il;
    let a90 = theta
        .iter()
        .position(|t| (*t - PI / 2.0).abs() < 1e-12)
        .unwrap();
    let mut phi_err = 0.0f64;
    let mut theta90 = 0.0f64;
    for b in 0..phi.len() {
        let (ft, fp) = at(&ff, 0, a90, b);
        let expect = f0 * phi[b].sin();
        phi_err = phi_err.max((fp - expect).norm() / f0.norm());
        theta90 = theta90.max(ft.norm() / f0.norm());
    }
    assert!(phi_err <= 0.01, "|Fφ − F0 sinφ|/|F0| = {phi_err}");
    assert!(theta90 <= 1e-12, "|Fθ(90°, φ)|/|F0| = {theta90}");
    let mut theta_err = 0.0f64;
    for a in 0..theta.len() {
        let (ft, _) = at(&ff, 0, a, 0);
        let expect = -f0 * theta[a].cos();
        theta_err = theta_err.max((ft - expect).norm() / f0.norm());
    }
    assert!(theta_err <= 0.01, "|Fθ + F0 cosθ|/|F0| = {theta_err}");
}

#[test]
fn g5_offset_source_phase_tracks_the_box_centre() {
    let mut sim = grid(32, 6);
    let fc = C0 / 0.020;
    let g = dgauss(sim.dt(), 700, fc);
    sim.add_source(
        Component::Ez,
        &[(16, 20, 16)],
        SourceKind::Soft,
        Waveform::Samples(g),
    )
    .unwrap();
    let f = C0 / 0.030;
    sim.set_ntff(&[f], 2).unwrap();
    sim.run(700, &mut never()).unwrap();
    let ff = sim
        .far_field(&[PI / 2.0], &[0.0, PI], None, RecordDecay::Pulse)
        .unwrap();
    let (f0, _) = at(&ff, 0, 0, 0);
    let (fp, _) = at(&ff, 0, 0, 1);
    let kd = ff.k[0] * 4.0e-3;
    let ratio = f0 / fp * Complex::from_polar(1.0, -2.0 * kd);
    assert!(ratio.arg().abs() <= 0.01, "phase error {} rad", ratio.arg());
    assert!(
        (ratio.norm() - 1.0).abs() <= 0.01,
        "magnitude ratio {}",
        ratio.norm()
    );
}

#[test]
fn g9_margin_changes_the_field_by_less_than_half_a_percent() {
    let f = C0 / 0.030;
    let origin = [16.0e-3, 16.0e-3, 16.5e-3];
    let mut fields = [C64::new(0.0, 0.0); 2];
    for (n, margin) in [2usize, 4].into_iter().enumerate() {
        let mut sim = grid(32, 6);
        let g = dgauss(sim.dt(), 700, C0 / 0.020);
        sim.add_source(
            Component::Ez,
            &[(16, 16, 16)],
            SourceKind::Soft,
            Waveform::Samples(g),
        )
        .unwrap();
        sim.set_ntff(&[f], margin).unwrap();
        sim.run(700, &mut never()).unwrap();
        let ff = sim
            .far_field(&[PI / 2.0], &[0.0], Some(origin), RecordDecay::Pulse)
            .unwrap();
        fields[n] = ff.e_theta[0];
    }
    let rel = (fields[0] / fields[1] - 1.0).norm();
    assert!(rel <= 0.005, "|F(margin 2)/F(margin 4) − 1| = {rel}");
}

#[test]
fn g10_port_power_matches_flux_and_gain_identities() {
    let (sim, ff) = port_run();
    let q = 0;
    let port = ff.port.as_ref().unwrap();
    let (ft, fp) = at(&ff, q, 0, 0);
    let d = directive_gain(ft, fp, ff.eta, ff.prad[q]);
    let g = directive_gain(ft, fp, ff.eta, port.accepted[q]);
    let gr = directive_gain(ft, fp, ff.eta, port.incident[q]);
    let p = (port.accepted[q] / ff.prad[q] - 1.0).abs();
    assert!(p <= 1.0e-3, "|Pacc/Pflux − 1| = {p}");
    let gd = (g / d - 1.0).abs();
    assert!(gd <= 1.0e-3, "|G/D − 1| = {gd}");
    let s = port.s11[q].norm_sqr();
    let alg = (gr / (g * (1.0 - s)) - 1.0).abs();
    assert!(
        alg <= 1.0e-9,
        "algebraic |Gr/(G(1−|S11|²)) − 1| = {alg} (self-consistency of the two power definitions)"
    );
    assert!(
        port.accepted[q] > 0.0,
        "accepted power {}",
        port.accepted[q]
    );
    let _ = sim;
}

fn port_run() -> (&'static FdtdSim, &'static FarField) {
    static SIM: OnceLock<FdtdSim> = OnceLock::new();
    static FF: OnceLock<FarField> = OnceLock::new();
    let sim = SIM.get_or_init(|| {
        let mut sim = grid(32, 6);
        let g = dgauss(sim.dt(), 700, C0 / 0.020);
        sim.add_port(Component::Ez, &[(16, 16, 16)], 50.0, Waveform::Samples(g))
            .unwrap();
        let dt = sim.dt();
        let bin = 13.0 / (700.0 * dt);
        sim.set_ntff(&[C0 / 0.030, bin], 2).unwrap();
        sim.run(700, &mut never()).unwrap();
        sim
    });
    let ff = FF.get_or_init(|| {
        sim.far_field(&[PI / 2.0], &[0.0], None, RecordDecay::Pulse)
            .unwrap()
    });
    (sim, ff)
}

#[test]
fn g11_s11_matches_the_fft_at_an_exact_bin() {
    let (sim, ff) = port_run();
    let dt = sim.dt();
    let n = 700.0;
    let m = 13usize;
    let f = m as f64 / (n * dt);
    let spec = s11(
        sim.port_voltage(),
        sim.port_current(),
        dt,
        50.0,
        PortWindow::Rect,
    )
    .unwrap();
    let s_fft = spec.s11[m - 1];
    let s_ntff = ff.port.as_ref().unwrap().s11[1];
    assert!((spec.frequencies[m - 1] - f).abs() / f < 1e-12);
    let err = (s_ntff - s_fft).norm();
    assert!(
        err <= 1e-9 * s_fft.norm(),
        "|S11_ntff − S11_fft| = {err}, |S11| = {}",
        s_fft.norm()
    );
}

#[test]
fn g12_uniform_dielectric_uses_the_medium_k_and_eta() {
    let mut sim = grid(32, 6);
    sim.set_media(
        CellMap::Uniform(2.0),
        CellMap::Uniform(1.0),
        CellMap::Uniform(0.0),
    )
    .unwrap();
    let nmed = 2.0f64.sqrt();
    let f = C0 / (nmed * 0.030);
    let g = dgauss(sim.dt(), 700, C0 / (nmed * 0.020));
    sim.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(g.clone()),
    )
    .unwrap();
    sim.set_ntff(&[f], 2).unwrap();
    sim.run(700, &mut never()).unwrap();
    let origin = [16.0e-3, 16.0e-3, 16.5e-3];
    let theta: Vec<f64> = (1..36).map(|a| PI * a as f64 / 36.0).collect();
    let ff = sim
        .far_field(&theta, &[0.0], Some(origin), RecordDecay::Pulse)
        .unwrap();
    let il = moment(&g, sim.dt(), f, 1.0e-6);
    let a90 = theta
        .iter()
        .position(|t| (*t - PI / 2.0).abs() < 1e-9)
        .unwrap();
    let analytic = fan_z(ff.k[0], ff.eta, il, PI / 2.0);
    let rel = ff.e_theta[a90] / analytic;
    assert!(
        (rel.norm() - 1.0).abs() <= 0.02,
        "|F/Fan| err {}",
        (rel.norm() - 1.0).abs()
    );
    let pan = ff.eta * ff.k[0].powi(2) * il.norm_sqr() / (12.0 * PI);
    let perr = (ff.prad[0] / pan - 1.0).abs();
    assert!(perr <= 0.02, "power err {perr}");
    let mut peak = 0.0f64;
    for a in 0..theta.len() {
        peak = peak.max(directive_gain(
            ff.e_theta[a],
            ff.e_phi[a],
            ff.eta,
            ff.prad[0],
        ));
    }
    let derr = (peak / 1.5 - 1.0).abs();
    assert!(derr <= 0.02, "|Dpeak/1.5 − 1| = {derr}");
}

/// `Pacc − Pflux` against an independent `½ Σ σ |Ê|² ΔV`.
///
/// The cube is the same 4 mm block at every resolution, on a 40 mm domain
/// with 8 mm of CPML and a 2 mm margin, so the Huygens surface does not
/// move. σ = 1 S/m. The centre Ez port edge does not touch the cube, so
/// its material σ is 0 and the port resistor is not in this sum.
///
/// Ê uses the same sample the surface flux uses: E after the update, at
/// time `n·dt`, times `dt`. σ on an edge is the arithmetic mean of the
/// cells around it, the mean the bake uses, and not the port's extra σ.
/// The centred sample `(E^n + E^{n−1})/2` is what the loss update
/// dissipates; it falls on the other side of the gap.
///
/// Measured `(diss / gap − 1)` for the end-of-step sample, then the
/// centred sample, at Δ = 2 mm, 1 mm, ⅔ mm (λ/Δ = 15, 30, 45 at 30 mm):
///
/// | Δ | end-of-step | centred |
/// |---|---|---|
/// | 2 mm | +2.46e-3 | −1.19e-2 |
/// | 1 mm | +1.44e-3 | −2.14e-3 |
/// | ⅔ mm | +7.57e-4 | −8.35e-4 |
///
/// Both shrink, and they bracket the gap. `Pacc/Pflux` is 1.0185 at all
/// three, so the gap is 1.85% of the flux and not a sign check. The
/// bounds sit just above that table. An omitted ½, a missing component,
/// or the port resistor added in would miss by a factor, not by 1e-3.
#[test]
fn loss_balances_accepted_minus_flux_against_sigma_e_squared() {
    let coarse = dissipation_balance(20, 4, 1);
    let mid = dissipation_balance(40, 8, 2);
    let fine = dissipation_balance(60, 12, 3);
    for (name, b) in [("2 mm", coarse), ("1 mm", mid), ("2/3 mm", fine)] {
        assert!(
            (0.015..0.022).contains(&(b.ratio - 1.0)),
            "{name}: Pacc/Pflux − 1 = {:.4e} (gap should be about 1.85% of the flux)",
            b.ratio - 1.0
        );
        assert!(
            b.decay < 1e-3,
            "{name}: surface tail/peak {} with the pulse check on",
            b.decay
        );
        assert!(
            b.end_rel > 0.0,
            "{name}: end-of-step dissipation is below the gap"
        );
        assert!(
            b.avg_rel < 0.0,
            "{name}: centred dissipation is above the gap"
        );
    }
    assert!(
        fine.end_rel < mid.end_rel && mid.end_rel < coarse.end_rel,
        "end-of-step error did not shrink: 2 mm {:.4e}, 1 mm {:.4e}, 2/3 mm {:.4e}",
        coarse.end_rel,
        mid.end_rel,
        fine.end_rel
    );
    assert!(
        fine.avg_rel.abs() < mid.avg_rel.abs() && mid.avg_rel.abs() < coarse.avg_rel.abs(),
        "centred error did not shrink: 2 mm {:.4e}, 1 mm {:.4e}, 2/3 mm {:.4e}",
        coarse.avg_rel,
        mid.avg_rel,
        fine.avg_rel
    );
    assert!(
        coarse.end_rel < 3.5e-3 && fine.end_rel < 1.0e-3 && fine.end_rel < 0.5 * coarse.end_rel,
        "end-of-step (diss/gap − 1) = {:.4e} at 2 mm, {:.4e} at 2/3 mm",
        coarse.end_rel,
        fine.end_rel
    );
    assert!(
        coarse.avg_rel.abs() < 1.5e-2 && fine.avg_rel.abs() < 1.5e-3,
        "centred (diss/gap − 1) = {:.4e} at 2 mm, {:.4e} at 2/3 mm",
        coarse.avg_rel,
        fine.avg_rel
    );
    let s = coarse.s11_sq;
    let alg = (coarse.realized / (coarse.gain * (1.0 - s)) - 1.0).abs();
    assert!(alg <= 1e-9, "algebraic identity err {alg}");
}

#[derive(Clone, Copy)]
struct Dissipation {
    end_rel: f64,
    avg_rel: f64,
    ratio: f64,
    decay: f64,
    gain: f64,
    realized: f64,
    s11_sq: f64,
}

fn dissipation_balance(n: usize, pml: usize, margin: usize) -> Dissipation {
    let sigma0 = 1.0;
    let dx = 0.040 / n as f64;
    let mut sim = FdtdSim::new(n, n, n, dx, dx, dx, 0.99).unwrap();
    sim.set_cpml(pml, CpmlParams::default()).unwrap();
    let mut cells = vec![0.0; n * n * n];
    let a = (0.012 / dx).round() as usize;
    let b = (0.016 / dx).round() as usize;
    for k in a..b {
        for i in a..b {
            for j in a..b {
                cells[(k * n + i) * n + j] = sigma0;
            }
        }
    }
    sim.set_media(
        CellMap::Uniform(1.0),
        CellMap::Uniform(1.0),
        CellMap::Cells(cells.clone()),
    )
    .unwrap();
    let f = C0 / 0.030;
    let g = dgauss(sim.dt(), 700, C0 / 0.020);
    let c = n / 2;
    sim.add_port(Component::Ez, &[(c, c, c)], 50.0, Waveform::Samples(g))
        .unwrap();
    sim.set_ntff(&[f], margin).unwrap();
    let edges = lossy_edges(&cells, n);
    let dt = sim.dt();
    let w = 2.0 * PI * f;
    let dv = dx * dx * dx;
    let mut prev: Vec<f64> = edges.iter().map(|e| sim_field(&sim, e.0)[e.1]).collect();
    let mut end = vec![C64::new(0.0, 0.0); edges.len()];
    let mut avg = vec![C64::new(0.0, 0.0); edges.len()];
    for _ in 0..700 {
        sim.run(1, &mut never()).unwrap();
        let s = sim.steps() as f64;
        let re = Complex::from_polar(dt, -w * s * dt);
        for (p, &(comp, idx, _)) in edges.iter().enumerate() {
            let en = sim_field(&sim, comp)[idx];
            end[p] += re * en;
            avg[p] += re * 0.5 * (en + prev[p]);
            prev[p] = en;
        }
    }
    let pd = |ph: &[C64]| {
        0.5 * edges
            .iter()
            .zip(ph)
            .map(|((_, _, sig), e)| sig * e.norm_sqr())
            .sum::<f64>()
            * dv
    };
    let ff = sim
        .far_field(&[PI / 2.0], &[0.0], None, RecordDecay::Pulse)
        .unwrap();
    let port = ff.port.as_ref().unwrap();
    let gap = port.accepted[0] - ff.prad[0];
    let (ft, fp) = at(&ff, 0, 0, 0);
    Dissipation {
        end_rel: pd(&end) / gap - 1.0,
        avg_rel: pd(&avg) / gap - 1.0,
        ratio: port.accepted[0] / ff.prad[0],
        decay: ff.decay,
        gain: directive_gain(ft, fp, ff.eta, port.accepted[0]),
        realized: directive_gain(ft, fp, ff.eta, port.incident[0]),
        s11_sq: port.s11[0].norm_sqr(),
    }
}

#[test]
fn l1_ntff_off_does_not_change_the_fields() {
    let mut off = grid(32, 6);
    let mut on = grid(32, 6);
    let g = dgauss(off.dt(), 200, C0 / 0.020);
    for s in [&mut off, &mut on] {
        s.add_source(
            Component::Ez,
            &[(16, 16, 16)],
            SourceKind::Soft,
            Waveform::Samples(g.clone()),
        )
        .unwrap();
    }
    on.set_ntff(&[C0 / 0.030], 2).unwrap();
    off.run(200, &mut never()).unwrap();
    on.run(200, &mut never()).unwrap();
    assert_eq!(off.field_bits(), on.field_bits());
}

#[test]
fn l1_port_media_and_pec_fields_do_not_change_when_ntff_is_on() {
    let drive = |s: &mut FdtdSim| {
        let g = dgauss(s.dt(), 40, C0 / 0.020);
        s.add_source(
            Component::Ez,
            &[(16, 16, 16)],
            SourceKind::Soft,
            Waveform::Samples(g),
        )
        .unwrap();
    };
    let mut off = grid(32, 6);
    let mut on = grid(32, 6);
    for s in [&mut off, &mut on] {
        s.add_port(
            Component::Ez,
            &[(16, 16, 16)],
            50.0,
            Waveform::Samples(dgauss(s.dt(), 40, C0 / 0.020)),
        )
        .unwrap();
    }
    on.set_ntff(&[C0 / 0.030], 2).unwrap();
    off.run(40, &mut never()).unwrap();
    on.run(40, &mut never()).unwrap();
    assert_eq!(off.field_bits(), on.field_bits(), "port");

    let mut off = grid(32, 6);
    let mut on = grid(32, 6);
    for s in [&mut off, &mut on] {
        s.set_media(
            CellMap::Uniform(2.0),
            CellMap::Uniform(1.0),
            CellMap::Uniform(0.0),
        )
        .unwrap();
        drive(s);
    }
    on.set_ntff(&[C0 / 0.030], 2).unwrap();
    off.run(40, &mut never()).unwrap();
    on.run(40, &mut never()).unwrap();
    assert_eq!(off.field_bits(), on.field_bits(), "media");

    let mut off = grid(32, 6);
    let mut on = grid(32, 6);
    let n = 32usize;
    let mut cells = vec![false; n * n * n];
    for k in 12..14 {
        for i in 12..14 {
            for j in 12..14 {
                cells[(k * n + i) * n + j] = true;
            }
        }
    }
    for s in [&mut off, &mut on] {
        s.add_pec_volume(&cells).unwrap();
        drive(s);
    }
    on.set_ntff(&[C0 / 0.030], 2).unwrap();
    off.run(40, &mut never()).unwrap();
    on.run(40, &mut never()).unwrap();
    assert_eq!(off.field_bits(), on.field_bits(), "pec");
}

#[test]
fn l2_split_runs_are_bit_identical() {
    let f = C0 / 0.030;
    let one = {
        let mut s = grid(32, 6);
        let g = dgauss(s.dt(), 300, C0 / 0.020);
        s.add_source(
            Component::Ez,
            &[(16, 16, 16)],
            SourceKind::Soft,
            Waveform::Samples(g),
        )
        .unwrap();
        s.set_ntff(&[f], 2).unwrap();
        s.run(300, &mut never()).unwrap();
        s.far_field(&[0.4, 1.2], &[0.3, 1.7], None, RecordDecay::Off)
            .unwrap()
    };
    let two = {
        let mut s = grid(32, 6);
        let g = dgauss(s.dt(), 300, C0 / 0.020);
        s.add_source(
            Component::Ez,
            &[(16, 16, 16)],
            SourceKind::Soft,
            Waveform::Samples(g),
        )
        .unwrap();
        s.set_ntff(&[f], 2).unwrap();
        s.run(130, &mut never()).unwrap();
        s.run(170, &mut never()).unwrap();
        s.far_field(&[0.4, 1.2], &[0.3, 1.7], None, RecordDecay::Off)
            .unwrap()
    };
    assert_eq!(one.e_theta, two.e_theta);
    assert_eq!(one.e_phi, two.e_phi);
    assert_eq!(one.prad, two.prad);
}

#[test]
fn l3_origin_is_a_pure_phase() {
    let run = z_dipole();
    let th = [0.7f64];
    let ph = [0.4f64];
    let o1 = [0.010, 0.012, 0.014];
    let o2 = [0.018, 0.011, 0.020];
    let a = run
        .sim
        .far_field(&th, &ph, Some(o1), RecordDecay::Pulse)
        .unwrap();
    let b = run
        .sim
        .far_field(&th, &ph, Some(o2), RecordDecay::Pulse)
        .unwrap();
    let q = idx_of(30.0);
    let rh = [
        th[0].sin() * ph[0].cos(),
        th[0].sin() * ph[0].sin(),
        th[0].cos(),
    ];
    let shift: f64 = (0..3).map(|c| rh[c] * (o2[c] - o1[c])).sum();
    let factor = Complex::from_polar(1.0, -a.k[q] * shift);
    for (got, base) in [(b.e_theta[q], a.e_theta[q]), (b.e_phi[q], a.e_phi[q])] {
        let err = (got - base * factor).norm() / base.norm().max(1e-30);
        assert!(err <= 1e-12, "origin phase err {err}");
        assert!((got.norm() - base.norm()).abs() <= 1e-12 * base.norm().max(1.0));
    }
}

#[test]
fn l4_every_face_must_be_on() {
    for face in [
        Face::XMin,
        Face::XMax,
        Face::YMin,
        Face::YMax,
        Face::ZMin,
        Face::ZMax,
    ] {
        let mut s = grid(32, 6);
        s.set_cpml_face(face, 0).unwrap();
        s.add_source(
            Component::Ez,
            &[(16, 16, 16)],
            SourceKind::Soft,
            Waveform::Samples(vec![1.0]),
        )
        .unwrap();
        s.set_ntff(&[C0 / 0.030], 2).unwrap();
        let err = s.run(1, &mut never()).unwrap_err().to_string();
        assert!(err.contains(face.name()), "{err}");
        assert!(err.contains("CPML"), "{err}");
        assert_eq!(s.steps(), 0);
    }
}

#[test]
fn l5_exterior_and_the_inner_layer_stay_homogeneous() {
    let mut bad = grid(32, 6);
    let n = 32usize;
    let mut eps = vec![1.0; n * n * n];
    eps[(16 * n + 16) * n + 8] = 2.0;
    bad.set_media(
        CellMap::Cells(eps),
        CellMap::Uniform(1.0),
        CellMap::Uniform(0.0),
    )
    .unwrap();
    bad.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(vec![1.0]),
    )
    .unwrap();
    bad.set_ntff(&[C0 / 0.030], 2).unwrap();
    let err = bad.run(1, &mut never()).unwrap_err().to_string();
    assert!(
        err.contains("homogeneous") || err.contains("material"),
        "{err}"
    );
    assert_eq!(bad.steps(), 0);

    let mut ok = grid(32, 6);
    let mut eps = vec![1.0; n * n * n];
    eps[(16 * n + 16) * n + 9] = 2.0;
    ok.set_media(
        CellMap::Cells(eps),
        CellMap::Uniform(1.0),
        CellMap::Uniform(0.0),
    )
    .unwrap();
    ok.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(vec![1.0]),
    )
    .unwrap();
    ok.set_ntff(&[C0 / 0.030], 2).unwrap();
    ok.run(1, &mut never()).unwrap();

    let mut lossy = grid(32, 6);
    lossy
        .set_media(
            CellMap::Uniform(1.0),
            CellMap::Uniform(1.0),
            CellMap::Uniform(0.1),
        )
        .unwrap();
    lossy
        .add_source(
            Component::Ez,
            &[(16, 16, 16)],
            SourceKind::Soft,
            Waveform::Samples(vec![1.0]),
        )
        .unwrap();
    lossy.set_ntff(&[C0 / 0.030], 2).unwrap();
    let err = lossy.run(1, &mut never()).unwrap_err().to_string();
    assert!(err.contains("sigma"), "{err}");
}

#[test]
fn l5_every_face_of_the_shell_is_exterior() {
    // n = 32, CPML 6, margin 2 → node planes 8 and 24. A cell on the
    // inner face of that plane (8, or 23) is the one-cell shell.
    let n = 32usize;
    let plant = |axis: &str, index: usize| {
        let mut eps = vec![1.0; n * n * n];
        let (i, j, k) = match axis {
            "x" => (16usize, index, 16usize),
            "y" => (index, 16, 16),
            _ => (16, 16, index),
        };
        eps[(k * n + i) * n + j] = 2.0;
        let mut s = grid(n, 6);
        s.set_media(
            CellMap::Cells(eps),
            CellMap::Uniform(1.0),
            CellMap::Uniform(0.0),
        )
        .unwrap();
        s.add_source(
            Component::Ez,
            &[(16, 16, 16)],
            SourceKind::Soft,
            Waveform::Samples(vec![1.0]),
        )
        .unwrap();
        s.set_ntff(&[C0 / 0.030], 2).unwrap();
        s
    };
    for axis in ["x", "y", "z"] {
        for index in [8usize, 23] {
            let mut s = plant(axis, index);
            let err = s.run(1, &mut never()).unwrap_err().to_string();
            assert!(
                err.contains("homogeneous") || err.contains("material"),
                "{axis}={index}: {err}"
            );
            assert_eq!(s.steps(), 0, "{axis}={index}");
        }
    }
    plant("z", 9).run(1, &mut never()).unwrap();
}

#[test]
fn l6_a_port_outside_the_box_is_refused() {
    let mut s = grid(32, 6);
    s.add_port(
        Component::Ez,
        &[(16, 8, 16)],
        50.0,
        Waveform::Samples(vec![1.0]),
    )
    .unwrap();
    s.set_ntff(&[C0 / 0.030], 2).unwrap();
    let err = s.run(1, &mut never()).unwrap_err().to_string();
    assert!(
        err.contains("Huygens") || err.contains("inside") || err.contains("port"),
        "{err}"
    );
    assert_eq!(s.steps(), 0);
}

#[test]
fn l6_drivers_must_sit_inside_the_box() {
    for (label, j, pec) in [
        ("face", 8usize, false),
        ("inside", 9usize, false),
        ("pec-face", 8, true),
        ("pec-in", 9, true),
    ] {
        let mut s = grid(32, 6);
        if pec {
            let n = 32usize;
            let mut cells = vec![false; n * n * n];
            cells[(16 * n + 16) * n + j] = true;
            s.add_pec_volume(&cells).unwrap();
            s.add_source(
                Component::Ez,
                &[(16, 16, 14)],
                SourceKind::Soft,
                Waveform::Samples(vec![1.0]),
            )
            .unwrap();
        } else {
            s.add_source(
                Component::Ez,
                &[(16, j, 16)],
                SourceKind::Soft,
                Waveform::Samples(vec![1.0]),
            )
            .unwrap();
        }
        s.set_ntff(&[C0 / 0.030], 2).unwrap();
        let result = s.run(1, &mut never());
        if j == 8 {
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("Huygens") || err.contains("inside"),
                "{label}: {err}"
            );
            assert_eq!(s.steps(), 0, "{label}");
        } else {
            result.expect(label);
        }
    }
}

#[test]
fn l7_resolution_and_nyquist() {
    let mut s = grid(32, 6);
    s.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(vec![1.0]),
    )
    .unwrap();
    let d = 1.0e-3;
    let coarse = C0 / (9.9 * d);
    s.set_ntff(&[coarse], 2).unwrap();
    let err = s.run(1, &mut never()).unwrap_err().to_string();
    assert!(err.contains("10") || err.contains("wavelength"), "{err}");
    assert_eq!(s.steps(), 0);

    let mut s = grid(32, 6);
    s.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(vec![1.0]),
    )
    .unwrap();
    s.set_ntff(&[C0 / (10.1 * d)], 2).unwrap();
    s.run(1, &mut never()).unwrap();

    let mut s = grid(32, 6);
    let nyq = 0.5 / s.dt();
    let err = s.set_ntff(&[nyq], 2).unwrap_err().to_string();
    assert!(err.contains("Nyquist") || err.contains("1/(2"), "{err}");
}

#[test]
fn l8_arming_lifecycle() {
    let mut s = grid(32, 6);
    s.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(vec![0.0; 4]),
    )
    .unwrap();
    s.run(1, &mut never()).unwrap();
    let err = s.set_ntff(&[C0 / 0.030], 2).unwrap_err().to_string();
    assert!(err.contains("before the first step"), "{err}");

    let mut s = grid(32, 6);
    s.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(vec![1.0]),
    )
    .unwrap();
    s.set_ntff(&[C0 / 0.030, C0 / 0.040], 2).unwrap();
    assert_eq!(s.ntff_freqs().len(), 2);
    s.set_ntff(&[C0 / 0.030], 2).unwrap();
    assert_eq!(s.ntff_freqs().len(), 1);
    let bx = s.ntff_box().unwrap();
    assert_eq!((bx.x.0, bx.x.1), (8, 24));
    s.set_cpml(8, CpmlParams::default()).unwrap();
    let bx = s.ntff_box().unwrap();
    assert_eq!((bx.x.0, bx.x.1), (10, 22));
    s.set_ntff(&[], 2).unwrap();
    assert!(s.ntff_freqs().is_empty());
    let err = s
        .far_field(&[1.0], &[0.0], None, RecordDecay::Off)
        .unwrap_err()
        .to_string();
    assert!(err.contains("fdtd_ntff") || err.contains("call"), "{err}");

    let mut s = grid(32, 6);
    s.set_ntff(&[C0 / 0.030], 2).unwrap();
    let err = s
        .far_field(&[1.0], &[0.0], None, RecordDecay::Off)
        .unwrap_err()
        .to_string();
    assert!(err.contains("run the grid"), "{err}");
    let err = s.set_ntff(&[C0 / 0.030], 0).unwrap_err().to_string();
    assert!(err.contains("margin"), "{err}");
}

#[test]
fn l9_decay_gate_quotes_the_ratio_and_off_skips_it() {
    let mut s = grid(32, 6);
    let g = dgauss(s.dt(), 700, C0 / 0.020);
    s.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(g[..60].to_vec()),
    )
    .unwrap();
    s.set_ntff(&[C0 / 0.030], 2).unwrap();
    s.run(60, &mut never()).unwrap();
    let err = s
        .far_field(&[1.0], &[0.0], None, RecordDecay::Pulse)
        .unwrap_err()
        .to_string();
    assert!(err.contains("has not decayed"), "{err}");
    assert!(err.contains("tail/peak"), "{err}");
    s.far_field(&[1.0], &[0.0], None, RecordDecay::Off).unwrap();

    let mut s = grid(32, 6);
    let tau = 50.0 * s.dt();
    s.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Pulse {
            t0: 0.0,
            tau,
            f0: C0 / 0.030,
        },
    )
    .unwrap();
    s.set_ntff(&[C0 / 0.030], 2).unwrap();
    s.run(200, &mut never()).unwrap();
    let err = s
        .far_field(&[1.0], &[0.0], None, RecordDecay::Pulse)
        .unwrap_err()
        .to_string();
    assert!(err.contains("has not decayed"), "{err}");
}

#[test]
fn l9_port_decay_is_refused_and_off_skips_it() {
    // The surface of this record has already decayed (G10). A late port
    // sample is what the port gate is for; the surface check would hide
    // it on a short record.
    let (sim, _) = port_run();
    let mut s = sim.clone();
    let n = s.port_v.len();
    let peak = s.port_v.iter().fold(0.0_f64, |p, v| p.max(v.abs()));
    s.port_v[n - 1] = peak * 10.0 + 1.0;
    let err = s
        .far_field(&[1.0], &[0.0], None, RecordDecay::Pulse)
        .unwrap_err()
        .to_string();
    assert!(err.contains("port"), "{err}");
    assert!(err.contains("has not decayed"), "{err}");
    s.far_field(&[1.0], &[0.0], None, RecordDecay::Off).unwrap();
}

#[test]
fn l10_bytes_match_the_table() {
    let mut s = grid(32, 6);
    s.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(vec![1.0; 20]),
    )
    .unwrap();
    let before = s.bytes();
    s.set_ntff(&[C0 / 0.030, C0 / 0.040], 2).unwrap();
    let armed = s.bytes();
    assert_eq!(armed - before, 16);
    s.run(20, &mut never()).unwrap();
    let ns = s.ntff_sample_count().unwrap();
    let nf = 2usize;
    let steps = 20usize;
    assert_eq!(s.bytes() - armed, ns * 48 + ns * nf * 32 + steps * 8);
}

#[test]
fn a_nonfinite_field_or_port_record_is_an_error() {
    let mut s = grid(16, 4);
    s.add_source(
        Component::Ez,
        &[(8, 8, 8)],
        SourceKind::Soft,
        Waveform::Samples(vec![1.0, 0.0, 0.0, 0.0]),
    )
    .unwrap();
    s.set_ntff(&[C0 / 0.020], 1).unwrap();
    s.run(3, &mut never()).unwrap();
    let poisoned = s.idx(8, 8, 4);
    s.ex[poisoned] = f64::NAN;
    s.run(1, &mut never()).unwrap();
    for decay in [RecordDecay::Pulse, RecordDecay::Off] {
        let err = s
            .far_field(&[1.0], &[0.0], None, decay)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not finite"), "decay {decay:?} returned {err}");
    }

    let mut s = grid(16, 4);
    let g = dgauss(s.dt(), 20, C0 / 0.020);
    s.add_port(Component::Ez, &[(8, 8, 8)], 50.0, Waveform::Samples(g))
        .unwrap();
    s.set_ntff(&[C0 / 0.020], 1).unwrap();
    s.run(8, &mut never()).unwrap();
    s.port_v[0] = f64::INFINITY;
    let err = s
        .far_field(&[1.0], &[0.0], None, RecordDecay::Off)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("port voltage") && err.contains("not finite"),
        "{err}"
    );
}

fn sim_field(sim: &FdtdSim, c: usize) -> &[f64] {
    match c {
        0 => &sim.ex,
        1 => &sim.ey,
        _ => &sim.ez,
    }
}

fn lossy_edges(cells: &[f64], n: usize) -> Vec<(usize, usize, f64)> {
    let span = [(1usize, 0usize, 1usize), (0, 1, 1), (1, 1, 0)];
    let mut out = Vec::new();
    for c in 0..3 {
        let (di, dj, dk) = span[c];
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    let mut sum = 0.0;
                    let mut cnt = 0.0;
                    for kk in k.saturating_sub(dk)..=k {
                        for ii in i.saturating_sub(di)..=i {
                            for jj in j.saturating_sub(dj)..=j {
                                sum += cells[(kk * n + ii) * n + jj];
                                cnt += 1.0;
                            }
                        }
                    }
                    let sig = sum / cnt;
                    if sig > 0.0 {
                        out.push((c, (k * n + i) * n + j, sig));
                    }
                }
            }
        }
    }
    out
}

#[test]
fn non_positive_power_is_not_a_gain() {
    let e = C64::new(1.0, 0.0);
    assert!(directive_gain(e, C64::new(0.0, 0.0), 377.0, 0.0).is_nan());
    assert!(directive_gain(e, C64::new(0.0, 0.0), 377.0, -1.0).is_nan());
    assert!(db10(0.0).is_nan());
    assert!(db10(-2.0).is_nan());
    let g = directive_gain(e, C64::new(0.0, 0.0), 2.0, 1.0);
    // U = 1/(2η) = 0.25, so 4π U / P = π.
    assert!((g - PI).abs() < 1e-12);
}

#[test]
fn run_zero_and_an_immediate_stop_do_not_resolve() {
    let mut s = grid(32, 6);
    s.set_ntff(&[C0 / 0.030], 2).unwrap();
    s.run(0, &mut never()).unwrap();
    assert_eq!(s.steps(), 0);
    assert!(s.ntff_sample_count().is_none());
    let mut s = grid(32, 6);
    s.add_source(
        Component::Ez,
        &[(16, 16, 16)],
        SourceKind::Soft,
        Waveform::Samples(vec![1.0]),
    )
    .unwrap();
    s.set_ntff(&[C0 / 0.030], 2).unwrap();
    s.run(5, &mut || true).unwrap();
    assert_eq!(s.steps(), 0);
    assert!(s.ntff_sample_count().is_none());
}

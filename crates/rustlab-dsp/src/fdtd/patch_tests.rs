//! Floating patch: finite ground and substrate in air, CPML on six faces.
//!
//! The lesson mesh (2 mm) and the reference mesh (1 mm) are the gates in
//! `dev/plans/fdtd3d_phase6_impl.md`. Both are `#[ignore]` release runs.
//! The default suite only builds a coarse grid.

use super::*;
use num_complex::Complex;
use std::f64::consts::PI;

const EPS_R: f64 = 4.4;
const H_SUB: f64 = 1.6e-3;
const F0: f64 = 2.4e9;

fn never() -> impl FnMut() -> bool {
    || false
}

#[derive(Clone)]
pub(super) struct PatchSpec {
    pub dxy: f64,
    pub nsub: usize,
    pub gpad: f64,
    pub air_xy: f64,
    pub air_z: f64,
    pub npml: usize,
    pub margin: usize,
    pub sigma: f64,
    pub nsteps: usize,
    pub freqs: Vec<f64>,
    /// `0` staircases the transmission-line design. Otherwise the metal span.
    pub lmetal: f64,
    pub wmetal: f64,
}

impl PatchSpec {
    pub(super) fn tiny() -> Self {
        Self {
            dxy: 4e-3,
            nsub: 1,
            gpad: 4e-3,
            air_xy: 8e-3,
            air_z: 3.2e-3,
            npml: 2,
            margin: 1,
            sigma: 0.0,
            nsteps: 1,
            freqs: vec![1.0e9],
            lmetal: 32e-3,
            wmetal: 32e-3,
        }
    }

    fn lesson(sigma: f64) -> Self {
        Self {
            dxy: 2e-3,
            nsub: 2,
            gpad: 16e-3,
            air_xy: 10e-3,
            air_z: 20e-3,
            npml: 8,
            margin: 2,
            sigma,
            nsteps: 16000,
            freqs: vec![2.34e9, 2.36e9, 2.38e9, 2.40e9],
            lmetal: 0.0,
            wmetal: 0.0,
        }
    }

    fn reference(sigma: f64) -> Self {
        Self {
            dxy: 1e-3,
            nsub: 4,
            gpad: 16e-3,
            air_xy: 10e-3,
            air_z: 20e-3,
            npml: 8,
            margin: 2,
            sigma,
            nsteps: 48000,
            freqs: vec![2.40e9, 2.42e9, 2.44e9, 2.46e9],
            lmetal: 0.0,
            wmetal: 0.0,
        }
    }
}

#[allow(dead_code)]
pub(super) struct Patch {
    pub sim: FdtdSim,
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
    /// 0-based feed row and column.
    pub i_feed: usize,
    pub j_feed: usize,
    pub kg: usize,
    pub nsub: usize,
    pub dx: f64,
    pub dy: f64,
    pub dz: f64,
    pub metal_x: f64,
    pub metal_y: f64,
    pub sigma_cells: Vec<f64>,
}

fn design() -> (f64, f64) {
    let w = C0 / (2.0 * F0) * (2.0 / (EPS_R + 1.0)).sqrt();
    let ee = (EPS_R + 1.0) / 2.0 + (EPS_R - 1.0) / 2.0 / (1.0 + 12.0 * H_SUB / w).sqrt();
    let dl = 0.412 * H_SUB * (ee + 0.3) * (w / H_SUB + 0.264) / ((ee - 0.258) * (w / H_SUB + 0.8));
    let l = C0 / (2.0 * F0 * ee.sqrt()) - 2.0 * dl;
    (l, w)
}

/// Staircase-aware transmission line: electrical length is the metal span
/// plus one cell, plus `2 ΔL` of the span widened by one cell.
fn tl_cell(lm: f64, wm: f64, d: f64) -> f64 {
    let w = wm + d;
    let ee = (EPS_R + 1.0) / 2.0 + (EPS_R - 1.0) / 2.0 / (1.0 + 12.0 * H_SUB / w).sqrt();
    let dl = 0.412 * H_SUB * (ee + 0.3) * (w / H_SUB + 0.264) / ((ee - 0.258) * (w / H_SUB + 0.8));
    C0 / (2.0 * (lm + d + 2.0 * dl) * ee.sqrt())
}

pub(super) fn build(spec: &PatchSpec) -> Patch {
    try_build(spec).unwrap_or_else(|e| panic!("{e}"))
}

fn try_build(spec: &PatchSpec) -> Result<Patch, DspError> {
    let (l_des, w_des) = design();
    let dx = spec.dxy;
    let dy = spec.dxy;
    let dz = H_SUB / spec.nsub as f64;
    let ngx = 2.0 * ((l_des / 2.0 + spec.gpad) / dx).round();
    let ngy = 2.0 * ((w_des / 2.0 + spec.gpad) / dy).round();
    let nax = (spec.air_xy / dx).round() as usize;
    let nay = (spec.air_xy / dy).round() as usize;
    let naz = (spec.air_z / dz).round() as usize;
    let nx = ngx as usize + 2 * (spec.npml + spec.margin + nax);
    let ny = ngy as usize + 2 * (spec.npml + spec.margin + nay);
    let nz = spec.nsub + 2 * (spec.npml + spec.margin + naz);
    let jg0 = spec.npml + spec.margin + nax;
    let ig0 = spec.npml + spec.margin + nay;
    let kg = spec.npml + spec.margin + naz;
    let mut sim = FdtdSim::new(nx, ny, nz, dx, dy, dz, DEFAULT_COURANT)?;
    let mut eps = vec![1.0; nx * ny * nz];
    let mut sig = vec![0.0; nx * ny * nz];
    for k in kg..kg + spec.nsub {
        for i in ig0..ig0 + ngy as usize {
            for j in jg0..jg0 + ngx as usize {
                let c = (k * ny + i) * nx + j;
                eps[c] = EPS_R;
                sig[c] = spec.sigma;
            }
        }
    }
    sim.set_media(
        CellMap::Cells(eps),
        CellMap::Uniform(1.0),
        CellMap::Cells(sig.clone()),
    )?;
    sim.set_cpml(spec.npml, CpmlParams::default())?;

    let mut ground = vec![false; ny * nx];
    for i in ig0..=ig0 + ngy as usize {
        for j in jg0..=jg0 + ngx as usize {
            ground[i * nx + j] = true;
        }
    }
    sim.add_pec_face(Axis::Z, kg, &ground)?;

    let mut patch = vec![false; ny * nx];
    if spec.lmetal > 0.0 {
        let nl = (spec.lmetal / dx).round() as usize;
        let nw = (spec.wmetal / dy).round() as usize;
        let j0 = (nx as f64 / 2.0 - nl as f64 / 2.0).round() as usize;
        let i0 = (ny as f64 / 2.0 - nw as f64 / 2.0).round() as usize;
        for i in i0..=i0 + nw {
            for j in j0..=j0 + nl {
                patch[i * nx + j] = true;
            }
        }
    } else {
        let px = nx as f64 / 2.0 * dx - l_des / 2.0;
        let py = ny as f64 / 2.0 * dy - w_des / 2.0;
        for i in 0..ny {
            for j in 0..nx {
                let xv = j as f64 * dx;
                let yv = i as f64 * dy;
                if xv >= px && xv <= px + l_des && yv >= py && yv <= py + w_des {
                    patch[i * nx + j] = true;
                }
            }
        }
    }
    sim.add_pec_face(Axis::Z, kg + spec.nsub, &patch)?;

    let i_feed = (ny as f64 / 2.0).round() as usize;
    let j_feed = (0..nx).find(|&j| patch[i_feed * nx + j]).unwrap();
    let mut j_last = j_feed;
    let mut i_first = i_feed;
    let mut i_last = i_feed;
    for j in 0..nx {
        if patch[i_feed * nx + j] {
            j_last = j;
        }
    }
    for i in 0..ny {
        if patch[i * nx + j_feed] {
            i_first = i_first.min(i);
            i_last = i_last.max(i);
        }
    }
    let nodes: Vec<(usize, usize, usize)> =
        (kg..kg + spec.nsub).map(|k| (i_feed, j_feed, k)).collect();
    let tau = 1.0 / (2.0 * F0);
    sim.add_port(
        Component::Ez,
        &nodes,
        50.0,
        Waveform::Pulse {
            t0: 4.0 * tau,
            tau,
            f0: F0,
        },
    )?;
    if !spec.freqs.is_empty() {
        sim.set_ntff(&spec.freqs, spec.margin)?;
    }
    Ok(Patch {
        sim,
        nx,
        ny,
        nz,
        i_feed,
        j_feed,
        kg,
        nsub: spec.nsub,
        dx,
        dy,
        dz,
        metal_x: (j_last - j_feed) as f64 * dx,
        metal_y: (i_last - i_first) as f64 * dy,
        sigma_cells: sig,
    })
}

struct Cut {
    dip_hz: f64,
    min_s11: f64,
    max_s11: f64,
    tail_v: f64,
    tail_i: f64,
}

fn s11_cut(v: &[f64], i: &[f64], dt: f64) -> Cut {
    let n = v.len();
    let s = s11_with(
        v,
        i,
        dt,
        50.0,
        S11Spec {
            window: PortWindow::Rect,
            nfft: Some(8 * n),
            decay: RecordDecay::Off,
        },
    )
    .unwrap();
    let mut dip_hz = 0.0;
    let mut min_s11 = f64::MAX;
    let mut max_s11: f64 = 0.0;
    for (f, z) in s.frequencies.iter().zip(s.s11.iter()) {
        let a = z.norm();
        if (1.8e9..3.2e9).contains(f) && a < min_s11 {
            min_s11 = a;
            dip_hz = *f;
        }
        if (1.5e9..3.5e9).contains(f) {
            max_s11 = max_s11.max(a);
        }
    }
    Cut {
        dip_hz,
        min_s11,
        max_s11,
        tail_v: super::port::tail_over_peak(v),
        tail_i: super::port::tail_over_peak(i),
    }
}

fn sphere(ff: &FarField, q: usize) -> f64 {
    let nt = ff.theta.len();
    let np = ff.phi.len();
    let dth = if nt > 1 {
        ff.theta[1] - ff.theta[0]
    } else {
        0.0
    };
    let dph = if np > 1 { ff.phi[1] - ff.phi[0] } else { 0.0 };
    let mut sum = 0.0;
    for a in 0..nt {
        let mut w = dth;
        if a == 0 || a + 1 == nt {
            w *= 0.5;
        }
        let s = ff.theta[a].sin();
        for b in 0..np {
            let idx = (q * nt + a) * np + b;
            let u = (ff.e_theta[idx].norm_sqr() + ff.e_phi[idx].norm_sqr()) / (2.0 * ff.eta);
            sum += u * s * w * dph;
        }
    }
    sum
}

fn angles() -> (Vec<f64>, Vec<f64>) {
    let th: Vec<f64> = (0..=60).map(|k| k as f64 * 3.0 * PI / 180.0).collect();
    let ph: Vec<f64> = (0..120).map(|k| k as f64 * 3.0 * PI / 180.0).collect();
    (th, ph)
}

struct Pattern {
    q: usize,
    d0: f64,
    g0: f64,
    gr0: f64,
    balance: f64,
    psph: f64,
    front_back_db: f64,
    cross: f64,
}

fn pattern_at(sim: &FdtdSim, dip_hz: f64, decay: RecordDecay) -> Pattern {
    let (th, ph) = angles();
    let ff = sim.far_field(&th, &ph, None, decay).unwrap();
    let q = ff
        .freqs
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (*a - dip_hz)
                .abs()
                .partial_cmp(&(*b - dip_hz).abs())
                .unwrap()
        })
        .unwrap()
        .0;
    let nt = ff.theta.len();
    let np = ff.phi.len();
    let at = |a: usize, b: usize| (q * nt + a) * np + b;
    let port = ff.port.as_ref().unwrap();
    let d = |a, b| directive_gain(ff.e_theta[at(a, b)], ff.e_phi[at(a, b)], ff.eta, ff.prad[q]);
    let g = |a, b| {
        directive_gain(
            ff.e_theta[at(a, b)],
            ff.e_phi[at(a, b)],
            ff.eta,
            port.accepted[q],
        )
    };
    let gr = |a, b| {
        directive_gain(
            ff.e_theta[at(a, b)],
            ff.e_phi[at(a, b)],
            ff.eta,
            port.incident[q],
        )
    };
    let mut best = 0.0;
    let mut best_at = (0, 0);
    for a in 0..nt {
        for b in 0..np {
            let v = d(a, b);
            if v > best {
                best = v;
                best_at = (a, b);
            }
        }
    }
    assert_eq!(best_at.0, 0, "broadside is not the directivity peak");
    let e_co = (0..nt)
        .map(|a| ff.e_theta[at(a, 0)].norm())
        .enumerate()
        .max_by(|x, y| x.1.partial_cmp(&y.1).unwrap())
        .unwrap()
        .0;
    let h_co = (0..nt)
        .map(|a| ff.e_phi[at(a, 30)].norm())
        .enumerate()
        .max_by(|x, y| x.1.partial_cmp(&y.1).unwrap())
        .unwrap()
        .0;
    assert_eq!(e_co, 0, "E-plane peak left theta = 0");
    assert_eq!(h_co, 0, "H-plane peak left theta = 0");
    let cross = ff.e_phi[at(0, 0)].norm() / ff.e_theta[at(0, 0)].norm();
    for a in 0..nt {
        let d1 = d(a, 30);
        let d2 = d(a, 90);
        let scale = d1.abs().max(1e-30);
        assert!(
            (d1 - d2).abs() / scale <= 1e-6,
            "H-plane symmetry {} at theta {}",
            (d1 - d2).abs() / scale,
            a
        );
    }
    for qf in 0..ff.freqs.len() {
        let bal = (port.accepted[qf] - ff.prad[qf]).abs() / port.accepted[qf];
        assert!(
            bal <= 1e-2,
            "power balance {bal} at {} GHz",
            ff.freqs[qf] / 1e9
        );
        let ds = db10(directive_gain(
            ff.e_theta[qf * nt * np],
            ff.e_phi[qf * nt * np],
            ff.eta,
            ff.prad[qf],
        ));
        let gs = db10(directive_gain(
            ff.e_theta[qf * nt * np],
            ff.e_phi[qf * nt * np],
            ff.eta,
            port.accepted[qf],
        ));
        assert!((gs - ds).abs() <= 0.05, "G - D = {} dB", gs - ds);
        let s = port.s11[qf].norm();
        let grs = db10(directive_gain(
            ff.e_theta[qf * nt * np],
            ff.e_phi[qf * nt * np],
            ff.eta,
            port.incident[qf],
        ));
        let expect = gs + db10(1.0 - s * s);
        assert!(
            (grs - expect).abs() <= 1e-6,
            "realized gain identity {} vs {}",
            grs,
            expect
        );
    }
    Pattern {
        q,
        d0: d(0, 0),
        g0: g(0, 0),
        gr0: gr(0, 0),
        balance: (port.accepted[q] - ff.prad[q]).abs() / port.accepted[q],
        psph: sphere(&ff, q) / ff.prad[q],
        front_back_db: db10(d(0, 0) / d(nt - 1, 0)),
        cross,
    }
}

fn assert_capstone(name: &str, spec: &PatchSpec, phase4: Option<f64>) {
    let mut p = build(spec);
    assert!(p.metal_y / p.dy % 2.0 == 0.0 || (p.metal_y / p.dy).round() % 2.0 == 0.0);
    eprintln!(
        "{name}: {} x {} x {}  metal {:.1} x {:.1} mm  feed j={}",
        p.nx,
        p.ny,
        p.nz,
        p.metal_x * 1e3,
        p.metal_y * 1e3,
        p.j_feed
    );
    p.sim.run(spec.nsteps, &mut never()).unwrap();
    let v = p.sim.port_voltage().to_vec();
    let i = p.sim.port_current().to_vec();
    let full = s11_cut(&v, &i, p.sim.dt());
    eprintln!(
        "{name}: dip {:.4} GHz  |S11| {:.3}  max {:.4}  tail {:.2e}",
        full.dip_hz / 1e9,
        full.min_s11,
        full.max_s11,
        full.tail_v
    );
    assert!(full.tail_v <= 1e-3 && full.tail_i <= 1e-3, "decay");
    assert!(full.max_s11 <= 1.0, "passivity {}", full.max_s11);
    assert!(
        (full.dip_hz - F0).abs() / F0 <= 0.05,
        "dip {:.4} GHz vs 2.4",
        full.dip_hz / 1e9
    );
    let model = tl_cell(p.metal_x, p.metal_y, p.dx);
    let err = (full.dip_hz - model).abs() / model;
    eprintln!(
        "{name}: TL+cell {:.4} GHz  err {:.3}%",
        model / 1e9,
        err * 100.0
    );
    assert!(err <= 0.015, "TL+cell {err}");
    if let Some(f4) = phase4 {
        let e4 = (full.dip_hz - f4).abs() / f4;
        assert!(e4 <= 0.01, "Phase 4 {e4}");
    }
    let n4 = 4000.min(spec.nsteps);
    let early = s11_cut(&v[..n4], &i[..n4], p.sim.dt());
    if name == "reference" {
        assert!(
            early.max_s11 > 1.0,
            "truncated reference record was passive ({})",
            early.max_s11
        );
        let refused = s11_with(&v[..n4], &i[..n4], p.sim.dt(), 50.0, S11Spec::pulse());
        assert!(refused.is_err(), "fdtd_s11 accepted a ringing record");
    }
    let pat = pattern_at(&p.sim, full.dip_hz, RecordDecay::Pulse);
    eprintln!(
        "{name}: q {}  D {:.2} dBi  G {:.2}  Gr {:.2}  F/B {:.1} dB  Psph {:.4}  balance {:.2e}  cross {:.2e}",
        pat.q,
        db10(pat.d0),
        db10(pat.g0),
        db10(pat.gr0),
        pat.front_back_db,
        pat.psph,
        pat.balance,
        pat.cross
    );
    assert!((6.0..=8.0).contains(&db10(pat.d0)), "D {}", db10(pat.d0));
    assert!(
        pat.front_back_db >= 10.0,
        "front/back {}",
        pat.front_back_db
    );
    assert!((pat.psph - 1.0).abs() <= 5e-3, "Psph {}", pat.psph);
    assert!(pat.cross <= 1e-6, "cross-pol {}", pat.cross);
}

#[test]
fn floating_patch_box_passes_ntff_validation() {
    let spec = PatchSpec::tiny();
    let p = build(&spec);
    assert!(p.nx * p.ny * p.nz < 20_000, "{} cells", p.nx * p.ny * p.nz);
    let mut sim = p.sim;
    sim.run(1, &mut never()).unwrap();
    assert!(sim.ntff_sample_count().unwrap() > 0);
}

#[test]
fn ground_on_the_box_is_refused() {
    let mut p = build(&PatchSpec::tiny());
    let mut face = vec![false; p.ny * p.nx];
    let i = p.ny / 2;
    let j = p.nx / 2;
    for di in 0..2 {
        for dj in 0..2 {
            face[(i + di) * p.nx + (j + dj)] = true;
        }
    }
    // zmin Huygens node plane is npml + margin.
    p.sim.add_pec_face(Axis::Z, 3, &face).unwrap();
    let err = p.sim.run(1, &mut never()).unwrap_err().to_string();
    assert!(err.contains("not strictly inside"), "{err}");
}

#[test]
fn patch_feed_on_mirror_plane_has_no_cross_pol() {
    let mut spec = PatchSpec::tiny();
    spec.nsteps = 400;
    spec.freqs = vec![1.0e9];
    let mut p = build(&spec);
    p.sim.run(400, &mut never()).unwrap();
    let ff = p
        .sim
        .far_field(&[0.0], &[0.0], None, RecordDecay::Off)
        .unwrap();
    let eth = ff.e_theta[0].norm();
    let eph = ff.e_phi[0].norm();
    assert!(eth > 0.0, "no broadside field after 400 steps");
    assert!(eph / eth <= 1e-6, "cross-pol {}", eph / eth);
}

#[test]
#[ignore = "release: cargo test --release -p rustlab-dsp --lib -- --ignored lesson_patch_gates"]
fn lesson_patch_gates() {
    assert_capstone("lesson", &PatchSpec::lesson(0.0), None);
}

#[test]
#[ignore = "release: cargo test --release -p rustlab-dsp --lib -- --ignored reference_patch_against_phase4"]
fn reference_patch_against_phase4() {
    assert_capstone("reference", &PatchSpec::reference(0.0), Some(2.4395e9));
}

#[test]
#[ignore = "release: cargo test --release -p rustlab-dsp --lib -- --ignored odd_width_patch_rings"]
fn odd_width_patch_rings_in_the_width_mode() {
    let mut odd = PatchSpec::lesson(0.0);
    odd.nsteps = 8000;
    odd.freqs.clear();
    odd.lmetal = 28e-3;
    odd.wmetal = 38e-3;
    let mut even = odd.clone();
    even.wmetal = 40e-3;
    let mut a = build(&odd);
    let mut b = build(&even);
    a.sim.run(odd.nsteps, &mut never()).unwrap();
    b.sim.run(even.nsteps, &mut never()).unwrap();
    let ta = super::port::tail_over_peak(a.sim.port_voltage());
    let tb = super::port::tail_over_peak(b.sim.port_voltage());
    eprintln!("odd tail {ta:.3e}  even tail {tb:.3e}");
    assert!(ta > 1e-3, "odd width decayed ({ta})");
    assert!(tb < 1e-4, "even width did not decay ({tb})");
}

/// Known limitation: 10 mm of vertical air leaves the z faces in the reactive
/// field and the pattern integral overshoots the flux. Not a requirement.
#[test]
#[ignore = "release: cargo test --release -p rustlab-dsp --lib -- --ignored air_z_10mm"]
fn air_z_10mm_pattern_integral_overshoots() {
    let mut spec = PatchSpec::reference(0.0);
    spec.air_z = 10e-3;
    spec.air_xy = 10e-3;
    spec.nsteps = 12000;
    let mut p = build(&spec);
    p.sim.run(spec.nsteps, &mut never()).unwrap();
    let v = p.sim.port_voltage().to_vec();
    let i = p.sim.port_current().to_vec();
    let cut = s11_cut(&v, &i, p.sim.dt());
    let pat = pattern_at(&p.sim, cut.dip_hz, RecordDecay::Off);
    eprintln!("air_z 10 mm Psph/Pflux = {:.4}", pat.psph);
    assert!(
        pat.psph - 1.0 > 1e-2,
        "the 10 mm overshoot went away ({})",
        pat.psph - 1.0
    );
}

fn lossy_edges(cells: &[f64], nx: usize, ny: usize, nz: usize) -> Vec<(Component, usize, f64)> {
    let span = [
        (Component::Ex, 1usize, 0usize, 1usize),
        (Component::Ey, 0, 1, 1),
        (Component::Ez, 1, 1, 0),
    ];
    let mut out = Vec::new();
    for (comp, di, dj, dk) in span {
        for k in 0..nz {
            for i in 0..ny {
                for j in 0..nx {
                    let mut sum = 0.0;
                    let mut cnt = 0.0;
                    for kk in k.saturating_sub(dk)..=k {
                        if kk >= nz {
                            continue;
                        }
                        for ii in i.saturating_sub(di)..=i {
                            if ii >= ny {
                                continue;
                            }
                            for jj in j.saturating_sub(dj)..=j {
                                if jj >= nx {
                                    continue;
                                }
                                sum += cells[(kk * ny + ii) * nx + jj];
                                cnt += 1.0;
                            }
                        }
                    }
                    let sig = sum / cnt;
                    if sig > 0.0 {
                        out.push((comp, (k * ny + i) * nx + j, sig));
                    }
                }
            }
        }
    }
    out
}

fn field<'a>(sim: &'a FdtdSim, c: Component) -> &'a [f64] {
    match c {
        Component::Ex => &sim.ex,
        Component::Ey => &sim.ey,
        Component::Ez => &sim.ez,
        _ => unreachable!(),
    }
}

#[test]
#[ignore = "release: cargo test --release -p rustlab-dsp --lib -- --ignored substrate_loss"]
fn substrate_loss_balances_accepted_minus_flux() {
    let spec = PatchSpec::lesson(2.0 * PI * F0 * EPS_R * EPS0 * 0.02);
    let lossless = PatchSpec::lesson(0.0);
    let mut bare = build(&lossless);
    let mut lossy = build(&spec);
    let edges = lossy_edges(&lossy.sigma_cells, lossy.nx, lossy.ny, lossy.nz);
    let dt = lossy.sim.dt();
    let mut end = vec![Complex::new(0.0, 0.0); edges.len()];
    let q_near = spec
        .freqs
        .iter()
        .enumerate()
        .min_by(|a, b| {
            (*a.1 - 2.38e9)
                .abs()
                .partial_cmp(&(*b.1 - 2.38e9).abs())
                .unwrap()
        })
        .unwrap()
        .0;
    let w = 2.0 * PI * spec.freqs[q_near];
    let dv = lossy.dx * lossy.dy * lossy.dz;
    for _ in 0..spec.nsteps {
        lossy.sim.run(1, &mut never()).unwrap();
        let s = lossy.sim.steps() as f64;
        let re = Complex::from_polar(dt, -w * s * dt);
        for (p, &(comp, idx, _)) in edges.iter().enumerate() {
            let en = field(&lossy.sim, comp)[idx];
            end[p] += re * en;
        }
    }
    let diss = 0.5
        * edges
            .iter()
            .zip(end.iter())
            .map(|((_, _, sig), e)| sig * e.norm_sqr())
            .sum::<f64>()
        * dv;
    let (th, ph) = angles();
    let ff = lossy
        .sim
        .far_field(&th, &ph, None, RecordDecay::Pulse)
        .unwrap();
    let portp = ff.port.as_ref().unwrap();
    let gap = portp.accepted[q_near] - ff.prad[q_near];
    let rel = diss / gap - 1.0;
    let eta = ff.prad[q_near] / portp.accepted[q_near];
    eprintln!(
        "lossy eta {eta:.3}  diss/gap - 1 = {rel:.3e}  edges {}",
        edges.len()
    );
    assert!((0.40..=0.60).contains(&eta), "eta {eta}");
    assert!(rel.abs() <= 1e-2, "dissipation balance {rel}");

    bare.sim.run(lossless.nsteps, &mut never()).unwrap();
    let vb = bare.sim.port_voltage().to_vec();
    let ib = bare.sim.port_current().to_vec();
    let vl = lossy.sim.port_voltage().to_vec();
    let il = lossy.sim.port_current().to_vec();
    let cb = s11_cut(&vb, &ib, bare.sim.dt());
    let cl = s11_cut(&vl, &il, lossy.sim.dt());
    assert!((cl.dip_hz - cb.dip_hz).abs() / cb.dip_hz <= 0.005);
    assert!(cl.min_s11 < cb.min_s11);
    let d_l = db10(directive_gain(
        ff.e_theta[q_near * th.len() * ph.len()],
        ff.e_phi[q_near * th.len() * ph.len()],
        ff.eta,
        ff.prad[q_near],
    ));
    let ffb = bare
        .sim
        .far_field(&th, &ph, None, RecordDecay::Pulse)
        .unwrap();
    let qb = ffb
        .freqs
        .iter()
        .enumerate()
        .min_by(|a, b| {
            (*a.1 - cb.dip_hz)
                .abs()
                .partial_cmp(&(*b.1 - cb.dip_hz).abs())
                .unwrap()
        })
        .unwrap()
        .0;
    let d_b = db10(directive_gain(
        ffb.e_theta[qb * th.len() * ph.len()],
        ffb.e_phi[qb * th.len() * ph.len()],
        ffb.eta,
        ffb.prad[qb],
    ));
    eprintln!(
        "dip lossless {:.4} GHz  lossy {:.4} GHz  |S11| {:.3} -> {:.3}",
        cb.dip_hz / 1e9,
        cl.dip_hz / 1e9,
        cb.min_s11,
        cl.min_s11
    );
    eprintln!("D lossless {d_b:.3}  D lossy {d_l:.3}");
    assert!((d_l - d_b).abs() <= 0.05, "D moved with loss");
}

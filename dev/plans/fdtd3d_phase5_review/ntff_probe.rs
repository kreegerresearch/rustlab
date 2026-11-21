//! Reviewer measurement for the Phase 5 brief. Not part of the workspace
//! build and not the runtime implementation.
//!
//! An independent near-to-far transform written against the public
//! `FdtdSim` API (`add_source`, `set_cpml`, `run`, `slice`). It copies the
//! fields out every step, so it is slow and only for small grids. It exists to
//! measure what the analytic gates in `dev/plans/fdtd3d_phase5_impl.md` can
//! actually hold on this Yee grid before the runtime is written.
//!
//! Run from the repo root:
//!
//! ```text
//! cp dev/plans/fdtd3d_phase5_review/ntff_probe.rs crates/rustlab-dsp/examples/
//! LIBRARY_PATH=/usr/lib/gcc/x86_64-linux-gnu/13 \
//!   cargo +stable run --locked --release -p rustlab-dsp --example ntff_probe
//! rm crates/rustlab-dsp/examples/ntff_probe.rs
//! ```
//!
//! `NTFF_N` / `NTFF_PML` set the grid and CPML thickness (default 40 / 8;
//! the brief's Grid A is 32 / 6). `NTFF_CASES` is a comma list of case-name
//! prefixes, or `half-wave` for the wire dipole of brief §3.4.
//! `NTFF_MUTATION` plants one bug for the negative controls of brief §3.3:
//! `exp_sign`, `l_sign`, `h_time`, `src_time`, `h_one_plane`, `no_trapezoid`.
//!
//! Conventions (the brief's §2): phasor `X(f) = Σ x(t_s) e^{−j2πf t_s} dt`,
//! E at `t = s·dt`, H at `t = (s − ½)·dt`. Huygens faces lie on E node
//! planes; tangential H is the mean of the two half-cell neighbours. Outward
//! `n̂`: `J = n̂ × H`, `M = −n̂ × E`. Returned pattern is
//! `F = r e^{jkr} E`: `Fθ = −jk/(4π)(Lφ + η Nθ)`, `Fφ = jk/(4π)(Lθ − η Nφ)`.

use num_complex::Complex;
use rustlab_dsp::fdtd::{
    Axis, Component, CpmlParams, FdtdSim, SourceKind, Waveform, C0, EPS0, MU0,
};
use std::f64::consts::PI;

type C = Complex<f64>;

#[derive(Clone, Copy)]
struct Sample {
    e: Component,
    e_at: (usize, usize, usize),
    h: Component,
    h_a: (usize, usize, usize),
    h_b: (usize, usize, usize),
    r: [f64; 3],
    w: f64,
    /// J = sj * H, along the E component's axis.
    sj: f64,
    /// M = sm * E, along `m_axis`.
    sm: f64,
    m_axis: usize,
}

fn axis_of(c: Component) -> usize {
    match c {
        Component::Ex | Component::Hx => 0,
        Component::Ey | Component::Hy => 1,
        _ => 2,
    }
}

/// Samples of the box with node planes `lo..=hi` on every axis.
fn surface(lo: [usize; 3], hi: [usize; 3], d: [f64; 3]) -> Vec<Sample> {
    let mut out = Vec::new();
    let wt = |m: usize, a: usize, b: usize| if m == a || m == b { 0.5 } else { 1.0 };
    // (i, j, k) = (y, x, z) indices; position x=(j[+½])dx, y=(i[+½])dy, z=(k[+½])dz.
    for (s, kz) in [(-1.0, lo[2]), (1.0, hi[2])] {
        // Ex at (j+½, i, kz) with Hy; Jx = −s Hy, My = −s Ex.
        for i in lo[1]..=hi[1] {
            for j in lo[0]..hi[0] {
                out.push(Sample {
                    e: Component::Ex,
                    e_at: (i, j, kz),
                    h: Component::Hy,
                    h_a: (i, j, kz - 1),
                    h_b: (i, j, kz),
                    r: [(j as f64 + 0.5) * d[0], i as f64 * d[1], kz as f64 * d[2]],
                    w: d[0] * d[1] * wt(i, lo[1], hi[1]),
                    sj: -s,
                    sm: -s,
                    m_axis: 1,
                });
            }
        }
        // Ey at (j, i+½, kz) with Hx; Jy = s Hx, Mx = s Ey.
        for i in lo[1]..hi[1] {
            for j in lo[0]..=hi[0] {
                out.push(Sample {
                    e: Component::Ey,
                    e_at: (i, j, kz),
                    h: Component::Hx,
                    h_a: (i, j, kz - 1),
                    h_b: (i, j, kz),
                    r: [j as f64 * d[0], (i as f64 + 0.5) * d[1], kz as f64 * d[2]],
                    w: d[0] * d[1] * wt(j, lo[0], hi[0]),
                    sj: s,
                    sm: s,
                    m_axis: 0,
                });
            }
        }
    }
    for (s, jx) in [(-1.0, lo[0]), (1.0, hi[0])] {
        // Ey at (jx, i+½, k) with Hz; Jy = −s Hz, Mz = −s Ey.
        for k in lo[2]..=hi[2] {
            for i in lo[1]..hi[1] {
                out.push(Sample {
                    e: Component::Ey,
                    e_at: (i, jx, k),
                    h: Component::Hz,
                    h_a: (i, jx - 1, k),
                    h_b: (i, jx, k),
                    r: [jx as f64 * d[0], (i as f64 + 0.5) * d[1], k as f64 * d[2]],
                    w: d[1] * d[2] * wt(k, lo[2], hi[2]),
                    sj: -s,
                    sm: -s,
                    m_axis: 2,
                });
            }
        }
        // Ez at (jx, i, k+½) with Hy; Jz = s Hy, My = s Ez.
        for k in lo[2]..hi[2] {
            for i in lo[1]..=hi[1] {
                out.push(Sample {
                    e: Component::Ez,
                    e_at: (i, jx, k),
                    h: Component::Hy,
                    h_a: (i, jx - 1, k),
                    h_b: (i, jx, k),
                    r: [jx as f64 * d[0], i as f64 * d[1], (k as f64 + 0.5) * d[2]],
                    w: d[1] * d[2] * wt(i, lo[1], hi[1]),
                    sj: s,
                    sm: s,
                    m_axis: 1,
                });
            }
        }
    }
    for (s, iy) in [(-1.0, lo[1]), (1.0, hi[1])] {
        // Ex at (j+½, iy, k) with Hz; Jx = s Hz, Mz = s Ex.
        for k in lo[2]..=hi[2] {
            for j in lo[0]..hi[0] {
                out.push(Sample {
                    e: Component::Ex,
                    e_at: (iy, j, k),
                    h: Component::Hz,
                    h_a: (iy - 1, j, k),
                    h_b: (iy, j, k),
                    r: [(j as f64 + 0.5) * d[0], iy as f64 * d[1], k as f64 * d[2]],
                    w: d[0] * d[2] * wt(k, lo[2], hi[2]),
                    sj: s,
                    sm: s,
                    m_axis: 2,
                });
            }
        }
        // Ez at (j, iy, k+½) with Hx; Jz = −s Hx, Mx = −s Ez.
        for k in lo[2]..hi[2] {
            for j in lo[0]..=hi[0] {
                out.push(Sample {
                    e: Component::Ez,
                    e_at: (iy, j, k),
                    h: Component::Hx,
                    h_a: (iy - 1, j, k),
                    h_b: (iy, j, k),
                    r: [j as f64 * d[0], iy as f64 * d[1], (k as f64 + 0.5) * d[2]],
                    w: d[0] * d[2] * wt(j, lo[0], hi[0]),
                    sj: -s,
                    sm: -s,
                    m_axis: 0,
                });
            }
        }
    }
    out
}

/// Planted bug for the negative controls (`NTFF_MUTATION=...`).
fn mutation() -> String {
    std::env::var("NTFF_MUTATION").unwrap_or_default()
}

struct Fields {
    nx: usize,
    ny: usize,
    f: [Vec<f64>; 6],
}

impl Fields {
    fn grab(sim: &FdtdSim) -> Fields {
        let comps = [
            Component::Ex,
            Component::Ey,
            Component::Ez,
            Component::Hx,
            Component::Hy,
            Component::Hz,
        ];
        let f = comps.map(|c| {
            let mut v = Vec::with_capacity(sim.n_cells());
            for k in 0..sim.nz() {
                v.extend(sim.slice(c, Axis::Z, k).unwrap().2);
            }
            v
        });
        Fields {
            nx: sim.nx(),
            ny: sim.ny(),
            f,
        }
    }
    fn at(&self, c: Component, (i, j, k): (usize, usize, usize)) -> f64 {
        let slot = match c {
            Component::Ex => 0,
            Component::Ey => 1,
            Component::Ez => 2,
            Component::Hx => 3,
            Component::Hy => 4,
            Component::Hz => 5,
        };
        self.f[slot][(k * self.ny + i) * self.nx + j]
    }
}

struct Case {
    n: usize,
    npml: usize,
    margin: usize,
    comp: Component,
    src: (usize, usize, usize),
    steps: usize,
    freqs: Vec<f64>,
    port: bool,
    /// PEC wire arms of this many cells above and below a z port edge.
    arms: usize,
}

struct Out {
    dt: f64,
    d: [f64; 3],
    samples: Vec<Sample>,
    /// [freq][sample] (E phasor, mean-H phasor).
    e: Vec<Vec<C>>,
    h: Vec<Vec<C>>,
    il: Vec<C>,
    tail: f64,
    /// Port (V, I) phasors at true sample times, when the case drives a port.
    vi: Vec<(C, C)>,
}

fn run(case: &Case) -> Out {
    let dx = 1e-3;
    let d = [dx, dx, dx];
    let mut sim = FdtdSim::new(case.n, case.n, case.n, dx, dx, dx, 0.99).unwrap();
    sim.set_cpml(case.npml, CpmlParams::default()).unwrap();
    let dt = sim.dt();
    let f_c = case.freqs[case.freqs.len() / 2];
    let tau = 2f64.sqrt() / (2.0 * PI * f_c);
    let t0 = 5.0 * tau;
    let g: Vec<f64> = (1..=case.steps)
        .map(|s| {
            let u = (s as f64 * dt - t0) / tau;
            -u * (-u * u).exp()
        })
        .collect();
    if case.port {
        sim.add_port(case.comp, &[case.src], 50.0, Waveform::Samples(g.clone()))
            .unwrap();
        let (i0, j0, k0) = case.src;
        for ks in [k0 - case.arms..=k0, k0 + 1..=k0 + 1 + case.arms] {
            if case.arms == 0 {
                break;
            }
            let mut mask = vec![false; case.n * case.n];
            for k in ks {
                mask[i0 * case.n + k] = true;
            }
            sim.add_pec_face(Axis::X, j0, &mask).unwrap();
        }
    } else {
        sim.add_source(
            case.comp,
            &[case.src],
            SourceKind::Soft,
            Waveform::Samples(g.clone()),
        )
        .unwrap();
    }
    let lo = case.npml + case.margin;
    let hi = case.n - case.npml - case.margin;
    let samples = surface([lo; 3], [hi; 3], d);
    let nf = case.freqs.len();
    let mut e = vec![vec![C::new(0.0, 0.0); samples.len()]; nf];
    let mut h = vec![vec![C::new(0.0, 0.0); samples.len()]; nf];
    let mut peak = 0.0f64;
    let mut late = 0.0f64;
    for s in 1..=case.steps {
        sim.run(1, &mut || false).unwrap();
        let fl = Fields::grab(&sim);
        let te = s as f64 * dt;
        let th = if mutation() == "h_time" {
            s as f64 * dt
        } else {
            (s as f64 - 0.5) * dt
        };
        let mut emax = 0.0f64;
        for (q, f) in case.freqs.iter().enumerate() {
            let w = 2.0 * PI * f;
            let re = C::from_polar(dt, -w * te);
            let rh = C::from_polar(dt, -w * th);
            for (p, sm) in samples.iter().enumerate() {
                let ev = fl.at(sm.e, sm.e_at);
                let hv = if mutation() == "h_one_plane" {
                    fl.at(sm.h, sm.h_b)
                } else {
                    0.5 * (fl.at(sm.h, sm.h_a) + fl.at(sm.h, sm.h_b))
                };
                e[q][p] += re * ev;
                h[q][p] += rh * hv;
                if q == 0 {
                    emax = emax.max(ev.abs());
                }
            }
        }
        peak = peak.max(emax);
        if s > case.steps - case.steps / 10 {
            late = late.max(emax);
        }
    }
    let area = match case.comp {
        Component::Ex => d[1] * d[2],
        Component::Ey => d[0] * d[2],
        _ => d[0] * d[1],
    };
    // Soft source sampled at s·dt acts as J at (s − ½)·dt (see the brief §2.4).
    let il = case
        .freqs
        .iter()
        .map(|f| {
            let w = 2.0 * PI * f;
            g.iter()
                .enumerate()
                .map(|(n, &x)| {
                    let half = if mutation() == "src_time" { 0.0 } else { 0.5 };
                    C::from_polar(dt * x * area, -w * ((n + 1) as f64 - half) * dt)
                })
                .sum()
        })
        .collect();
    let pv = sim.port_voltage();
    let pi = sim.port_current();
    let vi = if case.port {
        case.freqs
            .iter()
            .map(|f| {
                let w = 2.0 * PI * f;
                let mut v = C::new(0.0, 0.0);
                let mut i = C::new(0.0, 0.0);
                for n in 0..pv.len() {
                    let s = (n + 1) as f64;
                    v += C::from_polar(dt * pv[n], -w * s * dt);
                    i += C::from_polar(dt * pi[n], -w * (s - 0.5) * dt);
                }
                (v, i)
            })
            .collect()
    } else {
        Vec::new()
    };
    Out {
        dt,
        d,
        samples,
        e,
        h,
        il,
        tail: late / peak,
        vi,
    }
}

/// (Fθ, Fφ) at one direction, origin `o`.
fn far(out: &Out, q: usize, f: f64, th: f64, ph: f64, o: [f64; 3]) -> (C, C) {
    let k = 2.0 * PI * f / C0;
    let eta = (MU0 / EPS0).sqrt();
    let rh = [th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos()];
    let mut n = [C::new(0.0, 0.0); 3];
    let mut l = [C::new(0.0, 0.0); 3];
    for (p, sm) in out.samples.iter().enumerate() {
        let dot = (0..3).map(|a| rh[a] * (sm.r[a] - o[a])).sum::<f64>();
        let sign = if mutation() == "exp_sign" { -1.0 } else { 1.0 };
        let w = if mutation() == "no_trapezoid" {
            1e-6
        } else {
            sm.w
        };
        let ph = C::from_polar(w, sign * k * dot);
        n[axis_of(sm.e)] += ph * sm.sj * out.h[q][p];
        l[sm.m_axis] += ph * sm.sm * out.e[q][p];
    }
    let (ct, st, cp, sp) = (th.cos(), th.sin(), ph.cos(), ph.sin());
    let theta = |v: [C; 3]| v[0] * ct * cp + v[1] * ct * sp - v[2] * st;
    let phi = |v: [C; 3]| -v[0] * sp + v[1] * cp;
    if mutation() == "l_sign" {
        l = l.map(|v| -v);
    }
    let mj = C::new(0.0, -k / (4.0 * PI));
    (
        mj * (phi(l) + eta * theta(n)),
        -mj * (theta(l) - eta * phi(n)),
    )
}

fn flux(out: &Out, q: usize) -> f64 {
    out.samples
        .iter()
        .enumerate()
        .map(|(p, sm)| -0.5 * (out.e[q][p] * (sm.sj * out.h[q][p]).conj()).re * sm.w)
        .sum()
}

fn pattern_power(out: &Out, q: usize, f: f64, o: [f64; 3]) -> (f64, f64) {
    let eta = (MU0 / EPS0).sqrt();
    let (nt, np) = (61, 120);
    let mut p = 0.0;
    let mut umax = 0.0f64;
    for a in 0..nt {
        let th = PI * a as f64 / (nt - 1) as f64;
        let wt = if a == 0 || a == nt - 1 { 0.5 } else { 1.0 };
        for b in 0..np {
            let ph = 2.0 * PI * b as f64 / np as f64;
            let (ft, fp) = far(out, q, f, th, ph, o);
            let u = (ft.norm_sqr() + fp.norm_sqr()) / (2.0 * eta);
            umax = umax.max(u);
            p += u * th.sin() * wt * (PI / (nt - 1) as f64) * (2.0 * PI / np as f64);
        }
    }
    (p, umax)
}

fn half_wave() {
    let n = 48;
    let c = n / 2;
    let freqs: Vec<f64> = (0..13).map(|q| 7e9 + 0.5e9 * q as f64).collect();
    for steps in [1000, 2000] {
        let case = Case {
            n,
            npml: 8,
            margin: 2,
            comp: Component::Ez,
            src: (c, c, c),
            steps,
            freqs: freqs.clone(),
            port: true,
            arms: 7,
        };
        let out = run(&case);
        println!(
            "== half-wave wire dipole, arms 7 + gap 1, {steps} steps, tail/peak {:.2e}",
            out.tail
        );
        for (q, &f) in freqs.iter().enumerate() {
            let (v, i) = out.vi[q];
            let z0: f64 = 50.0;
            let a = (v + z0 * i) / (2.0 * z0.sqrt());
            let b = (v - z0 * i) / (2.0 * z0.sqrt());
            let pacc = 0.5 * (v * i.conj()).re;
            let pf = flux(&out, q);
            let (_, umax) = pattern_power(
                &out,
                q,
                f,
                [c as f64 * 1e-3, c as f64 * 1e-3, (c as f64 + 0.5) * 1e-3],
            );
            let d = 4.0 * PI * umax / pf;
            let g = 4.0 * PI * umax / pacc;
            let rg = 4.0 * PI * umax / (0.5 * a.norm_sqr());
            let z = v / i;
            println!(
                "  {:5.2} GHz Zin {:7.2}{:+8.2}j |S11| {:.4} P_acc/P_flux {:.6} D {:.4} ({:.3} dBi) G {:.4} RG {:.4} ({:.3} dBi)",
                f / 1e9, z.re, z.im, (b / a).norm(), pacc / pf, d, 10.0 * d.log10(), g, rg, 10.0 * rg.log10()
            );
        }
    }
}

fn main() {
    if std::env::var("NTFF_CASES").as_deref() == Ok("half-wave") {
        half_wave();
        return;
    }
    let eta = (MU0 / EPS0).sqrt();
    let n: usize = std::env::var("NTFF_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let npml: usize = std::env::var("NTFF_PML")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let c = n / 2;
    let freqs = vec![C0 / 0.040, C0 / 0.030, C0 / 0.020, C0 / 0.015];
    let labels = ["λ/Δ=40", "λ/Δ=30", "λ/Δ=20", "λ/Δ=15"];
    let only = std::env::var("NTFF_CASES").unwrap_or_default();
    for (name, comp, src, margin, steps) in [
        ("z-dipole centre m2", Component::Ez, (c, c, c), 2, 700),
        ("z-dipole centre m2 short", Component::Ez, (c, c, c), 2, 350),
        (
            "z-dipole centre m5",
            Component::Ez,
            (c, c, c),
            if n < 40 { 4 } else { 5 },
            700,
        ),
        ("x-dipole centre m2", Component::Ex, (c, c, c), 2, 700),
        ("z-dipole off +4x m2", Component::Ez, (c, c + 4, c), 2, 700),
        ("z-port centre m2", Component::Ez, (c, c, c), 2, 700),
    ] {
        if !only.is_empty() && !only.split(',').any(|w| name.starts_with(w)) {
            continue;
        }
        let port = name.contains("port");
        let case = Case {
            n,
            npml,
            margin,
            comp,
            src,
            steps,
            freqs: freqs.clone(),
            port,
            arms: 0,
        };
        let out = run(&case);
        let d = out.d;
        // Source position: Ez node (j, i, k+½), Ex (j+½, i, k).
        let sp = match comp {
            Component::Ex => [
                (src.1 as f64 + 0.5) * d[0],
                src.0 as f64 * d[1],
                src.2 as f64 * d[2],
            ],
            _ => [
                src.1 as f64 * d[0],
                src.0 as f64 * d[1],
                (src.2 as f64 + 0.5) * d[2],
            ],
        };
        let centre = [n as f64 * d[0] / 2.0; 3];
        println!(
            "== {name}: samples {} tail/peak {:.2e} dt {:.4e}",
            out.samples.len(),
            out.tail,
            out.dt
        );
        for (q, &f) in freqs.iter().enumerate() {
            let k = 2.0 * PI * f / C0;
            let il = out.il[q];
            let analytic = C::new(0.0, eta * k / (4.0 * PI)) * il;
            let pa = eta * k * k * il.norm_sqr() / (12.0 * PI);
            let pf = flux(&out, q);
            let (pp, umax) = pattern_power(&out, q, f, sp);
            match comp {
                Component::Ez if port => {
                    let (v, i) = out.vi[q];
                    let z0: f64 = 50.0;
                    let a = (v + z0 * i) / (2.0 * z0.sqrt());
                    let b = (v - z0 * i) / (2.0 * z0.sqrt());
                    let pacc = 0.5 * (v * i.conj()).re;
                    let pinc = 0.5 * a.norm_sqr();
                    let s11 = b / a;
                    let g = 4.0 * PI * umax / pacc;
                    let rg = 4.0 * PI * umax / pinc;
                    println!(
                        "  {}: Zin {:.2}{:+.2}j |S11| {:.6} P_acc/P_flux {:.5} (|a|²−|b|²)/2 / P_acc {:.6}",
                        labels[q], (v / i).re, (v / i).im, s11.norm(), pacc / pf,
                        0.5 * (a.norm_sqr() - b.norm_sqr()) / pacc
                    );
                    println!(
                        "     D_flux {:.5} G {:.5} RG {:.4e} RG/(G(1−|S11|²)) {:.6}",
                        4.0 * PI * umax / pf,
                        g,
                        rg,
                        rg / (g * (1.0 - s11.norm_sqr()))
                    );
                }
                Component::Ez if name.contains("off") => {
                    let (a, _) = far(&out, q, f, PI / 2.0, 0.0, centre);
                    let (b, _) = far(&out, q, f, PI / 2.0, PI, centre);
                    let dx_off = sp[0] - centre[0];
                    let want = 2.0 * k * dx_off;
                    let got = (a / b * C::from_polar(1.0, -want)).arg() + want;
                    println!(
                        "  {}: phase(F(φ=0)/F(φ=π)) {:.5} want 2kd {:.5} err {:.2e} rad; |a/b| {:.5}",
                        labels[q], got, want, got - want, (a / b).norm()
                    );
                }
                Component::Ez => {
                    let mut rip = (f64::MAX, 0.0f64);
                    let mut xpol = 0.0f64;
                    for b in 0..72 {
                        let (ft, fp) = far(&out, q, f, PI / 2.0, 2.0 * PI * b as f64 / 72.0, sp);
                        rip = (rip.0.min(ft.norm()), rip.1.max(ft.norm()));
                        xpol = xpol.max(fp.norm() / ft.norm());
                    }
                    let mut shape = 0.0f64;
                    for a in 1..36 {
                        let th = PI * a as f64 / 36.0;
                        let (ft, _) = far(&out, q, f, th, 0.3, sp);
                        shape = shape.max((ft.norm() / (analytic.norm() * th.sin()) - 1.0).abs());
                    }
                    let (ft, _) = far(&out, q, f, PI / 2.0, 0.0, sp);
                    let r = ft / analytic;
                    println!(
                        "  {}: |F/analytic| {:.5} arg {:+.4} rad | az ripple {:.2e} | xpol {:.2e} | sinθ shape {:.2e}",
                        labels[q], r.norm(), r.arg(), rip.1 / rip.0 - 1.0, xpol, shape
                    );
                    println!(
                        "     P_flux/P_an {:.5}  P_pat/P_flux {:.5}  D_flux {:.5}  D_pat {:.5} (1.5 ideal)",
                        pf / pa, pp / pf, 4.0 * PI * umax / pf, 4.0 * PI * umax / pp
                    );
                }
                _ => {
                    let mut err = 0.0f64;
                    let mut xpol = 0.0f64;
                    for b in 0..72 {
                        let ph = 2.0 * PI * b as f64 / 72.0;
                        let (ft, fp) = far(&out, q, f, PI / 2.0, ph, sp);
                        let want = analytic * ph.sin();
                        err = err.max((fp - want).norm() / analytic.norm());
                        xpol = xpol.max(ft.norm() / analytic.norm());
                    }
                    let mut eth = 0.0f64;
                    for a in 0..37 {
                        let th = PI * a as f64 / 36.0;
                        let (ft, _) = far(&out, q, f, th, 0.0, sp);
                        // x̂·θ̂ = cosθ cosφ, F = −(jηkIℓ/4π) x̂_⊥, so Fθ(θ, 0) = −F0 cosθ.
                        eth = eth.max((ft + analytic * th.cos()).norm() / analytic.norm());
                    }
                    println!(
                        "  {}: max|Fφ(90°,φ) − F0 sinφ|/|F0| {:.2e} | max|Fθ(90°)|/|F0| {:.2e} | max|Fθ(θ,0) − F0 cosθ|/|F0| {:.2e}",
                        labels[q], err, xpol, eth
                    );
                    println!(
                        "     P_flux/P_an {:.5}  P_pat/P_flux {:.5}  D_flux {:.5}",
                        pf / pa,
                        pp / pf,
                        4.0 * PI * umax / pf
                    );
                }
            }
        }
    }
}

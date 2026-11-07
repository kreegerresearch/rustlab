//! Near-to-far transform (Phase 5 of `dev/plans/fdtd3d.md`).
//!
//! The derivation and the measured gates are in
//! `dev/plans/fdtd3d_phase5_impl.md`. Two choices that are easy to flip:
//!
//! * Phasors use the sample's own time. E is at `s·dt`, H at `(s − ½)·dt`.
//!   There is no extra half-step multiply on top of that.
//! * `e^{+jωt}` and an outward normal: `J = n̂ × H`, `M = −n̂ × E`, and
//!   `Fθ = −jk/(4π)(Lφ + η Nθ)`, `Fφ = +jk/(4π)(Lθ − η Nφ)`. `F` is
//!   `r e^{jkr} E`.
//!
//! A z-directed electric dipole radiates `Eθ`, not `Eφ`.

use super::port::tail_over_peak;
use super::{Component, Face, FdtdSim, RecordDecay, C0, EPS0, MU0, PULSE_TAIL_LIMIT};
use crate::error::DspError;
use num_complex::Complex;
use rustlab_core::size::checked_elems;
use rustlab_core::C64;
use std::f64::consts::PI;

/// 0-based node planes `(lo, hi)` of the Huygens box on x, y, z.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NtffBox {
    pub x: (usize, usize),
    pub y: (usize, usize),
    pub z: (usize, usize),
}

/// Port phasors at one armed frequency list, using the same sample times as
/// the surface (`V` at `s·dt`, `I` at `(s − ½)·dt`).
#[derive(Clone, Debug)]
pub struct PortPower {
    pub z0: f64,
    pub v: Vec<C64>,
    pub i: Vec<C64>,
    pub s11: Vec<C64>,
    pub accepted: Vec<f64>,
    pub incident: Vec<f64>,
}

/// `F = r e^{jkr} E` on a `theta × phi` grid, plus the power each gain
/// definition divides by. Index of angle `(q, a, b)` is
/// `(q * ntheta + a) * nphi + b`.
#[derive(Clone, Debug)]
pub struct FarField {
    pub freqs: Vec<f64>,
    pub theta: Vec<f64>,
    pub phi: Vec<f64>,
    pub e_theta: Vec<C64>,
    pub e_phi: Vec<C64>,
    /// Outward Poynting flux through the box, one entry per frequency.
    pub prad: Vec<f64>,
    pub k: Vec<f64>,
    pub eta: f64,
    pub origin: [f64; 3],
    /// Surface `|E|` over the last 10% of the run, divided by its peak.
    pub decay: f64,
    pub port: Option<PortPower>,
    pub planes: NtffBox,
}

/// One tangential sample. `repr(C)` keeps the table at 48 bytes, which is
/// what `FdtdSim::bytes` counts.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Sample {
    e_slot: u8,
    h_slot: u8,
    m_axis: u8,
    _pad: u8,
    e_idx: u32,
    h_a: u32,
    h_b: u32,
    tx: u16,
    ty: u16,
    tz: u16,
    _pad2: u16,
    w: f64,
    sj: f64,
    sm: f64,
}

const _: () = assert!(std::mem::size_of::<Sample>() == 48);

#[derive(Clone, Debug)]
pub(super) struct Ntff {
    freqs: Vec<f64>,
    margin: usize,
    ready: Option<Ready>,
}

#[derive(Clone, Debug)]
struct Ready {
    samples: Vec<Sample>,
    /// Frequency-major, `q * n + p`.
    e: Vec<C64>,
    h: Vec<C64>,
    decay: Vec<f64>,
    planes: NtffBox,
    eps: f64,
    mu: f64,
    eta: f64,
    d: [f64; 3],
}

impl Ntff {
    fn new(freqs: Vec<f64>, margin: usize) -> Self {
        Self {
            freqs,
            margin,
            ready: None,
        }
    }

    pub(super) fn freqs(&self) -> &[f64] {
        &self.freqs
    }

    pub(super) fn margin(&self) -> usize {
        self.margin
    }

    /// Heap bytes that `FdtdSim::bytes` adds. Lengths, not capacities, so a
    /// split run and one run report the same number.
    pub(super) fn bytes(&self) -> usize {
        let mut n = self.freqs.len() * 8;
        if let Some(r) = &self.ready {
            n += r.samples.len() * 48;
            n += (r.e.len() + r.h.len()) * 16;
            n += r.decay.len() * 8;
        }
        n
    }

    fn accumulate(
        &mut self,
        step: usize,
        dt: f64,
        ex: &[f64],
        ey: &[f64],
        ez: &[f64],
        hx: &[f64],
        hy: &[f64],
        hz: &[f64],
    ) {
        let Some(r) = self.ready.as_mut() else {
            return;
        };
        let e_fields = [ex, ey, ez];
        let h_fields = [hx, hy, hz];
        let ns = r.samples.len();
        let mut emax = 0.0f64;
        for (q, &f) in self.freqs.iter().enumerate() {
            let w = 2.0 * PI * f;
            let s = step as f64;
            let re = Complex::from_polar(dt, -w * s * dt);
            let rh = Complex::from_polar(dt, -w * (s - 0.5) * dt);
            let e_base = q * ns;
            for (p, sm) in r.samples.iter().enumerate() {
                let ev = e_fields[sm.e_slot as usize][sm.e_idx as usize];
                let hv = 0.5
                    * (h_fields[sm.h_slot as usize][sm.h_a as usize]
                        + h_fields[sm.h_slot as usize][sm.h_b as usize]);
                r.e[e_base + p] += re * ev;
                r.h[e_base + p] += rh * hv;
                if q == 0 {
                    emax = emax.max(ev.abs());
                }
            }
        }
        r.decay.push(emax);
    }
}

impl FdtdSim {
    /// Arm the transform, or disarm it when `freqs` is empty. Only before
    /// the first step. A second call replaces the first. The box is resolved
    /// on the first step, so `fdtd_cpml` / `fdtd_media` / `fdtd_pec` may
    /// still follow this call.
    pub fn set_ntff(&mut self, freqs: &[f64], margin: usize) -> Result<(), DspError> {
        self.require_setup("a near-to-far transform")?;
        if freqs.is_empty() {
            self.ntff = None;
            return Ok(());
        }
        if margin < 1 {
            return Err(DspError::InvalidParameter(
                "margin must be an integer >= 1, so both H samples beside a face sit off the CPML. \
                 Got 0"
                    .into(),
            ));
        }
        let nyquist = 0.5 / self.dt();
        for &f in freqs {
            if !(f.is_finite() && f > 0.0) {
                return Err(DspError::InvalidParameter(format!(
                    "frequencies must be finite and > 0, got {f}"
                )));
            }
            if f >= nyquist {
                return Err(DspError::InvalidParameter(format!(
                    "frequency {f} Hz is at or above the Nyquist frequency 1/(2·dt) = {nyquist} Hz. \
                     A lower frequency, or a finer grid, is what resolves it"
                )));
            }
        }
        self.ntff = Some(Ntff::new(freqs.to_vec(), margin));
        Ok(())
    }

    pub fn ntff_freqs(&self) -> &[f64] {
        self.ntff.as_ref().map(|n| n.freqs()).unwrap_or(&[])
    }

    /// The box the current CPML thicknesses and margin would use, with every
    /// validity check applied. Does not allocate the accumulators.
    pub fn ntff_box(&self) -> Result<NtffBox, DspError> {
        let Some(n) = &self.ntff else {
            return Err(DspError::InvalidParameter(
                "call fdtd_ntff before asking for the Huygens box".into(),
            ));
        };
        Ok(self.validate_ntff(n.margin(), n.freqs())?.0)
    }

    #[cfg(test)]
    pub(super) fn ntff_sample_count(&self) -> Option<usize> {
        self.ntff
            .as_ref()
            .and_then(|n| n.ready.as_ref().map(|r| r.samples.len()))
    }

    pub(super) fn ntff_bytes(&self) -> usize {
        self.ntff.as_ref().map(|n| n.bytes()).unwrap_or(0)
    }

    /// Build the sample table on the first step. A failed check leaves
    /// `steps` at 0 and the fields untouched.
    pub(super) fn prepare_ntff(&mut self) -> Result<(), DspError> {
        if self.ntff.is_none() || self.ntff.as_ref().unwrap().ready.is_some() {
            return Ok(());
        }
        let margin = self.ntff.as_ref().unwrap().margin();
        let freqs = self.ntff.as_ref().unwrap().freqs().to_vec();
        let (bx, eps, mu) = self.validate_ntff(margin, &freqs)?;
        let (dx, dy, dz) = self.spacing();
        let samples = build_samples(bx, (self.nx(), self.ny(), self.nz()), [dx, dy, dz]);
        let ns = samples.len();
        let nf = freqs.len();
        checked_elems(&[ns, nf, 2], 16)?;
        let eta0 = (MU0 / EPS0).sqrt();
        let ready = Ready {
            samples,
            e: vec![C64::new(0.0, 0.0); ns * nf],
            h: vec![C64::new(0.0, 0.0); ns * nf],
            decay: Vec::new(),
            planes: bx,
            eps,
            mu,
            eta: eta0 * (mu / eps).sqrt(),
            d: [dx, dy, dz],
        };
        self.ntff.as_mut().unwrap().ready = Some(ready);
        Ok(())
    }

    pub(super) fn accumulate_ntff(&mut self) {
        let dt = self.dt();
        let step = self.steps();
        let FdtdSim {
            ntff,
            ex,
            ey,
            ez,
            hx,
            hy,
            hz,
            ..
        } = self;
        if let Some(ntff) = ntff.as_mut() {
            ntff.accumulate(step, dt, ex, ey, ez, hx, hy, hz);
        }
    }

    pub fn far_field(
        &self,
        theta: &[f64],
        phi: &[f64],
        origin: Option<[f64; 3]>,
        decay: RecordDecay,
    ) -> Result<FarField, DspError> {
        let Some(ntff) = &self.ntff else {
            return Err(DspError::InvalidParameter(
                "call fdtd_ntff before asking for a far field".into(),
            ));
        };
        let Some(r) = &ntff.ready else {
            return Err(DspError::InvalidParameter(
                "run the grid before fdtd_farfield. The surface phasors are accumulated during the run"
                    .into(),
            ));
        };
        if theta.is_empty() || phi.is_empty() {
            return Err(DspError::InvalidParameter(
                "theta and phi must each contain at least one angle".into(),
            ));
        }
        if theta.iter().any(|t| !t.is_finite()) || phi.iter().any(|p| !p.is_finite()) {
            return Err(DspError::InvalidParameter(
                "theta and phi must be finite".into(),
            ));
        }
        let origin = match origin {
            Some(o) => {
                if o.iter().any(|x| !x.is_finite()) {
                    return Err(DspError::InvalidParameter(
                        "origin must be three finite coordinates, in metres".into(),
                    ));
                }
                o
            }
            None => {
                let (dx, dy, dz) = (r.d[0], r.d[1], r.d[2]);
                [
                    (r.planes.x.0 + r.planes.x.1) as f64 * 0.5 * dx,
                    (r.planes.y.0 + r.planes.y.1) as f64 * 0.5 * dy,
                    (r.planes.z.0 + r.planes.z.1) as f64 * 0.5 * dz,
                ]
            }
        };
        // Once, at the end. A NaN field does not show up in the decay
        // trace: `f64::max` drops NaN, and the phasor sum then returns a
        // NaN pattern with no error. Inf survives that max and still has
        // to be refused. Gain stays NaN when there is no port or the
        // power is not positive; that is a defined result, not bad data.
        require_finite(&self.ex, "a field sample")?;
        require_finite(&self.ey, "a field sample")?;
        require_finite(&self.ez, "a field sample")?;
        require_finite(&self.hx, "a field sample")?;
        require_finite(&self.hy, "a field sample")?;
        require_finite(&self.hz, "a field sample")?;
        require_finite(&r.decay, "the surface field")?;
        if self.n_ports() > 0 {
            require_finite(self.port_voltage(), "the port voltage")?;
            require_finite(self.port_current(), "the port current")?;
        }
        if r.e.iter().any(|z| !z.is_finite()) || r.h.iter().any(|z| !z.is_finite()) {
            return Err(DspError::InvalidParameter(
                "a surface phasor is not finite (NaN or Inf). The far field is not a pattern"
                    .into(),
            ));
        }
        let surface = tail_over_peak(&r.decay);
        if decay == RecordDecay::Pulse {
            if surface > PULSE_TAIL_LIMIT {
                return Err(DspError::InvalidParameter(format!(
                    "the surface field has not decayed: |E| tail/peak = {surface:.3e} \
                     (limit {PULSE_TAIL_LIMIT:.0e} over the last 10%). A truncated pulse has no usable \
                     far field. A longer record is what converges; zero padding does not. A steady tone \
                     is not a pulse: pass \"decay\", \"off\""
                )));
            }
            if self.n_ports() > 0 {
                let rv = tail_over_peak(self.port_voltage());
                let ri = tail_over_peak(self.port_current());
                if rv > PULSE_TAIL_LIMIT || ri > PULSE_TAIL_LIMIT {
                    return Err(DspError::InvalidParameter(format!(
                        "the port record has not decayed: |V| tail/peak = {rv:.3e}, |I| tail/peak = {ri:.3e} \
                         (limit {PULSE_TAIL_LIMIT:.0e} over the last 10%). Gain needs those phasors. \
                         A longer record is what converges. Pass \"decay\", \"off\" to skip the check"
                    )));
                }
            }
        }
        let nf = ntff.freqs.len();
        let ns = r.samples.len();
        let nmed = (r.eps * r.mu).sqrt();
        let mut k = Vec::with_capacity(nf);
        let mut prad = Vec::with_capacity(nf);
        for (q, &f) in ntff.freqs.iter().enumerate() {
            k.push(2.0 * PI * f * nmed / C0);
            let mut p = 0.0;
            for (s, sm) in r.samples.iter().enumerate() {
                let ev = r.e[q * ns + s];
                let hv = r.h[q * ns + s];
                p += -0.5 * (ev * (sm.sj * hv).conj()).re * sm.w;
            }
            prad.push(p);
        }
        if prad.iter().any(|p| !p.is_finite()) {
            return Err(DspError::InvalidParameter(
                "the flux is not finite (NaN or Inf). The far field is not a pattern".into(),
            ));
        }
        let (nt, np) = (theta.len(), phi.len());
        let mut e_theta = vec![C64::new(0.0, 0.0); nf * nt * np];
        let mut e_phi = vec![C64::new(0.0, 0.0); nf * nt * np];
        let (nx, ny, nz) = (self.nx(), self.ny(), self.nz());
        for (q, &f) in ntff.freqs.iter().enumerate() {
            let kq = k[q];
            let _ = f;
            for (a, &th) in theta.iter().enumerate() {
                for (b, &ph) in phi.iter().enumerate() {
                    let rh = [th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos()];
                    let px = axis_phase(nx, r.d[0], kq * rh[0], origin[0]);
                    let py = axis_phase(ny, r.d[1], kq * rh[1], origin[1]);
                    let pz = axis_phase(nz, r.d[2], kq * rh[2], origin[2]);
                    let mut n = [C64::new(0.0, 0.0); 3];
                    let mut l = [C64::new(0.0, 0.0); 3];
                    for (s, sm) in r.samples.iter().enumerate() {
                        let phase =
                            px[sm.tx as usize] * py[sm.ty as usize] * pz[sm.tz as usize] * sm.w;
                        n[sm.e_slot as usize] += phase * sm.sj * r.h[q * ns + s];
                        l[sm.m_axis as usize] += phase * sm.sm * r.e[q * ns + s];
                    }
                    let (ft, fp) = spherical(th, ph, n, l, kq, r.eta);
                    let slot = (q * nt + a) * np + b;
                    e_theta[slot] = ft;
                    e_phi[slot] = fp;
                }
            }
        }
        let port = if self.n_ports() == 0 {
            None
        } else {
            Some(port_power(self, &ntff.freqs))
        };
        Ok(FarField {
            freqs: ntff.freqs.clone(),
            theta: theta.to_vec(),
            phi: phi.to_vec(),
            e_theta,
            e_phi,
            prad,
            k,
            eta: r.eta,
            origin,
            decay: surface,
            port,
            planes: r.planes,
        })
    }

    /// Box, background `eps_r`, background `mu_r`. Every §2.6 check.
    fn validate_ntff(&self, margin: usize, freqs: &[f64]) -> Result<(NtffBox, f64, f64), DspError> {
        if margin < 1 {
            return Err(DspError::InvalidParameter(
                "margin must be an integer >= 1".into(),
            ));
        }
        let faces = self.cpml_faces();
        let (nx, ny, nz) = (self.nx(), self.ny(), self.nz());
        let mut planes = [(0usize, 0usize); 3];
        for (axis, len, n_lo, n_hi, name_lo, name_hi) in [
            (
                0,
                nx,
                faces[0],
                faces[1],
                Face::XMin.name(),
                Face::XMax.name(),
            ),
            (
                1,
                ny,
                faces[2],
                faces[3],
                Face::YMin.name(),
                Face::YMax.name(),
            ),
            (
                2,
                nz,
                faces[4],
                faces[5],
                Face::ZMin.name(),
                Face::ZMax.name(),
            ),
        ] {
            if n_lo == 0 {
                return Err(off_face(name_lo));
            }
            if n_hi == 0 {
                return Err(off_face(name_hi));
            }
            let lo = n_lo + margin;
            let hi = len - n_hi - margin;
            if hi < lo + 2 {
                return Err(DspError::InvalidParameter(format!(
                    "the Huygens box collapses on axis {axis_name} (nodes {lo}..{hi}). \
                     Lower the margin or the CPML thickness so each face pair is at least two cells apart",
                    axis_name = ["x", "y", "z"][axis]
                )));
            }
            planes[axis] = (lo, hi);
        }
        let bx = NtffBox {
            x: planes[0],
            y: planes[1],
            z: planes[2],
        };
        let (eps, mu) = self.homogeneous_exterior(&bx)?;
        let nyquist = 0.5 / self.dt();
        let c_bg = C0 / (eps * mu).sqrt();
        let dmax = self.dx().max(self.dy()).max(self.dz());
        for &f in freqs {
            if f >= nyquist {
                return Err(DspError::InvalidParameter(format!(
                    "frequency {f} Hz is at or above the Nyquist frequency 1/(2·dt) = {nyquist} Hz"
                )));
            }
            let cells = c_bg / (f * dmax);
            if cells < 10.0 {
                return Err(DspError::InvalidParameter(format!(
                    "frequency {f} Hz is only {cells:.2} background cells per wavelength \
                     (need at least 10). The pattern error grows quickly below that"
                )));
            }
        }
        self.drivers_inside(&bx)?;
        Ok((bx, eps, mu))
    }

    fn dx(&self) -> f64 {
        self.spacing().0
    }
    fn dy(&self) -> f64 {
        self.spacing().1
    }
    fn dz(&self) -> f64 {
        self.spacing().2
    }

    fn homogeneous_exterior(&self, bx: &NtffBox) -> Result<(f64, f64), DspError> {
        let Some(m) = &self.media else {
            return Ok((1.0, 1.0));
        };
        let maps = [&m.eps, &m.mu, &m.sigma];
        let (e0, u0, s0) = (maps[0].at(0), maps[1].at(0), maps[2].at(0));
        if s0 != 0.0 {
            return Err(DspError::InvalidParameter(format!(
                "the background sigma is {s0} S/m. A lossy exterior needs a complex k, which this \
                 version does not do. Keep sigma = 0 outside the box and in the one-cell layer inside each face"
            )));
        }
        let (nx, ny, nz) = (self.nx(), self.ny(), self.nz());
        let inside = |j: usize, i: usize, k: usize| {
            j >= bx.x.0 + 1
                && j <= bx.x.1 - 2
                && i >= bx.y.0 + 1
                && i <= bx.y.1 - 2
                && k >= bx.z.0 + 1
                && k <= bx.z.1 - 2
        };
        for k in 0..nz {
            for i in 0..ny {
                for j in 0..nx {
                    if inside(j, i, k) {
                        continue;
                    }
                    let c = self.idx(i, j, k);
                    let (e, u, s) = (maps[0].at(c), maps[1].at(c), maps[2].at(c));
                    if e != e0 || u != u0 || s != 0.0 {
                        return Err(DspError::InvalidParameter(format!(
                            "cell (i={i}, j={j}, k={k}) is in the Huygens exterior or in the one-cell \
                             layer inside a face, and its material differs from the background \
                             (eps_r {e} vs {e0}, mu_r {u} vs {u0}, sigma {s}). Materials may change \
                             only for cells strictly inside that layer. A closed surface needs a \
                             homogeneous exterior"
                        )));
                    }
                }
            }
        }
        Ok((e0, u0))
    }

    fn drivers_inside(&self, bx: &NtffBox) -> Result<(), DspError> {
        let mut edges: Vec<(Component, usize, usize, usize)> = Vec::new();
        for s in &self.sources {
            for &e in &s.edges {
                let (i, j, k) = self.unravel(e);
                edges.push((s.comp, i, j, k));
            }
        }
        for p in &self.ports {
            for &(i, j, k) in &p.nodes {
                edges.push((p.comp, i, j, k));
            }
        }
        let (nx, ny, nz) = (self.nx, self.ny, self.nz);
        if self.pec.is_some() {
            for k in 0..nz {
                for i in 0..ny {
                    for j in 0..nx {
                        for comp in [Component::Ex, Component::Ey, Component::Ez] {
                            if self.is_pec_edge(comp, i, j, k) {
                                edges.push((comp, i, j, k));
                            }
                        }
                    }
                }
            }
        }
        for (comp, i, j, k) in edges {
            if !edge_inside(comp, i, j, k, bx) {
                return Err(DspError::InvalidParameter(format!(
                    "{} edge ({i}, {j}, {k}) is not strictly inside the Huygens box \
                     (node planes x {}..{}, y {}..{}, z {}..{}). Move the source or the metal \
                     inside, or reduce the margin",
                    comp.name(),
                    bx.x.0,
                    bx.x.1,
                    bx.y.0,
                    bx.y.1,
                    bx.z.0,
                    bx.z.1
                )));
            }
        }
        Ok(())
    }

    fn unravel(&self, idx: usize) -> (usize, usize, usize) {
        let j = idx % self.nx;
        let t = idx / self.nx;
        let i = t % self.ny;
        let k = t / self.ny;
        (i, j, k)
    }
}

/// `4π U / power` with `U = (|Fθ|² + |Fφ|²) / (2η)`. `NaN` when `power`
/// is not strictly positive, so a negative accepted power is not reported
/// as a gain.
pub fn directive_gain(e_theta: C64, e_phi: C64, eta: f64, power: f64) -> f64 {
    if !(power > 0.0) || !(eta.is_finite() && eta > 0.0) {
        return f64::NAN;
    }
    let u = (e_theta.norm_sqr() + e_phi.norm_sqr()) / (2.0 * eta);
    4.0 * PI * u / power
}

/// `10·log10(x)` for `x > 0`, otherwise `NaN`.
pub fn db10(x: f64) -> f64 {
    if x > 0.0 {
        10.0 * x.log10()
    } else {
        f64::NAN
    }
}

fn require_finite(samples: &[f64], what: &str) -> Result<(), DspError> {
    if samples.iter().any(|x| !x.is_finite()) {
        return Err(DspError::InvalidParameter(format!(
            "{what} is not finite (NaN or Inf). The far field is not a pattern"
        )));
    }
    Ok(())
}

fn off_face(name: &str) -> DspError {
    DspError::InvalidParameter(format!(
        "CPML face {name} is off. The near-to-far box needs an absorber on all six faces: a PEC wall \
         beyond the surface is not the homogeneous unbounded exterior the equivalence theorem uses. \
         Turn {name} on (v1 does not do a five-face box or image theory)"
    ))
}

fn axis_phase(len: usize, d: f64, k_comp: f64, origin: f64) -> Vec<C64> {
    (0..=2 * len)
        .map(|t| {
            let x = t as f64 * d * 0.5;
            Complex::from_polar(1.0, k_comp * (x - origin))
        })
        .collect()
}

fn spherical(th: f64, ph: f64, n: [C64; 3], l: [C64; 3], k: f64, eta: f64) -> (C64, C64) {
    let (ct, st, cp, sp) = (th.cos(), th.sin(), ph.cos(), ph.sin());
    let theta = |v: [C64; 3]| v[0] * ct * cp + v[1] * ct * sp - v[2] * st;
    let phi = |v: [C64; 3]| -v[0] * sp + v[1] * cp;
    let mj = Complex::new(0.0, -k / (4.0 * PI));
    (
        mj * (phi(l) + eta * theta(n)),
        -mj * (theta(l) - eta * phi(n)),
    )
}

fn port_power(sim: &FdtdSim, freqs: &[f64]) -> PortPower {
    let z0 = sim.port_z0()[0];
    let dt = sim.dt();
    let vv = sim.port_voltage();
    let ii = sim.port_current();
    let sqrt_z = z0.sqrt();
    let mut v = Vec::with_capacity(freqs.len());
    let mut i = Vec::with_capacity(freqs.len());
    let mut s11 = Vec::with_capacity(freqs.len());
    let mut accepted = Vec::with_capacity(freqs.len());
    let mut incident = Vec::with_capacity(freqs.len());
    for &f in freqs {
        let w = 2.0 * PI * f;
        let mut pv = C64::new(0.0, 0.0);
        let mut pi = C64::new(0.0, 0.0);
        for n in 0..vv.len() {
            let s = (n + 1) as f64;
            pv += Complex::from_polar(dt * vv[n], -w * s * dt);
            pi += Complex::from_polar(dt * ii[n], -w * (s - 0.5) * dt);
        }
        let a = (pv + z0 * pi) / (2.0 * sqrt_z);
        let b = (pv - z0 * pi) / (2.0 * sqrt_z);
        let s = if a.norm() == 0.0 {
            C64::new(f64::NAN, f64::NAN)
        } else {
            b / a
        };
        v.push(pv);
        i.push(pi);
        s11.push(s);
        accepted.push(0.5 * (pv * pi.conj()).re);
        incident.push(0.5 * a.norm_sqr());
    }
    PortPower {
        z0,
        v,
        i,
        s11,
        accepted,
        incident,
    }
}

fn edge_inside(comp: Component, i: usize, j: usize, k: usize, bx: &NtffBox) -> bool {
    let ends: [(usize, usize, usize); 2] = match comp {
        Component::Ex => [(j, i, k), (j + 1, i, k)],
        Component::Ey => [(j, i, k), (j, i + 1, k)],
        Component::Ez => [(j, i, k), (j, i, k + 1)],
        _ => return false,
    };
    ends.iter().all(|&(x, y, z)| {
        x >= bx.x.0 + 1
            && x <= bx.x.1 - 1
            && y >= bx.y.0 + 1
            && y <= bx.y.1 - 1
            && z >= bx.z.0 + 1
            && z <= bx.z.1 - 1
    })
}

fn build_samples(bx: NtffBox, (nx, ny, nz): (usize, usize, usize), d: [f64; 3]) -> Vec<Sample> {
    let mut out = Vec::new();
    let wt = |m: usize, a: usize, b: usize| if m == a || m == b { 0.5 } else { 1.0 };
    let flat = |i: usize, j: usize, k: usize| ((k * ny + i) * nx + j) as u32;
    let (xlo, xhi) = bx.x;
    let (ylo, yhi) = bx.y;
    let (zlo, zhi) = bx.z;
    for (s, kz) in [(-1.0, zlo), (1.0, zhi)] {
        for i in ylo..=yhi {
            for j in xlo..xhi {
                out.push(Sample {
                    e_slot: 0,
                    h_slot: 1,
                    m_axis: 1,
                    _pad: 0,
                    e_idx: flat(i, j, kz),
                    h_a: flat(i, j, kz - 1),
                    h_b: flat(i, j, kz),
                    tx: (2 * j + 1) as u16,
                    ty: (2 * i) as u16,
                    tz: (2 * kz) as u16,
                    _pad2: 0,
                    w: d[0] * d[1] * wt(i, ylo, yhi),
                    sj: -s,
                    sm: -s,
                });
            }
        }
        for i in ylo..yhi {
            for j in xlo..=xhi {
                out.push(Sample {
                    e_slot: 1,
                    h_slot: 0,
                    m_axis: 0,
                    _pad: 0,
                    e_idx: flat(i, j, kz),
                    h_a: flat(i, j, kz - 1),
                    h_b: flat(i, j, kz),
                    tx: (2 * j) as u16,
                    ty: (2 * i + 1) as u16,
                    tz: (2 * kz) as u16,
                    _pad2: 0,
                    w: d[0] * d[1] * wt(j, xlo, xhi),
                    sj: s,
                    sm: s,
                });
            }
        }
    }
    for (s, jx) in [(-1.0, xlo), (1.0, xhi)] {
        for k in zlo..=zhi {
            for i in ylo..yhi {
                out.push(Sample {
                    e_slot: 1,
                    h_slot: 2,
                    m_axis: 2,
                    _pad: 0,
                    e_idx: flat(i, jx, k),
                    h_a: flat(i, jx - 1, k),
                    h_b: flat(i, jx, k),
                    tx: (2 * jx) as u16,
                    ty: (2 * i + 1) as u16,
                    tz: (2 * k) as u16,
                    _pad2: 0,
                    w: d[1] * d[2] * wt(k, zlo, zhi),
                    sj: -s,
                    sm: -s,
                });
            }
        }
        for k in zlo..zhi {
            for i in ylo..=yhi {
                out.push(Sample {
                    e_slot: 2,
                    h_slot: 1,
                    m_axis: 1,
                    _pad: 0,
                    e_idx: flat(i, jx, k),
                    h_a: flat(i, jx - 1, k),
                    h_b: flat(i, jx, k),
                    tx: (2 * jx) as u16,
                    ty: (2 * i) as u16,
                    tz: (2 * k + 1) as u16,
                    _pad2: 0,
                    w: d[1] * d[2] * wt(i, ylo, yhi),
                    sj: s,
                    sm: s,
                });
            }
        }
    }
    for (s, iy) in [(-1.0, ylo), (1.0, yhi)] {
        for k in zlo..=zhi {
            for j in xlo..xhi {
                out.push(Sample {
                    e_slot: 0,
                    h_slot: 2,
                    m_axis: 2,
                    _pad: 0,
                    e_idx: flat(iy, j, k),
                    h_a: flat(iy - 1, j, k),
                    h_b: flat(iy, j, k),
                    tx: (2 * j + 1) as u16,
                    ty: (2 * iy) as u16,
                    tz: (2 * k) as u16,
                    _pad2: 0,
                    w: d[0] * d[2] * wt(k, zlo, zhi),
                    sj: s,
                    sm: s,
                });
            }
        }
        for k in zlo..zhi {
            for j in xlo..=xhi {
                out.push(Sample {
                    e_slot: 2,
                    h_slot: 0,
                    m_axis: 0,
                    _pad: 0,
                    e_idx: flat(iy, j, k),
                    h_a: flat(iy - 1, j, k),
                    h_b: flat(iy, j, k),
                    tx: (2 * j) as u16,
                    ty: (2 * iy) as u16,
                    tz: (2 * k + 1) as u16,
                    _pad2: 0,
                    w: d[0] * d[2] * wt(j, xlo, xhi),
                    sj: -s,
                    sm: -s,
                });
            }
        }
    }
    let _ = nz;
    out
}

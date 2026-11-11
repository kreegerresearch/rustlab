//! 3-D FDTD (Yee) time stepper.
//!
//! Phases 1–4 of `dev/plans/fdtd3d.md`: PEC outer walls, soft and hard
//! E-edge sources, point probes, cell-centred `eps_r` / `mu_r` / `sigma`
//! ([`FdtdSim::set_media`]), interior PEC volumes and faces
//! ([`FdtdSim::add_pec_volume`], [`FdtdSim::add_pec_face`]), a
//! convolutional PML inside those walls ([`FdtdSim::set_cpml`]), and one
//! lumped port ([`FdtdSim::add_port`], [`s11`]). The H and E curls run on
//! the shared rayon pool once the grid is at least 32,768 cells, unless
//! [`FdtdSim::set_threads`] forces one thread. Each row is the same
//! function either way, so the fields stay bit-identical.
//! The material bake is in `media.rs`; the absorber is in `cpml.rs`;
//! the port source and S11 are in `port.rs`.
//!
//! # Layout
//!
//! Script coordinates are `(i, j, k)` = (y row, x column, z page), the
//! `zeros3(ny, nx, nz)` order. Storage is six `Vec<f64>`, x-fastest:
//! `idx(i, j, k) = (k * ny + i) * nx + j` (0-based), so the inner loop of a
//! curl is unit stride. A script `Tensor3` is z-contiguous; slices are copied
//! out, never aliased.
//!
//! # Stagger
//!
//! Every component has the cell-grid shape `ny × nx × nz`. Physical position
//! of the value stored at 0-based `(i, j, k)`, in units of the cell size:
//!
//! | Component | x | y | z |
//! |---|---|---|---|
//! | `Ex` | `j + ½` | `i` | `k` |
//! | `Ey` | `j` | `i + ½` | `k` |
//! | `Ez` | `j` | `i` | `k + ½` |
//! | `Hx` | `j` | `i + ½` | `k + ½` |
//! | `Hy` | `j + ½` | `i` | `k + ½` |
//! | `Hz` | `j + ½` | `i + ½` | `k` |
//!
//! The box is `[0, nx·dx] × [0, ny·dy] × [0, nz·dz]` with PEC walls. On the
//! min walls the tangential E edges (index 0 on an axis normal to the wall)
//! are never updated and stay 0. The max walls are the missing edge past the
//! last index, read as 0 by the H update.
//!
//! # Step
//!
//! Step `s` (1-based) advances H to `(s − ½)·dt`, records H probes, advances
//! E to `s·dt`, applies sources and the lumped port with the waveform at
//! `s·dt`, then records E probes and the port's `V` and `I`. Soft:
//! `E ← E − Cb·g/Δ` per edge, with `Cb = dt/ε0` in vacuum and the edge's
//! baked `Cb` in a medium. Hard: `E ← −g/Δ`. `Δ` is the cell size along the
//! component; on a stack of `N` edges `g` is split as `g/N`. A port is a
//! series `Vg` with gap resistance `Z0`: the resistance is extra edge
//! conductivity, and each edge subtracts `(dt/ε0)·Cb·Vg/(Z0·Area)` (see
//! `port`). Both signs make a positive `g` or `Vg` a positive
//! `V = −Σ E·Δ`.

mod cpml;
mod media;
mod ntff;
mod port;
mod pulse;
mod yee;

#[cfg(test)]
mod ntff_tests;
#[cfg(test)]
mod par_tests;
#[cfg(test)]
mod patch_tests;
#[cfg(test)]
mod tests;

pub use cpml::{CpmlParams, Face};
pub use media::CellMap;
pub use ntff::{db10, directive_gain, FarField, NtffBox, PortPower};
pub use port::{s11, s11_with, PortWindow, RecordDecay, S11Spec, PULSE_TAIL_LIMIT, S11};
pub use pulse::gaussian_pulse;

use crate::error::DspError;
use rustlab_core::size::{checked_elems, ELEM_F64};

/// Speed of light in vacuum, m/s.
pub const C0: f64 = 299_792_458.0;
/// Vacuum permeability, H/m (CODATA 2018).
pub const MU0: f64 = 1.256_637_062_12e-6;
/// Vacuum permittivity, F/m, `1 / (μ0 c0²)`.
pub const EPS0: f64 = 1.0 / (MU0 * C0 * C0);
/// Courant number used when the caller does not pass one.
pub const DEFAULT_COURANT: f64 = 0.99;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Component {
    Ex,
    Ey,
    Ez,
    Hx,
    Hy,
    Hz,
}

impl Component {
    /// `"ex"` … `"hz"`, ASCII case-insensitive.
    pub fn parse(s: &str) -> Option<Component> {
        match s.to_ascii_lowercase().as_str() {
            "ex" => Some(Component::Ex),
            "ey" => Some(Component::Ey),
            "ez" => Some(Component::Ez),
            "hx" => Some(Component::Hx),
            "hy" => Some(Component::Hy),
            "hz" => Some(Component::Hz),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Component::Ex => "ex",
            Component::Ey => "ey",
            Component::Ez => "ez",
            Component::Hx => "hx",
            Component::Hy => "hy",
            Component::Hz => "hz",
        }
    }

    pub fn is_electric(self) -> bool {
        matches!(self, Component::Ex | Component::Ey | Component::Ez)
    }

    /// Axis the component points along.
    pub fn axis(self) -> Axis {
        match self {
            Component::Ex | Component::Hx => Axis::X,
            Component::Ey | Component::Hy => Axis::Y,
            Component::Ez | Component::Hz => Axis::Z,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn parse(s: &str) -> Option<Axis> {
        match s.to_ascii_lowercase().as_str() {
            "x" => Some(Axis::X),
            "y" => Some(Axis::Y),
            "z" => Some(Axis::Z),
            _ => None,
        }
    }
}

/// Source time function, sampled at `s·dt` on step `s`.
#[derive(Clone, Debug)]
pub enum Waveform {
    /// `gaussian_pulse(t, t0, tau, f0)`, applied on every step.
    Pulse { t0: f64, tau: f64, f0: f64 },
    /// Sample `s` (1-based) on step `s`. Past the end the source is off: a
    /// soft source adds nothing and a hard source releases the edge.
    Samples(Vec<f64>),
}

impl Waveform {
    fn value(&self, step: usize, dt: f64) -> Option<f64> {
        match self {
            Waveform::Pulse { t0, tau, f0 } => {
                Some(gaussian_pulse(step as f64 * dt, *t0, *tau, *f0))
            }
            Waveform::Samples(v) => v.get(step - 1).copied(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Soft,
    Hard,
}

#[derive(Clone, Debug)]
struct Source {
    comp: Component,
    kind: SourceKind,
    edges: Vec<usize>,
    waveform: Waveform,
}

#[derive(Clone, Debug)]
struct Probe {
    comp: Component,
    idx: usize,
}

/// One lumped port: series `Vg` with resistance `z0` on a collinear E stack.
///
/// v1 holds at most one. `nodes[0]` is the edge whose Ampère loop is `I`.
#[derive(Clone, Debug)]
struct Port {
    comp: Component,
    nodes: Vec<(usize, usize, usize)>,
    edges: Vec<usize>,
    z0: f64,
    waveform: Waveform,
}

/// Result of [`FdtdSim::run`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunReport {
    /// Steps completed by this call.
    pub completed: usize,
    /// `true` when the stop callback ended the call early.
    pub stopped: bool,
}

/// One FDTD grid: fields, sources, probes and the probe record.
///
/// The outer walls are PEC. [`FdtdSim::set_cpml`] adds an absorbing layer
/// inside them; a face left at 0 stays a plain PEC wall.
#[derive(Clone, Debug)]
pub struct FdtdSim {
    nx: usize,
    ny: usize,
    nz: usize,
    dx: f64,
    dy: f64,
    dz: f64,
    dt: f64,
    courant: f64,
    ex: Vec<f64>,
    ey: Vec<f64>,
    ez: Vec<f64>,
    hx: Vec<f64>,
    hy: Vec<f64>,
    hz: Vec<f64>,
    zero_row: Vec<f64>,
    sources: Vec<Source>,
    probes: Vec<Probe>,
    /// At most one in v1. Empty means the Phase 1–3 kernel, bit for bit.
    ports: Vec<Port>,
    /// Row-major `steps × probes.len()`.
    record: Vec<f64>,
    /// Row-major `steps × ports.len()`. Gap voltage after the source.
    port_v: Vec<f64>,
    /// Row-major `steps × ports.len()`. Ampère current at the half-step.
    port_i: Vec<f64>,
    steps: usize,
    interrupted: bool,
    /// `None` until [`FdtdSim::set_media`]: vacuum.
    media: Option<media::Media>,
    /// Per-cell PEC bits (`media::PEC_EX` …), `None` until the first PEC.
    pec: Option<Vec<u8>>,
    baked: media::Baked,
    /// `None` until [`FdtdSim::set_cpml`] with `n > 0`.
    cpml: Option<cpml::CpmlSettings>,
    /// Identity profiles and one scratch row when CPML is off; full `q`
    /// arrays when any face is on.
    absorber: cpml::Absorber,
    /// `None` until [`FdtdSim::set_ntff`] with a non-empty frequency list.
    /// Off is the Phase 1–4 step, bit for bit.
    ntff: Option<ntff::Ntff>,
    /// `true` keeps the serial curl (`"threads", 1`). `false` is the
    /// shared pool above the cell threshold (`"threads", 0`).
    force_serial: bool,
}

/// `courant / (c0 sqrt(1/dx² + 1/dy² + 1/dz²))`.
pub fn cfl_dt(dx: f64, dy: f64, dz: f64, courant: f64) -> f64 {
    cfl_dt_at(dx, dy, dz, courant, C0)
}

/// `courant / (speed sqrt(1/dx² + 1/dy² + 1/dz²))`.
fn cfl_dt_at(dx: f64, dy: f64, dz: f64, courant: f64, speed: f64) -> f64 {
    courant / (speed * (1.0 / (dx * dx) + 1.0 / (dy * dy) + 1.0 / (dz * dz)).sqrt())
}

impl FdtdSim {
    /// Vacuum grid with E = H = 0. `nx, ny, nz >= 2`; `dx, dy, dz` finite and
    /// positive (metres); `courant` in `(0, 1]`.
    pub fn new(
        nx: usize,
        ny: usize,
        nz: usize,
        dx: f64,
        dy: f64,
        dz: f64,
        courant: f64,
    ) -> Result<FdtdSim, DspError> {
        if nx < 2 || ny < 2 || nz < 2 {
            return Err(DspError::InvalidParameter(format!(
                "nx, ny, nz must each be at least 2, got {nx}, {ny}, {nz}"
            )));
        }
        for (name, v) in [("dx", dx), ("dy", dy), ("dz", dz)] {
            if !(v.is_finite() && v > 0.0) {
                return Err(DspError::InvalidParameter(format!(
                    "{name} must be finite and positive, got {v}"
                )));
            }
        }
        if !(courant > 0.0 && courant <= 1.0) {
            return Err(DspError::InvalidParameter(format!(
                "courant must be in (0, 1], got {courant}"
            )));
        }
        let n = checked_elems(&[nx, ny, nz, 6], ELEM_F64)? / 6;
        let dt = cfl_dt(dx, dy, dz, courant);
        if !(dt.is_finite() && dt > 0.0) {
            return Err(DspError::InvalidParameter(format!(
                "time step is not finite and positive (dt = {dt})"
            )));
        }
        Ok(FdtdSim {
            nx,
            ny,
            nz,
            dx,
            dy,
            dz,
            dt,
            courant,
            ex: vec![0.0; n],
            ey: vec![0.0; n],
            ez: vec![0.0; n],
            hx: vec![0.0; n],
            hy: vec![0.0; n],
            hz: vec![0.0; n],
            zero_row: vec![0.0; nx],
            sources: Vec::new(),
            probes: Vec::new(),
            ports: Vec::new(),
            record: Vec::new(),
            port_v: Vec::new(),
            port_i: Vec::new(),
            steps: 0,
            interrupted: false,
            media: None,
            pec: None,
            baked: media::Baked::default(),
            cpml: None,
            absorber: cpml::Absorber::off(nx, ny, nz),
            ntff: None,
            force_serial: false,
        })
    }

    /// `0` uses the shared pool above the cell threshold. `1` forces the
    /// serial curl. Any other value is an error. Safe before or after the
    /// first step: a step is the same arithmetic either way.
    pub fn set_threads(&mut self, threads: u32) -> Result<(), DspError> {
        match threads {
            0 => self.force_serial = false,
            1 => self.force_serial = true,
            _ => {
                return Err(DspError::InvalidParameter(format!(
                    "threads must be 0 (shared pool) or 1 (serial), got {threads}"
                )))
            }
        }
        Ok(())
    }

    /// `0` for the shared pool, `1` when serial stepping was requested.
    pub fn threads(&self) -> u32 {
        u32::from(self.force_serial)
    }

    fn parallel_step(&self) -> bool {
        !self.force_serial && self.n_cells() >= yee::par_cells_threshold()
    }

    pub fn nx(&self) -> usize {
        self.nx
    }
    pub fn ny(&self) -> usize {
        self.ny
    }
    pub fn nz(&self) -> usize {
        self.nz
    }
    pub fn spacing(&self) -> (f64, f64, f64) {
        (self.dx, self.dy, self.dz)
    }
    pub fn dt(&self) -> f64 {
        self.dt
    }
    pub fn courant(&self) -> f64 {
        self.courant
    }
    /// Steps completed since the grid was created.
    pub fn steps(&self) -> usize {
        self.steps
    }
    pub fn n_cells(&self) -> usize {
        self.nx * self.ny * self.nz
    }
    pub fn n_probes(&self) -> usize {
        self.probes.len()
    }
    /// Whether the last `run` was stopped early.
    pub fn interrupted(&self) -> bool {
        self.interrupted
    }
    /// Field storage, probe record, port V/I, material maps, baked
    /// coefficients, the PEC mask and, when a CPML face is on, the absorber
    /// arrays, in bytes.
    pub fn bytes(&self) -> usize {
        (6 * self.n_cells() + self.record.len() + self.port_v.len() + self.port_i.len()) * ELEM_F64
            + self.media.as_ref().map_or(0, |m| m.bytes())
            + self.baked.bytes()
            + self.pec.as_ref().map_or(0, |p| p.len())
            + self.absorber.bytes(self.cpml_active())
            + self.ntff_bytes()
    }

    /// Ports on the grid. v1 is 0 or 1.
    pub fn n_ports(&self) -> usize {
        self.ports.len()
    }

    /// Gap voltage, row-major `steps × n_ports`.
    pub fn port_voltage(&self) -> &[f64] {
        &self.port_v
    }

    /// Ampère current, row-major `steps × n_ports`. Not half-step corrected.
    pub fn port_current(&self) -> &[f64] {
        &self.port_i
    }

    /// Reference resistance of each port, in ohms, port order.
    pub fn port_z0(&self) -> Vec<f64> {
        self.ports.iter().map(|p| p.z0).collect()
    }

    /// Thickness per face, `[xmin, xmax, ymin, ymax, zmin, zmax]`. Zeros
    /// when the absorber is off.
    pub fn cpml_faces(&self) -> [usize; 6] {
        self.cpml.as_ref().map(|s| s.faces).unwrap_or([0; 6])
    }

    fn cpml_active(&self) -> bool {
        self.cpml_faces().iter().any(|&n| n > 0)
    }
    /// Row-major `steps × n_probes` probe record.
    pub fn record(&self) -> &[f64] {
        &self.record
    }

    #[inline]
    fn idx(&self, i: usize, j: usize, k: usize) -> usize {
        (k * self.ny + i) * self.nx + j
    }

    fn field(&self, c: Component) -> &[f64] {
        match c {
            Component::Ex => &self.ex,
            Component::Ey => &self.ey,
            Component::Ez => &self.ez,
            Component::Hx => &self.hx,
            Component::Hy => &self.hy,
            Component::Hz => &self.hz,
        }
    }

    /// Whether 0-based `(i, j, k)` is inside the grid.
    pub fn in_bounds(&self, i: usize, j: usize, k: usize) -> bool {
        i < self.ny && j < self.nx && k < self.nz
    }

    /// Whether the E edge of `comp` at 0-based `(i, j, k)` is tangential to a
    /// min wall, so it is held at 0 and never updated.
    pub fn is_wall_edge(&self, comp: Component, i: usize, j: usize, k: usize) -> bool {
        match comp {
            Component::Ex => i == 0 || k == 0,
            Component::Ey => j == 0 || k == 0,
            Component::Ez => i == 0 || j == 0,
            _ => false,
        }
    }

    fn cell_along(&self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.dx,
            Axis::Y => self.dy,
            Axis::Z => self.dz,
        }
    }

    /// Add an E-edge source on 0-based `edges` (each `(i, j, k)`). The caller
    /// checks bounds, the wall, collinearity, and `steps == 0`.
    pub fn add_source(
        &mut self,
        comp: Component,
        edges: &[(usize, usize, usize)],
        kind: SourceKind,
        waveform: Waveform,
    ) -> Result<(), DspError> {
        if !comp.is_electric() {
            return Err(DspError::InvalidParameter(format!(
                "sources drive E edges, not {}",
                comp.name()
            )));
        }
        if edges.is_empty() {
            return Err(DspError::InvalidParameter("source has no edges".into()));
        }
        if self.steps != 0 {
            return Err(DspError::InvalidParameter(
                "sources must be added before the first step".into(),
            ));
        }
        let mut flat = Vec::with_capacity(edges.len());
        for &(i, j, k) in edges {
            if !self.in_bounds(i, j, k)
                || self.is_wall_edge(comp, i, j, k)
                || self.is_pec_edge(comp, i, j, k)
            {
                return Err(DspError::InvalidParameter(format!(
                    "{} edge ({i}, {j}, {k}) is outside the grid or PEC",
                    comp.name()
                )));
            }
            flat.push(self.idx(i, j, k));
        }
        for s in &self.sources {
            if s.comp != comp {
                continue;
            }
            let shared = s.edges.iter().any(|e| flat.contains(e));
            if shared && (s.kind == SourceKind::Hard || kind == SourceKind::Hard) {
                return Err(DspError::InvalidParameter(
                    "a hard source cannot share an edge with another source".into(),
                ));
            }
        }
        for p in &self.ports {
            if p.comp == comp
                && kind == SourceKind::Hard
                && p.edges.iter().any(|e| flat.contains(e))
            {
                return Err(DspError::InvalidParameter(
                    "a hard source and a port cannot share an edge".into(),
                ));
            }
        }
        self.sources.push(Source {
            comp,
            kind,
            edges: flat,
            waveform,
        });
        Ok(())
    }

    /// Add the one lumped port on 0-based `edges` (each `(i, j, k)`).
    ///
    /// v1 accepts a single port. A second call errors and names the deferred
    /// S21 phase. `Z0` is the total gap resistance, finite and positive.
    /// The caller checks collinearity. Returns the 0-based port id.
    pub fn add_port(
        &mut self,
        comp: Component,
        edges: &[(usize, usize, usize)],
        z0: f64,
        waveform: Waveform,
    ) -> Result<usize, DspError> {
        if !comp.is_electric() {
            return Err(DspError::InvalidParameter(format!(
                "a port drives E edges, not {}",
                comp.name()
            )));
        }
        if edges.is_empty() {
            return Err(DspError::InvalidParameter("port has no edges".into()));
        }
        if !(z0.is_finite() && z0 > 0.0) {
            return Err(DspError::InvalidParameter(format!(
                "Z0 must be finite and positive, got {z0}"
            )));
        }
        // A second port is the deferred S21 phase even after the grid has
        // already stepped. The step check is for the first port only.
        if !self.ports.is_empty() {
            return Err(DspError::InvalidParameter(
                "one port in v1; S21 of a second port is deferred".into(),
            ));
        }
        if self.steps != 0 {
            return Err(DspError::InvalidParameter(
                "a port must be added before the first step".into(),
            ));
        }
        let mut flat = Vec::with_capacity(edges.len());
        for &(i, j, k) in edges {
            if !self.in_bounds(i, j, k)
                || self.is_wall_edge(comp, i, j, k)
                || self.is_pec_edge(comp, i, j, k)
            {
                return Err(DspError::InvalidParameter(format!(
                    "{} edge ({i}, {j}, {k}) is outside the grid or PEC",
                    comp.name()
                )));
            }
            flat.push(self.idx(i, j, k));
        }
        for s in &self.sources {
            if s.comp == comp
                && s.kind == SourceKind::Hard
                && s.edges.iter().any(|e| flat.contains(e))
            {
                return Err(DspError::InvalidParameter(
                    "a hard source and a port cannot share an edge".into(),
                ));
            }
        }
        self.ports.push(Port {
            comp,
            nodes: edges.to_vec(),
            edges: flat,
            z0,
            waveform,
        });
        self.rebake();
        Ok(0)
    }

    /// Add a probe at 0-based `(i, j, k)`; returns its 0-based column.
    pub fn add_probe(
        &mut self,
        comp: Component,
        i: usize,
        j: usize,
        k: usize,
    ) -> Result<usize, DspError> {
        if !self.in_bounds(i, j, k) {
            return Err(DspError::InvalidParameter(format!(
                "probe ({i}, {j}, {k}) is outside the grid"
            )));
        }
        if self.steps != 0 {
            return Err(DspError::InvalidParameter(
                "probes must be added before the first step".into(),
            ));
        }
        let idx = self.idx(i, j, k);
        self.probes.push(Probe { comp, idx });
        Ok(self.probes.len() - 1)
    }

    fn grid(&self) -> media::Grid {
        media::Grid {
            nx: self.nx,
            ny: self.ny,
            nz: self.nz,
        }
    }

    fn require_setup(&self, what: &str) -> Result<(), DspError> {
        if self.steps != 0 {
            return Err(DspError::InvalidParameter(format!(
                "{what} must be set before the first step"
            )));
        }
        Ok(())
    }

    /// Whether the E edge of `comp` at 0-based `(i, j, k)` is interior PEC.
    pub fn is_pec_edge(&self, comp: Component, i: usize, j: usize, k: usize) -> bool {
        let c = match comp {
            Component::Ex => 0,
            Component::Ey => 1,
            Component::Ez => 2,
            _ => return false,
        };
        self.pec
            .as_ref()
            .is_some_and(|p| p[self.idx(i, j, k)] & media::pec_bit(c) != 0)
    }

    /// Set the cell-centred materials, replacing any earlier call, then
    /// rebake the coefficients and `dt`. `eps_r` and `mu_r` finite and `> 0`;
    /// `sigma` (S/m) finite and `>= 0`. A [`CellMap::Cells`] map is x-fastest
    /// with one value per cell.
    ///
    /// `dt` is the CFL limit at the fastest phase speed,
    /// `c0 / sqrt(min eps_r · min mu_r)`, and never more than the vacuum
    /// limit.
    pub fn set_media(
        &mut self,
        eps_r: CellMap,
        mu_r: CellMap,
        sigma: CellMap,
    ) -> Result<(), DspError> {
        self.require_setup("materials")?;
        let n = self.n_cells();
        for (name, map, positive) in [
            ("eps_r", &eps_r, true),
            ("mu_r", &mu_r, true),
            ("sigma", &sigma, false),
        ] {
            if let CellMap::Cells(v) = map {
                if v.len() != n {
                    return Err(DspError::InvalidParameter(format!(
                        "{name} has {} cells, the grid has {n}",
                        v.len()
                    )));
                }
            }
            if let Some(&bad) = map
                .values()
                .iter()
                .find(|&&x| !(x.is_finite() && if positive { x > 0.0 } else { x >= 0.0 }))
            {
                let rule = if positive { "> 0" } else { ">= 0" };
                return Err(DspError::InvalidParameter(format!(
                    "{name} must be finite and {rule}, got {bad}"
                )));
            }
        }
        let p = eps_r.min() * mu_r.min();
        let speed = if p < 1.0 { C0 / p.sqrt() } else { C0 };
        let dt = cfl_dt_at(self.dx, self.dy, self.dz, self.courant, speed);
        if !(dt.is_finite() && dt > 0.0) {
            return Err(DspError::InvalidParameter(format!(
                "time step is not finite and positive (dt = {dt})"
            )));
        }
        self.dt = dt;
        self.media = Some(media::Media {
            eps: eps_r,
            mu: mu_r,
            sigma,
        });
        self.rebake();
        // `b` depends on `dt`. Rebuild the absorber after the step changes.
        self.rebake_cpml()?;
        Ok(())
    }

    /// Absorber on every face with thickness `n` cells. `n = 0` removes it.
    /// Replaces any earlier call (faces turned off before are on again).
    /// Does not change `dt`.
    pub fn set_cpml(&mut self, n: usize, params: CpmlParams) -> Result<(), DspError> {
        self.require_setup("CPML")?;
        params.validate()?;
        if n == 0 {
            self.cpml = None;
            self.absorber = cpml::Absorber::off(self.nx, self.ny, self.nz);
            debug_assert!(!self.absorber.is_full());
            return Ok(());
        }
        let faces = [n; 6];
        self.check_cpml_interior(&faces)?;
        let settings = cpml::CpmlSettings { n, faces, params };
        // Bake into a local first so a failure leaves the grid unchanged.
        let absorber = cpml::Absorber::bake(
            self.nx,
            self.ny,
            self.nz,
            (self.dx, self.dy, self.dz),
            self.dt,
            &settings,
        )?;
        self.cpml = Some(settings);
        self.absorber = absorber;
        debug_assert!(self.absorber.is_full());
        Ok(())
    }

    /// Turn one face off (`n = 0`, that side is a plain PEC wall) or back on
    /// (`n` must equal the current thickness). Needs an earlier [`FdtdSim::set_cpml`].
    pub fn set_cpml_face(&mut self, face: Face, n: usize) -> Result<(), DspError> {
        self.require_setup("CPML")?;
        let Some(settings) = self.cpml.clone() else {
            return Err(DspError::InvalidParameter(
                "call fdtd_cpml(sim, n) before turning a face on or off".into(),
            ));
        };
        if n != 0 && n != settings.n {
            return Err(DspError::InvalidParameter(format!(
                "a face takes 0 (off) or the absorber thickness {}, got {n}",
                settings.n
            )));
        }
        let mut faces = settings.faces;
        faces[face.index()] = n;
        self.check_cpml_interior(&faces)?;
        let settings = cpml::CpmlSettings { faces, ..settings };
        let absorber = cpml::Absorber::bake(
            self.nx,
            self.ny,
            self.nz,
            (self.dx, self.dy, self.dz),
            self.dt,
            &settings,
        )?;
        self.cpml = Some(settings);
        self.absorber = absorber;
        debug_assert_eq!(self.absorber.is_full(), self.cpml_active());
        Ok(())
    }

    fn check_cpml_interior(&self, faces: &[usize; 6]) -> Result<(), DspError> {
        for (axis, len, n_lo, n_hi) in [
            ("x", self.nx, faces[0], faces[1]),
            ("y", self.ny, faces[2], faces[3]),
            ("z", self.nz, faces[4], faces[5]),
        ] {
            let sum = n_lo.checked_add(n_hi);
            if sum.is_some_and(|s| s < len) {
                continue;
            }
            return Err(DspError::InvalidParameter(format!(
                "CPML of {n_lo} + {n_hi} cells leaves no interior on {axis} ({axis}: {len} cells); need fewer than {len} in total"
            )));
        }
        Ok(())
    }

    /// Rebuild the absorber from the current settings and `dt`, zeroing `q`.
    fn rebake_cpml(&mut self) -> Result<(), DspError> {
        let Some(settings) = self.cpml.clone() else {
            self.absorber = cpml::Absorber::off(self.nx, self.ny, self.nz);
            return Ok(());
        };
        self.absorber = cpml::Absorber::bake(
            self.nx,
            self.ny,
            self.nz,
            (self.dx, self.dy, self.dz),
            self.dt,
            &settings,
        )?;
        Ok(())
    }

    /// Make every E edge of each nonzero cell PEC. `cells` is x-fastest, one
    /// flag per cell.
    pub fn add_pec_volume(&mut self, cells: &[bool]) -> Result<(), DspError> {
        self.require_setup("PEC")?;
        if cells.len() != self.n_cells() {
            return Err(DspError::InvalidParameter(format!(
                "PEC mask has {} cells, the grid has {}",
                cells.len(),
                self.n_cells()
            )));
        }
        let g = self.grid();
        self.apply_pec(|pec| media::mark_volume(g, cells, pec))
    }

    /// Make the tangential E edges of one face PEC. `face` is the 0-based
    /// node plane on `axis`, `0 ..= n` (`n` is the max wall, already PEC).
    /// `nodes` is row-major in the [`FdtdSim::slice`] layout of that axis
    /// (`Z`: `ny × nx`, `Y`: `nx × nz`, `X`: `ny × nz`), one flag per node.
    /// An edge is PEC when both of its end nodes are set; a node on the max
    /// wall counts as set.
    pub fn add_pec_face(
        &mut self,
        axis: Axis,
        face: usize,
        nodes: &[bool],
    ) -> Result<(), DspError> {
        self.require_setup("PEC")?;
        let (limit, rows, cols) = match axis {
            Axis::X => (self.nx, self.ny, self.nz),
            Axis::Y => (self.ny, self.nx, self.nz),
            Axis::Z => (self.nz, self.ny, self.nx),
        };
        if face > limit {
            return Err(DspError::InvalidParameter(format!(
                "face index {face} out of range 0..={limit}"
            )));
        }
        if nodes.len() != rows * cols {
            return Err(DspError::InvalidParameter(format!(
                "face mask has {} nodes, expected {rows} × {cols}",
                nodes.len()
            )));
        }
        let g = self.grid();
        self.apply_pec(|pec| media::mark_face(g, axis, face, nodes, pec))
    }

    fn apply_pec(&mut self, mark: impl FnOnce(&mut [u8])) -> Result<(), DspError> {
        let mut pec = self
            .pec
            .clone()
            .unwrap_or_else(|| vec![0u8; self.n_cells()]);
        mark(&mut pec);
        for s in &self.sources {
            let bit = media::pec_bit(s.comp.axis() as usize);
            if s.edges.iter().any(|&e| pec[e] & bit != 0) {
                return Err(DspError::InvalidParameter(format!(
                    "PEC would cover an edge of an existing {} source",
                    s.comp.name()
                )));
            }
        }
        for p in &self.ports {
            let bit = media::pec_bit(p.comp.axis() as usize);
            if p.edges.iter().any(|&e| pec[e] & bit != 0) {
                return Err(DspError::InvalidParameter(format!(
                    "PEC would cover an edge of an existing {} port",
                    p.comp.name()
                )));
            }
        }
        if self.pec.is_none() && pec.iter().all(|&b| b == 0) {
            return Ok(());
        }
        self.pec = Some(pec);
        self.rebake();
        Ok(())
    }

    fn along_area(&self, comp: Component) -> (f64, f64) {
        match comp.axis() {
            Axis::X => (self.dx, self.dy * self.dz),
            Axis::Y => (self.dy, self.dx * self.dz),
            Axis::Z => (self.dz, self.dx * self.dy),
        }
    }

    /// Port-edge conductivity added on top of the material mean. Empty when
    /// there is no port, so the vacuum bake stays the Phase 1 kernel.
    fn port_extra_sigma(&self) -> Vec<(usize, usize, f64)> {
        let mut out = Vec::new();
        for p in &self.ports {
            let (along, area) = self.along_area(p.comp);
            let sig = port::edge_sigma(p.z0, p.edges.len(), along, area);
            let c = match p.comp {
                Component::Ex => 0,
                Component::Ey => 1,
                Component::Ez => 2,
                _ => continue,
            };
            for &e in &p.edges {
                out.push((c, e, sig));
            }
        }
        out
    }

    fn rebake(&mut self) {
        let extra = self.port_extra_sigma();
        self.baked = media::bake(
            self.grid(),
            self.media.as_ref(),
            self.pec.as_deref(),
            self.dt,
            &extra,
        );
    }

    /// Copy of one plane of `comp`, row-major `(rows, cols, data)`.
    ///
    /// `Z`, page `idx`: `ny × nx`. `Y`, row `idx`: `nx × nz`. `X`, column
    /// `idx`: `ny × nz`. Same shapes as squeezing the matching `Tensor3`
    /// slice `A(:, :, k)`, `A(i, :, :)`, `A(:, j, :)`.
    pub fn slice(
        &self,
        comp: Component,
        axis: Axis,
        idx: usize,
    ) -> Result<(usize, usize, Vec<f64>), DspError> {
        let (nx, ny, nz) = (self.nx, self.ny, self.nz);
        let limit = match axis {
            Axis::X => nx,
            Axis::Y => ny,
            Axis::Z => nz,
        };
        if idx >= limit {
            return Err(DspError::InvalidParameter(format!(
                "slice index {idx} out of range 0..{limit}"
            )));
        }
        let f = self.field(comp);
        Ok(match axis {
            Axis::Z => {
                let start = self.idx(0, 0, idx);
                (ny, nx, f[start..start + nx * ny].to_vec())
            }
            Axis::Y => {
                let mut out = Vec::with_capacity(nx * nz);
                for j in 0..nx {
                    for k in 0..nz {
                        out.push(f[self.idx(idx, j, k)]);
                    }
                }
                (nx, nz, out)
            }
            Axis::X => {
                let mut out = Vec::with_capacity(ny * nz);
                for i in 0..ny {
                    for k in 0..nz {
                        out.push(f[self.idx(i, idx, k)]);
                    }
                }
                (ny, nz, out)
            }
        })
    }

    #[cfg(test)]
    fn value_at(&self, comp: Component, i: usize, j: usize, k: usize) -> f64 {
        self.field(comp)[self.idx(i, j, k)]
    }

    /// Run up to `nsteps` steps. `stop` is called before every step; when it
    /// returns `true` the call ends with the fields, probes and step counter
    /// at the last finished step.
    pub fn run(
        &mut self,
        nsteps: usize,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<RunReport, DspError> {
        let total = self
            .steps
            .checked_add(nsteps)
            .ok_or(rustlab_core::CoreError::VariableTooLarge)?;
        checked_elems(&[total, self.probes.len()], ELEM_F64)?;
        checked_elems(&[total, self.ports.len()], ELEM_F64)?;
        self.record
            .try_reserve_exact(nsteps * self.probes.len())
            .map_err(|_| rustlab_core::CoreError::VariableTooLarge)?;
        let port_samples = nsteps * self.ports.len();
        self.port_v
            .try_reserve_exact(port_samples)
            .map_err(|_| rustlab_core::CoreError::VariableTooLarge)?;
        self.port_i
            .try_reserve_exact(port_samples)
            .map_err(|_| rustlab_core::CoreError::VariableTooLarge)?;

        self.interrupted = false;
        let mut completed = 0;
        while completed < nsteps {
            if stop() {
                self.interrupted = true;
                break;
            }
            // Resolve the Huygens box before any field is written, so a
            // rejected surface leaves steps at 0.
            if self.steps == 0 {
                self.prepare_ntff()?;
            }
            self.step();
            completed += 1;
        }
        Ok(RunReport {
            completed,
            stopped: self.interrupted,
        })
    }

    fn step(&mut self) {
        let dims = yee::Dims {
            nx: self.nx,
            ny: self.ny,
            nz: self.nz,
        };
        let db = yee::Coeffs {
            x: self.dt / (MU0 * self.dx),
            y: self.dt / (MU0 * self.dy),
            z: self.dt / (MU0 * self.dz),
        };
        let cb = yee::Coeffs {
            x: self.dt / (EPS0 * self.dx),
            y: self.dt / (EPS0 * self.dy),
            z: self.dt / (EPS0 * self.dz),
        };
        let np = self.probes.len();
        let row0 = self.record.len();
        self.record.resize(row0 + np, 0.0);
        // Copied out so the correction call is a branch around the Phase 2
        // update, not a parameter the update has to read.
        let cpml_on = self.cpml_active();
        let faces = self.cpml_faces();
        let parallel = self.parallel_step();

        // Split the grid so the absorber profiles, the `q` rows and the six
        // fields can be borrowed together. `HCoefs` / `ECoefs` hold the
        // material coefficients for the whole step. Order: plain H update,
        // H slab correction, H probes, plain E update, E slab correction.
        {
            let FdtdSim {
                absorber,
                ex,
                ey,
                ez,
                hx,
                hy,
                hz,
                zero_row,
                baked,
                probes,
                record,
                ..
            } = &mut *self;
            let hm = yee::HCoefs {
                db: baked.db.as_ref().map(|a| [&a[0][..], &a[1][..], &a[2][..]]),
            };
            let em = yee::ECoefs {
                cb: baked.cb.as_ref().map(|a| [&a[0][..], &a[1][..], &a[2][..]]),
                ca: baked.ca.as_ref().map(|a| [&a[0][..], &a[1][..], &a[2][..]]),
            };
            yee::update_h(dims, db, hm, zero_row, ex, ey, ez, hx, hy, hz, parallel);
            if cpml_on {
                let cpml::Absorber { h, q_h, .. } = absorber;
                let axes = yee::Axes {
                    x: &h[0],
                    y: &h[1],
                    z: &h[2],
                };
                yee::correct_h(
                    dims, db, hm, axes, faces, q_h, zero_row, ex, ey, ez, hx, hy, hz, parallel,
                );
            }
            for (p, probe) in probes.iter().enumerate() {
                if !probe.comp.is_electric() {
                    let src: &[f64] = match probe.comp {
                        Component::Hx => hx,
                        Component::Hy => hy,
                        Component::Hz => hz,
                        _ => unreachable!(),
                    };
                    record[row0 + p] = src[probe.idx];
                }
            }
            yee::update_e(dims, cb, em, hx, hy, hz, ex, ey, ez, parallel);
            if cpml_on {
                let cpml::Absorber { e, q_e, .. } = absorber;
                let axes = yee::Axes {
                    x: &e[0],
                    y: &e[1],
                    z: &e[2],
                };
                yee::correct_e(
                    dims, cb, em, axes, faces, q_e, hx, hy, hz, ex, ey, ez, parallel,
                );
            }
        }
        self.steps += 1;
        self.apply_sources();
        self.apply_port();
        for (p, probe) in self.probes.iter().enumerate() {
            if probe.comp.is_electric() {
                let v = self.field(probe.comp)[probe.idx];
                self.record[row0 + p] = v;
            }
        }
        self.record_port();
        // NTFF off skips this. The branch does not write a field.
        self.accumulate_ntff();
    }

    /// Series voltage on the port edges, after the E update. A grid with no
    /// port returns immediately and does not touch the fields.
    fn apply_port(&mut self) {
        if self.ports.is_empty() {
            return;
        }
        let step = self.steps;
        let dt = self.dt;
        let comp = self.ports[0].comp;
        let z0 = self.ports[0].z0;
        let Some(vg) = self.ports[0].waveform.value(step, dt) else {
            return;
        };
        let edges = self.ports[0].edges.clone();
        let c = match comp {
            Component::Ex => 0,
            Component::Ey => 1,
            Component::Ez => 2,
            _ => return,
        };
        let area = self.along_area(comp).1;
        let rel: Vec<f64> = match &self.baked.cb {
            None => vec![1.0; edges.len()],
            Some(cb) => edges.iter().map(|&e| cb[c][e]).collect(),
        };
        let field = match comp {
            Component::Ex => &mut self.ex,
            Component::Ey => &mut self.ey,
            _ => &mut self.ez,
        };
        for (&e, r) in edges.iter().zip(rel) {
            field[e] -= port::series_delta(r, dt, vg, z0, area);
        }
    }

    fn record_port(&mut self) {
        if self.ports.is_empty() {
            return;
        }
        let comp = self.ports[0].comp;
        let (i, j, k) = self.ports[0].nodes[0];
        let edges = self.ports[0].edges.clone();
        let delta = self.cell_along(comp.axis());
        let sum: f64 = {
            let f = self.field(comp);
            edges.iter().map(|&e| f[e]).sum()
        };
        let v = -sum * delta;
        let amp = self.ampere_at(comp, i, j, k);
        self.port_v.push(v);
        self.port_i.push(amp);
    }

    fn ampere_at(&self, comp: Component, i: usize, j: usize, k: usize) -> f64 {
        let (dx, dy, dz) = (self.dx, self.dy, self.dz);
        match comp {
            Component::Ez => port::ampere_ez(
                self.hy[self.idx(i, j, k)],
                self.hy[self.idx(i, j - 1, k)],
                self.hx[self.idx(i, j, k)],
                self.hx[self.idx(i - 1, j, k)],
                dx,
                dy,
            ),
            Component::Ex => port::ampere_ex(
                self.hz[self.idx(i, j, k)],
                self.hz[self.idx(i - 1, j, k)],
                self.hy[self.idx(i, j, k)],
                self.hy[self.idx(i, j, k - 1)],
                dy,
                dz,
            ),
            Component::Ey => port::ampere_ey(
                self.hx[self.idx(i, j, k)],
                self.hx[self.idx(i, j, k - 1)],
                self.hz[self.idx(i, j, k)],
                self.hz[self.idx(i, j - 1, k)],
                dx,
                dz,
            ),
            _ => 0.0,
        }
    }

    fn apply_sources(&mut self) {
        let step = self.steps;
        for s in &self.sources {
            let Some(g) = s.waveform.value(step, self.dt) else {
                continue;
            };
            let delta = self.cell_along(s.comp.axis());
            let per_edge = g / (s.edges.len() as f64 * delta);
            let (field, c) = match s.comp {
                Component::Ex => (&mut self.ex, 0),
                Component::Ey => (&mut self.ey, 1),
                _ => (&mut self.ez, 2),
            };
            match s.kind {
                SourceKind::Soft => {
                    let cb = self.dt / EPS0;
                    match &self.baked.cb {
                        None => {
                            for &e in &s.edges {
                                field[e] -= cb * per_edge;
                            }
                        }
                        Some(rel) => {
                            for &e in &s.edges {
                                field[e] -= cb * rel[c][e] * per_edge;
                            }
                        }
                    }
                }
                SourceKind::Hard => {
                    for &e in &s.edges {
                        field[e] = -per_edge;
                    }
                }
            }
        }
    }

    /// Discrete field energy `½ Σ (ε0 E² + μ0 H²) ΔV`.
    #[cfg(test)]
    fn energy(&self) -> f64 {
        let dv = self.dx * self.dy * self.dz;
        let e2: f64 = [&self.ex, &self.ey, &self.ez]
            .iter()
            .flat_map(|v| v.iter())
            .map(|x| x * x)
            .sum();
        let h2: f64 = [&self.hx, &self.hy, &self.hz]
            .iter()
            .flat_map(|v| v.iter())
            .map(|x| x * x)
            .sum();
        0.5 * (EPS0 * e2 + MU0 * h2) * dv
    }

    #[cfg(test)]
    fn field_bits(&self) -> Vec<u64> {
        [&self.ex, &self.ey, &self.ez, &self.hx, &self.hy, &self.hz]
            .iter()
            .flat_map(|v| v.iter().map(|x| x.to_bits()))
            .collect()
    }
}

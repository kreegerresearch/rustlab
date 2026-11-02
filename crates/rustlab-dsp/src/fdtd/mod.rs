//! 3-D FDTD (Yee) time stepper.
//!
//! Phase 1 of `dev/plans/fdtd3d.md`: vacuum, PEC outer walls, soft and hard
//! E-edge sources, point probes. Serial.
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
//! E to `s·dt`, applies sources with the waveform at `s·dt`, then records E
//! probes. Soft: `E ← E − (dt/ε0)·g/Δ` per edge. Hard: `E ← −g/Δ`. `Δ` is the
//! cell size along the component; on a stack of `N` edges `g` is split as
//! `g/N`. Both signs make a positive `g` a positive `V = −Σ E·Δ`.

mod pulse;
mod yee;

#[cfg(test)]
mod tests;

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

/// Result of [`FdtdSim::run`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunReport {
    /// Steps completed by this call.
    pub completed: usize,
    /// `true` when the stop callback ended the call early.
    pub stopped: bool,
}

/// One FDTD grid: fields, sources, probes and the probe record.
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
    /// Row-major `steps × probes.len()`.
    record: Vec<f64>,
    steps: usize,
    interrupted: bool,
}

/// `courant / (c0 sqrt(1/dx² + 1/dy² + 1/dz²))`.
pub fn cfl_dt(dx: f64, dy: f64, dz: f64, courant: f64) -> f64 {
    courant / (C0 * (1.0 / (dx * dx) + 1.0 / (dy * dy) + 1.0 / (dz * dz)).sqrt())
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
            record: Vec::new(),
            steps: 0,
            interrupted: false,
        })
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
    /// Field storage plus the probe record, in bytes.
    pub fn bytes(&self) -> usize {
        (6 * self.n_cells() + self.record.len()) * ELEM_F64
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
            if !self.in_bounds(i, j, k) || self.is_wall_edge(comp, i, j, k) {
                return Err(DspError::InvalidParameter(format!(
                    "{} edge ({i}, {j}, {k}) is outside the grid or on a PEC wall",
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
        self.sources.push(Source {
            comp,
            kind,
            edges: flat,
            waveform,
        });
        Ok(())
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
        self.record
            .try_reserve_exact(nsteps * self.probes.len())
            .map_err(|_| rustlab_core::CoreError::VariableTooLarge)?;

        self.interrupted = false;
        let mut completed = 0;
        while completed < nsteps {
            if stop() {
                self.interrupted = true;
                break;
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

        yee::update_h(
            dims,
            db,
            &self.zero_row,
            &self.ex,
            &self.ey,
            &self.ez,
            &mut self.hx,
            &mut self.hy,
            &mut self.hz,
        );
        for (p, probe) in self.probes.iter().enumerate() {
            if !probe.comp.is_electric() {
                let v = self.field(probe.comp)[probe.idx];
                self.record[row0 + p] = v;
            }
        }

        yee::update_e(
            dims,
            cb,
            &self.hx,
            &self.hy,
            &self.hz,
            &mut self.ex,
            &mut self.ey,
            &mut self.ez,
        );
        self.steps += 1;
        self.apply_sources();
        for (p, probe) in self.probes.iter().enumerate() {
            if probe.comp.is_electric() {
                let v = self.field(probe.comp)[probe.idx];
                self.record[row0 + p] = v;
            }
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
            let field = match s.comp {
                Component::Ex => &mut self.ex,
                Component::Ey => &mut self.ey,
                _ => &mut self.ez,
            };
            match s.kind {
                SourceKind::Soft => {
                    let cb = self.dt / EPS0;
                    for &e in &s.edges {
                        field[e] -= cb * per_edge;
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

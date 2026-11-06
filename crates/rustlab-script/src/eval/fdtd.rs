//! Script builtins for the 3-D FDTD stepper (`rustlab_dsp::fdtd`).
//!
//! The grid lives in `Value::Fdtd3d`. Every call locks it, copies what it
//! returns into plain vectors, and builds the `Value`s after the lock drops.
//! Indices are 1-based `(i, j, k)` = (y row, x column, z page).

use super::builtin_cancel;
use super::value::Value;
use crate::error::ScriptError;
use ndarray::{Array1, Array2};
use rustlab_dsp::error::DspError;
use rustlab_dsp::fdtd::{
    gaussian_pulse, Axis, CellMap, Component, CpmlParams, Face, FdtdSim, SourceKind, Waveform, C0,
    DEFAULT_COURANT,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

fn err(name: &str, msg: impl std::fmt::Display) -> ScriptError {
    ScriptError::runtime(format!("{name}: {msg}"))
}

fn dsp_err(name: &str, e: DspError) -> ScriptError {
    match e {
        DspError::InvalidParameter(msg) => err(name, msg),
        DspError::Core(c) => ScriptError::Core(c),
        other => err(name, other),
    }
}

fn sim_arg(name: &str, v: &Value) -> Result<Arc<Mutex<FdtdSim>>, ScriptError> {
    match v {
        Value::Fdtd3d(s) => Ok(Arc::clone(s)),
        other => Err(ScriptError::type_err(format!(
            "{name}: first argument must be an fdtd3d handle, got {}",
            other.type_name()
        ))),
    }
}

fn lock<'a>(name: &str, s: &'a Mutex<FdtdSim>) -> Result<MutexGuard<'a, FdtdSim>, ScriptError> {
    s.lock()
        .map_err(|_| err(name, "the simulation is unusable after an earlier panic"))
}

fn real_scalar(name: &str, what: &str, v: &Value) -> Result<f64, ScriptError> {
    match v {
        Value::Scalar(_) | Value::Matrix(_) | Value::Vector(_) | Value::Complex(_) => v
            .to_scalar()
            .map_err(|e| ScriptError::type_err(format!("{name}: {what}: {e}"))),
        other => Err(ScriptError::type_err(format!(
            "{name}: {what} must be a real scalar, got {}",
            other.type_name()
        ))),
    }
}

fn finite_scalar(name: &str, what: &str, v: &Value) -> Result<f64, ScriptError> {
    let x = real_scalar(name, what, v)?;
    if !x.is_finite() {
        return Err(err(name, format!("{what} must be finite, got {x}")));
    }
    Ok(x)
}

fn positive_scalar(name: &str, what: &str, v: &Value) -> Result<f64, ScriptError> {
    let x = real_scalar(name, what, v)?;
    if !(x.is_finite() && x > 0.0) {
        return Err(err(
            name,
            format!("{what} must be finite and positive, got {x}"),
        ));
    }
    Ok(x)
}

fn positive_int(name: &str, what: &str, v: &Value) -> Result<usize, ScriptError> {
    let x = real_scalar(name, what, v)?;
    match Value::count_from_f64(x) {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(err(
            name,
            format!("{what} must be a positive integer, got {x}"),
        )),
    }
}

fn nonneg_int(name: &str, what: &str, v: &Value) -> Result<usize, ScriptError> {
    let x = real_scalar(name, what, v)?;
    match Value::count_from_f64(x) {
        Ok(n) => Ok(n),
        _ => Err(err(
            name,
            format!("{what} must be a non-negative integer, got {x}"),
        )),
    }
}

fn str_arg(name: &str, what: &str, v: &Value) -> Result<String, ScriptError> {
    v.to_str().map_err(|_| {
        ScriptError::type_err(format!(
            "{name}: {what} must be a string, got {}",
            v.type_name()
        ))
    })
}

fn component_arg(name: &str, v: &Value) -> Result<Component, ScriptError> {
    let s = str_arg(name, "component", v)?;
    Component::parse(&s).ok_or_else(|| {
        err(
            name,
            format!("unknown component \"{s}\" (use ex, ey, ez, hx, hy, hz)"),
        )
    })
}

/// Real values and shape of a scalar, vector, or matrix (row-major).
fn real_values(name: &str, what: &str, v: &Value) -> Result<(Vec<f64>, Vec<usize>), ScriptError> {
    use super::shared::Shared;
    let not_real = || err(name, format!("{what} must be real"));
    match v {
        Value::Scalar(x) => Ok((vec![*x], vec![])),
        Value::Vector(a) => {
            let data = match Shared::as_real(a) {
                Some(r) => r.to_vec(),
                None => {
                    let mut out = Vec::with_capacity(a.len());
                    for c in a.iter() {
                        if c.im != 0.0 {
                            return Err(not_real());
                        }
                        out.push(c.re);
                    }
                    out
                }
            };
            let n = data.len();
            Ok((data, vec![n]))
        }
        Value::Matrix(m) => {
            let shape = vec![m.nrows(), m.ncols()];
            let data = match Shared::as_real(m) {
                Some(r) => r.iter().copied().collect(),
                None => {
                    let mut out = Vec::with_capacity(m.len());
                    for c in m.iter() {
                        if c.im != 0.0 {
                            return Err(not_real());
                        }
                        out.push(c.re);
                    }
                    out
                }
            };
            Ok((data, shape))
        }
        Value::Complex(c) if c.im == 0.0 => Ok((vec![c.re], vec![])),
        Value::Complex(_) => Err(not_real()),
        other => Err(ScriptError::type_err(format!(
            "{name}: {what} must be a real scalar or vector, got {}",
            other.type_name()
        ))),
    }
}

/// 1-based index list (scalar or vector of integers in `1..=limit`), 0-based out.
fn index_list(name: &str, what: &str, v: &Value, limit: usize) -> Result<Vec<usize>, ScriptError> {
    let (vals, shape) = real_values(name, what, v)?;
    if shape.len() == 2 && shape[0] != 1 && shape[1] != 1 {
        return Err(err(name, format!("{what} must be a scalar or a vector")));
    }
    if vals.is_empty() {
        return Err(err(name, format!("{what} is empty")));
    }
    vals.iter()
        .map(|&x| match Value::count_from_f64(x) {
            Ok(n) if (1..=limit).contains(&n) => Ok(n - 1),
            _ => Err(err(
                name,
                format!("{what} must be an integer in 1..{limit}, got {x}"),
            )),
        })
        .collect()
}

fn index_one(name: &str, what: &str, v: &Value, limit: usize) -> Result<usize, ScriptError> {
    let idx = index_list(name, what, v, limit)?;
    if idx.len() != 1 {
        return Err(err(name, format!("{what} must be a scalar")));
    }
    Ok(idx[0])
}

fn record_fields(sim: &FdtdSim) -> HashMap<String, Value> {
    let steps = sim.steps();
    let np = sim.n_probes();
    let dt = sim.dt();
    let t = Array1::from_iter((1..=steps).map(|n| n as f64 * dt));
    let probe = Array2::from_shape_vec((steps, np), sim.record().to_vec())
        .expect("record is steps × probes");
    let mut f = HashMap::new();
    f.insert("t".to_string(), Value::real_vector(t));
    f.insert("dt".to_string(), Value::Scalar(dt));
    f.insert("steps".to_string(), Value::Scalar(steps as f64));
    f.insert("probe".to_string(), Value::real_matrix(probe));
    f.insert(
        "V".to_string(),
        Value::real_matrix(Array2::zeros((steps, 0))),
    );
    f.insert(
        "I".to_string(),
        Value::real_matrix(Array2::zeros((steps, 0))),
    );
    f.insert("Z0".to_string(), Value::real_vector(Array1::zeros(0)));
    f.insert("interrupted".to_string(), Value::Bool(sim.interrupted()));
    f
}

/// `sim = fdtd3d(nx, ny, nz, dx, dy, dz [, "courant", s])`
pub(crate) fn builtin_fdtd3d(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd3d";
    if args.len() != 6 && args.len() != 8 {
        return Err(ScriptError::arg_count_range(NAME.into(), 6, 8, args.len()));
    }
    let nx = positive_int(NAME, "nx", &args[0])?;
    let ny = positive_int(NAME, "ny", &args[1])?;
    let nz = positive_int(NAME, "nz", &args[2])?;
    let dx = positive_scalar(NAME, "dx", &args[3])?;
    let dy = positive_scalar(NAME, "dy", &args[4])?;
    let dz = positive_scalar(NAME, "dz", &args[5])?;
    let mut courant = DEFAULT_COURANT;
    if args.len() == 8 {
        let key = str_arg(NAME, "option name", &args[6])?;
        if !key.eq_ignore_ascii_case("courant") {
            return Err(err(
                NAME,
                format!("unknown option \"{key}\" (expected \"courant\")"),
            ));
        }
        courant = real_scalar(NAME, "courant", &args[7])?;
    }
    let sim = FdtdSim::new(nx, ny, nz, dx, dy, dz, courant).map_err(|e| dsp_err(NAME, e))?;
    Ok(Value::Fdtd3d(Arc::new(Mutex::new(sim))))
}

/// `info = fdtd_info(sim)`
pub(crate) fn builtin_fdtd_info(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd_info";
    if args.len() != 1 {
        return Err(ScriptError::arg_count(NAME.into(), 1, args.len()));
    }
    let s = sim_arg(NAME, &args[0])?;
    let g = lock(NAME, &s)?;
    let (dx, dy, dz) = g.spacing();
    let mut f = HashMap::new();
    for (k, v) in [
        ("nx", g.nx() as f64),
        ("ny", g.ny() as f64),
        ("nz", g.nz() as f64),
        ("dx", dx),
        ("dy", dy),
        ("dz", dz),
        ("dt", g.dt()),
        ("courant", g.courant()),
        ("steps", g.steps() as f64),
        ("bytes", g.bytes() as f64),
        ("c0", C0),
    ] {
        f.insert(k.to_string(), Value::Scalar(v));
    }
    let faces = g.cpml_faces();
    f.insert(
        "cpml".to_string(),
        Value::real_vector(Array1::from_vec(faces.map(|n| n as f64).to_vec())),
    );
    Ok(Value::Struct(f))
}

/// `y = gaussian_pulse(t, t0, tau [, f0])`
pub(crate) fn builtin_gaussian_pulse(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "gaussian_pulse";
    if !(3..=4).contains(&args.len()) {
        return Err(ScriptError::arg_count_range(NAME.into(), 3, 4, args.len()));
    }
    let t0 = finite_scalar(NAME, "t0", &args[1])?;
    let tau = positive_scalar(NAME, "tau", &args[2])?;
    let f0 = match args.get(3) {
        Some(v) => finite_scalar(NAME, "f0", v)?,
        None => 0.0,
    };
    let (t, shape) = real_values(NAME, "t", &args[0])?;
    let y: Vec<f64> = t.iter().map(|&t| gaussian_pulse(t, t0, tau, f0)).collect();
    Ok(match shape.as_slice() {
        [] => Value::Scalar(y[0]),
        [_] => Value::real_vector(Array1::from_vec(y)),
        [r, c] => Value::real_matrix(
            Array2::from_shape_vec((*r, *c), y).expect("same element count as t"),
        ),
        _ => unreachable!("real_values returns rank ≤ 2"),
    })
}

/// `fdtd_source(sim, comp, i, j, k, "soft"|"hard", waveform…)`
pub(crate) fn builtin_fdtd_source(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd_source";
    if !(7..=10).contains(&args.len()) {
        return Err(ScriptError::arg_count_range(NAME.into(), 7, 10, args.len()));
    }
    let s = sim_arg(NAME, &args[0])?;
    let comp = component_arg(NAME, &args[1])?;
    if !comp.is_electric() {
        return Err(err(
            NAME,
            format!("sources drive E edges (ex, ey, ez), not {}", comp.name()),
        ));
    }
    let kind = match str_arg(NAME, "source type", &args[5])?
        .to_ascii_lowercase()
        .as_str()
    {
        "soft" => SourceKind::Soft,
        "hard" => SourceKind::Hard,
        other => {
            return Err(err(
                NAME,
                format!("source type must be \"soft\" or \"hard\", got \"{other}\""),
            ))
        }
    };
    let waveform = match &args[6] {
        Value::Str(w) => {
            let w = w.to_ascii_lowercase();
            let want = match w.as_str() {
                "gaussian" => 9,
                "modulated" => 10,
                _ => {
                    return Err(err(
                        NAME,
                        format!("waveform must be \"gaussian\", \"modulated\", or a real vector, got \"{w}\""),
                    ))
                }
            };
            if args.len() != want {
                return Err(err(
                    NAME,
                    format!(
                        "\"{w}\" takes {} arguments after the waveform name, got {}",
                        want - 7,
                        args.len() - 7
                    ),
                ));
            }
            let t0 = finite_scalar(NAME, "t0", &args[7])?;
            let tau = positive_scalar(NAME, "tau", &args[8])?;
            let f0 = if want == 10 {
                finite_scalar(NAME, "f0", &args[9])?
            } else {
                0.0
            };
            Waveform::Pulse { t0, tau, f0 }
        }
        v => {
            if args.len() != 7 {
                return Err(ScriptError::arg_count(NAME.into(), 7, args.len()));
            }
            let (samples, shape) = real_values(NAME, "waveform", v)?;
            if shape.len() == 2 && shape[0] != 1 && shape[1] != 1 {
                return Err(err(NAME, "waveform must be a vector"));
            }
            if samples.is_empty() || samples.iter().any(|x| !x.is_finite()) {
                return Err(err(
                    NAME,
                    "waveform must be a non-empty vector of finite values",
                ));
            }
            Waveform::Samples(samples)
        }
    };

    let mut g = lock(NAME, &s)?;
    if g.steps() != 0 {
        return Err(err(
            NAME,
            format!(
                "add sources before the first step ({} steps done); build a new fdtd3d",
                g.steps()
            ),
        ));
    }
    let rows = index_list(NAME, "i", &args[2], g.ny())?;
    let cols = index_list(NAME, "j", &args[3], g.nx())?;
    let pages = index_list(NAME, "k", &args[4], g.nz())?;
    let (along_name, along) = match comp.axis() {
        Axis::X => ("j", &cols),
        Axis::Y => ("i", &rows),
        Axis::Z => ("k", &pages),
    };
    for (n, list) in [("i", &rows), ("j", &cols), ("k", &pages)] {
        if list.len() > 1 && n != along_name {
            return Err(err(
                NAME,
                format!(
                    "only {along_name} may be a range for {} (a stack of collinear edges); {n} has {} values",
                    comp.name(),
                    list.len()
                ),
            ));
        }
    }
    if along.windows(2).any(|w| w[1] != w[0] + 1) {
        return Err(err(
            NAME,
            format!("{along_name} must be consecutive ascending indices"),
        ));
    }
    let edges: Vec<(usize, usize, usize)> = match comp.axis() {
        Axis::X => cols.iter().map(|&j| (rows[0], j, pages[0])).collect(),
        Axis::Y => rows.iter().map(|&i| (i, cols[0], pages[0])).collect(),
        Axis::Z => pages.iter().map(|&k| (rows[0], cols[0], k)).collect(),
    };
    for &(i, j, k) in &edges {
        if g.is_pec_edge(comp, i, j, k) {
            return Err(err(
                NAME,
                format!(
                    "{} at (i, j, k) = ({}, {}, {}) is PEC (fdtd_pec) and held at 0",
                    comp.name(),
                    i + 1,
                    j + 1,
                    k + 1
                ),
            ));
        }
        if g.is_wall_edge(comp, i, j, k) {
            return Err(err(
                NAME,
                format!(
                    "{} at (i, j, k) = ({}, {}, {}) lies on a PEC outer wall and is held at 0",
                    comp.name(),
                    i + 1,
                    j + 1,
                    k + 1
                ),
            ));
        }
    }
    g.add_source(comp, &edges, kind, waveform)
        .map_err(|e| dsp_err(NAME, e))?;
    Ok(Value::None)
}

/// `id = fdtd_probe(sim, comp, i, j, k)`
pub(crate) fn builtin_fdtd_probe(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd_probe";
    if args.len() != 5 {
        return Err(ScriptError::arg_count(NAME.into(), 5, args.len()));
    }
    let s = sim_arg(NAME, &args[0])?;
    let comp = component_arg(NAME, &args[1])?;
    let mut g = lock(NAME, &s)?;
    if g.steps() != 0 {
        return Err(err(
            NAME,
            format!(
                "add probes before the first step ({} steps done); build a new fdtd3d",
                g.steps()
            ),
        ));
    }
    let i = index_one(NAME, "i", &args[2], g.ny())?;
    let j = index_one(NAME, "j", &args[3], g.nx())?;
    let k = index_one(NAME, "k", &args[4], g.nz())?;
    let id = g.add_probe(comp, i, j, k).map_err(|e| dsp_err(NAME, e))?;
    Ok(Value::Scalar((id + 1) as f64))
}

fn run_steps(name: &str, sim: &Value, nsteps: usize) -> Result<Value, ScriptError> {
    let s = sim_arg(name, sim)?;
    let mut g = lock(name, &s)?;
    // One step is short; arming the Ctrl+C handler per call would cost a
    // signal-disposition swap on every `fdtd_step(sim)` in a script loop.
    let armed = nsteps > 1;
    let _guard = armed.then(rustlab_plot::InterruptGuard::arm);
    let mut stop =
        || builtin_cancel::cancel_requested() || (armed && rustlab_plot::interrupt_requested());
    let start = Instant::now();
    let report = g.run(nsteps, &mut stop).map_err(|e| dsp_err(name, e))?;
    let seconds = start.elapsed().as_secs_f64();
    if report.stopped && builtin_cancel::cancel_requested() {
        return Err(ScriptError::Cancelled);
    }
    let cell_updates = (g.n_cells() * report.completed) as f64;
    let mut f = record_fields(&g);
    drop(g);
    if report.stopped {
        eprintln!("interrupted");
    }
    f.insert("seconds".to_string(), Value::Scalar(seconds));
    f.insert("cell_updates".to_string(), Value::Scalar(cell_updates));
    let rate = if seconds > 0.0 {
        cell_updates / seconds
    } else {
        0.0
    };
    f.insert("cell_rate".to_string(), Value::Scalar(rate));
    Ok(Value::Struct(f))
}

/// `out = fdtd_run(sim, nsteps)`
pub(crate) fn builtin_fdtd_run(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd_run";
    if args.len() != 2 {
        return Err(ScriptError::arg_count(NAME.into(), 2, args.len()));
    }
    let n = positive_int(NAME, "nsteps", &args[1])?;
    run_steps(NAME, &args[0], n)
}

/// `out = fdtd_step(sim [, n])`
pub(crate) fn builtin_fdtd_step(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd_step";
    if !(1..=2).contains(&args.len()) {
        return Err(ScriptError::arg_count_range(NAME.into(), 1, 2, args.len()));
    }
    let n = match args.get(1) {
        Some(v) => positive_int(NAME, "n", v)?,
        None => 1,
    };
    run_steps(NAME, &args[0], n)
}

/// `rec = fdtd_record(sim)`
pub(crate) fn builtin_fdtd_record(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd_record";
    if args.len() != 1 {
        return Err(ScriptError::arg_count(NAME.into(), 1, args.len()));
    }
    let s = sim_arg(NAME, &args[0])?;
    let g = lock(NAME, &s)?;
    let f = record_fields(&g);
    drop(g);
    Ok(Value::Struct(f))
}

/// `M = fdtd_slice(sim, comp, axis, index)`
pub(crate) fn builtin_fdtd_slice(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd_slice";
    if args.len() != 4 {
        return Err(ScriptError::arg_count(NAME.into(), 4, args.len()));
    }
    let s = sim_arg(NAME, &args[0])?;
    let comp = component_arg(NAME, &args[1])?;
    let axis_s = str_arg(NAME, "axis", &args[2])?;
    let axis = Axis::parse(&axis_s).ok_or_else(|| {
        err(
            NAME,
            format!("axis must be \"x\", \"y\", or \"z\", got \"{axis_s}\""),
        )
    })?;
    let g = lock(NAME, &s)?;
    let (what, limit) = match axis {
        Axis::X => ("x index (column j)", g.nx()),
        Axis::Y => ("y index (row i)", g.ny()),
        Axis::Z => ("z index (page k)", g.nz()),
    };
    let idx = index_one(NAME, what, &args[3], limit)?;
    let (rows, cols, data) = g.slice(comp, axis, idx).map_err(|e| dsp_err(NAME, e))?;
    drop(g);
    let m = Array2::from_shape_vec((rows, cols), data).expect("slice is rows × cols");
    Ok(Value::real_matrix(m))
}

fn setup_check(name: &str, what: &str, g: &FdtdSim) -> Result<(), ScriptError> {
    if g.steps() != 0 {
        return Err(err(
            name,
            format!(
                "set {what} before the first step ({} steps done); build a new fdtd3d",
                g.steps()
            ),
        ));
    }
    Ok(())
}

/// Values of a `(ny, nx, nz)` Tensor3, x-fastest like the grid.
fn tensor_cells(
    name: &str,
    what: &str,
    v: &Value,
    (ny, nx, nz): (usize, usize, usize),
) -> Result<Vec<f64>, ScriptError> {
    let not_real = || {
        if what == "eps_r" {
            err(name, EPS_NOT_REAL)
        } else {
            err(name, format!("{what} must be real"))
        }
    };
    use super::shared::Shared;
    let Value::Tensor3(t) = v else {
        unreachable!("caller matched Tensor3")
    };
    let shape = Shared::shape(t);
    if shape != [ny, nx, nz] {
        return Err(err(
            name,
            format!(
                "{what} must be {ny} × {nx} × {nz} (ny × nx × nz), got {} × {} × {}",
                shape[0], shape[1], shape[2]
            ),
        ));
    }
    let mut out = Vec::with_capacity(ny * nx * nz);
    match Shared::as_real(t) {
        Some(r) => {
            for k in 0..nz {
                for i in 0..ny {
                    for j in 0..nx {
                        out.push(r[[i, j, k]]);
                    }
                }
            }
        }
        None => {
            for k in 0..nz {
                for i in 0..ny {
                    for j in 0..nx {
                        let c = t[[i, j, k]];
                        if c.im != 0.0 {
                            return Err(not_real());
                        }
                        out.push(c.re);
                    }
                }
            }
        }
    }
    Ok(out)
}

const EPS_NOT_REAL: &str =
    "eps_r must be real; give loss as sigma (S/m), e.g. sigma = 2*pi*f*eps_r*eps0*tan_delta";

fn media_map(
    name: &str,
    what: &str,
    v: &Value,
    dims: (usize, usize, usize),
) -> Result<CellMap, ScriptError> {
    match v {
        Value::Tensor3(_) => Ok(CellMap::Cells(tensor_cells(name, what, v, dims)?)),
        Value::Complex(c) if c.im != 0.0 && what == "eps_r" => Err(err(name, EPS_NOT_REAL)),
        Value::Scalar(_) | Value::Complex(_) => Ok(CellMap::Uniform(real_scalar(name, what, v)?)),
        Value::Matrix(m) if m.len() == 1 => Ok(CellMap::Uniform(real_scalar(name, what, v)?)),
        other => Err(ScriptError::type_err(format!(
            "{name}: {what} must be a real scalar or an ny × nx × nz Tensor3, got {}",
            other.type_name()
        ))),
    }
}

/// `fdtd_media(sim, eps_r [, mu_r [, sigma]])`
pub(crate) fn builtin_fdtd_media(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd_media";
    if !(2..=4).contains(&args.len()) {
        return Err(ScriptError::arg_count_range(NAME.into(), 2, 4, args.len()));
    }
    let s = sim_arg(NAME, &args[0])?;
    let mut g = lock(NAME, &s)?;
    setup_check(NAME, "materials", &g)?;
    let dims = (g.ny(), g.nx(), g.nz());
    let eps = media_map(NAME, "eps_r", &args[1], dims)?;
    let mu = match args.get(2) {
        Some(v) => media_map(NAME, "mu_r", v, dims)?,
        None => CellMap::Uniform(1.0),
    };
    let sigma = match args.get(3) {
        Some(v) => media_map(NAME, "sigma", v, dims)?,
        None => CellMap::Uniform(0.0),
    };
    g.set_media(eps, mu, sigma).map_err(|e| dsp_err(NAME, e))?;
    Ok(Value::None)
}

fn mask_flags(name: &str, values: &[f64]) -> Result<Vec<bool>, ScriptError> {
    if values.iter().any(|x| x.is_nan()) {
        return Err(err(name, "mask contains NaN"));
    }
    Ok(values.iter().map(|&x| x != 0.0).collect())
}

/// `fdtd_pec(sim, mask)` / `fdtd_pec(sim, axis, index, mask2d)`
pub(crate) fn builtin_fdtd_pec(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd_pec";
    if args.len() != 2 && args.len() != 4 {
        return Err(err(
            NAME,
            format!(
                "expected fdtd_pec(sim, mask) or fdtd_pec(sim, axis, index, mask), got {} arguments",
                args.len()
            ),
        ));
    }
    let s = sim_arg(NAME, &args[0])?;
    let mut g = lock(NAME, &s)?;
    setup_check(NAME, "PEC", &g)?;
    let (ny, nx, nz) = (g.ny(), g.nx(), g.nz());

    if args.len() == 2 {
        let cells = match &args[1] {
            Value::Tensor3(_) => tensor_cells(NAME, "mask", &args[1], (ny, nx, nz))?,
            other => {
                return Err(ScriptError::type_err(format!(
                    "{NAME}: a volume mask must be an ny × nx × nz Tensor3, got {} \
                     (for a face use fdtd_pec(sim, \"z\", k, mask))",
                    other.type_name()
                )))
            }
        };
        let flags = mask_flags(NAME, &cells)?;
        return g
            .add_pec_volume(&flags)
            .map_err(|e| dsp_err(NAME, e))
            .map(|_| Value::None);
    }

    let axis_s = str_arg(NAME, "axis", &args[1])?;
    let axis = Axis::parse(&axis_s).ok_or_else(|| {
        err(
            NAME,
            format!("axis must be \"x\", \"y\", or \"z\", got \"{axis_s}\""),
        )
    })?;
    let (what, limit, rows, cols, layout) = match axis {
        Axis::X => ("x face (column j)", nx, ny, nz, "ny × nz"),
        Axis::Y => ("y face (row i)", ny, nx, nz, "nx × nz"),
        Axis::Z => ("z face (page k)", nz, ny, nx, "ny × nx"),
    };
    let face = index_one(NAME, what, &args[2], limit + 1)?;
    let nodes = match &args[3] {
        Value::Bool(b) => vec![*b; rows * cols],
        v => {
            let (vals, shape) = real_values(NAME, "mask", v)?;
            match shape.as_slice() {
                [] => vec![mask_flags(NAME, &vals)?[0]; rows * cols],
                [r, c] if *r == rows && *c == cols => mask_flags(NAME, &vals)?,
                _ => {
                    let got = match shape.as_slice() {
                        [n] => format!("a vector of {n}"),
                        [r, c] => format!("{r} × {c}"),
                        _ => unreachable!("real_values returns rank ≤ 2"),
                    };
                    return Err(err(
                        NAME,
                        format!(
                            "mask for the {axis_s} face must be {layout} = {rows} × {cols} \
                             (the fdtd_slice shape), got {got}"
                        ),
                    ));
                }
            }
        }
    };
    g.add_pec_face(axis, face, &nodes)
        .map_err(|e| dsp_err(NAME, e))?;
    Ok(Value::None)
}

/// `fdtd_cpml(sim, n [, key, value]...)` / `fdtd_cpml(sim, face, n)`
pub(crate) fn builtin_fdtd_cpml(args: Vec<Value>) -> Result<Value, ScriptError> {
    const NAME: &str = "fdtd_cpml";
    if !(2..=12).contains(&args.len()) {
        return Err(ScriptError::arg_count_range(NAME.into(), 2, 12, args.len()));
    }
    let s = sim_arg(NAME, &args[0])?;
    let mut g = lock(NAME, &s)?;
    setup_check(NAME, "CPML", &g)?;
    if matches!(&args[1], Value::Str(_)) {
        if args.len() != 3 {
            return Err(err(NAME, "expected fdtd_cpml(sim, face, n)"));
        }
        let face_s = str_arg(NAME, "face", &args[1])?;
        let face = Face::parse(&face_s).ok_or_else(|| {
            err(
                NAME,
                format!(
                    "unknown face \"{face_s}\" (expected xmin, xmax, ymin, ymax, zmin or zmax)"
                ),
            )
        })?;
        let n = nonneg_int(NAME, "n", &args[2])?;
        g.set_cpml_face(face, n).map_err(|e| dsp_err(NAME, e))?;
        return Ok(Value::None);
    }
    let n = nonneg_int(NAME, "n", &args[1])?;
    if !(args.len() - 2).is_multiple_of(2) {
        return Err(err(NAME, "options come in name, value pairs"));
    }
    let mut params = CpmlParams::default();
    let names = [
        "sigma_factor",
        "kappa_max",
        "alpha_max",
        "order",
        "alpha_order",
    ];
    let mut seen = [false; 5];
    for pair in args[2..].chunks(2) {
        let key = str_arg(NAME, "option name", &pair[0])?;
        let idx = names
            .iter()
            .position(|n| key.eq_ignore_ascii_case(n))
            .ok_or_else(|| {
                err(
                    NAME,
                    format!(
                        "unknown option \"{key}\" (expected sigma_factor, kappa_max, alpha_max, order or alpha_order)"
                    ),
                )
            })?;
        if seen[idx] {
            return Err(err(NAME, format!("option \"{key}\" given twice")));
        }
        seen[idx] = true;
        let v = real_scalar(NAME, &key, &pair[1])?;
        match idx {
            0 => params.sigma_factor = v,
            1 => params.kappa_max = v,
            2 => params.alpha_max = v,
            3 => params.order = v,
            _ => params.alpha_order = v,
        }
    }
    g.set_cpml(n, params).map_err(|e| dsp_err(NAME, e))?;
    Ok(Value::None)
}

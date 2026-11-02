//! Script builtins for the 3-D FDTD stepper (`rustlab_dsp::fdtd`).
//!
//! The grid lives in `Value::Fdtd3d`. Every call locks it, copies what it
//! returns into plain vectors, and builds the `Value`s after the lock drops.
//! Indices are 1-based `(i, j, k)` = (y row, x column, z page).

use super::builtin_cancel;
use super::value::Value;
use crate::error::ScriptError;
use ndarray::{Array1, Array2};
use rustlab_dsp::fdtd::{
    gaussian_pulse, Axis, Component, FdtdSim, SourceKind, Waveform, C0, DEFAULT_COURANT,
};
use rustlab_dsp::error::DspError;
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
        return Err(err(name, format!("{what} must be finite and positive, got {x}")));
    }
    Ok(x)
}

fn positive_int(name: &str, what: &str, v: &Value) -> Result<usize, ScriptError> {
    let x = real_scalar(name, what, v)?;
    match Value::count_from_f64(x) {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(err(name, format!("{what} must be a positive integer, got {x}"))),
    }
}

fn str_arg(name: &str, what: &str, v: &Value) -> Result<String, ScriptError> {
    v.to_str()
        .map_err(|_| ScriptError::type_err(format!("{name}: {what} must be a string, got {}", v.type_name())))
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
    f.insert("V".to_string(), Value::real_matrix(Array2::zeros((steps, 0))));
    f.insert("I".to_string(), Value::real_matrix(Array2::zeros((steps, 0))));
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
            return Err(err(NAME, format!("unknown option \"{key}\" (expected \"courant\")")));
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
        return Err(err(NAME, format!("sources drive E edges (ex, ey, ez), not {}", comp.name())));
    }
    let kind = match str_arg(NAME, "source type", &args[5])?.to_ascii_lowercase().as_str() {
        "soft" => SourceKind::Soft,
        "hard" => SourceKind::Hard,
        other => {
            return Err(err(NAME, format!("source type must be \"soft\" or \"hard\", got \"{other}\"")))
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
                    format!("\"{w}\" takes {} arguments after the waveform name, got {}", want - 7, args.len() - 7),
                ));
            }
            let t0 = finite_scalar(NAME, "t0", &args[7])?;
            let tau = positive_scalar(NAME, "tau", &args[8])?;
            let f0 = if want == 10 { finite_scalar(NAME, "f0", &args[9])? } else { 0.0 };
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
                return Err(err(NAME, "waveform must be a non-empty vector of finite values"));
            }
            Waveform::Samples(samples)
        }
    };

    let mut g = lock(NAME, &s)?;
    if g.steps() != 0 {
        return Err(err(
            NAME,
            format!("add sources before the first step ({} steps done); build a new fdtd3d", g.steps()),
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
        return Err(err(NAME, format!("{along_name} must be consecutive ascending indices")));
    }
    let edges: Vec<(usize, usize, usize)> = match comp.axis() {
        Axis::X => cols.iter().map(|&j| (rows[0], j, pages[0])).collect(),
        Axis::Y => rows.iter().map(|&i| (i, cols[0], pages[0])).collect(),
        Axis::Z => pages.iter().map(|&k| (rows[0], cols[0], k)).collect(),
    };
    for &(i, j, k) in &edges {
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
            format!("add probes before the first step ({} steps done); build a new fdtd3d", g.steps()),
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
    let rate = if seconds > 0.0 { cell_updates / seconds } else { 0.0 };
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
    let axis = Axis::parse(&axis_s)
        .ok_or_else(|| err(NAME, format!("axis must be \"x\", \"y\", or \"z\", got \"{axis_s}\"")))?;
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

//! Script surface of the 3-D FDTD stepper (`fdtd3d` and friends).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustlab_script::{Evaluator, ScriptError, Value};

fn parse(src: &str) -> Vec<rustlab_script::ast::Stmt> {
    let tokens = rustlab_script::lexer::tokenize(src).unwrap();
    rustlab_script::parser::parse(tokens).unwrap()
}

fn eval(src: &str) -> Evaluator {
    let mut ev = Evaluator::new();
    ev.run(&parse(src))
        .unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"));
    ev
}

fn run_err(src: &str) -> String {
    let mut ev = Evaluator::new();
    match ev.run(&parse(src)) {
        Ok(()) => panic!("expected an error from:\n{src}"),
        Err(e) => e.to_string(),
    }
}

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name) {
        Some(Value::Scalar(x)) => *x,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        other => panic!("{name}: expected scalar, got {other:?}"),
    }
}

fn shape(ev: &Evaluator, name: &str) -> Vec<f64> {
    match ev.get(name) {
        Some(Value::Vector(v)) => vec![1.0, v.len() as f64],
        Some(Value::Matrix(m)) => vec![m.nrows() as f64, m.ncols() as f64],
        other => panic!("{name}: expected an array, got {other:?}"),
    }
}

fn bits(ev: &Evaluator, name: &str) -> Vec<(u64, u64)> {
    let pairs: Vec<_> = match ev.get(name) {
        Some(Value::Vector(v)) => v.iter().copied().collect(),
        Some(Value::Matrix(m)) => m.iter().copied().collect(),
        other => panic!("{name}: expected an array, got {other:?}"),
    };
    pairs
        .iter()
        .map(|c| (c.re.to_bits(), c.im.to_bits()))
        .collect()
}

const SETUP: &str = r#"
sim = fdtd3d(12, 10, 8, 1e-3, 1e-3, 1e-3);
info = fdtd_info(sim);
tau = 10 * info.dt;
fdtd_source(sim, "ez", 5, 6, 4, "soft", "gaussian", 4*tau, tau);
p1 = fdtd_probe(sim, "ez", 5, 9, 4);
p2 = fdtd_probe(sim, "hy", 5, 8, 4);
"#;

#[test]
fn run_returns_the_record_struct() {
    let ev = eval(&format!(
        "{SETUP}
out = fdtd_run(sim, 40);
n = out.steps; it = out.interrupted; dt = out.dt;
t = out.t; P = out.probe; V = out.V; I = out.I; Z0 = out.Z0;
cu = out.cell_updates; rate = out.cell_rate; secs = out.seconds;
t1 = t(1); tN = t(end);
S = fdtd_slice(sim, \"ez\", \"z\", 4);
Sy = fdtd_slice(sim, \"ez\", \"y\", 5);
Sx = fdtd_slice(sim, \"ez\", \"x\", 6);
rec = fdtd_record(sim); nrec = rec.steps; Prec = rec.probe;
info2 = fdtd_info(sim); steps2 = info2.steps; bytes = info2.bytes;
"
    ));
    assert_eq!(scalar(&ev, "p1"), 1.0);
    assert_eq!(scalar(&ev, "p2"), 2.0);
    assert_eq!(scalar(&ev, "n"), 40.0);
    assert_eq!(scalar(&ev, "it"), 0.0);
    let dt = scalar(&ev, "dt");
    assert_eq!(scalar(&ev, "t1"), dt);
    assert!((scalar(&ev, "tN") - 40.0 * dt).abs() < 1e-24);
    assert_eq!(shape(&ev, "t"), vec![1.0, 40.0]);
    assert_eq!(shape(&ev, "P"), vec![40.0, 2.0]);
    assert_eq!(shape(&ev, "V"), vec![40.0, 0.0]);
    assert_eq!(shape(&ev, "I"), vec![40.0, 0.0]);
    assert_eq!(shape(&ev, "Z0"), vec![1.0, 0.0]);
    assert_eq!(shape(&ev, "S"), vec![10.0, 12.0]);
    assert_eq!(shape(&ev, "Sy"), vec![12.0, 8.0]);
    assert_eq!(shape(&ev, "Sx"), vec![10.0, 8.0]);
    assert_eq!(shape(&ev, "Prec"), vec![40.0, 2.0]);
    assert_eq!(scalar(&ev, "cu"), 12.0 * 10.0 * 8.0 * 40.0);
    assert!(scalar(&ev, "secs") >= 0.0);
    assert!(scalar(&ev, "rate") >= 0.0);
    assert_eq!(scalar(&ev, "nrec"), 40.0);
    assert_eq!(scalar(&ev, "steps2"), 40.0);
    assert!(scalar(&ev, "bytes") >= 6.0 * 960.0 * 8.0);
}

#[test]
fn info_reports_the_cfl_step() {
    let ev = eval(
        "sim = fdtd3d(4, 5, 6, 1e-3, 2e-3, 4e-3, \"courant\", 0.5);
         info = fdtd_info(sim); dt = info.dt; c = info.courant; c0 = info.c0;
         nx = info.nx; ny = info.ny; nz = info.nz; s = info.steps;",
    );
    let c0 = 299_792_458.0;
    let expect = 0.5 / (c0 * (1e6f64 + 0.25e6 + 0.0625e6).sqrt());
    assert!((scalar(&ev, "dt") - expect).abs() < 1e-12 * expect);
    assert_eq!(scalar(&ev, "c"), 0.5);
    assert_eq!(scalar(&ev, "c0"), c0);
    assert_eq!(
        (scalar(&ev, "nx"), scalar(&ev, "ny"), scalar(&ev, "nz")),
        (4.0, 5.0, 6.0)
    );
    assert_eq!(scalar(&ev, "s"), 0.0);
}

#[test]
fn step_and_run_agree_bit_for_bit() {
    let run_once = eval(&format!(
        "{SETUP}
a = fdtd_run(sim, 30);
A = fdtd_slice(sim, \"ez\", \"z\", 4);
Pa = a.probe;
"
    ));
    let stepped = eval(&format!(
        "{SETUP}
b1 = fdtd_step(sim, 10);
b2 = fdtd_step(sim);
b3 = fdtd_step(sim, 19);
B = fdtd_slice(sim, \"ez\", \"z\", 4);
Pb = b3.probe;
n1 = b2.steps; n3 = b3.steps;
"
    ));
    assert_eq!(bits(&run_once, "A"), bits(&stepped, "B"));
    assert_eq!(bits(&run_once, "Pa"), bits(&stepped, "Pb"));
    assert!(bits(&run_once, "A").iter().any(|&(re, _)| re != 0));
    assert_eq!(scalar(&stepped, "n1"), 11.0);
    assert_eq!(scalar(&stepped, "n3"), 30.0);
}

#[test]
fn assignment_shares_the_grid() {
    let ev = eval(
        "sim = fdtd3d(4, 4, 4, 1e-3, 1e-3, 1e-3);
         other = sim;
         fdtd_step(other, 3);
         info = fdtd_info(sim); s = info.steps;",
    );
    assert_eq!(scalar(&ev, "s"), 3.0);
}

#[test]
fn gaussian_pulse_shapes() {
    let ev = eval(
        "a = gaussian_pulse(1, 1, 0.5);
         b = gaussian_pulse(1.5, 1, 0.5);
         v = gaussian_pulse(0:0.1:1, 0.5, 0.2);
         m = gaussian_pulse([0, 1; 2, 3], 1, 1, 0.25);
         c = gaussian_pulse(1.25, 1, 0.5, 1);",
    );
    assert_eq!(scalar(&ev, "a"), 1.0);
    assert!((scalar(&ev, "b") - (-1.0f64).exp()).abs() < 1e-15);
    assert!(scalar(&ev, "c").abs() < 1e-15);
    assert_eq!(shape(&ev, "v"), vec![1.0, 11.0]);
    assert_eq!(shape(&ev, "m"), vec![2.0, 2.0]);
}

#[test]
fn setup_errors() {
    let e = run_err("sim = fdtd3d(10, 10, 10, 1e-3, 1e-3, 1e-3, \"courant\", 1.5);");
    assert!(
        e.contains("fdtd3d: courant must be in (0, 1], got 1.5"),
        "{e}"
    );
    eval("sim = fdtd3d(4, 4, 4, 1e-3, 1e-3, 1e-3, \"courant\", 1);");
    for (src, want) in [
        ("fdtd3d(1, 4, 4, 1e-3, 1e-3, 1e-3)", "at least 2"),
        (
            "fdtd3d(4, 4, 4, 0, 1e-3, 1e-3)",
            "dx must be finite and positive",
        ),
        (
            "fdtd3d(4, 4, 4, 1e-3, Inf, 1e-3)",
            "dy must be finite and positive",
        ),
        (
            "fdtd3d(4.5, 4, 4, 1e-3, 1e-3, 1e-3)",
            "nx must be a positive integer",
        ),
        (
            "fdtd3d(4, 4, 4, 1e-3, 1e-3, 1e-3, \"speed\", 1)",
            "unknown option",
        ),
        (
            "fdtd3d(4, 4, 4, 1e-3, 1e-3, 1e-3, \"courant\", 0)",
            "courant must be in (0, 1]",
        ),
        (
            "fdtd3d(4, 4, 4, 1e-3, 1e-3, 1e-3, \"threads\", 2)",
            "threads must be 0",
        ),
        (
            "fdtd3d(4, 4, 4, 1e-3, 1e-3, 1e-3, \"threads\", 1, \"threads\", 0)",
            "given twice",
        ),
        ("fdtd3d(4, 4, 4, 1e-3, 1e-3)", "wrong number of arguments"),
        ("fdtd_info(3)", "fdtd3d handle"),
        ("gaussian_pulse(0, 0, 0)", "tau must be finite and positive"),
        ("gaussian_pulse(0, NaN, 1)", "t0 must be finite"),
        ("gaussian_pulse(1j, 0, 1)", "t must be real"),
    ] {
        let e = run_err(&format!("x = {src};"));
        assert!(e.contains(want), "{src}: {e}");
    }
}

#[test]
fn source_probe_run_and_slice_errors() {
    let pre = "sim = fdtd3d(6, 5, 4, 1e-3, 1e-3, 1e-3);\n";
    for (src, want) in [
        (
            "fdtd_source(sim, \"hz\", 2, 2, 2, \"soft\", [1, 2])",
            "sources drive E edges",
        ),
        (
            "fdtd_source(sim, \"qq\", 2, 2, 2, \"soft\", [1, 2])",
            "unknown component",
        ),
        (
            "fdtd_source(sim, \"ez\", 2, 2, 2, \"warm\", [1, 2])",
            "\"soft\" or \"hard\"",
        ),
        (
            "fdtd_source(sim, \"ez\", 6, 2, 2, \"soft\", [1, 2])",
            "i must be an integer in 1..5",
        ),
        (
            "fdtd_source(sim, \"ez\", 1, 2, 2, \"soft\", [1, 2])",
            "PEC outer wall",
        ),
        (
            "fdtd_source(sim, \"ez\", 2, 1, 2, \"soft\", [1, 2])",
            "PEC outer wall",
        ),
        (
            "fdtd_source(sim, \"ex\", 2, 2, 1, \"soft\", [1, 2])",
            "PEC outer wall",
        ),
        (
            "fdtd_source(sim, \"ez\", 2:3, 2, 2, \"soft\", [1, 2])",
            "only k may be a range",
        ),
        (
            "fdtd_source(sim, \"ez\", 2, 2, [1, 3], \"soft\", [1, 2])",
            "consecutive ascending",
        ),
        (
            "fdtd_source(sim, \"ez\", 2, 2, 2, \"soft\", \"square\", 1, 1)",
            "waveform must be",
        ),
        (
            "fdtd_source(sim, \"ez\", 2, 2, 2, \"soft\", \"gaussian\", 1)",
            "takes 2 arguments after the waveform name, got 1",
        ),
        (
            "fdtd_source(sim, \"ez\", 2, 2, 2, \"soft\", \"gaussian\", 1, 1, 1)",
            "takes 2 arguments",
        ),
        (
            "fdtd_source(sim, \"ez\", 2, 2, 2, \"soft\", \"gaussian\", 1, -1)",
            "tau must be",
        ),
        (
            "fdtd_source(sim, \"ez\", 2, 2, 2, \"soft\", [1, NaN])",
            "finite values",
        ),
        (
            "fdtd_source(sim, \"ez\", 2, 2, 2, \"soft\", ones(2, 2))",
            "must be a vector",
        ),
        (
            "fdtd_probe(sim, \"ez\", 2, 7, 2)",
            "j must be an integer in 1..6",
        ),
        ("fdtd_probe(sim, \"ez\", 2, 2:3, 2)", "j must be a scalar"),
        (
            "fdtd_probe(sim, \"ez\", 0, 2, 2)",
            "i must be an integer in 1..5",
        ),
        ("fdtd_run(sim, 0)", "nsteps must be a positive integer"),
        ("fdtd_run(sim, 2.5)", "nsteps must be a positive integer"),
        ("fdtd_step(sim, -1)", "n must be a positive integer"),
        ("fdtd_slice(sim, \"ez\", \"w\", 1)", "axis must be"),
        (
            "fdtd_slice(sim, \"ez\", \"z\", 5)",
            "z index (page k) must be an integer in 1..4",
        ),
        (
            "fdtd_slice(sim, \"ez\", \"x\", 7)",
            "x index (column j) must be an integer in 1..6",
        ),
    ] {
        let e = run_err(&format!("{pre}x = {src};"));
        assert!(e.contains(want), "{src}: {e}");
    }
    let e = run_err(&format!(
        "{pre}fdtd_source(sim, \"ez\", 2, 2, 2, \"hard\", [1, 2]);
         fdtd_source(sim, \"ez\", 2, 2, 2, \"soft\", [1, 2]);"
    ));
    assert!(e.contains("cannot share an edge"), "{e}");
    let e = run_err(&format!(
        "{pre}fdtd_step(sim); p = fdtd_probe(sim, \"ez\", 2, 2, 2);"
    ));
    assert!(e.contains("add probes before the first step"), "{e}");
    let e = run_err(&format!(
        "{pre}fdtd_step(sim); fdtd_source(sim, \"ez\", 2, 2, 2, \"soft\", [1]);"
    ));
    assert!(e.contains("add sources before the first step"), "{e}");
}

#[test]
fn handle_is_opaque() {
    let ev = eval("sim = fdtd3d(4, 4, 4, 1e-3, 1e-3, 1e-3);");
    let shown = ev.get("sim").unwrap().to_string();
    assert!(shown.starts_with("<fdtd3d 4×4×4, dt="), "{shown}");
    assert!(shown.ends_with("steps=0>"), "{shown}");
    for src in [
        "x = sim + 1;",
        "x = -sim;",
        "x = sim(1);",
        "x = [sim, sim];",
    ] {
        run_err(&format!("sim = fdtd3d(4, 4, 4, 1e-3, 1e-3, 1e-3);\n{src}"));
    }
}

#[test]
fn stack_source_and_hard_waveform() {
    let ev = eval(
        "sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
         fdtd_source(sim, \"ez\", 3, 3, 2:4, \"hard\", [3e-3, 3e-3]);
         p = fdtd_probe(sim, \"ez\", 3, 3, 3);
         out = fdtd_step(sim, 2);
         P = out.probe; a = P(1); b = P(2);",
    );
    assert_eq!(scalar(&ev, "a"), -1.0);
    assert_eq!(scalar(&ev, "b"), -1.0);
}

#[test]
fn interrupt_before_run_returns_an_empty_struct() {
    let mut ev = eval(SETUP);
    rustlab_plot::request_interrupt();
    ev.run(&parse(
        "out = fdtd_run(sim, 50); n = out.steps; it = out.interrupted; P = out.probe; t = out.t;",
    ))
    .unwrap();
    assert_eq!(scalar(&ev, "n"), 0.0);
    assert_eq!(scalar(&ev, "it"), 1.0);
    assert_eq!(shape(&ev, "P"), vec![0.0, 2.0]);
    assert_eq!(shape(&ev, "t"), vec![1.0, 0.0]);
    ev.run(&parse("rec = fdtd_record(sim); rit = rec.interrupted;"))
        .unwrap();
    assert_eq!(scalar(&ev, "rit"), 1.0);
    ev.run(&parse(
        "out2 = fdtd_run(sim, 5); n2 = out2.steps; it2 = out2.interrupted;",
    ))
    .unwrap();
    assert_eq!(scalar(&ev, "n2"), 5.0);
    assert_eq!(scalar(&ev, "it2"), 0.0);
}

#[test]
fn interrupt_after_two_steps_keeps_the_partial_record() {
    let mut ev = eval(&format!("{SETUP}\nfirst = fdtd_step(sim, 2);"));
    rustlab_plot::request_interrupt();
    ev.run(&parse(
        "out = fdtd_run(sim, 500); n = out.steps; it = out.interrupted; P = out.probe; t = out.t; cu = out.cell_updates;",
    ))
    .unwrap();
    assert_eq!(scalar(&ev, "n"), 2.0);
    assert_eq!(scalar(&ev, "it"), 1.0);
    assert_eq!(shape(&ev, "P"), vec![2.0, 2.0]);
    assert_eq!(shape(&ev, "t"), vec![1.0, 2.0]);
    assert_eq!(scalar(&ev, "cu"), 0.0);
}

#[test]
fn notebook_cancel_stops_a_long_run() {
    let mut ev = eval("sim = fdtd3d(30, 30, 30, 1e-3, 1e-3, 1e-3);");
    let flag = Arc::new(AtomicBool::new(false));
    ev.set_cancel(flag.clone());
    let trip = flag.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        trip.store(true, Ordering::Relaxed);
    });
    let start = Instant::now();
    let result = ev.run(&parse("out = fdtd_run(sim, 100000000); done = 1;"));
    let elapsed = start.elapsed();
    handle.join().unwrap();
    assert!(
        matches!(result, Err(ScriptError::Cancelled)),
        "expected Cancelled, got {result:?}"
    );
    assert!(ev.get("out").is_none());
    assert!(ev.get("done").is_none());
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    match ev.get("sim").unwrap() {
        Value::Fdtd3d(s) => {
            let steps = s.lock().unwrap().steps();
            assert!(steps > 0 && steps < 100_000_000, "{steps}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn cancel_flag_is_scoped_to_the_call() {
    // The flag is only visible while the evaluator that owns it is inside a
    // builtin; a run with no flag on the same thread is not cancelled.
    let mut ev = eval("sim = fdtd3d(4, 4, 4, 1e-3, 1e-3, 1e-3);");
    ev.set_cancel(Arc::new(AtomicBool::new(false)));
    ev.run(&parse("a = fdtd_run(sim, 3);")).unwrap();
    let ev2 = eval("sim = fdtd3d(4, 4, 4, 1e-3, 1e-3, 1e-3); b = fdtd_run(sim, 3); n = b.steps;");
    assert_eq!(scalar(&ev2, "n"), 3.0);
}

#[test]
fn media_sets_dt_from_the_fastest_speed() {
    let ev = eval(
        "sim = fdtd3d(6, 5, 4, 1e-3, 1e-3, 1e-3);
dt0 = fdtd_info(sim).dt;
fdtd_media(sim, 4, 2);
dt4 = fdtd_info(sim).dt;
eps = ones3(5, 6, 4); eps(1, 1, 1) = 0.25;
fdtd_media(sim, eps);
dt1 = fdtd_info(sim).dt;
mu = 9 * ones3(5, 6, 4); mu(2, 2, 2) = 0.5;
fdtd_media(sim, 0.5, mu);
dt9 = fdtd_info(sim).dt;
",
    );
    let (dt0, dt4, dt1, dt9) = (
        scalar(&ev, "dt0"),
        scalar(&ev, "dt4"),
        scalar(&ev, "dt1"),
        scalar(&ev, "dt9"),
    );
    // Slower than light everywhere: the vacuum step stands.
    assert_eq!(dt4, dt0);
    // min(eps) · min(mu) = 0.25 → 2 c0
    assert!((dt1 / dt0 - 0.5).abs() < 1e-12, "{}", dt1 / dt0);
    // min(eps) · min(mu) = 0.5 · 0.5 → 2 c0
    assert!((dt9 / dt0 - 0.5).abs() < 1e-12, "{}", dt9 / dt0);
}

#[test]
fn uniform_media_scalar_and_tensor_agree_bit_for_bit() {
    let body = "
info = fdtd_info(sim); tau = 10 * info.dt;
fdtd_source(sim, \"ez\", 5, 6, 4, \"soft\", \"gaussian\", 4*tau, tau);
fdtd_probe(sim, \"ez\", 5, 9, 4);
fdtd_probe(sim, \"hy\", 5, 8, 4);
out = fdtd_run(sim, 60); P = out.probe;
";
    let a = eval(&format!(
        "sim = fdtd3d(12, 10, 8, 1e-3, 1e-3, 1e-3);
fdtd_media(sim, 2.5, 1.5, 0.02);{body}"
    ));
    let b = eval(&format!(
        "sim = fdtd3d(12, 10, 8, 1e-3, 1e-3, 1e-3);
fdtd_media(sim, 2.5 * ones3(10, 12, 8), 1.5 * ones3(10, 12, 8), 0.02 * ones3(10, 12, 8));{body}"
    ));
    assert_eq!(bits(&a, "P"), bits(&b, "P"));
    let peak = bits(&a, "P")
        .iter()
        .map(|&(re, _)| f64::from_bits(re).abs())
        .fold(0.0, f64::max);
    assert!(peak > 0.0, "the medium run never moved the probe");
}

#[test]
fn unit_media_matches_vacuum_bit_for_bit() {
    let tail = "
out = fdtd_run(sim, 50); P = out.probe;
";
    let vac = eval(&format!("{SETUP}{tail}"));
    let unit = eval(&format!(
        "sim = fdtd3d(12, 10, 8, 1e-3, 1e-3, 1e-3);
fdtd_media(sim, 1, ones3(10, 12, 8), 0);
info = fdtd_info(sim);
tau = 10 * info.dt;
fdtd_source(sim, \"ez\", 5, 6, 4, \"soft\", \"gaussian\", 4*tau, tau);
p1 = fdtd_probe(sim, \"ez\", 5, 9, 4);
p2 = fdtd_probe(sim, \"hy\", 5, 8, 4);
{tail}"
    ));
    assert_eq!(bits(&vac, "P"), bits(&unit, "P"));
}

#[test]
fn pec_face_holds_its_tangential_e_at_zero() {
    let probes = "
a = fdtd_probe(sim, \"ex\", 5, 6, 3);
b = fdtd_probe(sim, \"ey\", 4, 6, 3);
c = fdtd_probe(sim, \"ez\", 5, 6, 3);
out = fdtd_run(sim, 60); P = out.probe;
pa = max(abs(P(:, 3))); pb = max(abs(P(:, 4))); pc = max(abs(P(:, 5)));
";
    let open = eval(&format!("{SETUP}{probes}"));
    let shut = eval(&format!("{SETUP}fdtd_pec(sim, \"z\", 3, true);{probes}"));
    for name in ["pa", "pb", "pc"] {
        assert!(scalar(&open, name) > 0.0, "{name} never moved without PEC");
    }
    assert_eq!(scalar(&shut, "pa"), 0.0);
    assert_eq!(scalar(&shut, "pb"), 0.0);
    // Ez is normal to a z face, so the face leaves it free.
    assert!(scalar(&shut, "pc") > 0.0);
}

#[test]
fn pec_face_mask_and_volume_mask() {
    // A patch on the z = 3 face: only edges with both nodes inside are metal.
    let ev = eval(&format!(
        "{SETUP}
m = zeros(10, 12); m(4:6, 5:7) = 1;
fdtd_pec(sim, \"z\", 3, m);
vol = zeros3(10, 12, 8); vol(2, 2, 6) = 1;
fdtd_pec(sim, vol);
a = fdtd_probe(sim, \"ex\", 5, 6, 3);
b = fdtd_probe(sim, \"ex\", 5, 7, 3);
c = fdtd_probe(sim, \"ez\", 3, 3, 6);
d = fdtd_probe(sim, \"ez\", 3, 4, 6);
out = fdtd_run(sim, 60); P = out.probe;
pa = max(abs(P(:, 3))); pb = max(abs(P(:, 4))); pc = max(abs(P(:, 5))); pd = max(abs(P(:, 6)));
"
    ));
    assert_eq!(scalar(&ev, "pa"), 0.0, "ex inside the patch");
    assert!(scalar(&ev, "pb") > 0.0, "ex from the patch edge to outside");
    assert_eq!(scalar(&ev, "pc"), 0.0, "ez on the metal cell's corner");
    assert!(scalar(&ev, "pd") > 0.0, "ez one node past the metal cell");
}

#[test]
fn media_and_pec_errors() {
    let pre = "sim = fdtd3d(6, 5, 4, 1e-3, 1e-3, 1e-3);\n";
    for (src, want) in [
        (
            "fdtd_media(sim, 2 + 0.1j)",
            "eps_r must be real; give loss as sigma",
        ),
        ("fdtd_media(sim, 0)", "eps_r must be finite and > 0"),
        ("fdtd_media(sim, 1, -1)", "mu_r must be finite and > 0"),
        (
            "fdtd_media(sim, 1, 1, -0.5)",
            "sigma must be finite and >= 0",
        ),
        ("fdtd_media(sim, ones3(4, 6, 4))", "eps_r must be 5 × 6 × 4"),
        (
            "fdtd_media(sim, [1, 2])",
            "real scalar or an ny × nx × nz Tensor3",
        ),
        ("fdtd_media(sim)", "wrong number of arguments"),
        (
            "fdtd_pec(sim, zeros(5, 6))",
            "a volume mask must be an ny × nx × nz Tensor3",
        ),
        (
            "fdtd_pec(sim, \"z\", 6, true)",
            "z face (page k) must be an integer in 1..5",
        ),
        ("fdtd_pec(sim, \"q\", 1, true)", "axis must be"),
        (
            "fdtd_pec(sim, \"y\", 2, ones(5, 6))",
            "mask for the y face must be nx × nz = 6 × 4 (the fdtd_slice shape), got 5 × 6",
        ),
        ("fdtd_pec(sim, \"z\", 2, NaN)", "mask contains NaN"),
        ("fdtd_pec(sim, \"z\", 2)", "expected fdtd_pec(sim, mask)"),
        (
            "fdtd_source(sim, \"ex\", 2, 2, 2, \"soft\", [1, 2]); fdtd_pec(sim, \"z\", 2, true)",
            "PEC would cover an edge of an existing ex source",
        ),
        (
            "fdtd_pec(sim, \"z\", 2, true); fdtd_source(sim, \"ex\", 2, 2, 2, \"soft\", [1, 2])",
            "ex at (i, j, k) = (2, 2, 2) is PEC (fdtd_pec) and held at 0",
        ),
        (
            "fdtd_step(sim); fdtd_media(sim, 2)",
            "set materials before the first step (1 steps done)",
        ),
        (
            "fdtd_step(sim); fdtd_pec(sim, \"z\", 2, true)",
            "set PEC before the first step",
        ),
    ] {
        let e = run_err(&format!("{pre}{src};"));
        assert!(e.contains(want), "{src}: {e}");
    }
    // The max-wall face is already PEC, so it is accepted and changes nothing.
    eval(&format!("{pre}fdtd_pec(sim, \"z\", 5, true);"));
}

#[test]
fn info_bytes_grow_with_materials() {
    let ev = eval(
        "sim = fdtd3d(6, 5, 4, 1e-3, 1e-3, 1e-3);
b0 = fdtd_info(sim).bytes;
fdtd_media(sim, 2);
b1 = fdtd_info(sim).bytes;
fdtd_media(sim, 2, 1, 0.1);
b2 = fdtd_info(sim).bytes;
",
    );
    let (b0, b1, b2) = (scalar(&ev, "b0"), scalar(&ev, "b1"), scalar(&ev, "b2"));
    assert!(b1 > b0 && b2 > b1, "{b0} {b1} {b2}");
}

#[test]
fn cpml_errors() {
    let pre = "sim = fdtd3d(12, 10, 8, 1e-3, 1e-3, 1e-3);\n";
    for (src, want) in [
        ("fdtd_cpml(sim, 4, \"nope\", 1)", "unknown option \"nope\""),
        (
            "fdtd_cpml(sim, 4, \"sigma_factor\")",
            "options come in name, value pairs",
        ),
        (
            "fdtd_cpml(sim, 4, \"order\", 2, \"order\", 3)",
            "option \"order\" given twice",
        ),
        ("fdtd_cpml(sim, \"wall\", 0)", "unknown face \"wall\""),
        (
            "fdtd_cpml(sim, 3); fdtd_cpml(sim, \"xmin\", 1)",
            "0 (off) or the absorber thickness 3",
        ),
        ("fdtd_cpml(sim, \"zmin\", 0)", "before turning a face"),
        ("fdtd_cpml(sim, 1.5)", "non-negative integer"),
        ("fdtd_cpml(sim, -1)", "non-negative integer"),
        ("fdtd_cpml(sim, 4)", "leaves no interior on z"),
        (
            "fdtd_step(sim); fdtd_cpml(sim, 2)",
            "set CPML before the first step (1 steps done)",
        ),
    ] {
        let e = run_err(&format!("{pre}{src};"));
        assert!(e.contains(want), "{src}: {e}");
    }
}

#[test]
fn cpml_sigma_zero_matches_no_cpml_bit_for_bit() {
    let src = r#"
function out = go(use_cpml)
  sim = fdtd3d(12, 12, 12, 1e-3, 1e-3, 1e-3);
  dt = fdtd_info(sim).dt;
  tau = 6 * dt;
  t = (1:120) * dt;
  u = (t - 4 * tau) / tau;
  w = -u .* exp(-u .* u);
  fdtd_source(sim, "ez", 6, 6, 6, "soft", w);
  fdtd_probe(sim, "ez", 6, 8, 6);
  if use_cpml
    fdtd_cpml(sim, 3, "sigma_factor", 0);
  end
  out = fdtd_run(sim, 120);
end
a = go(0);
b = go(1);
pa = a.probe;
pb = b.probe;
"#;
    let ev = eval(src);
    assert_eq!(bits(&ev, "pa"), bits(&ev, "pb"));
}

#[test]
fn cpml_info_reports_faces_and_bytes() {
    let ev = eval(
        r#"
sim = fdtd3d(6, 5, 4, 1e-3, 1e-3, 1e-3);
b0 = fdtd_info(sim).bytes;
fdtd_cpml(sim, 1);
info = fdtd_info(sim);
b1 = info.bytes;
c = info.cpml;
fdtd_cpml(sim, "zmin", 0);
z = fdtd_info(sim).cpml;
"#,
    );
    let n = 6 * 5 * 4;
    let add = 12 * n * 8 + 2 * 3 * (6 + 5 + 4) * 8;
    assert_eq!(scalar(&ev, "b1") - scalar(&ev, "b0"), add as f64);
    let c = bits(&ev, "c");
    assert_eq!(c.len(), 6);
    assert!(c
        .iter()
        .all(|(re, im)| *im == 0 && f64::from_bits(*re) == 1.0));
    let z = bits(&ev, "z");
    let z: Vec<f64> = z.iter().map(|(re, _)| f64::from_bits(*re)).collect();
    assert_eq!(z, vec![1.0, 1.0, 1.0, 1.0, 0.0, 1.0]);
}

#[test]
fn cpml_absorbs_at_script_level() {
    let ev = eval(
        r#"
function ratio = late(use_cpml)
  n = 24;
  sim = fdtd3d(n, n, n, 1e-3, 1e-3, 1e-3);
  dt = fdtd_info(sim).dt;
  tau_steps = 6;
  tau = tau_steps * dt;
  steps = 250;
  t = (1:steps) * dt;
  u = (t - 4 * tau) / tau;
  w = -u .* exp(-u .* u);
  ic = n / 2;
  fdtd_source(sim, "ez", ic, ic, ic, "soft", w);
  fdtd_probe(sim, "ez", ic, ic + 4, ic);
  if use_cpml
    fdtd_cpml(sim, 6);
  end
  out = fdtd_run(sim, steps);
  p = out.probe;
  n_left = ceil(8 * tau_steps + 20);
  ratio = max(abs(p(n_left:end))) / max(abs(p));
end
pec = late(0);
cpml = late(1);
"#,
    );
    let pec = scalar(&ev, "pec");
    let cpml = scalar(&ev, "cpml");
    let db = 20.0 * (cpml / pec).log10();
    println!("script late residual: pec {pec:.3e}, cpml {cpml:.3e}, {db:.1} dB");
    assert!(db < -30.0, "cpml is only {db} dB below pec");
}

#[test]
fn fdtd_s11_of_a_resistance_is_a_one_port_network() {
    let ev = eval(
        r#"
n = 256;
dt = 1e-9;
Z0 = 50;
m = 8;
t = (0:n-1) * dt;
w = 2 * pi * m / (n * dt);
V = cos(w * t);
I = cos(w * (t - dt / 2)) / (3 * Z0);
s = fdtd_s11(V, I, dt, Z0, "window", "rect");
kind = parameter_type(s);
ports = nports(s);
zref = s.impedance;
f = s.frequencies;
tone = sij(s, 1, 1);
tr = real(tone(m));
ti = imag(tone(m));
f1 = real(f(1));
"#,
    );
    assert_eq!(
        match ev.get("kind") {
            Some(Value::Str(s)) => s.as_str(),
            other => panic!("kind: {other:?}"),
        },
        "S"
    );
    assert_eq!(scalar(&ev, "ports"), 1.0);
    assert_eq!(scalar(&ev, "zref"), 50.0);
    assert!(
        (scalar(&ev, "tr") - 0.5).abs() < 1e-9,
        "re {}",
        scalar(&ev, "tr")
    );
    assert!(scalar(&ev, "ti").abs() < 1e-9, "im {}", scalar(&ev, "ti"));
    let df = 1.0 / (256.0 * 1e-9);
    assert!((scalar(&ev, "f1") - df).abs() < 1e-3);
}

#[test]
fn fdtd_port_record_round_trips_through_fdtd_s11() {
    let ev = eval(
        r#"
sim = fdtd3d(8, 8, 8, 1e-3, 1e-3, 1e-3);
dt = fdtd_info(sim).dt;
tau = 15 * dt;
id = fdtd_port(sim, "ez", 4, 4, 3:4, 75, "gaussian", 4*tau, tau);
out = fdtd_run(sim, 48);
s = fdtd_s11(sim, "port", 1, "window", "rect");
s2 = fdtd_s11(out.V, out.I, out.dt, out.Z0, "window", "rect");
a = sij(s, 1, 1);
b = sij(s2, 1, 1);
same = max(abs(a - b));
V = out.V;
I = out.I;
z = out.Z0;
"#,
    );
    assert_eq!(scalar(&ev, "id"), 1.0);
    assert!(scalar(&ev, "same") == 0.0, "sim and samples disagree");
    assert_eq!(shape(&ev, "V"), vec![48.0, 1.0]);
    assert_eq!(shape(&ev, "I"), vec![48.0, 1.0]);
    assert_eq!(shape(&ev, "z"), vec![1.0, 1.0]);
}

#[test]
fn fdtd_port_rejects_a_second_port_and_a_shared_hard_edge() {
    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_port(sim, "ez", 3, 3, 3, 50, "gaussian", 1e-9, 2e-10);
fdtd_port(sim, "ey", 3, 3, 3, 50, "gaussian", 1e-9, 2e-10);
"#,
    );
    assert!(msg.contains("S21"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_source(sim, "ez", 3, 3, 3, "hard", "gaussian", 1e-9, 2e-10);
fdtd_port(sim, "ez", 3, 3, 3, 50, "gaussian", 1e-9, 2e-10);
"#,
    );
    assert!(msg.contains("hard source and a port"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_port(sim, "ez", 3, 3, 3, 50, "gaussian", 1e-9, 2e-10);
fdtd_source(sim, "ez", 3, 3, 3, "hard", "gaussian", 1e-9, 2e-10);
"#,
    );
    assert!(msg.contains("hard source and a port"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_port(sim, "ez", 1, 3, 3, 50, "gaussian", 1e-9, 2e-10);
"#,
    );
    assert!(msg.contains("PEC outer wall"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_port(sim, "hx", 3, 3, 3, 50, "gaussian", 1e-9, 2e-10);
"#,
    );
    assert!(msg.contains("E edges"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_port(sim, "ez", 3, 3, 3, 0, "gaussian", 1e-9, 2e-10);
"#,
    );
    assert!(msg.contains("positive"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_port(sim, "ez", 3, 3, [1, 3], 50, "gaussian", 1e-9, 2e-10);
"#,
    );
    assert!(msg.contains("consecutive"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_port(sim, "ez", 3, 3, 3, 50, "gaussian", 1e-9, 2e-10);
fdtd_run(sim, 2);
fdtd_port(sim, "ez", 3, 4, 3, 50, "gaussian", 1e-9, 2e-10);
"#,
    );
    // The second call is refused because a port already exists, before the
    // step check. Build a fresh grid that has already stepped.
    assert!(msg.contains("S21"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_run(sim, 1);
fdtd_port(sim, "ez", 3, 3, 3, 50, "gaussian", 1e-9, 2e-10);
"#,
    );
    assert!(msg.contains("before the first step"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_s11(sim);
"#,
    );
    assert!(msg.contains("no port"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_port(sim, "ez", 3, 3, 3, 50, "gaussian", 1e-9, 2e-10);
fdtd_s11(sim, "port", 2);
"#,
    );
    assert!(msg.contains("S21"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(6, 6, 6, 1e-3, 1e-3, 1e-3);
fdtd_port(sim, "ez", 3, 3, 3, 50, "gaussian", 1e-9, 2e-10);
fdtd_run(sim, 8);
fdtd_s11(sim, "window", "kaiser");
"#,
    );
    assert!(msg.contains("hann"), "{msg}");

    let msg = run_err(
        r#"
sim = fdtd3d(8, 8, 8, 1e-3, 1e-3, 1e-3);
mask = zeros3(8, 8, 8);
mask(3, 3, 3) = 1;
fdtd_pec(sim, mask);
fdtd_port(sim, "ez", 3, 3, 3, 50, "gaussian", 1e-9, 2e-10);
"#,
    );
    assert!(msg.contains("PEC"), "{msg}");
}

#[test]
fn fdtd_s11_default_is_a_pulse_and_the_samples_form_is_not() {
    let msg = run_err(
        r#"
sim = fdtd3d(8, 8, 8, 1e-3, 1e-3, 1e-3);
dt = fdtd_info(sim).dt;
fdtd_port(sim, "ez", 4, 4, 4, 50, "gaussian", 4 * dt, dt);
fdtd_run(sim, 8);
fdtd_s11(sim);
"#,
    );
    assert!(msg.contains("has not decayed"), "{msg}");

    // A named window is the steady-tone path: the tail check stays off.
    eval(
        r#"
sim = fdtd3d(8, 8, 8, 1e-3, 1e-3, 1e-3);
dt = fdtd_info(sim).dt;
fdtd_port(sim, "ez", 4, 4, 4, 50, "gaussian", 4 * dt, dt);
fdtd_run(sim, 16);
s = fdtd_s11(sim, "window", "hann");
n = length(s.frequencies);
"#,
    );

    let msg = run_err(
        r#"
sim = fdtd3d(8, 8, 8, 1e-3, 1e-3, 1e-3);
dt = fdtd_info(sim).dt;
fdtd_port(sim, "ez", 4, 4, 4, 50, "gaussian", 4 * dt, dt);
fdtd_run(sim, 16);
fdtd_s11(sim, "window", "hann", "decay", "pulse");
"#,
    );
    assert!(msg.contains("has not decayed"), "{msg}");

    let ev = eval(
        r#"
n = 256;
dt = 1e-9;
Z0 = 50;
m = 8;
t = (0:n - 1) * dt;
w = 2 * pi * m / (n * dt);
V = cos(w * t);
I = cos(w * (t - dt / 2)) / (3 * Z0);
s = fdtd_s11(V, I, dt, Z0);
tone = sij(s, 1, 1);
tr = real(tone(m));
padded = fdtd_s11(V, I, dt, Z0, "nfft", 4 * n);
tp = real(sij(padded, 1, 1)(4 * m));
"#,
    );
    assert!((scalar(&ev, "tr") - 0.5).abs() < 1e-9);
    assert!((scalar(&ev, "tp") - 0.5).abs() < 1e-9);

    let msg = run_err(
        r#"
V = [1, 0, 0, 0];
I = [1, 0, 0, 0] / 50;
fdtd_s11(V, I, 1e-9, 50, "nfft", 2);
"#,
    );
    assert!(
        msg.contains("interpolat") && msg.contains("resolution"),
        "{msg}"
    );
}

#[test]
fn fdtd_s11_of_an_independent_pillar_is_a_small_reflection() {
    let ev = eval(
        r#"
dx = 1e-3; dy = 1.5e-3; dz = 1e-3; z0 = 50; n = 4; kp = 2; ip = 6; jp = 8; tau = 1e-9;
sim = fdtd3d(16, 14, 8, dx, dy, dz);
sig = zeros3(14, 16, 8);
sig(ip:ip + n - 1, jp:jp + n - 1, 1:kp) = kp * dz / (z0 * n * dx * n * dy);
fdtd_media(sim, 1, 1, sig);
pad = zeros(14, 16);
pad(ip:ip + n, jp - 1:jp + n) = 1;
fdtd_pec(sim, "z", kp + 1, pad);
fdtd_port(sim, "ez", ip + n / 2, jp - 1, 1:kp, z0, "gaussian", 4 * tau, tau);
out = fdtd_run(sim, round(16 * tau / fdtd_info(sim).dt));
at = argmax(abs(out.I));
vi = out.V(at) / out.I(at);
s = fdtd_s11(sim);
mag = abs(sij(s, 1, 1));
f = s.frequencies;
keep = and(mag == mag, f <= 300e6);
sabs = max(mag(keep));
"#,
    );
    let vi = scalar(&ev, "vi");
    let sabs = scalar(&ev, "sabs");
    assert!(
        (vi / 50.0 - 1.0).abs() < 0.01,
        "V/I at the current peak is {vi}"
    );
    assert!(sabs <= 0.01, "max |S11| to 300 MHz is {sabs}");
}

#[test]
fn fdtd_farfield_shapes_match_the_rust_phasors() {
    let src = r#"
sim = fdtd3d(16, 16, 16, 1e-3, 1e-3, 1e-3, "courant", 0.99);
fdtd_cpml(sim, 4);
g = [1, -0.5, 0.25, 0.1, 0];
fdtd_source(sim, "ez", 9, 9, 8:9, "soft", g);
f = 299792458 / 0.020;
fdtd_ntff(sim, f, "margin", 1);
fdtd_run(sim, 5);
pat = fdtd_farfield(sim, [0.4, 1.2], [0.2, 1.1, 2.0], "decay", "off");
Eth = pat.Etheta; Eph = pat.Ephi; U = pat.U;
D = pat.directivity; Dd = pat.directivity_dBi;
G = pat.gain; Gd = pat.gain_dBi; Gr = pat.realized_gain; Grd = pat.realized_gain_dBi;
freq = pat.freq; th = pat.theta; ph = pat.phi;
pr = pat.prad; pa = pat.paccepted; pi = pat.pincident; s = pat.s11;
org = pat.origin; bx = pat.box; kk = pat.k; eta = pat.eta; dec = pat.decay;
szE = size(Eth); ntf = fdtd_info(sim).ntff;
"#;
    let ev = eval(src);
    let sz = match ev.get("szE") {
        Some(Value::Vector(v)) => v.iter().map(|c| c.re).collect::<Vec<_>>(),
        other => panic!("size: {other:?}"),
    };
    assert_eq!(sz, vec![1.0, 2.0, 3.0]);
    assert_eq!(scalar(&ev, "ntf"), 1.0);
    assert_eq!(scalar(&ev, "eta").is_finite(), true);

    let mut sim = rustlab_dsp::fdtd::FdtdSim::new(16, 16, 16, 1e-3, 1e-3, 1e-3, 0.99).unwrap();
    sim.set_cpml(4, rustlab_dsp::fdtd::CpmlParams::default())
        .unwrap();
    sim.add_source(
        rustlab_dsp::fdtd::Component::Ez,
        &[(8, 8, 7), (8, 8, 8)],
        rustlab_dsp::fdtd::SourceKind::Soft,
        rustlab_dsp::fdtd::Waveform::Samples(vec![1.0, -0.5, 0.25, 0.1, 0.0]),
    )
    .unwrap();
    let f = 299792458.0 / 0.020;
    sim.set_ntff(&[f], 1).unwrap();
    sim.run(5, &mut || false).unwrap();
    let ff = sim
        .far_field(
            &[0.4, 1.2],
            &[0.2, 1.1, 2.0],
            None,
            rustlab_dsp::fdtd::RecordDecay::Off,
        )
        .unwrap();
    let got = match ev.get("Eth") {
        Some(Value::Tensor3(t)) => t[[0, 1, 2]],
        other => panic!("Etheta: {other:?}"),
    };
    assert_eq!(
        got.re.to_bits(),
        ff.e_theta[(0 * 2 + 1) * 3 + 2].re.to_bits()
    );
    assert_eq!(
        got.im.to_bits(),
        ff.e_theta[(0 * 2 + 1) * 3 + 2].im.to_bits()
    );
}

#[test]
fn fdtd_farfield_matches_the_hertzian_dipole_at_thirty_cells() {
    let ev = eval(
        r#"
c0 = 299792458;
dx = 1e-3;
sim = fdtd3d(32, 32, 32, dx, dx, dx, "courant", 0.99);
fdtd_cpml(sim, 6);
info = fdtd_info(sim);
dt = info.dt;
n = 700;
fc = c0 / 0.020;
tau = sqrt(2) / (2 * pi * fc);
t0 = 5 * tau;
s = 1:n;
u = (s * dt - t0) / tau;
g = -u .* exp(-u .* u);
fdtd_source(sim, "ez", 17, 17, 17, "soft", g);
f = c0 / 0.030;
fdtd_ntff(sim, f, "margin", 2);
fdtd_run(sim, n);
org = [16e-3, 16e-3, 16.5e-3];
pat = fdtd_farfield(sim, [pi / 2, pi / 2], 0, "origin", org);
mu0 = 1.25663706212e-6;
eps0 = 1 / (mu0 * c0 * c0);
eta = sqrt(mu0 / eps0);
k = 2 * pi * f / c0;
il = sum(g .* exp(-j * 2 * pi * f * (s - 0.5) * dt)) * dt * dx * dx;
fan = j * eta * k * il / (4 * pi);
eth = pat.Etheta(1, :, 1);
one = pat.Etheta(1, 1, 1);
ae = abs(eth);
amp = abs(ae(1) / abs(fan) - 1);
amp1 = abs(abs(one) / abs(fan) - 1);
ph = abs(angle(one) - angle(fan));
"#,
    );
    let stored = match ev.get("pat") {
        Some(Value::Struct(s)) => match s.get("Etheta") {
            Some(Value::Tensor3(t)) => t[[0, 0, 0]],
            other => panic!("Etheta field: {other:?}"),
        },
        other => panic!("pat: {other:?}"),
    };
    let fan = match ev.get("fan") {
        Some(Value::Complex(c)) => *c,
        other => panic!("fan: {other:?}"),
    };
    let rel = stored / fan;
    let amp = (rel.norm() - 1.0).abs();
    let ph = rel.arg().abs();
    assert!(amp <= 0.01, "|F/Fan| error {amp}");
    assert!(ph <= 0.02, "phase error {ph} rad");
    assert!((scalar(&ev, "amp") - amp).abs() < 1e-9);
    assert!((scalar(&ev, "amp1") - amp).abs() < 1e-9);
    assert!((scalar(&ev, "ph") - ph).abs() < 1e-9);
    assert!(ph <= 0.02, "phase error {ph}");
}

#[test]
fn fdtd_farfield_without_a_port_has_no_gain() {
    let ev = eval(
        r#"
sim = fdtd3d(16, 16, 16, 1e-3, 1e-3, 1e-3, "courant", 0.99);
fdtd_cpml(sim, 4);
fdtd_source(sim, "ez", 9, 9, 9, "soft", [1, 0, 0, 0]);
fdtd_ntff(sim, 299792458 / 0.020, "margin", 1);
fdtd_run(sim, 4);
pat = fdtd_farfield(sim, 1.0, 0.2, "decay", "off");
gv = pat.gain(1, 1, 1);
gr = pat.realized_gain(1, 1, 1);
pa = pat.paccepted(1);
s = pat.s11(1);
dv = pat.directivity(1, 1, 1);
"#,
    );
    let nan = |name: &str| match ev.get(name) {
        Some(Value::Scalar(x)) => assert!(x.is_nan(), "{name} = {x}"),
        Some(Value::Complex(c)) => assert!(c.re.is_nan() && c.im.is_nan(), "{name} = {c}"),
        other => panic!("{name}: {other:?}"),
    };
    nan("gv");
    nan("gr");
    nan("pa");
    nan("s");
    let d = scalar(&ev, "dv");
    assert!(d.is_finite() && d > 0.0, "directivity {d}");
}

#[test]
fn fdtd_ntff_rejects_bad_options() {
    let base = r#"
sim = fdtd3d(16, 16, 16, 1e-3, 1e-3, 1e-3, "courant", 0.99);
fdtd_cpml(sim, 4);
fdtd_source(sim, "ez", 9, 9, 9, "soft", [1]);
"#;
    let msg = run_err(&format!("{base}\nfdtd_ntff(sim, 1e9, \"scale\", 1);"));
    assert!(msg.contains("unknown option"), "{msg}");
    let msg = run_err(&format!(
        "{base}\nfdtd_ntff(sim, 1e9, \"margin\", 2, \"margin\", 3);"
    ));
    assert!(msg.contains("twice"), "{msg}");
    let msg = run_err(&format!("{base}\nfdtd_ntff(sim, 1e9, \"margin\", 0);"));
    assert!(msg.contains("margin"), "{msg}");
    let msg = run_err(&format!(
        "{base}\nfdtd_ntff(sim, 1e9);\nfdtd_run(sim, 1);\nfdtd_farfield(sim, [1, 2], 0, \"origin\", [0, 0]);"
    ));
    assert!(msg.contains("origin") || msg.contains("three"), "{msg}");
    let msg = run_err(&format!(
        "{base}\nfdtd_ntff(sim, 1e9);\nfdtd_run(sim, 1);\nfdtd_farfield(sim, 1 + j, 0, \"decay\", \"off\");"
    ));
    assert!(msg.contains("real") || msg.contains("theta"), "{msg}");
}

#[test]
fn fdtd_info_reports_the_armed_frequency_count() {
    let ev = eval(
        r#"
sim = fdtd3d(16, 16, 16, 1e-3, 1e-3, 1e-3);
b0 = fdtd_info(sim).bytes;
n0 = fdtd_info(sim).ntff;
fdtd_cpml(sim, 4);
fdtd_ntff(sim, [1e9, 2e9]);
n1 = fdtd_info(sim).ntff;
b1 = fdtd_info(sim).bytes;
fdtd_ntff(sim, []);
n2 = fdtd_info(sim).ntff;
"#,
    );
    assert_eq!(scalar(&ev, "n0"), 0.0);
    assert_eq!(scalar(&ev, "n1"), 2.0);
    assert_eq!(scalar(&ev, "n2"), 0.0);
    assert!(scalar(&ev, "b1") > scalar(&ev, "b0"));
}

fn vec_re(ev: &Evaluator, name: &str) -> Vec<f64> {
    match ev.get(name) {
        Some(Value::Vector(v)) => v.iter().map(|c| c.re).collect(),
        other => panic!("{name}: expected vector, got {other:?}"),
    }
}

/// One complex element, a row `abs`, and the phase, at three source amplitudes.
/// `U` and `prad` already scaled; `abs` of the phasor used to drop the
/// imaginary part below `1e-10` and report a magnitude about 17 times low.
#[test]
fn fdtd_farfield_phasor_scales_from_1e_minus_6_to_1e6() {
    let ev = eval(
        r#"
c0 = 299792458;
dx = 1e-3;
n = 32;
nsteps = 700;
fc = c0 / 0.020;
f = c0 / 0.030;
mu0 = 1.25663706212e-6;
eta = sqrt(mu0 / (1 / (mu0 * c0 * c0)));
amps = [1e-6, 1, 1e6];
mag = [0, 0, 0];
mag1 = [0, 0, 0];
ph = [0, 0, 0];
pw = [0, 0, 0];
uu = [0, 0, 0];
dd = [0, 0, 0];
for ia = 1:3
    A = amps(ia);
    sim = fdtd3d(n, n, n, dx, dx, dx, "courant", 0.99);
    fdtd_cpml(sim, 6);
    dt = fdtd_info(sim).dt;
    tau = sqrt(2) / (2 * pi * fc);
    t0 = 5 * tau;
    s = 1:nsteps;
    u = (s * dt - t0) / tau;
    g = -u .* exp(-u .* u);
    fdtd_source(sim, "ez", 17, 17, 17, "soft", A * g);
    fdtd_ntff(sim, f, "margin", 2);
    fdtd_run(sim, nsteps);
    th = (1:35) * pi / 36;
    pat = fdtd_farfield(sim, th, 0, "origin", [16e-3, 16e-3, 16.5e-3]);
    one = pat.Etheta(1, 18, 1);
    if class(one) != "complex"
        error("Etheta(1, 18, 1) was not complex");
    end
    row = abs(pat.Etheta(1, :, 1));
    mag(ia) = row(18) / A;
    mag1(ia) = abs(one) / A;
    ph(ia) = angle(one);
    pw(ia) = pat.prad(1) / A^2;
    uu(ia) = pat.U(1, 18, 1) / A^2;
    dd(ia) = pat.directivity(1, 18, 1);
end
"#,
    );
    let mag = vec_re(&ev, "mag");
    let mag1 = vec_re(&ev, "mag1");
    let ph = vec_re(&ev, "ph");
    let pw = vec_re(&ev, "pw");
    let uu = vec_re(&ev, "uu");
    let dd = vec_re(&ev, "dd");
    let eta = {
        let mu0 = 1.25663706212e-6;
        (mu0 / (1.0 / (mu0 * 299792458.0_f64.powi(2)))).sqrt()
    };
    for i in 0..3 {
        let rel = (mag[i] - mag[0]).abs() / mag[0];
        assert!(rel < 1e-9, "row |E|/A moved by {rel} at {i}");
        let rel1 = (mag1[i] - mag[i]).abs() / mag[i];
        assert!(rel1 < 1e-12, "single-element abs vs row: {rel1}");
        let dph = (ph[i] - ph[0]).abs();
        assert!(dph < 1e-9, "phase moved by {dph}");
        let relp = (pw[i] - pw[0]).abs() / pw[0];
        assert!(relp < 1e-9, "prad/A^2 moved by {relp}");
        let relu = (uu[i] - uu[0]).abs() / uu[0];
        assert!(relu < 1e-9, "U/A^2 moved by {relu}");
        let reld = (dd[i] - dd[0]).abs() / dd[0];
        assert!(reld < 1e-9, "directivity moved by {reld}");
        let from_u = (2.0 * eta * uu[i]).sqrt();
        let ident = (mag1[i] / from_u - 1.0).abs();
        assert!(
            ident < 1e-9,
            "|E| vs sqrt(2 eta U) differs by {ident} at amplitude index {i}"
        );
    }
    assert!(ph[0].abs() > 0.1, "phase collapsed to {}", ph[0]);
}

#[test]
fn fdtd_farfield_port_reports_gain_and_s11() {
    let ev = eval(
        r#"
c0 = 299792458;
dx = 1e-3;
n = 20;
sim = fdtd3d(n, n, n, dx, dx, dx, "courant", 0.99);
fdtd_cpml(sim, 4);
dt = fdtd_info(sim).dt;
nsteps = 250;
fc = c0 / 0.020;
tau = sqrt(2) / (2 * pi * fc);
t0 = 5 * tau;
s = 1:nsteps;
u = (s * dt - t0) / tau;
g = -u .* exp(-u .* u);
ic = n / 2 + 1;
fdtd_port(sim, "ez", ic, ic, ic, 50, g);
fdtd_ntff(sim, c0 / 0.030, "margin", 1);
fdtd_run(sim, nsteps);
pat = fdtd_farfield(sim, pi / 2, 0, "decay", "off");
gv = pat.gain(1, 1, 1);
gr = pat.realized_gain(1, 1, 1);
dv = pat.directivity(1, 1, 1);
s11 = pat.s11(1);
alg = abs(gr / (gv * (1 - abs(s11)^2)) - 1);
"#,
    );
    let g = scalar(&ev, "gv");
    let d = scalar(&ev, "dv");
    assert!(g.is_finite() && g > 0.0, "gain {g}");
    assert!(d.is_finite() && d > 0.0, "directivity {d}");
    assert!(scalar(&ev, "alg") < 1e-9, "identity {}", scalar(&ev, "alg"));
    match ev.get("s11") {
        Some(Value::Complex(c)) => assert!(c.is_finite(), "s11 {c}"),
        other => panic!("s11: {other:?}"),
    }
}

#[test]
fn fdtd3d_threads_option() {
    let ev = eval(
        "sim = fdtd3d(6, 5, 4, 1e-3, 1e-3, 1e-3, \"threads\", 1, \"courant\", 0.5);
         info = fdtd_info(sim);
         t = info.threads; c = info.courant;",
    );
    assert_eq!(scalar(&ev, "t"), 1.0);
    assert!((scalar(&ev, "c") - 0.5).abs() < 1e-15);
    let ev = eval(
        "sim = fdtd3d(6, 5, 4, 1e-3, 1e-3, 1e-3, \"courant\", 0.8, \"threads\", 0);
         t = fdtd_info(sim).threads;",
    );
    assert_eq!(scalar(&ev, "t"), 0.0);
}

#[test]
fn parmap_of_fdtd_matches_arrayfun() {
    let ev = eval(
        r#"
function r = one(k)
    sim = fdtd3d(12, 10, 8, 2e-3, 2e-3, 1e-3, "threads", 1);
    fdtd_cpml(sim, 2);
    info = fdtd_info(sim);
    fdtd_source(sim, "ez", 5, 6, 4, "soft", "gaussian", 30 * info.dt, 8 * info.dt);
    fdtd_run(sim, 12 + k);
    M = fdtd_slice(sim, "ez", "z", 4);
    r = [max(abs(M(:))), sum(sum(M))];
end
A = arrayfun(@(k) one(k), [0, 1]);
P = parmap(@(k) one(k), [0, 1]);
d = max(abs(A(:) - P(:)));
"#,
    );
    assert_eq!(
        bits(&ev, "A"),
        bits(&ev, "P"),
        "parmap and arrayfun diverged"
    );
    match ev.get("P") {
        Some(Value::Matrix(m)) => assert_eq!((m.nrows(), m.ncols()), (2, 2)),
        other => panic!("P: {other:?}"),
    }
}

/// `parmap` runs each trial on the global pool, and a grid above the cell
/// threshold then steps on that same pool. The result matches `"threads", 1`.
#[test]
fn parmap_sweep_parallel_kernel_matches_threads_one() {
    let ev = eval(
        r#"
function r = one(k, threads)
    sim = fdtd3d(22, 22, 22, 1e-3, 1e-3, 1e-3, "threads", threads);
    fdtd_cpml(sim, 4);
    info = fdtd_info(sim);
    fdtd_source(sim, "ez", 11, 11, 11, "soft", "gaussian", 20 * info.dt, 6 * info.dt);
    fdtd_run(sim, 4 + k);
    M = fdtd_slice(sim, "ez", "z", 11);
    r = M(:);
end
S = arrayfun(@(k) one(k, 1), [0, 1]);
P = parmap(@(k) one(k, 0), [0, 1]);
"#,
    );
    assert_eq!(
        bits(&ev, "S"),
        bits(&ev, "P"),
        "parallel kernel inside parmap diverged from threads = 1"
    );
}

fn patch_antenna_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/pde/patch_antenna.rlab");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Statement text with strings and comments removed, so a depth scan does
/// not treat `%s` or a quoted `end` as syntax.
fn bare_statement(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            while let Some(d) = chars.next() {
                if d == '\\' {
                    chars.next();
                    continue;
                }
                if d == '"' {
                    break;
                }
            }
            out.push(' ');
            continue;
        }
        if c == '%' || c == '#' {
            break;
        }
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn block_open(stmt: &str) -> bool {
    stmt.starts_with("function ")
        || stmt == "if"
        || stmt.starts_with("if ")
        || stmt.starts_with("for ")
        || stmt.starts_with("while ")
        || stmt == "switch"
        || stmt.starts_with("switch ")
}

fn block_end(stmt: &str) -> bool {
    stmt == "end" || stmt.starts_with("end;") || stmt.starts_with("end ")
}

fn extract_function(src: &str, name: &str) -> String {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut started = false;
    for line in src.lines() {
        let stmt = bare_statement(line);
        if !started {
            if stmt.starts_with("function ") && stmt.contains(name) {
                started = true;
                depth = 1;
                out.push(line);
            }
            continue;
        }
        if block_open(&stmt) {
            depth += 1;
        }
        if block_end(&stmt) {
            depth -= 1;
        }
        out.push(line);
        if depth == 0 {
            break;
        }
    }
    assert!(
        started && depth == 0,
        "examples/pde/patch_antenna.rlab has no function {name}"
    );
    out.join("\n")
}

fn preset_fields(src: &str, name: &str) -> Evaluator {
    let func = extract_function(src, "patch_preset");
    eval(&format!(
        "{func}\nm = patch_preset(\"{name}\");\n\
         dx = m.dx; dy = m.dy; nsub = m.nsub; nsteps = m.nsteps;\n\
         gpad = m.gpad; air_xy = m.air_xy; air_z = m.air_z;\n\
         npml = m.npml; margin = m.margin;\n"
    ))
}

/// `quick` is the default and `full` is the 2 mm, 16 000-step lesson.
/// Any other name errors and names both presets. This does not step a grid.
#[test]
fn patch_antenna_presets_accept_quick_and_full() {
    let src = patch_antenna_source();
    let is_line = |line: &str, want: &str| {
        let trimmed = line.trim();
        !trimmed.starts_with('%') && !trimmed.starts_with('#') && trimmed == want
    };
    assert!(
        src.lines().any(|line| is_line(line, "preset = \"quick\";")),
        "the default assignment is preset = \"quick\";"
    );
    assert!(
        src.lines()
            .any(|line| is_line(line, "mesh = patch_preset(preset);")),
        "the script runs the preset it names"
    );

    let quick = preset_fields(&src, "quick");
    let full = preset_fields(&src, "full");
    assert_eq!(scalar(&full, "dx"), 2e-3);
    assert_eq!(scalar(&full, "dy"), 2e-3);
    assert_eq!(scalar(&full, "nsub"), 2.0);
    assert_eq!(scalar(&full, "nsteps"), 16_000.0);
    assert_eq!(scalar(&full, "gpad"), 16e-3);
    assert_eq!(scalar(&full, "air_xy"), 10e-3);
    assert_eq!(scalar(&full, "air_z"), 20e-3);
    assert_eq!(scalar(&full, "npml"), 8.0);
    assert_eq!(scalar(&full, "margin"), 2.0);

    let coarser = scalar(&quick, "dx") > scalar(&full, "dx")
        || scalar(&quick, "nsub") < scalar(&full, "nsub")
        || scalar(&quick, "nsteps") < scalar(&full, "nsteps");
    assert!(
        coarser,
        "quick is the shorter mesh: dx {} nsub {} nsteps {}",
        scalar(&quick, "dx"),
        scalar(&quick, "nsub"),
        scalar(&quick, "nsteps")
    );
    for name in [
        "dx", "dy", "nsub", "nsteps", "gpad", "air_xy", "air_z", "npml", "margin",
    ] {
        let v = scalar(&quick, name);
        assert!(v.is_finite() && v > 0.0, "quick {name} = {v}");
    }

    let func = extract_function(&src, "patch_preset");
    let err = run_err(&format!("{func}\npatch_preset(\"sideways\");\n"));
    assert!(
        err.contains("sideways"),
        "the error should name the bad preset, got {err}"
    );
    assert!(
        err.contains("quick") && err.contains("full"),
        "the error should name both presets, got {err}"
    );
}

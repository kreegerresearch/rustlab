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

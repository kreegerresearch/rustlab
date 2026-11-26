//! The Viterbi decoder of `examples/notebooks/comm/viterbi_fec.md` must decode
//! exactly as it did before any interpreter change.
//!
//! The decoder is script code (`viterbi`, `viterbi_frames`), so the cells
//! that define it are read out of the notebook itself. Each case decodes a
//! seeded noisy block and checks the bits two ways: against a Rust Viterbi
//! that runs the same add-compare-select on the script's own branch metrics
//! (`b > a` picks predecessor `P1`, a tie keeps `P0`), and against a
//! checksum recorded with rustlab 0.3.89, which also pins the encoder, the
//! seeded noise, and the trellis tables. Every case runs with real and with
//! complex array storage.

use rustlab_script::eval::shared::set_real_storage;
use rustlab_script::{Evaluator, Value};
use std::path::PathBuf;

fn notebook_cells() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/notebooks/comm/viterbi_fec.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut cells = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        match (&mut cur, line.trim_end()) {
            (None, "```rustlab") => cur = Some(String::new()),
            (Some(c), "```") => {
                cells.push(std::mem::take(c));
                cur = None;
            }
            (Some(c), l) => {
                c.push_str(l);
                c.push('\n');
            }
            _ => {}
        }
    }
    cells
}

/// The notebook cells that define `name`.
fn defining_cell(cells: &[String], name: &str) -> String {
    cells
        .iter()
        .find(|c| {
            c.lines().any(|l| {
                let l = l.trim_start();
                l.starts_with("function") && l.contains(&format!(" {name}("))
            })
        })
        .unwrap_or_else(|| panic!("no notebook cell defines {name}"))
        .clone()
}

fn run(ev: &mut Evaluator, src: &str) {
    let src = format!("{src}\n");
    let tokens = rustlab_script::lexer::tokenize(&src).unwrap();
    let stmts = rustlab_script::parser::parse(tokens).unwrap();
    ev.run(&stmts)
        .unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"));
}

/// An evaluator holding the notebook's encoder, trellis, and both decoders.
fn decoder() -> Evaluator {
    let cells = notebook_cells();
    let mut ev = Evaluator::new();
    for name in ["conv_encode", "trellis", "viterbi", "viterbi_frames"] {
        run(&mut ev, &defining_cell(&cells, name));
    }
    ev
}

fn reals(ev: &Evaluator, name: &str) -> Vec<f64> {
    match ev.get(name) {
        Some(Value::Vector(v)) => v.iter().map(|z| z.re).collect(),
        Some(Value::Matrix(m)) => m.iter().map(|z| z.re).collect(),
        Some(Value::Scalar(x)) => vec![*x],
        other => panic!("{name}: {other:?}"),
    }
}

fn matrix(ev: &Evaluator, name: &str) -> (usize, usize, Vec<f64>) {
    match ev.get(name) {
        Some(Value::Matrix(m)) => (m.nrows(), m.ncols(), m.iter().map(|z| z.re).collect()),
        other => panic!("{name}: {other:?}"),
    }
}

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name) {
        Some(Value::Scalar(x)) => *x,
        other => panic!("{name}: {other:?}"),
    }
}

/// Reference decoder over the branch metrics `B0`, `B1` (64 × L, row-major)
/// for steps `t0 .. t0 + l` of each row; the notebook's rule exactly.
fn reference(
    p0: &[usize],
    p1: &[usize],
    b0: &[f64],
    b1: &[f64],
    ncols: usize,
    t0: usize,
    l: usize,
) -> Vec<f64> {
    let mut pm = vec![-1e9; 64];
    pm[0] = 0.0;
    let mut d = vec![[false; 64]; l];
    for t in 0..l {
        let mut next = [0.0; 64];
        for s in 0..64 {
            let a = pm[p0[s]] + b0[s * ncols + t0 + t];
            let b = pm[p1[s]] + b1[s * ncols + t0 + t];
            d[t][s] = b > a;
            next[s] = if b > a { b } else { a };
        }
        pm = next.to_vec();
    }
    let mut s = 0usize;
    let mut u = vec![0.0; l];
    for t in (0..l).rev() {
        u[t] = (s / 32) as f64;
        s = (s % 32) * 2 + usize::from(d[t][s]);
    }
    u.truncate(l - 6);
    u
}

fn tables(ev: &Evaluator) -> (Vec<usize>, Vec<usize>) {
    let ix = |n| reals(ev, n).iter().map(|&x| x as usize - 1).collect();
    (ix("P0"), ix("P1"))
}

fn with_both_storages(f: impl Fn()) {
    for real in [true, false] {
        set_real_storage(Some(real));
        f();
    }
    set_real_storage(None);
}

/// One frame through `viterbi`: (bit errors, checksum of the decoded bits).
fn single_frame(seed: u32, n: usize, ebn0_db: f64, hard: bool) -> (f64, f64) {
    let mut ev = decoder();
    run(
        &mut ev,
        &format!(
            "G = [1 1 1 1 0 0 1; 1 0 1 1 0 1 1];
             [P0, P1, C0, C1] = trellis(G);
             seed({seed});
             u = randi([0, 1], 1, {n});
             x = 1 - 2 * conv_encode(u, G);
             Y = x + sqrt(1 / (2 * 0.5 * 10^({ebn0_db} / 10))) * randn(size(x, 1), size(x, 2));
             if {h}
               Y = sign(Y);
             end
             B0 = C0 * Y;
             B1 = C1 * Y;
             uh = viterbi(Y, P0, P1, C0, C1);
             errors = sum(uh != u);
             chk = mod(sum(uh .* mod((1:{n}) * 7919, 1000003)), 1000003);",
            h = u8::from(hard)
        ),
    );
    let (p0, p1) = tables(&ev);
    let (_, ncols, b0) = matrix(&ev, "B0");
    let (_, _, b1) = matrix(&ev, "B1");
    let want = reference(&p0, &p1, &b0, &b1, ncols, 0, ncols);
    assert_eq!(reals(&ev, "uh"), want, "seed {seed} hard {hard}");
    (scalar(&ev, "errors"), scalar(&ev, "chk"))
}

#[test]
fn viterbi_soft_decisions_are_unchanged() {
    with_both_storages(|| {
        assert_eq!(single_frame(42, 4000, 3.0, false), (0.0, 863584.0));
        assert_eq!(single_frame(5, 4000, 1.0, false), (190.0, 278313.0));
    });
}

#[test]
fn viterbi_hard_decisions_are_unchanged() {
    with_both_storages(|| {
        assert_eq!(single_frame(42, 4000, 3.0, true), (112.0, 586296.0));
        assert_eq!(single_frame(9, 4000, 4.0, true), (39.0, 203002.0));
    });
}

/// `viterbi_frames` with punctured (rate 3/4) soft frames side by side:
/// every frame decodes as the reference does on its own columns.
#[test]
fn viterbi_frames_punctured_is_unchanged() {
    with_both_storages(|| {
        let mut ev = decoder();
        run(
            &mut ev,
            "G = [1 1 1 1 0 0 1; 1 0 1 1 0 1 1];
             [P0, P1, C0, C1] = trellis(G);
             F = 6; N = 500; L = N + 6;
             seed(17);
             u = randi([0, 1], F, N);
             stream = reshape([u, zeros(F, 6)]', 1, F * L);
             X = 1 - 2 * conv_encode(stream(1 : F * L - 6), G);
             P = [1 1 0; 1 0 1];
             M = repmat(P, 1, ceil(L / 3));
             M = repmat(M(:, 1:L), 1, F);
             Y = (X + 0.8 * randn(size(X, 1), size(X, 2))) .* M;
             B0 = C0 * Y;
             B1 = C1 * Y;
             U = viterbi_frames(Y, F, P0, P1, C0, C1);
             errors = sum(sum(U != u));
             chk = mod(sum(sum(U .* mod(reshape(1:(F * N), F, N) * 7919, 1000003))), 1000003);",
        );
        let (p0, p1) = tables(&ev);
        let (_, ncols, b0) = matrix(&ev, "B0");
        let (_, _, b1) = matrix(&ev, "B1");
        let (f, n, got) = matrix(&ev, "U");
        assert_eq!((f, n), (6, 500));
        let l = n + 6;
        for fr in 0..f {
            let want = reference(&p0, &p1, &b0, &b1, ncols, fr * l, l);
            assert_eq!(&got[fr * n..(fr + 1) * n], &want[..], "frame {fr}");
        }
        assert_eq!(
            (scalar(&ev, "errors"), scalar(&ev, "chk")),
            (853.0, 923906.0)
        );
    });
}

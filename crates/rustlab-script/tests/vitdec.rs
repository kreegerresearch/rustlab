//! `poly2trellis` and `vitdec`.
//!
//! The trellis tables are MATLAB's (state = register contents, newest input
//! in the most significant bit; outputs written in octal). The decoder must
//! give exactly the bits of the script decoders in
//! `examples/notebooks/comm/viterbi_fec.md` when its traceback covers the
//! whole frame, which is how the notebook calls it.

use rustlab_script::{Evaluator, Value};
use std::path::PathBuf;

fn run(ev: &mut Evaluator, src: &str) -> Result<(), String> {
    let src = format!("{src}\n");
    let tokens = rustlab_script::lexer::tokenize(&src).map_err(|e| e.to_string())?;
    let stmts = rustlab_script::parser::parse(tokens).map_err(|e| e.to_string())?;
    ev.run(&stmts).map_err(|e| e.to_string())
}

fn eval(src: &str) -> Evaluator {
    let mut ev = Evaluator::new();
    run(&mut ev, src).unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"));
    ev
}

fn eval_err(src: &str) -> String {
    let mut ev = Evaluator::new();
    match run(&mut ev, src) {
        Err(e) => e,
        Ok(()) => panic!("expected an error\n{src}"),
    }
}

fn reals(ev: &Evaluator, name: &str) -> Vec<f64> {
    match ev.get(name) {
        Some(Value::Vector(v)) => v.iter().map(|z| z.re).collect(),
        Some(Value::Matrix(m)) => m.iter().map(|z| z.re).collect(),
        Some(Value::Scalar(x)) => vec![*x],
        other => panic!("{name}: {other:?}"),
    }
}

fn shape(ev: &Evaluator, name: &str) -> (usize, usize) {
    match ev.get(name) {
        Some(Value::Vector(v)) => (1, v.len()),
        Some(Value::Matrix(m)) => (m.nrows(), m.ncols()),
        other => panic!("{name}: {other:?}"),
    }
}

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name) {
        Some(Value::Scalar(x)) => *x,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        other => panic!("{name}: {other:?}"),
    }
}

/// A rate-k/n encoder that walks a trellis struct: `c = enc(u, t)`.
const ENCODER: &str = r#"
function c = enc(u, t)
  k = log2(t.numInputSymbols);
  n = log2(t.numOutputSymbols);
  steps = length(u) / k;
  c = zeros(1, steps * n);
  s = 0;
  for i = 1:steps
    sym = 0;
    for b = 1:k
      sym = 2 * sym + u((i - 1) * k + b);
    end
    o = t.outputs(s + 1, sym + 1);
    # outputs are written in octal
    d = 0; p = 1;
    while o > 0
      d = d + mod(o, 10) * p;
      o = floor(o / 10);
      p = p * 8;
    end
    for b = n:-1:1
      c((i - 1) * n + b) = mod(d, 2);
      d = floor(d / 2);
    end
    s = t.nextStates(s + 1, sym + 1);
  end
end
"#;

#[test]
fn poly2trellis_matches_matlab_tables() {
    let ev = eval(
        "t = poly2trellis(3, [7 5]);
         a = t.numInputSymbols; b = t.numOutputSymbols; s = t.numStates;
         ns = t.nextStates; out = t.outputs;
         u = poly2trellis(7, [171 133]);
         s7 = u.numStates; ns7 = u.nextStates; out7 = u.outputs;
         ok = istrellis(u);",
    );
    assert_eq!(
        (scalar(&ev, "a"), scalar(&ev, "b"), scalar(&ev, "s")),
        (2.0, 4.0, 4.0)
    );
    assert_eq!(shape(&ev, "ns"), (4, 2));
    assert_eq!(reals(&ev, "ns"), vec![0., 2., 0., 2., 1., 3., 1., 3.]);
    assert_eq!(reals(&ev, "out"), vec![0., 3., 3., 0., 2., 1., 1., 2.]);
    assert_eq!(scalar(&ev, "s7"), 64.0);
    assert_eq!(shape(&ev, "ns7"), (64, 2));
    let ns7 = reals(&ev, "ns7");
    for s in 0..64 {
        assert_eq!(ns7[2 * s], (s / 2) as f64);
        assert_eq!(ns7[2 * s + 1], (s / 2 + 32) as f64);
    }
    // State 0, input 1: register 1000000, 171 → 1, 133 → 1.
    assert_eq!(&reals(&ev, "out7")[0..2], &[0.0, 3.0]);
    assert_eq!(scalar(&ev, "ok"), 1.0);
}

/// Two inputs: K = [5 4], codegen [23 35 0; 0 5 13] (MATLAB's example) has
/// 2^(4+3) states, 4 input symbols, and 8 output symbols, and a noiseless
/// round trip returns the message.
#[test]
fn poly2trellis_two_inputs_round_trip() {
    let ev = eval(&format!(
        "{ENCODER}
         t = poly2trellis([5 4], [23 35 0; 0 5 13]);
         s = t.numStates; a = t.numInputSymbols; b = t.numOutputSymbols;
         seed(3);
         u = [randi([0, 1], 1, 400), zeros(1, 8)];
         c = enc(u, t);
         d = vitdec(c, t, 30, \"term\", \"hard\");
         e = sum(d != u);"
    ));
    assert_eq!(
        (scalar(&ev, "s"), scalar(&ev, "a"), scalar(&ev, "b")),
        (128.0, 4.0, 8.0)
    );
    assert_eq!(scalar(&ev, "e"), 0.0);
}

/// Noiseless round trips in every mode, feedforward and recursive.
#[test]
fn noiseless_round_trips() {
    let ev = eval(&format!(
        "{ENCODER}
         t = poly2trellis(7, [171 133]);
         seed(1);
         u = [randi([0, 1], 1, 500), zeros(1, 6)];
         c = enc(u, t);
         y = 1 - 2 * c;
         e_term = sum(vitdec(c, t, 34, \"term\", \"hard\") != u);
         e_trunc = sum(vitdec(c, t, 34, \"trunc\", \"hard\") != u);
         e_unq = sum(vitdec(y, t, 96, \"trunc\", \"unquant\") != u);
         e_soft = sum(vitdec(7 * c, t, 40, \"term\", \"soft\", 3) != u);
         dc = vitdec(c, t, 20, \"cont\", \"hard\");
         e_cont = sum(dc(21:end) != u(1:end-20));
         head = sum(dc(1:20));
         r = poly2trellis(5, [37 33], 37);
         ur = randi([0, 1], 1, 300);
         er = sum(vitdec(enc(ur, r), r, 25, \"trunc\", \"hard\") != ur);"
    ));
    for name in [
        "e_term", "e_trunc", "e_unq", "e_soft", "e_cont", "head", "er",
    ] {
        assert_eq!(scalar(&ev, name), 0.0, "{name}");
    }
}

/// A recursive code with systematic first output: `poly2trellis(5, [37 33],
/// 37)` sends each input bit unchanged on output 1.
#[test]
fn recursive_code_is_systematic() {
    let ev = eval(&format!(
        "{ENCODER}
         r = poly2trellis(5, [37 33], 37);
         seed(4);
         u = randi([0, 1], 1, 200);
         c = enc(u, r);
         sys = sum(c(1:2:end) != u);"
    ));
    assert_eq!(scalar(&ev, "sys"), 0.0);
}

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

fn defining_cell(cells: &[String], name: &str) -> Option<String> {
    cells
        .iter()
        .find(|c| {
            c.lines().any(|l| {
                let l = l.trim_start();
                l.starts_with("function") && l.contains(&format!(" {name}("))
            })
        })
        .cloned()
}

/// The script decoder the notebook used before `vitdec`, with the encoder
/// and trellis-table helpers it relied on.
const SCRIPT_DECODER: &str = r#"
function c = conv_encode(u, G)
  x = [u, zeros(1, 6)];
  L = length(x);
  n = size(G, 1);
  c = zeros(n, L);
  for j = 1:n
    v = convolve(x, G(j, :));
    c(j, :) = mod(v(1:L), 2);
  end
end

function [P0, P1, C0, C1] = trellis(G)
  n = size(G, 1);
  P0 = zeros(1, 64); P1 = zeros(1, 64);
  C0 = zeros(64, n); C1 = zeros(64, n);
  for ns = 0:63
    u = floor(ns / 32);
    for b = 0:1
      s = mod(ns, 32) * 2 + b;
      reg = [u, mod(floor(s ./ [32, 16, 8, 4, 2, 1]), 2)];
      sym = 1 - 2 * mod(G * reg', 2);
      if b == 0
        P0(ns + 1) = s + 1;
        C0(ns + 1, :) = sym';
      else
        P1(ns + 1) = s + 1;
        C1(ns + 1, :) = sym';
      end
    end
  end
end

function u = viterbi(Y, P0, P1, C0, C1)
  L = size(Y, 2);
  pm = -1e9 * ones(1, 64);
  pm(1) = 0;
  B0 = C0 * Y;
  B1 = C1 * Y;
  D = zeros(64, L);
  for t = 1:L
    a = pm(P0) + B0(:, t);
    b = pm(P1) + B1(:, t);
    D(:, t) = b > a;
    pm = max(a, b);
  end
  s = 0;
  u = zeros(1, L);
  for t = L:-1:1
    u(t) = floor(s / 32);
    s = mod(s, 32) * 2 + D(s + 1, t);
  end
  u = u(1:L-6);
end
"#;

/// An evaluator holding the script decoder. The notebook's own copy is used
/// while it still defines one (before the switch to `vitdec`).
fn script_decoder() -> Evaluator {
    let cells = notebook_cells();
    let mut ev = Evaluator::new();
    let from_notebook = ["conv_encode", "trellis", "viterbi"]
        .iter()
        .map(|n| defining_cell(&cells, n))
        .collect::<Option<Vec<_>>>();
    match from_notebook {
        Some(cs) if defining_cell(&cells, "viterbi").is_some_and(|c| c.contains("B0 = C0 * Y")) => {
            for c in cs {
                run(&mut ev, &c).unwrap();
            }
        }
        _ => run(&mut ev, SCRIPT_DECODER).unwrap(),
    }
    ev
}

/// `vitdec` with the traceback over the whole frame decodes exactly as the
/// script decoder, soft and hard, for both generator orders.
#[test]
fn whole_frame_traceback_matches_the_script_decoder() {
    for (gens, oct) in [
        ("[1 0 1 1 0 1 1; 1 1 1 1 0 0 1]", "[133 171]"),
        ("[1 1 1 1 0 0 1; 1 0 1 1 0 1 1]", "[171 133]"),
    ] {
        for (seed, db) in [(42, 3.0), (5, 1.0), (8, -1.0)] {
            let mut ev = script_decoder();
            run(
                &mut ev,
                &format!(
                    "G = {gens};
                     [P0, P1, C0, C1] = trellis(G);
                     t = poly2trellis(7, {oct});
                     seed({seed});
                     u = randi([0, 1], 1, 3000);
                     x = 1 - 2 * conv_encode(u, G);
                     Y = x + sqrt(1 / (2 * 0.5 * 10^({db} / 10))) * randn(size(x, 1), size(x, 2));
                     L = size(Y, 2);
                     s_ref = viterbi(Y, P0, P1, C0, C1);
                     s_nat = vitdec(reshape(Y, 1, 2 * L), t, L, \"term\", \"unquant\");
                     s_nat = s_nat(1:L-6);
                     H = sign(Y);
                     h_ref = viterbi(H, P0, P1, C0, C1);
                     h_nat = vitdec(reshape(H < 0, 1, 2 * L), t, L, \"term\", \"hard\");
                     h_nat = h_nat(1:L-6);
                     ds = sum(s_ref != s_nat); dh = sum(h_ref != h_nat);
                     es = sum(s_ref != u); eh = sum(h_ref != u);"
                ),
            )
            .unwrap();
            assert_eq!(
                reals(&ev, "s_nat"),
                reals(&ev, "s_ref"),
                "soft {oct} seed {seed}"
            );
            assert_eq!(
                reals(&ev, "h_nat"),
                reals(&ev, "h_ref"),
                "hard {oct} seed {seed}"
            );
            // The low-SNR cases make real errors, so agreement is not trivial.
            if db < 2.0 {
                assert!(scalar(&ev, "es") > 0.0 && scalar(&ev, "eh") > 0.0);
            }
        }
    }
}

/// Punctured rate 3/4 (erasures are zeros) and rate 1/3, against the
/// script's multi-frame decoder: one `vitdec` per frame.
#[test]
fn punctured_and_rate_one_third_match_the_script_decoder() {
    let mut ev = script_decoder();
    run(
        &mut ev,
        r#"
function U = script_frames(Y, F, P0, P1, C0, C1)
  L = size(Y, 2) / F;
  U = zeros(F, L - 6);
  for f = 1:F
    U(f, :) = viterbi(Y(:, (f - 1) * L + (1:L)), P0, P1, C0, C1);
  end
end

function U = native_frames(Y, F, t)
  n = size(Y, 1);
  L = size(Y, 2) / F;
  U = zeros(F, L - 6);
  for f = 1:F
    d = vitdec(reshape(Y(:, (f - 1) * L + (1:L)), 1, n * L), t, L, "term", "unquant");
    U(f, :) = d(1:L-6);
  end
end

G2 = [1 0 1 1 0 1 1; 1 1 1 1 0 0 1];
G3 = [1 0 1 1 0 1 1; 1 1 1 1 0 0 1; 1 1 1 0 1 0 1];
F = 5; N = 400; L = N + 6;
seed(17);
u = randi([0, 1], F, N);
stream = reshape([u, zeros(F, 6)]', 1, F * L);

[P0, P1, C0, C1] = trellis(G2);
X = 1 - 2 * conv_encode(stream(1 : F * L - 6), G2);
M = repmat([1 1 0; 1 0 1], 1, ceil(L / 3));
M = repmat(M(:, 1:L), 1, F);
Y = (X + 0.9 * randn(size(X, 1), size(X, 2))) .* M;
A2 = script_frames(Y, F, P0, P1, C0, C1);
B2 = native_frames(Y, F, poly2trellis(7, [133 171]));

[P0, P1, C0, C1] = trellis(G3);
X = 1 - 2 * conv_encode(stream(1 : F * L - 6), G3);
Y = X + 1.2 * randn(size(X, 1), size(X, 2));
A3 = script_frames(Y, F, P0, P1, C0, C1);
B3 = native_frames(Y, F, poly2trellis(7, [133 171 165]));
e2 = sum(sum(A2 != u)); e3 = sum(sum(A3 != u));
"#,
    )
    .unwrap();
    assert_eq!(reals(&ev, "B2"), reals(&ev, "A2"));
    assert_eq!(reals(&ev, "B3"), reals(&ev, "A3"));
    assert!(scalar(&ev, "e2") > 0.0 && scalar(&ev, "e3") > 0.0);
}

/// `soft` with `nsdec` bits decides exactly as `unquant` on the matching
/// levels: level v (0 = surest 0) is the real value 2^nsdec - 1 - 2v.
#[test]
fn soft_matches_unquant_on_the_same_levels() {
    let ev = eval(
        "t = poly2trellis(7, [171 133]);
         seed(9);
         q = randi([0, 7], 1, 4000);
         for mode = 1:3
           names = {\"trunc\", \"term\", \"cont\"};
           a = vitdec(q, t, 35, names(mode), \"soft\", 3);
           b = vitdec(7 - 2 * q, t, 35, names(mode), \"unquant\");
           d(mode) = sum(a != b);
         end",
    );
    assert_eq!(reals(&ev, "d"), vec![0.0, 0.0, 0.0]);
}

#[test]
fn output_orientation_and_length() {
    let ev = eval(
        "t = poly2trellis(3, [7 5]);
         r = vitdec([0 0 1 1 1 0 1 1], t, 2, \"trunc\", \"hard\");
         c = vitdec([0; 0; 1; 1; 1; 0; 1; 1], t, 2, \"trunc\", \"hard\");",
    );
    assert_eq!(shape(&ev, "r"), (1, 4));
    assert_eq!(shape(&ev, "c"), (4, 1));
    assert_eq!(reals(&ev, "r"), vec![0.0, 1.0, 0.0, 0.0]);
    assert_eq!(reals(&ev, "c"), reals(&ev, "r"));
}

#[test]
fn errors_name_the_problem() {
    let t = "t = poly2trellis(3, [7 5]);";
    for (src, want) in [
        (
            "vitdec([0 1 1], t, 2, \"trunc\", \"hard\")",
            "multiple of 2",
        ),
        ("vitdec([0 2 1 1], t, 2, \"trunc\", \"hard\")", "0 or 1"),
        ("vitdec([0 1 1 1], t, 0, \"trunc\", \"hard\")", "tblen"),
        ("vitdec([0 1 1 1], t, 2, \"bogus\", \"hard\")", "opmode"),
        ("vitdec([0 1 1 1], t, 2, \"trunc\", \"bogus\")", "dectype"),
        ("vitdec([0 1 1 1], t, 2, \"trunc\", \"soft\")", "nsdec"),
        ("vitdec([0 1 9 1], t, 2, \"trunc\", \"soft\", 3)", "0 to 7"),
        (
            "vitdec([0 1 NaN 1], t, 2, \"trunc\", \"unquant\")",
            "finite",
        ),
        ("vitdec([0 1 1 1], 5, 2, \"trunc\", \"hard\")", "trellis"),
        ("poly2trellis(3, [9 5])", "octal"),
        ("poly2trellis(3, [17 5])", "constraint length"),
    ] {
        let err = eval_err(&format!("{t}\n{src}"));
        assert!(err.contains(want), "{src}: {err}");
    }
}

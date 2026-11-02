//! `obj.name(args)`: indexing a struct field, or method-call sugar.

use rustlab_script::{Evaluator, Value};

fn eval(src: &str) -> Evaluator {
    let tokens = rustlab_script::lexer::tokenize(src).unwrap();
    let stmts = rustlab_script::parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    ev.run(&stmts)
        .unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"));
    ev
}

fn reals(ev: &Evaluator, name: &str) -> Vec<f64> {
    match ev.get(name) {
        Some(Value::Scalar(x)) => vec![*x],
        Some(Value::Vector(v)) => v.iter().map(|c| c.re).collect(),
        Some(Value::Matrix(m)) => m.iter().map(|c| c.re).collect(),
        other => panic!("{name}: expected a number or array, got {other:?}"),
    }
}

fn dims(ev: &Evaluator, name: &str) -> (usize, usize) {
    match ev.get(name) {
        Some(Value::Scalar(_)) => (1, 1),
        Some(Value::Vector(v)) => (1, v.len()),
        Some(Value::Matrix(m)) => (m.nrows(), m.ncols()),
        other => panic!("{name}: expected an array, got {other:?}"),
    }
}

#[test]
fn struct_field_is_indexed_not_called() {
    let ev = eval(
        "out.probe = [1, 2; 3, 4; 5, 6];
c = out.probe(:, 1);
r = out.probe(2, :);
x = out.probe(3, 2);
last = out.probe(end, end);
lin = out.probe(end);
M = out.probe;
cm = M(:, 1);
",
    );
    assert_eq!(reals(&ev, "c"), vec![1.0, 3.0, 5.0]);
    assert_eq!(dims(&ev, "c"), dims(&ev, "cm"));
    assert_eq!(reals(&ev, "r"), vec![3.0, 4.0]);
    assert_eq!(reals(&ev, "x"), vec![6.0]);
    assert_eq!(reals(&ev, "last"), vec![6.0]);
    assert_eq!(reals(&ev, "lin"), vec![6.0]);
}

#[test]
fn vector_field_binds_end_and_chains() {
    let ev = eval(
        "s.v = [10, 20, 30, 40];
a = s.v(end);
b = s.v(2:end);
q.w = [7, 8, 9];
t.inner = q;
w = t.inner.w(2);
f = @(k) s.v(k);
g = f(3);
",
    );
    assert_eq!(reals(&ev, "a"), vec![40.0]);
    assert_eq!(reals(&ev, "b"), vec![20.0, 30.0, 40.0]);
    assert_eq!(reals(&ev, "w"), vec![8.0]);
    assert_eq!(reals(&ev, "g"), vec![30.0]);
}

#[test]
fn a_field_shadows_a_function_of_the_same_name() {
    let ev = eval(
        "s.size = [7, 8, 9];
r = s.size(2);
",
    );
    assert_eq!(reals(&ev, "r"), vec![8.0]);
}

#[test]
fn method_sugar_still_calls() {
    let ev = eval(
        "function y = twice(s)
  y = 2 * s.x;
end
s.x = 3;
r = s.twice();
v = [3, 1, 2];
n = v.numel();
[m, i] = v.max();
",
    );
    assert_eq!(reals(&ev, "r"), vec![6.0]);
    assert_eq!(reals(&ev, "n"), vec![3.0]);
    assert_eq!(reals(&ev, "m"), vec![3.0]);
    assert_eq!(reals(&ev, "i"), vec![1.0]);
}

#[test]
fn statement_position_field_index_binds_ans() {
    let ev = eval(
        "s.v = [4, 5, 6];
s.v(size(s.v, 2))
",
    );
    assert_eq!(reals(&ev, "ans"), vec![6.0]);
}

#[test]
fn fdtd_record_probe_column() {
    let ev = eval(
        "sim = fdtd3d(8, 8, 8, 1e-3, 1e-3, 1e-3);
tau = 10 * fdtd_info(sim).dt;
fdtd_source(sim, \"ez\", 4, 4, 4, \"soft\", \"gaussian\", 4*tau, tau);
fdtd_probe(sim, \"ez\", 4, 6, 4);
fdtd_probe(sim, \"hy\", 4, 5, 4);
out = fdtd_run(sim, 30);
P = out.probe;
c1 = out.probe(:, 1);
c2 = out.probe(:, 2);
p1 = P(:, 1);
p2 = P(:, 2);
",
    );
    assert_eq!(reals(&ev, "c1"), reals(&ev, "p1"));
    assert_eq!(reals(&ev, "c2"), reals(&ev, "p2"));
    assert_eq!(dims(&ev, "c1"), dims(&ev, "p1"));
    assert_eq!(reals(&ev, "c1").len(), 30);
}

//! The `widget(name)` builtin (Phase 1 of the notebook interactive-widgets
//! work). A notebook executor installs a value table via
//! `Evaluator::with_widgets`; `widget("name")` then resolves against it.
//! Seeded with declared defaults by the executor, overlaid with live values
//! by the interactive server. Outside a notebook (no table) `widget()` is a
//! hard error, and an unknown name is a hard error too — a silent default
//! would mask a renamed/forgotten widget reference.

use std::collections::BTreeMap;
use std::sync::Arc;

use rustlab_script::{Evaluator, WidgetValue};

fn parse(src: &str) -> Vec<rustlab_script::ast::Stmt> {
    let tokens = rustlab_script::lexer::tokenize(src).unwrap();
    rustlab_script::parser::parse(tokens).unwrap()
}

fn table(pairs: &[(&str, WidgetValue)]) -> Arc<BTreeMap<String, WidgetValue>> {
    Arc::new(
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    )
}

#[test]
fn slider_value_resolves_to_scalar() {
    let mut ev = Evaluator::new().with_widgets(table(&[("cutoff", WidgetValue::Number(2.5))]));
    let stmts = parse("x = widget(\"cutoff\") * 2;");
    ev.run(&stmts).unwrap();
    assert_eq!(format!("{}", ev.get("x").unwrap()), "5");
}

#[test]
fn option_value_resolves_to_string() {
    let mut ev =
        Evaluator::new().with_widgets(table(&[("window", WidgetValue::Text("hann".into()))]));
    let stmts = parse("w = widget(\"window\");");
    ev.run(&stmts).unwrap();
    assert_eq!(format!("{}", ev.get("w").unwrap()), "hann");
}

#[test]
fn unknown_widget_is_a_hard_error() {
    let mut ev = Evaluator::new().with_widgets(table(&[("cutoff", WidgetValue::Number(1.0))]));
    let stmts = parse("y = widget(\"typo\");");
    let err = ev.run(&stmts).unwrap_err();
    assert!(
        err.to_string().contains("unknown widget 'typo'"),
        "expected unknown-widget error, got: {err}"
    );
    assert!(ev.get("y").is_none(), "assignment should not have happened");
}

#[test]
fn widget_without_a_table_errors() {
    // REPL / one-shot CLI: no notebook, no widget table installed.
    let mut ev = Evaluator::new();
    let stmts = parse("z = widget(\"cutoff\");");
    let err = ev.run(&stmts).unwrap_err();
    assert!(
        err.to_string().contains("only available when rendering a notebook"),
        "expected no-notebook error, got: {err}"
    );
}

#[test]
fn widget_requires_exactly_one_string_arg() {
    let mut ev = Evaluator::new().with_widgets(table(&[("cutoff", WidgetValue::Number(1.0))]));
    assert!(ev.run(&parse("a = widget();")).is_err());
    assert!(ev.run(&parse("b = widget(\"a\", \"b\");")).is_err());
    assert!(ev.run(&parse("c = widget(42);")).is_err());
}

//! Free-variable analysis for lambda capture.
//!
//! A lambda body is one expression. The language has no assignment
//! expression, so the body cannot introduce locals: the only names that
//! hide an outer binding are the lambda's parameters (and, inside a
//! nested lambda, that nested lambda's parameters). `end` is not a
//! captured variable. Indexing binds it for the duration of the
//! subscript and then removes it.
//!
//! `feval` is the one eval-like builtin. A string-literal target is a
//! name, so that name is part of the capture set. Any other target is
//! chosen at call time, and the lookup has to see the creation-time
//! workspace (`dynamic_feval`).

use crate::ast::Expr;
use std::collections::BTreeSet;

/// Names a lambda must copy out of the environment where it is created,
/// plus whether a dynamic `feval` forces a full snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaCapture {
    /// `feval` is called with a non-literal name somewhere in this body
    /// or in a nested lambda. The name is not known until the call, so
    /// the snapshot has to be the whole environment.
    pub dynamic_feval: bool,
    /// Free names. Empty when `dynamic_feval` is set (the caller clones
    /// the environment instead of consulting this set). Parameters and
    /// `end` are already removed.
    pub names: BTreeSet<String>,
}

/// Free names of `body` when the lambda is called with `params`.
///
/// Nested lambdas contribute the names *they* will snapshot when the
/// outer body runs, minus their own parameters. A name that is a
/// parameter of an inner lambda does not escape.
pub fn analyze_lambda(params: &[String], body: &Expr) -> LambdaCapture {
    let mut bound = BTreeSet::new();
    for p in params {
        bound.insert(p.clone());
    }
    let mut names = BTreeSet::new();
    let mut dynamic_feval = false;
    walk(body, &bound, &mut names, &mut dynamic_feval);
    if dynamic_feval {
        names.clear();
    }
    LambdaCapture {
        dynamic_feval,
        names,
    }
}

fn walk(
    expr: &Expr,
    bound: &BTreeSet<String>,
    names: &mut BTreeSet<String>,
    dynamic_feval: &mut bool,
) {
    match expr {
        Expr::Number(_) | Expr::Imag(_) | Expr::IntLit(_) | Expr::Str(_) | Expr::All => {}
        Expr::Var(n) => note_name(n, bound, names),
        Expr::UnaryMinus(e)
        | Expr::UnaryNot(e)
        | Expr::Transpose(e)
        | Expr::NonConjTranspose(e) => {
            walk(e, bound, names, dynamic_feval);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            walk(lhs, bound, names, dynamic_feval);
            walk(rhs, bound, names, dynamic_feval);
        }
        Expr::Call { name, args } => {
            // The callee may be a workspace value (indexing, or a stored
            // lambda / handle) rather than a builtin. Copy it when it
            // exists; a missing name still resolves to a user function
            // or a builtin at the call.
            note_name(name, bound, names);
            // `feval` is dispatched on the AST name, before the
            // environment lookup, so a parameter or variable of the
            // same name does not hide it.
            if name == "feval" && !args.is_empty() {
                match &args[0] {
                    Expr::Str(target) => {
                        // The string is a lookup key, not a mention of a
                        // parameter, so it is recorded even when it
                        // matches a parameter name. The parameter
                        // overwrites that slot at the call, which is the
                        // same shadowing a full snapshot has.
                        names.insert(target.clone());
                    }
                    _ => *dynamic_feval = true,
                }
            }
            for a in args {
                walk(a, bound, names, dynamic_feval);
            }
        }
        Expr::FuncHandle(n) => note_name(n, bound, names),
        Expr::Lambda { params, body } => {
            let mut inner_bound = bound.clone();
            for p in params {
                inner_bound.insert(p.clone());
            }
            walk(body, &inner_bound, names, dynamic_feval);
        }
        Expr::Matrix(rows) => {
            for row in rows {
                for c in row {
                    walk(c, bound, names, dynamic_feval);
                }
            }
        }
        Expr::CellArray(items) => {
            for item in items {
                walk(item, bound, names, dynamic_feval);
            }
        }
        Expr::Range { start, step, stop } => {
            walk(start, bound, names, dynamic_feval);
            if let Some(step) = step {
                walk(step, bound, names, dynamic_feval);
            }
            walk(stop, bound, names, dynamic_feval);
        }
        Expr::Field { object, .. } => walk(object, bound, names, dynamic_feval),
        Expr::Index { expr, args } => {
            walk(expr, bound, names, dynamic_feval);
            for a in args {
                walk(a, bound, names, dynamic_feval);
            }
        }
    }
}

/// `end` is bound by indexing for the subscript and is not a workspace
/// value a lambda can close over. Parameters are bound by the call.
fn note_name(name: &str, bound: &BTreeSet<String>, names: &mut BTreeSet<String>) {
    if name != "end" && !bound.contains(name) {
        names.insert(name.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expr, StmtKind};
    use crate::lexer;
    use crate::parser;

    fn analyze(src: &str) -> LambdaCapture {
        let tokens = lexer::tokenize(&format!("{src}\n")).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        match &stmts[0].kind {
            StmtKind::Expr(Expr::Lambda { params, body }, _) => analyze_lambda(params, body),
            other => panic!("expected a lambda expression, got {other:?}"),
        }
    }

    fn names(src: &str) -> BTreeSet<String> {
        let cap = analyze(src);
        assert!(!cap.dynamic_feval, "expected a static capture for {src}");
        cap.names
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn params_and_literals_are_not_free() {
        assert!(names("@(x) x + 1").is_empty());
        assert_eq!(names("@(x, y) x + y + a"), set(&["a"]));
    }

    #[test]
    fn call_and_handle_names_are_free() {
        assert_eq!(names("@(x) sin(x) + v(1)"), set(&["sin", "v"]));
        assert_eq!(names("@(x) @sq"), set(&["sq"]));
    }

    #[test]
    fn end_inside_an_index_is_not_free() {
        assert_eq!(names("@(i) M(i, end)"), set(&["M"]));
        assert_eq!(names("@(x) v(1:end)"), set(&["v"]));
        assert_eq!(names("@() M(end, end) + v(end)"), set(&["M", "v"]));
    }

    #[test]
    fn nested_lambda_propagates_free_names_not_its_params() {
        assert_eq!(names("@(x) @(y) x + y + a"), set(&["a"]));
        // Inner `x` shadows the outer parameter. Only `a` escapes.
        assert_eq!(names("@(x) @(x) x + a"), set(&["a"]));
        // Inner body mentions the outer parameter, which the outer
        // lambda binds itself, so the outer capture is empty.
        assert!(names("@(g) @(x) x + g").is_empty());
    }

    #[test]
    fn both_sides_of_short_circuit_are_free() {
        assert_eq!(
            names("@(x) (x > 0) && (a > 0) || (b < 0)"),
            set(&["a", "b"])
        );
    }

    #[test]
    fn field_object_is_free_and_the_field_name_is_not() {
        assert_eq!(names("@(x) s.gain + x"), set(&["s"]));
    }

    #[test]
    fn feval_literal_adds_the_target_name() {
        assert_eq!(names("@(n) feval(\"sq\", n)"), set(&["feval", "sq"]));
    }

    #[test]
    fn feval_of_a_computed_name_is_dynamic_and_propagates() {
        let cap = analyze("@(name, v) feval(name, v)");
        assert!(cap.dynamic_feval);
        assert!(cap.names.is_empty());

        let nested = analyze("@(x) @(name) feval(name, x)");
        assert!(nested.dynamic_feval);
    }
}

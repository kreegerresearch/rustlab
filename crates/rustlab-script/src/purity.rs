//! Two AST walkers gating which functions reach the persistent cache.
//!
//! - [`check_free_vars`] enforces the **purity contract** at the
//!   namespace level: a cached function may only reference names that
//!   are its parameters, its own locals, sibling functions in the same
//!   file, or known builtins. A reference to anything else (a global,
//!   a name from the REPL session) silently snapshots whatever value
//!   it held at hash time, which would corrupt the cache the moment
//!   the caller's scope drifts. Hard-reject is the v1 stance.
//!
//! - [`check_impurity`] enforces the same contract at the operations
//!   level: a cached function may not directly call a builtin from
//!   [`IMPURE_BUILTINS`] — RNG, clock readers, plotting, I/O. Same
//!   silent-staleness hazard if we let them through.
//!
//! v1 limitations the plan accepts:
//!
//! - **Direct calls only.** A function `f` that calls user-defined
//!   `g` which itself calls `rand` is not flagged. Phase 6 could add
//!   transitive analysis; until then, document this in user-facing
//!   docs alongside the "blow away the cache file if you don't trust
//!   it" escape hatch.
//! - **Name-based, not resolution-based.** A user function literally
//!   named `rand` will be flagged as impure even though it's their own
//!   pure function. Renaming sidesteps it; the false positive errs on
//!   the side of correctness.

use crate::ast::{Expr, Stmt, StmtKind};
use std::collections::BTreeSet;

/// Builtins whose results depend on hidden state (RNG, wall clock,
/// filesystem, terminal) or whose only purpose is side effects
/// (plotting, audio). Calls to any of these from inside a function
/// body disqualify it from caching.
///
/// Stored flat-sorted so `binary_search` in [`is_impure_builtin`] is
/// O(log n). The `denylist_is_sorted_for_binary_search` test enforces
/// this invariant. Categories (for human reviewers):
///
/// - RNG: `rand`, `randi`, `randn`, `randperm`
/// - Wall clock: `clock`, `now`, `tic`, `toc`
/// - User input: `input`, `keyboard`
/// - File I/O: `audio_play/read/write`, `fclose`/`fopen`/`fread`/`fwrite`,
///   `load`, `readmatrix`, `save`
/// - TTY side effects: `disp`, `fprintf`, `print`, `printf`
/// - Plotting (every call mutates plot thread-locals): `bar`, `figure`,
///   `grid`, `hold`, `legend`, `plot`, `scatter`, `stem`, `subplot`,
///   `title`, `xlabel`, `ylabel`
pub const IMPURE_BUILTINS: &[&str] = &[
    "audio_play",
    "audio_read",
    "audio_write",
    "bar",
    "clock",
    "disp",
    "fclose",
    "figure",
    "fopen",
    "fprintf",
    "fread",
    "fwrite",
    "grid",
    "hold",
    "input",
    "keyboard",
    "legend",
    "load",
    "now",
    "plot",
    "print",
    "printf",
    "rand",
    "randi",
    "randn",
    "randperm",
    "readmatrix",
    "save",
    "scatter",
    "stem",
    "subplot",
    "tic",
    "title",
    "toc",
    "xlabel",
    "ylabel",
];

/// Names referenced by a function body that are not bound by it.
/// Sorted, deduplicated. Empty `free_vars` means the function passes
/// the purity contract.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FreeVarReport {
    pub free_vars: Vec<String>,
}

impl FreeVarReport {
    pub fn is_clean(&self) -> bool {
        self.free_vars.is_empty()
    }
}

/// Direct impure-builtin calls found inside a function body. Sorted,
/// deduplicated. `is_clean()` means no flagged calls were found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImpurityReport {
    pub impure_calls: Vec<String>,
}

impl ImpurityReport {
    pub fn is_clean(&self) -> bool {
        self.impure_calls.is_empty()
    }
}

/// Walk `body` and collect every identifier referenced but not bound
/// by the function. `params` is the function's parameter list;
/// `is_builtin` returns `true` for any name registered as a builtin;
/// `is_sibling_fn` returns `true` for any function name defined
/// elsewhere in the same file (and therefore reachable from this body).
///
/// The walker treats every assignment target — `=`, `[a, b] = …`,
/// `name(i) = …`, `for v = …` — as binding a local for the duration
/// of the whole body. We don't track use-before-assign because the
/// script's runtime semantics allow forward references inside a
/// function body anyway.
pub fn check_free_vars<B, S>(
    body: &[Stmt],
    params: &[String],
    is_builtin: &B,
    is_sibling_fn: &S,
) -> FreeVarReport
where
    B: Fn(&str) -> bool,
    S: Fn(&str) -> bool,
{
    let mut locals: BTreeSet<String> = params.iter().cloned().collect();
    collect_locals(body, &mut locals);

    let mut referenced: BTreeSet<String> = BTreeSet::new();
    visit_stmts_for_var_refs(body, &mut referenced);

    let free_vars: Vec<String> = referenced
        .into_iter()
        .filter(|name| !locals.contains(name) && !is_builtin(name) && !is_sibling_fn(name))
        .collect();
    FreeVarReport { free_vars }
}

/// Walk `body` and collect every direct call to a name from
/// [`IMPURE_BUILTINS`]. Also catches `@name` function handles to
/// impure builtins, since `feval(@rand, …)` is just `rand(…)` with
/// indirection.
pub fn check_impurity(body: &[Stmt]) -> ImpurityReport {
    let mut found: BTreeSet<String> = BTreeSet::new();
    visit_stmts_for_impure_calls(body, &mut found);
    ImpurityReport {
        impure_calls: found.into_iter().collect(),
    }
}

/// Walk `body` and recursively check whether it (or any user-defined
/// function it calls transitively) calls an impure builtin. `user_fn_body`
/// is a closure that resolves a function name to its body — `None`
/// when the name isn't a known user function (builtin, lambda, etc.).
///
/// Cycle-safe: a function calling itself (direct or via a chain) is
/// walked exactly once. Self-recursion is therefore allowed without
/// false positives.
///
/// **Order-of-definition caveat:** a callee not yet present in
/// `user_fn_body` is treated as opaque (no impurity propagation). The
/// usual mitigation is a sweep on `cache enable` once all helpers are
/// defined; `cache add file` similarly installs every function from
/// the file before gating, so sibling calls within a file are fully
/// analysed.
pub fn check_transitive_impurity<'b, F>(
    body: &'b [Stmt],
    user_fn_body: &F,
) -> ImpurityReport
where
    F: for<'a> Fn(&'a str) -> Option<&'b [Stmt]>,
{
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    visit_transitive(body, user_fn_body, &mut found, &mut visited);
    ImpurityReport {
        impure_calls: found.into_iter().collect(),
    }
}

fn visit_transitive<'b, F>(
    body: &'b [Stmt],
    user_fn_body: &F,
    found: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) where
    F: for<'a> Fn(&'a str) -> Option<&'b [Stmt]>,
{
    // Direct hits at this layer.
    for name in check_impurity(body).impure_calls {
        found.insert(name);
    }
    // Then any user-fn callee we haven't walked yet.
    let mut callees: BTreeSet<String> = BTreeSet::new();
    visit_stmts_collect_call_names(body, &mut callees);
    for callee in callees {
        if !visited.insert(callee.clone()) {
            continue; // already walked this path — cycle or repeat
        }
        if let Some(callee_body) = user_fn_body(&callee) {
            visit_transitive(callee_body, user_fn_body, found, visited);
        }
    }
}

/// Collect every name appearing as the head of a call (`Expr::Call`)
/// or as a function handle (`Expr::FuncHandle`). Builtins included —
/// the caller filters by what counts as a user-fn.
fn visit_stmts_collect_call_names(stmts: &[Stmt], out: &mut BTreeSet<String>) {
    for stmt in stmts {
        visit_stmt_collect_call_names(stmt, out);
    }
}

fn visit_stmt_collect_call_names(stmt: &Stmt, out: &mut BTreeSet<String>) {
    match &stmt.kind {
        StmtKind::Assign { expr, .. }
        | StmtKind::FieldAssign { expr, .. }
        | StmtKind::MultiAssign { expr, .. } => visit_expr_collect_call_names(expr, out),
        StmtKind::Expr(e, _) => visit_expr_collect_call_names(e, out),
        StmtKind::If {
            cond,
            then_body,
            elseif_arms,
            else_body,
        } => {
            visit_expr_collect_call_names(cond, out);
            visit_stmts_collect_call_names(then_body, out);
            for (c, b) in elseif_arms {
                visit_expr_collect_call_names(c, out);
                visit_stmts_collect_call_names(b, out);
            }
            visit_stmts_collect_call_names(else_body, out);
        }
        StmtKind::Switch {
            expr,
            cases,
            otherwise,
        } => {
            visit_expr_collect_call_names(expr, out);
            for (c, b) in cases {
                visit_expr_collect_call_names(c, out);
                visit_stmts_collect_call_names(b, out);
            }
            visit_stmts_collect_call_names(otherwise, out);
        }
        StmtKind::For { iter, body, .. } => {
            visit_expr_collect_call_names(iter, out);
            visit_stmts_collect_call_names(body, out);
        }
        StmtKind::While { cond, body } => {
            visit_expr_collect_call_names(cond, out);
            visit_stmts_collect_call_names(body, out);
        }
        StmtKind::IndexAssign { indices, expr, .. } => {
            for i in indices {
                visit_expr_collect_call_names(i, out);
            }
            visit_expr_collect_call_names(expr, out);
        }
        StmtKind::FunctionDef { .. }
        | StmtKind::Return
        | StmtKind::Run { .. }
        | StmtKind::Format { .. }
        | StmtKind::Hold { .. }
        | StmtKind::Grid { .. }
        | StmtKind::Viewer { .. }
        | StmtKind::Cache(_) => {}
    }
}

fn visit_expr_collect_call_names(e: &Expr, out: &mut BTreeSet<String>) {
    match e {
        Expr::Call { name, args } => {
            out.insert(name.clone());
            for a in args {
                visit_expr_collect_call_names(a, out);
            }
        }
        Expr::FuncHandle(name) => {
            out.insert(name.clone());
        }
        Expr::Lambda { body, .. } => visit_expr_collect_call_names(body, out),
        Expr::BinOp { lhs, rhs, .. } => {
            visit_expr_collect_call_names(lhs, out);
            visit_expr_collect_call_names(rhs, out);
        }
        Expr::UnaryMinus(e) | Expr::UnaryNot(e) => visit_expr_collect_call_names(e, out),
        Expr::Matrix(rows) => {
            for row in rows {
                for c in row {
                    visit_expr_collect_call_names(c, out);
                }
            }
        }
        Expr::CellArray(items) => {
            for i in items {
                visit_expr_collect_call_names(i, out);
            }
        }
        Expr::Range { start, step, stop } => {
            visit_expr_collect_call_names(start, out);
            if let Some(s) = step {
                visit_expr_collect_call_names(s, out);
            }
            visit_expr_collect_call_names(stop, out);
        }
        Expr::Transpose(e) | Expr::NonConjTranspose(e) => visit_expr_collect_call_names(e, out),
        Expr::Field { object, .. } => visit_expr_collect_call_names(object, out),
        Expr::Index { expr, args } => {
            visit_expr_collect_call_names(expr, out);
            for a in args {
                visit_expr_collect_call_names(a, out);
            }
        }
        Expr::Number(_) | Expr::Str(_) | Expr::Var(_) | Expr::All => {}
    }
}

/// Convenience: `true` iff `name` is in the impure-builtin denylist.
pub fn is_impure_builtin(name: &str) -> bool {
    IMPURE_BUILTINS.binary_search(&name).is_ok()
}

// ── locals collection ────────────────────────────────────────────────

fn collect_locals(stmts: &[Stmt], out: &mut BTreeSet<String>) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Assign { name, .. }
            | StmtKind::IndexAssign { name, .. }
            | StmtKind::FieldAssign { object: name, .. } => {
                out.insert(name.clone());
            }
            StmtKind::MultiAssign { names, .. } => {
                for n in names {
                    out.insert(n.clone());
                }
            }
            StmtKind::For { var, body, .. } => {
                out.insert(var.clone());
                collect_locals(body, out);
            }
            StmtKind::While { body, .. } => collect_locals(body, out),
            StmtKind::If {
                then_body,
                elseif_arms,
                else_body,
                ..
            } => {
                collect_locals(then_body, out);
                for (_, b) in elseif_arms {
                    collect_locals(b, out);
                }
                collect_locals(else_body, out);
            }
            StmtKind::Switch {
                cases, otherwise, ..
            } => {
                for (_, b) in cases {
                    collect_locals(b, out);
                }
                collect_locals(otherwise, out);
            }
            StmtKind::FunctionDef { name, .. } => {
                // A nested function defines a name in the enclosing
                // scope (it can be `@`-referenced). Its own body is a
                // separate scope and is NOT walked here — Phase 3
                // calls check_free_vars on each FunctionDef
                // independently.
                out.insert(name.clone());
            }
            // Statements that don't bind names. `Cache` is a
            // top-level directive; it doesn't introduce locals into
            // the surrounding scope.
            StmtKind::Expr(_, _)
            | StmtKind::Return
            | StmtKind::Run { .. }
            | StmtKind::Format { .. }
            | StmtKind::Hold { .. }
            | StmtKind::Grid { .. }
            | StmtKind::Viewer { .. }
            | StmtKind::Cache(_) => {}
        }
    }
}

// ── variable-reference walker ────────────────────────────────────────

fn visit_stmts_for_var_refs(stmts: &[Stmt], out: &mut BTreeSet<String>) {
    for stmt in stmts {
        visit_stmt_for_var_refs(stmt, out);
    }
}

fn visit_stmt_for_var_refs(stmt: &Stmt, out: &mut BTreeSet<String>) {
    match &stmt.kind {
        StmtKind::Assign { expr, .. } => visit_expr_for_var_refs(expr, out),
        StmtKind::Expr(e, _) => visit_expr_for_var_refs(e, out),
        StmtKind::FunctionDef { .. } => {
            // Nested function bodies aren't walked here; see
            // collect_locals comment.
        }
        StmtKind::FieldAssign { expr, .. } => visit_expr_for_var_refs(expr, out),
        StmtKind::Return => {}
        StmtKind::If {
            cond,
            then_body,
            elseif_arms,
            else_body,
        } => {
            visit_expr_for_var_refs(cond, out);
            visit_stmts_for_var_refs(then_body, out);
            for (c, b) in elseif_arms {
                visit_expr_for_var_refs(c, out);
                visit_stmts_for_var_refs(b, out);
            }
            visit_stmts_for_var_refs(else_body, out);
        }
        StmtKind::Switch {
            expr,
            cases,
            otherwise,
        } => {
            visit_expr_for_var_refs(expr, out);
            for (c, b) in cases {
                visit_expr_for_var_refs(c, out);
                visit_stmts_for_var_refs(b, out);
            }
            visit_stmts_for_var_refs(otherwise, out);
        }
        StmtKind::MultiAssign { expr, .. } => visit_expr_for_var_refs(expr, out),
        StmtKind::For { iter, body, .. } => {
            visit_expr_for_var_refs(iter, out);
            visit_stmts_for_var_refs(body, out);
        }
        StmtKind::While { cond, body } => {
            visit_expr_for_var_refs(cond, out);
            visit_stmts_for_var_refs(body, out);
        }
        StmtKind::IndexAssign { indices, expr, .. } => {
            for i in indices {
                visit_expr_for_var_refs(i, out);
            }
            visit_expr_for_var_refs(expr, out);
        }
        StmtKind::Run { .. }
        | StmtKind::Format { .. }
        | StmtKind::Hold { .. }
        | StmtKind::Grid { .. }
        | StmtKind::Viewer { .. }
        | StmtKind::Cache(_) => {}
    }
}

fn visit_expr_for_var_refs(e: &Expr, out: &mut BTreeSet<String>) {
    match e {
        Expr::Var(n) => {
            out.insert(n.clone());
        }
        Expr::Call { name, args } => {
            out.insert(name.clone());
            for a in args {
                visit_expr_for_var_refs(a, out);
            }
        }
        Expr::FuncHandle(n) => {
            out.insert(n.clone());
        }
        Expr::Lambda { params, body } => {
            // Lambda creates an inner scope. Names referenced by the
            // body that aren't lambda params still count as references
            // from the enclosing function — that's the whole point of
            // capture. Filter the lambda params out so they don't
            // bubble up as "free."
            let mut inner: BTreeSet<String> = BTreeSet::new();
            visit_expr_for_var_refs(body, &mut inner);
            for n in inner {
                if !params.contains(&n) {
                    out.insert(n);
                }
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            visit_expr_for_var_refs(lhs, out);
            visit_expr_for_var_refs(rhs, out);
        }
        Expr::UnaryMinus(e) | Expr::UnaryNot(e) => visit_expr_for_var_refs(e, out),
        Expr::Matrix(rows) => {
            for row in rows {
                for c in row {
                    visit_expr_for_var_refs(c, out);
                }
            }
        }
        Expr::CellArray(items) => {
            for i in items {
                visit_expr_for_var_refs(i, out);
            }
        }
        Expr::Range { start, step, stop } => {
            visit_expr_for_var_refs(start, out);
            if let Some(s) = step {
                visit_expr_for_var_refs(s, out);
            }
            visit_expr_for_var_refs(stop, out);
        }
        Expr::Transpose(e) | Expr::NonConjTranspose(e) => visit_expr_for_var_refs(e, out),
        Expr::Field { object, .. } => visit_expr_for_var_refs(object, out),
        Expr::Index { expr, args } => {
            visit_expr_for_var_refs(expr, out);
            for a in args {
                visit_expr_for_var_refs(a, out);
            }
        }
        Expr::Number(_) | Expr::Str(_) | Expr::All => {}
    }
}

// ── impurity walker ──────────────────────────────────────────────────

fn visit_stmts_for_impure_calls(stmts: &[Stmt], out: &mut BTreeSet<String>) {
    for stmt in stmts {
        visit_stmt_for_impure_calls(stmt, out);
    }
}

fn visit_stmt_for_impure_calls(stmt: &Stmt, out: &mut BTreeSet<String>) {
    match &stmt.kind {
        StmtKind::Assign { expr, .. }
        | StmtKind::FieldAssign { expr, .. }
        | StmtKind::MultiAssign { expr, .. } => visit_expr_for_impure_calls(expr, out),
        StmtKind::Expr(e, _) => visit_expr_for_impure_calls(e, out),
        StmtKind::FunctionDef { .. } => {}
        StmtKind::Return => {}
        StmtKind::If {
            cond,
            then_body,
            elseif_arms,
            else_body,
        } => {
            visit_expr_for_impure_calls(cond, out);
            visit_stmts_for_impure_calls(then_body, out);
            for (c, b) in elseif_arms {
                visit_expr_for_impure_calls(c, out);
                visit_stmts_for_impure_calls(b, out);
            }
            visit_stmts_for_impure_calls(else_body, out);
        }
        StmtKind::Switch {
            expr,
            cases,
            otherwise,
        } => {
            visit_expr_for_impure_calls(expr, out);
            for (c, b) in cases {
                visit_expr_for_impure_calls(c, out);
                visit_stmts_for_impure_calls(b, out);
            }
            visit_stmts_for_impure_calls(otherwise, out);
        }
        StmtKind::For { iter, body, .. } => {
            visit_expr_for_impure_calls(iter, out);
            visit_stmts_for_impure_calls(body, out);
        }
        StmtKind::While { cond, body } => {
            visit_expr_for_impure_calls(cond, out);
            visit_stmts_for_impure_calls(body, out);
        }
        StmtKind::IndexAssign { indices, expr, .. } => {
            for i in indices {
                visit_expr_for_impure_calls(i, out);
            }
            visit_expr_for_impure_calls(expr, out);
        }
        // The Hold / Grid / Viewer directives are themselves graphics
        // side effects — flag them so the user gets the same "this
        // function can't be cached" message they would for `plot(…)`.
        StmtKind::Hold { .. } => {
            out.insert("hold".to_string());
        }
        StmtKind::Grid { .. } => {
            out.insert("grid".to_string());
        }
        StmtKind::Viewer { .. } => {
            out.insert("viewer".to_string());
        }
        // Cache management itself is a side effect, but the cache
        // statement only legitimately appears at file/REPL scope —
        // not inside a function body. We don't synthesize a fake
        // impure-call entry for it here; an enclosing function that
        // contains `cache enable` would already fail caching for
        // structural reasons (top-level directive in nested position).
        StmtKind::Format { .. } | StmtKind::Run { .. } | StmtKind::Cache(_) => {}
    }
}

fn visit_expr_for_impure_calls(e: &Expr, out: &mut BTreeSet<String>) {
    match e {
        Expr::Call { name, args } => {
            if is_impure_builtin(name) {
                out.insert(name.clone());
            }
            for a in args {
                visit_expr_for_impure_calls(a, out);
            }
        }
        Expr::FuncHandle(name) => {
            if is_impure_builtin(name) {
                out.insert(name.clone());
            }
        }
        Expr::Lambda { body, .. } => visit_expr_for_impure_calls(body, out),
        Expr::BinOp { lhs, rhs, .. } => {
            visit_expr_for_impure_calls(lhs, out);
            visit_expr_for_impure_calls(rhs, out);
        }
        Expr::UnaryMinus(e) | Expr::UnaryNot(e) => visit_expr_for_impure_calls(e, out),
        Expr::Matrix(rows) => {
            for row in rows {
                for c in row {
                    visit_expr_for_impure_calls(c, out);
                }
            }
        }
        Expr::CellArray(items) => {
            for i in items {
                visit_expr_for_impure_calls(i, out);
            }
        }
        Expr::Range { start, step, stop } => {
            visit_expr_for_impure_calls(start, out);
            if let Some(s) = step {
                visit_expr_for_impure_calls(s, out);
            }
            visit_expr_for_impure_calls(stop, out);
        }
        Expr::Transpose(e) | Expr::NonConjTranspose(e) => visit_expr_for_impure_calls(e, out),
        Expr::Field { object, .. } => visit_expr_for_impure_calls(object, out),
        Expr::Index { expr, args } => {
            visit_expr_for_impure_calls(expr, out);
            for a in args {
                visit_expr_for_impure_calls(a, out);
            }
        }
        Expr::Number(_) | Expr::Str(_) | Expr::Var(_) | Expr::All => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::StmtKind;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn parse_one_fn(src: &str) -> (Vec<String>, Vec<Stmt>) {
        let stmts = parse(tokenize(src).expect("tokenize")).expect("parse");
        for s in stmts {
            if let StmtKind::FunctionDef { params, body, .. } = s.kind {
                return (params, body);
            }
        }
        panic!("source contained no FunctionDef");
    }

    fn no_builtins(_: &str) -> bool {
        false
    }
    fn no_siblings(_: &str) -> bool {
        false
    }

    #[test]
    fn denylist_is_sorted_for_binary_search() {
        // is_impure_builtin uses binary_search; that requires sorted
        // order. Also assert there are no duplicates.
        let mut sorted_dedup = IMPURE_BUILTINS.to_vec();
        sorted_dedup.sort();
        sorted_dedup.dedup();
        assert_eq!(
            IMPURE_BUILTINS.len(),
            sorted_dedup.len(),
            "IMPURE_BUILTINS contains duplicates"
        );
        let mut sorted = IMPURE_BUILTINS.to_vec();
        sorted.sort();
        assert_eq!(
            IMPURE_BUILTINS.iter().copied().collect::<Vec<_>>(),
            sorted,
            "IMPURE_BUILTINS must stay flat-sorted for binary_search"
        );
    }

    // ── free-var walker ──────────────────────────────────────────

    #[test]
    fn pure_function_has_no_free_vars() {
        let (params, body) = parse_one_fn(
            "function y = f(x)\n  y = x + 1\nend\n",
        );
        let report = check_free_vars(&body, &params, &no_builtins, &no_siblings);
        assert!(report.is_clean(), "unexpected free vars: {:?}", report);
    }

    #[test]
    fn unbound_name_is_flagged_as_free() {
        let (params, body) = parse_one_fn(
            "function y = f(x)\n  y = x + k\nend\n",
        );
        let report = check_free_vars(&body, &params, &no_builtins, &no_siblings);
        assert_eq!(report.free_vars, vec!["k".to_string()]);
    }

    #[test]
    fn locals_assigned_in_body_are_not_free() {
        let (params, body) = parse_one_fn(
            "function y = f(x)\n  k = 2\n  y = x + k\nend\n",
        );
        let report = check_free_vars(&body, &params, &no_builtins, &no_siblings);
        assert!(report.is_clean(), "{:?}", report);
    }

    #[test]
    fn for_loop_var_counts_as_local() {
        let (params, body) = parse_one_fn(
            "function y = f(x)\n  y = 0\n  for i = 1:10\n    y = y + i\n  end\nend\n",
        );
        let report = check_free_vars(&body, &params, &no_builtins, &no_siblings);
        assert!(report.is_clean(), "{:?}", report);
    }

    #[test]
    fn multi_assign_targets_are_locals() {
        // `[a, b] = expr` should introduce a and b as locals.
        let (params, body) = parse_one_fn(
            "function y = f(x)\n  [a, b] = size(x)\n  y = a + b\nend\n",
        );
        // `size` is a builtin in our hypothetical environment.
        let is_builtin = |n: &str| n == "size";
        let report = check_free_vars(&body, &params, &is_builtin, &no_siblings);
        assert!(report.is_clean(), "{:?}", report);
    }

    #[test]
    fn builtin_callback_suppresses_flag() {
        let (params, body) = parse_one_fn(
            "function y = f(x)\n  y = sqrt(x)\nend\n",
        );
        let is_builtin = |n: &str| n == "sqrt";
        let report = check_free_vars(&body, &params, &is_builtin, &no_siblings);
        assert!(report.is_clean());
    }

    #[test]
    fn sibling_callback_suppresses_flag() {
        let (params, body) = parse_one_fn(
            "function y = f(x)\n  y = helper(x) + 1\nend\n",
        );
        let is_sibling = |n: &str| n == "helper";
        let report = check_free_vars(&body, &params, &no_builtins, &is_sibling);
        assert!(report.is_clean());
    }

    #[test]
    fn lambda_params_dont_leak_out() {
        // The lambda parameter `t` is bound inside the lambda and
        // should NOT bubble up as a free var of `f`.
        let (params, body) = parse_one_fn(
            "function y = f(x)\n  g = @(t) t + x\n  y = g(1)\nend\n",
        );
        let report = check_free_vars(&body, &params, &no_builtins, &no_siblings);
        assert!(report.is_clean(), "{:?}", report);
    }

    #[test]
    fn lambda_body_can_still_capture_free_var() {
        // Lambda references `k` which is neither its param nor bound
        // in the enclosing scope.
        let (params, body) = parse_one_fn(
            "function y = f(x)\n  g = @(t) t + k\n  y = g(1)\nend\n",
        );
        let report = check_free_vars(&body, &params, &no_builtins, &no_siblings);
        assert_eq!(report.free_vars, vec!["k".to_string()]);
    }

    #[test]
    fn function_handle_reference_counts_as_use() {
        let (params, body) = parse_one_fn(
            "function y = f(x)\n  y = feval(@helper, x)\nend\n",
        );
        let is_builtin = |n: &str| n == "feval";
        // helper isn't a sibling or builtin → flagged.
        let report = check_free_vars(&body, &params, &is_builtin, &no_siblings);
        assert_eq!(report.free_vars, vec!["helper".to_string()]);
    }

    // ── impurity walker ──────────────────────────────────────────

    #[test]
    fn pure_function_has_no_impure_calls() {
        let (_, body) = parse_one_fn(
            "function y = f(x)\n  y = sqrt(x * x)\nend\n",
        );
        let report = check_impurity(&body);
        assert!(report.is_clean(), "{:?}", report);
    }

    #[test]
    fn direct_rand_call_is_flagged() {
        let (_, body) = parse_one_fn(
            "function y = f(x)\n  y = x + rand()\nend\n",
        );
        let report = check_impurity(&body);
        assert_eq!(report.impure_calls, vec!["rand".to_string()]);
    }

    #[test]
    fn plot_call_is_flagged() {
        let (_, body) = parse_one_fn(
            "function y = f(x)\n  plot(x)\n  y = x\nend\n",
        );
        let report = check_impurity(&body);
        assert_eq!(report.impure_calls, vec!["plot".to_string()]);
    }

    #[test]
    fn nested_call_in_expression_is_found() {
        // `2 + rand()` — the rand call is nested inside a BinOp.
        let (_, body) = parse_one_fn(
            "function y = f(x)\n  y = x + 2 + rand()\nend\n",
        );
        let report = check_impurity(&body);
        assert_eq!(report.impure_calls, vec!["rand".to_string()]);
    }

    #[test]
    fn func_handle_to_impure_is_flagged() {
        // Aliasing `rand` via @rand still gets called somewhere — the
        // walker flags the handle even if the actual call site is
        // hidden in a builtin like feval.
        let (_, body) = parse_one_fn(
            "function y = f(x)\n  g = @rand\n  y = x\nend\n",
        );
        let report = check_impurity(&body);
        assert_eq!(report.impure_calls, vec!["rand".to_string()]);
    }

    #[test]
    fn hold_directive_is_flagged_as_graphics_side_effect() {
        let (_, body) = parse_one_fn(
            "function y = f(x)\n  hold on\n  y = x\nend\n",
        );
        let report = check_impurity(&body);
        assert_eq!(report.impure_calls, vec!["hold".to_string()]);
    }

    #[test]
    fn multiple_distinct_impure_calls_are_collected() {
        let (_, body) = parse_one_fn(
            "function y = f(x)\n  plot(x)\n  y = x + rand()\nend\n",
        );
        let report = check_impurity(&body);
        // Sorted alphabetically (BTreeSet ordering).
        assert_eq!(
            report.impure_calls,
            vec!["plot".to_string(), "rand".to_string()],
        );
    }

    #[test]
    fn transitive_impurity_is_intentionally_missed_in_v1() {
        // `f` calls `g`; `g` calls `rand`. Direct-only walker on f's
        // body sees only `g` — which isn't on the denylist — so the
        // report is clean. This documents the limitation; Phase 6
        // would add transitive analysis.
        let (_, body) = parse_one_fn(
            "function y = f(x)\n  y = g(x)\nend\n",
        );
        let report = check_impurity(&body);
        assert!(
            report.is_clean(),
            "v1 walker only checks direct calls — transitive miss is documented"
        );
    }

    #[test]
    fn is_impure_builtin_works() {
        assert!(is_impure_builtin("rand"));
        assert!(is_impure_builtin("plot"));
        assert!(is_impure_builtin("tic"));
        assert!(!is_impure_builtin("sqrt"));
        assert!(!is_impure_builtin("not_a_builtin"));
    }
}

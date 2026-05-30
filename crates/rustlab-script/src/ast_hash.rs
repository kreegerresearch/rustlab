//! Stable BLAKE3 fingerprint of a parsed AST.
//!
//! The persistent function-result cache uses these hashes to key on
//! "this exact function body, regardless of where it appears in the
//! file or what whitespace surrounds it." Rules:
//!
//! - **Skip every `Stmt.line` field.** Moving a function down by a
//!   line must NOT bust the cache.
//! - **Identifier renames bust the cache** intentionally. A rename
//!   changes the semantics whenever the function captures the renamed
//!   name from an outer scope; we err on the side of correctness.
//! - **`f64` literals are hashed by bit pattern**, including NaN — a
//!   NaN-shaped source token is part of the structure of the program,
//!   not data input. (Contrast with `rustlab_core::Fingerprint` for
//!   runtime f64s, which rejects NaN to bypass the cache.)
//! - **Variable-length data is length-prefixed** so `(a, b)` and `(ab,)`
//!   never collide.
//! - **Domain-separator tags** per `Stmt` / `Expr` variant prevent
//!   distinct nodes with similar bytes from colliding.
//!
//! Wire format version is encoded in the leading [`FILE_TAG`] /
//! [`INLINE_TAG`] / [`FN_ENTRY_TAG`] byte strings. Bumping any AST
//! variant's tag requires bumping the relevant `vN` suffix, which
//! cleanly invalidates every cache entry written by an older binary.

use crate::ast::{BinOp, CacheStmt, Expr, Stmt, StmtKind};
use std::collections::{BTreeMap, BTreeSet};

const FILE_TAG: &[u8] = b"rustlab-ast/file/v1\0";
const INLINE_TAG: &[u8] = b"rustlab-ast/inline-fn/v1\0";
const FN_ENTRY_TAG: &[u8] = b"rustlab-ast/fn-entry/v1\0";
const CANONICAL_TAG: &[u8] = b"rustlab-ast/canonical/v1\0";

/// Hash an entire parsed file's top-level statements. Used as the
/// "file AST hash" component of cache entry identity for functions
/// loaded via `cache add file ...`.
pub fn hash_stmts(stmts: &[Stmt]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(FILE_TAG);
    feed_stmts(&mut h, stmts);
    *h.finalize().as_bytes()
}

/// Hash one inline function definition's signature + body. Used for
/// functions defined directly in a REPL session or a notebook code
/// block, where there is no enclosing file to anchor identity.
pub fn hash_function_body(
    name: &str,
    params: &[String],
    return_vars: &[String],
    body: &[Stmt],
) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(INLINE_TAG);
    feed_str(&mut h, name);
    feed_names(&mut h, params);
    feed_names(&mut h, return_vars);
    feed_stmts(&mut h, body);
    *h.finalize().as_bytes()
}

/// Derive a per-function cache entry id from a file's AST hash and a
/// function name. This is the value stored in the `entry_id` column of
/// `cache_entries` for functions loaded from a file.
pub fn function_entry_id(file_ast_hash: &[u8; 32], fn_name: &str) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(FN_ENTRY_TAG);
    h.update(file_ast_hash);
    feed_str(&mut h, fn_name);
    *h.finalize().as_bytes()
}

// ── walker ───────────────────────────────────────────────────────────

fn feed_stmts(h: &mut blake3::Hasher, stmts: &[Stmt]) {
    h.update(&(stmts.len() as u64).to_le_bytes());
    for stmt in stmts {
        // Deliberately skip stmt.line — see module docs.
        feed_stmt_kind(h, &stmt.kind);
    }
}

fn feed_stmt_kind(h: &mut blake3::Hasher, kind: &StmtKind) {
    match kind {
        StmtKind::Assign {
            name,
            expr,
            suppress,
        } => {
            h.update(&[0x01]);
            feed_str(h, name);
            feed_expr(h, expr);
            feed_bool(h, *suppress);
        }
        StmtKind::Expr(e, suppress) => {
            h.update(&[0x02]);
            feed_expr(h, e);
            feed_bool(h, *suppress);
        }
        StmtKind::FunctionDef {
            name,
            params,
            return_vars,
            body,
        } => {
            h.update(&[0x03]);
            feed_str(h, name);
            feed_names(h, params);
            feed_names(h, return_vars);
            feed_stmts(h, body);
        }
        StmtKind::FieldAssign {
            object,
            field,
            expr,
            suppress,
        } => {
            h.update(&[0x04]);
            feed_str(h, object);
            feed_str(h, field);
            feed_expr(h, expr);
            feed_bool(h, *suppress);
        }
        StmtKind::Return => {
            h.update(&[0x05]);
        }
        StmtKind::If {
            cond,
            then_body,
            elseif_arms,
            else_body,
        } => {
            h.update(&[0x06]);
            feed_expr(h, cond);
            feed_stmts(h, then_body);
            h.update(&(elseif_arms.len() as u64).to_le_bytes());
            for (c, b) in elseif_arms {
                feed_expr(h, c);
                feed_stmts(h, b);
            }
            feed_stmts(h, else_body);
        }
        StmtKind::Switch {
            expr,
            cases,
            otherwise,
        } => {
            h.update(&[0x07]);
            feed_expr(h, expr);
            h.update(&(cases.len() as u64).to_le_bytes());
            for (c, b) in cases {
                feed_expr(h, c);
                feed_stmts(h, b);
            }
            feed_stmts(h, otherwise);
        }
        StmtKind::Run { path } => {
            h.update(&[0x08]);
            feed_str(h, path);
        }
        StmtKind::Format { mode } => {
            h.update(&[0x09]);
            feed_str(h, mode);
        }
        StmtKind::Hold { on } => {
            h.update(&[0x0a]);
            feed_bool(h, *on);
        }
        StmtKind::Grid { on } => {
            h.update(&[0x0b]);
            feed_bool(h, *on);
        }
        StmtKind::Viewer { on, name } => {
            h.update(&[0x0c]);
            match on {
                None => {
                    h.update(&[0u8]);
                }
                Some(b) => {
                    h.update(&[1u8]);
                    feed_bool(h, *b);
                }
            }
            match name {
                None => {
                    h.update(&[0u8]);
                }
                Some(n) => {
                    h.update(&[1u8]);
                    feed_str(h, n);
                }
            }
        }
        StmtKind::MultiAssign {
            names,
            expr,
            suppress,
        } => {
            h.update(&[0x0d]);
            feed_names(h, names);
            feed_expr(h, expr);
            feed_bool(h, *suppress);
        }
        StmtKind::For { var, iter, body } => {
            h.update(&[0x0e]);
            feed_str(h, var);
            feed_expr(h, iter);
            feed_stmts(h, body);
        }
        StmtKind::While { cond, body } => {
            h.update(&[0x0f]);
            feed_expr(h, cond);
            feed_stmts(h, body);
        }
        StmtKind::IndexAssign {
            name,
            indices,
            expr,
            suppress,
        } => {
            h.update(&[0x10]);
            feed_str(h, name);
            h.update(&(indices.len() as u64).to_le_bytes());
            for idx in indices {
                feed_expr(h, idx);
            }
            feed_expr(h, expr);
            feed_bool(h, *suppress);
        }
        StmtKind::Cache(c) => {
            h.update(&[0x11]);
            feed_cache_stmt(h, c);
        }
    }
}

fn feed_cache_stmt(h: &mut blake3::Hasher, c: &CacheStmt) {
    match c {
        CacheStmt::Enable { path } => {
            h.update(&[0x80]);
            match path {
                None => {
                    h.update(&[0u8]);
                }
                Some(p) => {
                    h.update(&[1u8]);
                    feed_str(h, p);
                }
            }
        }
        CacheStmt::Off => {
            h.update(&[0x81]);
        }
        CacheStmt::AddFile { path } => {
            h.update(&[0x82]);
            feed_str(h, path);
        }
        CacheStmt::AddFunctions { names } => {
            h.update(&[0x83]);
            feed_names(h, names);
        }
        CacheStmt::RemoveFunction { name } => {
            h.update(&[0x84]);
            feed_str(h, name);
        }
        CacheStmt::Status => {
            h.update(&[0x85]);
        }
        CacheStmt::Clear => {
            h.update(&[0x86]);
        }
        CacheStmt::List { limit } => {
            h.update(&[0x88]);
            match limit {
                None => {
                    h.update(&[0u8]);
                }
                Some(n) => {
                    h.update(&[1u8]);
                    h.update(&n.to_le_bytes());
                }
            }
        }
        CacheStmt::Prune {
            older,
            max_size_bytes,
        } => {
            h.update(&[0x87]);
            match older {
                None => {
                    h.update(&[0u8]);
                }
                Some(s) => {
                    h.update(&[1u8]);
                    feed_str(h, s);
                }
            }
            match max_size_bytes {
                None => {
                    h.update(&[0u8]);
                }
                Some(b) => {
                    h.update(&[1u8]);
                    h.update(&b.to_le_bytes());
                }
            }
        }
    }
}

fn feed_expr(h: &mut blake3::Hasher, e: &Expr) {
    match e {
        Expr::Number(n) => {
            h.update(&[0x20]);
            // Source-level f64: keep the bit pattern. NaN in source
            // is structural ("the user typed `nan`"), not invalid data.
            h.update(&n.to_le_bytes());
        }
        Expr::Str(s) => {
            h.update(&[0x21]);
            feed_str(h, s);
        }
        Expr::Var(n) => {
            h.update(&[0x22]);
            feed_str(h, n);
        }
        Expr::BinOp { op, lhs, rhs } => {
            h.update(&[0x23]);
            feed_binop(h, *op);
            feed_expr(h, lhs);
            feed_expr(h, rhs);
        }
        Expr::UnaryMinus(e) => {
            h.update(&[0x24]);
            feed_expr(h, e);
        }
        Expr::UnaryNot(e) => {
            h.update(&[0x25]);
            feed_expr(h, e);
        }
        Expr::Call { name, args } => {
            h.update(&[0x26]);
            feed_str(h, name);
            h.update(&(args.len() as u64).to_le_bytes());
            for a in args {
                feed_expr(h, a);
            }
        }
        Expr::Matrix(rows) => {
            h.update(&[0x27]);
            h.update(&(rows.len() as u64).to_le_bytes());
            for row in rows {
                h.update(&(row.len() as u64).to_le_bytes());
                for cell in row {
                    feed_expr(h, cell);
                }
            }
        }
        Expr::CellArray(items) => {
            h.update(&[0x28]);
            h.update(&(items.len() as u64).to_le_bytes());
            for x in items {
                feed_expr(h, x);
            }
        }
        Expr::Range { start, step, stop } => {
            h.update(&[0x29]);
            feed_expr(h, start);
            match step {
                None => {
                    h.update(&[0u8]);
                }
                Some(s) => {
                    h.update(&[1u8]);
                    feed_expr(h, s);
                }
            }
            feed_expr(h, stop);
        }
        Expr::Transpose(e) => {
            h.update(&[0x2a]);
            feed_expr(h, e);
        }
        Expr::NonConjTranspose(e) => {
            h.update(&[0x2b]);
            feed_expr(h, e);
        }
        Expr::All => {
            h.update(&[0x2c]);
        }
        Expr::Field { object, field } => {
            h.update(&[0x2d]);
            feed_expr(h, object);
            feed_str(h, field);
        }
        Expr::Index { expr, args } => {
            h.update(&[0x2e]);
            feed_expr(h, expr);
            h.update(&(args.len() as u64).to_le_bytes());
            for a in args {
                feed_expr(h, a);
            }
        }
        Expr::Lambda { params, body } => {
            h.update(&[0x2f]);
            feed_names(h, params);
            feed_expr(h, body);
        }
        Expr::FuncHandle(name) => {
            h.update(&[0x30]);
            feed_str(h, name);
        }
    }
}

fn feed_binop(h: &mut blake3::Hasher, op: BinOp) {
    // Explicit numeric tags so the variant order in `ast.rs` can shift
    // (insert/reorder enum variants) without silently breaking cached
    // hashes. Add new tags at the end; never recycle.
    let tag: u8 = match op {
        BinOp::Add => 0,
        BinOp::Sub => 1,
        BinOp::Mul => 2,
        BinOp::Div => 3,
        BinOp::Pow => 4,
        BinOp::ElemMul => 5,
        BinOp::ElemDiv => 6,
        BinOp::ElemPow => 7,
        BinOp::Eq => 8,
        BinOp::Ne => 9,
        BinOp::Lt => 10,
        BinOp::Le => 11,
        BinOp::Gt => 12,
        BinOp::Ge => 13,
        BinOp::And => 14,
        BinOp::Or => 15,
    };
    h.update(&[tag]);
}

fn feed_str(h: &mut blake3::Hasher, s: &str) {
    h.update(&(s.len() as u64).to_le_bytes());
    h.update(s.as_bytes());
}

fn feed_names(h: &mut blake3::Hasher, names: &[String]) {
    h.update(&(names.len() as u64).to_le_bytes());
    for n in names {
        feed_str(h, n);
    }
}

fn feed_bool(h: &mut blake3::Hasher, b: bool) {
    h.update(&[b as u8]);
}

// ── Canonical (rename-invariant) hash — Option 3 ────────────────────
//
// The original `hash_function_body` includes every identifier verbatim
// in the BLAKE3 stream. That's correct but conservative: cosmetic
// renames of params, locals, the function name itself, or sibling
// functions all bust the cache even though the algorithm is unchanged.
// And — more importantly — editing an inline-defined callee doesn't
// rotate any caller's hash, so the cache can silently return stale
// results.
//
// The canonical hash below fixes both. It:
//
// 1. **Replaces every binding identifier with a positional token.**
//    Parameters become `p0, p1, ...` in declaration order; return
//    vars become `r0, r1, ...`; locals become `lN` in first-occurrence
//    order. The function's own name is dropped entirely (it's
//    implicit at the call site). Renaming any of these doesn't change
//    the hash.
//
// 2. **Resolves name references through a scope stack** so lambdas can
//    capture outer-scope names by their canonical position rather
//    than by string. A lambda's `Var(name)` is fed as `(depth,
//    kind, index)` triple; depth=0 means innermost (lambda's own
//    scope), depth=1 means one out (enclosing function).
//
// 3. **Transitively hashes user-fn sibling calls**: when the body
//    calls another user-defined function, we recursively compute
//    that callee's canonical hash and feed its 32 bytes into the
//    caller's stream. Editing a callee's body therefore rotates
//    every caller's hash. Cycles are broken by the `visiting` set
//    and fall back to feeding the callee's name as bytes — mutual
//    recursion participants lose rename invariance for the names
//    involved in the cycle but everything else stays canonical.
//
// 4. **Self-recursion (direct)** is emitted as a `self` marker so
//    the hash doesn't need its own value to compute. Renaming a
//    self-recursive function preserves the hash.
//
// 5. **Builtin calls** keep their name verbatim. Builtin names are
//    external anchors (rustlab's API); changing them is a deliberate
//    rustlab-side change that should bust caches.
//
// 6. **Literals, operators, control-flow shape** are fed identically
//    to the original walker.
//
// The wire format is domain-separated from the original walker via
// [`CANONICAL_TAG`]; the two hash schemes can coexist without
// collision.

/// A snapshot of one user-defined function as required by the
/// canonical hasher. Cloned from `eval::UserFn` at the start of each
/// dispatcher pass so the closure-borrows the walker needs don't
/// conflict with mutable evaluator state.
#[derive(Clone)]
pub struct CanonicalFnSnapshot {
    pub params: Vec<String>,
    pub return_vars: Vec<String>,
    pub body: Vec<Stmt>,
}

/// Compute the canonical (rename-invariant, transitively-correct)
/// entry id for a user function. `user_fns` provides body snapshots
/// for any sibling call discovered during the walk; `is_builtin`
/// distinguishes builtin names from user-fn names at call sites.
pub fn canonical_entry_id(
    fn_name: &str,
    params: &[String],
    return_vars: &[String],
    body: &[Stmt],
    user_fns: &BTreeMap<String, CanonicalFnSnapshot>,
    is_builtin: &dyn Fn(&str) -> bool,
) -> [u8; 32] {
    let mut memo: BTreeMap<String, [u8; 32]> = BTreeMap::new();
    canonical_entry_id_inner(
        fn_name,
        params,
        return_vars,
        body,
        user_fns,
        is_builtin,
        &mut memo,
        &mut BTreeSet::new(),
    )
}

fn canonical_entry_id_inner(
    fn_name: &str,
    params: &[String],
    return_vars: &[String],
    body: &[Stmt],
    user_fns: &BTreeMap<String, CanonicalFnSnapshot>,
    is_builtin: &dyn Fn(&str) -> bool,
    memo: &mut BTreeMap<String, [u8; 32]>,
    visiting: &mut BTreeSet<String>,
) -> [u8; 32] {
    if let Some(&cached) = memo.get(fn_name) {
        return cached;
    }
    visiting.insert(fn_name.to_string());

    let mut h = blake3::Hasher::new();
    h.update(CANONICAL_TAG);
    // Hash structural counts so callers with different arities never
    // collide even if their bodies are isomorphic.
    h.update(&(params.len() as u32).to_le_bytes());
    h.update(&(return_vars.len() as u32).to_le_bytes());

    let mut stack = ScopeStack::default();
    stack.enter_function(params, return_vars);
    let mut ctx = CanonicalCtx {
        user_fns,
        is_builtin,
        self_name: fn_name,
        memo,
        visiting,
    };
    feed_stmts_canonical(&mut h, body, &mut stack, &mut ctx);
    stack.exit_function();

    let id = *h.finalize().as_bytes();
    ctx.memo.insert(fn_name.to_string(), id);
    ctx.visiting.remove(fn_name);
    id
}

/// One namespace frame in the scope stack. Function/lambda scopes
/// stack; nested control-flow blocks (if/for/while/switch) share the
/// enclosing function's frame.
#[derive(Default)]
struct Scope {
    params: BTreeMap<String, u32>,
    return_vars: BTreeMap<String, u32>,
    locals: BTreeMap<String, u32>,
    next_local: u32,
}

#[derive(Default)]
struct ScopeStack {
    frames: Vec<Scope>,
}

#[derive(Clone, Copy)]
enum NameKind {
    Param = 1,
    Return = 2,
    Local = 3,
}

impl ScopeStack {
    fn enter_function(&mut self, params: &[String], return_vars: &[String]) {
        let mut scope = Scope::default();
        for (i, p) in params.iter().enumerate() {
            scope.params.insert(p.clone(), i as u32);
        }
        for (i, r) in return_vars.iter().enumerate() {
            scope.return_vars.insert(r.clone(), i as u32);
        }
        self.frames.push(scope);
    }
    fn exit_function(&mut self) {
        self.frames.pop();
    }
    fn enter_lambda(&mut self, params: &[String]) {
        let mut scope = Scope::default();
        for (i, p) in params.iter().enumerate() {
            scope.params.insert(p.clone(), i as u32);
        }
        self.frames.push(scope);
    }
    fn exit_lambda(&mut self) {
        self.frames.pop();
    }
    /// Register an assigned name as a local in the innermost frame.
    /// First occurrence allocates a fresh `lN`; subsequent occurrences
    /// of the same name in the same scope return the same number.
    /// Names that already resolve as a param or return var in the
    /// innermost scope are NOT re-registered as locals — the existing
    /// number wins. (A function that writes to its own return var is
    /// still writing to `r0`, not creating a separate `l0`.)
    fn record_assignment(&mut self, name: &str) {
        let scope = self.frames.last_mut().expect("scope stack underflow");
        if scope.params.contains_key(name) || scope.return_vars.contains_key(name) {
            return;
        }
        if scope.locals.contains_key(name) {
            return;
        }
        let id = scope.next_local;
        scope.next_local += 1;
        scope.locals.insert(name.to_string(), id);
    }
    /// Resolve a name reference. Returns `(depth_from_inner, kind, index)`
    /// on success; `None` if the name doesn't resolve in any open
    /// scope. Depth 0 = innermost open frame; 1 = one out, etc.
    fn resolve(&self, name: &str) -> Option<(u32, NameKind, u32)> {
        for (depth_from_outer, scope) in self.frames.iter().enumerate().rev() {
            let depth = (self.frames.len() - 1 - depth_from_outer) as u32;
            if let Some(&i) = scope.params.get(name) {
                return Some((depth, NameKind::Param, i));
            }
            if let Some(&i) = scope.return_vars.get(name) {
                return Some((depth, NameKind::Return, i));
            }
            if let Some(&i) = scope.locals.get(name) {
                return Some((depth, NameKind::Local, i));
            }
        }
        None
    }
}

struct CanonicalCtx<'a> {
    user_fns: &'a BTreeMap<String, CanonicalFnSnapshot>,
    is_builtin: &'a dyn Fn(&str) -> bool,
    self_name: &'a str,
    memo: &'a mut BTreeMap<String, [u8; 32]>,
    visiting: &'a mut BTreeSet<String>,
}

/// Emit a canonical "name reference" — either a (depth, kind, index)
/// triple for a name bound in some open scope, or a length-prefixed
/// raw string for anything else (free vars, which should not occur in
/// a pure cached function, but we emit them defensively for tests).
fn feed_name_ref(h: &mut blake3::Hasher, stack: &ScopeStack, name: &str) {
    match stack.resolve(name) {
        Some((depth, kind, idx)) => {
            h.update(&[0xC0]); // bound-name tag
            h.update(&depth.to_le_bytes());
            h.update(&[kind as u8]);
            h.update(&idx.to_le_bytes());
        }
        None => {
            h.update(&[0xC1]); // free-name tag (defensive)
            feed_str(h, name);
        }
    }
}

/// Walk a body and pre-register every assignment target so that uses
/// preceding their (lexical-order) assignment still resolve. matlab
/// semantics: assignment in any branch of an if creates a function-
/// scope local; uses can reach it.
fn collect_locals_in_body(stmts: &[Stmt], stack: &mut ScopeStack) {
    for stmt in stmts {
        collect_locals_in_stmt(&stmt.kind, stack);
    }
}

fn collect_locals_in_stmt(kind: &StmtKind, stack: &mut ScopeStack) {
    match kind {
        StmtKind::Assign { name, .. }
        | StmtKind::IndexAssign { name, .. }
        | StmtKind::FieldAssign { object: name, .. } => {
            stack.record_assignment(name);
        }
        StmtKind::MultiAssign { names, .. } => {
            for n in names {
                stack.record_assignment(n);
            }
        }
        StmtKind::For { var, body, .. } => {
            stack.record_assignment(var);
            collect_locals_in_body(body, stack);
        }
        StmtKind::While { body, .. } => collect_locals_in_body(body, stack),
        StmtKind::If {
            then_body,
            elseif_arms,
            else_body,
            ..
        } => {
            collect_locals_in_body(then_body, stack);
            for (_, b) in elseif_arms {
                collect_locals_in_body(b, stack);
            }
            collect_locals_in_body(else_body, stack);
        }
        StmtKind::Switch {
            cases, otherwise, ..
        } => {
            for (_, b) in cases {
                collect_locals_in_body(b, stack);
            }
            collect_locals_in_body(otherwise, stack);
        }
        StmtKind::FunctionDef { name, .. } => {
            // A nested function definition installs `name` in the
            // enclosing scope's name table. The body is hashed
            // separately as its own canonical entry (see
            // feed_stmt_kind_canonical below).
            stack.record_assignment(name);
        }
        _ => {}
    }
}

fn feed_stmts_canonical(
    h: &mut blake3::Hasher,
    stmts: &[Stmt],
    stack: &mut ScopeStack,
    ctx: &mut CanonicalCtx<'_>,
) {
    // Two-pass: register every assignment in this block first, then
    // walk and feed. That way `disp(k)` followed by `k = 1` still
    // resolves consistently.
    collect_locals_in_body(stmts, stack);
    h.update(&(stmts.len() as u64).to_le_bytes());
    for stmt in stmts {
        feed_stmt_kind_canonical(h, &stmt.kind, stack, ctx);
    }
}

fn feed_stmt_kind_canonical(
    h: &mut blake3::Hasher,
    kind: &StmtKind,
    stack: &mut ScopeStack,
    ctx: &mut CanonicalCtx<'_>,
) {
    match kind {
        StmtKind::Assign { name, expr, suppress } => {
            h.update(&[0x01]);
            feed_name_ref(h, stack, name);
            feed_expr_canonical(h, expr, stack, ctx);
            feed_bool(h, *suppress);
        }
        StmtKind::Expr(e, suppress) => {
            h.update(&[0x02]);
            feed_expr_canonical(h, e, stack, ctx);
            feed_bool(h, *suppress);
        }
        StmtKind::FunctionDef {
            name: _,
            params,
            return_vars,
            body,
        } => {
            // Hash the nested function as a structural marker: its
            // arity shape and a fresh canonical id of its own body.
            // The id is computed in a sub-context so the nested fn's
            // scope doesn't leak into the outer hash.
            h.update(&[0x03]);
            h.update(&(params.len() as u32).to_le_bytes());
            h.update(&(return_vars.len() as u32).to_le_bytes());
            let mut sub_stack = ScopeStack::default();
            sub_stack.enter_function(params, return_vars);
            // Reuse the outer ctx (same memo, visiting, user_fns)
            // because nested fns share the global name space — but
            // we run the walk on the inner body only.
            feed_stmts_canonical(h, body, &mut sub_stack, ctx);
            sub_stack.exit_function();
        }
        StmtKind::FieldAssign { object, field, expr, suppress } => {
            h.update(&[0x04]);
            feed_name_ref(h, stack, object);
            feed_str(h, field); // field names ARE structural (struct schema)
            feed_expr_canonical(h, expr, stack, ctx);
            feed_bool(h, *suppress);
        }
        StmtKind::Return => {
            h.update(&[0x05]);
        }
        StmtKind::If {
            cond,
            then_body,
            elseif_arms,
            else_body,
        } => {
            h.update(&[0x06]);
            feed_expr_canonical(h, cond, stack, ctx);
            feed_stmts_canonical(h, then_body, stack, ctx);
            h.update(&(elseif_arms.len() as u64).to_le_bytes());
            for (c, b) in elseif_arms {
                feed_expr_canonical(h, c, stack, ctx);
                feed_stmts_canonical(h, b, stack, ctx);
            }
            feed_stmts_canonical(h, else_body, stack, ctx);
        }
        StmtKind::Switch { expr, cases, otherwise } => {
            h.update(&[0x07]);
            feed_expr_canonical(h, expr, stack, ctx);
            h.update(&(cases.len() as u64).to_le_bytes());
            for (c, b) in cases {
                feed_expr_canonical(h, c, stack, ctx);
                feed_stmts_canonical(h, b, stack, ctx);
            }
            feed_stmts_canonical(h, otherwise, stack, ctx);
        }
        StmtKind::Run { path } => {
            h.update(&[0x08]);
            feed_str(h, path);
        }
        StmtKind::Format { mode } => {
            h.update(&[0x09]);
            feed_str(h, mode);
        }
        StmtKind::Hold { on } => {
            h.update(&[0x0a]);
            feed_bool(h, *on);
        }
        StmtKind::Grid { on } => {
            h.update(&[0x0b]);
            feed_bool(h, *on);
        }
        StmtKind::Viewer { on, name } => {
            h.update(&[0x0c]);
            match on {
                None => {
                    h.update(&[0u8]);
                }
                Some(b) => {
                    h.update(&[1u8]);
                    feed_bool(h, *b);
                }
            }
            match name {
                None => {
                    h.update(&[0u8]);
                }
                Some(n) => {
                    h.update(&[1u8]);
                    feed_str(h, n);
                }
            }
        }
        StmtKind::MultiAssign { names, expr, suppress } => {
            h.update(&[0x0d]);
            h.update(&(names.len() as u64).to_le_bytes());
            for n in names {
                feed_name_ref(h, stack, n);
            }
            feed_expr_canonical(h, expr, stack, ctx);
            feed_bool(h, *suppress);
        }
        StmtKind::For { var, iter, body } => {
            h.update(&[0x0e]);
            feed_name_ref(h, stack, var);
            feed_expr_canonical(h, iter, stack, ctx);
            feed_stmts_canonical(h, body, stack, ctx);
        }
        StmtKind::While { cond, body } => {
            h.update(&[0x0f]);
            feed_expr_canonical(h, cond, stack, ctx);
            feed_stmts_canonical(h, body, stack, ctx);
        }
        StmtKind::IndexAssign { name, indices, expr, suppress } => {
            h.update(&[0x10]);
            feed_name_ref(h, stack, name);
            h.update(&(indices.len() as u64).to_le_bytes());
            for i in indices {
                feed_expr_canonical(h, i, stack, ctx);
            }
            feed_expr_canonical(h, expr, stack, ctx);
            feed_bool(h, *suppress);
        }
        StmtKind::Cache(c) => {
            h.update(&[0x11]);
            feed_cache_stmt(h, c);
        }
    }
}

fn feed_expr_canonical(
    h: &mut blake3::Hasher,
    e: &Expr,
    stack: &mut ScopeStack,
    ctx: &mut CanonicalCtx<'_>,
) {
    match e {
        Expr::Number(n) => {
            h.update(&[0x20]);
            h.update(&n.to_le_bytes());
        }
        Expr::Str(s) => {
            h.update(&[0x21]);
            feed_str(h, s);
        }
        Expr::Var(n) => {
            h.update(&[0x22]);
            feed_name_ref(h, stack, n);
        }
        Expr::BinOp { op, lhs, rhs } => {
            h.update(&[0x23]);
            feed_binop(h, *op);
            feed_expr_canonical(h, lhs, stack, ctx);
            feed_expr_canonical(h, rhs, stack, ctx);
        }
        Expr::UnaryMinus(e) => {
            h.update(&[0x24]);
            feed_expr_canonical(h, e, stack, ctx);
        }
        Expr::UnaryNot(e) => {
            h.update(&[0x25]);
            feed_expr_canonical(h, e, stack, ctx);
        }
        Expr::Call { name, args } => {
            feed_callable(h, name, stack, ctx);
            h.update(&(args.len() as u64).to_le_bytes());
            for a in args {
                feed_expr_canonical(h, a, stack, ctx);
            }
        }
        Expr::Matrix(rows) => {
            h.update(&[0x27]);
            h.update(&(rows.len() as u64).to_le_bytes());
            for row in rows {
                h.update(&(row.len() as u64).to_le_bytes());
                for cell in row {
                    feed_expr_canonical(h, cell, stack, ctx);
                }
            }
        }
        Expr::CellArray(items) => {
            h.update(&[0x28]);
            h.update(&(items.len() as u64).to_le_bytes());
            for x in items {
                feed_expr_canonical(h, x, stack, ctx);
            }
        }
        Expr::Range { start, step, stop } => {
            h.update(&[0x29]);
            feed_expr_canonical(h, start, stack, ctx);
            match step {
                None => {
                    h.update(&[0u8]);
                }
                Some(s) => {
                    h.update(&[1u8]);
                    feed_expr_canonical(h, s, stack, ctx);
                }
            }
            feed_expr_canonical(h, stop, stack, ctx);
        }
        Expr::Transpose(e) => {
            h.update(&[0x2a]);
            feed_expr_canonical(h, e, stack, ctx);
        }
        Expr::NonConjTranspose(e) => {
            h.update(&[0x2b]);
            feed_expr_canonical(h, e, stack, ctx);
        }
        Expr::All => {
            h.update(&[0x2c]);
        }
        Expr::Field { object, field } => {
            h.update(&[0x2d]);
            feed_expr_canonical(h, object, stack, ctx);
            feed_str(h, field);
        }
        Expr::Index { expr, args } => {
            h.update(&[0x2e]);
            feed_expr_canonical(h, expr, stack, ctx);
            h.update(&(args.len() as u64).to_le_bytes());
            for a in args {
                feed_expr_canonical(h, a, stack, ctx);
            }
        }
        Expr::Lambda { params, body } => {
            h.update(&[0x2f]);
            h.update(&(params.len() as u32).to_le_bytes());
            stack.enter_lambda(params);
            feed_expr_canonical(h, body, stack, ctx);
            stack.exit_lambda();
        }
        Expr::FuncHandle(name) => {
            feed_callable(h, name, stack, ctx);
        }
    }
}

/// Feed a name appearing in callable position (`Call` or `FuncHandle`).
/// The substitution rules match the proposal:
///
/// - **self-recursion** (name == self_name) → fixed `0xA0` tag.
/// - **sibling user fn** (in user_fns, not currently being visited) →
///   tag `0xA1` + the callee's 32-byte canonical id, computed
///   recursively with the shared memo.
/// - **cycle** (in user_fns AND currently being visited) → tag `0xA2`
///   + the callee's name length-prefixed. The cycle participants
///   lose rename invariance for the names involved; everything else
///   stays canonical.
/// - **builtin** (`is_builtin` returns true) → tag `0xA3` +
///   length-prefixed name. Builtin names are external rustlab API
///   anchors.
/// - **unresolved** (neither user fn nor builtin) → tag `0xA4` +
///   length-prefixed name. Happens when a callee hasn't been
///   defined yet at hash time; the rescan-on-FunctionDef hook
///   (Phase 6c-era) re-runs the gate so this resolves later.
fn feed_callable(
    h: &mut blake3::Hasher,
    name: &str,
    _stack: &ScopeStack,
    ctx: &mut CanonicalCtx<'_>,
) {
    if name == ctx.self_name {
        h.update(&[0xA0]);
        return;
    }
    if let Some(snapshot) = ctx.user_fns.get(name) {
        if ctx.visiting.contains(name) {
            // Cycle — fall back to feeding the name as bytes so the
            // recursion terminates. Mutual recursion participants
            // therefore retain the name-bust property for the names
            // forming the cycle.
            h.update(&[0xA2]);
            feed_str(h, name);
            return;
        }
        // Take ownership of the visiting set briefly so the recursive
        // call can mutate it without overlapping borrows.
        let callee_params = snapshot.params.clone();
        let callee_returns = snapshot.return_vars.clone();
        let callee_body = snapshot.body.clone();
        let id = canonical_entry_id_inner(
            name,
            &callee_params,
            &callee_returns,
            &callee_body,
            ctx.user_fns,
            ctx.is_builtin,
            ctx.memo,
            ctx.visiting,
        );
        h.update(&[0xA1]);
        h.update(&id);
        return;
    }
    if (ctx.is_builtin)(name) {
        h.update(&[0xA3]);
        feed_str(h, name);
        return;
    }
    h.update(&[0xA4]);
    feed_str(h, name);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn parse_src(src: &str) -> Vec<Stmt> {
        let tokens = tokenize(src).expect("tokenize");
        parse(tokens).expect("parse")
    }

    fn fh(src: &str) -> [u8; 32] {
        hash_stmts(&parse_src(src))
    }

    #[test]
    fn identical_source_same_hash() {
        let src = "function y = f(x)\n  y = x + 1\nend\n";
        assert_eq!(fh(src), fh(src));
    }

    #[test]
    fn whitespace_only_change_same_hash() {
        let a = "function y = f(x)\n  y = x + 1\nend\n";
        let b = "function y = f(x)\n\n\n    y = x + 1\nend\n";
        assert_eq!(fh(a), fh(b), "whitespace must not affect the hash");
    }

    #[test]
    fn moving_function_down_doesnt_change_inline_hash() {
        // For inline-fn hashing the absolute Stmt.line should be
        // irrelevant. Build the FunctionDef twice from sources that
        // place it at different line numbers and assert equality.
        let early = "function y = f(x)\n  y = x + 1\nend\n";
        let late = "\n\n\n\nfunction y = f(x)\n  y = x + 1\nend\n";
        let s1 = parse_src(early);
        let s2 = parse_src(late);
        let f1 = extract_fn(&s1, "f");
        let f2 = extract_fn(&s2, "f");
        let h1 = hash_function_body(&f1.0, &f1.1, &f1.2, f1.3);
        let h2 = hash_function_body(&f2.0, &f2.1, &f2.2, f2.3);
        assert_eq!(h1, h2);
    }

    #[test]
    fn identifier_rename_busts_hash() {
        // Renaming the bound parameter changes the hash. This is by
        // design: a function that captures the renamed name from an
        // outer scope would be semantically different.
        let a = "function y = f(x)\n  y = x + 1\nend\n";
        let b = "function y = f(z)\n  y = z + 1\nend\n";
        assert_ne!(fh(a), fh(b));
    }

    #[test]
    fn function_name_change_busts_hash() {
        let a = "function y = f(x)\n  y = x + 1\nend\n";
        let b = "function y = g(x)\n  y = x + 1\nend\n";
        assert_ne!(fh(a), fh(b));
    }

    #[test]
    fn literal_value_change_busts_hash() {
        let a = "function y = f(x)\n  y = x + 1\nend\n";
        let b = "function y = f(x)\n  y = x + 2\nend\n";
        assert_ne!(fh(a), fh(b));
    }

    #[test]
    fn operator_change_busts_hash() {
        let a = "function y = f(x)\n  y = x + 1\nend\n";
        let b = "function y = f(x)\n  y = x - 1\nend\n";
        assert_ne!(fh(a), fh(b));
    }

    #[test]
    fn elementwise_vs_matrix_op_distinct() {
        let a = "function y = f(x)\n  y = x * x\nend\n";
        let b = "function y = f(x)\n  y = x .* x\nend\n";
        assert_ne!(fh(a), fh(b));
    }

    #[test]
    fn statement_reorder_busts_hash() {
        let a = "a = 1\nb = 2\n";
        let b = "b = 2\na = 1\n";
        assert_ne!(fh(a), fh(b));
    }

    #[test]
    fn suppress_semicolon_affects_hash() {
        // `a = 1` vs `a = 1;` differ in display behaviour. Treat as
        // structurally distinct.
        let a = "a = 1\n";
        let b = "a = 1;\n";
        assert_ne!(fh(a), fh(b));
    }

    #[test]
    fn function_entry_id_depends_on_both_inputs() {
        let file_a = [1u8; 32];
        let file_b = [2u8; 32];
        let id_a_f = function_entry_id(&file_a, "f");
        let id_a_g = function_entry_id(&file_a, "g");
        let id_b_f = function_entry_id(&file_b, "f");
        assert_ne!(id_a_f, id_a_g, "fn name must affect entry_id");
        assert_ne!(id_a_f, id_b_f, "file hash must affect entry_id");
        // Stability — same inputs → same id.
        assert_eq!(id_a_f, function_entry_id(&file_a, "f"));
    }

    #[test]
    fn variant_distinction_via_tag() {
        // `a = 1` (Assign) vs `1` (Expr) — both contain a single
        // numeric literal. The Stmt-kind tag should disambiguate.
        let a = "a = 1;\n";
        let b = "1;\n";
        assert_ne!(fh(a), fh(b));
    }

    #[test]
    fn length_prefix_blocks_concat_collision() {
        // Two assignments to names that, if concatenated, collide:
        // `a = 1; bc = 2` vs `ab = 1; c = 2`. Length prefixes on
        // identifiers force these to hash differently.
        let a = "a = 1;\nbc = 2;\n";
        let b = "ab = 1;\nc = 2;\n";
        assert_ne!(fh(a), fh(b));
    }

    #[test]
    fn nan_literal_in_source_is_stable() {
        // Even though runtime NaN bypasses the cache (per
        // rustlab_core::Fingerprint), a `nan` literal in source code
        // is structural and hashes deterministically.
        let a = "function y = f(x)\n  y = nan\nend\n";
        let b = "function y = f(x)\n  y = nan\nend\n";
        assert_eq!(fh(a), fh(b));
    }

    /// Pull a FunctionDef from a list of statements by name and return
    /// (name, params, return_vars, body) borrowed from the AST.
    fn extract_fn(
        stmts: &[Stmt],
        target: &str,
    ) -> (String, Vec<String>, Vec<String>, &'static [Stmt]) {
        for stmt in stmts {
            if let StmtKind::FunctionDef {
                name,
                params,
                return_vars,
                body,
            } = &stmt.kind
            {
                if name == target {
                    // SAFETY for the test helper: extend the lifetime
                    // to 'static via leak. The test process tears down
                    // immediately after; we trade a small allocation
                    // leak for ergonomic borrowing.
                    let body_static: &'static [Stmt] = Box::leak(body.clone().into_boxed_slice());
                    return (
                        name.clone(),
                        params.clone(),
                        return_vars.clone(),
                        body_static,
                    );
                }
            }
        }
        panic!("function `{target}` not found in statements");
    }
}

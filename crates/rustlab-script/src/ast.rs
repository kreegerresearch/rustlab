#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Stmt {
    pub kind: StmtKind,
    pub line: usize,
}

impl Stmt {
    pub fn new(kind: StmtKind, line: usize) -> Self {
        Self { kind, line }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StmtKind {
    /// `name = expr` — suppress=true when line ends with `;`
    Assign {
        name: String,
        expr: Expr,
        suppress: bool,
    },
    /// bare expression — suppress=true when line ends with `;`
    Expr(Expr, bool),
    /// `function [retvar =] name(params) ... end` or
    /// `function [a, b, ...] = name(params) ... end` (matlab multi-output).
    /// `return_vars` is empty for a no-return body, length 1 for the
    /// classic single-output form, length >= 2 for multi-output.
    FunctionDef {
        name: String,
        params: Vec<String>,
        return_vars: Vec<String>,
        body: Vec<Stmt>,
    },
    /// `object.field = expr` — struct field assignment
    FieldAssign {
        object: String,
        field: String,
        expr: Expr,
        suppress: bool,
    },
    /// `return` statement inside a function body
    Return,
    /// `if cond \n then_body [elseif cond \n body]* [else \n else_body] end`
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        elseif_arms: Vec<(Expr, Vec<Stmt>)>,
        else_body: Vec<Stmt>,
    },
    /// `switch expr \n case val \n body ... [otherwise \n body] end`
    Switch {
        expr: Expr,
        cases: Vec<(Expr, Vec<Stmt>)>,
        otherwise: Vec<Stmt>,
    },
    /// `run path` — execute another .rlab script and merge its definitions
    Run { path: String },
    /// `format commas` / `format default` — change display mode
    Format { mode: String },
    /// `hold on` / `hold off` — toggle hold mode
    Hold { on: bool },
    /// `grid on` / `grid off` — toggle grid on current subplot
    Grid { on: bool },
    /// `viewer on` / `viewer on <name>` / `viewer off` — connect/disconnect external viewer.
    /// Bare `viewer` (no on/off) is a status query (`on = None`).
    Viewer {
        on: Option<bool>,
        name: Option<String>,
    },
    /// `[a, b, c] = expr` — multi-value assignment (unpacks a Tuple)
    MultiAssign {
        names: Vec<String>,
        expr: Expr,
        suppress: bool,
    },
    /// `for VAR = iter_expr ... end` — iterate over elements of a vector
    For {
        var: String,
        iter: Expr,
        body: Vec<Stmt>,
    },
    /// `while cond ... end` — repeat body while cond is truthy
    While { cond: Expr, body: Vec<Stmt> },
    /// `name(i) = expr` or `name(i,j) = expr` — indexed assignment
    IndexAssign {
        name: String,
        indices: Vec<Expr>,
        expr: Expr,
        suppress: bool,
    },
    /// `cache <subcommand>` — persistent function-result cache directive.
    /// See [`CacheStmt`] for the sub-forms.
    Cache(CacheStmt),
}

/// Sub-forms of the `cache` statement (see Phase 3 of
/// `dev/plans/persistent_function_cache.md`).
///
/// Semantics live with the evaluator; this enum is purely the parsed
/// shape of the user's directive.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CacheStmt {
    /// `cache enable [path]`
    ///
    /// With no `path`, opens or creates the per-project default store at
    /// `.rustlab/cache.db`. With a `path`, opens or creates that file.
    Enable { path: Option<String> },
    /// `cache off` — close the active store. Subsequent calls fall back
    /// to direct execution; in-DB entries are preserved.
    Off,
    /// `cache add file <path>` — source a `.rlab` file and register
    /// every top-level function it defines as cacheable. Free-variable
    /// + impurity checks fire here.
    AddFile { path: String },
    /// `cache add function <name>[, <name>, ...]` — register one or
    /// more already-defined functions for caching. Loud error if any
    /// fails the purity contract.
    AddFunctions { names: Vec<String> },
    /// `cache remove function <name>` — drop a function from scope.
    /// DB rows are kept; only the in-process dispatch routing changes.
    RemoveFunction { name: String },
    /// `cache status` — print this process's active store, scope, and
    /// counters.
    Status,
    /// `cache clear` — wipe every entry in the active store. DB file
    /// is preserved.
    Clear,
    /// `cache prune [older=DUR] [max_size=BYTES]` — drop old or
    /// oversized entries. With no kwargs, defaults to `older=30d`.
    Prune {
        /// Duration string like `"30d"`, `"12h"`, `"500ms"`. Parsed
        /// at exec time so the AST stays format-agnostic.
        older: Option<String>,
        max_size_bytes: Option<u64>,
    },
    /// `cache list [limit=N]` — print the active store's entries
    /// (short-hex keys + sizes + timestamps). Never prints cached
    /// values. Mirrors `rustlab cache list` so users in the REPL
    /// don't have to drop to a shell to inspect what's been stored.
    List { limit: Option<u64> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Expr {
    Number(f64),
    Str(String),
    Var(String),
    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    UnaryMinus(Box<Expr>),
    UnaryNot(Box<Expr>),
    /// `name(args)` — at eval time, if `name` is a vector/matrix in env, treated as indexing
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// `[rows]` literal — rows separated by `;`, elements by `,`
    Matrix(Vec<Vec<Expr>>),
    /// `{expr, expr, ...}` — cell/string array literal
    CellArray(Vec<Expr>),
    /// `start:stop` or `start:step:stop` — produces a vector
    Range {
        start: Box<Expr>,
        step: Option<Box<Expr>>,
        stop: Box<Expr>,
    },
    /// `expr'` — conjugate transpose
    Transpose(Box<Expr>),
    /// `expr.'` — non-conjugate (plain) transpose
    NonConjTranspose(Box<Expr>),
    /// `:` used as an index meaning "all elements in this dimension"
    All,
    /// `expr.field` — struct field access
    Field {
        object: Box<Expr>,
        field: String,
    },
    /// `expr(args)` — index or call on the result of an arbitrary expression
    /// Used for chained indexing: `f(a, b)(i)` → `Index { expr: Call{f,[a,b]}, args: [i] }`
    Index {
        expr: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `@(params) body` — anonymous function (lambda); captures env at creation time
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
    },
    /// `@name` — handle to a named function (user-defined or builtin)
    FuncHandle(String),
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    /// Element-wise: .*  ./  .^
    ElemMul,
    ElemDiv,
    ElemPow,
    /// Comparison operators
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Logical operators
    And,
    Or,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub line: usize,
}

impl Stmt {
    pub fn new(kind: StmtKind, line: usize) -> Self {
        Self { kind, line }
    }
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    /// `name = expr` — suppress=true when line ends with `;`
    Assign {
        name: String,
        expr: Expr,
        suppress: bool,
    },
    /// bare expression — suppress=true when line ends with `;`
    Expr(Expr, bool),
    /// `function [retvar =] name(params) ... end`
    FunctionDef {
        name: String,
        params: Vec<String>,
        return_var: Option<String>,
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
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Copy, PartialEq)]
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

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
    Not,
}

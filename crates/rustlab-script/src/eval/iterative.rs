//! `pcg` and `ichol`, MATLAB's iterative SPD solver and its incomplete
//! Cholesky preconditioner (dev/plans/sparse_spd_solve.md, Phase 3).
//!
//! `pcg` lives on the evaluator because `A`, `M1`, and `M2` may be
//! function handles. The iteration itself is
//! `rustlab_core::sparse_solve::pcg`; this file parses MATLAB's argument
//! forms, builds the operators, and prints MATLAB's messages.

use super::builtins::{fmt_general, solution_column};
use super::output;
use super::sparse_auto::{self, AutoFactor};
use super::value::Value;
use super::Evaluator;
use crate::error::ScriptError;
use ndarray::Array1;
use num_complex::Complex;
use rustlab_core::sparse_solve::{
    ichol, pcg, FromComplex, IcholOptions, IcholType, PcgOperators, PcgOutcome, SparseCsc,
    SparseScalar, TriangularSolve,
};
use rustlab_core::{CVector, SparseMat, C64};

/// The two scalar types the sparse code runs on.
pub(crate) trait PcgScalar: FromComplex {
    fn to_c64(self) -> C64;
}

impl PcgScalar for f64 {
    fn to_c64(self) -> C64 {
        Complex::new(self, 0.0)
    }
}

impl PcgScalar for C64 {
    fn to_c64(self) -> C64 {
        self
    }
}

fn err(msg: impl Into<String>) -> ScriptError {
    ScriptError::type_err(msg.into())
}

fn is_callable(v: &Value) -> bool {
    matches!(v, Value::Lambda { .. } | Value::FuncHandle(_))
}

/// MATLAB's `[]` placeholder: any value with no elements.
fn is_empty_arg(v: &Value) -> bool {
    match v {
        Value::Vector(x) => x.is_empty(),
        Value::Matrix(m) => m.is_empty(),
        Value::SparseMatrix(s) => s.rows == 0 || s.cols == 0,
        _ => false,
    }
}

/// A numeric matrix argument as a sparse matrix. `None` for anything
/// that is not a matrix.
fn as_sparse(v: &Value) -> Option<SparseMat> {
    match v {
        Value::SparseMatrix(s) => Some(s.clone()),
        Value::Matrix(m) => Some(SparseMat::from_dense(m)),
        Value::Scalar(x) => Some(SparseMat::new(1, 1, vec![(0, 0, Complex::new(*x, 0.0))])),
        Value::Complex(c) => Some(SparseMat::new(1, 1, vec![(0, 0, *c)])),
        Value::Vector(x) if x.len() == 1 => Some(SparseMat::new(1, 1, vec![(0, 0, x[0])])),
        _ => None,
    }
}

/// A right-hand side or initial guess: a column (or a 1-D vector).
fn as_column(name: &str, v: &Value) -> Result<CVector, ScriptError> {
    if let Value::Matrix(m) = v {
        if m.nrows() == 1 && m.ncols() > 1 {
            return Err(err(format!(
                "pcg: {name} must be a column vector, got 1×{}",
                m.ncols()
            )));
        }
        if m.ncols() > 1 {
            return Err(err(format!(
                "pcg: {name} must be a column vector, got {}×{}",
                m.nrows(),
                m.ncols()
            )));
        }
    }
    v.to_cvector().map_err(|e| err(format!("pcg: {name}: {e}")))
}

fn all_real(v: &CVector) -> bool {
    v.iter().all(|c| c.im == 0.0)
}

fn sparse_real(s: &SparseMat) -> bool {
    s.entries.iter().all(|&(_, _, v)| v.im == 0.0)
}

/// `%0.2g`, as MATLAB's messages print residuals and tolerances.
fn g2(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Inf" } else { "-Inf" }.to_string();
    }
    let body = fmt_general(x.abs(), 2, false, false);
    if x < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

enum AOp<T: SparseScalar> {
    Matrix(SparseCsc<T>),
    Handle(Value),
}

enum MOp<T: SparseScalar> {
    Triangular(TriangularSolve<T>),
    Factor(Box<AutoFactor>),
    /// The matrix could not be factored: `M \ r` is not finite (flag 2).
    Singular,
    Handle(Value),
}

/// A preconditioner argument before the scalar type is chosen.
enum MArg {
    Matrix(SparseMat),
    Handle(Value),
}

struct ScriptOps<'e, T: SparseScalar> {
    ev: &'e mut Evaluator,
    n: usize,
    a: AOp<T>,
    m1: Option<MOp<T>>,
    m2: Option<MOp<T>>,
}

impl<T: PcgScalar> ScriptOps<'_, T> {
    fn call(&mut self, f: &Value, x: &[T], what: &str) -> Result<Vec<T>, ScriptError> {
        let arg = solution_column(Array1::from_iter(x.iter().map(|&v| v.to_c64())));
        let out = self.ev.call_callable(f.clone(), vec![arg])?;
        let y = out
            .to_cvector()
            .map_err(|e| err(format!("pcg: {what} must return a vector: {e}")))?;
        if y.len() != self.n {
            return Err(err(format!(
                "pcg: {what} returned length {}, expected length {}",
                y.len(),
                self.n
            )));
        }
        y.iter()
            .map(|&c| T::from_c64(c, f64::INFINITY))
            .collect::<Result<_, _>>()
            .map_err(|e| err(format!("pcg: {e}")))
    }

    fn precondition(&mut self, which: u8, r: &[T]) -> Result<Vec<T>, ScriptError> {
        let m = if which == 1 { &self.m1 } else { &self.m2 };
        let (what, handle) = match m {
            None => return Ok(r.to_vec()),
            Some(MOp::Triangular(t)) => return Ok(t.solve(r)),
            Some(MOp::Singular) => return Ok(vec![T::from_f64(f64::NAN); r.len()]),
            Some(MOp::Factor(f)) => {
                let b = Array1::from_iter(r.iter().map(|&v| v.to_c64()));
                return match f.solve(&b) {
                    Ok(x) => x
                        .iter()
                        .map(|&c| T::from_c64(c, f64::INFINITY))
                        .collect::<Result<_, _>>()
                        .map_err(|e| err(format!("pcg: {e}"))),
                    Err(_) => Ok(vec![T::from_f64(f64::NAN); r.len()]),
                };
            }
            Some(MOp::Handle(f)) => (if which == 1 { "M1" } else { "M2" }, f.clone()),
        };
        self.call(&handle, r, what)
    }
}

impl<T: PcgScalar> PcgOperators<T> for ScriptOps<'_, T> {
    type Error = ScriptError;

    fn apply_a(&mut self, x: &[T]) -> Result<Vec<T>, ScriptError> {
        self.ev.check_cancelled()?;
        match &self.a {
            AOp::Matrix(a) => Ok(a.spmv(x)),
            AOp::Handle(f) => {
                let f = f.clone();
                self.call(&f, x, "afun")
            }
        }
    }
    fn has_m1(&self) -> bool {
        self.m1.is_some()
    }
    fn has_m2(&self) -> bool {
        self.m2.is_some()
    }
    fn solve_m1(&mut self, r: &[T]) -> Result<Vec<T>, ScriptError> {
        self.precondition(1, r)
    }
    fn solve_m2(&mut self, y: &[T]) -> Result<Vec<T>, ScriptError> {
        self.precondition(2, y)
    }
}

fn to_csc<T: PcgScalar>(s: &SparseMat) -> Result<SparseCsc<T>, ScriptError> {
    s.to_csc().map_err(|e| err(format!("pcg: {e}")))
}

fn build_m<T: PcgScalar>(m: MArg) -> Result<MOp<T>, ScriptError> {
    match m {
        MArg::Handle(f) => Ok(MOp::Handle(f)),
        MArg::Matrix(s) => {
            let csc: SparseCsc<T> = to_csc(&s)?;
            if let Some(t) = TriangularSolve::classify(&csc) {
                return Ok(MOp::Triangular(t));
            }
            Ok(
                match sparse_auto::factor(
                    &s,
                    sparse_auto::Mode::Auto,
                    super::builtins::OrderingChoice::Auto,
                ) {
                    Ok(f) => MOp::Factor(Box::new(f)),
                    Err(_) => MOp::Singular,
                },
            )
        }
    }
}

struct PcgArgs {
    a: Result<SparseMat, Value>,
    b: CVector,
    tol: f64,
    maxit: usize,
    m1: Option<MArg>,
    m2: Option<MArg>,
    x0: Option<CVector>,
    warned: bool,
}

fn parse_m(name: &str, v: Option<&Value>, n: usize) -> Result<Option<MArg>, ScriptError> {
    let Some(v) = v else { return Ok(None) };
    if is_empty_arg(v) {
        return Ok(None);
    }
    if is_callable(v) {
        return Ok(Some(MArg::Handle(v.clone())));
    }
    let s = as_sparse(v).ok_or_else(|| {
        err(format!(
            "pcg: {name} must be a matrix, a function handle, or [], got {}",
            v.type_name()
        ))
    })?;
    if s.rows != n || s.cols != n {
        return Err(err(format!(
            "pcg: preconditioner {name} must be {n}×{n}, got {}×{}",
            s.rows, s.cols
        )));
    }
    Ok(Some(MArg::Matrix(s)))
}

fn parse_args(args: Vec<Value>) -> Result<PcgArgs, ScriptError> {
    if args.len() < 2 || args.len() > 7 {
        return Err(err(format!(
            "pcg: expected 2 to 7 arguments (A, b, tol, maxit, M1, M2, x0), got {}",
            args.len()
        )));
    }
    let b = as_column("b", &args[1])?;
    let n = b.len();
    if n == 0 {
        return Err(err("pcg: b must not be empty"));
    }
    let a = if is_callable(&args[0]) {
        Err(args[0].clone())
    } else {
        let s = as_sparse(&args[0]).ok_or_else(|| {
            err(format!(
                "pcg: A must be a square matrix or a function handle, got {}",
                args[0].type_name()
            ))
        })?;
        if s.rows != s.cols {
            return Err(err(format!(
                "pcg: A must be square, got {}×{}",
                s.rows, s.cols
            )));
        }
        if s.rows != n {
            return Err(err(format!(
                "pcg: A is {0}×{0} but b has length {n}",
                s.rows
            )));
        }
        Ok(s)
    };

    let mut warned = false;
    let tol = match args.get(2).filter(|v| !is_empty_arg(v)) {
        None => 1e-6,
        Some(v) => {
            let t = v
                .to_scalar()
                .map_err(|_| err("pcg: tol must be a real scalar"))?;
            if t.is_nan() {
                return Err(err("pcg: tol must be a real scalar, got NaN"));
            }
            if t <= f64::EPSILON {
                eprintln!(
                    "warning: pcg: input tol is smaller than eps and may not be achievable by pcg; \
                     using eps. Try a bigger tolerance."
                );
                warned = true;
                f64::EPSILON
            } else if t >= 1.0 {
                eprintln!(
                    "warning: pcg: input tol is bigger than 1; using 1 - eps. \
                     Try a smaller tolerance."
                );
                warned = true;
                1.0 - f64::EPSILON
            } else {
                t
            }
        }
    };
    let maxit = match args.get(3).filter(|v| !is_empty_arg(v)) {
        None => n.min(20),
        Some(v) => {
            let m = v
                .to_scalar()
                .map_err(|_| err("pcg: maxit must be a real scalar"))?;
            if m.is_nan() {
                return Err(err("pcg: maxit must be a real scalar, got NaN"));
            }
            if m <= 0.0 {
                0
            } else if m >= usize::MAX as f64 {
                usize::MAX
            } else {
                m.floor() as usize
            }
        }
    };
    let m1 = parse_m("M1", args.get(4), n)?;
    let m2 = parse_m("M2", args.get(5), n)?;
    let x0 = match args.get(6).filter(|v| !is_empty_arg(v)) {
        None => None,
        Some(v) => {
            let x = as_column("x0", v)?;
            if x.len() != n {
                return Err(err(format!(
                    "pcg: x0 must have length {n}, got {}",
                    x.len()
                )));
            }
            Some(x)
        }
    };
    Ok(PcgArgs {
        a,
        b,
        tol,
        maxit,
        m1,
        m2,
        x0,
        warned,
    })
}

fn run_pcg<T: PcgScalar>(ev: &mut Evaluator, p: PcgArgs) -> Result<PcgOutcome<C64>, ScriptError> {
    let n = p.b.len();
    let a = match p.a {
        Ok(s) => AOp::Matrix(to_csc::<T>(&s)?),
        Err(f) => AOp::Handle(f),
    };
    let m1 = p.m1.map(build_m::<T>).transpose()?;
    let m2 = p.m2.map(build_m::<T>).transpose()?;
    let conv = |v: &CVector| -> Result<Vec<T>, ScriptError> {
        v.iter()
            .map(|&c| T::from_c64(c, f64::INFINITY))
            .collect::<Result<_, _>>()
            .map_err(|e| err(format!("pcg: {e}")))
    };
    let b = conv(&p.b)?;
    let x0 = p.x0.as_ref().map(conv).transpose()?;
    let mut ops = ScriptOps { ev, n, a, m1, m2 };
    let out = pcg(&mut ops, &b, x0.as_deref(), p.tol, p.maxit)?;
    Ok(PcgOutcome {
        x: out.x.into_iter().map(PcgScalar::to_c64).collect(),
        flag: out.flag,
        relres: out.relres,
        iter: out.iter,
        stopped_at: out.stopped_at,
        resvec: out.resvec,
        zero_rhs: out.zero_rhs,
        tol_unreachable: out.tol_unreachable,
    })
}

/// MATLAB's `itermsg` for `pcg`.
fn report(out: &PcgOutcome<C64>, tol: f64) {
    if out.flag == 0 {
        let msg = if out.zero_rhs {
            "The right hand side vector is all zero so pcg\n\
             returned an all zero solution without iterating."
                .to_string()
        } else if out.iter == 0 {
            format!(
                "The initial guess has relative residual {} which is within\n\
                 the desired tolerance {} so pcg returned it without iterating.",
                g2(out.relres),
                g2(tol)
            )
        } else {
            format!(
                "pcg converged at iteration {} to a solution with relative residual {}.",
                out.iter,
                g2(out.relres)
            )
        };
        output::script_println(&msg);
        return;
    }
    let reason = match out.flag {
        1 => "the maximum number of iterations was reached",
        2 => "the system involving the preconditioner was ill conditioned",
        3 => "the method stagnated",
        _ => "a scalar quantity became too small or too large to continue computing",
    };
    eprintln!(
        "warning: pcg stopped at iteration {} without converging to the desired tolerance {}\n\
         because {reason}.\n\
         The iterate returned (number {}) has relative residual {}.",
        out.stopped_at,
        g2(tol),
        out.iter,
        g2(out.relres)
    );
}

impl Evaluator {
    /// `x = pcg(A, b, tol, maxit, M1, M2, x0)` and
    /// `[x, flag, relres, iter, resvec] = pcg(...)`.
    ///
    /// `A` is a square matrix (sparse or dense) or a handle `afun(x) = A*x`;
    /// `M1`, `M2` are matrices (`M = M1*M2`, applied as `M2 \ (M1 \ r)`) or
    /// handles `mfun(r) = M \ r`. `[]` takes the default (`tol = 1e-6`,
    /// `maxit = min(n, 20)`, no preconditioner, `x0 = 0`). A triangular or
    /// diagonal matrix `M` is solved by substitution; any other `M` is
    /// factored once (sparse `\`'s choice of Cholesky or LU). Real `A`, `b`,
    /// `x0`, and matrix `M` run in real arithmetic; a complex value or any
    /// function handle runs in complex arithmetic and returns a real `x`
    /// when every imaginary part is zero.
    pub(crate) fn eval_pcg(
        &mut self,
        args: Vec<Value>,
        nargout: usize,
    ) -> Result<Value, ScriptError> {
        let p = parse_args(args)?;
        let tol = p.tol;
        let warned = p.warned;
        let real = all_real(&p.b)
            && p.x0.as_ref().is_none_or(all_real)
            && p.a.as_ref().is_ok_and(sparse_real)
            && [&p.m1, &p.m2].iter().all(|m| match m {
                None => true,
                Some(MArg::Matrix(s)) => sparse_real(s),
                Some(MArg::Handle(_)) => false,
            });
        let out = if real {
            run_pcg::<f64>(self, p)?
        } else {
            run_pcg::<C64>(self, p)?
        };
        if out.tol_unreachable && !warned {
            eprintln!(
                "warning: pcg: input tol may not be achievable by pcg; try a bigger tolerance"
            );
        }
        if nargout < 2 {
            report(&out, tol);
        }
        let x = solution_column(Array1::from_vec(out.x));
        if nargout < 2 {
            return Ok(x);
        }
        let resvec = if out.resvec.len() == 1 {
            Value::Scalar(out.resvec[0])
        } else {
            let k = out.resvec.len();
            Value::real_matrix(ndarray::Array2::from_shape_vec((k, 1), out.resvec).expect("k×1"))
        };
        Ok(Value::Tuple(vec![
            x,
            Value::Scalar(f64::from(out.flag)),
            Value::Scalar(out.relres),
            Value::Scalar(out.iter as f64),
            resvec,
        ]))
    }
}

// ─── ichol ────────────────────────────────────────────────────────────────

fn opt_str(name: &str, v: &Value) -> Result<String, ScriptError> {
    v.to_str()
        .map(|s| s.to_ascii_lowercase())
        .map_err(|_| err(format!("ichol: opts.{name} must be a string")))
}

fn opt_nonneg(name: &str, v: &Value) -> Result<f64, ScriptError> {
    let x = v
        .to_scalar()
        .map_err(|_| err(format!("ichol: opts.{name} must be a real scalar")))?;
    if x.is_nan() || x < 0.0 || !x.is_finite() {
        return Err(err(format!(
            "ichol: opts.{name} must be a nonnegative finite scalar, got {x}"
        )));
    }
    Ok(x)
}

fn on_off(name: &str, v: &Value) -> Result<bool, ScriptError> {
    match opt_str(name, v)?.as_str() {
        "on" => Ok(true),
        "off" => Ok(false),
        other => Err(err(format!(
            "ichol: opts.{name} must be \"on\" or \"off\", got \"{other}\""
        ))),
    }
}

/// `L = ichol(A)` / `ichol(A, opts)`: incomplete Cholesky factor of a sparse
/// Hermitian positive definite `A`, as a sparse lower-triangular `L` with
/// `L*L' ≈ A` (or upper `R` with `R'*R ≈ A` for `opts.shape = "upper"`).
/// Reads only the chosen triangle of `A`. `opts` fields: `type`
/// (`"nofill"` default, `"ict"`), `droptol` (0), `michol` (`"off"`),
/// `diagcomp` (0), `shape` (`"lower"`).
pub(crate) fn builtin_ichol(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() || args.len() > 2 {
        return Err(err(format!(
            "ichol: expected ichol(A) or ichol(A, opts), got {} arguments",
            args.len()
        )));
    }
    let a = match &args[0] {
        Value::SparseMatrix(s) => s,
        other => {
            return Err(err(format!(
                "ichol: A must be a sparse matrix (use sparse(A)), got {}",
                other.type_name()
            )))
        }
    };
    if a.rows != a.cols {
        return Err(err(format!(
            "ichol: A must be square, got {}×{}",
            a.rows, a.cols
        )));
    }

    let mut kind = "nofill".to_string();
    let mut droptol = 0.0;
    let mut opts = IcholOptions::default();
    let mut upper = false;
    if let Some(o) = args.get(1) {
        let fields = match o {
            Value::Struct(f) => f,
            other => {
                return Err(err(format!(
                    "ichol: opts must be a struct, got {}",
                    other.type_name()
                )))
            }
        };
        for (name, v) in fields {
            match name.as_str() {
                "type" => kind = opt_str("type", v)?,
                "droptol" => droptol = opt_nonneg("droptol", v)?,
                "michol" => opts.michol = on_off("michol", v)?,
                "diagcomp" => opts.diagcomp = opt_nonneg("diagcomp", v)?,
                "shape" => {
                    upper = match opt_str("shape", v)?.as_str() {
                        "lower" => false,
                        "upper" => true,
                        other => {
                            return Err(err(format!(
                                "ichol: opts.shape must be \"lower\" or \"upper\", got \"{other}\""
                            )))
                        }
                    }
                }
                other => {
                    return Err(err(format!(
                        "ichol: unknown option \"{other}\"; expected type, droptol, michol, diagcomp, or shape"
                    )))
                }
            }
        }
    }
    opts.kind = match kind.as_str() {
        "nofill" => IcholType::NoFill,
        "ict" => IcholType::Threshold { droptol },
        other => {
            return Err(err(format!(
                "ichol: opts.type must be \"nofill\" or \"ict\", got \"{other}\""
            )))
        }
    };

    // The upper triangle of A is the lower triangle of A'.
    let src = if upper {
        let mut t = a.transpose();
        for e in &mut t.entries {
            e.2 = e.2.conj();
        }
        t
    } else {
        a.clone()
    };
    let real = sparse_real(&src);
    if opts.michol && !real {
        return Err(err(
            "ichol: opts.michol = \"on\" needs a real matrix (dropped entries cannot keep a complex diagonal real)",
        ));
    }
    let entries = if real {
        let csc: SparseCsc<f64> = src.to_csc().map_err(|e| err(format!("ichol: {e}")))?;
        lower_entries(ichol(&csc, &opts), upper)?
    } else {
        let csc: SparseCsc<C64> = src.to_csc().map_err(|e| err(format!("ichol: {e}")))?;
        lower_entries(ichol(&csc, &opts), upper)?
    };
    Ok(Value::SparseMatrix(SparseMat::from_sorted_entries(
        a.rows, a.cols, entries,
    )))
}

/// `L` (or `L'` when `upper`) as row-major sorted COO entries.
fn lower_entries<T: PcgScalar>(
    l: Result<SparseCsc<T>, rustlab_core::sparse_solve::SparseSolveError>,
    upper: bool,
) -> Result<Vec<(usize, usize, C64)>, ScriptError> {
    let l = l.map_err(|e| err(format!("ichol: {e}")))?;
    let mut entries = Vec::with_capacity(l.nnz());
    for j in 0..l.ncols() {
        for (i, v) in l.col_iter(j) {
            let c = v.to_c64();
            entries.push(if upper { (j, i, c.conj()) } else { (i, j, c) });
        }
    }
    entries.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    Ok(entries)
}

pub mod builtins;
pub mod output;
pub mod parmap;
pub mod profile;
pub mod rng;
pub mod toml_io;
pub mod value;

use crate::ast::{BinOp, Expr, Stmt, StmtKind};
use crate::error::ScriptError;
pub use builtins::BuiltinRegistry;
use ndarray::{Array1, Array2};
use num_complex::Complex;
pub use profile::FnStats;
use rustlab_core::C64;
use std::collections::HashMap;
pub use value::NumberFormat;
pub use value::Value;

#[derive(Clone)]
struct UserFn {
    name: String,
    params: Vec<String>,
    /// Declared output variables. Empty for a no-return body, length 1 for
    /// the classic single-output form, length >= 2 for matlab multi-output.
    return_vars: Vec<String>,
    body: Vec<Stmt>,
}

#[derive(Clone)]
pub struct Evaluator {
    env: HashMap<String, Value>,
    builtins: BuiltinRegistry,
    user_fns: HashMap<String, UserFn>,
    /// True while executing a user-defined function body — suppresses auto-print of assignments.
    in_function: bool,
    profiler: profile::Profiler,
    /// When true, assignment output uses ANSI colour (green var name, dim `=`).
    pub color_output: bool,
    /// Active numeric display format (short, long, hex, commas).
    pub number_format: value::NumberFormat,
    /// Source line of the statement currently being executed (for error messages).
    current_line: usize,
}

impl Evaluator {
    /// Snapshot-safe deep clone. Identical to `Clone` except every `Value`
    /// in `env` goes through `Value::deep_clone()` so mutable `Arc<Mutex<_>>`
    /// interiors (`FirState`) don't alias between the snapshot and the live
    /// evaluator. Used by the notebook prefix cache to roll back to a
    /// previously-captured state without later mutations leaking in.
    pub fn deep_clone(&self) -> Self {
        Evaluator {
            env: self
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.deep_clone()))
                .collect(),
            builtins: self.builtins.clone(),
            user_fns: self.user_fns.clone(),
            in_function: self.in_function,
            profiler: self.profiler.clone(),
            color_output: self.color_output,
            number_format: self.number_format,
            current_line: self.current_line,
        }
    }

    pub fn new() -> Self {
        let mut env = HashMap::new();
        // Predefined constants: i and j both equal Complex(0, 1)
        env.insert(
            "j".to_string(),
            Value::Complex(num_complex::Complex::new(0.0, 1.0)),
        );
        env.insert(
            "i".to_string(),
            Value::Complex(num_complex::Complex::new(0.0, 1.0)),
        );
        // Also pi and e for convenience
        env.insert("pi".to_string(), Value::Scalar(std::f64::consts::PI));
        env.insert("e".to_string(), Value::Scalar(std::f64::consts::E));
        // IEEE special values
        env.insert("Inf".to_string(), Value::Scalar(f64::INFINITY));
        env.insert("NaN".to_string(), Value::Scalar(f64::NAN));
        // Boolean literals
        env.insert("true".to_string(), Value::Bool(true));
        env.insert("false".to_string(), Value::Bool(false));

        Self {
            env,
            builtins: BuiltinRegistry::with_defaults(),
            user_fns: HashMap::new(),
            in_function: false,
            profiler: profile::Profiler::default(),
            color_output: false,
            number_format: value::NumberFormat::Short,
            current_line: 0,
        }
    }

    /// Look up a variable in the environment (used by tests).
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.env.get(name)
    }

    /// Set a variable in the environment.
    pub fn set(&mut self, name: &str, value: Value) {
        self.env.insert(name.to_string(), value);
    }

    /// Remove a variable from the environment.
    pub fn remove(&mut self, name: &str) {
        self.env.remove(name);
    }

    /// Remove all user-defined variables and functions, keeping built-in constants (j, pi, e).
    pub fn clear_vars(&mut self) {
        const BUILTIN_CONSTS: &[&str] = &["i", "j", "pi", "e", "Inf", "NaN", "true", "false"];
        self.env.retain(|k, _| BUILTIN_CONSTS.contains(&k.as_str()));
        self.user_fns.clear();
    }

    /// Return names of all user-defined functions, sorted.
    pub fn user_fn_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.user_fns.keys().map(|k| k.as_str()).collect();
        names.sort();
        names
    }

    /// Return all user-defined variables, sorted by name.
    /// Excludes built-in constants (j, pi, e).
    pub fn vars(&self) -> Vec<(&str, &Value)> {
        const BUILTIN_CONSTS: &[&str] = &["i", "j", "pi", "e", "Inf", "NaN", "true", "false"];
        let mut entries: Vec<(&str, &Value)> = self
            .env
            .iter()
            .filter(|(k, _)| !BUILTIN_CONSTS.contains(&k.as_str()))
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        entries.sort_by_key(|(k, _)| *k);
        entries
    }

    /// Enable profiling. `names = None` tracks all functions; `Some(v)` tracks only the listed names.
    pub fn enable_profiling(&mut self, names: Option<Vec<String>>) {
        self.profiler.enable(names);
    }

    /// True if any profiling data has been recorded.
    pub fn has_profile_data(&self) -> bool {
        self.profiler.has_data()
    }

    /// Drain the profiling stats and return report rows sorted by total time.
    pub fn take_profile(&mut self) -> Vec<(String, FnStats)> {
        self.profiler.take_report()
    }

    /// Run statements and auto-print any profiling report to stderr at the end.
    /// Use this instead of `run` for top-level script execution.
    pub fn run_script(&mut self, stmts: &[Stmt]) -> Result<(), ScriptError> {
        let result = self.run(stmts);
        if self.profiler.has_data() {
            let rows = self.profiler.take_report();
            profile::print_report(&rows);
        }
        result
    }

    pub fn run(&mut self, stmts: &[Stmt]) -> Result<(), ScriptError> {
        for stmt in stmts {
            self.exec_stmt(stmt)?;
        }
        Ok(())
    }

    pub fn exec_stmt(&mut self, stmt: &Stmt) -> Result<(), ScriptError> {
        self.current_line = stmt.line;
        self.exec_stmt_kind(&stmt.kind)
            .map_err(|e| e.with_line(stmt.line))
    }

    /// True when assignment / index-assignment echo should be printed.
    ///
    /// Echo is suppressed inside user-defined functions and in notebook
    /// rendering mode — notebooks show only `print()` / `disp()` and bare
    /// expressions, matching Jupyter notebook conventions.
    #[inline]
    fn echo_enabled(&self) -> bool {
        !self.in_function && rustlab_plot::plot_context() != rustlab_plot::PlotContext::Notebook
    }

    fn exec_stmt_kind(&mut self, stmt: &StmtKind) -> Result<(), ScriptError> {
        match stmt {
            StmtKind::Assign {
                name,
                expr,
                suppress,
            } => {
                let val = self.eval_expr(expr)?;
                if !suppress && self.echo_enabled() {
                    let display = val.format_display(self.number_format);
                    if self.color_output && !output::capturing() {
                        output::script_println(&format!("\x1b[32m{}\x1b[0m = {}", name, display));
                    } else {
                        output::script_println(&format!("{} = {}", name, display));
                    }
                }
                self.env.insert(name.clone(), val);
            }
            StmtKind::FunctionDef {
                name,
                params,
                return_vars,
                body,
            } => {
                self.user_fns.insert(
                    name.clone(),
                    UserFn {
                        name: name.clone(),
                        params: params.clone(),
                        return_vars: return_vars.clone(),
                        body: body.clone(),
                    },
                );
            }
            StmtKind::FieldAssign {
                object,
                field,
                expr,
                suppress,
            } => {
                let val = self.eval_expr(expr)?;
                if !suppress && self.echo_enabled() {
                    let display = val.format_display(self.number_format);
                    if self.color_output && !output::capturing() {
                        output::script_println(&format!(
                            "\x1b[32m{}.{}\x1b[0m = {}",
                            object, field, display
                        ));
                    } else {
                        output::script_println(&format!("{}.{} = {}", object, field, display));
                    }
                }
                match self.env.get_mut(object) {
                    Some(Value::Struct(fields)) => {
                        fields.insert(field.clone(), val);
                    }
                    Some(other) => {
                        return Err(ScriptError::runtime(format!(
                            "'{}' is a {}, not a struct",
                            object,
                            other.type_name()
                        )));
                    }
                    None => {
                        // Auto-create a new struct when assigning to unknown.field
                        let mut fields = HashMap::new();
                        fields.insert(field.clone(), val);
                        self.env.insert(object.clone(), Value::Struct(fields));
                    }
                }
            }
            StmtKind::Return => {
                return Err(ScriptError::EarlyReturn);
            }
            StmtKind::If {
                cond,
                then_body,
                elseif_arms,
                else_body,
            } => {
                let cv = self.eval_expr(cond)?;
                let branch = Self::is_truthy(&cv, "if")?;
                if branch {
                    for s in then_body {
                        self.exec_stmt(s)?;
                    }
                } else {
                    let mut handled = false;
                    for (ei_cond, ei_body) in elseif_arms {
                        let ei_cv = self.eval_expr(ei_cond)?;
                        if Self::is_truthy(&ei_cv, "elseif")? {
                            for s in ei_body {
                                self.exec_stmt(s)?;
                            }
                            handled = true;
                            break;
                        }
                    }
                    if !handled {
                        for s in else_body {
                            self.exec_stmt(s)?;
                        }
                    }
                }
            }
            StmtKind::Switch {
                expr,
                cases,
                otherwise,
            } => {
                let switch_val = self.eval_expr(expr)?;
                let mut matched = false;
                for (case_expr, case_body) in cases {
                    let case_val = self.eval_expr(case_expr)?;
                    if Self::values_equal(&switch_val, &case_val) {
                        for s in case_body {
                            self.exec_stmt(s)?;
                        }
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    for s in otherwise {
                        self.exec_stmt(s)?;
                    }
                }
            }
            StmtKind::Format { mode } => {
                use value::NumberFormat;
                match mode.as_str() {
                    "short" | "default" => {
                        self.number_format = NumberFormat::Short;
                        output::script_println("format: short");
                    }
                    "long" => {
                        self.number_format = NumberFormat::Long;
                        output::script_println("format: long");
                    }
                    "hex" => {
                        self.number_format = NumberFormat::Hex;
                        output::script_println("format: hex");
                    }
                    "commas" => {
                        self.number_format = NumberFormat::Commas;
                        output::script_println("format: commas");
                    }
                    "" => {
                        output::script_println(&format!("format: {}", self.number_format.name()));
                    }
                    other => {
                        return Err(ScriptError::runtime(format!(
                            "format: unknown mode '{}' (try short, long, hex, commas)",
                            other
                        )));
                    }
                }
            }
            StmtKind::Hold { on } => {
                rustlab_plot::FIGURE.with(|fig| fig.borrow_mut().hold = *on);
                rustlab_plot::sync_figure_outputs();
            }
            StmtKind::Grid { on } => {
                rustlab_plot::FIGURE.with(|fig| fig.borrow_mut().current_mut().grid = *on);
                rustlab_plot::sync_figure_outputs();
            }
            StmtKind::Viewer { on, name } => {
                #[cfg(feature = "viewer")]
                {
                    match on {
                        Some(true) => {
                            let connect_result = if let Some(name) = name {
                                rustlab_plot::connect_viewer_named(name)
                            } else {
                                rustlab_plot::connect_viewer()
                            };
                            match connect_result {
                                Ok(true) => {
                                    let fig_id =
                                        rustlab_plot::viewer_live::get_viewer_fig_id()
                                            .unwrap_or(1);
                                    rustlab_plot::set_current_figure_output(
                                        rustlab_plot::FigureOutput::Viewer(fig_id),
                                    );
                                    if let Some(n) = name {
                                        eprintln!("viewer: connected to session '{}' — plots will render in rustlab-viewer", n);
                                    } else {
                                        eprintln!(
                                            "viewer: connected — plots will render in rustlab-viewer"
                                        );
                                    }
                                }
                                Ok(false) => {
                                    if let Some(n) = name {
                                        eprintln!("viewer: could not connect to session '{}' — is rustlab-viewer --name {} running?", n, n);
                                    } else {
                                        eprintln!(
                                            "viewer: could not connect — is rustlab-viewer running?"
                                        );
                                    }
                                    eprintln!("  plots will continue to render in the terminal");
                                }
                                Err(e) => {
                                    eprintln!("viewer: connection failed — {}", e);
                                    eprintln!("  plots will continue to render in the terminal");
                                }
                            }
                        }
                        Some(false) => {
                            rustlab_plot::disconnect_viewer();
                            rustlab_plot::set_current_figure_output(
                                rustlab_plot::FigureOutput::Terminal,
                            );
                            eprintln!("viewer: disconnected — plots will render in the terminal");
                        }
                        None => {
                            // Status query: report connection + current routing.
                            let connected = rustlab_plot::viewer_active();
                            let routing = match rustlab_plot::current_figure_output() {
                                rustlab_plot::FigureOutput::Viewer(id) => {
                                    format!("rustlab-viewer (figure id {})", id)
                                }
                                rustlab_plot::FigureOutput::Html(path) if !path.is_empty() => {
                                    format!("HTML file '{}'", path)
                                }
                                rustlab_plot::FigureOutput::Html(_) => {
                                    "HTML (pending savefig path)".to_string()
                                }
                                rustlab_plot::FigureOutput::Terminal => "the TUI".to_string(),
                            };
                            if connected {
                                eprintln!("viewer: connected");
                            } else {
                                eprintln!("viewer: not connected");
                            }
                            eprintln!("  current figure → {}", routing);
                            if !connected {
                                eprintln!("  use `viewer on` to connect an external rustlab-viewer");
                            }
                        }
                    }
                }
                #[cfg(not(feature = "viewer"))]
                {
                    let _ = (on, name);
                    eprintln!("viewer: not available in this build");
                    eprintln!("  rebuild with:  cargo build --features viewer");
                }
            }
            StmtKind::Run { path } => {
                let source = std::fs::read_to_string(path)
                    .map_err(|e| ScriptError::runtime(format!("run: {}: {}", path, e)))?;
                let tokens = crate::lexer::tokenize(&source)?;
                let stmts = crate::parser::parse(tokens)?;
                for s in &stmts {
                    self.exec_stmt(s)?;
                }
            }
            StmtKind::MultiAssign {
                names,
                expr,
                suppress,
            } => {
                // For a top-level Call with multi-output destructuring, pass
                // the LHS name count as `nargout` so the callee can dispatch
                // on caller arity (matlab-style overloading). Three Call
                // shapes are handled here:
                //   - registered builtin (not in env, not in user_fns) →
                //     `call_builtin_tracked_nargout`
                //   - user function → `eval_user_fn_nargout`
                //   - lambda or non-Call expression → fall through to the
                //     normal eval path (the resulting `Value::Tuple` is
                //     destructured below)
                let val = match expr {
                    Expr::Call { name, args }
                        if !self.env.contains_key(name)
                            && self.user_fns.contains_key(name) =>
                    {
                        let func = self.user_fns.get(name).cloned().unwrap();
                        let vals: Vec<Value> = args
                            .iter()
                            .map(|a| self.eval_expr(a))
                            .collect::<Result<_, _>>()?;
                        self.eval_user_fn_nargout(func, vals, names.len())?
                    }
                    Expr::Call { name, args }
                        if !self.env.contains_key(name)
                            && !self.user_fns.contains_key(name) =>
                    {
                        let vals: Vec<Value> = args
                            .iter()
                            .map(|a| self.eval_expr(a))
                            .collect::<Result<_, _>>()?;
                        self.call_builtin_tracked_nargout(name, vals, names.len())?
                    }
                    _ => self.eval_expr(expr)?,
                };
                match val {
                    Value::Tuple(values) => {
                        if values.len() < names.len() {
                            return Err(ScriptError::runtime(format!(
                                "multi-assign: expected {} values, function returned {}",
                                names.len(),
                                values.len()
                            )));
                        }
                        for (name, v) in names.iter().zip(values.into_iter()) {
                            if name == "~" {
                                continue;
                            } // discard
                            if !suppress && self.echo_enabled() {
                                let display = v.format_display(self.number_format);
                                if self.color_output && !output::capturing() {
                                    output::script_println(&format!(
                                        "\x1b[32m{}\x1b[0m = {}",
                                        name, display
                                    ));
                                } else {
                                    output::script_println(&format!("{} = {}", name, display));
                                }
                            }
                            self.env.insert(name.clone(), v);
                        }
                    }
                    single => {
                        if names.len() != 1 {
                            return Err(ScriptError::runtime(format!(
                                "multi-assign: expected {} values, function returned 1",
                                names.len()
                            )));
                        }
                        if names[0] != "~" {
                            if !suppress && self.echo_enabled() {
                                if self.color_output && !output::capturing() {
                                    output::script_println(&format!(
                                        "\x1b[32m{}\x1b[0m = {}",
                                        names[0], single
                                    ));
                                } else {
                                    output::script_println(&format!("{} = {}", names[0], single));
                                }
                            }
                            self.env.insert(names[0].clone(), single);
                        }
                    }
                }
            }
            StmtKind::While { cond, body } => loop {
                let cv = self.eval_expr(cond)?;
                if !Self::is_truthy(&cv, "while")? {
                    break;
                }
                for s in body {
                    self.exec_stmt(s)?;
                }
            },
            StmtKind::For { var, iter, body } => {
                let iter_val = self.eval_expr(iter)?;
                let elements = match iter_val {
                    Value::Vector(v) => v.to_vec(),
                    Value::Scalar(n) => vec![Complex::new(n, 0.0)],
                    other => {
                        return Err(ScriptError::runtime(format!(
                            "for: cannot iterate over {}",
                            other.type_name()
                        )))
                    }
                };
                for elem in elements {
                    let val = if elem.im == 0.0 {
                        Value::Scalar(elem.re)
                    } else {
                        Value::Complex(elem)
                    };
                    self.env.insert(var.clone(), val);
                    for s in body {
                        self.exec_stmt(s)?;
                    }
                }
            }
            StmtKind::IndexAssign {
                name,
                indices,
                expr,
                suppress,
            } => {
                let val = self.eval_expr(expr)?;

                // Octave/matlab `[]` deletion: when the right-hand side is an
                // empty vector or matrix, the assignment removes the indexed
                // elements from the container instead of writing into them.
                // Vector form `v(idx) = []` is supported; matrix row/column
                // deletion (`M(i, :) = []`) is a follow-up.
                let rhs_is_empty = match &val {
                    Value::Vector(v) => v.is_empty(),
                    Value::Matrix(m) => m.is_empty(),
                    _ => false,
                };
                if rhs_is_empty {
                    return self.exec_index_delete(name, indices, *suppress);
                }

                // Evaluate indices with `end` bound to current container length (if any).
                // Tensor3 with 3 indices gets per-dim `end` binding.
                let idx_vals: Vec<Value> = if indices.len() == 3
                    && matches!(self.env.get(name.as_str()), Some(Value::Tensor3(_)))
                {
                    let (m, n, p) = match self.env.get(name.as_str()) {
                        Some(Value::Tensor3(t)) => {
                            let s = t.shape();
                            (s[0], s[1], s[2])
                        }
                        _ => unreachable!(),
                    };
                    self.env.insert("end".to_string(), Value::Scalar(m as f64));
                    let iv = self.eval_expr(&indices[0])?;
                    self.env.insert("end".to_string(), Value::Scalar(n as f64));
                    let jv = self.eval_expr(&indices[1])?;
                    self.env.insert("end".to_string(), Value::Scalar(p as f64));
                    let kv = self.eval_expr(&indices[2])?;
                    self.env.remove("end");
                    vec![iv, jv, kv]
                } else {
                    let container_len = match self.env.get(name.as_str()) {
                        Some(Value::Vector(v)) => v.len(),
                        Some(Value::Matrix(m)) if indices.len() == 1 => m.nrows() * m.ncols(),
                        Some(Value::SparseVector(sv)) => sv.len,
                        Some(Value::SparseMatrix(sm)) if indices.len() == 1 => sm.rows * sm.cols,
                        _ => 0,
                    };
                    self.env
                        .insert("end".to_string(), Value::Scalar(container_len as f64));
                    let vals: Vec<Value> = indices
                        .iter()
                        .map(|a| self.eval_expr(a))
                        .collect::<Result<_, _>>()?;
                    self.env.remove("end");
                    vals
                };

                if idx_vals.len() == 1 {
                    // Strided / range / vector LHS into a Vector:
                    // `v(idx_set) = rhs` where idx_set is `:`, a Vector,
                    // or any colon range that evaluates to a Vector.
                    // Handled before the scalar path because `to_scalar()`
                    // would reject the Vector index.
                    let idx_is_multi = matches!(&idx_vals[0], Value::All | Value::Vector(_));
                    let env_is_vector =
                        matches!(self.env.get(name.as_str()), Some(Value::Vector(_)));
                    if idx_is_multi && env_is_vector {
                        let vec_len = match self.env.get(name.as_str()) {
                            Some(Value::Vector(v)) => v.len(),
                            _ => unreachable!(),
                        };
                        let positions = Value::resolve_index_dim_public(&idx_vals[0], vec_len)
                            .map_err(ScriptError::runtime)?;
                        let n_target = positions.len();
                        let src: Vec<Complex<f64>> = match &val {
                            Value::Scalar(n) => vec![Complex::new(*n, 0.0); n_target],
                            Value::Complex(c) => vec![*c; n_target],
                            Value::Vector(v) => {
                                if v.len() != n_target {
                                    return Err(ScriptError::runtime(format!(
                                        "index assignment: RHS vector length {} does not match \
                                         target index count {}",
                                        v.len(),
                                        n_target
                                    )));
                                }
                                v.iter().copied().collect()
                            }
                            other => {
                                return Err(ScriptError::runtime(format!(
                                    "index assignment: right-hand side must be scalar, complex, \
                                     or vector; got {}",
                                    other.type_name()
                                )))
                            }
                        };
                        match self.env.get_mut(name.as_str()) {
                            Some(Value::Vector(v)) => {
                                for (k, &p) in positions.iter().enumerate() {
                                    v[p] = src[k];
                                }
                                if !suppress && self.echo_enabled() {
                                    output::script_println(&format!(
                                        "{}(...) = (vector positions {} updated)",
                                        name, n_target
                                    ));
                                }
                            }
                            _ => unreachable!(),
                        }
                        return Ok(());
                    }
                    let idx = idx_vals[0]
                        .to_scalar()
                        .map_err(|e| ScriptError::type_err(e))?
                        as usize;
                    if idx < 1 {
                        return Err(ScriptError::runtime(
                            "index assignment: index must be >= 1".to_string(),
                        ));
                    }
                    // Single-index sparse vector assignment: sv(k) = val
                    let is_sparse_vec_assign =
                        matches!(self.env.get(name.as_str()), Some(Value::SparseVector(_)));
                    if is_sparse_vec_assign {
                        let assign_val = match &val {
                            Value::Scalar(n)  => Complex::new(*n, 0.0),
                            Value::Complex(c) => *c,
                            other => return Err(ScriptError::runtime(format!(
                                "index assignment: right-hand side must be scalar or complex, got {}",
                                other.type_name()
                            ))),
                        };
                        match self.env.get_mut(name.as_str()) {
                            Some(Value::SparseVector(sv)) => {
                                if idx > sv.len {
                                    return Err(ScriptError::runtime(format!(
                                        "index assignment: index {} out of bounds (length {})",
                                        idx, sv.len
                                    )));
                                }
                                sv.set(idx - 1, assign_val);
                                if !suppress && self.echo_enabled() {
                                    output::script_println(&format!(
                                        "{}({}) = {}",
                                        name,
                                        idx,
                                        Value::Complex(assign_val)
                                    ));
                                }
                            }
                            _ => unreachable!(),
                        }
                    } else
                    // Single-index matrix row assignment: M(i) = row_vector
                    if matches!(self.env.get(name.as_str()), Some(Value::Matrix(_)))
                        && matches!(&val, Value::Vector(_))
                    {
                        let row_data = match &val {
                            Value::Vector(v) => v.clone(),
                            _ => unreachable!(),
                        };
                        match self.env.get_mut(name.as_str()) {
                            Some(Value::Matrix(m)) => {
                                if idx > m.nrows() {
                                    return Err(ScriptError::runtime(format!(
                                        "index assignment: row {} out of bounds for {}×{} matrix",
                                        idx,
                                        m.nrows(),
                                        m.ncols()
                                    )));
                                }
                                if row_data.len() != m.ncols() {
                                    return Err(ScriptError::runtime(format!(
                                        "index assignment: row vector length {} does not match matrix columns {}",
                                        row_data.len(), m.ncols()
                                    )));
                                }
                                for (col, &v) in row_data.iter().enumerate() {
                                    m[[idx - 1, col]] = v;
                                }
                                if !suppress && self.echo_enabled() {
                                    output::script_println(&format!(
                                        "{}({}) = [{}]",
                                        name,
                                        idx,
                                        row_data
                                            .iter()
                                            .map(|c| format!("{}", Value::Complex(*c)))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    ));
                                }
                            }
                            _ => unreachable!(),
                        }
                    } else if matches!(self.env.get(name.as_str()), Some(Value::Matrix(_))) {
                        // Single-index scalar/complex assignment into an existing
                        // Matrix. A row vector (1×N) or column vector (N×1) keeps
                        // its shape and grows along its non-singleton axis if
                        // idx exceeds the current length. A general 2D matrix
                        // takes column-major linear-index semantics with no
                        // growth (out-of-bounds is an error).
                        let assign_val = match &val {
                            Value::Scalar(n) => Complex::new(*n, 0.0),
                            Value::Complex(c) => *c,
                            other => {
                                return Err(ScriptError::runtime(format!(
                            "index assignment: right-hand side must be scalar or complex, got {}",
                            other.type_name()
                        )))
                            }
                        };
                        match self.env.get_mut(name.as_str()) {
                            Some(Value::Matrix(m)) => {
                                let nr = m.nrows();
                                let nc = m.ncols();
                                if nr == 1 {
                                    if idx > nc {
                                        let mut new_m = Array2::zeros((1, idx));
                                        for c in 0..nc {
                                            new_m[[0, c]] = m[[0, c]];
                                        }
                                        *m = new_m;
                                    }
                                    m[[0, idx - 1]] = assign_val;
                                } else if nc == 1 {
                                    if idx > nr {
                                        let mut new_m = Array2::zeros((idx, 1));
                                        for r in 0..nr {
                                            new_m[[r, 0]] = m[[r, 0]];
                                        }
                                        *m = new_m;
                                    }
                                    m[[idx - 1, 0]] = assign_val;
                                } else {
                                    let total = nr * nc;
                                    if idx > total {
                                        return Err(ScriptError::runtime(format!(
                                            "index assignment: linear index {} out of bounds for {}×{} matrix ({} elements)",
                                            idx, nr, nc, total
                                        )));
                                    }
                                    // column-major: k0 = col * nrows + row
                                    let k0 = idx - 1;
                                    let col = k0 / nr;
                                    let row = k0 % nr;
                                    m[[row, col]] = assign_val;
                                }
                            }
                            _ => unreachable!(),
                        }
                        if !suppress && self.echo_enabled() {
                            output::script_println(&format!(
                                "{}({}) = {}",
                                name,
                                idx,
                                Value::Complex(assign_val)
                            ));
                        }
                    } else {
                        // Single-index: vector assignment (auto-create/grow)
                        let assign_val = match &val {
                            Value::Scalar(n) => Complex::new(*n, 0.0),
                            Value::Complex(c) => *c,
                            other => {
                                return Err(ScriptError::runtime(format!(
                            "index assignment: right-hand side must be scalar or complex, got {}",
                            other.type_name()
                        )))
                            }
                        };
                        let vec = match self.env.get_mut(name.as_str()) {
                            Some(Value::Vector(v)) => {
                                if idx > v.len() {
                                    let mut new_vec = vec![Complex::new(0.0, 0.0); idx];
                                    for (i, c) in v.iter().enumerate() {
                                        new_vec[i] = *c;
                                    }
                                    *v = Array1::from_vec(new_vec);
                                }
                                v
                            }
                            _ => {
                                // Create new vector of length idx, filled with zeros
                                let new_vec = vec![Complex::new(0.0, 0.0); idx];
                                self.env
                                    .insert(name.clone(), Value::Vector(Array1::from_vec(new_vec)));
                                match self.env.get_mut(name.as_str()) {
                                    Some(Value::Vector(v)) => v,
                                    _ => unreachable!(),
                                }
                            }
                        };
                        vec[idx - 1] = assign_val;
                        if !suppress && self.echo_enabled() {
                            output::script_println(&format!(
                                "{}({}) = {}",
                                name,
                                idx,
                                Value::Complex(assign_val)
                            ));
                        }
                    } // end else scalar assignment
                } else if idx_vals.len() == 2 {
                    // Two-index: matrix assignment. Indices may be
                    // Scalar, `:` (Value::All), or Vector — resolved
                    // to 0-based row/col lists. The RHS must broadcast
                    // to the (rows.len(), cols.len()) target region:
                    // a Scalar/Complex broadcasts everywhere; a Vector
                    // matches a single row or column; a Matrix matches
                    // the full sub-shape. Single-element writes go
                    // through the original Scalar/Complex path so the
                    // SparseMatrix branch is preserved unchanged.
                    let single_element = matches!(idx_vals[0], Value::Scalar(_))
                        && matches!(idx_vals[1], Value::Scalar(_))
                        && matches!(&val, Value::Scalar(_) | Value::Complex(_));
                    if single_element {
                        let row = idx_vals[0]
                            .to_scalar()
                            .map_err(|e| ScriptError::type_err(e))?
                            as usize;
                        let col = idx_vals[1]
                            .to_scalar()
                            .map_err(|e| ScriptError::type_err(e))?
                            as usize;
                        if row < 1 || col < 1 {
                            return Err(ScriptError::runtime(
                                "index assignment: indices must be >= 1".to_string(),
                            ));
                        }
                        let assign_val = match &val {
                            Value::Scalar(n) => Complex::new(*n, 0.0),
                            Value::Complex(c) => *c,
                            _ => unreachable!(),
                        };
                        match self.env.get_mut(name.as_str()) {
                            Some(Value::Matrix(m)) => {
                                if row > m.nrows() || col > m.ncols() {
                                    return Err(ScriptError::runtime(format!(
                                        "index assignment: ({},{}) out of bounds for {}×{} matrix",
                                        row,
                                        col,
                                        m.nrows(),
                                        m.ncols()
                                    )));
                                }
                                m[[row - 1, col - 1]] = assign_val;
                                if !suppress && self.echo_enabled() {
                                    output::script_println(&format!(
                                        "{}({},{}) = {}",
                                        name,
                                        row,
                                        col,
                                        Value::Complex(assign_val)
                                    ));
                                }
                            }
                            Some(Value::SparseMatrix(sm)) => {
                                if row > sm.rows || col > sm.cols {
                                    return Err(ScriptError::runtime(format!(
                                        "index assignment: ({},{}) out of bounds for {}×{} sparse matrix",
                                        row, col, sm.rows, sm.cols
                                    )));
                                }
                                sm.set(row - 1, col - 1, assign_val);
                                if !suppress && self.echo_enabled() {
                                    output::script_println(&format!(
                                        "{}({},{}) = {}",
                                        name,
                                        row,
                                        col,
                                        Value::Complex(assign_val)
                                    ));
                                }
                            }
                            _ => {
                                return Err(ScriptError::runtime(format!(
                                    "index assignment: '{}' is not a matrix",
                                    name
                                )))
                            }
                        }
                    } else {
                        // Region write: A(rows, cols) = ...
                        let (nr, nc) = match self.env.get(name.as_str()) {
                            Some(Value::Matrix(m)) => (m.nrows(), m.ncols()),
                            _ => {
                                return Err(ScriptError::runtime(format!(
                                    "index assignment: '{}' is not a matrix \
                                     (sparse and growing forms not supported for region writes)",
                                    name
                                )))
                            }
                        };
                        let rows = Value::resolve_index_dim_public(&idx_vals[0], nr)
                            .map_err(ScriptError::runtime)?;
                        let cols = Value::resolve_index_dim_public(&idx_vals[1], nc)
                            .map_err(ScriptError::runtime)?;
                        let n_target = rows.len() * cols.len();
                        // Build the (rows.len(), cols.len()) source data.
                        let src: Vec<Complex<f64>> = match &val {
                            Value::Scalar(n) => vec![Complex::new(*n, 0.0); n_target],
                            Value::Complex(c) => vec![*c; n_target],
                            Value::Vector(v) => {
                                if rows.len() == 1 && v.len() == cols.len() {
                                    v.iter().copied().collect()
                                } else if cols.len() == 1 && v.len() == rows.len() {
                                    v.iter().copied().collect()
                                } else if v.len() == n_target {
                                    v.iter().copied().collect()
                                } else {
                                    return Err(ScriptError::runtime(format!(
                                        "index assignment: vector of length {} does not match \
                                         target region {}×{} ({} elements)",
                                        v.len(),
                                        rows.len(),
                                        cols.len(),
                                        n_target
                                    )));
                                }
                            }
                            Value::Matrix(rhs) => {
                                if rhs.nrows() != rows.len() || rhs.ncols() != cols.len() {
                                    return Err(ScriptError::runtime(format!(
                                        "index assignment: RHS shape {}×{} does not match \
                                         target region {}×{}",
                                        rhs.nrows(),
                                        rhs.ncols(),
                                        rows.len(),
                                        cols.len()
                                    )));
                                }
                                let mut out = Vec::with_capacity(n_target);
                                for i in 0..rhs.nrows() {
                                    for j in 0..rhs.ncols() {
                                        out.push(rhs[[i, j]]);
                                    }
                                }
                                out
                            }
                            other => {
                                return Err(ScriptError::runtime(format!(
                                    "index assignment: right-hand side must be scalar, complex, \
                                     vector, or matrix; got {}",
                                    other.type_name()
                                )))
                            }
                        };
                        match self.env.get_mut(name.as_str()) {
                            Some(Value::Matrix(m)) => {
                                let mut k = 0usize;
                                for &r in &rows {
                                    for &c in &cols {
                                        m[[r, c]] = src[k];
                                        k += 1;
                                    }
                                }
                                if !suppress && self.echo_enabled() {
                                    output::script_println(&format!(
                                        "{}(...) = (region {}×{} updated)",
                                        name,
                                        rows.len(),
                                        cols.len()
                                    ));
                                }
                            }
                            _ => unreachable!(),
                        }
                    }
                } else if idx_vals.len() == 3 {
                    // Three-index: tensor3 assignment.
                    self.tensor3_index_assign(name, &idx_vals, &val, *suppress)?;
                } else {
                    return Err(ScriptError::runtime(
                        "index assignment: only 1, 2, or 3 indices are supported".to_string(),
                    ));
                }
            }
            StmtKind::Expr(expr, suppress) => {
                // Special case: bare `clear` and `clf` commands (no parens)
                if let Expr::Var(name) = expr {
                    if name == "clear" {
                        self.clear_vars();
                        return Ok(());
                    }
                    if name == "clf" {
                        rustlab_plot::FIGURE.with(|fig| fig.borrow_mut().reset());
                        rustlab_plot::sync_figure_outputs();
                        return Ok(());
                    }
                }

                // Special case: bare load("file.npz") injects all variables into the workspace.
                if let Expr::Call { name, args } = expr {
                    if name == "load" && args.len() == 1 {
                        let path_val = self.eval_expr(&args[0])?;
                        if let Ok(path) = path_val.to_str() {
                            if path.ends_with(".npz") {
                                let vars = builtins::load_all_from_npz(&path)
                                    .map_err(|e| ScriptError::runtime(e))?;
                                if !suppress {
                                    let names: Vec<&str> =
                                        vars.iter().map(|(n, _)| n.as_str()).collect();
                                    output::script_println(&format!(
                                        "loaded: {}",
                                        names.join(", ")
                                    ));
                                }
                                for (var_name, val) in vars {
                                    self.env.insert(var_name, val);
                                }
                                return Ok(());
                            }
                        }
                    }
                }

                let val = self.eval_expr(expr)?;
                if !suppress && !self.in_function && !matches!(val, Value::None) {
                    output::script_println(&val.format_display(self.number_format));
                }
            }
        }
        Ok(())
    }

    /// Assign into a Tensor3 via 3-index indexing.
    ///
    /// Supports: scalar target `A(i, j, k) = s`, full-tensor region
    /// `A(rows, cols, pages) = Tensor3`, and reduced-rank RHS where the index
    /// set implies a Matrix (one singleton index) or Vector (two singletons).
    fn tensor3_index_assign(
        &mut self,
        name: &str,
        idx_vals: &[Value],
        val: &Value,
        suppress: bool,
    ) -> Result<(), ScriptError> {
        let (m, n, p) = match self.env.get(name) {
            Some(Value::Tensor3(t)) => {
                let s = t.shape();
                (s[0], s[1], s[2])
            }
            _ => {
                return Err(ScriptError::runtime(format!(
                    "index assignment: '{}' is not a tensor3",
                    name
                )))
            }
        };
        let rows = Value::resolve_index_dim_public(&idx_vals[0], m)
            .map_err(|e| ScriptError::runtime(e))?;
        let cols = Value::resolve_index_dim_public(&idx_vals[1], n)
            .map_err(|e| ScriptError::runtime(e))?;
        let pages = Value::resolve_index_dim_public(&idx_vals[2], p)
            .map_err(|e| ScriptError::runtime(e))?;
        let (nr, nc, np) = (rows.len(), cols.len(), pages.len());
        let total = nr * nc * np;

        // Gather RHS into a flat buffer of length `total`, in (r,c,k)-major order.
        let flat: Vec<Complex<f64>> = match val {
            Value::Scalar(s) => vec![Complex::new(*s, 0.0); total],
            Value::Complex(c) => vec![*c; total],
            Value::Matrix(rhs) => {
                // Determine which dim the matrix maps to based on singleton pattern.
                let singletons = [nr == 1, nc == 1, np == 1];
                let (exp_rows, exp_cols) = if singletons[0] {
                    (nc, np) // A(i, :, :) = M  → M is (nc × np)
                } else if singletons[1] {
                    (nr, np) // A(:, j, :) = M  → M is (nr × np)
                } else if singletons[2] {
                    (nr, nc) // A(:, :, k) = M  → M is (nr × nc)
                } else {
                    return Err(ScriptError::runtime(format!(
                        "index assignment: cannot assign {}×{} matrix into {}×{}×{} region",
                        rhs.nrows(), rhs.ncols(), nr, nc, np
                    )));
                };
                if rhs.nrows() != exp_rows || rhs.ncols() != exp_cols {
                    return Err(ScriptError::runtime(format!(
                        "index assignment: RHS matrix is {}×{} but expected {}×{}",
                        rhs.nrows(), rhs.ncols(), exp_rows, exp_cols
                    )));
                }
                // Walk (r, c, k) in the same outer→inner order as index_3d and
                // place each element from the matrix using the right (row, col) pair.
                let mut out = Vec::with_capacity(total);
                for ir in 0..nr {
                    for ic in 0..nc {
                        for ik in 0..np {
                            let (mr, mc) = if singletons[0] {
                                (ic, ik)
                            } else if singletons[1] {
                                (ir, ik)
                            } else {
                                (ir, ic)
                            };
                            out.push(rhs[[mr, mc]]);
                        }
                    }
                }
                out
            }
            Value::Vector(rhs) => {
                // Only legal when exactly one dimension is non-singleton.
                let non_singleton = [nr, nc, np].iter().filter(|&&x| x > 1).count();
                if non_singleton != 1 {
                    return Err(ScriptError::runtime(format!(
                        "index assignment: vector RHS requires exactly one non-singleton index, got region {}×{}×{}",
                        nr, nc, np
                    )));
                }
                if rhs.len() != total {
                    return Err(ScriptError::runtime(format!(
                        "index assignment: vector RHS length {} does not match region size {}",
                        rhs.len(), total
                    )));
                }
                rhs.iter().copied().collect()
            }
            Value::Tensor3(rhs) => {
                if rhs.shape() != [nr, nc, np] {
                    return Err(ScriptError::runtime(format!(
                        "index assignment: RHS tensor3 shape {:?} does not match region {}×{}×{}",
                        rhs.shape(), nr, nc, np
                    )));
                }
                let mut out = Vec::with_capacity(total);
                for ir in 0..nr {
                    for ic in 0..nc {
                        for ik in 0..np {
                            out.push(rhs[[ir, ic, ik]]);
                        }
                    }
                }
                out
            }
            other => {
                return Err(ScriptError::runtime(format!(
                    "index assignment: right-hand side must be scalar, complex, matrix, vector, or tensor3, got {}",
                    other.type_name()
                )))
            }
        };

        // Write back into the stored Tensor3.
        if let Some(Value::Tensor3(t)) = self.env.get_mut(name) {
            let mut idx = 0usize;
            for &r in &rows {
                for &c in &cols {
                    for &k in &pages {
                        t[[r, c, k]] = flat[idx];
                        idx += 1;
                    }
                }
            }
        } else {
            unreachable!();
        }

        if !suppress && self.echo_enabled() {
            output::script_println(&format!(
                "{}({}×{}×{} region) = <{}>",
                name,
                nr,
                nc,
                np,
                val.type_name()
            ));
        }
        Ok(())
    }

    /// `v(idx) = []` and `M(rows, :) = []` / `M(:, cols) = []` —
    /// remove the indexed positions and write the shortened result back.
    /// Octave/matlab compatibility.
    fn exec_index_delete(
        &mut self,
        name: &str,
        indices: &[Expr],
        suppress: bool,
    ) -> Result<(), ScriptError> {
        let new_val = match (indices.len(), self.env.get(name).cloned()) {
            // ── Single-index forms ──────────────────────────────────────────
            (1, Some(Value::Vector(v))) => {
                let len = v.len();
                self.env
                    .insert("end".to_string(), Value::Scalar(len as f64));
                let idx_val = self.eval_expr(&indices[0])?;
                self.env.remove("end");
                let mut to_remove =
                    Value::resolve_index_dim(&idx_val, len).map_err(ScriptError::runtime)?;
                to_remove.sort_unstable();
                to_remove.dedup();
                let kept: Vec<C64> = (0..len)
                    .filter(|i| to_remove.binary_search(i).is_err())
                    .map(|i| v[i])
                    .collect();
                Value::Vector(Array1::from_vec(kept))
            }

            // ── Matrix row/column deletion (one axis Value::All, other a list) ──
            (2, Some(Value::Matrix(m))) => {
                let nrows = m.nrows();
                let ncols = m.ncols();
                self.env
                    .insert("end".to_string(), Value::Scalar(nrows as f64));
                let idx0 = self.eval_expr(&indices[0])?;
                self.env
                    .insert("end".to_string(), Value::Scalar(ncols as f64));
                let idx1 = self.eval_expr(&indices[1])?;
                self.env.remove("end");

                let row_all = matches!(idx0, Value::All);
                let col_all = matches!(idx1, Value::All);

                if row_all && col_all {
                    // M(:, :) = [] → empty matrix
                    Value::Matrix(Array2::zeros((0, 0)))
                } else if col_all {
                    // M(rows, :) = [] → drop the listed rows
                    let mut to_remove = Value::resolve_index_dim(&idx0, nrows)
                        .map_err(ScriptError::runtime)?;
                    to_remove.sort_unstable();
                    to_remove.dedup();
                    let kept_rows: Vec<usize> = (0..nrows)
                        .filter(|r| to_remove.binary_search(r).is_err())
                        .collect();
                    let new_nrows = kept_rows.len();
                    let new = Array2::from_shape_fn((new_nrows, ncols), |(i, j)| {
                        m[[kept_rows[i], j]]
                    });
                    Value::Matrix(new)
                } else if row_all {
                    // M(:, cols) = [] → drop the listed columns
                    let mut to_remove = Value::resolve_index_dim(&idx1, ncols)
                        .map_err(ScriptError::runtime)?;
                    to_remove.sort_unstable();
                    to_remove.dedup();
                    let kept_cols: Vec<usize> = (0..ncols)
                        .filter(|c| to_remove.binary_search(c).is_err())
                        .collect();
                    let new_ncols = kept_cols.len();
                    let new = Array2::from_shape_fn((nrows, new_ncols), |(i, j)| {
                        m[[i, kept_cols[j]]]
                    });
                    Value::Matrix(new)
                } else {
                    return Err(ScriptError::runtime(
                        "[]-deletion on a matrix: one of the two indices must be `:` \
                         (deleting individual elements would leave a hole)"
                            .to_string(),
                    ));
                }
            }

            // ── Unsupported shapes ──────────────────────────────────────────
            (1, Some(Value::Matrix(_))) => {
                return Err(ScriptError::runtime(
                    "[]-deletion on a matrix: use M(rows, :) = [] or M(:, cols) = [] \
                     (single-index `M(k) = []` is not supported — would leave a hole)"
                        .to_string(),
                ));
            }
            (n, Some(other)) => {
                return Err(ScriptError::runtime(format!(
                    "[]-deletion: '{}' is a {}; expected vector or matrix, got {} indices",
                    name,
                    other.type_name(),
                    n
                )));
            }
            (_, None) => {
                return Err(ScriptError::runtime(format!(
                    "[]-deletion: undefined variable '{}'",
                    name
                )));
            }
        };

        if !suppress && self.echo_enabled() {
            let display = new_val.format_display(self.number_format);
            if self.color_output && !output::capturing() {
                output::script_println(&format!("\x1b[32m{}\x1b[0m = {}", name, display));
            } else {
                output::script_println(&format!("{} = {}", name, display));
            }
        }
        self.env.insert(name.to_string(), new_val);
        Ok(())
    }

    fn is_truthy(val: &Value, context: &str) -> Result<bool, ScriptError> {
        match val {
            Value::Bool(b) => Ok(*b),
            Value::Scalar(n) => Ok(*n != 0.0),
            Value::Complex(c) => Ok(c.re != 0.0 || c.im != 0.0),
            other => Err(ScriptError::runtime(format!(
                "{} condition must be a bool or scalar, got {}",
                context,
                other.type_name()
            ))),
        }
    }

    fn values_equal(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Scalar(x), Value::Scalar(y)) => x == y,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Complex(x), Value::Complex(y)) => x == y,
            (Value::Scalar(x), Value::Complex(y)) => *x == y.re && y.im == 0.0,
            (Value::Complex(x), Value::Scalar(y)) => x.re == *y && x.im == 0.0,
            (Value::Str(x), Value::Str(y)) => x == y,
            _ => false,
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, ScriptError> {
        match expr {
            Expr::Number(n) => Ok(Value::Scalar(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Var(name) => self
                .env
                .get(name)
                .cloned()
                .ok_or_else(|| ScriptError::undefined(name.clone())),
            Expr::UnaryMinus(inner) => {
                let v = self.eval_expr(inner)?;
                v.negate().map_err(|e| ScriptError::type_err(e))
            }
            Expr::UnaryNot(inner) => {
                let v = self.eval_expr(inner)?;
                v.not().map_err(|e| ScriptError::type_err(e))
            }
            Expr::BinOp { op, lhs, rhs } => {
                // Short-circuit logical ops: evaluate rhs only when lhs is
                // not decisive. Matches matlab/octave `&&` / `||`.
                if matches!(op, BinOp::And | BinOp::Or) {
                    let l = self.eval_expr(lhs)?;
                    let lt = l.is_truthy_for_logical().map_err(ScriptError::type_err)?;
                    let decisive = match op {
                        BinOp::And => !lt,
                        BinOp::Or => lt,
                        _ => unreachable!(),
                    };
                    if decisive {
                        return Ok(Value::Bool(lt));
                    }
                    let r = self.eval_expr(rhs)?;
                    let rt = r.is_truthy_for_logical().map_err(ScriptError::type_err)?;
                    return Ok(Value::Bool(rt));
                }
                let l = self.eval_expr(lhs)?;
                let r = self.eval_expr(rhs)?;
                Value::binop(*op, l, r).map_err(|e| ScriptError::type_err(e))
            }
            Expr::Call { name, args } => {
                // ── In-script profiling control ───────────────────────────
                if name == "profile" {
                    // profile(fft, myfun) or profile() — args are bare Var names or strings
                    let names: Vec<String> = args.iter().map(|a| match a {
                        Expr::Var(n) | Expr::Str(n) => Ok(n.clone()),
                        _ => Err(ScriptError::runtime(
                            "profile: arguments must be function names (e.g. profile(fft, myfun))".to_string()
                        )),
                    }).collect::<Result<_, _>>()?;
                    let whitelist = if names.is_empty() { None } else { Some(names) };
                    self.profiler.enable(whitelist);
                    return Ok(Value::None);
                }
                if name == "profile_report" && args.is_empty() {
                    let rows = self.profiler.take_report();
                    profile::print_report(&rows);
                    return Ok(Value::None);
                }

                // ── Evaluator-level higher-order functions ────────────────
                if name == "arrayfun" && args.len() == 2 {
                    let func_val = self.eval_expr(&args[0])?;
                    let input = self.eval_expr(&args[1])?;
                    return self.eval_arrayfun(func_val, input);
                }
                if name == "parmap" && args.len() == 2 {
                    let func_val = self.eval_expr(&args[0])?;
                    let input = self.eval_expr(&args[1])?;
                    return self.eval_parmap(func_val, input);
                }
                if name == "rk4" && args.len() == 3 {
                    let func_val = self.eval_expr(&args[0])?;
                    let x0 = self.eval_expr(&args[1])?;
                    let t_val = self.eval_expr(&args[2])?;
                    return self.eval_rk4(func_val, x0, t_val);
                }
                if name == "feval" && !args.is_empty() {
                    let name_val = self.eval_expr(&args[0])?;
                    let fn_name = name_val.to_str().map_err(|_| {
                        ScriptError::runtime(
                            "feval: first argument must be a string function name".to_string(),
                        )
                    })?;
                    let rest: Vec<Value> = args[1..]
                        .iter()
                        .map(|a| self.eval_expr(a))
                        .collect::<Result<_, _>>()?;
                    return self.eval_feval(&fn_name, rest);
                }

                // If the name refers to a vector/matrix in the environment, this is indexing.
                if matches!(
                    self.env.get(name.as_str()),
                    Some(Value::Vector(_))
                        | Some(Value::Matrix(_))
                        | Some(Value::Tensor3(_))
                        | Some(Value::SparseVector(_))
                        | Some(Value::SparseMatrix(_))
                        | Some(Value::Tuple(_))
                        | Some(Value::Str(_))
                        | Some(Value::StringArray(_))
                ) {
                    let container = self.env[name.as_str()].clone();

                    // For 3-argument tensor3 indexing, bind `end` per-dimension.
                    let idx_vals: Vec<Value> = if args.len() == 3 {
                        if let Value::Tensor3(t) = &container {
                            let s = t.shape();
                            let (m, n, p) = (s[0], s[1], s[2]);
                            self.env.insert("end".to_string(), Value::Scalar(m as f64));
                            let iv = self.eval_expr(&args[0])?;
                            self.env.insert("end".to_string(), Value::Scalar(n as f64));
                            let jv = self.eval_expr(&args[1])?;
                            self.env.insert("end".to_string(), Value::Scalar(p as f64));
                            let kv = self.eval_expr(&args[2])?;
                            self.env.remove("end");
                            vec![iv, jv, kv]
                        } else {
                            let vals: Vec<Value> = args
                                .iter()
                                .map(|a| self.eval_expr(a))
                                .collect::<Result<_, _>>()?;
                            vals
                        }
                    }
                    // For 2-argument matrix indexing, bind `end` context-sensitively per dimension.
                    else if args.len() == 2 {
                        let (nrows, ncols) = match &container {
                            Value::Matrix(m) => (m.nrows(), m.ncols()),
                            Value::SparseMatrix(sm) => (sm.rows, sm.cols),
                            Value::Vector(v) => (1, v.len()),
                            Value::SparseVector(sv) => (1, sv.len),
                            _ => unreachable!(),
                        };
                        if nrows > 1
                            || matches!(&container, Value::SparseMatrix(_) | Value::Matrix(_))
                        {
                            self.env
                                .insert("end".to_string(), Value::Scalar(nrows as f64));
                            let row_val = self.eval_expr(&args[0])?;
                            self.env
                                .insert("end".to_string(), Value::Scalar(ncols as f64));
                            let col_val = self.eval_expr(&args[1])?;
                            self.env.remove("end");
                            vec![row_val, col_val]
                        } else {
                            self.env
                                .insert("end".to_string(), Value::Scalar(ncols as f64));
                            let vals: Vec<Value> = args
                                .iter()
                                .map(|a| self.eval_expr(a))
                                .collect::<Result<_, _>>()?;
                            self.env.remove("end");
                            vals
                        }
                    } else {
                        let len = match &container {
                            Value::Vector(v) => v.len(),
                            Value::Matrix(m) => m.nrows(),
                            Value::SparseVector(sv) => sv.len,
                            Value::SparseMatrix(sm) => sm.rows,
                            Value::Tuple(t) => t.len(),
                            Value::Str(s) => s.chars().count(),
                            Value::StringArray(v) => v.len(),
                            _ => unreachable!(),
                        };
                        self.env
                            .insert("end".to_string(), Value::Scalar(len as f64));
                        let vals: Vec<Value> = args
                            .iter()
                            .map(|a| self.eval_expr(a))
                            .collect::<Result<_, _>>()?;
                        self.env.remove("end");
                        vals
                    };

                    container
                        .index(idx_vals)
                        .map_err(|e| ScriptError::runtime(e))
                } else if let Some(func) = self.user_fns.get(name.as_str()).cloned() {
                    let vals: Vec<Value> = args
                        .iter()
                        .map(|a| self.eval_expr(a))
                        .collect::<Result<_, _>>()?;
                    self.eval_user_fn(func, vals)
                } else if let Some(env_val) = self.env.get(name.as_str()).cloned() {
                    // Lambda or FuncHandle stored in a variable, e.g. `f = @(x) x^2; f(3)`
                    match env_val {
                        Value::Lambda {
                            params,
                            body,
                            captured_env,
                        } => {
                            let arg_vals: Vec<Value> = args
                                .iter()
                                .map(|a| self.eval_expr(a))
                                .collect::<Result<_, _>>()?;
                            // Pass the variable name so profiler records it as "f", not "<lambda>"
                            self.eval_lambda_call(name, &params, &body, captured_env, arg_vals)
                        }
                        Value::FuncHandle(target) => self.eval_expr(&Expr::Call {
                            name: target,
                            args: args.clone(),
                        }),
                        _ => {
                            let vals: Vec<Value> = args
                                .iter()
                                .map(|a| self.eval_expr(a))
                                .collect::<Result<_, _>>()?;
                            self.call_builtin_tracked(name, vals)
                        }
                    }
                } else {
                    let vals: Vec<Value> = args
                        .iter()
                        .map(|a| self.eval_expr(a))
                        .collect::<Result<_, _>>()?;
                    self.call_builtin_tracked(name, vals)
                }
            }
            Expr::Matrix(rows) => {
                let evaled: Vec<Vec<Value>> = rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|e| self.eval_expr(e))
                            .collect::<Result<_, _>>()
                    })
                    .collect::<Result<_, _>>()?;
                Value::from_matrix_rows(evaled).map_err(|e| ScriptError::type_err(e))
            }
            Expr::CellArray(elems) => {
                let evaled: Vec<Value> = elems
                    .iter()
                    .map(|e| self.eval_expr(e))
                    .collect::<Result<_, _>>()?;
                Value::from_cell_elements(evaled).map_err(|e| ScriptError::type_err(e))
            }
            Expr::Range { start, step, stop } => {
                let s = self
                    .eval_expr(start)?
                    .to_scalar()
                    .map_err(|e| ScriptError::type_err(e))?;
                let e = self
                    .eval_expr(stop)?
                    .to_scalar()
                    .map_err(|e| ScriptError::type_err(e))?;
                let inc = match step {
                    Some(st) => self
                        .eval_expr(st)?
                        .to_scalar()
                        .map_err(|e| ScriptError::type_err(e))?,
                    None => 1.0,
                };
                if inc == 0.0 {
                    return Err(ScriptError::runtime(
                        "range step cannot be zero".to_string(),
                    ));
                }
                let mut vals: Vec<C64> = Vec::new();
                let mut cur = s;
                // Use a small epsilon to avoid float boundary issues
                let eps = inc.abs() * 1e-10;
                if inc > 0.0 {
                    while cur <= e + eps {
                        vals.push(Complex::new(cur, 0.0));
                        cur += inc;
                    }
                } else {
                    while cur >= e - eps {
                        vals.push(Complex::new(cur, 0.0));
                        cur += inc;
                    }
                }
                Ok(Value::Vector(Array1::from_vec(vals)))
            }
            Expr::Transpose(inner) => {
                let v = self.eval_expr(inner)?;
                v.transpose().map_err(|e| ScriptError::runtime(e))
            }
            Expr::NonConjTranspose(inner) => {
                let v = self.eval_expr(inner)?;
                v.non_conj_transpose().map_err(|e| ScriptError::runtime(e))
            }
            Expr::All => Ok(Value::All),
            Expr::Index { expr, args } => {
                let container = self.eval_expr(expr)?;
                // Bind `end` to length of the container for use inside index expressions
                let end_val = match &container {
                    Value::Vector(v) => v.len(),
                    Value::Matrix(m) => m.nrows(),
                    _ => 0,
                };
                self.env
                    .insert("end".to_string(), Value::Scalar(end_val as f64));
                let idx_vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.eval_expr(a))
                    .collect::<Result<_, _>>()?;
                self.env.remove("end");
                container
                    .index(idx_vals)
                    .map_err(|e| ScriptError::runtime(e))
            }
            Expr::Lambda { params, body } => Ok(Value::Lambda {
                params: params.clone(),
                body: body.clone(),
                captured_env: self.env.clone(),
            }),
            Expr::FuncHandle(name) => {
                // If the name is a lambda stored in env, capture it directly so it
                // remains callable when passed into a function's clean scope.
                if let Some(Value::Lambda { .. }) = self.env.get(name.as_str()) {
                    Ok(self.env[name.as_str()].clone())
                } else {
                    Ok(Value::FuncHandle(name.clone()))
                }
            }
            Expr::Field { object, field } => {
                let obj = self.eval_expr(object)?;
                match obj {
                    Value::Struct(fields) => fields.get(field.as_str()).cloned().ok_or_else(|| {
                        ScriptError::runtime(format!("struct has no field '{}'", field))
                    }),
                    Value::StateSpace { a, b, c, d } => match field.as_str() {
                        "A" => Ok(Value::Matrix(a)),
                        "B" => Ok(Value::Matrix(b)),
                        "C" => Ok(Value::Matrix(c)),
                        "D" => Ok(Value::Matrix(d)),
                        other => Err(ScriptError::runtime(format!(
                            "ss has no field '{}'; valid fields are A, B, C, D",
                            other
                        ))),
                    },
                    other => Err(ScriptError::runtime(format!(
                        "cannot access field '{}' on {}",
                        field,
                        other.type_name()
                    ))),
                }
            }
        }
    }

    /// Apply a callable (Lambda or FuncHandle) to each element of a vector or
    /// each row of a matrix.
    ///
    /// - All-scalar outputs → `Value::Vector`
    /// - All-vector outputs of equal length → `Value::Matrix` (one row per input element)
    /// - Mixed or inconsistent output shapes → runtime error
    fn eval_arrayfun(&mut self, func: Value, input: Value) -> Result<Value, ScriptError> {
        let tracking = self.profiler.should_track("arrayfun");
        let in_bytes: u64 = if tracking {
            Self::value_bytes(&input)
        } else {
            0
        };
        let t0 = if tracking {
            Some(std::time::Instant::now())
        } else {
            None
        };

        let result = self.eval_arrayfun_inner(func, input);

        if let (Some(t0), Ok(ref v)) = (t0, &result) {
            let ns = t0.elapsed().as_nanos() as u64;
            self.profiler
                .record("arrayfun", ns, in_bytes, Self::value_bytes(v));
        }
        result
    }

    fn eval_arrayfun_inner(&mut self, func: Value, input: Value) -> Result<Value, ScriptError> {
        let elements: Vec<Value> = match &input {
            Value::Vector(v) => v
                .iter()
                .map(|&c| {
                    if c.im == 0.0 {
                        Value::Scalar(c.re)
                    } else {
                        Value::Complex(c)
                    }
                })
                .collect(),
            Value::Scalar(n) => vec![Value::Scalar(*n)],
            Value::Complex(c) => vec![Value::Complex(*c)],
            other => {
                return Err(ScriptError::runtime(format!(
                    "arrayfun: second argument must be a vector or scalar, got {}",
                    other.type_name()
                )))
            }
        };

        let mut results: Vec<Value> = Vec::with_capacity(elements.len());
        for elem in elements {
            let out = self.call_callable(func.clone(), vec![elem])?;
            results.push(out);
        }

        // Determine output shape from first result
        match results.first() {
            None => Ok(Value::Vector(Array1::from_vec(vec![]))),
            Some(Value::Scalar(_)) | Some(Value::Complex(_)) => {
                // All must be scalar/complex → assemble into a vector
                let mut out = Vec::with_capacity(results.len());
                for (i, r) in results.into_iter().enumerate() {
                    match r {
                        Value::Scalar(n) => out.push(Complex::new(n, 0.0)),
                        Value::Complex(c) => out.push(c),
                        other => {
                            return Err(ScriptError::runtime(format!(
                                "arrayfun: element {} returned {}, expected scalar",
                                i + 1,
                                other.type_name()
                            )))
                        }
                    }
                }
                Ok(Value::Vector(Array1::from_vec(out)))
            }
            Some(Value::Vector(first_v)) => {
                // All must be vectors of the same length → assemble into a matrix (rows)
                let row_len = first_v.len();
                let nrows = results.len();
                let mut flat: Vec<C64> = Vec::with_capacity(nrows * row_len);
                for (i, r) in results.into_iter().enumerate() {
                    match r {
                        Value::Vector(v) => {
                            if v.len() != row_len {
                                return Err(ScriptError::runtime(format!(
                                    "arrayfun: element {} returned vector of length {}, expected {}",
                                    i + 1, v.len(), row_len
                                )));
                            }
                            flat.extend(v.iter().copied());
                        }
                        other => {
                            return Err(ScriptError::runtime(format!(
                                "arrayfun: element {} returned {}, expected vector",
                                i + 1,
                                other.type_name()
                            )))
                        }
                    }
                }
                let m = ndarray::Array2::from_shape_vec((nrows, row_len), flat)
                    .map_err(|e| ScriptError::runtime(e.to_string()))?;
                Ok(Value::Matrix(m))
            }
            Some(other) => Err(ScriptError::runtime(format!(
                "arrayfun: function returned unsupported type {}",
                other.type_name()
            ))),
        }
    }

    /// `parmap(f, xs)` — parallel map.
    ///
    /// Phase 2 of `dev/plans/parmap_parreduce.md`. Applies `f` to each
    /// element of `xs` in parallel via rayon, collects the results into
    /// a single vector. The user-facing surface is identical to
    /// `arrayfun(f, xs)` minus the matrix-output support; the difference
    /// is that `parmap` fans out across the rayon thread pool.
    ///
    /// Phase 2 ships the parallel orchestration only — purity of the
    /// lambda is the user's responsibility until Phase 3 layers on the
    /// per-task RNG seeding and the runtime pure-lambda contract.
    fn eval_parmap(&mut self, func: Value, input: Value) -> Result<Value, ScriptError> {
        use parmap::{LocalRayonBackend, ParmapBackend};

        // Validate the callable up front (clearer error than waiting for
        // the per-task error).
        parmap::validate_callable(&func)?;

        // Extract iterable elements. Reuses the same shape rules as
        // arrayfun: vectors, scalars, and complex scalars.
        let elements: Vec<Value> = match &input {
            Value::Vector(v) => v
                .iter()
                .map(|&c| {
                    if c.im == 0.0 {
                        Value::Scalar(c.re)
                    } else {
                        Value::Complex(c)
                    }
                })
                .collect(),
            Value::Scalar(n) => vec![Value::Scalar(*n)],
            Value::Complex(c) => vec![Value::Complex(*c)],
            other => {
                return Err(ScriptError::runtime(format!(
                    "parmap: second argument must be a vector or scalar, got {}",
                    other.type_name()
                )))
            }
        };

        // Profiler: track total parmap time; inner per-task calls are
        // suppressed (the lambda body's higher-order callback machinery
        // already handles that path).
        let tracking = self.profiler.should_track("parmap");
        let in_bytes: u64 = if tracking {
            Self::value_bytes(&input)
        } else {
            0
        };
        let t0 = if tracking {
            Some(std::time::Instant::now())
        } else {
            None
        };

        // Clone the Evaluator once as a template; each rayon task clones
        // again from the template (one Evaluator per task in v1; per-
        // thread caching is a follow-on).
        let template = self.clone_for_parallel_lambda();
        let backend = LocalRayonBackend::new();
        // Master seed: read this thread's current `seed(N)` value. If
        // the user has never called `seed()`, we pull a single u64 from
        // OS entropy as the base — that gives each task an independent
        // RNG stream without disturbing the master RNG. Subsequent
        // parmap calls without an intervening `seed()` will use a
        // *different* random base — that's the intended behaviour: the
        // user opted out of determinism by not calling seed().
        let master_seed = rng::current_master_seed().unwrap_or_else(|| {
            use rand::RngCore;
            // Independent OS-entropy draw; does NOT touch the master RNG.
            rand::rngs::OsRng.next_u64()
        });
        let raw = backend.run(
            &move || template.clone(),
            func,
            elements,
            master_seed,
        )?;
        let result = parmap::pack_results(raw);

        if let (Some(t0), Ok(ref v)) = (t0, &result) {
            let ns = t0.elapsed().as_nanos() as u64;
            self.profiler
                .record("parmap", ns, in_bytes, Self::value_bytes(v));
        }

        result
    }

    /// Fixed-step 4th-order Runge-Kutta integrator.
    /// `rk4(f, x0, t)` — f(x, t) returns x_dot; x0 is initial state; t is time vector.
    /// Returns an n×length(t) matrix where column k is the state at t[k].
    fn eval_rk4(&mut self, func: Value, x0: Value, t_val: Value) -> Result<Value, ScriptError> {
        use ndarray::Array2;
        use num_complex::Complex;

        let t_vec = t_val
            .to_cvector()
            .map_err(|e| ScriptError::runtime(format!("rk4: t must be a vector: {}", e)))?;
        let nt = t_vec.len();
        if nt < 2 {
            return Err(ScriptError::runtime(
                "rk4: t must have at least 2 points".to_string(),
            ));
        }

        // x0 can be a scalar, vector (column), or 1×1 matrix
        let state0: Vec<f64> = match &x0 {
            Value::Scalar(s) => vec![*s],
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            Value::Matrix(m) if m.ncols() == 1 => m.column(0).iter().map(|c| c.re).collect(),
            other => {
                return Err(ScriptError::runtime(format!(
                    "rk4: x0 must be a scalar or column vector, got {}",
                    other.type_name()
                )))
            }
        };
        let nx = state0.len();

        // Output: nx × nt matrix
        let mut result: Vec<Vec<f64>> = vec![vec![0.0; nt]; nx];
        for i in 0..nx {
            result[i][0] = state0[i];
        }

        // Helper: call f(x, t) and return x_dot as Vec<f64>
        let call_f = |ev: &mut Evaluator,
                      x_state: &[f64],
                      t_scalar: f64,
                      func: &Value|
         -> Result<Vec<f64>, ScriptError> {
            let x_arg = if nx == 1 {
                Value::Scalar(x_state[0])
            } else {
                // column vector as Matrix nx×1
                let col: ndarray::Array2<num_complex::Complex<f64>> =
                    Array2::from_shape_fn((nx, 1), |(i, _)| Complex::new(x_state[i], 0.0));
                Value::Matrix(col)
            };
            let t_arg = Value::Scalar(t_scalar);
            let out = ev.call_callable(func.clone(), vec![x_arg, t_arg])?;
            match out {
                Value::Scalar(s) => Ok(vec![s]),
                Value::Vector(v) => Ok(v.iter().map(|c| c.re).collect()),
                Value::Matrix(m) if m.ncols() == 1 => {
                    Ok(m.column(0).iter().map(|c| c.re).collect())
                }
                other => Err(ScriptError::runtime(format!(
                    "rk4: f must return a scalar or column vector, got {}",
                    other.type_name()
                ))),
            }
        };

        let mut x = state0.clone();
        for k in 0..(nt - 1) {
            let tk = t_vec[k].re;
            let tk1 = t_vec[k + 1].re;
            let h = tk1 - tk;

            let k1 = call_f(self, &x, tk, &func)?;
            let x2: Vec<f64> = x
                .iter()
                .zip(&k1)
                .map(|(xi, ki)| xi + 0.5 * h * ki)
                .collect();
            let k2 = call_f(self, &x2, tk + 0.5 * h, &func)?;
            let x3: Vec<f64> = x
                .iter()
                .zip(&k2)
                .map(|(xi, ki)| xi + 0.5 * h * ki)
                .collect();
            let k3 = call_f(self, &x3, tk + 0.5 * h, &func)?;
            let x4: Vec<f64> = x.iter().zip(&k3).map(|(xi, ki)| xi + h * ki).collect();
            let k4 = call_f(self, &x4, tk1, &func)?;

            for i in 0..nx {
                x[i] += h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
                result[i][k + 1] = x[i];
            }
        }

        // Build nx×nt matrix (row i = state component i over time)
        let mut out_mat: ndarray::Array2<num_complex::Complex<f64>> = Array2::zeros((nx, nt));
        for i in 0..nx {
            for k in 0..nt {
                out_mat[[i, k]] = Complex::new(result[i][k], 0.0);
            }
        }
        if nx == 1 {
            // 1-state system: return as a plain vector for convenience
            Ok(Value::Vector(out_mat.row(0).to_owned()))
        } else {
            Ok(Value::Matrix(out_mat))
        }
    }

    /// Call a function by string name — dispatches to user_fns, env lambdas, then builtins.
    fn eval_feval(&mut self, name: &str, args: Vec<Value>) -> Result<Value, ScriptError> {
        if let Some(func) = self.user_fns.get(name).cloned() {
            return self.eval_user_fn(func, args);
        }
        if let Some(env_val) = self.env.get(name).cloned() {
            if let Value::Lambda {
                params,
                body,
                captured_env,
            } = env_val
            {
                return self.eval_lambda_call(name, &params, &body, captured_env, args);
            }
        }
        self.call_builtin_tracked(name, args)
    }

    /// Invoke any callable value (Lambda or FuncHandle) with the given args.
    /// Used by `eval_arrayfun` — inner calls are not tracked individually (outer captures total).
    pub(crate) fn call_callable(
        &mut self,
        func: Value,
        args: Vec<Value>,
    ) -> Result<Value, ScriptError> {
        match func {
            Value::Lambda {
                params,
                body,
                captured_env,
            } => {
                // Empty call_name suppresses per-call profiling; outer arrayfun captures total time.
                self.eval_lambda_call("", &params, &body, captured_env, args)
            }
            Value::FuncHandle(name) => {
                // Suppress inner tracking — outer (arrayfun) captures total time.
                self.profiler.enter_higher_order();
                let result = self.eval_feval(&name, args);
                self.profiler.exit_higher_order();
                result
            }
            other => Err(ScriptError::runtime(format!(
                "arrayfun: first argument must be a lambda or function handle, got {}",
                other.type_name()
            ))),
        }
    }

    /// Call a lambda value with its captured environment.
    ///
    /// `call_name` is the variable name at the call site (e.g. `"f"` for `f(3)`).
    /// Pass `""` when invoking as a callback (arrayfun inner calls) — profiling is suppressed
    /// and the outer higher-order function's time captures the total cost instead.
    fn eval_lambda_call(
        &mut self,
        call_name: &str,
        params: &[String],
        body: &Expr,
        captured_env: HashMap<String, Value>,
        args: Vec<Value>,
    ) -> Result<Value, ScriptError> {
        if args.len() != params.len() {
            return Err(ScriptError::runtime(format!(
                "lambda expects {} argument(s), got {}",
                params.len(),
                args.len()
            )));
        }

        // Profiling: check before entering scope (while higher_order_depth is still outer value)
        let tracking = !call_name.is_empty() && self.profiler.should_track(call_name);
        let in_bytes: u64 = if tracking {
            args.iter().map(Self::value_bytes).sum()
        } else {
            0
        };
        let t0 = if tracking {
            Some(std::time::Instant::now())
        } else {
            None
        };

        // Save outer env; install captured env + parameter bindings
        let saved_env = std::mem::replace(&mut self.env, captured_env);
        let saved_in_fn = self.in_function;
        self.in_function = true;
        self.profiler.enter_higher_order(); // suppress inner function call recording
        for (pname, val) in params.iter().zip(args) {
            self.env.insert(pname.clone(), val);
        }
        let result = self.eval_expr(body);
        // Restore outer env
        self.env = saved_env;
        self.in_function = saved_in_fn;
        self.profiler.exit_higher_order();

        if let (true, Some(t0), Ok(ref v)) = (tracking, t0, &result) {
            let ns = t0.elapsed().as_nanos() as u64;
            self.profiler
                .record(call_name, ns, in_bytes, Self::value_bytes(v));
        }
        result
    }

    /// Call a user-defined function with scope isolation.
    /// Single-output convenience wrapper — defers to the nargout-aware
    /// path with `nargout=1` so a bare `p = userfn(x)` always picks the
    /// first declared output.
    fn eval_user_fn(&mut self, func: UserFn, args: Vec<Value>) -> Result<Value, ScriptError> {
        self.eval_user_fn_nargout(func, args, 1)
    }

    /// Call a user-defined function with an explicit `nargout` request.
    /// `nargout` is the number of values the caller wants:
    ///   - `0` — caller is using the call as a statement; return `Value::None`.
    ///   - `1` — back-compat: return a single `Value` (not a `Tuple`).
    ///   - `n >= 2` — return a `Value::Tuple` of the first `n` declared outputs.
    /// Multi-output errors when `nargout` exceeds declared outputs or when
    /// any picked output variable was never assigned in the body.
    fn eval_user_fn_nargout(
        &mut self,
        func: UserFn,
        args: Vec<Value>,
        nargout: usize,
    ) -> Result<Value, ScriptError> {
        if args.len() != func.params.len() {
            return Err(ScriptError::runtime(format!(
                "function expects {} argument(s), got {}",
                func.params.len(),
                args.len()
            )));
        }

        // Profiling: check before entering scope
        let tracking = self.profiler.should_track(&func.name);
        let in_bytes: u64 = if tracking {
            args.iter().map(Self::value_bytes).sum()
        } else {
            0
        };
        let t0 = if tracking {
            Some(std::time::Instant::now())
        } else {
            None
        };

        // Save outer env and function flag
        let saved_env = std::mem::take(&mut self.env);
        let saved_in_fn = self.in_function;
        self.in_function = true;
        self.profiler.enter_higher_order(); // suppress inner call recordings
                                            // Seed with built-in constants
        for name in &["i", "j", "pi", "e", "Inf", "NaN"] {
            if let Some(v) = saved_env.get(*name) {
                self.env.insert((*name).to_string(), v.clone());
            }
        }
        // Bind parameters
        for (param, val) in func.params.iter().zip(args) {
            self.env.insert(param.clone(), val);
        }
        // Run body — EarlyReturn is not an error, just early exit
        let mut body_err: Option<ScriptError> = None;
        match self.run(&func.body) {
            Err(ScriptError::EarlyReturn) => {} // normal early return
            Err(e) => {
                body_err = Some(e);
            }
            Ok(()) => {}
        }
        // Build the return based on declared outputs and the caller's nargout.
        // - 0 declared OR nargout=0 → None
        // - 1 picked → single Value (back-compat: never a Tuple)
        // - n >= 2 picked → Tuple of the first n declared outputs
        // For the classic single-output case (1 declared, body never assigned
        // it) we fall back to None to match prior behaviour. Multi-output
        // missing-assignment is loud — matlab errors and so do we.
        let declared = func.return_vars.len();
        let ret_val = if declared == 0 || nargout == 0 {
            Value::None
        } else if nargout > declared {
            // Restore env before erroring so subsequent calls don't see leftover state.
            self.env = saved_env;
            self.in_function = saved_in_fn;
            self.profiler.exit_higher_order();
            return Err(ScriptError::runtime(format!(
                "function '{}' declares {} output(s), but caller asked for {}",
                func.name, declared, nargout
            )));
        } else if nargout == 1 {
            // Back-compat path: 1-output single value, missing assignment → None.
            self.env
                .get(func.return_vars[0].as_str())
                .cloned()
                .unwrap_or(Value::None)
        } else {
            let mut vals: Vec<Value> = Vec::with_capacity(nargout);
            for ret in func.return_vars.iter().take(nargout) {
                let v = self.env.get(ret.as_str()).cloned().ok_or_else(|| {
                    ScriptError::runtime(format!(
                        "function '{}': output '{}' was not assigned in the body",
                        func.name, ret
                    ))
                })?;
                vals.push(v);
            }
            Value::Tuple(vals)
        };
        // Restore outer env and function flag
        self.env = saved_env;
        self.in_function = saved_in_fn;
        self.profiler.exit_higher_order();

        // Record if tracking and no error
        if let (true, Some(t0), None) = (tracking, t0, &body_err) {
            let ns = t0.elapsed().as_nanos() as u64;
            self.profiler
                .record(&func.name, ns, in_bytes, Self::value_bytes(&ret_val));
        }

        if let Some(e) = body_err {
            return Err(e);
        }
        Ok(ret_val)
    }

    /// Call a builtin, recording timing and IO bytes if profiling is active for this name.
    fn call_builtin_tracked(&mut self, name: &str, vals: Vec<Value>) -> Result<Value, ScriptError> {
        self.call_builtin_tracked_nargout(name, vals, 1)
    }

    /// Same as [`call_builtin_tracked`] but with the matlab-style `nargout`
    /// hint forwarded to nargout-aware builtins (`eig`, `sort`, `find`, …).
    fn call_builtin_tracked_nargout(
        &mut self,
        name: &str,
        vals: Vec<Value>,
        nargout: usize,
    ) -> Result<Value, ScriptError> {
        // Pure-lambda contract: when running inside a `parmap` worker
        // task, impure builtins (plotting, file I/O, audio, FIR
        // streaming, live figures) hard-error with a clear message.
        // See `eval/parmap.rs` for the contract and the IMPURE_BUILTINS
        // list this guards against.
        if IMPURE_BUILTINS.contains(&name) {
            parmap::require_pure_context(name)?;
        }

        if !self.profiler.should_track(name) {
            return self.builtins.call_with_nargout(name, vals, nargout);
        }
        let in_bytes: u64 = vals.iter().map(Self::value_bytes).sum();
        let t0 = std::time::Instant::now();
        let result = self.builtins.call_with_nargout(name, vals, nargout);
        let ns = t0.elapsed().as_nanos() as u64;
        if let Ok(ref v) = result {
            self.profiler
                .record(name, ns, in_bytes, Self::value_bytes(v));
        }
        result
    }

    /// Clone the Evaluator for use as a per-worker copy under `parmap`.
    /// Phase 2 of `dev/plans/parmap_parreduce.md` — each rayon task gets a
    /// fresh Evaluator carrying the user-defined functions and the global
    /// env, so user-fn calls + captured-env reads inside a parallel lambda
    /// work identically to the sequential case.
    ///
    /// Currently a thin wrapper around `Clone::clone`. Future refinements
    /// could trim per-worker state (e.g. drop the profiler, clear `env`
    /// since lambdas install their own captured env at call time) but the
    /// trivial clone is correct and fast enough for v1.
    pub(crate) fn clone_for_parallel_lambda(&self) -> Evaluator {
        self.clone()
    }

    /// Approximate byte size of a Value for IO throughput accounting.
    /// Only numeric types are counted; strings, structs, etc. return 0.
    fn value_bytes(v: &Value) -> u64 {
        match v {
            Value::Scalar(_) => 8,
            Value::Complex(_) => 16,
            Value::Vector(v) => (v.len() * 16) as u64,
            Value::Matrix(m) => (m.nrows() * m.ncols() * 16) as u64,
            _ => 0,
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Builtins that mutate global / process-level state and therefore cannot
/// run safely inside a `parmap` parallel lambda. The dispatch in
/// [`Evaluator::call_builtin_tracked_nargout`] checks this list before
/// invoking any builtin; if `parmap` has installed its parallel-context
/// flag on the current worker thread, the call hard-errors via
/// [`parmap::require_pure_context`].
///
/// Categories:
/// - **Plotting / figure state**: every `plot`-family builtin mutates the
///   per-thread `FIGURE` singleton, so they're banned under parmap.
/// - **File I/O**: `fprintf`, `savefig`, `saveanim`, `frame` etc. write to
///   disk in undefined order if invoked from parallel tasks.
/// - **Live figures / external viewer**: `figure_live` and its update
///   helpers share state across calls and across threads.
/// - **RNG control**: `seed` would override per-task seeds installed by
///   parmap and break the determinism contract; banned.
///
/// Keep this list sorted alphabetically for easy maintenance.
const IMPURE_BUILTINS: &[&str] = &[
    "clf",
    "contour",
    "contourf",
    "figure",
    "figure_close",
    "figure_draw",
    "figure_live",
    "fprintf",
    "frame",
    "grid",
    "hold",
    "imagesc",
    "legend",
    "loglog",
    "plot",
    "plot_labels",
    "plot_limits",
    "plot_update",
    "plotdb",
    "polar",
    "quiver",
    "saveanim",
    "savefig",
    "seed",
    "semilogx",
    "semilogy",
    "stem",
    "streamplot",
    "surf",
    "title",
    "xlabel",
    "ylabel",
    "zlabel",
];

// Compile-time assertion that Evaluator and Value are Send. The parallel-
// map (`parmap`) implementation in `dev/plans/parmap_parreduce.md` Phase 2
// depends on this: rayon workers carry per-thread `Evaluator` clones, and
// captured-env / argument / result `Value`s cross thread boundaries.
//
// If a future Value variant adds a non-Send type (e.g., `Cell`, `Rc`, a
// raw pointer), this compile assertion will fail and the parmap plan
// needs to either fix the offending variant or partition Value into a
// Send subset. Catching it here is much better than hitting it during
// Phase 2 wiring.
#[allow(dead_code)]
fn _assert_send<T: Send>() {}
#[allow(dead_code)]
fn _assert_sync<T: Sync>() {}
#[allow(dead_code)]
fn _assert_evaluator_and_value_are_send_sync() {
    _assert_send::<Evaluator>();
    _assert_send::<Value>();
    _assert_sync::<Evaluator>();
    _assert_sync::<Value>();
}

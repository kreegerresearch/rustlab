pub mod builtins;
mod xcorr;
pub(crate) mod elementwise;
pub(crate) mod interp1;
pub(crate) mod linsys;
pub(crate) mod real_index;
pub(crate) mod real_ops;
pub mod line_io;
pub mod output;
pub mod table;
pub mod parmap;
pub mod profile;
pub mod rng;
pub mod fast_hash;
pub mod shared;
pub mod sparam_analysis;
pub mod sparam_conv;
pub mod toml_io;
pub mod touchstone;
pub mod value;

use crate::ast::{BinOp, CacheStmt, Expr, Stmt, StmtKind};
use crate::cache_registry::CacheRegistry;
use crate::error::ScriptError;
use shared::Shared;
pub use builtins::BuiltinRegistry;
use ndarray::{Array1, Array2};
use num_complex::Complex;
pub use profile::FnStats;
use rustlab_core::C64;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
pub use value::NumberFormat;
pub use value::Value;
pub use value::WidgetValue;

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
    env: fast_hash::FxHashMap<String, Value>,
    builtins: BuiltinRegistry,
    user_fns: HashMap<String, UserFn>,
    /// True while executing a user-defined function body — suppresses auto-print of assignments.
    in_function: bool,
    /// One-shot marker armed by [`eval_expr_stmt`](Self::eval_expr_stmt)
    /// for a statement-position call and consumed at the `Expr::Call`
    /// arm's entry: the outermost call of a bare expression statement
    /// dispatches builtins with `nargout = 0` (silent `figure()` /
    /// `histogram(v)`); everything nested runs with the usual 1.
    stmt_call_nargout0: bool,
    profiler: profile::Profiler,
    /// When true, assignment output uses ANSI colour (green var name, dim `=`).
    pub color_output: bool,
    /// Active numeric display format (short, long, hex, commas).
    pub number_format: value::NumberFormat,
    /// Source line of the statement currently being executed (for error messages).
    current_line: usize,
    /// Persistent function-result cache state. Phase 3b: data only;
    /// the StmtKind::Cache arm and the dispatcher (Phase 3c/d) operate
    /// on this. Defaults to inactive — `cache enable` opens a store.
    pub(crate) cache_registry: CacheRegistry,
    /// Memoized canonical entry ids — Phase 7 (Option 3) rename-
    /// invariant function identity hashes. Populated lazily by the
    /// dispatcher; cleared en bloc on every `FunctionDef` so a
    /// redefinition (or a sibling redefinition that transitively
    /// affects a caller) forces re-computation. The keys here are
    /// user-fn names; the values are the canonical 32-byte ids.
    cached_entry_ids: HashMap<String, [u8; 32]>,
    /// Optional cooperative-cancellation flag. When `Some` and set to
    /// `true`, the evaluator returns [`ScriptError::Cancelled`] at the
    /// next statement / loop-iteration boundary. `None` (the default)
    /// means execution never cancels — so the REPL and one-shot CLI are
    /// unaffected; only the notebook watch server's preemptible renders
    /// install a flag (via [`Evaluator::with_cancel`]).
    cancel: Option<Arc<AtomicBool>>,
    /// Current notebook widget values, read by the `widget(name)` builtin.
    /// `None` outside a notebook (REPL / one-shot CLI) — `widget()` then
    /// errors. The notebook executor installs a table (via
    /// [`Evaluator::with_widgets`]) seeded with each widget's declared
    /// default; the interactive server overlays live values on top. So
    /// batch `notebook render` resolves `widget()` to declared defaults
    /// while the watch server resolves it to the live slider position.
    /// `Arc` so snapshots in the prefix cache share the table cheaply,
    /// exactly like `cancel`.
    widget_values: Option<Arc<BTreeMap<String, WidgetValue>>>,
    /// Names of widgets read via `widget(name)` since the last
    /// [`Evaluator::take_widget_reads`]. The notebook executor clears this
    /// before each code block and harvests it after, so it learns which
    /// widgets a block depends on — the basis for scoped re-render (only
    /// re-run blocks whose widget read-set changed). Transient per block;
    /// not part of cached state.
    widget_reads: BTreeSet<String>,
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
            stmt_call_nargout0: self.stmt_call_nargout0,
            profiler: self.profiler.clone(),
            color_output: self.color_output,
            number_format: self.number_format,
            current_line: self.current_line,
            // Cache registry: the store is `Arc<Store>`, so the
            // clone shares the SQLite connection. Scope sets and
            // counters are snapshotted with the evaluator — the
            // notebook prefix cache relies on this to roll back
            // cache state consistently with the rest of the eval
            // session.
            cache_registry: self.cache_registry.clone(),
            cached_entry_ids: self.cached_entry_ids.clone(),
            // Share the same cancel flag — a snapshot restored mid-render
            // should still observe a preemption signal.
            cancel: self.cancel.clone(),
            // Widget table is immutable per render; an `Arc` clone shares it.
            widget_values: self.widget_values.clone(),
            // Read-set is transient per block; snapshots needn't carry it,
            // but cloning keeps `deep_clone` a faithful copy.
            widget_reads: self.widget_reads.clone(),
        }
    }

    pub fn new() -> Self {
        let mut env = fast_hash::FxHashMap::default();
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
        env.insert("stdin".to_string(), Value::Stdin);

        Self {
            env,
            builtins: BuiltinRegistry::with_defaults(),
            user_fns: HashMap::new(),
            in_function: false,
            stmt_call_nargout0: false,
            profiler: profile::Profiler::default(),
            color_output: false,
            number_format: value::default_number_format(),
            current_line: 0,
            cache_registry: CacheRegistry::new(),
            cached_entry_ids: HashMap::new(),
            cancel: None,
            widget_values: None,
            widget_reads: BTreeSet::new(),
        }
    }

    /// Install a cooperative cancellation flag (builder form). While the
    /// flag reads `true`, execution returns [`ScriptError::Cancelled`] at
    /// the next statement or loop-iteration boundary. Long-running
    /// *builtins* are not interruptible (they're bounded); this targets
    /// runaway script-level loops. Default-off: a fresh `Evaluator` has
    /// no flag and never cancels.
    pub fn with_cancel(mut self, flag: Arc<AtomicBool>) -> Self {
        self.cancel = Some(flag);
        self
    }

    /// Install (or replace) the cooperative cancellation flag in place.
    pub fn set_cancel(&mut self, flag: Arc<AtomicBool>) {
        self.cancel = Some(flag);
    }

    /// Install the notebook widget value table (builder form). After this,
    /// `widget(name)` resolves against `table`. The notebook executor seeds
    /// `table` with each widget's declared default; the interactive server
    /// overlays live slider/option values before calling. A fresh
    /// `Evaluator` has no table and `widget()` errors — that's the REPL /
    /// one-shot CLI case where widgets are meaningless.
    pub fn with_widgets(mut self, table: Arc<BTreeMap<String, WidgetValue>>) -> Self {
        self.widget_values = Some(table);
        self
    }

    /// Install (or replace) the widget value table in place.
    pub fn set_widgets(&mut self, table: Arc<BTreeMap<String, WidgetValue>>) {
        self.widget_values = Some(table);
    }

    /// Take and clear the set of widget names read since the last call.
    /// The notebook executor calls this around each code block to record
    /// that block's widget read-set for scoped re-render.
    pub fn take_widget_reads(&mut self) -> BTreeSet<String> {
        std::mem::take(&mut self.widget_reads)
    }

    /// Return `Err(Cancelled)` if a cancel flag is installed and set.
    /// A plot abort (Ctrl+C during a viewer send or a terminal figure)
    /// is the same kind of stop: [`ScriptError::Interrupted`], which the
    /// CLI treats as a clean exit.
    #[inline]
    fn check_cancelled(&self) -> Result<(), ScriptError> {
        if rustlab_plot::take_plot_interrupt() {
            return Err(ScriptError::Interrupted);
        }
        if let Some(flag) = &self.cancel {
            if flag.load(Ordering::Relaxed) {
                return Err(ScriptError::Cancelled);
            }
        }
        Ok(())
    }

    /// Look up a variable in the environment (used by tests).
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.env.get(name)
    }

    // ── Cache-state accessors (used by tests + future REPL `cache status`) ──

    /// `true` when the cache has an open store.
    pub fn cache_active(&self) -> bool {
        self.cache_registry.is_active()
    }

    /// `true` iff a call to a user fn named `name` would be routed
    /// through the cache. Resolution rules in
    /// [`crate::cache_registry::CacheRegistry::is_in_scope`].
    pub fn is_fn_cache_scoped(&self, name: &str) -> bool {
        self.cache_registry.is_in_scope(name)
    }

    /// `true` iff a user function with this name has been defined in
    /// the evaluator (inline, via `run`, or via `cache add file`).
    pub fn is_user_fn_defined(&self, name: &str) -> bool {
        self.user_fns.contains_key(name)
    }

    /// Snapshot of the per-process cache counters surfaced by
    /// `cache status`.
    pub fn cache_counters(&self) -> crate::cache_registry::CacheCounters {
        self.cache_registry.counters().clone()
    }

    /// Set a variable in the environment.
    pub fn set(&mut self, name: &str, value: Value) {
        self.set_env(name, value);
    }

    /// Remove a variable from the environment.
    pub fn remove(&mut self, name: &str) {
        self.env.remove(name);
    }

    /// Remove all user-defined variables and functions, keeping built-in constants (j, pi, e).
    pub fn clear_vars(&mut self) {
        self.env
            .retain(|k, _| line_io::BUILTIN_CONSTS.contains(&k.as_str()));
        self.user_fns.clear();
        // `stdin` is a handle, not a number a script is likely to
        // overwrite on purpose. Put the handle back so `fgetl(stdin)`
        // still works after `clear` or after `stdin = 1`.
        self.env.insert("stdin".to_string(), Value::Stdin);
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
        let mut entries: Vec<(&str, &Value)> = self
            .env
            .iter()
            .filter(|(k, _)| !line_io::BUILTIN_CONSTS.contains(&k.as_str()))
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

    /// Also record operator and indexing rows (`[+]`, `[.*]`, `[index]`, …)
    /// while profiling. `[matmul]` is recorded without this.
    pub fn enable_op_profiling(&mut self) {
        self.profiler.enable_ops();
    }

    /// Drain this evaluator's rows without the `[top-level]` row (a
    /// `parmap` worker's per-task stats).
    pub(crate) fn take_profile_stats(&mut self) -> HashMap<String, FnStats> {
        self.profiler.take_stats()
    }

    /// Print the profiling report to stderr if profiling is on and recorded
    /// anything. Called when a script finishes, so an in-script `profile()`
    /// reports at exit without an explicit `profile_report()`.
    pub fn print_profile_report(&mut self) {
        if self.profiler.is_enabled() {
            let rows = self.profiler.take_report();
            profile::print_report(&rows);
        }
    }

    /// Run statements and auto-print any profiling report to stderr at the end.
    /// Use this instead of `run` for top-level script execution.
    pub fn run_script(&mut self, stmts: &[Stmt]) -> Result<(), ScriptError> {
        let result = self.run(stmts);
        self.print_profile_report();
        result
    }

    pub fn run(&mut self, stmts: &[Stmt]) -> Result<(), ScriptError> {
        for stmt in stmts {
            self.exec_stmt(stmt)?;
        }
        Ok(())
    }

    pub fn exec_stmt(&mut self, stmt: &Stmt) -> Result<(), ScriptError> {
        // Cooperative cancellation: bail before each statement. Combined
        // with the per-iteration checks in the While/For arms, this makes
        // even an empty-body infinite loop (`while true; end`)
        // interruptible. No-op when no cancel flag is installed.
        self.check_cancelled()?;
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

    /// `name(indices) = val` — the body of `StmtKind::IndexAssign` after the
    /// right-hand side is evaluated (split out so the profiler can time it
    /// as one `[index-assign]` row).
    fn exec_index_assign(
        &mut self,
        name: &String,
        indices: &Vec<Expr>,
        val: Value,
        suppress: &bool,
    ) -> Result<(), ScriptError> {

        if matches!(self.env.get(name.as_str()), Some(Value::Table(_))) {
            return Err(ScriptError::runtime(
                "table index assignment is not supported (T(rows, vars) = ...). \
                 Assign a whole column with T.name = value"
                    .to_string(),
            ));
        }

        // Octave/matlab `[]` deletion: when the right-hand side is an
        // empty vector or matrix, the assignment removes the indexed
        // elements from the container instead of writing into them.
        // Vector form `v(idx) = []` is supported; matrix row/column
        // deletion (`M(i, :) = []`) is a follow-up.
        let rhs_is_empty = match &val {
            Value::Vector(v) => Shared::len(v) == 0,
            Value::Matrix(m) => Shared::len(m) == 0,
            _ => false,
        };
        if rhs_is_empty {
            return self.exec_index_delete(name, indices, *suppress);
        }

        // Evaluate indices with `end` bound the same way reads do.
        // Tensor3 with 3 indices, and a matrix/sparse with 2 indices,
        // bind `end` per axis. A single index binds `end` to the
        // element count. Anything else (no container yet) binds 0.
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
            self.set_env("end", Value::Scalar(m as f64));
            let iv = self.eval_expr(&indices[0])?;
            self.set_env("end", Value::Scalar(n as f64));
            let jv = self.eval_expr(&indices[1])?;
            self.set_env("end", Value::Scalar(p as f64));
            let kv = self.eval_expr(&indices[2])?;
            self.env.remove("end");
            vec![iv, jv, kv]
        } else if indices.len() == 2 {
            let dims = match self.env.get(name.as_str()) {
                Some(Value::Matrix(m)) => {
                    let s = Shared::shape(m);
                    Some((s[0], s[1]))
                }
                Some(Value::SparseMatrix(sm)) => Some((sm.rows, sm.cols)),
                _ => None,
            };
            if let Some((nrows, ncols)) = dims {
                self.env
                    .insert("end".to_string(), Value::Scalar(nrows as f64));
                let row_val = self.eval_expr(&indices[0])?;
                self.env
                    .insert("end".to_string(), Value::Scalar(ncols as f64));
                let col_val = self.eval_expr(&indices[1])?;
                self.env.remove("end");
                vec![row_val, col_val]
            } else {
                let container_len = match self.env.get(name.as_str()) {
                    Some(Value::Vector(v)) => Shared::len(v),
                    Some(Value::SparseVector(sv)) => sv.len,
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
            }
        } else {
            let container_len = match self.env.get(name.as_str()) {
                Some(Value::Vector(v)) => Shared::len(v),
                Some(Value::Matrix(m)) if indices.len() == 1 => Shared::len(m),
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

        if (*suppress || !self.echo_enabled())
            && self.try_real_index_assign(name, &idx_vals, &val).is_some()
        {
            return Ok(());
        }

        if idx_vals.len() == 1 {
            // `g(idx) = c` where `g` is a matrix (including the row or
            // column that `zeros(size(v))` builds) and `idx` is `:`,
            // a numeric index vector, or a logical mask. A scalar
            // index still uses the paths below, so `M(i) = row`
            // is unchanged.
            if matches!(self.env.get(name.as_str()), Some(Value::Matrix(_)))
                && matches!(
                    &idx_vals[0],
                    Value::All | Value::Vector(_) | Value::Matrix(_)
                )
            {
                return self.assign_matrix_linear(name, &idx_vals[0], &val, *suppress);
            }
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
                        let v = Shared::make_mut(v);
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
                        let m = Shared::make_mut(m);
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
                        let m = Shared::make_mut(m);
                        let nr = m.nrows();
                        let nc = m.ncols();
                        if nr == 1 {
                            if idx > nc {
                                let mut new_m = Array2::zeros((1, idx));
                                for c in 0..nc {
                                    new_m[[0, c]] = m[[0, c]];
                                }
                                *m = new_m.into();
                            }
                            m[[0, idx - 1]] = assign_val;
                        } else if nc == 1 {
                            if idx > nr {
                                let mut new_m = Array2::zeros((idx, 1));
                                for r in 0..nr {
                                    new_m[[r, 0]] = m[[r, 0]];
                                }
                                *m = new_m.into();
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
                            *v = Array1::from_vec(new_vec).into();
                        }
                        v
                    }
                    _ => {
                        // Create new vector of length idx, filled with zeros
                        let new_vec = vec![Complex::new(0.0, 0.0); idx];
                        self.env
                            .insert(name.clone(), Value::Vector(Array1::from_vec(new_vec).into()));
                        match self.env.get_mut(name.as_str()) {
                            Some(Value::Vector(v)) => v,
                            _ => unreachable!(),
                        }
                    }
                };
                Shared::make_mut(vec)[idx - 1] = assign_val;
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
                        let m = Shared::make_mut(m);
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
                    Some(Value::Matrix(m)) => {
                        let s = Shared::shape(m);
                        (s[0], s[1])
                    }
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
                if self.region_write_runs(name, &rows, &cols, &val) {
                    if !suppress && self.echo_enabled() {
                        output::script_println(&format!(
                            "{}(...) = (region {}×{} updated)",
                            name,
                            rows.len(),
                            cols.len()
                        ));
                    }
                    return Ok(());
                }
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
                        let m = Shared::make_mut(m);
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
        Ok(())
    }

    /// Bind `name` to `v`, overwriting an existing binding in place so a
    /// reassignment (a loop variable, `acc = acc + k`, the `end` binding)
    /// does not allocate a new key.
    #[inline]
    fn set_env(&mut self, name: &str, v: Value) {
        match self.env.get_mut(name) {
            Some(slot) => *slot = v,
            None => {
                self.env.insert(name.to_owned(), v);
            }
        }
    }

    /// Index writes that keep a real-stored target real: the target is a
    /// real-stored matrix or vector and the right-hand side is a real
    /// scalar or a real-stored array. Covers `M(rows, cols) = …` (any index
    /// forms, the right-hand side shapes of the general path), `M(idx) = …`
    /// (linear, column-major, in bounds) and `v(idx) = …` (in bounds).
    /// Returns `None` without writing for everything else, including
    /// out-of-range indices and growth, so the complex paths below keep
    /// their behavior and error messages. The caller skips it when the
    /// assignment echoes.
    fn try_real_index_assign(&mut self, name: &str, idx: &[Value], val: &Value) -> Option<()> {
        #[derive(Clone, Copy)]
        enum Src<'a> {
            Fill(f64),
            Data(&'a [f64]),
            /// Complex-stored, every imaginary part exactly `+0.0`: the
            /// real parts are the values, so writing them leaves the
            /// target real with nothing observable changed.
            Re(&'a [C64]),
        }
        impl Src<'_> {
            #[inline]
            fn at(self, k: usize) -> f64 {
                match self {
                    Src::Fill(x) => x,
                    Src::Data(d) => d[k],
                    Src::Re(d) => d[k].re,
                }
            }
            fn len(self) -> usize {
                match self {
                    Src::Fill(_) => 0,
                    Src::Data(d) => d.len(),
                    Src::Re(d) => d.len(),
                }
            }
        }
        fn zero_im(d: &[C64]) -> bool {
            d.iter().all(|z| z.im.to_bits() == 0)
        }
        let src = match val {
            Value::Scalar(x) => Src::Fill(*x),
            Value::Vector(v) => match Shared::real_slice(v) {
                Some(d) => Src::Data(d),
                None if !Shared::is_real(v) => Src::Re(v.as_slice().filter(|d| zero_im(d))?),
                None => return None,
            },
            Value::Matrix(m) => match Shared::real_slice(m) {
                Some(d) => Src::Data(d),
                None if !Shared::is_real(m) => Src::Re(m.as_slice().filter(|d| zero_im(d))?),
                None => return None,
            },
            _ => return None,
        };
        let target = self.env.get(name)?;
        let real_target = match target {
            Value::Matrix(m) => Shared::is_real(m),
            Value::Vector(v) => Shared::is_real(v),
            _ => false,
        };
        if !real_target {
            return None;
        }
        match (target, idx) {
            (Value::Matrix(m), [ri, ci]) => {
                let s = Shared::shape(m);
                let (nr, nc) = (s[0], s[1]);
                let rows = Value::resolve_index_dim(ri, nr).ok()?;
                let cols = Value::resolve_index_dim(ci, nc).ok()?;
                let n = rows.len() * cols.len();
                // The right-hand side shapes the general path accepts.
                match (src, val) {
                    (Src::Fill(_), _) => {}
                    (_, Value::Vector(_)) => {
                        let len = src.len();
                        let ok = (rows.len() == 1 && len == cols.len())
                            || (cols.len() == 1 && len == rows.len())
                            || len == n;
                        if !ok {
                            return None;
                        }
                    }
                    (_, Value::Matrix(rhs)) => {
                        if Shared::shape(rhs) != [rows.len(), cols.len()] {
                            return None;
                        }
                    }
                    _ => return None,
                }
                let Some(Value::Matrix(m)) = self.env.get_mut(name) else { unreachable!() };
                let dst = Shared::make_mut_real(m)?.as_slice_mut()?;
                let run = Value::run_of(&cols);
                let mut k = 0;
                for &r in &rows {
                    let row = &mut dst[r * nc..(r + 1) * nc];
                    match (run, src) {
                        (Some((c0, w)), Src::Fill(x)) => row[c0..c0 + w].fill(x),
                        (Some((c0, w)), Src::Data(d)) => row[c0..c0 + w].copy_from_slice(&d[k..k + w]),
                        (Some((c0, w)), Src::Re(d)) => {
                            for (t, z) in row[c0..c0 + w].iter_mut().zip(&d[k..k + w]) {
                                *t = z.re;
                            }
                        }
                        (None, _) => {
                            for (j, &c) in cols.iter().enumerate() {
                                row[c] = src.at(k + j);
                            }
                        }
                    }
                    k += cols.len();
                }
                Some(())
            }
            (Value::Matrix(m), [i]) => {
                let s = Shared::shape(m);
                let (nr, nc) = (s[0], s[1]);
                let positions = match i {
                    Value::Scalar(_) if matches!(val, Value::Scalar(_)) => Value::resolve_index_dim(i, nr * nc).ok()?,
                    Value::All | Value::Vector(_) | Value::Matrix(_) => matrix_linear_positions(i, nr, nc).ok()?,
                    _ => return None,
                };
                if positions.iter().any(|&p| p >= nr * nc) {
                    return None;
                }
                // A matrix right-hand side is read column-major, like the
                // target positions; vectors and scalars in order.
                let col_major: Vec<f64>;
                let src = match (src, val) {
                    (Src::Data(_) | Src::Re(_), Value::Matrix(rhs)) => {
                        let rs = Shared::shape(rhs);
                        col_major = (0..rs[1]).flat_map(|c| (0..rs[0]).map(move |r| src.at(r * rs[1] + c))).collect();
                        Src::Data(&col_major)
                    }
                    (src, _) => src,
                };
                if !matches!(src, Src::Fill(_)) && src.len() != positions.len() {
                    return None;
                }
                let Some(Value::Matrix(m)) = self.env.get_mut(name) else { unreachable!() };
                let dst = Shared::make_mut_real(m)?.as_slice_mut()?;
                for (k, &p) in positions.iter().enumerate() {
                    let (row, col) = (p % nr, p / nr);
                    dst[row * nc + col] = src.at(k);
                }
                Some(())
            }
            (Value::Vector(v), [i]) => {
                let len = Shared::len(v);
                let positions = match i {
                    Value::Scalar(_) if matches!(val, Value::Scalar(_)) => Value::resolve_index_dim(i, len).ok()?,
                    Value::All | Value::Vector(_) => Value::resolve_index_dim_public(i, len).ok()?,
                    _ => return None,
                };
                if !matches!(src, Src::Fill(_)) && (!matches!(val, Value::Vector(_)) || src.len() != positions.len()) {
                    return None;
                }
                let Some(Value::Vector(v)) = self.env.get_mut(name) else { unreachable!() };
                let dst = Shared::make_mut_real(v)?.as_slice_mut()?;
                for (k, &p) in positions.iter().enumerate() {
                    dst[p] = src.at(k);
                }
                Some(())
            }
            _ => None,
        }
    }

    /// `M(rows, cols) = val` when `cols` is one consecutive run, `M` is
    /// row-major and `val` already has the region's shape (a scalar, or a
    /// vector / row-major matrix of `rows.len() × cols.len()` elements in
    /// row order): each target row is one slice copy or fill. Returns false
    /// without writing otherwise, for the general path (which also reports
    /// shape errors).
    fn region_write_runs(&mut self, name: &str, rows: &[usize], cols: &[usize], val: &Value) -> bool {
        let Some((c0, nc)) = Value::run_of(cols) else { return false };
        let n = rows.len() * nc;
        let (fill, src) = match val {
            Value::Scalar(x) => (Some(Complex::new(*x, 0.0)), None),
            Value::Complex(c) => (Some(*c), None),
            Value::Vector(v) if Shared::len(v) == n => (None, v.as_slice()),
            Value::Matrix(m) if Shared::shape(m) == [rows.len(), nc] => (None, m.as_slice()),
            _ => return false,
        };
        if fill.is_none() && src.is_none() {
            return false;
        }
        let Some(Value::Matrix(m)) = self.env.get_mut(name) else { return false };
        let stride = Shared::shape(m)[1];
        let Some(dst) = Shared::make_mut(m).as_slice_mut() else { return false };
        for (i, &r) in rows.iter().enumerate() {
            let row = &mut dst[r * stride + c0..r * stride + c0 + nc];
            match (fill, src) {
                (Some(x), _) => row.fill(x),
                (None, Some(s)) => row.copy_from_slice(&s[i * nc..(i + 1) * nc]),
                (None, None) => unreachable!(),
            }
        }
        true
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
                self.set_env(name, val);
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
                // Phase 7: invalidate the canonical-entry-id cache.
                // A new or redefined function may be a callee of any
                // already-hashed caller, so the safe correct thing is
                // to nuke the whole memo and let the dispatcher
                // rebuild on demand. The cost is one walk per fn next
                // call; cheap.
                self.cached_entry_ids.clear();
                // Phase 6a: if the cache is on, gate the freshly
                // defined function against the purity contract. An
                // impure function quietly falls out of cache scope
                // here — the dispatcher would otherwise route through
                // the cache under `all_scope` and silently return
                // stale results. The function itself is still
                // callable; only the cache-routing changes.
                self.cache_gate_user_fn(name, params, body);
                // Phase 6c: re-check any previously-removed sibling.
                // A function that failed the free-var gate because a
                // helper wasn't defined yet can now pass — mutual
                // recursion (`ping` ↔ `pong`) is the canonical case.
                self.cache_rescan_removed();
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
                    Some(Value::Table(t)) => {
                        t.assign_column(&field, val)
                            .map_err(ScriptError::runtime)?;
                    }
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
                        self.set_env(object, Value::Struct(fields));
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
                                        rustlab_plot::viewer_live::get_viewer_fig_id().unwrap_or(1);
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
                                    // Name the socket we tried. Over an SSH
                                    // forward that path is the whole story —
                                    // a stale or mistyped one is the usual
                                    // cause. Named sessions get no env-var
                                    // hint because they ignore it: the path
                                    // is derived from the uid and name.
                                    if let Some(n) = name {
                                        eprintln!("viewer: could not connect to session '{}' — is rustlab-viewer --name {} running?", n, n);
                                        eprintln!(
                                            "  tried: {}",
                                            rustlab_plot::socket_path_for_name(n).display()
                                        );
                                    } else {
                                        eprintln!(
                                            "viewer: could not connect — is rustlab-viewer running?"
                                        );
                                        eprintln!(
                                            "  tried: {} (override with RUSTLAB_VIEWER_SOCK)",
                                            rustlab_plot::default_socket_path().display()
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
                                eprintln!(
                                    "  use `viewer on` to connect an external rustlab-viewer"
                                );
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
                self.exec_run_path(path)?;
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
                        if !self.env.contains_key(name) && self.user_fns.contains_key(name) =>
                    {
                        let func = self.user_fns.get(name).cloned().unwrap();
                        let vals: Vec<Value> = args
                            .iter()
                            .map(|a| self.eval_expr(a))
                            .collect::<Result<_, _>>()?;
                        self.eval_user_fn_nargout(func, vals, names.len())?
                    }
                    Expr::Call { name, args }
                        if !self.env.contains_key(name) && !self.user_fns.contains_key(name) =>
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
                            self.set_env(name, v);
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
                            self.set_env(&names[0], single);
                        }
                    }
                }
            }
            StmtKind::While { cond, body } => loop {
                // Per-iteration cancel check so an empty-body loop still
                // stops (the body's `exec_stmt` checks cover non-empty ones).
                self.check_cancelled()?;
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
                    // A 1×N matrix is the row-vector form. Iterate its
                    // elements the same way a vector does. An N×1 column
                    // and any wider matrix still cannot be iterated.
                    Value::Matrix(m) if m.nrows() == 1 => m.row(0).iter().copied().collect(),
                    Value::Scalar(n) => vec![Complex::new(n, 0.0)],
                    other => {
                        return Err(ScriptError::runtime(format!(
                            "for: cannot iterate over {}",
                            other.type_name()
                        )))
                    }
                };
                for elem in elements {
                    // Per-iteration cancel check (covers empty-body loops).
                    self.check_cancelled()?;
                    let val = if elem.im == 0.0 {
                        Value::Scalar(elem.re)
                    } else {
                        Value::Complex(elem)
                    };
                    self.set_env(var, val);
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
                let frame = self.profiler.enter_op("[index-assign]");
                let result = self.exec_index_assign(name, indices, val, suppress);
                self.profiler.exit_op(frame, 0, 0);
                result?;
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

                let val = self.eval_expr_stmt(expr)?;
                if !suppress && !self.in_function && !matches!(val, Value::None) {
                    output::script_println(&val.format_display(self.number_format));
                    // `ans` is an ordinary variable. Assignments and `;`
                    // do not update it. A bare expression inside a
                    // function stays silent and leaves the caller's ans.
                    self.set_env("ans", val);
                }
            }
            StmtKind::Cache(c) => {
                self.exec_cache_stmt(c)?;
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
            let t = Shared::make_mut(t);
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
                Value::Vector(Array1::from_vec(kept).into())
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
                    Value::Matrix(Array2::zeros((0, 0)).into())
                } else if col_all {
                    // M(rows, :) = [] → drop the listed rows
                    let mut to_remove =
                        Value::resolve_index_dim(&idx0, nrows).map_err(ScriptError::runtime)?;
                    to_remove.sort_unstable();
                    to_remove.dedup();
                    let kept_rows: Vec<usize> = (0..nrows)
                        .filter(|r| to_remove.binary_search(r).is_err())
                        .collect();
                    let new_nrows = kept_rows.len();
                    let new =
                        Array2::from_shape_fn((new_nrows, ncols), |(i, j)| m[[kept_rows[i], j]]);
                    Value::Matrix(new.into())
                } else if row_all {
                    // M(:, cols) = [] → drop the listed columns
                    let mut to_remove =
                        Value::resolve_index_dim(&idx1, ncols).map_err(ScriptError::runtime)?;
                    to_remove.sort_unstable();
                    to_remove.dedup();
                    let kept_cols: Vec<usize> = (0..ncols)
                        .filter(|c| to_remove.binary_search(c).is_err())
                        .collect();
                    let new_ncols = kept_cols.len();
                    let new =
                        Array2::from_shape_fn((nrows, new_ncols), |(i, j)| m[[i, kept_cols[j]]]);
                    Value::Matrix(new.into())
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
        self.set_env(name, new_val);
        Ok(())
    }

    fn is_truthy(val: &Value, context: &str) -> Result<bool, ScriptError> {
        match val {
            Value::Bool(b) => Ok(*b),
            Value::Scalar(n) => Ok(*n != 0.0),
            Value::Int { data, .. } => Ok(*data != 0),
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
            Expr::Imag(n) => Ok(Value::Complex(Complex::new(0.0, *n))),
            Expr::IntLit(v) => {
                // Smallest fitting unsigned class (decision 7); the lexer
                // guarantees v ≤ u64::MAX, so this never returns None.
                let class = rustlab_core::IntClass::smallest_unsigned_for(*v)
                    .expect("lexer caps radix literals at u64::MAX");
                Ok(Value::Int {
                    data: *v as i128,
                    class,
                    overflow: rustlab_core::OverflowMode::Saturate,
                })
            }
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Var(name) if name == "dbcont" || name == "dbquit" => self.debugger_word(name),
            Expr::Var(name) => match self.env.get(name) {
                Some(v) => Ok(v.clone()),
                // Bare `tic` / `toc` (no parens) are the canonical timing
                // idiom, and bare `keyboard` is the debugger entry, so
                // fall through to the zero-arg builtin on an env miss.
                // A user variable of the same name still shadows it.
                // (Same spirit as the bare `clear` / `clf` statements.)
                None if name == "tic" || name == "toc" || name == "keyboard" => {
                    self.call_builtin_tracked(name, vec![])
                }
                None => Err(ScriptError::undefined(name.clone())),
            },
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
                self.eval_arith_binop(*op, lhs, rhs)
            }
            Expr::Call { name, args } => {
                // Consume the statement-position marker FIRST — before any
                // special case or argument evaluation can recurse — so only
                // this outermost call sees nargout 0; nested calls get 1.
                let call_nargout = if std::mem::take(&mut self.stmt_call_nargout0) {
                    0
                } else {
                    1
                };

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

                // ── Notebook widget read ──────────────────────────────────
                // `widget("name")` returns the current value of a notebook
                // widget. Resolves against the value table installed by the
                // notebook executor (seeded with declared defaults, overlaid
                // with live values by the interactive server). Outside a
                // notebook the table is absent and this is a hard error;
                // an unknown widget name is likewise a hard error (a silent
                // default would mask a renamed/forgotten widget reference).
                if name == "widget" {
                    if args.len() != 1 {
                        return Err(ScriptError::runtime(
                            "widget(name): expected exactly one string argument".to_string(),
                        ));
                    }
                    let key = self.eval_expr(&args[0])?.to_str().map_err(|_| {
                        ScriptError::runtime(
                            "widget(name): argument must be a string widget name".to_string(),
                        )
                    })?;
                    // Record the read so the executor can scope re-renders to
                    // blocks that actually depend on this widget.
                    self.widget_reads.insert(key.clone());
                    let table = self.widget_values.as_ref().ok_or_else(|| {
                        ScriptError::runtime(
                            "widget() is only available when rendering a notebook".to_string(),
                        )
                    })?;
                    return match table.get(&key) {
                        Some(WidgetValue::Number(n)) => Ok(Value::Scalar(*n)),
                        Some(WidgetValue::Text(s)) => Ok(Value::Str(s.clone())),
                        None => Err(ScriptError::runtime(format!("unknown widget '{key}'"))),
                    };
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
                if name == "fzero" && (args.len() == 2 || args.len() == 3) {
                    let func_val = self.eval_expr(&args[0])?;
                    let x0 = self.eval_expr(&args[1])?;
                    let tol = if args.len() == 3 {
                        Some(self.eval_expr(&args[2])?)
                    } else {
                        None
                    };
                    return self.eval_fzero(func_val, x0, tol);
                }
                // `table(a, b)` needs the argument expressions, not just
                // their values, so a bare variable becomes the column name.
                if name == "table" {
                    return self.eval_table(args);
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
                if matches!(self.env.get(name.as_str()), Some(Value::Table(_))) {
                    let table = match self.env.get(name.as_str()) {
                        Some(Value::Table(t)) => t.clone(),
                        _ => unreachable!(),
                    };
                    let idx_vals = self.eval_table_subscripts(&table, args)?;
                    return table.select(&idx_vals).map_err(ScriptError::runtime);
                }
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
                        | Some(Value::Scalar(_))
                        | Some(Value::Complex(_))
                        | Some(Value::Bool(_))
                        | Some(Value::Int { .. })
                        | Some(Value::IntArray { .. })
                ) {
                    self.eval_var_index(name, args)
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
                        Value::FuncHandle(target) => {
                            // `@name` is the function, never a workspace
                            // variable of the same name. Arguments are
                            // evaluated here; dispatch does not re-enter
                            // `Expr::Call`, which would index a vector
                            // named `sin` or call a lambda stored there.
                            let vals: Vec<Value> = args
                                .iter()
                                .map(|a| self.eval_expr(a))
                                .collect::<Result<_, _>>()?;
                            self.dispatch_func_handle(&target, vals, call_nargout)
                        }
                        _ => {
                            let vals: Vec<Value> = args
                                .iter()
                                .map(|a| self.eval_expr(a))
                                .collect::<Result<_, _>>()?;
                            self.call_builtin_tracked_nargout(name, vals, call_nargout)
                        }
                    }
                } else {
                    let vals: Vec<Value> = args
                        .iter()
                        .map(|a| self.eval_expr(a))
                        .collect::<Result<_, _>>()?;
                    self.call_builtin_tracked_nargout(name, vals, call_nargout)
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
                // Reserve the exact element count up front (large ranges like
                // 1:1e7 otherwise pay ~24 doubling reallocations).
                let expected = ((e - s) / inc).floor();
                // Guard against absurd ranges (e.g. a typo like `1:1e12`) that
                // would exhaust memory or loop effectively forever.
                const MAX_RANGE_LEN: f64 = 1e8;
                if !expected.is_finite() || expected > MAX_RANGE_LEN {
                    return Err(ScriptError::runtime(format!(
                        "range produces too many elements ({:.3e}); the limit is {:.0e}. \
                         Use a larger step or smaller bounds.",
                        expected.max(0.0),
                        MAX_RANGE_LEN
                    )));
                }
                let capacity = if expected >= 0.0 {
                    expected as usize + 1
                } else {
                    0
                };
                let mut vals: Vec<C64> = Vec::with_capacity(capacity);
                let mut cur = s;
                // Use a small epsilon to avoid float boundary issues
                let eps = inc.abs() * 1e-10;
                // Integer start and step (`1:n`, `i:i + n - 1`, `10:-2:0`):
                // every value is an exact integer, so `s + k·inc` equals the
                // accumulated `cur` below; fill without the serial dependence.
                const EXACT: f64 = (1u64 << 52) as f64;
                if s.fract() == 0.0
                    && inc.fract() == 0.0
                    && s.abs() < EXACT
                    && e.is_finite()
                    && e.abs() < EXACT
                {
                    let in_range = |v: f64| if inc > 0.0 { v <= e + eps } else { v >= e - eps };
                    let mut n = capacity;
                    while n > 0 && !in_range(s + (n - 1) as f64 * inc) {
                        n -= 1;
                    }
                    while in_range(s + n as f64 * inc) {
                        n += 1;
                    }
                    vals.extend((0..n).map(|k| Complex::new(s + k as f64 * inc, 0.0)));
                    return Ok(Value::Vector(Array1::from_vec(vals).into()));
                }
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
                Ok(Value::Vector(Array1::from_vec(vals).into()))
            }
            Expr::Transpose(inner) => self.eval_transpose(inner, true),
            Expr::NonConjTranspose(inner) => self.eval_transpose(inner, false),
            Expr::All => Ok(Value::All),
            Expr::Index { expr, args } => {
                let container = self.eval_expr(expr)?;
                if let Value::Table(t) = &container {
                    let idx_vals = self.eval_table_subscripts(t, args)?;
                    return t.select(&idx_vals).map_err(ScriptError::runtime);
                }
                // `end` inside a chained index. One index is linear
                // (`numel` for a matrix). Two indexes on a matrix bind
                // `end` per dimension, same as `M(i, j)`.
                let idx_vals = if args.len() == 2 {
                    if let Value::Matrix(m) = &container {
                        self.env
                            .insert("end".to_string(), Value::Scalar(m.nrows() as f64));
                        let row_val = self.eval_expr(&args[0])?;
                        self.env
                            .insert("end".to_string(), Value::Scalar(m.ncols() as f64));
                        let col_val = self.eval_expr(&args[1])?;
                        self.env.remove("end");
                        vec![row_val, col_val]
                    } else {
                        let end_val = match &container {
                            Value::Vector(v) => v.len(),
                            _ => 0,
                        };
                        self.env
                            .insert("end".to_string(), Value::Scalar(end_val as f64));
                        let vals: Vec<Value> = args
                            .iter()
                            .map(|a| self.eval_expr(a))
                            .collect::<Result<_, _>>()?;
                        self.env.remove("end");
                        vals
                    }
                } else {
                    let end_val = match &container {
                        Value::Vector(v) => v.len(),
                        Value::Matrix(m) => m.nrows() * m.ncols(),
                        _ => 0,
                    };
                    self.env
                        .insert("end".to_string(), Value::Scalar(end_val as f64));
                    let vals: Vec<Value> = args
                        .iter()
                        .map(|a| self.eval_expr(a))
                        .collect::<Result<_, _>>()?;
                    self.env.remove("end");
                    vals
                };
                let frame = self.profiler.enter_op("[index]");
                let result = container.index(idx_vals).map_err(|e| ScriptError::runtime(e));
                self.profiler.exit_op(frame, 0, 0);
                result
            }
            Expr::Lambda { params, body } => Ok(Value::Lambda {
                params: params.clone(),
                body: body.clone(),
                captured_env: self.capture_lambda_env(params, body),
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
            Expr::ContentIndex { expr, args } => {
                let container = self.eval_expr(expr)?;
                let Value::Table(t) = &container else {
                    return Err(ScriptError::runtime(format!(
                        "content indexing with {{}} is only supported for a table, got {}",
                        container.type_name()
                    )));
                };
                let idx_vals = self.eval_table_subscripts(t, args)?;
                t.contents(&idx_vals).map_err(ScriptError::runtime)
            }
            Expr::Field { object, field } => {
                // `S.f` on a struct variable (or a chain `S.a.b`): borrow the
                // struct and clone only the field. Evaluating `object` would
                // deep-copy the whole struct first, which for a model's
                // parameter struct is megabytes per read.
                if let Some(Value::Struct(fields)) = self.borrow_field_chain(object) {
                    if let Some(v) = fields.get(field.as_str()) {
                        return Ok(v.clone());
                    }
                }
                let obj = self.eval_expr(object)?;
                match obj {
                    Value::Table(t) => {
                        if field == "Properties" {
                            Ok(t.properties())
                        } else {
                            t.column(field).map_err(ScriptError::runtime)
                        }
                    }
                    Value::Struct(fields) => fields.get(field.as_str()).cloned().ok_or_else(|| {
                        ScriptError::runtime(format!("struct has no field '{}'", field))
                    }),
                    Value::StateSpace { a, b, c, d } => match field.as_str() {
                        "A" => Ok(Value::Matrix(a.into())),
                        "B" => Ok(Value::Matrix(b.into())),
                        "C" => Ok(Value::Matrix(c.into())),
                        "D" => Ok(Value::Matrix(d.into())),
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
        let in_bytes = Self::value_bytes(&input);
        let tracking = self.profiler.enter("arrayfun");
        let result = self.eval_arrayfun_inner(func, input);
        if tracking {
            let out = result.as_ref().map_or(0, Self::value_bytes);
            self.profiler.exit("arrayfun", in_bytes, out);
        }
        result
    }

    /// Convert a 1-D iterable argument (vector, scalar, or complex scalar)
    /// into per-element `Value`s — the shared shape contract of `arrayfun`
    /// and `parmap`. `caller` names the builtin in the error message.
    fn iterable_elements(caller: &str, input: &Value) -> Result<Vec<Value>, ScriptError> {
        match input {
            Value::Vector(v) => Ok(v
                .iter()
                .map(|&c| {
                    if c.im == 0.0 {
                        Value::Scalar(c.re)
                    } else {
                        Value::Complex(c)
                    }
                })
                .collect()),
            Value::Scalar(n) => Ok(vec![Value::Scalar(*n)]),
            Value::Complex(c) => Ok(vec![Value::Complex(*c)]),
            other => Err(ScriptError::runtime(format!(
                "{caller}: second argument must be a vector or scalar, got {}",
                other.type_name()
            ))),
        }
    }

    fn eval_arrayfun_inner(&mut self, func: Value, input: Value) -> Result<Value, ScriptError> {
        let elements: Vec<Value> = Self::iterable_elements("arrayfun", &input)?;

        let mut results: Vec<Value> = Vec::with_capacity(elements.len());
        for elem in elements {
            let out = self.call_callable(func.clone(), vec![elem])?;
            results.push(out);
        }

        // Determine output shape from first result
        match results.first() {
            None => Ok(Value::Vector(Array1::from_vec(vec![]).into())),
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
                Ok(Value::Vector(Array1::from_vec(out).into()))
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
                Ok(Value::Matrix(m.into()))
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

        // Extract iterable elements — same shape rules as arrayfun.
        let elements: Vec<Value> = Self::iterable_elements("parmap", &input)?;

        // Profiler: the parmap row is the call's wall time. Workers profile
        // into their own copies, merged back below as `parmap:<name>` rows.
        let in_bytes = Self::value_bytes(&input);
        let tracking = self.profiler.enter("parmap");

        // Template without the caller's variables; each rayon split clones
        // it once and reuses that worker for the tasks it runs.
        let template = self.clone_for_parallel_lambda(&func);
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
        let run = backend.run(&move || template.clone(), func, elements, master_seed);
        let result = run.and_then(|out| {
            for worker_stats in out.profiles {
                self.profiler.merge_parallel(worker_stats);
            }
            parmap::pack_results(out.values)
        });
        if tracking {
            let out = result.as_ref().map_or(0, Self::value_bytes);
            self.profiler.exit("parmap", in_bytes, out);
        }
        result
    }

    /// Linear / logical assignment into a matrix: `M(idx) = rhs`.
    ///
    /// Positions are column-major, matching `M(k)` reads. A same-length 0/1
    /// vector, or a same-shape 0/1 matrix, is a logical mask and does not
    /// grow the target. A numeric index vector writes in the order given
    /// (a repeated index keeps the last value). A 1×N or N×1 matrix grows
    /// along its long axis when a numeric index runs past the end, the same
    /// way a scalar `M(k) = v` does; a general 2-D matrix does not grow.
    /// A scalar right-hand side broadcasts. A vector (or a 1-D matrix, or a
    /// matrix with the same number of elements) must match the selection.
    fn assign_matrix_linear(
        &mut self,
        name: &str,
        idx: &Value,
        val: &Value,
        suppress: bool,
    ) -> Result<(), ScriptError> {
        let (nr0, nc0) = match self.env.get(name) {
            Some(Value::Matrix(m)) => {
                let s = Shared::shape(m);
                (s[0], s[1])
            }
            _ => {
                return Err(ScriptError::runtime(
                    "index assignment: expected a matrix".to_string(),
                ))
            }
        };
        let total = nr0 * nc0;
        let shape_1d = nr0 <= 1 || nc0 <= 1;
        let positions = matrix_linear_positions(idx, nr0, nc0)?;
        let needs = positions.iter().copied().max().map(|p| p + 1).unwrap_or(0);
        if needs > total {
            if !shape_1d {
                return Err(ScriptError::runtime(format!(
                    "index assignment: linear index {} out of bounds for {}×{} matrix ({} elements)",
                    needs, nr0, nc0, total
                )));
            }
            match self.env.get_mut(name) {
                Some(Value::Matrix(m)) => grow_oned_matrix(Shared::make_mut(m), needs),
                _ => unreachable!(),
            }
        }
        let n_target = positions.len();
        let src = rhs_for_linear_assign(val, n_target)?;
        match self.env.get_mut(name) {
            Some(Value::Matrix(m)) => {
                let m = Shared::make_mut(m);
                let nr = m.nrows();
                for (k, &p) in positions.iter().enumerate() {
                    let col = p / nr.max(1);
                    let row = if nr == 0 { 0 } else { p % nr };
                    m[[row, col]] = src[k];
                }
            }
            _ => unreachable!(),
        }
        if !suppress && self.echo_enabled() {
            output::script_println(&format!(
                "{}(...) = ({} element(s) updated)",
                name, n_target
            ));
        }
        Ok(())
    }

    /// `table(a, b, 'VariableNames', names)`. A bare variable lends its
    /// name; any other expression is `VarN`. The option name is a string
    /// literal so a computed string stays a one-row text column.
    fn eval_table(&mut self, args: &[Expr]) -> Result<Value, ScriptError> {
        let mut inputs = Vec::new();
        let mut override_names = None;
        let mut i = 0;
        while i < args.len() {
            if let Expr::Str(s) = &args[i] {
                if table::is_varnames(s) {
                    if override_names.is_some() {
                        return Err(ScriptError::runtime(
                            "table: VariableNames specified more than once".to_string(),
                        ));
                    }
                    if i + 1 >= args.len() {
                        return Err(ScriptError::runtime(
                            "table: VariableNames needs a string array of names".to_string(),
                        ));
                    }
                    let names_val = self.eval_expr(&args[i + 1])?;
                    override_names = Some(
                        table::names_from_value(names_val).map_err(ScriptError::type_err)?,
                    );
                    i += 2;
                    continue;
                }
            }
            let name = match &args[i] {
                Expr::Var(n) => Some(n.clone()),
                _ => None,
            };
            let value = self.eval_expr(&args[i])?;
            inputs.push(table::TableInput { name, value });
            i += 1;
        }
        table::build_table(inputs, override_names)
            .map(Value::Table)
            .map_err(ScriptError::type_err)
    }

    /// Bind `end` per axis while evaluating a table's two subscripts.
    fn eval_table_subscripts(
        &mut self,
        table: &table::Table,
        args: &[Expr],
    ) -> Result<Vec<Value>, ScriptError> {
        if args.len() == 2 {
            self.env
                .insert("end".to_string(), Value::Scalar(table.height() as f64));
            let row_val = self.eval_expr(&args[0])?;
            self.env
                .insert("end".to_string(), Value::Scalar(table.width() as f64));
            let col_val = self.eval_expr(&args[1])?;
            self.env.remove("end");
            Ok(vec![row_val, col_val])
        } else {
            let n = if args.len() == 1 {
                table.height()
            } else {
                0
            };
            self.env
                .insert("end".to_string(), Value::Scalar(n as f64));
            let vals = args
                .iter()
                .map(|a| self.eval_expr(a))
                .collect::<Result<Vec<_>, _>>()?;
            self.env.remove("end");
            Ok(vals)
        }
    }

    /// `fzero(f, x0)` / `fzero(f, [a, b])` / `fzero(f, x0, tol)`.
    ///
    /// A bracket must change sign. A scalar start searches outward (step
    /// grows by 1.6, positive side first) until `f` changes sign against
    /// `f(x0)`, then Brent's method. `tol` is an absolute x tolerance;
    /// the iteration also stops within `2*eps*|x|`.
    fn eval_fzero(
        &mut self,
        func: Value,
        x0: Value,
        tol: Option<Value>,
    ) -> Result<Value, ScriptError> {
        let tol = match tol {
            None => 1e-12,
            Some(v) => {
                let t = v.to_scalar().map_err(|e| ScriptError::type_err(e))?;
                if !t.is_finite() || t < 0.0 {
                    return Err(ScriptError::type_err(format!(
                        "fzero: TolX must be a non-negative finite number, got {t}"
                    )));
                }
                t
            }
        };
        let eval_at = |this: &mut Self, x: f64| -> Result<f64, ScriptError> {
            let y = this.call_callable(func.clone(), vec![Value::Scalar(x)])?;
            match y {
                Value::Scalar(v) if v.is_finite() => Ok(v),
                Value::Complex(c) if c.re.is_finite() && c.im.abs() < 1e-8 => Ok(c.re),
                other => Err(ScriptError::runtime(format!(
                    "fzero: function must return a finite real scalar, got {}",
                    other.type_name()
                ))),
            }
        };
        let (a, b, fa, fb) = match &x0 {
            Value::Vector(v) if v.len() == 2 => {
                let a = v[0].re;
                let b = v[1].re;
                let fa = eval_at(self, a)?;
                let fb = eval_at(self, b)?;
                (a, b, fa, fb)
            }
            Value::Matrix(m) if m.len() == 2 => {
                let vals: Vec<f64> = m.iter().map(|c| c.re).collect();
                let fa = eval_at(self, vals[0])?;
                let fb = eval_at(self, vals[1])?;
                (vals[0], vals[1], fa, fb)
            }
            Value::Scalar(x) => {
                let x = *x;
                let fx = eval_at(self, x)?;
                if fx == 0.0 {
                    return Ok(Value::Scalar(x));
                }
                let mut dx = if x == 0.0 { 0.02 } else { x.abs() / 50.0 };
                if dx == 0.0 {
                    dx = 0.02;
                }
                let mut bracket = None;
                for _ in 0..60 {
                    let right = x + dx;
                    let fr = eval_at(self, right)?;
                    if fr == 0.0 {
                        return Ok(Value::Scalar(right));
                    }
                    if fx * fr < 0.0 {
                        bracket = Some((x, right, fx, fr));
                        break;
                    }
                    let left = x - dx;
                    let fl = eval_at(self, left)?;
                    if fl == 0.0 {
                        return Ok(Value::Scalar(left));
                    }
                    if fx * fl < 0.0 {
                        bracket = Some((left, x, fl, fx));
                        break;
                    }
                    dx *= 1.6;
                }
                bracket.ok_or_else(|| {
                    ScriptError::runtime(
                        "fzero: failed to find a sign change around the start point".to_string(),
                    )
                })?
            }
            other => {
                return Err(ScriptError::type_err(format!(
                    "fzero: start must be a scalar or a 2-element bracket, got {}",
                    other.type_name()
                )))
            }
        };
        if !fa.is_finite() || !fb.is_finite() {
            return Err(ScriptError::runtime(
                "fzero: function values at the bracket must be finite".to_string(),
            ));
        }
        if fa * fb > 0.0 {
            return Err(ScriptError::runtime(
                "fzero: function values at the interval endpoints must differ in sign".to_string(),
            ));
        }
        if fa == 0.0 {
            return Ok(Value::Scalar(a));
        }
        if fb == 0.0 {
            return Ok(Value::Scalar(b));
        }
        let root = brent_root(&mut |x| eval_at(self, x), a, b, fa, fb, tol)?;
        Ok(Value::Scalar(root))
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
                Value::Matrix(col.into())
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
            Ok(Value::Vector(out_mat.row(0).to_owned().into()))
        } else {
            Ok(Value::Matrix(out_mat.into()))
        }
    }

    /// Call the function named by a `@name` handle.
    ///
    /// User function, then builtin. A workspace variable of the same
    /// name is not consulted: `sin = [1, 2, 3]; h = @sin; h(0)` is
    /// `sin(0)`. `@feval` is the exception, because `feval` is not a
    /// builtin — it is the evaluator's dynamic call, and that call
    /// still looks names up in the current environment (the lambda
    /// snapshot, when the handle is invoked from a lambda).
    ///
    /// A handle taken while the name holds a lambda is not a
    /// `FuncHandle` at all: `Expr::FuncHandle` clones that lambda at
    /// creation, so `g = @f` keeps the lambda even if `f` later changes.
    fn dispatch_func_handle(
        &mut self,
        name: &str,
        args: Vec<Value>,
        nargout: usize,
    ) -> Result<Value, ScriptError> {
        if name == "feval" {
            if args.is_empty() {
                return Err(ScriptError::runtime(
                    "feval: first argument must be a string function name".to_string(),
                ));
            }
            let fn_name = args[0].to_str().map_err(|_| {
                ScriptError::runtime(
                    "feval: first argument must be a string function name".to_string(),
                )
            })?;
            return self.eval_feval(&fn_name, args.into_iter().skip(1).collect());
        }
        if let Some(func) = self.user_fns.get(name).cloned() {
            return self.eval_user_fn(func, args);
        }
        self.call_builtin_tracked_nargout(name, args, nargout)
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
                // Same rule as a direct `h(x)`: the handle names a function,
                // not a variable that happens to share that name.
                self.dispatch_func_handle(&name, args, 1)
            }
            other => Err(ScriptError::runtime(format!(
                "expected a lambda or function handle, got {}",
                other.type_name()
            ))),
        }
    }

    /// Snapshot the bindings a lambda body can see.
    ///
    /// MATLAB-style: by value, at creation. Later writes to the outer
    /// workspace do not change the lambda. Only free names are copied,
    /// so a large unreferenced matrix is not cloned into every closure
    /// (and a closure stored in the workspace is not cloned into the
    /// next one). Built-in constants are always included. They are a
    /// handful of scalars, and a user function called from the lambda
    /// seeds `pi` / `i` / `j` from the caller's environment — that
    /// caller is this snapshot.
    ///
    /// A dynamic `feval`, or a call through a handle whose target is
    /// `feval`, still clones the whole environment. The name those look
    /// up is not in the syntax. Any other `@name` handle does not pull
    /// a same-named variable into the snapshot: calling the handle does
    /// not look that variable up.
    ///
    /// Values that are referenced are cloned in full. There is no
    /// copy-on-write, so a lambda that mentions a large matrix copies
    /// that matrix once.
    fn capture_lambda_env(&self, params: &[String], body: &Expr) -> fast_hash::FxHashMap<String, Value> {
        let analysis = crate::free_vars::analyze_lambda(params, body);
        if analysis.dynamic_feval || self.captured_handle_is_feval(&analysis.names) {
            return self.env.clone();
        }
        let names = analysis.names;
        // Same set `clear` preserves, including `stdin`. User functions
        // copy `i`, `j`, `pi`, `e`, `Inf`, `NaN`, and `stdin` from the
        // caller, which is this snapshot, so a shadowed `pi` still
        // reaches `addpi` called from the lambda. `true` / `false` ride
        // along; they are the same kind of constant and cost nothing
        // next to a matrix.
        const CONSTS: &[&str] = line_io::BUILTIN_CONSTS;
        let mut captured = fast_hash::FxHashMap::with_capacity_and_hasher(names.len() + CONSTS.len(), Default::default());
        for name in names {
            if let Some(v) = self.env.get(&name) {
                captured.insert(name, v.clone());
            }
        }
        for name in CONSTS {
            if captured.contains_key(*name) {
                continue;
            }
            if let Some(v) = self.env.get(*name) {
                captured.insert((*name).to_string(), v.clone());
            }
        }
        captured
    }

    /// `true` when a free name holds `@feval`. Calling that handle looks
    /// a name up dynamically, so the snapshot has to be the whole
    /// environment. Other handles name a function and do not capture
    /// a variable of the same name.
    fn captured_handle_is_feval(&self, names: &BTreeSet<String>) -> bool {
        names.iter().any(|name| {
            matches!(
                self.env.get(name),
                Some(Value::FuncHandle(target)) if target == "feval"
            )
        })
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
        captured_env: fast_hash::FxHashMap<String, Value>,
        args: Vec<Value>,
    ) -> Result<Value, ScriptError> {
        if args.len() != params.len() {
            return Err(ScriptError::runtime(format!(
                "lambda expects {} argument(s), got {}",
                params.len(),
                args.len()
            )));
        }

        // Profiling: a lambda called through a variable gets a row under
        // that name; an anonymous callback (arrayfun / parmap) does not, so
        // its children are attributed to the caller.
        let in_bytes: u64 = args.iter().map(Self::value_bytes).sum();
        let tracking = !call_name.is_empty() && self.profiler.enter(call_name);

        // Save outer env; install captured env + parameter bindings
        let saved_env = std::mem::replace(&mut self.env, captured_env);
        let saved_in_fn = self.in_function;
        self.in_function = true;
        for (pname, val) in params.iter().zip(args) {
            self.set_env(pname, val);
        }
        let result = self.eval_expr(body);
        // Restore outer env
        self.env = saved_env;
        self.in_function = saved_in_fn;
        if tracking {
            let out = result.as_ref().map_or(0, Self::value_bytes);
            self.profiler.exit(call_name, in_bytes, out);
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

        // ── Cache fast path (Phase 3d + 6d) ───────────────────────
        //
        // Both single- and multi-output calls go through the cache.
        // The stored blob is always a `Value::Tuple` of the function's
        // declared return vars in source order — the **canonical
        // output set**, with `Value::None` filling unassigned slots.
        // The caller's `nargout` only shapes the user-facing return
        // value (via [`shape_user_fn_return`]); the body execution
        // and the cache key are both nargout-independent. So
        // `p = f(x)` and `[p, q] = f(x)` share a cache entry — the
        // body runs once across both call sites.
        //
        // The keys: entry_id mixes (file_ast_hash, fn_name) when the
        // function was loaded via `cache add file`, or the body's
        // own AST hash for inline-defined functions. input_hash is
        // BLAKE3 over the canonical encoding of the argument list.
        // Either can fail (NaN arg, non-cacheable variant) — in that
        // case we bypass the cache for this invocation only.
        let cache_in_scope = self.cache_registry.is_in_scope(&func.name);
        let cache_keys: Option<([u8; 32], [u8; 32])> = if cache_in_scope {
            // Phase 7 (Option 3): canonical, rename-invariant,
            // transitively-correct identity hash. Both inline-
            // defined and file-loaded functions use the same
            // formula — the file_ast_hash mixing the original
            // design used for file-loaded functions is dropped on
            // purpose, so moving a function between files (without
            // editing the body) preserves cache warmth.
            let entry_id = self.canonical_entry_id_for(&func);
            match crate::cache_value::fingerprint_args(&args) {
                Some(input_hash) => Some((entry_id, input_hash)),
                None => {
                    self.cache_registry.counters_mut().uncacheable_arg_skips += 1;
                    None
                }
            }
        } else {
            None
        };

        // GET path: on a hit, decode the canonical tuple and shape
        // the user-facing return per the caller's nargout. On miss
        // or any error, fall through to the normal compute path;
        // we'll PUT after.
        if let Some((entry_id, input_hash)) = cache_keys {
            let stored = self
                .cache_registry
                .store()
                .and_then(|s| s.get(&entry_id, &input_hash).ok().flatten());
            if let Some(bytes) = stored {
                match crate::cache_value::deserialize_value(&bytes) {
                    Some(Value::Tuple(canonical)) => {
                        self.cache_registry.record_hit(&func.name);
                        return shape_user_fn_return(
                            &canonical,
                            nargout,
                            &func.name,
                            &func.return_vars,
                        );
                    }
                    // Either the wire format mismatched (e.g. an old
                    // v1 single-Value blob) or the row decoded as
                    // something that isn't the expected Tuple. Either
                    // way: stale; bump the skip counter and recompute.
                    // A `cache clear` or version-aware prune handles
                    // cleanup of the leftover row.
                    Some(_) | None => {
                        self.cache_registry.counters_mut().serialization_skips += 1;
                    }
                }
            } else {
                self.cache_registry.record_miss(&func.name);
            }
        }

        let in_bytes: u64 = args.iter().map(Self::value_bytes).sum();
        let tracking = self.profiler.enter(&func.name);

        // Save outer env and function flag
        let saved_env = std::mem::take(&mut self.env);
        let saved_in_fn = self.in_function;
        self.in_function = true;
        // Seed with built-in constants
        for name in line_io::BUILTIN_CONSTS {
            if let Some(v) = saved_env.get(*name) {
                self.env.insert((*name).to_string(), v.clone());
            }
        }
        // Bind parameters
        for (param, val) in func.params.iter().zip(args) {
            self.set_env(param, val);
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
        // Capture the canonical full output set from the function
        // body's environment BEFORE we restore. One slot per declared
        // return var, in source order, with `Value::None` filling
        // any slot the body didn't assign. This is the unit the
        // cache stores; the caller's `nargout` only shapes the
        // user-facing return below.
        // The body's scope is discarded just below, so outputs are moved out
        // of it rather than cloned (a repeated output name clones the copy
        // already taken).
        let mut canonical: Vec<Value> = Vec::with_capacity(func.return_vars.len());
        for (k, ret) in func.return_vars.iter().enumerate() {
            let v = match func.return_vars[..k].iter().position(|r| r == ret) {
                Some(first) => canonical[first].clone(),
                None => self.env.remove(ret.as_str()).unwrap_or(Value::None),
            };
            canonical.push(v);
        }

        // Restore outer env and function flag
        self.env = saved_env;
        self.in_function = saved_in_fn;
        if tracking {
            let out: u64 = canonical.iter().map(Self::value_bytes).sum();
            self.profiler.exit(&func.name, in_bytes, out);
        }

        if let Some(e) = body_err {
            return Err(e);
        }

        // Shape the user-facing return from the canonical tuple.
        // Same helper the cache-hit path uses; the under-assignment
        // error path is identical whether we computed or replayed.
        let ret_val = shape_user_fn_return(&canonical, nargout, &func.name, &func.return_vars)?;

        // ── Cache PUT (Phase 3d + 6d) ────────────────────────────
        // Store the canonical full output set wrapped in a
        // `Value::Tuple`. If any output is non-cacheable (e.g. a
        // `Lambda` captured from outer scope, a `LiveFigure` handle),
        // `serialize_value` returns `None` and we silently skip the
        // store — the call ran correctly, just no cache row.
        // `Store::put` swallows transient write errors (disk full,
        // RO FS) per Phase 1's design.
        if let Some((entry_id, input_hash)) = cache_keys {
            if let Some(bytes) = crate::cache_value::serialize_value(&Value::Tuple(canonical)) {
                if let Some(store) = self.cache_registry.store() {
                    // Pass the function name through so `cache list`
                    // can show it. The metadata table is sibling to
                    // cache_entries; if the row already has metadata
                    // recorded (same entry_id from a prior write or
                    // process), INSERT OR IGNORE keeps the first.
                    let _ = store.put_with_meta(&entry_id, &input_hash, &bytes, &func.name);
                }
            }
        }

        Ok(ret_val)
    }

    /// Evaluate a bare expression statement. When the statement is a
    /// call, arm the one-shot `stmt_call_nargout0` flag: the `Expr::Call`
    /// arm consumes it at entry (before any recursion, so nested calls
    /// are unaffected) and passes `nargout = 0` to the builtin dispatch.
    /// Nargout-aware builtins like `figure()` / `histogram(v)` use that
    /// to stay silent at statement position (no handle / bin-matrix
    /// echo) while still returning their value when assigned
    /// (`h = figure()` passes 1). Special-cased calls (`profile`,
    /// `widget`, `arrayfun`, …), variable indexing, lambdas, and user
    /// functions are untouched — they ignore the consumed flag.
    fn eval_expr_stmt(&mut self, expr: &Expr) -> Result<Value, ScriptError> {
        if matches!(expr, Expr::Call { .. }) {
            self.stmt_call_nargout0 = true;
        }
        let result = self.eval_expr(expr);
        self.stmt_call_nargout0 = false;
        result
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
        // Line input checks the notebook first, then the parallel-lambda
        // denylist, then the terminal. Doing it inside the dispatcher
        // keeps that order. A worker therefore reports `parmap: cannot …`
        // rather than the "no interactive terminal" error.
        if Self::is_line_io(name) {
            return self.dispatch_line_io(name, vals);
        }
        if IMPURE_BUILTINS.contains(&name) {
            parmap::require_pure_context(name)?;
        }

        // Integer widening (dev/plans/integer_types.md, decision 8): every
        // builtin except the integer-aware set receives integers widened to
        // their double form, so the whole builtin surface accepts them without
        // per-builtin arms. The allowlist covers builtins that must see the raw
        // integer (casts / introspection) or that intentionally preserve the
        // integer class (abs / real / imag / conj / transpose).
        let vals: Vec<Value> = if INT_AWARE_BUILTINS.contains(&name) {
            vals
        } else {
            vals.into_iter().map(Value::widen_int).collect()
        };

        if !self.profiler.is_enabled() {
            return self.builtins.call_with_nargout(name, vals, nargout);
        }
        let in_bytes: u64 = vals.iter().map(Self::value_bytes).sum();
        self.profiler.enter(name);
        let result = self.builtins.call_with_nargout(name, vals, nargout);
        let out = result.as_ref().map_or(0, Self::value_bytes);
        self.profiler.exit(name, in_bytes, out);
        result
    }

    /// `name(args)` where `name` holds an indexable value (the indexing
    /// branch of `Expr::Call`). Kept out of `eval_expr` so its locals do not
    /// enlarge every recursive `eval_expr` frame.
    #[inline(never)]
    fn eval_var_index(&mut self, name: &str, args: &[Expr]) -> Result<Value, ScriptError> {
        // A scalar / complex / bool / integer is a 1×1 value; promote
        // it to a 1-element container so `s(1)` indexes like MATLAB
        // instead of falling through and reporting `s` as undefined.
        // Arrays are not copied: their shape is read here, the
        // subscripts are evaluated, and the variable is borrowed
        // for the selection itself.
        let promoted: Option<Value> = match &self.env[name] {
            Value::Scalar(x) => {
                Some(Value::Vector(Array1::from_elem(1, Complex::new(*x, 0.0)).into()))
            }
            Value::Complex(c) => Some(Value::Vector(Array1::from_elem(1, *c).into())),
            Value::Bool(b) => Some(Value::Vector(Array1::from_elem(
                1,
                Complex::new(if *b { 1.0 } else { 0.0 }, 0.0),
            ).into())),
            // A scalar integer becomes a 1×1 integer array so the
            // indexing path (which handles IntArray) accepts it.
            Value::Int {
                data,
                class,
                overflow,
            } => Some(Value::IntArray {
                data: vec![*data],
                rows: 1,
                cols: 1,
                class: *class,
                overflow: *overflow,
            }),
            _ => None,
        };
        let shape = IndexShape::of(promoted.as_ref().unwrap_or(&self.env[name]));

        // For 3-argument tensor3 indexing, bind `end` per-dimension.
        let idx_vals: Vec<Value> = if args.len() == 3 {
            if let Some((m, n, p)) = shape.tensor {
                self.set_env("end", Value::Scalar(m as f64));
                let iv = self.eval_expr(&args[0])?;
                self.set_env("end", Value::Scalar(n as f64));
                let jv = self.eval_expr(&args[1])?;
                self.set_env("end", Value::Scalar(p as f64));
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
            let (nrows, ncols) = shape.rows_cols.unwrap_or_else(|| unreachable!());
            if nrows > 1 || shape.matrix_like {
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
            let len = shape.len.unwrap_or_else(|| unreachable!());
            self.env
                .insert("end".to_string(), Value::Scalar(len as f64));
            let vals: Vec<Value> = args
                .iter()
                .map(|a| self.eval_expr(a))
                .collect::<Result<_, _>>()?;
            self.env.remove("end");
            vals
        };

        let frame = self.profiler.enter_op("[index]");
        let container = match &promoted {
            Some(v) => v,
            None => &self.env[name],
        };
        let result = container.index_ref(idx_vals).map_err(|e| ScriptError::runtime(e));
        self.profiler.exit_op(frame, 0, 0);
        result
    }

    /// `lhs op rhs` for the arithmetic and comparison operators. An operand
    /// that is a variable holding an array is borrowed from the workspace
    /// rather than copied (`elementwise::binop` reuses owned temporaries for
    /// the result). Variables cannot change while the other operand is
    /// evaluated (expressions do not assign), so the order is unobservable:
    /// non-variable operands are evaluated first, then variables borrowed.
    #[inline(never)]
    fn eval_arith_binop(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Value, ScriptError> {
        if op == BinOp::Mul && (is_transpose_expr(lhs) || is_transpose_expr(rhs)) {
            return self.eval_transposed_matmul(lhs, rhs);
        }
        // Left to right, as before; a borrowed variable is only looked up.
        let l = self.binop_operand(lhs)?;
        let r = self.binop_operand(rhs)?;
        let frame_names = self.profiler.is_enabled();
        // Real scalar arithmetic (`acc = acc + k` in a loop) skips the
        // general dispatch: `Value::binop`'s Scalar∘Scalar arm is exactly
        // this `f64` operation for these operators, and nothing before that
        // arm applies to two real scalars. With profiling on, the general
        // path runs so the operator rows are recorded.
        if !frame_names {
            if let (PreOperand::Owned(Value::Scalar(a)), PreOperand::Owned(Value::Scalar(b))) = (&l, &r) {
                let (a, b) = (*a, *b);
                let fast = match op {
                    BinOp::Add => Some(a + b),
                    BinOp::Sub => Some(a - b),
                    BinOp::Mul | BinOp::ElemMul => Some(a * b),
                    BinOp::Div | BinOp::ElemDiv => Some(a / b),
                    _ => None,
                };
                if let Some(x) = fast {
                    return Ok(Value::Scalar(x));
                }
            }
        }
        self.finish_binop(op, l, r)
    }

    /// The general part of [`eval_arith_binop`](Self::eval_arith_binop),
    /// once both operands are evaluated.
    fn finish_binop(&mut self, op: BinOp, l: PreOperand, r: PreOperand) -> Result<Value, ScriptError> {
        use elementwise::Operand;
        let frame_names = self.profiler.is_enabled();
        let result = match (l, r) {
            // Neither side is an array variable: the plain path (scalar
            // arithmetic in loops must not pay for the borrowing machinery).
            (PreOperand::Owned(l), PreOperand::Owned(r)) if !is_dense(&l) && !is_dense(&r) => {
                let frame = if frame_names {
                    self.profiler.enter_op(profile_op_name(op, &l, &r))
                } else {
                    None
                };
                let in_bytes = if frame.is_some() { Self::value_bytes(&l) + Self::value_bytes(&r) } else { 0 };
                let result = Value::binop(op, l, r).map_err(ScriptError::type_err);
                self.exit_op_result(frame, in_bytes, &result);
                return result;
            }
            (l, r) => {
                let l = match l {
                    PreOperand::Owned(v) => Operand::Owned(v),
                    PreOperand::Var(name) => Operand::Borrowed(&self.env[name]),
                };
                let r = match r {
                    PreOperand::Owned(v) => Operand::Owned(v),
                    PreOperand::Var(name) => Operand::Borrowed(&self.env[name]),
                };
                let frame = if frame_names {
                    self.profiler.enter_op(profile_op_name(op, l.get(), r.get()))
                } else {
                    None
                };
                let in_bytes = if frame.is_some() {
                    Self::value_bytes(l.get()) + Self::value_bytes(r.get())
                } else {
                    0
                };
                let result = elementwise::binop(op, l, r).map_err(ScriptError::type_err);
                self.exit_op_result(frame, in_bytes, &result);
                result
            }
        };
        result
    }

    /// `A' * B`, `A * B'` and `A' * B'` (and `.'`). When both operands are
    /// matrices, the product is formed from a transposed view, so the
    /// transpose is never written out: `matmul_conj` reads the view's
    /// strides directly and applies `'`'s conjugation itself (a no-op for a
    /// real operand). Anything else (vectors, or a dimension mismatch, which
    /// then reports `binop`'s error) writes the transposes out and takes the
    /// ordinary path.
    #[inline(never)]
    fn eval_transposed_matmul(&mut self, lhs: &Expr, rhs: &Expr) -> Result<Value, ScriptError> {
        let l = self.matmul_side(lhs)?;
        let r = self.matmul_side(rhs)?;
        // Count stored bytes before the views exist. A real operand that
        // the complex path widens is still 8 bytes per element here.
        let in_bytes = if self.profiler.is_enabled() {
            l.stored_bytes(&self.env) + r.stored_bytes(&self.env)
        } else {
            0
        };
        if let (Some(a), Some(b)) = (l.real_view(&self.env), r.real_view(&self.env)) {
            if a.ncols() == b.nrows() {
                // Real-stored: conjugation does nothing, and the product
                // stays real (the same kernel as the complex path's real
                // case, so the same bits).
                let frame = self.profiler.enter_op("[matmul]");
                let result = Value::real_matrix(rustlab_core::matmul::matmul_real(&a, &b));
                let out_bytes = if frame.is_some() { Self::value_bytes(&result) } else { 0 };
                self.profiler.exit_op(frame, in_bytes, out_bytes);
                return Ok(result);
            }
        }
        if let (Some((a, conj_a)), Some((b, conj_b))) = (l.view(&self.env), r.view(&self.env)) {
            if a.ncols() == b.nrows() {
                let frame = self.profiler.enter_op("[matmul]");
                let result = Value::Matrix(rustlab_core::matmul::matmul_conj(&a, conj_a, &b, conj_b).into());
                let out_bytes = if frame.is_some() { Self::value_bytes(&result) } else { 0 };
                self.profiler.exit_op(frame, in_bytes, out_bytes);
                return Ok(result);
            }
        }
        let l = self.materialize_side(l)?;
        let r = self.materialize_side(r)?;
        self.finish_binop(BinOp::Mul, l, r)
    }

    /// An operand of a product: plain, or a transpose whose inner operand
    /// is evaluated (or named, to borrow) but not yet transposed.
    fn matmul_side<'e>(&mut self, e: &'e Expr) -> Result<MatmulSide<'e>, ScriptError> {
        Ok(match e {
            Expr::Transpose(inner) => MatmulSide::T { inner: self.binop_operand(inner)?, conj: true },
            Expr::NonConjTranspose(inner) => MatmulSide::T { inner: self.binop_operand(inner)?, conj: false },
            _ => MatmulSide::Plain(self.binop_operand(e)?),
        })
    }

    /// Write out a pending transpose, as `eval_transpose` would.
    fn materialize_side<'e>(&mut self, side: MatmulSide<'e>) -> Result<PreOperand<'e>, ScriptError> {
        let (inner, conj) = match side {
            MatmulSide::Plain(p) => return Ok(p),
            MatmulSide::T { inner, conj } => (inner, conj),
        };
        let frame = self.profiler.enter_op("[transpose]");
        let (in_bytes, out_bytes, result) = {
            let v = match &inner {
                PreOperand::Owned(v) => v,
                PreOperand::Var(name) => &self.env[*name],
            };
            let result = if conj { v.transpose_ref() } else { v.non_conj_transpose_ref() };
            let in_bytes = if frame.is_some() { Self::value_bytes(v) } else { 0 };
            let out_bytes = if frame.is_some() { result.as_ref().map_or(0, Self::value_bytes) } else { 0 };
            (in_bytes, out_bytes, result)
        };
        self.profiler.exit_op(frame, in_bytes, out_bytes);
        Ok(PreOperand::Owned(result.map_err(ScriptError::runtime)?))
    }

    /// Evaluate a binary-operator operand. A variable holding a dense array
    /// is not copied: it is returned by name and borrowed once both operands
    /// are ready. Other variables are cloned with a single lookup.
    fn binop_operand<'e>(&mut self, e: &'e Expr) -> Result<PreOperand<'e>, ScriptError> {
        if let Expr::Var(name) = e {
            match self.env.get(name) {
                Some(Value::Vector(_) | Value::Matrix(_)) => return Ok(PreOperand::Var(name)),
                Some(v) => return Ok(PreOperand::Owned(v.clone())),
                None => {}
            }
        }
        Ok(PreOperand::Owned(self.eval_expr(e)?))
    }

    /// `inner'` (conjugate) or `inner.'`. A variable is transposed from a
    /// borrow instead of being copied out of the workspace first.
    #[inline(never)]
    fn eval_transpose(&mut self, inner: &Expr, conj: bool) -> Result<Value, ScriptError> {
        let owned = match inner {
            Expr::Var(name) if self.env.contains_key(name) => None,
            _ => Some(self.eval_expr(inner)?),
        };
        let frame = self.profiler.enter_op("[transpose]");
        // The borrow of a workspace variable ends before `exit_op` mutably
        // uses the profiler.
        let (in_bytes, out_bytes, result) = {
            let v = match (&owned, inner) {
                (Some(v), _) => v,
                (None, Expr::Var(name)) => &self.env[name],
                (None, _) => unreachable!("only a variable is borrowed"),
            };
            let result = if conj { v.transpose_ref() } else { v.non_conj_transpose_ref() };
            let in_bytes = if frame.is_some() { Self::value_bytes(v) } else { 0 };
            let out_bytes = if frame.is_some() { result.as_ref().map_or(0, Self::value_bytes) } else { 0 };
            (in_bytes, out_bytes, result)
        };
        self.profiler.exit_op(frame, in_bytes, out_bytes);
        result.map_err(ScriptError::runtime)
    }

    /// Borrow the value named by a variable or a chain of struct field
    /// reads on one (`S`, `S.a`, `S.a.b`) without copying it. `None` for any
    /// other expression, or when a link is missing or not a struct; callers
    /// then fall back to evaluating, which also produces the right error.
    fn borrow_field_chain(&self, e: &Expr) -> Option<&Value> {
        match e {
            Expr::Var(name) => self.env.get(name),
            Expr::Field { object, field } => match self.borrow_field_chain(object)? {
                Value::Struct(fields) => fields.get(field.as_str()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Build the template Evaluator that `parmap` workers are cloned from.
    ///
    /// A worker needs the user-defined functions, the builtins and the
    /// session settings, but not the caller's variables: a lambda brings
    /// its own captured values and swaps them in at call time, and a user
    /// function runs in a fresh scope seeded only with the built-in
    /// constants. So the template's `env` keeps just those constants.
    /// Copying the whole caller environment per task was the dominant
    /// parmap overhead when it held large matrices (perf flywheel
    /// iteration 01, R3).
    ///
    /// `@feval` is the one callable that reads the current environment
    /// (it can name a lambda held in a variable), so for it the full
    /// environment is kept.
    pub(crate) fn clone_for_parallel_lambda(&self, func: &Value) -> Evaluator {
        let keep_env = matches!(func, Value::FuncHandle(name) if name == "feval");
        let env = if keep_env {
            self.env.clone()
        } else {
            self.env
                .iter()
                .filter(|(k, _)| line_io::BUILTIN_CONSTS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        Evaluator {
            env,
            builtins: self.builtins.clone(),
            user_fns: self.user_fns.clone(),
            in_function: self.in_function,
            stmt_call_nargout0: false,
            profiler: self.profiler.for_worker(),
            color_output: self.color_output,
            number_format: self.number_format,
            current_line: self.current_line,
            cache_registry: self.cache_registry.clone(),
            cached_entry_ids: self.cached_entry_ids.clone(),
            cancel: self.cancel.clone(),
            widget_values: self.widget_values.clone(),
            widget_reads: BTreeSet::new(),
        }
    }

    /// Close an operator frame with `in_bytes` and the stored size of `result`.
    fn exit_op_result(&mut self, frame: profile::OpFrame, in_bytes: u64, result: &Result<Value, ScriptError>) {
        let out_bytes = if frame.is_some() { result.as_ref().map_or(0, Self::value_bytes) } else { 0 };
        self.profiler.exit_op(frame, in_bytes, out_bytes);
    }

    /// Approximate byte size of a Value for IO throughput accounting.
    /// Only numeric types are counted; strings, structs, etc. return 0.
    fn value_bytes(v: &Value) -> u64 {
        match v {
            Value::Scalar(_) => 8,
            Value::Complex(_) => 16,
            Value::Vector(v) => Shared::stored_bytes(v) as u64,
            Value::Matrix(m) => Shared::stored_bytes(m) as u64,
            _ => 0,
        }
    }

    // ── Persistent function-result cache (Phase 3c) ───────────────────
    //
    // Each `cache ...` statement lands here from `exec_stmt_kind`. The
    // registry (`self.cache_registry`) is the source of truth for the
    // active store, scope set, and counters; this layer wires the
    // parsed AST to the registry's lifecycle methods, runs the purity
    // walks at file-load time, and prints any user-visible output.

    fn exec_cache_stmt(&mut self, c: &CacheStmt) -> Result<(), ScriptError> {
        match c {
            CacheStmt::Enable { path } => self.cache_enable(path.as_deref()),
            CacheStmt::Off => {
                self.cache_registry.off();
                Ok(())
            }
            CacheStmt::AddFile { path } => self.cache_add_file(path),
            CacheStmt::AddFunctions { names } => self.cache_add_functions(names),
            CacheStmt::RemoveFunction { name } => {
                self.cache_registry.remove_function(name);
                Ok(())
            }
            CacheStmt::Status => {
                let text = self.cache_registry.status_text();
                output::script_print(text.trim_end_matches('\n'));
                Ok(())
            }
            CacheStmt::Clear => self.cache_clear(),
            CacheStmt::Prune {
                older,
                max_size_bytes,
            } => self.cache_prune(older.as_deref(), *max_size_bytes),
            CacheStmt::List { limit } => self.cache_list(*limit),
        }
    }

    fn cache_enable(&mut self, path: Option<&str>) -> Result<(), ScriptError> {
        let path = match path {
            Some(p) => p,
            None => crate::cache_registry::DEFAULT_STORE_PATH,
        };
        let path = crate::path_jail::check_path(path).map_err(ScriptError::runtime)?;
        self.cache_registry
            .enable(Some(path))
            .map_err(|e| ScriptError::runtime(format!("cache enable: {e}")))?;
        // Echo the resolved store path. Until this was added, `cache
        // enable` ran silently — which made a typo like `cache list`
        // (silently misparsed as `cache enable list` via the bareword
        // sugar) invisible. Now the user sees the actual file that
        // got opened.
        if let Some(p) = self.cache_registry.store_path() {
            output::script_println(&format!(
                "cache: active ({})",
                crate::path_jail::display_path(p)
            ));
        }
        // Scan every already-defined user function for purity. Without
        // this, a script that defines `noisy()` *before* `cache enable`
        // would let `noisy()` cache under `all_scope`. Capture the
        // fingerprintable inputs up front so we can drop `self`'s
        // borrows before mutating the registry.
        let snapshots: Vec<(String, Vec<String>, Vec<Stmt>)> = self
            .user_fns
            .values()
            .map(|f| (f.name.clone(), f.params.clone(), f.body.clone()))
            .collect();
        for (name, params, body) in &snapshots {
            self.cache_gate_user_fn(name, params, body);
        }
        Ok(())
    }

    /// Phase 6a gate. With the cache active, refuse to route calls to
    /// `name` through the cache if its body fails the purity contract:
    /// any unbound name (free var) or call to an impure builtin marks
    /// the function as removed from scope. The function itself is
    /// untouched — it still runs normally when called; only the
    /// dispatcher's `is_in_scope` check changes.
    ///
    /// No-op when the cache is off so the FunctionDef arm pays nothing
    /// on the common cold-path. When the cache is on, the cost is one
    /// AST walk per definition, which is dwarfed by the parse cost.
    fn cache_gate_user_fn(&mut self, name: &str, params: &[String], body: &[Stmt]) {
        if !self.cache_registry.is_active() {
            return;
        }
        // Snapshot the set of sibling names so the closures don't
        // hold a borrow of `self` while we later mutate the registry.
        let sibling_names: std::collections::BTreeSet<String> =
            self.user_fns.keys().cloned().collect();
        let free = crate::purity::check_free_vars(
            body,
            params,
            &|n: &str| self.builtins.contains(n),
            &|n: &str| sibling_names.contains(n),
        );
        if !free.is_clean() {
            self.cache_registry.counters_mut().free_var_skips += 1;
            self.cache_registry.remove_function(name);
            return;
        }
        // Transitive impurity check (Phase 6c): catches `f → g →
        // rand` where g is an already-defined user function. Helpers
        // defined *after* the caller are opaque to this walk; the
        // caller will be re-checked next time it's redefined or on
        // the next `cache enable` sweep.
        let bodies: std::collections::BTreeMap<String, Vec<Stmt>> = self
            .user_fns
            .iter()
            .map(|(k, f)| (k.clone(), f.body.clone()))
            .collect();
        let imp = crate::purity::check_transitive_impurity(body, &|n: &str| {
            bodies.get(n).map(|v| v.as_slice())
        });
        if !imp.is_clean() {
            self.cache_registry.counters_mut().impurity_skips += 1;
            self.cache_registry.remove_function(name);
        }
    }

    /// Phase 7 (Option 3): compute and memoize a function's canonical
    /// entry id. The walker is rename-invariant for params, locals,
    /// return vars, and the function's own name; transitively folds
    /// in every called sibling user-fn so editing a callee invalidates
    /// every caller. Builtin names stay verbatim (external anchors).
    ///
    /// Memoized in `self.cached_entry_ids`; the table is cleared
    /// wholesale on every `FunctionDef` so a redefinition forces
    /// re-computation of every caller.
    fn canonical_entry_id_for(&mut self, func: &UserFn) -> [u8; 32] {
        if let Some(&id) = self.cached_entry_ids.get(&func.name) {
            return id;
        }
        // Snapshot every user fn body so the walker can recurse into
        // sibling callees without borrow conflicts. Cloning is
        // necessary because the canonical walker reads bodies while
        // we'd otherwise hold a mutable borrow on `self` for the
        // memo. The snapshot cost is amortised by the memo — the
        // first call to any function pays it, subsequent calls hit
        // the cache.
        let snapshot: std::collections::BTreeMap<String, crate::ast_hash::CanonicalFnSnapshot> =
            self.user_fns
                .iter()
                .map(|(k, f)| {
                    (
                        k.clone(),
                        crate::ast_hash::CanonicalFnSnapshot {
                            params: f.params.clone(),
                            return_vars: f.return_vars.clone(),
                            body: f.body.clone(),
                        },
                    )
                })
                .collect();
        // Snapshot the builtin set into a HashSet so the closure
        // doesn't borrow self.
        let builtin_names: std::collections::HashSet<String> =
            self.builtins.all_names().into_iter().collect();
        let is_builtin = |n: &str| builtin_names.contains(n);
        let id = crate::ast_hash::canonical_entry_id(
            &func.name,
            &func.params,
            &func.return_vars,
            &func.body,
            &snapshot,
            &is_builtin,
        );
        self.cached_entry_ids.insert(func.name.clone(), id);
        id
    }

    /// Walk every name currently marked removed and try the gate
    /// again. A name re-enters scope when its previously-failing
    /// free-var or impurity check now passes — typically because a
    /// helper has just been defined or one was redefined as pure.
    /// Counters are not double-counted: `restore_removed` is the only
    /// state change on success, and re-failures stay in the removed
    /// set with their original counter bump intact.
    fn cache_rescan_removed(&mut self) {
        if !self.cache_registry.is_active() {
            return;
        }
        let candidates = self.cache_registry.removed_fns_snapshot();
        // Snapshot bodies up front so the gate's closures and the
        // mutable cache_registry calls don't fight the borrow checker.
        let user_fn_names: std::collections::BTreeSet<String> =
            self.user_fns.keys().cloned().collect();
        let bodies: std::collections::BTreeMap<String, (Vec<String>, Vec<Stmt>)> = self
            .user_fns
            .iter()
            .map(|(k, f)| (k.clone(), (f.params.clone(), f.body.clone())))
            .collect();
        for name in candidates {
            let Some((params, body)) = bodies.get(&name) else {
                continue; // user_fns no longer holds this name
            };
            let free = crate::purity::check_free_vars(
                body,
                params,
                &|n: &str| self.builtins.contains(n),
                &|n: &str| user_fn_names.contains(n),
            );
            if !free.is_clean() {
                continue;
            }
            let imp = crate::purity::check_transitive_impurity(body, &|n: &str| {
                bodies.get(n).map(|(_, b)| b.as_slice())
            });
            if !imp.is_clean() {
                continue;
            }
            // Now clean. Lift the removal so the dispatcher will
            // route calls through the cache next time.
            self.cache_registry.restore_removed(&name);
        }
    }

    fn cache_add_file(&mut self, path: &str) -> Result<(), ScriptError> {
        if !self.cache_registry.is_active() {
            return Err(ScriptError::runtime(
                "cache add file: no active store — run `cache enable` first".to_string(),
            ));
        }
        let path_buf = crate::path_jail::check_path(path).map_err(ScriptError::runtime)?;
        let canonical = std::fs::canonicalize(&path_buf)
            .map_err(|e| ScriptError::runtime(format!("cache add file: {path}: {e}")))?;
        let stmts = crate::parse_file(&canonical)?;
        let file_hash = crate::ast_hash::hash_stmts(&stmts);

        // Pre-compute the set of function names defined in this file
        // so the free-variable walker can recognise sibling calls.
        let sibling_names: BTreeSet<String> = stmts
            .iter()
            .filter_map(|s| match &s.kind {
                StmtKind::FunctionDef { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        if sibling_names.is_empty() {
            return Err(ScriptError::runtime(format!(
                "cache add file: {path}: no function definitions found"
            )));
        }

        let mut registered: usize = 0;
        let mut skipped: Vec<(String, String)> = Vec::new();

        for stmt in &stmts {
            let StmtKind::FunctionDef {
                name,
                params,
                return_vars,
                body,
            } = &stmt.kind
            else {
                // Non-FunctionDef top-level statements in a cached
                // file are deliberately ignored. The file is treated
                // as a library of definitions; running its top-level
                // side effects would surprise users who source the
                // same file twice.
                continue;
            };

            // Free-variable check is a hard error at file-load time —
            // capturing the wrong outer name would silently corrupt
            // every cached call to the function.
            let free = crate::purity::check_free_vars(
                body,
                params,
                &|n: &str| self.builtins.contains(n),
                &|n: &str| sibling_names.contains(n),
            );
            if !free.is_clean() {
                return Err(ScriptError::runtime(format!(
                    "cache add file: {path}: function '{name}' references unbound symbols: {} — only pure functions can be cached",
                    free.free_vars.join(", "),
                )));
            }

            // Impurity check is a silent skip — `cache add file` is a
            // batch operation and one impure helper shouldn't block
            // the rest of the file from loading. The function is
            // still installed so it can be called normally; only the
            // cache-registration step is skipped. Marking the name
            // as removed prevents the default `all_scope` from
            // re-picking the impure function up at dispatch time.
            let imp = crate::purity::check_impurity(body);
            if !imp.is_clean() {
                self.cache_registry.counters_mut().impurity_skips += 1;
                skipped.push((
                    name.clone(),
                    format!("calls impure builtins: {}", imp.impure_calls.join(", ")),
                ));
                self.user_fns.insert(
                    name.clone(),
                    UserFn {
                        name: name.clone(),
                        params: params.clone(),
                        return_vars: return_vars.clone(),
                        body: body.clone(),
                    },
                );
                self.cache_registry.remove_function(name);
                continue;
            }

            self.user_fns.insert(
                name.clone(),
                UserFn {
                    name: name.clone(),
                    params: params.clone(),
                    return_vars: return_vars.clone(),
                    body: body.clone(),
                },
            );
            self.cache_registry
                .register_file_function(name.clone(), file_hash, canonical.clone());
            registered += 1;
        }

        // `cache` lifecycle messages bypass `echo_enabled()` — they
        // describe an explicit user action, not an auto-printed
        // expression result. The notebook context would otherwise
        // hide them, which makes the operation feel like a silent
        // no-op.
        let mut msg = format!("cache: loaded {registered} function(s) from {path}");
        for (n, reason) in &skipped {
            msg.push_str(&format!("\n  skipped {n}: {reason}"));
        }
        output::script_println(&msg);
        Ok(())
    }

    fn cache_add_functions(&mut self, names: &[String]) -> Result<(), ScriptError> {
        if !self.cache_registry.is_active() {
            return Err(ScriptError::runtime(
                "cache add function: no active store — run `cache enable` first".to_string(),
            ));
        }
        // Validate every name first so a failure mid-list doesn't
        // leave half-registered scope.
        for name in names {
            let user_fn = self.user_fns.get(name).cloned().ok_or_else(|| {
                ScriptError::runtime(format!(
                    "cache add function: '{name}' is not a user-defined function"
                ))
            })?;

            let free = crate::purity::check_free_vars(
                &user_fn.body,
                &user_fn.params,
                &|n: &str| self.builtins.contains(n),
                &|n: &str| self.user_fns.contains_key(n),
            );
            if !free.is_clean() {
                return Err(ScriptError::runtime(format!(
                    "cache add function: '{name}' references unbound symbols: {}",
                    free.free_vars.join(", "),
                )));
            }

            // Explicit-mode policy: impurity is a hard error here
            // (the user named this function specifically).
            let imp = crate::purity::check_impurity(&user_fn.body);
            if !imp.is_clean() {
                return Err(ScriptError::runtime(format!(
                    "cache add function: '{name}' calls impure builtins ({}) — cannot cache",
                    imp.impure_calls.join(", "),
                )));
            }
        }
        self.cache_registry.add_functions(names.iter().cloned());
        Ok(())
    }

    fn cache_list(&mut self, limit: Option<u64>) -> Result<(), ScriptError> {
        let store = match self.cache_registry.store() {
            Some(s) => s,
            None => {
                return Err(ScriptError::runtime(
                    "cache list: no active store — run `cache enable` first".to_string(),
                ));
            }
        };
        let rows = store
            .list_entries(limit.map(|n| n as usize))
            .map_err(|e| ScriptError::runtime(format!("cache list: {e}")))?;
        if rows.is_empty() {
            let path = self
                .cache_registry
                .store_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<active store>".to_string());
            output::script_println(&format!("cache: no entries in {path}"));
            return Ok(());
        }

        // Index currently-loaded user fns by entry_id (the same hash
        // the dispatcher uses) so each list row can be tagged with
        // its load status. We also index by name → entry_id so we
        // can detect *variants*: a cached row whose name matches a
        // loaded function but whose entry_id doesn't is from an
        // earlier body version that's been edited.
        //
        // Cloning the user_fns map into a local Vec sidesteps the
        // borrow conflict between the iterator and the mutable call
        // to canonical_entry_id_for.
        let mut loaded_by_id: std::collections::BTreeMap<[u8; 32], String> =
            std::collections::BTreeMap::new();
        let mut loaded_by_name: std::collections::BTreeMap<String, [u8; 32]> =
            std::collections::BTreeMap::new();
        let fn_list: Vec<UserFn> = self.user_fns.values().cloned().collect();
        for f in &fn_list {
            let id = self.canonical_entry_id_for(f);
            loaded_by_id.insert(id, f.name.clone());
            loaded_by_name.insert(f.name.clone(), id);
        }

        let mut out = String::new();
        out.push_str(&format!(
            "{:<20}  {:>9}  {:>10}  {:>8}  {:>8}  {}\n",
            "fn name", "entry_id", "input_hash", "bytes", "rl_ver", "status",
        ));
        for row in rows {
            let displayed_name = row
                .fn_name
                .clone()
                .unwrap_or_else(|| "<unknown>".to_string());
            // Status logic:
            //   loaded            — entry_id matches a currently-defined fn
            //   loaded (variant)  — fn_name matches a defined fn, but the
            //                       loaded body's entry_id differs (stale)
            //   not loaded        — no defined fn carries this name
            //   <unknown>         — pre-metadata row; can't say either way
            let status = match &row.fn_name {
                None => "<unknown>".to_string(),
                Some(name) => {
                    if loaded_by_id.contains_key(&row.entry_id) {
                        "loaded".to_string()
                    } else if loaded_by_name.contains_key(name) {
                        "loaded (variant)".to_string()
                    } else {
                        "not loaded".to_string()
                    }
                }
            };
            out.push_str(&format!(
                "{:<20}  {:>9}  {:>10}  {:>8}  {:>8}  {}\n",
                displayed_name,
                row.entry_id_short,
                row.input_hash_short,
                row.bytes,
                row.rustlab_version,
                status,
            ));
        }
        output::script_print(out.trim_end_matches('\n'));
        Ok(())
    }

    fn cache_clear(&mut self) -> Result<(), ScriptError> {
        if !self.cache_registry.is_active() {
            return Err(ScriptError::runtime(
                "cache clear: no active store".to_string(),
            ));
        }
        let removed = self
            .cache_registry
            .clear()
            .map_err(|e| ScriptError::runtime(format!("cache clear: {e}")))?;
        // Bypass echo_enabled — see comment in cache_add_file.
        output::script_println(&format!("cache: cleared {removed} entries"));
        Ok(())
    }

    fn cache_prune(
        &mut self,
        older: Option<&str>,
        max_size_bytes: Option<u64>,
    ) -> Result<(), ScriptError> {
        if !self.cache_registry.is_active() {
            return Err(ScriptError::runtime(
                "cache prune: no active store".to_string(),
            ));
        }
        // Order of operations when both kwargs are present: age-based
        // first, then size-based. That way `older=` clears stale rows
        // before `max_size=` has to evict newer ones to make room.
        let mut total_removed: usize = 0;
        let mut messages: Vec<String> = Vec::new();
        let did_specify = older.is_some() || max_size_bytes.is_some();
        if let Some(s) = older {
            let secs = rustlab_cache::parse_duration_secs(s)
                .map_err(|e| ScriptError::runtime(format!("cache prune: {e}")))?;
            let n = self
                .cache_registry
                .prune_older(Some(secs))
                .map_err(|e| ScriptError::runtime(format!("cache prune: {e}")))?;
            total_removed += n;
            messages.push(format!("{n} older than {secs}s"));
        }
        if let Some(max) = max_size_bytes {
            let n = self
                .cache_registry
                .prune_to_max_size(max)
                .map_err(|e| ScriptError::runtime(format!("cache prune: {e}")))?;
            total_removed += n;
            messages.push(format!("{n} to fit max_size={max}"));
        }
        if !did_specify {
            // No kwargs → age-based default of 30 days per the
            // locked design.
            let n = self
                .cache_registry
                .prune_older(None)
                .map_err(|e| ScriptError::runtime(format!("cache prune: {e}")))?;
            total_removed += n;
            messages.push(format!("{n} older than 30 days (default)"));
        }
        // Bypass echo_enabled — see comment in cache_add_file.
        output::script_println(&format!(
            "cache: pruned {total_removed} entries ({})",
            messages.join(", "),
        ));
        Ok(())
    }
}

/// Shape a user function's user-facing return value from its
/// canonical output set (one slot per declared return var) given the
/// caller's `nargout`. Used by both the body-execution path and the
/// cache-replay path so the error messages and edge-case semantics
/// are identical regardless of where the canonical came from.
///
/// Rules:
/// - `nargout == 0`, OR a function that declares zero outputs → return
///   `Value::None`.
/// - `nargout > declared` → loud error (matches the body-path
///   behaviour for an over-asking caller).
/// - `nargout == 1` → return `canonical[0]` even if it's `Value::None`
///   (back-compat: a 1-output function whose body never assigned its
///   return var has always returned `None` rather than erroring).
/// - `nargout >= 2` → first `nargout` slots must each be assigned
///   (i.e. not `Value::None`), else error naming the unassigned
///   output; return them as a `Value::Tuple`.
fn shape_user_fn_return(
    canonical: &[Value],
    nargout: usize,
    fn_name: &str,
    return_vars: &[String],
) -> Result<Value, ScriptError> {
    let declared = canonical.len();
    if declared == 0 || nargout == 0 {
        return Ok(Value::None);
    }
    if nargout > declared {
        return Err(ScriptError::runtime(format!(
            "function '{}' declares {} output(s), but caller asked for {}",
            fn_name, declared, nargout
        )));
    }
    if nargout == 1 {
        return Ok(canonical[0].clone());
    }
    let mut vals: Vec<Value> = Vec::with_capacity(nargout);
    for (i, v) in canonical.iter().take(nargout).enumerate() {
        if matches!(v, Value::None) {
            return Err(ScriptError::runtime(format!(
                "function '{}': output '{}' was not assigned in the body",
                fn_name, return_vars[i],
            )));
        }
        vals.push(v.clone());
    }
    Ok(Value::Tuple(vals))
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

fn one_based_index(n: f64) -> Result<usize, ScriptError> {
    if !n.is_finite() || n.fract() != 0.0 || n < 1.0 {
        return Err(ScriptError::runtime(format!(
            "index assignment: index {n} is invalid (must be a positive integer)"
        )));
    }
    Ok((n as usize).saturating_sub(1))
}

fn logical_mask_positions(v: &[C64], dim_len: usize) -> Option<Vec<usize>> {
    if v.len() != dim_len {
        return None;
    }
    if v.iter()
        .any(|c| c.im.abs() > 1e-12 || (c.re != 0.0 && c.re != 1.0))
    {
        return None;
    }
    Some(
        v.iter()
            .enumerate()
            .filter(|(_, c)| c.re == 1.0)
            .map(|(i, _)| i)
            .collect(),
    )
}

fn matrix_linear_positions(idx: &Value, nr: usize, nc: usize) -> Result<Vec<usize>, ScriptError> {
    let total = nr * nc;
    match idx {
        Value::All => Ok((0..total).collect()),
        Value::Vector(v) => {
            if let Some(pos) = logical_mask_positions(v.as_slice().unwrap_or(&[]), total) {
                // `as_slice` fails on a non-standard layout; fall through to
                // an owned copy in that case. Vectors from the evaluator are
                // contiguous, so this is the normal path.
                if v.as_slice().is_some() {
                    return Ok(pos);
                }
            }
            if v.as_slice().is_none() {
                let owned: Vec<C64> = v.iter().copied().collect();
                if let Some(pos) = logical_mask_positions(&owned, total) {
                    return Ok(pos);
                }
                return owned.iter().map(|c| one_based_index(c.re)).collect();
            }
            v.iter().map(|c| one_based_index(c.re)).collect()
        }
        Value::Matrix(m) => {
            let same_shape_logical = m.nrows() == nr
                && m.ncols() == nc
                && m.iter()
                    .all(|c| c.im.abs() <= 1e-12 && (c.re == 0.0 || c.re == 1.0));
            if same_shape_logical {
                let mut pos = Vec::new();
                for col in 0..nc {
                    for row in 0..nr {
                        if m[[row, col]].re == 1.0 {
                            pos.push(col * nr + row);
                        }
                    }
                }
                return Ok(pos);
            }
            if m.nrows() > 1 && m.ncols() > 1 {
                return Err(ScriptError::runtime(
                    "index assignment: a matrix index must be a logical mask of the same shape \
                     or a row/column of numeric indices"
                        .to_string(),
                ));
            }
            let mut flat = Vec::with_capacity(m.len());
            for col in 0..m.ncols() {
                for row in 0..m.nrows() {
                    flat.push(m[[row, col]]);
                }
            }
            if let Some(pos) = logical_mask_positions(&flat, total) {
                return Ok(pos);
            }
            flat.iter().map(|c| one_based_index(c.re)).collect()
        }
        other => Err(ScriptError::runtime(format!(
            "index assignment: invalid index type {}",
            other.type_name()
        ))),
    }
}

fn grow_oned_matrix(m: &mut Array2<C64>, new_len: usize) {
    let (nr, nc) = m.dim();
    if nr <= 1 {
        let mut grown = Array2::zeros((1, new_len));
        if nr == 1 {
            for c in 0..nc.min(new_len) {
                grown[[0, c]] = m[[0, c]];
            }
        }
        *m = grown;
    } else {
        let mut grown = Array2::zeros((new_len, 1));
        for r in 0..nr.min(new_len) {
            grown[[r, 0]] = m[[r, 0]];
        }
        *m = grown;
    }
}

fn rhs_for_linear_assign(val: &Value, n: usize) -> Result<Vec<C64>, ScriptError> {
    let flat = match val {
        Value::Scalar(s) => return Ok(vec![Complex::new(*s, 0.0); n]),
        Value::Complex(c) => return Ok(vec![*c; n]),
        Value::Vector(v) => v.iter().copied().collect::<Vec<_>>(),
        Value::Matrix(m) => {
            let mut data = Vec::with_capacity(m.len());
            for col in 0..m.ncols() {
                for row in 0..m.nrows() {
                    data.push(m[[row, col]]);
                }
            }
            data
        }
        other => return Err(ScriptError::runtime(format!(
            "index assignment: right-hand side must be scalar, complex, vector, or matrix; got {}",
            other.type_name()
        ))),
    };
    if flat.len() != n {
        return Err(ScriptError::runtime(format!(
            "index assignment: RHS length {} does not match target index count {n}",
            flat.len()
        )));
    }
    Ok(flat)
}

/// Brent's method (Numerical Recipes). `fa` and `fb` have opposite signs.
fn brent_root(
    f: &mut dyn FnMut(f64) -> Result<f64, ScriptError>,
    mut a: f64,
    mut b: f64,
    mut fa: f64,
    mut fb: f64,
    tol: f64,
) -> Result<f64, ScriptError> {
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }
    let mut c = a;
    let mut fc = fa;
    let mut d = b - a;
    let mut e = d;
    for _ in 0..100 {
        if (fb > 0.0 && fc > 0.0) || (fb < 0.0 && fc < 0.0) {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol_x = 2.0 * f64::EPSILON * b.abs() + tol;
        let m = 0.5 * (c - b);
        if m.abs() <= tol_x || fb == 0.0 {
            return Ok(b);
        }
        if e.abs() < tol_x || fa.abs() <= fb.abs() {
            d = m;
            e = m;
        } else {
            let s = fb / fa;
            let (mut p, mut q) = if a == c {
                (2.0 * m * s, 1.0 - s)
            } else {
                let q = fa / fc;
                let r = fb / fc;
                (
                    s * (2.0 * m * q * (q - r) - (b - a) * (r - 1.0)),
                    (q - 1.0) * (r - 1.0) * (s - 1.0),
                )
            };
            if p > 0.0 {
                q = -q;
            } else {
                p = -p;
            }
            let cond = 2.0 * p < (3.0 * m * q - (tol_x * q).abs()).min((e * q).abs());
            if cond {
                e = d;
                d = p / q;
            } else {
                d = m;
                e = m;
            }
        }
        a = b;
        fa = fb;
        if d.abs() > tol_x {
            b += d;
        } else {
            b += tol_x.copysign(m);
        }
        fb = f(b)?;
    }
    Ok(b)
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
/// Builtins that receive integer arguments *without* widening to double —
/// either because they must inspect the raw integer (casts / introspection)
/// or because they intentionally preserve the integer class (see
/// `dev/plans/integer_types.md`). Every other builtin gets widened integers.
const INT_AWARE_BUILTINS: &[&str] = &[
    // Casts + introspection (must see the raw integer / class).
    "int8",
    "int16",
    "int32",
    "int64",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "class",
    "cast",
    "intmax",
    "intmin",
    "isinteger",
    "isa",
    "double",
    // Class-preserving element ops.
    "abs",
    "real",
    "imag",
    "conj",
    "transpose",
    // Storage / passthrough — must keep the integer class, not a widened copy
    // (save writes the native NPY dtype; struct stores fields verbatim).
    "save",
    "struct",
    // Printing. f64 cannot hold every uint64, so widening first rounds
    // 2^64-1 to 18446744073709552000 and drops the class tag.
    "disp",
    "print",
    "fprintf",
    "sprintf",
];

const IMPURE_BUILTINS: &[&str] = &[
    "clf",
    "contour",
    "contourf",
    "dbcont",
    "dbquit",
    "fgetl",
    "figure",
    "figure_close",
    "figure_draw",
    "figure_live",
    "fprintf",
    "frame",
    "grid",
    "hold",
    "imagesc",
    "input",
    "keyboard",
    "legend",
    "loglog",
    "plot",
    "plot_labels",
    "plot_limits",
    "plot_update",
    "plotdb",
    "polar",
    "quiver",
    "rng",
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

// Compile-time assertion that Evaluator and Value are Send + Sync.
// `parmap` depends on this: rayon workers carry per-thread `Evaluator`
// clones, and captured-env / argument / result `Value`s cross thread
// boundaries. The anonymous const is type-checked even though nothing
// calls it, so a future non-Send variant (`Cell`, `Rc`, a raw pointer)
// fails this crate instead of the first cross-thread use.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<Evaluator>();
    assert_send::<Value>();
    assert_sync::<Evaluator>();
    assert_sync::<Value>();
};

/// Profiler row for a binary operator: `[matmul]` for a matrix product
/// (always recorded while profiling), otherwise the operator in brackets
/// (recorded only with op profiling on).
fn profile_op_name(op: BinOp, l: &Value, r: &Value) -> &'static str {
    use BinOp::*;
    let matrix_like = |v: &Value| matches!(v, Value::Matrix(_) | Value::Vector(_));
    match op {
        Mul if matches!(l, Value::Matrix(_)) && matrix_like(r)
            || matches!(r, Value::Matrix(_)) && matrix_like(l) =>
        {
            "[matmul]"
        }
        Add => "[+]",
        Sub => "[-]",
        Mul => "[*]",
        Div => "[/]",
        Pow => "[^]",
        ElemMul => "[.*]",
        ElemDiv => "[./]",
        ElemPow => "[.^]",
        Eq => "[==]",
        Ne => "[~=]",
        Lt => "[<]",
        Le => "[<=]",
        Gt => "[>]",
        Ge => "[>=]",
        And => "[&&]",
        Or => "[||]",
        LeftDiv => "[\\]",
    }
}

/// Shape facts an index expression needs to bind `end`, read from a borrow
/// of the container before its subscripts are evaluated.
struct IndexShape {
    /// `(m, n, p)` for a Tensor3.
    tensor: Option<(usize, usize, usize)>,
    /// `(rows, cols)` for the two-subscript form; `None` where that form
    /// does not apply.
    rows_cols: Option<(usize, usize)>,
    /// Matrix or sparse matrix: `end` per axis even with one row.
    matrix_like: bool,
    /// Element count for the one-subscript form.
    len: Option<usize>,
}

impl IndexShape {
    fn of(v: &Value) -> Self {
        let tensor = match v {
            Value::Tensor3(t) => {
                let s = t.shape();
                Some((s[0], s[1], s[2]))
            }
            _ => None,
        };
        let rows_cols = match v {
            Value::Matrix(m) => {
                let s = Shared::shape(m);
                Some((s[0], s[1]))
            }
            Value::SparseMatrix(sm) => Some((sm.rows, sm.cols)),
            Value::Vector(v) => Some((1, Shared::len(v))),
            Value::SparseVector(sv) => Some((1, sv.len)),
            Value::IntArray { rows, cols, .. } => Some((*rows, *cols)),
            _ => None,
        };
        let len = match v {
            Value::Vector(v) => Some(Shared::len(v)),
            // Linear index: `end` is the element count, matching the write
            // path (`IndexAssign`) and MATLAB.
            Value::Matrix(m) => Some(Shared::len(m)),
            Value::SparseVector(sv) => Some(sv.len),
            Value::SparseMatrix(sm) => Some(sm.rows * sm.cols),
            // Single-index tensor access is linear over the column-major flat
            // view, so `end` is the total element count (T(:) / T(k)).
            Value::Tensor3(t) => Some(t.len()),
            Value::Tuple(t) => Some(t.len()),
            Value::Str(s) => Some(s.chars().count()),
            Value::StringArray(v) => Some(v.len()),
            Value::IntArray { data, .. } => Some(data.len()),
            _ => None,
        };
        IndexShape {
            tensor,
            rows_cols,
            matrix_like: matches!(v, Value::SparseMatrix(_) | Value::Matrix(_)),
            len,
        }
    }
}

/// A binary-operator operand before borrowing: an evaluated value, or the
/// name of a workspace array to borrow.
enum PreOperand<'e> {
    Owned(Value),
    Var(&'e str),
}

/// One operand of a product, for [`Evaluator::eval_transposed_matmul`].
enum MatmulSide<'e> {
    Plain(PreOperand<'e>),
    /// `inner'` (`conj`) or `inner.'`, not yet transposed.
    T { inner: PreOperand<'e>, conj: bool },
}

impl MatmulSide<'_> {
    /// Stored bytes of the matrix this operand reads. A transpose does not
    /// change the element count, so `A'` counts the same as `A`.
    fn stored_bytes(&self, env: &fast_hash::FxHashMap<String, Value>) -> u64 {
        let p = match self {
            MatmulSide::Plain(p) | MatmulSide::T { inner: p, .. } => p,
        };
        let v = match p {
            PreOperand::Owned(v) => v,
            PreOperand::Var(name) => &env[*name],
        };
        Evaluator::value_bytes(v)
    }

    /// The real matrix this operand stands for, transposed or not, when it
    /// is a real-stored matrix.
    fn real_view<'a>(&'a self, env: &'a fast_hash::FxHashMap<String, Value>) -> Option<ndarray::ArrayView2<'a, f64>> {
        let get = |p: &'a PreOperand| match p {
            PreOperand::Owned(v) => v,
            PreOperand::Var(name) => &env[*name],
        };
        match self {
            MatmulSide::Plain(p) => match get(p) {
                Value::Matrix(m) => Shared::as_real(m).map(|r| r.view()),
                _ => None,
            },
            MatmulSide::T { inner, .. } => match get(inner) {
                Value::Matrix(m) => Shared::as_real(m).map(|r| r.t()),
                _ => None,
            },
        }
    }

    /// The matrix this operand stands for, as a view plus whether its
    /// elements are to be conjugated, when it is a matrix.
    fn view<'a>(&'a self, env: &'a fast_hash::FxHashMap<String, Value>) -> Option<(ndarray::ArrayView2<'a, C64>, bool)> {
        let get = |p: &'a PreOperand| match p {
            PreOperand::Owned(v) => v,
            PreOperand::Var(name) => &env[*name],
        };
        match self {
            MatmulSide::Plain(p) => match get(p) {
                Value::Matrix(m) => Some((m.view(), false)),
                _ => None,
            },
            MatmulSide::T { inner, conj } => match get(inner) {
                Value::Matrix(m) => Some((m.t(), *conj)),
                _ => None,
            },
        }
    }
}

fn is_transpose_expr(e: &Expr) -> bool {
    matches!(e, Expr::Transpose(_) | Expr::NonConjTranspose(_))
}

fn is_dense(v: &Value) -> bool {
    matches!(v, Value::Vector(_) | Value::Matrix(_))
}

#[cfg(test)]
mod plot_abort_tests {
    use super::Evaluator;
    use crate::error::ScriptError;
    use crate::eval::value::Value;
    use crate::lexer;
    use crate::parser;

    fn run_src(ev: &mut Evaluator, src: &str) -> Result<(), ScriptError> {
        let tokens = lexer::tokenize(src).expect("lex");
        let stmts = parser::parse(tokens).expect("parse");
        ev.run(&stmts)
    }

    #[test]
    fn sticky_plot_abort_stops_before_the_next_statement() {
        rustlab_plot::note_plot_aborted();
        let mut ev = Evaluator::new();
        let err = run_src(&mut ev, "x = 1\n").expect_err("abort");
        assert!(matches!(err, ScriptError::Interrupted));
        assert!(ev.get("x").is_none());

        run_src(&mut ev, "x = 1\n").expect("next statement runs");
        match ev.get("x") {
            Some(Value::Scalar(v)) => assert_eq!(*v, 1.0),
            other => panic!("expected scalar 1, got {other:?}"),
        }
    }
}

//! Parallel map (`parmap`) backend abstraction + local rayon implementation.
//!
//! Phase 2 of `dev/plans/parmap_parreduce.md`. The user-facing `parmap(f, xs)`
//! dispatches through the [`ParmapBackend`] trait, which lets the local
//! shared-memory rayon implementation in this module and a future
//! `rustlab-server` cluster implementation (Phase 6, deferred) plug into the
//! same call site without changes to user scripts.
//!
//! ## Pure-lambda contract (deferred to Phase 3)
//!
//! Phase 2 ships the parallel orchestration only. Phase 3 layers on top:
//!  - Per-task RNG seeding from a master seed for deterministic Monte Carlo.
//!  - Runtime enforcement that the lambda doesn't touch global state
//!    (plotting, file I/O, audio, FIR streaming, live figures).
//!
//! Calling `parmap` from this version with an impure lambda will Just Work
//! but produce undefined results — the user is on their own for purity
//! until Phase 3 ships. That's deliberate: Phase 2 establishes the wiring;
//! Phase 3 makes it safe.

use crate::error::ScriptError;
use crate::eval::profile::FnStats;
use crate::eval::shared::Shared;
use crate::eval::value::Value;
use crate::Evaluator;
use rayon::prelude::*;
use std::cell::Cell;
use std::collections::HashMap;

/// What a [`ParmapBackend`] returns: the per-element results in input
/// order, plus the profiler rows each task recorded (empty unless
/// profiling is on), for the caller to merge.
pub struct ParmapOutput {
    pub values: Vec<Value>,
    pub profiles: Vec<HashMap<String, FnStats>>,
}

// ─── Pure-lambda contract ───────────────────────────────────────────────────
//
// `parmap` guarantees that lambdas it invokes run in parallel across rayon
// worker threads. For that to be safe, the lambda body must NOT touch
// global mutable state — no plotting, no file I/O, no audio writes, no
// FIR streaming state mutation, no live-figure handles.
//
// We enforce this at runtime: a thread-local `PARALLEL_CONTEXT` flag is
// set on each rayon worker thread for the duration of a `parmap` task.
// Impure builtins call [`require_pure_context`] at their entry point;
// inside parmap they error with a clear message naming both `parmap` and
// the offending builtin. Outside parmap the check is a no-op.
//
// This is a "hard error, not warning" design (per the plan's decision 6):
// silent-wrong is much worse than loud-fail for parallel-correctness bugs.

thread_local! {
    static PARALLEL_CONTEXT: Cell<bool> = const { Cell::new(false) };
}

/// Guard that sets `PARALLEL_CONTEXT = true` for its lifetime and restores
/// the previous value on drop. Used by the rayon worker tasks to mark the
/// span where the user's lambda is running.
struct ParallelContextGuard {
    previous: bool,
}

impl ParallelContextGuard {
    fn enter() -> Self {
        let previous = PARALLEL_CONTEXT.with(|c| c.replace(true));
        Self { previous }
    }
}

impl Drop for ParallelContextGuard {
    fn drop(&mut self) {
        PARALLEL_CONTEXT.with(|c| c.set(self.previous));
    }
}

/// Installs the calling thread's path jail and plot context for one
/// worker task, then restores whatever that worker thread had before.
///
/// Both are thread-locals, so a rayon worker does not see them unless
/// they are copied in. Restoring (including `None` for the jail) means a
/// reused worker cannot keep a notebook jail after the task, and a later
/// unjailed `parmap` cannot inherit a stale root.
struct WorkerScope {
    _jail: crate::path_jail::PathJailGuard,
    plot_prev: rustlab_plot::PlotContext,
}

impl WorkerScope {
    fn enter(jail: Option<crate::path_jail::PathJail>, plot: rustlab_plot::PlotContext) -> Self {
        let plot_prev = rustlab_plot::plot_context();
        rustlab_plot::set_plot_context(plot);
        Self {
            _jail: crate::path_jail::PathJailGuard::install(jail),
            plot_prev,
        }
    }
}

impl Drop for WorkerScope {
    fn drop(&mut self) {
        rustlab_plot::set_plot_context(self.plot_prev);
    }
}

/// Seeds this thread's RNG for one task and puts back the thread's previous
/// RNG and master seed when dropped. A task can run on the calling thread
/// (a nested `parmap`, or a script run inside a rayon pool); without the
/// restore, it would replace the caller's `seed(N)` state.
struct TaskRng(crate::eval::rng::RngSnapshot);

impl TaskRng {
    fn enter(master_seed: u64, idx: usize) -> Self {
        use crate::eval::rng;
        let prev = rng::capture();
        rng::seed_rng(rng::derive_task_seed(master_seed, idx));
        Self(prev)
    }
}

impl Drop for TaskRng {
    fn drop(&mut self) {
        crate::eval::rng::restore(&self.0);
    }
}

/// Returns `Err(...)` if the current thread is inside a `parmap` worker
/// task (i.e. running a parallel-lambda body); otherwise returns `Ok(())`.
///
/// Impure builtins call this at their entry point. The message names both
/// `parmap` and the offending builtin so users get an actionable error:
///
/// ```text
/// parmap: cannot clf from a parallel lambda — the lambda must be pure
/// ```
/// Whether this thread is running a `parmap` task.
pub fn in_parallel_context() -> bool {
    PARALLEL_CONTEXT.with(|c| c.get())
}

pub fn require_pure_context(builtin_name: &str) -> Result<(), ScriptError> {
    PARALLEL_CONTEXT.with(|c| {
        if c.get() {
            Err(ScriptError::runtime(format!(
                "parmap: cannot {builtin_name} from a parallel lambda — the lambda must be pure"
            )))
        } else {
            Ok(())
        }
    })
}

/// Validate that a `Value` is a callable accepted by `parmap` / `parsum`
/// (lambda or function handle). `caller` names the function in the error.
pub fn validate_callable(caller: &str, v: &Value) -> Result<(), ScriptError> {
    match v {
        Value::Lambda { .. } | Value::FuncHandle(_) => Ok(()),
        other => Err(ScriptError::type_err(format!(
            "{caller}: first argument must be a lambda or function handle, got {}",
            other.type_name()
        ))),
    }
}

/// Backend strategy for parallel map. The local shared-memory backend
/// (in this file) uses rayon's thread pool. Phase 6 (deferred) will add
/// a `rustlab-server` cluster backend that implements this same trait
/// — user scripts won't change; only the backend selection differs.
pub trait ParmapBackend: Send + Sync {
    /// Invoke `callable(x)` for each `x` in `xs` in parallel. Returns the
    /// per-element results in input order, or the first error encountered
    /// (cancel-and-propagate semantics, matching `for`-loop convention).
    ///
    /// The `worker_factory` produces an `Evaluator` for each worker the
    /// backend uses (one per rayon split for the local backend, reused
    /// across the tasks in that split; the
    /// future cluster backend ignores it — workers are remote processes
    /// already configured with the user's function library).
    ///
    /// `master_seed` is reserved for Phase 3 per-task RNG seeding. Phase 2
    /// accepts it but doesn't use it yet.
    fn run(
        &self,
        worker_factory: &(dyn Fn() -> Evaluator + Send + Sync),
        callable: Value,
        xs: Vec<Value>,
        master_seed: u64,
    ) -> Result<ParmapOutput, ScriptError>;
}

/// Shared-memory rayon backend. The only implementation shipped in Phase 2.
pub struct LocalRayonBackend;

impl LocalRayonBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LocalRayonBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ParmapBackend for LocalRayonBackend {
    fn run(
        &self,
        worker_factory: &(dyn Fn() -> Evaluator + Send + Sync),
        callable: Value,
        xs: Vec<Value>,
        master_seed: u64,
    ) -> Result<ParmapOutput, ScriptError> {
        // `map_init` builds one worker Evaluator per rayon split and reuses
        // it for every task in that split. Reuse is safe: a lambda call
        // swaps its captured env in and restores the worker's env after,
        // and a user-function call does the same with a fresh scope, so no
        // task sees state left by an earlier one. The template carries no
        // caller variables (`clone_for_parallel_lambda`), so a worker is
        // cheap to build whatever the caller's environment holds. The
        // callable is still cloned per task, because a lambda call takes
        // ownership of its captured env.
        //
        // Per-task RNG: each task seeds the worker thread's thread-local
        // RNG with a deterministic mix of (master_seed, task_index) before
        // calling the lambda. That gives Monte Carlo determinism — the
        // same `seed(N); parmap(...)` produces bit-identical results
        // across runs — without disturbing the calling thread's master
        // RNG. Each task restores the thread's previous RNG when it ends,
        // so the calling thread's state is unchanged even when a task runs
        // on it (nested `parmap`, or a script run inside a rayon pool).
        // Captured on the calling thread. Workers install these per task.
        let jail = crate::path_jail::path_jail();
        let plot = rustlab_plot::plot_context();
        let results: Vec<(Result<Value, ScriptError>, HashMap<String, FnStats>)> = xs
            .into_par_iter()
            .enumerate()
            .map_init(worker_factory, |worker, (idx, x)| {
                // Same path jail and plot context as the caller, for this
                // task only. Nested `parmap` re-captures whatever this
                // task installed.
                let _scope = WorkerScope::enter(jail.clone(), plot);
                // Per-task RNG seed; the thread's own RNG comes back after.
                let _rng = TaskRng::enter(master_seed, idx);
                // Mark this thread as inside a parmap worker — impure
                // builtins (clf, fprintf, savefig, etc.) will hard-error
                // if invoked during the lambda body. The guard restores
                // the flag when it drops at end-of-task scope.
                let _guard = ParallelContextGuard::enter();
                let r = worker.call_callable(callable.clone(), vec![x]);
                (r, worker.take_profile_stats())
            })
            .collect();

        // Cancel + propagate: return the first Err with its position in
        // the input. Matches `for`-loop semantics; matches what users
        // intuit from MATLAB / Octave precedents.
        let mut out = Vec::with_capacity(results.len());
        let mut profiles = Vec::new();
        for (idx, (r, stats)) in results.into_iter().enumerate() {
            if !stats.is_empty() {
                profiles.push(stats);
            }
            match r {
                Ok(v) => out.push(v),
                Err(e) => {
                    return Err(ScriptError::runtime(format!(
                        "parmap: trial {} of {} errored: {}",
                        idx + 1,
                        out.len() + 1, // total = handled-so-far + this one + (unprocessed)
                        e
                    )));
                }
            }
        }
        Ok(ParmapOutput {
            values: out,
            profiles,
        })
    }
}

// ─── Reducing parmap: `parmap(f, xs, "sum")` / `parsum(f, xs)` ─────────────
//
// The trials are summed as a fixed binary tree over trial indices:
//
//     S(lo, hi) = S(lo, mid) + S(mid, hi),   mid = lo + ceil((hi - lo) / 2)
//
// with the earlier trials on the left and a single trial as a leaf. For
// B = 8 that is ((r1 + r2) + (r3 + r4)) + ((r5 + r6) + (r7 + r8)); for
// B = 5 it is ((r1 + r2) + r3) + (r4 + r5). The tree depends only on B, so
// every element of the result has the same bits whatever the thread count
// or the order in which workers finish. Threads change only which subtrees
// run at the same time: subtrees of at most `grain` trials run serially on
// one worker Evaluator, larger ones split with `rayon::join`. A node is
// added as soon as both children are done and the children are dropped, so
// the B results are never held together. Each add is in place into a
// child's buffer when that buffer is not shared; long arrays add in
// parallel chunks, which does not change any element's bits.

/// What [`sum_reduce`] returns: the sum, plus the profiler rows each task
/// recorded (in trial order, empty unless profiling is on).
pub struct ParsumOutput {
    pub value: Value,
    pub profiles: Vec<HashMap<String, FnStats>>,
}

/// Elements per parallel chunk when adding two long arrays.
const SUM_CHUNK: usize = 1 << 15;

struct SumCtx<'a> {
    caller: &'a str,
    factory: &'a (dyn Fn() -> Evaluator + Send + Sync),
    callable: &'a Value,
    xs: &'a [Value],
    master_seed: u64,
    jail: Option<crate::path_jail::PathJail>,
    plot: rustlab_plot::PlotContext,
    grain: usize,
    profiles: std::sync::Mutex<Vec<(usize, HashMap<String, FnStats>)>>,
}

/// A subtree's sum and the index of its first trial (for error messages).
struct Partial {
    first: usize,
    value: Value,
}

/// Sum `callable(x)` over `xs` in the fixed tree order described above.
/// Per-task RNG seeds, the path jail, the plot context, and the pure-lambda
/// contract are the same as [`LocalRayonBackend::run`]. When several
/// trials fail, the error of the earliest one is returned. No trials sum
/// to the scalar 0.
pub fn sum_reduce(
    caller: &str,
    worker_factory: &(dyn Fn() -> Evaluator + Send + Sync),
    callable: Value,
    xs: Vec<Value>,
    master_seed: u64,
) -> Result<ParsumOutput, ScriptError> {
    let n = xs.len();
    if n == 0 {
        return Ok(ParsumOutput {
            value: Value::Scalar(0.0),
            profiles: Vec::new(),
        });
    }
    let threads = rayon::current_num_threads().max(1);
    let ctx = SumCtx {
        caller,
        factory: worker_factory,
        callable: &callable,
        xs: &xs,
        master_seed,
        jail: crate::path_jail::path_jail(),
        plot: rustlab_plot::plot_context(),
        grain: n.div_ceil(4 * threads).max(1),
        profiles: std::sync::Mutex::new(Vec::new()),
    };
    let result = sum_tree(&ctx, 0, n);
    let mut profiles = ctx.profiles.into_inner().unwrap_or_else(|e| e.into_inner());
    profiles.sort_by_key(|(idx, _)| *idx);
    let value = result?.value;
    Ok(ParsumOutput {
        value,
        profiles: profiles.into_iter().map(|(_, s)| s).collect(),
    })
}

fn split(lo: usize, hi: usize) -> usize {
    lo + (hi - lo).div_ceil(2)
}

fn sum_tree(ctx: &SumCtx, lo: usize, hi: usize) -> Result<Partial, ScriptError> {
    if hi - lo <= ctx.grain {
        let mut worker = (ctx.factory)();
        return sum_serial(ctx, &mut worker, lo, hi);
    }
    let mid = split(lo, hi);
    let (left, right) = rayon::join(|| sum_tree(ctx, lo, mid), || sum_tree(ctx, mid, hi));
    add_partials(ctx.caller, left?, right?)
}

fn sum_serial(
    ctx: &SumCtx,
    worker: &mut Evaluator,
    lo: usize,
    hi: usize,
) -> Result<Partial, ScriptError> {
    if hi - lo == 1 {
        return sum_leaf(ctx, worker, lo);
    }
    let mid = split(lo, hi);
    let left = sum_serial(ctx, worker, lo, mid)?;
    let right = sum_serial(ctx, worker, mid, hi)?;
    add_partials(ctx.caller, left, right)
}

fn sum_leaf(ctx: &SumCtx, worker: &mut Evaluator, idx: usize) -> Result<Partial, ScriptError> {
    let r = {
        let _scope = WorkerScope::enter(ctx.jail.clone(), ctx.plot);
        let _rng = TaskRng::enter(ctx.master_seed, idx);
        let _guard = ParallelContextGuard::enter();
        worker.call_callable(ctx.callable.clone(), vec![ctx.xs[idx].clone()])
    };
    let stats = worker.take_profile_stats();
    if !stats.is_empty() {
        if let Ok(mut p) = ctx.profiles.lock() {
            p.push((idx, stats));
        }
    }
    let value = r.map_err(|e| {
        ScriptError::runtime(format!(
            "{}: trial {} of {} errored: {}",
            ctx.caller,
            idx + 1,
            ctx.xs.len(),
            e
        ))
    })?;
    let value = match value {
        Value::Bool(b) => Value::Scalar(if b { 1.0 } else { 0.0 }),
        v @ (Value::Scalar(_)
        | Value::Complex(_)
        | Value::Vector(_)
        | Value::Matrix(_)
        | Value::Tensor3(_)) => v,
        other => {
            return Err(ScriptError::type_err(format!(
                "{}: trial {} returned {}; a summed trial must return a scalar, \
                 vector, or matrix",
                ctx.caller,
                idx + 1,
                other.type_name()
            )))
        }
    };
    Ok(Partial { first: idx, value })
}

fn describe(v: &Value) -> String {
    match v {
        Value::Vector(a) => format!("vector of length {}", Shared::len(a)),
        Value::Matrix(m) => {
            let s = Shared::shape(m);
            format!("matrix of shape {}×{}", s[0], s[1])
        }
        Value::Tensor3(t) => {
            let s = Shared::shape(t);
            format!("tensor3 of shape {}×{}×{}", s[0], s[1], s[2])
        }
        _ => "a scalar".to_string(),
    }
}

/// `left + right`, element by element. `left` holds the earlier trials.
fn add_partials(caller: &str, left: Partial, right: Partial) -> Result<Partial, ScriptError> {
    use num_complex::Complex;
    let first = left.first;
    let mismatch = |l: &Value, r: &Value| {
        ScriptError::type_err(format!(
            "{caller}: trial {} returned {} but trial {} returned {}; \
             all trials must return the same shape",
            right.first + 1,
            describe(r),
            left.first + 1,
            describe(l),
        ))
    };
    let value = match (left.value, right.value) {
        (Value::Scalar(a), Value::Scalar(b)) => Value::Scalar(a + b),
        (
            a @ (Value::Scalar(_) | Value::Complex(_)),
            b @ (Value::Scalar(_) | Value::Complex(_)),
        ) => {
            let c = |v: Value| match v {
                Value::Scalar(x) => Complex::new(x, 0.0),
                Value::Complex(z) => z,
                _ => unreachable!("matched above"),
            };
            Value::Complex(c(a) + c(b))
        }
        (Value::Vector(a), Value::Vector(b)) if Shared::shape(&a) == Shared::shape(&b) => {
            Value::Vector(add_shared(a, b))
        }
        (Value::Matrix(a), Value::Matrix(b)) if Shared::shape(&a) == Shared::shape(&b) => {
            Value::Matrix(add_shared(a, b))
        }
        (Value::Tensor3(a), Value::Tensor3(b)) if Shared::shape(&a) == Shared::shape(&b) => {
            Value::Tensor3(add_shared(a, b))
        }
        (l, r) => return Err(mismatch(&l, &r)),
    };
    Ok(Partial { first, value })
}

/// Same-shape add. The result is written into whichever operand's buffer
/// is not shared (IEEE addition commutes, so the bits do not depend on
/// which), real-stored when both operands are.
fn add_shared<D: ndarray::Dimension>(
    a: Shared<ndarray::Array<num_complex::Complex<f64>, D>>,
    b: Shared<ndarray::Array<num_complex::Complex<f64>, D>>,
) -> Shared<ndarray::Array<num_complex::Complex<f64>, D>> {
    let (mut acc, other) = if !Shared::is_unique(&a) && Shared::is_unique(&b) {
        (b, a)
    } else {
        (a, b)
    };
    if Shared::is_real(&acc) && Shared::is_real(&other) {
        let src = Shared::as_real(&other).expect("checked real");
        let dst = Shared::make_mut_real(&mut acc).expect("checked real");
        add_into(dst, src);
    } else {
        let dst = Shared::make_mut(&mut acc);
        add_into(dst, &other);
    }
    acc
}

fn add_into<T, D>(dst: &mut ndarray::Array<T, D>, src: &ndarray::Array<T, D>)
where
    T: Copy + Send + Sync + std::ops::AddAssign,
    D: ndarray::Dimension,
{
    if let (Some(s), true) = (src.as_slice(), dst.is_standard_layout()) {
        let d = dst.as_slice_mut().expect("standard layout");
        if d.len() >= crate::eval::elementwise::PAR_MIN_LEN {
            d.par_chunks_mut(SUM_CHUNK)
                .zip(s.par_chunks(SUM_CHUNK))
                .for_each(|(d, s)| d.iter_mut().zip(s).for_each(|(x, y)| *x += *y));
        } else {
            d.iter_mut().zip(s).for_each(|(x, y)| *x += *y);
        }
    } else {
        dst.zip_mut_with(src, |x, y| *x += *y);
    }
}

/// Pack the per-element results into a single `Value`. Output shape is
/// decided from the first result and every subsequent result must match:
/// scalar/complex/bool → `Vector` of complex; vector of length `d` →
/// `(N, d)` Matrix (per-call index = row, matching `arrayfun`); matrix
/// of shape `m × n` → `Tensor3` of shape `(m, n, N)` (per-call index =
/// trailing pages axis, so `result(:, :, k)` extracts the k-th
/// per-call matrix). Mixed shapes hard-error with the divergent index
/// named.
pub fn pack_results(results: Vec<Value>) -> Result<Value, ScriptError> {
    if results.is_empty() {
        return Ok(Value::Vector(ndarray::Array1::zeros(0).into()));
    }
    let results: Vec<Value> = results.into_iter().map(Value::column_into_vector).collect();
    enum Shape {
        ScalarLike,
        Vector(usize),
        Matrix(usize, usize),
    }
    let shape = match &results[0] {
        Value::Scalar(_) | Value::Complex(_) | Value::Bool(_) => Shape::ScalarLike,
        Value::Vector(v) => Shape::Vector(Shared::len(v)),
        Value::Matrix(m) => Shape::Matrix(Shared::shape(m)[0], Shared::shape(m)[1]),
        other => {
            return Err(ScriptError::type_err(format!(
                "parmap: lambda return type {} is not supported \
                 (expected scalar, complex, bool, vector, or matrix)",
                other.type_name()
            )));
        }
    };
    match shape {
        Shape::ScalarLike => pack_scalar_like(results),
        Shape::Vector(d) => pack_vectors_as_matrix(results, d),
        Shape::Matrix(rows, cols) => pack_matrices_as_tensor3(results, rows, cols),
    }
}

fn pack_scalar_like(results: Vec<Value>) -> Result<Value, ScriptError> {
    // Real scalars and bools only: a real vector of the same values.
    if results
        .iter()
        .all(|v| matches!(v, Value::Scalar(_) | Value::Bool(_)))
    {
        let out = results
            .iter()
            .map(|v| match v {
                Value::Scalar(x) => *x,
                Value::Bool(b) => {
                    if *b {
                        1.0
                    } else {
                        0.0
                    }
                }
                _ => unreachable!("checked above"),
            })
            .collect();
        return Ok(Value::real_vector(ndarray::Array1::from_vec(out)));
    }
    let mut out: Vec<num_complex::Complex<f64>> = Vec::with_capacity(results.len());
    for (idx, v) in results.into_iter().enumerate() {
        match v {
            Value::Scalar(x) => out.push(num_complex::Complex::new(x, 0.0)),
            Value::Complex(c) => out.push(c),
            Value::Bool(b) => out.push(num_complex::Complex::new(if b { 1.0 } else { 0.0 }, 0.0)),
            other => {
                return Err(ScriptError::type_err(format!(
                    "parmap: trial {} returned {} but trial 1 returned a scalar; \
                     all trials must return the same shape",
                    idx + 1,
                    other.type_name()
                )))
            }
        }
    }
    Ok(Value::Vector(ndarray::Array1::from_vec(out).into()))
}

fn pack_vectors_as_matrix(results: Vec<Value>, row_len: usize) -> Result<Value, ScriptError> {
    // Validate every row first, so the parallel copy below cannot fail.
    for (idx, v) in results.iter().enumerate() {
        match v {
            Value::Vector(vec) if Shared::len(vec) == row_len => {}
            Value::Vector(vec) => {
                return Err(ScriptError::type_err(format!(
                    "parmap: trial {} returned vector of length {} but trial 1 \
                     returned vector of length {}; all trials must return the same shape",
                    idx + 1,
                    Shared::len(vec),
                    row_len
                )));
            }
            other => {
                return Err(ScriptError::type_err(format!(
                    "parmap: trial {} returned {} but trial 1 returned a vector \
                     of length {}; all trials must return the same shape",
                    idx + 1,
                    other.type_name(),
                    row_len
                )))
            }
        }
    }
    if let Some(m) = pack_real_rows(&results, row_len) {
        return Ok(m);
    }
    // One write per element, in parallel, into spare capacity. A row that
    // is not a vector of `row_len` panics before `set_len`, so a missed
    // write is never published. `C64` is `Copy`: a panic drops a len-0
    // `Vec` and does not drop uninitialised slots. This stays a direct
    // write (no zero-fill, no second copy of the ~100 MB gradient rows).
    let nrows = results.len();
    let n = nrows * row_len;
    let mut flat: Vec<num_complex::Complex<f64>> = Vec::with_capacity(n);
    if n > 0 {
        let dst = &mut flat.spare_capacity_mut()[..n];
        dst.par_chunks_mut(row_len)
            .zip(results.par_iter())
            .for_each(|(chunk, v)| {
                let Value::Vector(vec) = v else {
                    panic!("parmap: row changed type after validation");
                };
                assert_eq!(
                    vec.len(),
                    chunk.len(),
                    "parmap: row fill stopped short of {} elements",
                    chunk.len()
                );
                for (d, s) in chunk.iter_mut().zip(vec.iter()) {
                    d.write(*s);
                }
            });
        // SAFETY: each chunk has length `row_len` and was zipped with a
        // vector of that length, so every slot was written. The assert
        // above runs before `set_len`. Chunks partition `0..n`.
        unsafe { flat.set_len(n) };
    }
    let m =
        ndarray::Array2::from_shape_vec((nrows, row_len), flat).expect("nrows × row_len elements");
    Ok(Value::Matrix(m.into()))
}

/// [`pack_vectors_as_matrix`] when every row is real-stored: an
/// `nrows × row_len` real matrix, written once per element in parallel.
/// Rows are already validated to be vectors of `row_len`.
fn pack_real_rows(results: &[Value], row_len: usize) -> Option<Value> {
    let rows: Vec<&[f64]> = results
        .iter()
        .map(|v| match v {
            Value::Vector(vec) => Shared::real_slice(vec),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let nrows = rows.len();
    let n = nrows * row_len;
    let mut flat: Vec<f64> = Vec::with_capacity(n);
    if n > 0 {
        let dst = &mut flat.spare_capacity_mut()[..n];
        dst.par_chunks_mut(row_len)
            .zip(rows.par_iter())
            .for_each(|(chunk, row)| {
                assert_eq!(
                    row.len(),
                    chunk.len(),
                    "parmap: row fill stopped short of {} elements",
                    chunk.len()
                );
                for (d, &x) in chunk.iter_mut().zip(row.iter()) {
                    d.write(x);
                }
            });
        // SAFETY: each chunk has length `row_len` and was zipped with a row
        // of that length (asserted first), so every slot was written. The
        // chunks partition `0..n`, and `f64` is `Copy`, so a panic before
        // here drops a len-0 `Vec`.
        unsafe { flat.set_len(n) };
    }
    let m =
        ndarray::Array2::from_shape_vec((nrows, row_len), flat).expect("nrows × row_len elements");
    Some(Value::real_matrix(m))
}

fn pack_matrices_as_tensor3(
    results: Vec<Value>,
    rows: usize,
    cols: usize,
) -> Result<Value, ScriptError> {
    let n_pages = results.len();
    let mut matrices: Vec<ndarray::Array2<num_complex::Complex<f64>>> = Vec::with_capacity(n_pages);
    for (idx, v) in results.into_iter().enumerate() {
        match v {
            Value::Matrix(m) => {
                if m.nrows() != rows || m.ncols() != cols {
                    return Err(ScriptError::type_err(format!(
                        "parmap: trial {} returned matrix of shape {}×{} but trial 1 \
                         returned matrix of shape {}×{}; all trials must return the same shape",
                        idx + 1,
                        m.nrows(),
                        m.ncols(),
                        rows,
                        cols
                    )));
                }
                matrices.push(Shared::into_inner(m));
            }
            other => {
                return Err(ScriptError::type_err(format!(
                    "parmap: trial {} returned {} but trial 1 returned a matrix \
                     of shape {}×{}; all trials must return the same shape",
                    idx + 1,
                    other.type_name(),
                    rows,
                    cols
                )));
            }
        }
    }
    // Layout: per-call index lives on the trailing (pages) axis so
    // `result(:, :, k)` returns the k-th per-call matrix — matches the
    // Tensor3 docstring in eval/value.rs ("rows × cols × pages") and
    // the `cat(3, ...)` page-stacking idiom users already know.
    // Array3::from_shape_vec is row-major, so for shape (rows, cols,
    // n_pages) the linear index for (i, j, k) is
    // i*cols*n_pages + j*n_pages + k.
    let mut flat: Vec<num_complex::Complex<f64>> = Vec::with_capacity(rows * cols * n_pages);
    for i in 0..rows {
        for j in 0..cols {
            for m in &matrices {
                flat.push(m[(i, j)]);
            }
        }
    }
    let t = ndarray::Array3::from_shape_vec((rows, cols, n_pages), flat)
        .map_err(|e| ScriptError::runtime(e.to_string()))?;
    Ok(Value::Tensor3(t.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_empty_results() {
        let v = pack_results(vec![]).unwrap();
        match v {
            Value::Vector(a) => assert_eq!(a.len(), 0),
            _ => panic!("expected empty vector"),
        }
    }

    #[test]
    fn pack_scalar_results() {
        let v = pack_results(vec![
            Value::Scalar(1.0),
            Value::Scalar(2.0),
            Value::Scalar(3.0),
        ])
        .unwrap();
        match v {
            Value::Vector(a) => {
                assert_eq!(a.len(), 3);
                assert_eq!(a[0].re, 1.0);
                assert_eq!(a[2].re, 3.0);
            }
            _ => panic!("expected vector"),
        }
    }

    #[test]
    fn pack_mixed_scalar_then_matrix_errors() {
        let bad = vec![
            Value::Scalar(1.0),
            Value::Matrix(ndarray::Array2::zeros((2, 2)).into()),
        ];
        let err = pack_results(bad).unwrap_err().to_string();
        assert!(err.contains("trial 2"), "msg: {err}");
        assert!(err.contains("same shape"), "msg: {err}");
    }

    fn cvec(xs: &[f64]) -> Value {
        Value::Vector(
            ndarray::Array1::from_iter(xs.iter().map(|&x| num_complex::Complex::new(x, 0.0)))
                .into(),
        )
    }

    fn cmat(rows: usize, cols: usize, fill: f64) -> Value {
        Value::Matrix(
            ndarray::Array2::from_elem((rows, cols), num_complex::Complex::new(fill, 0.0)).into(),
        )
    }

    #[test]
    fn pack_vector_results_become_matrix() {
        let v = pack_results(vec![cvec(&[1.0, 2.0, 3.0]), cvec(&[4.0, 5.0, 6.0])]).unwrap();
        match v {
            Value::Matrix(m) => {
                assert_eq!(m.shape(), &[2, 3]);
                assert_eq!(m[(0, 0)].re, 1.0);
                assert_eq!(m[(0, 2)].re, 3.0);
                assert_eq!(m[(1, 0)].re, 4.0);
                assert_eq!(m[(1, 2)].re, 6.0);
            }
            other => panic!("expected matrix, got {}", other.type_name()),
        }
    }

    #[test]
    fn pack_vector_length_mismatch_errors() {
        let err = pack_results(vec![cvec(&[1.0, 2.0, 3.0]), cvec(&[4.0, 5.0])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("trial 2"), "msg: {err}");
        assert!(err.contains("length 2"), "msg: {err}");
        assert!(err.contains("length 3"), "msg: {err}");
    }

    #[test]
    fn pack_vector_then_scalar_errors() {
        let err = pack_results(vec![cvec(&[1.0, 2.0]), Value::Scalar(7.0)])
            .unwrap_err()
            .to_string();
        assert!(err.contains("trial 2"), "msg: {err}");
        assert!(
            err.contains("scalar") && err.contains("vector"),
            "msg: {err}"
        );
    }

    #[test]
    fn pack_matrix_results_become_tensor3() {
        // Three 2×3 matrices (filled 1.0, 2.0, 3.0 respectively) →
        // Tensor3 of shape (2, 3, 3) with page k entirely == k+1.
        let v = pack_results(vec![cmat(2, 3, 1.0), cmat(2, 3, 2.0), cmat(2, 3, 3.0)]).unwrap();
        match v {
            Value::Tensor3(t) => {
                assert_eq!(t.shape(), &[2, 3, 3]);
                for i in 0..2 {
                    for j in 0..3 {
                        for k in 0..3 {
                            let expected = (k + 1) as f64;
                            assert!(
                                (t[(i, j, k)].re - expected).abs() < 1e-15,
                                "({i},{j},{k}): got {}, expected {}",
                                t[(i, j, k)].re,
                                expected
                            );
                        }
                    }
                }
            }
            other => panic!("expected Tensor3, got {}", other.type_name()),
        }
    }

    #[test]
    fn pack_matrix_shape_mismatch_errors() {
        let err = pack_results(vec![cmat(2, 3, 0.0), cmat(2, 4, 0.0)])
            .unwrap_err()
            .to_string();
        assert!(err.contains("trial 2"), "msg: {err}");
        assert!(err.contains("2×3") && err.contains("2×4"), "msg: {err}");
    }

    #[test]
    fn pack_matrix_then_vector_errors() {
        let err = pack_results(vec![cmat(2, 3, 0.0), cvec(&[1.0, 2.0, 3.0])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("trial 2"), "msg: {err}");
        assert!(
            err.contains("vector") && err.contains("matrix"),
            "msg: {err}"
        );
    }

    #[test]
    fn validate_rejects_non_callable() {
        let err = validate_callable("parmap", &Value::Scalar(42.0)).unwrap_err();
        assert!(err.to_string().contains("lambda or function handle"));
    }

    #[test]
    fn worker_scope_inherits_caller_jail_and_restores() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let _jail = crate::path_jail::PathJailGuard::with_base(Some(root.clone()), Some(root));
        struct PlotRestore(rustlab_plot::PlotContext);
        impl Drop for PlotRestore {
            fn drop(&mut self) {
                rustlab_plot::set_plot_context(self.0);
            }
        }
        let _plot = PlotRestore(rustlab_plot::plot_context());
        rustlab_plot::set_plot_context(rustlab_plot::PlotContext::Headless);
        let jail = crate::path_jail::path_jail();
        let plot = rustlab_plot::plot_context();
        (0..64).into_par_iter().for_each(|_| {
            // Rayon may run a task on this thread, which already holds the
            // jail. Restore must put back that thread's previous state,
            // not unconditionally clear it.
            let before_plot = rustlab_plot::plot_context();
            let before_root = crate::path_jail::path_jail().map(|j| j.root);
            let scope = WorkerScope::enter(jail.clone(), plot);
            assert_eq!(
                rustlab_plot::plot_context(),
                rustlab_plot::PlotContext::Headless
            );
            let installed = crate::path_jail::path_jail().expect("jail installed on worker");
            assert_eq!(installed.root, dir.path());
            drop(scope);
            assert_eq!(rustlab_plot::plot_context(), before_plot);
            assert_eq!(crate::path_jail::path_jail().map(|j| j.root), before_root);
        });
        assert_eq!(
            rustlab_plot::plot_context(),
            rustlab_plot::PlotContext::Headless
        );
        assert_eq!(
            crate::path_jail::path_jail().map(|j| j.root),
            Some(dir.path().to_path_buf())
        );
    }
}

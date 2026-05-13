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
use crate::eval::value::Value;
use crate::Evaluator;
use rayon::prelude::*;
use std::cell::Cell;

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

/// Returns `Err(...)` if the current thread is inside a `parmap` worker
/// task (i.e. running a parallel-lambda body); otherwise returns `Ok(())`.
///
/// Impure builtins call this at their entry point. The message names both
/// `parmap` and the offending builtin so users get an actionable error:
///
/// ```text
/// parmap: cannot clf from a parallel lambda — the lambda must be pure
/// ```
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

/// Validate that a `Value` is a callable accepted by `parmap` (lambda or
/// function handle). Returns the validated value or a `parmap`-specific
/// error message.
pub fn validate_callable(v: &Value) -> Result<(), ScriptError> {
    match v {
        Value::Lambda { .. } | Value::FuncHandle(_) => Ok(()),
        other => Err(ScriptError::type_err(format!(
            "parmap: first argument must be a lambda or function handle, got {}",
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
    /// backend uses (one per rayon task for the local backend's v1; the
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
    ) -> Result<Vec<Value>, ScriptError>;
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
    ) -> Result<Vec<Value>, ScriptError> {
        use crate::eval::rng;

        // par_iter clones the Evaluator AND the callable per element in
        // this v1 — simple and correct. Per-thread caching (one clone
        // per worker thread, shared across the tasks that land on it) is
        // a follow-on optimization; rayon's `map_with` provides the
        // natural API surface. For the gallery-sized parmap calls (up
        // to ~1000 elements) the clone overhead is well under 1 ms total.
        //
        // Per-task RNG: each task seeds the worker thread's thread-local
        // RNG with a deterministic mix of (master_seed, task_index) before
        // calling the lambda. That gives Monte Carlo determinism — the
        // same `seed(N); parmap(...)` produces bit-identical results
        // across runs — without disturbing the calling thread's master
        // RNG. After parmap completes, the calling thread's RNG state is
        // exactly what it was before; worker threads' RNGs are left in
        // whatever state the last task left them, which is fine because
        // each task re-seeds at entry.
        let results: Vec<Result<Value, ScriptError>> = xs
            .into_par_iter()
            .enumerate()
            .map(|(idx, x)| {
                // Install per-task RNG seed.
                rng::seed_rng(rng::derive_task_seed(master_seed, idx));
                // Mark this thread as inside a parmap worker — impure
                // builtins (clf, fprintf, savefig, etc.) will hard-error
                // if invoked during the lambda body. The guard restores
                // the flag when it drops at end-of-task scope.
                let _guard = ParallelContextGuard::enter();
                let mut worker = worker_factory();
                worker.call_callable(callable.clone(), vec![x])
            })
            .collect();

        // Cancel + propagate: return the first Err with its position in
        // the input. Matches `for`-loop semantics; matches what users
        // intuit from MATLAB / Octave precedents.
        let mut out = Vec::with_capacity(results.len());
        for (idx, r) in results.into_iter().enumerate() {
            match r {
                Ok(v) => out.push(v),
                Err(e) => {
                    return Err(ScriptError::runtime(format!(
                        "parmap: trial {} of {} errored: {}",
                        idx + 1,
                        out.len() + 1, // total = handled-so-far + this one + (unprocessed)
                        e
                    )))
                }
            }
        }
        Ok(out)
    }
}

/// Pack the per-element results into a single `Value`. Output shape is
/// decided from the first result and every subsequent result must match:
/// scalar/complex/bool → `Vector` of complex; vector → `(N, d)` Matrix
/// (per-call index = row, matching `arrayfun`); other types still error.
/// Mixed shapes hard-error with the divergent index named.
pub fn pack_results(results: Vec<Value>) -> Result<Value, ScriptError> {
    if results.is_empty() {
        return Ok(Value::Vector(ndarray::Array1::zeros(0)));
    }
    let row_len = match &results[0] {
        Value::Scalar(_) | Value::Complex(_) | Value::Bool(_) => None,
        Value::Vector(v) => Some(v.len()),
        other => {
            return Err(ScriptError::type_err(format!(
                "parmap: lambda return type {} is not supported \
                 (expected scalar, complex, bool, or vector)",
                other.type_name()
            )));
        }
    };
    match row_len {
        None => pack_scalar_like(results),
        Some(d) => pack_vectors_as_matrix(results, d),
    }
}

fn pack_scalar_like(results: Vec<Value>) -> Result<Value, ScriptError> {
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
    Ok(Value::Vector(ndarray::Array1::from_vec(out)))
}

fn pack_vectors_as_matrix(results: Vec<Value>, row_len: usize) -> Result<Value, ScriptError> {
    let nrows = results.len();
    let mut flat: Vec<num_complex::Complex<f64>> = Vec::with_capacity(nrows * row_len);
    for (idx, v) in results.into_iter().enumerate() {
        match v {
            Value::Vector(vec) => {
                if vec.len() != row_len {
                    return Err(ScriptError::type_err(format!(
                        "parmap: trial {} returned vector of length {} but trial 1 \
                         returned vector of length {}; all trials must return the same shape",
                        idx + 1,
                        vec.len(),
                        row_len
                    )));
                }
                flat.extend(vec.iter().copied());
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
    let m = ndarray::Array2::from_shape_vec((nrows, row_len), flat)
        .map_err(|e| ScriptError::runtime(e.to_string()))?;
    Ok(Value::Matrix(m))
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
        let v = pack_results(vec![Value::Scalar(1.0), Value::Scalar(2.0), Value::Scalar(3.0)])
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
        let bad = vec![Value::Scalar(1.0), Value::Matrix(ndarray::Array2::zeros((2, 2)))];
        let err = pack_results(bad).unwrap_err().to_string();
        assert!(err.contains("trial 2"), "msg: {err}");
        assert!(err.contains("same shape"), "msg: {err}");
    }

    #[test]
    fn pack_matrix_first_result_errors() {
        // Phase 2 will lift this — Phase 1 only handles scalar/vector first results.
        let err = pack_results(vec![Value::Matrix(ndarray::Array2::zeros((2, 2)))])
            .unwrap_err()
            .to_string();
        assert!(err.contains("not supported"), "msg: {err}");
    }

    fn cvec(xs: &[f64]) -> Value {
        Value::Vector(ndarray::Array1::from_iter(
            xs.iter().map(|&x| num_complex::Complex::new(x, 0.0)),
        ))
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
        assert!(err.contains("scalar") && err.contains("vector"), "msg: {err}");
    }

    #[test]
    fn validate_rejects_non_callable() {
        let err = validate_callable(&Value::Scalar(42.0)).unwrap_err();
        assert!(err.to_string().contains("lambda or function handle"));
    }
}

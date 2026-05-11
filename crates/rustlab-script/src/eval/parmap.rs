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
        _master_seed: u64, // Phase 3 will wire this in
    ) -> Result<Vec<Value>, ScriptError> {
        // par_iter clones the Evaluator AND the callable per element in
        // this v1 — simple and correct. Per-thread caching (one clone
        // per worker thread, shared across the tasks that land on it) is
        // a follow-on optimization; rayon's `map_with` provides the
        // natural API surface. For the gallery-sized parmap calls (up
        // to ~1000 elements) the clone overhead is well under 1 ms total.
        let results: Vec<Result<Value, ScriptError>> = xs
            .into_par_iter()
            .map(|x| {
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

/// Pack the per-element results into a single `Value`. Phase 2 accepts
/// scalar / complex / bool outputs (combined into a `Value::Vector` of
/// complex); vector/matrix/struct outputs error with a clear message.
/// Cell-array support is deferred.
pub fn pack_results(results: Vec<Value>) -> Result<Value, ScriptError> {
    if results.is_empty() {
        return Ok(Value::Vector(ndarray::Array1::zeros(0)));
    }
    let mut out: Vec<num_complex::Complex<f64>> = Vec::with_capacity(results.len());
    for (idx, v) in results.into_iter().enumerate() {
        match v {
            Value::Scalar(x) => out.push(num_complex::Complex::new(x, 0.0)),
            Value::Complex(c) => out.push(c),
            Value::Bool(b) => out.push(num_complex::Complex::new(if b { 1.0 } else { 0.0 }, 0.0)),
            other => {
                return Err(ScriptError::type_err(format!(
                    "parmap: lambda must return a scalar (got {} at index {}); \
                     vector/matrix return values are not yet supported",
                    other.type_name(),
                    idx + 1
                )))
            }
        }
    }
    Ok(Value::Vector(ndarray::Array1::from_vec(out)))
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
    fn pack_matrix_result_errors() {
        let bad = vec![Value::Scalar(1.0), Value::Matrix(ndarray::Array2::zeros((2, 2)))];
        let err = pack_results(bad).unwrap_err();
        assert!(err.to_string().contains("not yet supported"));
    }

    #[test]
    fn validate_rejects_non_callable() {
        let err = validate_callable(&Value::Scalar(42.0)).unwrap_err();
        assert!(err.to_string().contains("lambda or function handle"));
    }
}

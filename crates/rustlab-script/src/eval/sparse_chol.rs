//! MATLAB's sparse `chol`: `R = chol(S)`, `[R, p] = chol(S)`, and
//! `[R, p, P] = chol(S)` with `"matrix"` / `"vector"` and `"upper"` /
//! `"lower"` (dev/plans/sparse_spd_solve.md, Phase 4).
//!
//! - One output is the upper factor of `S` in its own order (`R' * R = S`)
//!   and errors when `S` is not positive definite.
//! - `[R, p]` does not error: `p = 0`, or the 1-based pivot that failed;
//!   then `R` is `q × n` (`q = p − 1`) and `R' * R` agrees with `S` on the
//!   first `q` rows and columns.
//! - `[R, p, P]` adds a fill-reducing permutation: `R' * R = P' * S * P`
//!   (`"vector"`: `R' * R = S(p, p)` with a `1 × n` row `p`). The ordering
//!   is nested dissection or natural by predicted fill (MATLAB uses AMD);
//!   an ordering name selects another.
//! - Only the upper triangle is read (`"lower"`: the lower triangle, and
//!   the result is `L` with `L * L' = S`). Symmetry is not checked.

use super::builtins::{sparse_all_real, OrderingChoice};
use super::value::Value;
use crate::error::ScriptError;
use num_complex::Complex;
use rustlab_core::sparse_solve::{
    AmdOrdering, IdentityOrdering, NestedDissectionOrdering, PartialChol, SparseChol, SparseCsc,
    SparseScalar, SparseSolveError, SpdAutoOrdering,
};
use rustlab_core::{SparseMat, C64};

/// Which triangle of `S` defines the Hermitian matrix that is factored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Triangle {
    Upper,
    Lower,
}

/// `f64` / `C64` back to the `SparseMat` entry type.
pub(crate) trait ToC64: SparseScalar {
    fn to_c64(self) -> C64;
}

impl ToC64 for f64 {
    fn to_c64(self) -> C64 {
        Complex::new(self, 0.0)
    }
}

impl ToC64 for C64 {
    fn to_c64(self) -> C64 {
        self
    }
}

/// The Hermitian matrix that `chol` factors: the chosen triangle and its
/// conjugate mirror. `S` must be square.
pub(crate) fn hermitian_from_triangle<T: ToC64 + rustlab_core::sparse_solve::FromComplex>(
    sm: &SparseMat,
    tri: Triangle,
) -> SparseCsc<T> {
    let mut coo: Vec<(usize, usize, T)> = Vec::with_capacity(sm.entries.len() * 2);
    for &(i, j, v) in &sm.entries {
        let keep = match tri {
            Triangle::Upper => i <= j,
            Triangle::Lower => i >= j,
        };
        if !keep {
            continue;
        }
        let t = T::from_c64(v, f64::INFINITY).unwrap_or_else(|_| T::zero());
        coo.push((i, j, t));
        if i != j {
            coo.push((j, i, t.conj()));
        }
    }
    // The column scatter keeps rows ascending for row-major input and
    // sorts any column that is not.
    SparseCsc::from_coo_sorted(sm.rows, sm.cols, &coo)
}

/// Factor with the chosen ordering, keeping the partial factor on failure.
pub(crate) fn partial_chol<T: SparseScalar>(
    csc: &SparseCsc<T>,
    ordering: OrderingChoice,
) -> Result<PartialChol<T>, SparseSolveError> {
    match ordering {
        OrderingChoice::Auto => SparseChol::factor_flagged(csc, &SpdAutoOrdering),
        OrderingChoice::Identity => SparseChol::factor_flagged(csc, &IdentityOrdering),
        OrderingChoice::Amd => SparseChol::factor_flagged(csc, &AmdOrdering),
        OrderingChoice::Nd => SparseChol::factor_flagged(csc, &NestedDissectionOrdering),
    }
}

/// A `SparseMat` from entries that are already unique and row-major.
/// Entries of `L` that cancelled to exactly zero are not stored.
fn sparse(rows: usize, cols: usize, mut entries: Vec<(usize, usize, C64)>) -> SparseMat {
    entries.retain(|e| e.2.re != 0.0 || e.2.im != 0.0);
    SparseMat {
        rows,
        cols,
        entries,
        ordering_hint: None,
    }
}

/// `R = L^H` as a `q × n` sparse matrix. `L` is lower with sorted rows,
/// so column `j` of `L` is row `j` of `R`, already in row-major order.
fn upper_from_l<T: ToC64>(l: &SparseCsc<T>) -> SparseMat {
    let mut entries = Vec::with_capacity(l.nnz());
    for j in 0..l.ncols() {
        for (i, v) in l.col_iter(j) {
            entries.push((j, i, v.conj().to_c64()));
        }
    }
    sparse(l.ncols(), l.nrows(), entries)
}

/// `L` itself as an `n × q` sparse matrix (row-major entries).
fn lower_from_l<T: ToC64>(l: &SparseCsc<T>) -> SparseMat {
    let n = l.nrows();
    let mut ptr = vec![0usize; n + 1];
    for &i in &l.row_idx {
        ptr[i + 1] += 1;
    }
    for i in 0..n {
        ptr[i + 1] += ptr[i];
    }
    let mut entries = vec![(0usize, 0usize, Complex::new(0.0, 0.0)); l.nnz()];
    let mut next = ptr[..n].to_vec();
    for j in 0..l.ncols() {
        for (i, v) in l.col_iter(j) {
            entries[next[i]] = (i, j, v.to_c64());
            next[i] += 1;
        }
    }
    sparse(n, l.ncols(), entries)
}

/// `P` with `P(perm[k], k) = 1`, so `P' * S * P = S(perm, perm)`.
fn permutation_matrix(perm: &[usize], inv: &[usize]) -> SparseMat {
    let entries = (0..perm.len())
        .map(|r| (r, inv[r], Complex::new(1.0, 0.0)))
        .collect();
    sparse(perm.len(), perm.len(), entries)
}

/// The factor of one scalar type, turned into script values.
fn outputs<T: ToC64>(
    f: PartialChol<T>,
    tri: Triangle,
    nargout: usize,
    vector: bool,
) -> Result<Value, ScriptError> {
    let (l, perm, failed) = f.into_parts();
    if nargout <= 1 {
        if let Some(k) = failed {
            return Err(ScriptError::type_err(format!(
                "chol: matrix is not positive definite (failed at pivot {})\n  hint: [R, p] = chol(A) returns p instead of an error",
                k + 1
            )));
        }
    }
    let factor = match tri {
        Triangle::Upper => upper_from_l(&l),
        Triangle::Lower => lower_from_l(&l),
    };
    let factor = Value::SparseMatrix(factor);
    if nargout <= 1 {
        return Ok(factor);
    }
    let p = Value::Scalar(failed.map_or(0.0, |k| (k + 1) as f64));
    if nargout == 2 {
        return Ok(Value::Tuple(vec![factor, p]));
    }
    let perm_value = if vector {
        Value::real_vector(ndarray::Array1::from_iter(
            perm.perm().iter().map(|&i| (i + 1) as f64),
        ))
    } else {
        Value::SparseMatrix(permutation_matrix(perm.perm(), perm.inv_perm()))
    };
    Ok(Value::Tuple(vec![factor, p, perm_value]))
}

/// `chol` on a sparse matrix. `args[0]` is the matrix.
pub(crate) fn builtin_chol_sparse(args: &[Value], nargout: usize) -> Result<Value, ScriptError> {
    let sm = match &args[0] {
        Value::SparseMatrix(sm) => sm,
        other => {
            return Err(ScriptError::type_err(format!(
                "chol: A must be a matrix, got {}",
                other.type_name()
            )))
        }
    };
    if nargout > 3 {
        return Err(ScriptError::type_err(
            "chol: returns at most [R, p, P]".to_string(),
        ));
    }
    let mut tri = Triangle::Upper;
    let mut vector = false;
    let mut ordering: Option<(OrderingChoice, String)> = None;
    for a in &args[1..] {
        let s = match a {
            Value::Str(s) => s.as_str(),
            other => return Err(ScriptError::type_err(format!(
                "chol: options are strings (\"upper\", \"lower\", \"vector\", \"matrix\"), got {}",
                other.type_name()
            ))),
        };
        match s.to_ascii_lowercase().as_str() {
            "upper" => tri = Triangle::Upper,
            "lower" => tri = Triangle::Lower,
            "vector" => vector = true,
            "matrix" => vector = false,
            "auto" => ordering = Some((OrderingChoice::Auto, s.to_string())),
            "identity" | "natural" => ordering = Some((OrderingChoice::Identity, s.to_string())),
            "amd" => ordering = Some((OrderingChoice::Amd, s.to_string())),
            "nd" => ordering = Some((OrderingChoice::Nd, s.to_string())),
            _ => {
                return Err(ScriptError::type_err(format!(
                    "chol: unknown option \"{s}\"; expected \"upper\", \"lower\", \"vector\", \"matrix\", or an ordering (\"auto\", \"amd\", \"nd\", \"natural\") with [R, p, P]"
                )))
            }
        }
    }
    let ordering = match ordering {
        Some((choice, _)) if nargout == 3 => choice,
        Some((_, name)) => {
            return Err(ScriptError::type_err(format!(
                "chol: an ordering (\"{name}\") selects the permutation P; use [R, p, P] = chol(A, \"{name}\")"
            )))
        }
        None if nargout == 3 => OrderingChoice::Auto,
        None => OrderingChoice::Identity,
    };
    if sm.rows != sm.cols {
        return Err(ScriptError::type_err(format!(
            "chol: A must be square (got {}×{})",
            sm.rows, sm.cols
        )));
    }
    if sm
        .entries
        .iter()
        .any(|(_, _, v)| !v.re.is_finite() || !v.im.is_finite())
    {
        return Err(ScriptError::type_err(
            "chol: entries must be finite".to_string(),
        ));
    }
    let err = |e: SparseSolveError| ScriptError::type_err(format!("chol: {e}"));
    if sparse_all_real(sm, None) {
        let csc: SparseCsc<f64> = hermitian_from_triangle(sm, tri);
        outputs(
            partial_chol(&csc, ordering).map_err(err)?,
            tri,
            nargout,
            vector,
        )
    } else {
        let csc: SparseCsc<C64> = hermitian_from_triangle(sm, tri);
        outputs(
            partial_chol(&csc, ordering).map_err(err)?,
            tri,
            nargout,
            vector,
        )
    }
}

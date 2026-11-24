//! MATLAB's `decomposition`: factor a matrix once, then `dA \ b` for every
//! right-hand side (dev/plans/sparse_spd_solve.md, Phase 4).
//!
//! `decomposition(A)` picks the factor `\` would use; `"chol"`, `"lu"`, and
//! (dense only) `"qr"` force one. `"upper"` / `"lower"` choose the triangle a
//! `"chol"` reads. Sparse factors are the ones sparse `\` uses
//! (`sparse_auto`), so a real factor takes a complex `b` as two solves.

use super::builtins::{solve_columns, sparse_all_real, sparse_rhs_columns};
use super::linsys::{dense_decomposition, dense_decomposition_solve, DenseDecomp};
use super::sparse_auto::{self, AutoFactor, Mode};
use super::sparse_chol::{hermitian_from_triangle, Triangle};
use super::value::{SparseFactor, Value};
use crate::error::ScriptError;
use rustlab_core::sparse_solve::{SparseChol, SparseCsc, SparseSolveError, SpdAutoOrdering};
use rustlab_core::{SparseMat, C64};
use std::fmt;
use std::sync::Arc;

/// A factored matrix. Cloning the `Value` shares it.
pub struct Decomposition {
    rows: usize,
    cols: usize,
    kind: &'static str,
    inner: Inner,
}

enum Inner {
    Sparse(AutoFactor),
    Dense(DenseDecomp),
}

impl fmt::Debug for Decomposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Decomposition({}×{}, {})",
            self.rows, self.cols, self.kind
        )
    }
}

impl fmt::Display for Decomposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  decomposition with properties:")?;
        writeln!(f)?;
        writeln!(f, "    MatrixSize: [{} {}]", self.rows, self.cols)?;
        write!(f, "          Type: {}", self.kind)
    }
}

impl Decomposition {
    /// `"chol"`, `"lu"`, `"qr"`, `"triangular"`, or `"diagonal"`.
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn size(&self) -> (usize, usize) {
        (self.rows, self.cols)
    }

    pub fn is_sparse(&self) -> bool {
        matches!(self.inner, Inner::Sparse(_))
    }

    /// `dA \ b`: one column gives `n×1`, several give a matrix.
    pub(crate) fn solve(&self, b: &Value) -> Result<Value, String> {
        match &self.inner {
            Inner::Sparse(f) => {
                let columns = sparse_rhs_columns(self.rows, b)?;
                solve_columns(f, &columns).map_err(|e| format!("mldivide: {e}"))
            }
            Inner::Dense(d) => dense_decomposition_solve(d, b),
        }
    }

    /// `dA.Type` and `dA.MatrixSize`.
    pub(crate) fn field(&self, name: &str) -> Result<Value, String> {
        match name {
            "Type" => Ok(Value::Str(self.kind.to_string())),
            "MatrixSize" => Ok(Value::real_vector(ndarray::Array1::from_vec(vec![
                self.rows as f64,
                self.cols as f64,
            ]))),
            other => Err(format!(
                "decomposition has no property '{other}'; properties are MatrixSize and Type"
            )),
        }
    }
}

fn sparse_kind(f: &AutoFactor) -> &'static str {
    match f.kind() {
        "cholesky" | "reduced-cholesky" => "chol",
        other => other,
    }
}

fn not_spd(e: SparseSolveError) -> String {
    match e {
        SparseSolveError::NotSpd { col } => format!(
            "decomposition: matrix is not positive definite (failed at pivot {})",
            col + 1
        ),
        other => format!("decomposition: {other}"),
    }
}

/// `"chol"` on a sparse matrix: the chosen triangle, mirrored, factored in
/// the fill-reducing order `chol`'s `[R, p, P]` uses.
fn sparse_chol(sm: &SparseMat, tri: Triangle) -> Result<AutoFactor, String> {
    let factor = if sparse_all_real(sm, None) {
        let csc: SparseCsc<f64> = hermitian_from_triangle(sm, tri);
        SparseFactor::CholReal(Arc::new(
            SparseChol::factor(&csc, &SpdAutoOrdering).map_err(not_spd)?,
        ))
    } else {
        let csc: SparseCsc<C64> = hermitian_from_triangle(sm, tri);
        SparseFactor::CholComplex(Arc::new(
            SparseChol::factor(&csc, &SpdAutoOrdering).map_err(not_spd)?,
        ))
    };
    Ok(AutoFactor::Plain(factor))
}

fn build_sparse(sm: &SparseMat, kind: &str, tri: Triangle) -> Result<Decomposition, String> {
    if sm.rows != sm.cols {
        return Err(format!(
            "decomposition: a rectangular sparse matrix ({}×{}) is not supported; use decomposition(full(A), \"qr\")",
            sm.rows, sm.cols
        ));
    }
    if sm
        .entries
        .iter()
        .any(|(_, _, v)| !v.re.is_finite() || !v.im.is_finite())
    {
        return Err("decomposition: entries must be finite".to_string());
    }
    let ordering = super::builtins::OrderingChoice::Auto;
    let f = match kind {
        "auto" => sparse_auto::factor(sm, Mode::Auto, ordering)
            .map_err(|e| format!("decomposition: {e}"))?,
        "lu" => sparse_auto::factor(sm, Mode::Lu, ordering)
            .map_err(|e| format!("decomposition: {e}"))?,
        "chol" => sparse_chol(sm, tri)?,
        _ => {
            return Err(format!(
                "decomposition: \"{kind}\" needs a dense matrix; use decomposition(full(A), \"{kind}\")"
            ))
        }
    };
    Ok(Decomposition {
        rows: sm.rows,
        cols: sm.cols,
        kind: sparse_kind(&f),
        inner: Inner::Sparse(f),
    })
}

/// `decomposition(A [, type] [, "upper" | "lower"])`.
pub(crate) fn builtin_decomposition(args: Vec<Value>) -> Result<Value, ScriptError> {
    let err = |m: String| ScriptError::type_err(m);
    if args.is_empty() || args.len() > 3 {
        return Err(err(format!(
            "decomposition: expected (A [, type] [, \"upper\" | \"lower\"]), got {} arguments",
            args.len()
        )));
    }
    let mut kind = "auto";
    let mut tri = Triangle::Upper;
    let mut tri_given = false;
    for a in &args[1..] {
        let s = match a {
            Value::Str(s) => s.to_ascii_lowercase(),
            other => {
                return Err(err(format!(
                    "decomposition: options are strings, got {}",
                    other.type_name()
                )))
            }
        };
        match s.as_str() {
            "auto" => kind = "auto",
            "chol" => kind = "chol",
            "lu" => kind = "lu",
            "qr" => kind = "qr",
            "upper" => {
                tri = Triangle::Upper;
                tri_given = true;
            }
            "lower" => {
                tri = Triangle::Lower;
                tri_given = true;
            }
            _ => {
                return Err(err(format!(
                    "decomposition: unknown type \"{s}\"; expected \"auto\", \"chol\", \"lu\", or \"qr\" (and \"upper\" / \"lower\" with \"chol\")"
                )))
            }
        }
    }
    if tri_given && kind != "chol" {
        return Err(err(
            "decomposition: \"upper\" / \"lower\" go with \"chol\"".to_string()
        ));
    }
    let d = match &args[0] {
        Value::SparseMatrix(sm) => build_sparse(sm, kind, tri),
        Value::Matrix(_) | Value::Vector(_) | Value::Scalar(_) | Value::Complex(_) => {
            dense_decomposition(&args[0], kind, tri == Triangle::Lower).map(|d| Decomposition {
                rows: d.rows,
                cols: d.cols,
                kind: d.kind,
                inner: Inner::Dense(d),
            })
        }
        other => Err(format!(
            "decomposition: A must be a matrix, got {}",
            other.type_name()
        )),
    }
    .map_err(err)?;
    Ok(Value::Decomposition(Arc::new(d)))
}

use crate::ast::{BinOp, Expr};
use ndarray::{Array1, Array2};
use num_complex::Complex;
use rustlab_core::sparse_solve::{SparseChol, SparseLU};
use rustlab_core::{CMatrix, CTensor3, CVector, SparseMat, SparseVec, C64};
use rustlab_dsp::fixed::QFmtSpec;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Cached sparse factorization handle returned by `chol(A)` / `lu(A)` and
/// consumed by `solve(F, b)`. Wraps the underlying `SparseChol`/`SparseLU`
/// in an `Arc` so cloning the `Value` is cheap. Real (`f64`) and complex
/// (`C64`) factor variants are kept separate so the back-solve can route
/// without re-detecting realness on every solve.
#[derive(Debug, Clone)]
pub enum SparseFactor {
    CholReal(Arc<SparseChol<f64>>),
    CholComplex(Arc<SparseChol<C64>>),
    LuReal(Arc<SparseLU<f64>>),
    LuComplex(Arc<SparseLU<C64>>),
}

impl SparseFactor {
    /// "chol" or "lu", for diagnostics and the `Display` impl.
    pub fn kind(&self) -> &'static str {
        match self {
            SparseFactor::CholReal(_) | SparseFactor::CholComplex(_) => "chol",
            SparseFactor::LuReal(_) | SparseFactor::LuComplex(_) => "lu",
        }
    }

    /// "real" or "complex".
    pub fn scalar(&self) -> &'static str {
        match self {
            SparseFactor::CholReal(_) | SparseFactor::LuReal(_) => "real",
            SparseFactor::CholComplex(_) | SparseFactor::LuComplex(_) => "complex",
        }
    }

    /// Dimension `n` of the factored `n×n` matrix.
    pub fn dim(&self) -> usize {
        match self {
            SparseFactor::CholReal(c) => c.factor_csc().nrows(),
            SparseFactor::CholComplex(c) => c.factor_csc().nrows(),
            SparseFactor::LuReal(lu) => lu.l_factor().nrows(),
            SparseFactor::LuComplex(lu) => lu.l_factor().nrows(),
        }
    }

    /// nnz of the factor (sum of L and U for LU; just L for Cholesky).
    pub fn nnz(&self) -> usize {
        match self {
            SparseFactor::CholReal(c) => c.nnz(),
            SparseFactor::CholComplex(c) => c.nnz(),
            SparseFactor::LuReal(lu) => lu.nnz(),
            SparseFactor::LuComplex(lu) => lu.nnz(),
        }
    }
}

/// Numeric display format, set by the `format` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NumberFormat {
    /// Default: 4 decimal places for scalars, 6 for vectors/matrices.
    #[default]
    Short,
    /// Full f64 precision (15 significant digits).
    Long,
    /// IEEE-754 hex encoding of the 64-bit float.
    Hex,
    /// Thousands-separator commas.
    Commas,
}

impl NumberFormat {
    pub fn name(&self) -> &'static str {
        match self {
            NumberFormat::Short => "short",
            NumberFormat::Long => "long",
            NumberFormat::Hex => "hex",
            NumberFormat::Commas => "commas",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Scalar(f64),
    Complex(C64),
    Vector(CVector),
    Matrix(CMatrix),
    /// Rank-3 complex tensor, shape `(m, n, p)` — rows × cols × pages.
    /// 1-based indexing via `A(i, j, k)`; `A(:, :, k)` returns a `Matrix` (drops trailing singleton).
    Tensor3(CTensor3),
    Bool(bool),
    Str(String),
    QFmt(QFmtSpec),
    /// Key-value struct: `s.x`, created with `struct("x", 1, "y", 2)` or `s.x = 1`
    Struct(HashMap<String, Value>),
    /// Multiple return values from builtins; consumed by `[a, b] = f()` assignment.
    /// Should never appear as a standalone value in the environment.
    Tuple(Vec<Value>),
    /// Sentinel for `:` used as an index ("all elements in this dimension").
    /// Only meaningful inside indexing expressions; errors elsewhere.
    All,
    None,
    /// Continuous-time transfer function G(s) = num(s) / den(s).
    /// Coefficients in descending-power order (index 0 = highest power).
    TransferFn {
        num: Vec<f64>,
        den: Vec<f64>,
    },
    /// Continuous-time state-space model: ẋ = Ax + Bu, y = Cx + Du.
    StateSpace {
        a: CMatrix,
        b: CMatrix,
        c: CMatrix,
        d: CMatrix,
    },
    /// Anonymous function: `@(params) expr`. Captures the environment lexically at creation time.
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
        captured_env: HashMap<String, Value>,
    },
    /// Handle to a named function: `@name`. Dispatch happens at call time.
    FuncHandle(String),
    /// Opaque history buffer for stateful FIR streaming.
    /// Arc<Mutex<...>> allows cheap Clone (ref-counted) while providing
    /// &mut access inside filter_stream with no heap reallocation per frame.
    FirState(Arc<Mutex<Vec<C64>>>),
    /// Metadata handle for stdin audio input. Opens no file descriptors.
    AudioIn {
        sample_rate: f64,
        frame_size: usize,
    },
    /// Metadata handle for stdout audio output. Opens no file descriptors.
    AudioOut {
        sample_rate: f64,
        frame_size: usize,
    },
    /// Handle to a persistent live-updating plot (terminal or viewer).
    /// Arc<Mutex<Option<...>>> allows cheap Clone while the Option lets
    /// figure_close drop the inner backend (firing Drop → cleanup)
    /// without invalidating other clones of the Arc.
    LiveFigure(Arc<Mutex<Option<Box<dyn rustlab_plot::LivePlot>>>>),
    /// Sparse vector (COO format).
    SparseVector(SparseVec),
    /// Sparse matrix (COO format).
    SparseMatrix(SparseMat),
    /// String array: `{"a", "b", "c"}`.
    StringArray(Vec<String>),
    /// Cached sparse factorization handle from `chol(A)` / `lu(A)`,
    /// consumed by `solve(F, b)`. Reusing a factor across many RHS is
    /// the canonical fast path for parameter sweeps and animations.
    SparseFactor(SparseFactor),
}

impl Value {
    pub fn negate(self) -> Result<Value, String> {
        match self {
            Value::Scalar(n) => Ok(Value::Scalar(-n)),
            Value::Complex(c) => Ok(Value::Complex(-c)),
            Value::Vector(v) => Ok(Value::Vector(-v)),
            Value::Matrix(m) => Ok(Value::Matrix(-m)),
            Value::Tensor3(t) => Ok(Value::Tensor3(-t)),
            Value::TransferFn { num, den } => Ok(Value::TransferFn {
                num: num.iter().map(|&x| -x).collect(),
                den,
            }),
            Value::SparseVector(sv) => {
                let entries = sv.entries.iter().map(|&(i, v)| (i, -v)).collect();
                Ok(Value::SparseVector(SparseVec {
                    len: sv.len,
                    entries,
                }))
            }
            Value::SparseMatrix(sm) => {
                let entries = sm.entries.iter().map(|&(r, c, v)| (r, c, -v)).collect();
                // Negation preserves the nonzero pattern, so the hint
                // (if any) is still meaningful on the result.
                Ok(Value::SparseMatrix(SparseMat {
                    rows: sm.rows,
                    cols: sm.cols,
                    entries,
                    ordering_hint: sm.ordering_hint,
                }))
            }
            Value::LiveFigure(_) => Err("cannot negate live_figure".to_string()),
            Value::StringArray(_) => Err("cannot negate string_array".to_string()),
            Value::SparseFactor(_) => Err("cannot negate sparse_factor".to_string()),
            other => Err(format!("cannot negate {}", other.type_name())),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Scalar(_) => "scalar",
            Value::Complex(_) => "complex",
            Value::Vector(_) => "vector",
            Value::Matrix(_) => "matrix",
            Value::Tensor3(_) => "tensor3",
            Value::Bool(_) => "bool",
            Value::Str(_) => "string",
            Value::QFmt(_) => "qfmt",
            Value::Struct(_) => "struct",
            Value::Tuple(_) => "tuple",
            Value::All => "all-index",
            Value::None => "none",
            Value::TransferFn { .. } => "tf",
            Value::StateSpace { .. } => "ss",
            Value::Lambda { .. } => "lambda",
            Value::FuncHandle(_) => "function_handle",
            Value::FirState(_) => "fir_state",
            Value::AudioIn { .. } => "audio_in",
            Value::AudioOut { .. } => "audio_out",
            Value::LiveFigure(_) => "live_figure",
            Value::SparseVector(_) => "sparse_vector",
            Value::SparseMatrix(_) => "sparse_matrix",
            Value::StringArray(_) => "string_array",
            Value::SparseFactor(_) => "sparse_factor",
        }
    }

    /// Extract a QFmtSpec — errors with a descriptive message if not a QFmt value.
    pub fn to_qfmt(&self) -> Result<QFmtSpec, String> {
        match self {
            Value::QFmt(spec) => Ok(spec.clone()),
            other => Err(format!(
                "expected qfmt spec (from qfmt()), got {}",
                other.type_name()
            )),
        }
    }

    /// Promote a scalar f64 to C64
    fn scalar_to_c64(n: f64) -> C64 {
        Complex::new(n, 0.0)
    }

    /// Promote a Scalar value to Complex
    fn promote_to_complex(v: Value) -> Result<C64, String> {
        match v {
            Value::Scalar(n) => Ok(Self::scalar_to_c64(n)),
            Value::Complex(c) => Ok(c),
            other => Err(format!("cannot promote {} to complex", other.type_name())),
        }
    }

    /// Em_requests §4 Option A — real-input → real-output for element-
    /// wise ops. rustlab stores everything as `Complex<f64>`; when both
    /// operands of an elem-op are essentially real (every imag part below
    /// `f64::EPSILON`), the result's imag part is floating-point noise.
    /// Zero it so `real(...)` wrappers aren't needed downstream.
    fn vector_all_real(v: &CVector) -> bool {
        v.iter().all(|c| c.im.abs() < f64::EPSILON)
    }

    fn matrix_all_real(m: &CMatrix) -> bool {
        m.iter().all(|c| c.im.abs() < f64::EPSILON)
    }

    fn vector_zero_imag(mut v: CVector) -> CVector {
        for c in v.iter_mut() {
            c.im = 0.0;
        }
        v
    }

    fn matrix_zero_imag(mut m: CMatrix) -> CMatrix {
        for c in m.iter_mut() {
            c.im = 0.0;
        }
        m
    }

    /// Octave/matlab implicit expansion: broadcast two matrices to a common
    /// shape. Each dimension must be equal between the two, or one of them
    /// must be 1 (the singleton dimension is repeated to fill the other).
    fn broadcast_pair(a: &CMatrix, b: &CMatrix) -> Result<(CMatrix, CMatrix), String> {
        let (am, an) = (a.nrows(), a.ncols());
        let (bm, bn) = (b.nrows(), b.ncols());
        let nrows = if am == bm {
            am
        } else if am == 1 {
            bm
        } else if bm == 1 {
            am
        } else {
            return Err(format!(
                "implicit expansion: row dimensions [{}] and [{}] are incompatible (must match, or one must be 1)",
                am, bm
            ));
        };
        let ncols = if an == bn {
            an
        } else if an == 1 {
            bn
        } else if bn == 1 {
            an
        } else {
            return Err(format!(
                "implicit expansion: column dimensions [{}] and [{}] are incompatible (must match, or one must be 1)",
                an, bn
            ));
        };
        let a_exp = if (am, an) == (nrows, ncols) {
            a.clone()
        } else {
            Array2::from_shape_fn((nrows, ncols), |(i, j)| {
                a[[if am == 1 { 0 } else { i }, if an == 1 { 0 } else { j }]]
            })
        };
        let b_exp = if (bm, bn) == (nrows, ncols) {
            b.clone()
        } else {
            Array2::from_shape_fn((nrows, ncols), |(i, j)| {
                b[[if bm == 1 { 0 } else { i }, if bn == 1 { 0 } else { j }]]
            })
        };
        Ok((a_exp, b_exp))
    }

    /// Apply an elementwise op (Add / Sub / ElemMul / ElemDiv / ElemPow / Pow)
    /// to two matrices, with implicit expansion. Real-real inputs collapse
    /// floating-point imag noise back to zero, matching the existing
    /// matrix-matrix code path.
    fn elementwise_with_broadcast(a: &CMatrix, b: &CMatrix, op: BinOp) -> Result<CMatrix, String> {
        use BinOp::*;
        let (a_exp, b_exp) = Self::broadcast_pair(a, b)?;
        let result = match op {
            Add => &a_exp + &b_exp,
            Sub => &a_exp - &b_exp,
            ElemMul => &a_exp * &b_exp,
            ElemDiv => &a_exp / &b_exp,
            ElemPow | Pow => {
                let rows = a_exp.nrows();
                let cols = a_exp.ncols();
                let data: Vec<C64> = a_exp
                    .iter()
                    .zip(b_exp.iter())
                    .map(|(&x, &y)| {
                        let ln_x = Complex::new(x.norm().ln(), x.arg());
                        (y * ln_x).exp()
                    })
                    .collect();
                Array2::from_shape_vec((rows, cols), data).map_err(|e| e.to_string())?
            }
            _ => {
                return Err(format!(
                    "elementwise_with_broadcast: op {:?} is not elementwise",
                    op
                ))
            }
        };
        if Self::matrix_all_real(a) && Self::matrix_all_real(b) {
            Ok(Self::matrix_zero_imag(result))
        } else {
            Ok(result)
        }
    }

    /// Promote a `Value::Vector` to a `1×N` row matrix, leaving the
    /// existing matrix layout convention (vector → row by default).
    fn vector_to_row_matrix(v: &CVector) -> CMatrix {
        let n = v.len();
        Array2::from_shape_fn((1, n), |(_, j)| v[j])
    }

    /// Conjugate transpose: `A'`
    /// A row vector (1×n) becomes a column vector stored as Matrix(n×1).
    /// A Matrix is conjugate-transposed normally.
    pub fn transpose(self) -> Result<Value, String> {
        match self {
            Value::Vector(v) => {
                // Row vector (1×n) → column vector (n×1), conjugated
                let n = v.len();
                let data: Vec<C64> = v.iter().map(|c| c.conj()).collect();
                let col = Array2::from_shape_vec((n, 1), data).map_err(|e| e.to_string())?;
                Ok(Value::Matrix(col))
            }
            Value::Matrix(m) => {
                let t = m.t().mapv(|c| c.conj());
                Ok(Value::Matrix(t.to_owned()))
            }
            Value::Scalar(n) => Ok(Value::Scalar(n)),
            Value::Complex(c) => Ok(Value::Complex(c.conj())),
            Value::SparseMatrix(sm) => {
                let entries = sm
                    .entries
                    .iter()
                    .map(|&(r, c, v)| (c, r, v.conj()))
                    .collect();
                Ok(Value::SparseMatrix(SparseMat::new(
                    sm.cols, sm.rows, entries,
                )))
            }
            other => Err(format!("cannot transpose {}", other.type_name())),
        }
    }

    /// Non-conjugate transpose: `A.'`
    /// A row vector (1×n) becomes a column vector stored as Matrix(n×1), without conjugation.
    pub fn non_conj_transpose(self) -> Result<Value, String> {
        match self {
            Value::Vector(v) => {
                let n = v.len();
                let data: Vec<C64> = v.iter().copied().collect();
                let col = Array2::from_shape_vec((n, 1), data).map_err(|e| e.to_string())?;
                Ok(Value::Matrix(col))
            }
            Value::Matrix(m) => Ok(Value::Matrix(m.t().to_owned())),
            Value::Scalar(n) => Ok(Value::Scalar(n)),
            Value::Complex(c) => Ok(Value::Complex(c)),
            Value::SparseMatrix(sm) => {
                let entries = sm.entries.iter().map(|&(r, c, v)| (c, r, v)).collect();
                Ok(Value::SparseMatrix(SparseMat::new(
                    sm.cols, sm.rows, entries,
                )))
            }
            other => Err(format!("cannot transpose {}", other.type_name())),
        }
    }

    /// Convert a 1-based index to 0-based, returning an error if the index is < 1.
    fn one_based_to_zero(n: f64) -> Result<usize, String> {
        let i = n as usize;
        if i < 1 {
            return Err(format!("index {} is invalid (1-based indexing)", n));
        }
        Ok(i - 1)
    }

    /// Resolve an index value to a list of 0-based indices for a dimension of `dim_len`.
    /// Public re-export for use by assignment paths in the evaluator.
    pub fn resolve_index_dim_public(idx: &Value, dim_len: usize) -> Result<Vec<usize>, String> {
        Self::resolve_index_dim(idx, dim_len)
    }

    /// Resolve an index value to a list of 0-based indices for a dimension of `dim_len`.
    pub fn resolve_index_dim(idx: &Value, dim_len: usize) -> Result<Vec<usize>, String> {
        match idx {
            Value::All => Ok((0..dim_len).collect()),
            Value::Scalar(n) => {
                let i = Self::one_based_to_zero(*n)?;
                if i >= dim_len {
                    return Err(format!("index {} out of bounds (size {})", n, dim_len));
                }
                Ok(vec![i])
            }
            Value::Vector(v) => v
                .iter()
                .map(|c| {
                    let i = Self::one_based_to_zero(c.re)?;
                    if i >= dim_len {
                        Err(format!(
                            "index {} out of bounds (size {})",
                            c.re as usize, dim_len
                        ))
                    } else {
                        Ok(i)
                    }
                })
                .collect(),
            other => Err(format!("invalid index type: {}", other.type_name())),
        }
    }

    /// 1-based indexing into a Vector, Matrix, or Tensor3.
    /// Single index: 1D selection. Two indices: 2D selection. Three indices: 3D selection.
    /// `:` (All) is supported in any dimension.
    pub fn index(self, indices: Vec<Value>) -> Result<Value, String> {
        match indices.len() {
            1 => self.index_1d(indices.into_iter().next().unwrap()),
            2 => {
                let mut it = indices.into_iter();
                let row_idx = it.next().unwrap();
                let col_idx = it.next().unwrap();
                self.index_2d(row_idx, col_idx)
            }
            3 => {
                let mut it = indices.into_iter();
                let i_idx = it.next().unwrap();
                let j_idx = it.next().unwrap();
                let k_idx = it.next().unwrap();
                self.index_3d(i_idx, j_idx, k_idx)
            }
            n => Err(format!("indexing requires 1, 2, or 3 arguments, got {}", n)),
        }
    }

    fn index_1d(self, idx: Value) -> Result<Value, String> {
        match self {
            Value::Vector(v) => match &idx {
                Value::All => Ok(Value::Vector(v)),
                Value::Scalar(n) => {
                    let i = Self::one_based_to_zero(*n)?;
                    if i >= v.len() {
                        return Err(format!("index {} out of bounds (length {})", n, v.len()));
                    }
                    let c = v[i];
                    if c.im.abs() < 1e-12 {
                        Ok(Value::Scalar(c.re))
                    } else {
                        Ok(Value::Complex(c))
                    }
                }
                Value::Vector(idx_v) => {
                    let result: Result<Vec<_>, _> = idx_v
                        .iter()
                        .map(|c| {
                            let i = Self::one_based_to_zero(c.re)?;
                            if i >= v.len() {
                                Err(format!(
                                    "index {} out of bounds (length {})",
                                    c.re as usize,
                                    v.len()
                                ))
                            } else {
                                Ok(v[i])
                            }
                        })
                        .collect();
                    Ok(Value::Vector(Array1::from_vec(result?)))
                }
                other => Err(format!("invalid index type: {}", other.type_name())),
            },
            Value::Matrix(m) => {
                // Single-arg matrix indexing follows matlab convention:
                // the matrix is flattened in column-major order and the
                // index/indices pick elements from that flat view.
                //   M(k)   → scalar element at 1-based linear position k
                //   M(I)   → vector of picks for each k in I (preserves I's length)
                //   M(:)   → full column-major flatten as a vector
                // Use M(i, :) for a row, M(:, j) for a column.
                let pick = |k_one_based: f64| -> Result<C64, String> {
                    let k0 = Self::one_based_to_zero(k_one_based)?;
                    let total = m.nrows() * m.ncols();
                    if k0 >= total {
                        return Err(format!(
                            "linear index {} out of bounds ({}×{} = {} elements)",
                            k_one_based as i64, m.nrows(), m.ncols(), total
                        ));
                    }
                    // column-major: k0 = col * nrows + row
                    let col = k0 / m.nrows();
                    let row = k0 % m.nrows();
                    Ok(m[[row, col]])
                };
                match &idx {
                    Value::Scalar(k) => {
                        let c = pick(*k)?;
                        if c.im.abs() < 1e-12 {
                            Ok(Value::Scalar(c.re))
                        } else {
                            Ok(Value::Complex(c))
                        }
                    }
                    Value::Vector(idx_v) => {
                        let picks: Result<Vec<C64>, _> =
                            idx_v.iter().map(|c| pick(c.re)).collect();
                        Ok(Value::Vector(Array1::from_vec(picks?)))
                    }
                    // 1-D-shaped matrix (1×N or N×1) used as an index list —
                    // treat its entries as the linear indices.
                    Value::Matrix(idx_m) if idx_m.nrows() == 1 || idx_m.ncols() == 1 => {
                        let picks: Result<Vec<C64>, _> =
                            idx_m.iter().map(|c| pick(c.re)).collect();
                        Ok(Value::Vector(Array1::from_vec(picks?)))
                    }
                    Value::All => {
                        let mut flat_data: Vec<C64> = Vec::with_capacity(m.nrows() * m.ncols());
                        for c in 0..m.ncols() {
                            for r in 0..m.nrows() {
                                flat_data.push(m[[r, c]]);
                            }
                        }
                        Ok(Value::Vector(Array1::from_vec(flat_data)))
                    }
                    other => Err(format!(
                        "matrix single-index with {} not supported; use M(i,j) or M(linear) or M(I) where I is a numeric vector",
                        other.type_name()
                    )),
                }
            }
            Value::SparseVector(sv) => match &idx {
                Value::Scalar(n) => {
                    let i = Self::one_based_to_zero(*n)?;
                    if i >= sv.len {
                        return Err(format!("index {} out of bounds (length {})", n, sv.len));
                    }
                    let c = sv.get(i);
                    if c.im.abs() < 1e-12 {
                        Ok(Value::Scalar(c.re))
                    } else {
                        Ok(Value::Complex(c))
                    }
                }
                _ => Err(format!(
                    "sparse vector indexing: unsupported index type {}",
                    idx.type_name()
                )),
            },
            Value::SparseMatrix(sm) => {
                match &idx {
                    Value::Scalar(n) => {
                        let i = Self::one_based_to_zero(*n)?;
                        if i >= sm.rows {
                            return Err(format!(
                                "row index {} out of bounds ({} rows)",
                                n, sm.rows
                            ));
                        }
                        // Return dense row vector
                        let mut row = Array1::from_elem(sm.cols, Complex::new(0.0, 0.0));
                        for &(r, c, v) in &sm.entries {
                            if r == i {
                                row[c] = v;
                            }
                        }
                        Ok(Value::Vector(row))
                    }
                    _ => Err(format!(
                        "sparse matrix single-index: unsupported index type {}",
                        idx.type_name()
                    )),
                }
            }
            Value::Tuple(items) => match &idx {
                Value::Scalar(n) => {
                    let i = Self::one_based_to_zero(*n)?;
                    if i >= items.len() {
                        return Err(format!(
                            "index {} out of bounds (length {})",
                            n,
                            items.len()
                        ));
                    }
                    Ok(items.into_iter().nth(i).unwrap())
                }
                other => Err(format!(
                    "tuple indexing requires a scalar index, got {}",
                    other.type_name()
                )),
            },
            Value::Str(s) => {
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len();
                match &idx {
                    Value::All => Ok(Value::Str(s)),
                    Value::Scalar(n) => {
                        let i = Self::one_based_to_zero(*n)?;
                        if i >= len {
                            return Err(format!(
                                "string index {} out of bounds (length {})",
                                *n as usize, len
                            ));
                        }
                        Ok(Value::Str(chars[i].to_string()))
                    }
                    Value::Vector(idx_v) => {
                        let mut result = String::with_capacity(idx_v.len());
                        for c in idx_v.iter() {
                            let i = Self::one_based_to_zero(c.re)?;
                            if i >= len {
                                return Err(format!(
                                    "string index {} out of bounds (length {})",
                                    c.re as usize, len
                                ));
                            }
                            result.push(chars[i]);
                        }
                        Ok(Value::Str(result))
                    }
                    other => Err(format!("invalid string index type: {}", other.type_name())),
                }
            }
            Value::StringArray(arr) => match &idx {
                Value::All => Ok(Value::StringArray(arr)),
                Value::Scalar(n) => {
                    let i = Self::one_based_to_zero(*n)?;
                    if i >= arr.len() {
                        return Err(format!(
                            "index {} out of bounds (length {})",
                            *n as usize,
                            arr.len()
                        ));
                    }
                    Ok(Value::Str(arr[i].clone()))
                }
                Value::Vector(idx_v) => {
                    let result: Result<Vec<_>, _> = idx_v
                        .iter()
                        .map(|c| {
                            let i = Self::one_based_to_zero(c.re)?;
                            if i >= arr.len() {
                                Err(format!(
                                    "index {} out of bounds (length {})",
                                    c.re as usize,
                                    arr.len()
                                ))
                            } else {
                                Ok(arr[i].clone())
                            }
                        })
                        .collect();
                    Ok(Value::StringArray(result?))
                }
                other => Err(format!(
                    "invalid string_array index type: {}",
                    other.type_name()
                )),
            },
            other => Err(format!("cannot index into {}", other.type_name())),
        }
    }

    fn index_2d(self, row_idx: Value, col_idx: Value) -> Result<Value, String> {
        match self {
            Value::Matrix(m) => {
                let rows = Self::resolve_index_dim(&row_idx, m.nrows())?;
                let cols = Self::resolve_index_dim(&col_idx, m.ncols())?;

                if rows.len() == 1 && cols.len() == 1 {
                    // Single element
                    let c = m[[rows[0], cols[0]]];
                    if c.im.abs() < 1e-12 {
                        Ok(Value::Scalar(c.re))
                    } else {
                        Ok(Value::Complex(c))
                    }
                } else if rows.len() == 1 {
                    // Single row → Vector
                    let r = rows[0];
                    Ok(Value::Vector(Array1::from_iter(
                        cols.iter().map(|&c| m[[r, c]]),
                    )))
                } else if cols.len() == 1 {
                    // Single column → Vector
                    let c = cols[0];
                    Ok(Value::Vector(Array1::from_iter(
                        rows.iter().map(|&r| m[[r, c]]),
                    )))
                } else {
                    // Submatrix
                    let nr = rows.len();
                    let nc = cols.len();
                    let mut data: Vec<C64> = Vec::with_capacity(nr * nc);
                    for &r in &rows {
                        for &c in &cols {
                            data.push(m[[r, c]]);
                        }
                    }
                    Ok(Value::Matrix(
                        Array2::from_shape_vec((nr, nc), data).map_err(|e| e.to_string())?,
                    ))
                }
            }
            Value::Vector(v) => {
                // Allow v(i, 1) or v(1, j) for column/row vector indexing
                match (&row_idx, &col_idx) {
                    (_, Value::Scalar(c)) if (*c as usize) == 1 => {
                        Value::Vector(v).index_1d(row_idx)
                    }
                    (Value::Scalar(r), _) if (*r as usize) == 1 => {
                        Value::Vector(v).index_1d(col_idx)
                    }
                    _ => Err("2D indexing on a vector requires one dimension to be 1".to_string()),
                }
            }
            Value::SparseMatrix(sm) => {
                match (&row_idx, &col_idx) {
                    (Value::Scalar(r), Value::Scalar(c)) => {
                        let ri = Self::one_based_to_zero(*r)?;
                        let ci = Self::one_based_to_zero(*c)?;
                        if ri >= sm.rows || ci >= sm.cols {
                            return Err(format!(
                                "index ({},{}) out of bounds for {}×{} sparse matrix",
                                r, c, sm.rows, sm.cols
                            ));
                        }
                        let v = sm.get(ri, ci);
                        if v.im.abs() < 1e-12 {
                            Ok(Value::Scalar(v.re))
                        } else {
                            Ok(Value::Complex(v))
                        }
                    }
                    _ => {
                        // Fall back to dense for complex indexing
                        Value::Matrix(sm.to_dense()).index_2d(row_idx, col_idx)
                    }
                }
            }
            other => Err(format!(
                "2D indexing requires a matrix, got {}",
                other.type_name()
            )),
        }
    }

    /// 1-based indexing into a Tensor3: `A(i, j, k)`.
    /// Trailing singleton dimensions are dropped so e.g. `A(:, :, k)` returns a `Matrix`.
    /// A single-element extract returns `Scalar`/`Complex`.
    fn index_3d(self, i_idx: Value, j_idx: Value, k_idx: Value) -> Result<Value, String> {
        let t = match self {
            Value::Tensor3(t) => t,
            other => {
                return Err(format!(
                    "3D indexing requires a tensor3, got {}",
                    other.type_name()
                ))
            }
        };
        let (m, n, p) = (t.shape()[0], t.shape()[1], t.shape()[2]);
        let is_all = |v: &Value| matches!(v, Value::All);
        let rows = Self::resolve_index_dim(&i_idx, m)?;
        let cols = Self::resolve_index_dim(&j_idx, n)?;
        let pages = Self::resolve_index_dim(&k_idx, p)?;
        let (nr, nc, np) = (rows.len(), cols.len(), pages.len());

        // Single-element extract
        if nr == 1 && nc == 1 && np == 1 {
            let c = t[[rows[0], cols[0], pages[0]]];
            return if c.im.abs() < 1e-12 {
                Ok(Value::Scalar(c.re))
            } else {
                Ok(Value::Complex(c))
            };
        }

        // All-range extract: clone the whole tensor (or the sub-tensor)
        if is_all(&i_idx) && is_all(&j_idx) && is_all(&k_idx) {
            return Ok(Value::Tensor3(t));
        }

        // Determine result shape by dropping *trailing* singleton dimensions
        // (Octave-style: A(:, :, k) → Matrix, A(:, j, :) → Matrix, A(i, :, :) → Matrix,
        //  A(1, 1, :) → Vector (row×col collapse), A(:, :, :) → Tensor3 already handled).
        // General rule: build a (nr × nc × np) buffer, then squeeze trailing singletons
        // OR inner singletons to return the lowest-rank type that fits.
        let mut data: Vec<C64> = Vec::with_capacity(nr * nc * np);
        for &r in &rows {
            for &c in &cols {
                for &k in &pages {
                    data.push(t[[r, c, k]]);
                }
            }
        }

        // Decide return shape.
        // - 2 singleton dims → Vector along the non-singleton axis.
        // - 1 singleton dim  → Matrix with the two non-singleton axes.
        // - 0 singleton dims → Tensor3.
        let singletons = [nr == 1, nc == 1, np == 1];
        let singleton_count = singletons.iter().filter(|b| **b).count();
        match singleton_count {
            2 => {
                // Single non-singleton axis → Vector
                Ok(Value::Vector(Array1::from_vec(data)))
            }
            1 => {
                // Two non-singleton axes → Matrix in their natural (earlier-first) order
                if singletons[0] {
                    // Drop i: Matrix (nc × np)
                    Ok(Value::Matrix(
                        Array2::from_shape_vec((nc, np), data).map_err(|e| e.to_string())?,
                    ))
                } else if singletons[1] {
                    // Drop j: Matrix (nr × np)
                    Ok(Value::Matrix(
                        Array2::from_shape_vec((nr, np), data).map_err(|e| e.to_string())?,
                    ))
                } else {
                    // Drop k: Matrix (nr × nc), but the walk order above is
                    // (r major, c middle, k inner) — so for np=1 we already
                    // collected in row-major (nr, nc) order.
                    Ok(Value::Matrix(
                        Array2::from_shape_vec((nr, nc), data).map_err(|e| e.to_string())?,
                    ))
                }
            }
            _ => {
                // No singletons → Tensor3.
                // The walk was (r outer, c middle, k inner), which is row-major (C order)
                // for shape (nr, nc, np) — what Array3::from_shape_vec expects by default.
                Ok(Value::Tensor3(
                    ndarray::Array3::from_shape_vec((nr, nc, np), data)
                        .map_err(|e| e.to_string())?,
                ))
            }
        }
    }

    /// Logical NOT — only valid for Bool.
    pub fn not(self) -> Result<Value, String> {
        match self {
            Value::Bool(b) => Ok(Value::Bool(!b)),
            other => Err(format!(
                "'!' operator requires bool, got {}",
                other.type_name()
            )),
        }
    }

    /// Truth value for the short-circuit logical operators `&&` / `||`.
    /// Matches matlab/octave: scalars are truthy iff non-zero (complex iff
    /// either part is non-zero). Matrix/Vector operands error — matlab
    /// requires explicit `any(...)`/`all(...)` to collapse them first.
    pub fn is_truthy_for_logical(&self) -> Result<bool, String> {
        match self {
            Value::Bool(b) => Ok(*b),
            Value::Scalar(s) => Ok(*s != 0.0),
            Value::Complex(c) => Ok(c.re != 0.0 || c.im != 0.0),
            other => Err(format!(
                "&&/||: operand must be scalar (got {}); use any() or all() to reduce a matrix/vector",
                other.type_name()
            )),
        }
    }

    pub fn binop(op: BinOp, lhs: Value, rhs: Value) -> Result<Value, String> {
        use BinOp::*;

        // Logical operators: scalar truthiness, eager. Short-circuiting is
        // handled in the evaluator (`Expr::BinOp` arm) so that `false && rhs`
        // never evaluates rhs at all. This direct path exists for callers
        // that already have both values in hand.
        if matches!(op, And | Or) {
            let a = lhs.is_truthy_for_logical()?;
            let b = rhs.is_truthy_for_logical()?;
            return Ok(Value::Bool(match op {
                And => a && b,
                Or => a || b,
                _ => unreachable!(),
            }));
        }

        // Comparison operators: compare scalar/complex values, return Bool
        if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) {
            return match (&lhs, &rhs) {
                (Value::Scalar(a), Value::Scalar(b)) => Ok(Value::Bool(match op {
                    Eq => a == b,
                    Ne => a != b,
                    Lt => a < b,
                    Le => a <= b,
                    Gt => a > b,
                    Ge => a >= b,
                    _ => unreachable!(),
                })),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(match op {
                    Eq => a == b,
                    Ne => a != b,
                    _ => return Err("ordered comparison not defined for bool".to_string()),
                })),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(match op {
                    Eq => a == b,
                    Ne => a != b,
                    _ => return Err("ordered comparison not defined for strings".to_string()),
                })),
                // Element-wise comparison: Vector op Scalar or Scalar op Vector → real 0/1 vector
                (Value::Vector(v), Value::Scalar(b)) => {
                    let b = *b;
                    let data: Vec<C64> = v
                        .iter()
                        .map(|c| {
                            let a = c.re;
                            let flag = match op {
                                Eq => a == b,
                                Ne => a != b,
                                Lt => a < b,
                                Le => a <= b,
                                Gt => a > b,
                                Ge => a >= b,
                                _ => unreachable!(),
                            };
                            Complex::new(if flag { 1.0 } else { 0.0 }, 0.0)
                        })
                        .collect();
                    Ok(Value::Vector(Array1::from_vec(data)))
                }
                (Value::Scalar(a), Value::Vector(v)) => {
                    let a = *a;
                    let data: Vec<C64> = v
                        .iter()
                        .map(|c| {
                            let b = c.re;
                            let flag = match op {
                                Eq => a == b,
                                Ne => a != b,
                                Lt => a < b,
                                Le => a <= b,
                                Gt => a > b,
                                Ge => a >= b,
                                _ => unreachable!(),
                            };
                            Complex::new(if flag { 1.0 } else { 0.0 }, 0.0)
                        })
                        .collect();
                    Ok(Value::Vector(Array1::from_vec(data)))
                }
                _ => Err(format!(
                    "comparison requires two scalars (or vector op scalar), got {} and {}",
                    lhs.type_name(),
                    rhs.type_name()
                )),
            };
        }

        // String concatenation
        if let (Value::Str(a), Value::Str(b)) = (&lhs, &rhs) {
            if op == Add {
                return Ok(Value::Str(format!("{}{}", a, b)));
            }
        }

        match (&lhs, &rhs) {
            // ── Scalar op Scalar ──────────────────────────────────────────────
            (Value::Scalar(a), Value::Scalar(b)) => {
                let (a, b) = (*a, *b);
                let result = match op {
                    Add => a + b,
                    Sub => a - b,
                    Mul | ElemMul => a * b,
                    Div | ElemDiv => a / b,
                    Pow | ElemPow => a.powf(b),
                    _ => unreachable!(),
                };
                Ok(Value::Scalar(result))
            }

            // ── Complex arithmetic ────────────────────────────────────────────
            (Value::Scalar(_), Value::Complex(_))
            | (Value::Complex(_), Value::Scalar(_))
            | (Value::Complex(_), Value::Complex(_)) => {
                let a = Self::promote_to_complex(lhs)?;
                let b = Self::promote_to_complex(rhs)?;
                let result = match op {
                    Add => a + b,
                    Sub => a - b,
                    Mul | ElemMul => a * b,
                    Div | ElemDiv => a / b,
                    Pow | ElemPow => {
                        let ln_a = Complex::new(a.norm().ln(), a.arg());
                        (b * ln_a).exp()
                    }
                    _ => unreachable!(),
                };
                Ok(Value::Complex(result))
            }

            // ── Vector op Vector ──────────────────────────────────────────────
            (Value::Vector(a), Value::Vector(b)) => {
                match op {
                    // `*` between two row vectors is a dimension error (both are 1×n).
                    // Use `dot(u,v)` for dot product, or `u * v'` (v' makes a column n×1).
                    Mul => {
                        return Err(format!(
                            "cannot multiply two row vectors [1×{}] * [1×{}]\n  hint: use `u * v'` for dot product (v' is a [{}×1] column)\n  hint: use `dot(u,v)` for the dot product directly\n  hint: use `.*` for element-wise multiply",
                            a.len(), b.len(), b.len()
                        ));
                    }
                    // Element-wise ops
                    Add | Sub | ElemMul | ElemDiv | ElemPow => {
                        if a.len() != b.len() {
                            return Err(format!(
                                "vector length mismatch: {} vs {}",
                                a.len(),
                                b.len()
                            ));
                        }
                        let result: CVector = match op {
                            Add => a + b,
                            Sub => a - b,
                            ElemMul => a * b,
                            ElemDiv => a / b,
                            ElemPow => Array1::from_iter(a.iter().zip(b.iter()).map(|(&x, &y)| {
                                let ln_x = Complex::new(x.norm().ln(), x.arg());
                                (y * ln_x).exp()
                            })),
                            _ => unreachable!(),
                        };
                        // §4 Option A: real-input → real-output for elem-ops.
                        let result = if matches!(op, ElemMul | ElemDiv | ElemPow)
                            && Self::vector_all_real(a)
                            && Self::vector_all_real(b)
                        {
                            Self::vector_zero_imag(result)
                        } else {
                            result
                        };
                        Ok(Value::Vector(result))
                    }
                    Div => Err(
                        "cannot divide two vectors with /; use ./ for element-wise division"
                            .to_string(),
                    ),
                    Pow => Err(
                        "cannot raise two vectors with ^; use .^ for element-wise power"
                            .to_string(),
                    ),
                    _ => unreachable!(),
                }
            }

            // ── Scalar/Complex broadcast onto Vector ──────────────────────────
            (Value::Scalar(_), Value::Vector(_)) | (Value::Complex(_), Value::Vector(_)) => {
                let scalar = Self::promote_to_complex(lhs)?;
                let vec = match rhs {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let result: CVector = match op {
                    Add => Array1::from_iter(vec.iter().map(|&x| scalar + x)),
                    Sub => Array1::from_iter(vec.iter().map(|&x| scalar - x)),
                    Mul | ElemMul => Array1::from_iter(vec.iter().map(|&x| scalar * x)),
                    Div | ElemDiv => Array1::from_iter(vec.iter().map(|&x| scalar / x)),
                    Pow | ElemPow => Array1::from_iter(vec.iter().map(|&x| {
                        let ln_s = Complex::new(scalar.norm().ln(), scalar.arg());
                        (x * ln_s).exp()
                    })),
                    _ => unreachable!(),
                };
                Ok(Value::Vector(result))
            }

            // ── Vector broadcast with Scalar/Complex ──────────────────────────
            (Value::Vector(_), Value::Scalar(_)) | (Value::Vector(_), Value::Complex(_)) => {
                let vec = match lhs {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let scalar = Self::promote_to_complex(rhs)?;
                let result: CVector = match op {
                    Add => Array1::from_iter(vec.iter().map(|&x| x + scalar)),
                    Sub => Array1::from_iter(vec.iter().map(|&x| x - scalar)),
                    Mul | ElemMul => Array1::from_iter(vec.iter().map(|&x| x * scalar)),
                    Div | ElemDiv => Array1::from_iter(vec.iter().map(|&x| x / scalar)),
                    Pow | ElemPow => Array1::from_iter(vec.iter().map(|&x| {
                        let ln_x = Complex::new(x.norm().ln(), x.arg());
                        (scalar * ln_x).exp()
                    })),
                    _ => unreachable!(),
                };
                Ok(Value::Vector(result))
            }

            // ── Matrix * Matrix (matrix multiply for `*`, element-wise for `.*`) ──
            (Value::Matrix(a), Value::Matrix(b)) => match op {
                Mul => {
                    if a.ncols() != b.nrows() {
                        return Err(format!(
                                "matrix multiply: inner dimensions must match\n  left:  [{}×{}]\n  right: [{}×{}]\n  hint:  use .* for element-wise multiply",
                                a.nrows(), a.ncols(), b.nrows(), b.ncols()
                            ));
                    }
                    Ok(Value::Matrix(a.dot(b)))
                }
                Add | Sub | ElemMul | ElemDiv => {
                    Self::elementwise_with_broadcast(a, b, op).map(Value::Matrix)
                }
                Div => Err(
                    "use ./ for element-wise matrix division; or inv(A)*B for left-divide"
                        .to_string(),
                ),
                ElemPow | Pow => {
                    Self::elementwise_with_broadcast(a, b, op).map(Value::Matrix)
                }
                _ => unreachable!(),
            },

            // ── Matrix * Vector (matrix × row-vector) ─────────────────────────
            // Vector is a row (1×n). Matrix(m×k) * row(1×n) requires k==1.
            // The common case M*v where v is meant as a column: write M * v' instead.
            (Value::Matrix(m), Value::Vector(v)) => {
                match op {
                    Mul => {
                        if m.ncols() != 1 {
                            return Err(format!(
                                "cannot multiply matrix [{}×{}] by row vector [1×{}]\n  hint: transpose v with v' to make a column vector [{}×1], then use M * v'",
                                m.nrows(), m.ncols(), v.len(), v.len()
                            ));
                        }
                        // Matrix(m×1) * row(1×n) = outer product Matrix(m×n)
                        let (nrows, ncols) = (m.nrows(), v.len());
                        let mut data: Vec<C64> = Vec::with_capacity(nrows * ncols);
                        for r in 0..nrows {
                            for c in 0..ncols {
                                data.push(m[[r, 0]] * v[c]);
                            }
                        }
                        Ok(Value::Matrix(Array2::from_shape_vec((nrows, ncols), data)
                            .map_err(|e| e.to_string())?))
                    }
                    Add | Sub | ElemMul | ElemDiv | ElemPow | Pow => {
                        // Implicit expansion: treat the row Vector as a 1×N row
                        // matrix and broadcast against the M×K matrix.
                        let v_row = Self::vector_to_row_matrix(v);
                        Self::elementwise_with_broadcast(m, &v_row, op).map(Value::Matrix)
                    }
                    _ => Err(format!(
                        "operator {:?} not defined for matrix and vector; use .* ./ .^ for element-wise ops",
                        op
                    )),
                }
            }

            // ── Vector * Matrix (row-vector × matrix) ─────────────────────────
            (Value::Vector(v), Value::Matrix(m)) => {
                match op {
                    Mul => {
                        if v.len() != m.nrows() {
                            return Err(format!(
                                "vector-matrix multiply: row vector [1×{}] * matrix [{}×{}]: inner dimension mismatch\n  hint: use .* for element-wise multiply",
                                v.len(), m.nrows(), m.ncols()
                            ));
                        }
                        // Row vector (1×n) * matrix (n×k) → row vector (1×k)
                        let k = m.ncols();
                        let result: CVector = Array1::from_iter((0..k).map(|j| {
                            v.iter().zip(m.column(j).iter()).map(|(&vi, &mij)| vi * mij).sum::<C64>()
                        }));
                        // Collapse 1×1 result to scalar (e.g. v * v' gives scalar)
                        if k == 1 {
                            let c = result[0];
                            if c.im.abs() < 1e-12 { Ok(Value::Scalar(c.re)) } else { Ok(Value::Complex(c)) }
                        } else {
                            Ok(Value::Vector(result))
                        }
                    }
                    Add | Sub | ElemMul | ElemDiv | ElemPow | Pow => {
                        // Implicit expansion: row Vector promoted to 1×N matrix
                        // on the lhs, then broadcast against M×K rhs.
                        let v_row = Self::vector_to_row_matrix(v);
                        Self::elementwise_with_broadcast(&v_row, m, op).map(Value::Matrix)
                    }
                    _ => Err(format!(
                        "operator {:?} not defined for vector and matrix; use .* ./ .^ for element-wise ops",
                        op
                    )),
                }
            }

            // ── Scalar/Complex broadcast onto Matrix ──────────────────────────
            (Value::Scalar(_), Value::Matrix(_)) | (Value::Complex(_), Value::Matrix(_)) => {
                let scalar = Self::promote_to_complex(lhs)?;
                let mat = match rhs {
                    Value::Matrix(m) => m,
                    _ => unreachable!(),
                };
                let rows = mat.nrows();
                let cols = mat.ncols();
                let data: Vec<C64> = mat
                    .iter()
                    .map(|&x| match op {
                        Add => scalar + x,
                        Sub => scalar - x,
                        Mul | ElemMul => scalar * x,
                        Div | ElemDiv => scalar / x,
                        Pow | ElemPow => {
                            let ln_s = Complex::new(scalar.norm().ln(), scalar.arg());
                            (x * ln_s).exp()
                        }
                        _ => unreachable!(),
                    })
                    .collect();
                Ok(Value::Matrix(
                    Array2::from_shape_vec((rows, cols), data).map_err(|e| e.to_string())?,
                ))
            }

            // ── Matrix broadcast with Scalar/Complex ──────────────────────────
            (Value::Matrix(_), Value::Scalar(_)) | (Value::Matrix(_), Value::Complex(_)) => {
                let mat = match lhs {
                    Value::Matrix(m) => m,
                    _ => unreachable!(),
                };
                let scalar = Self::promote_to_complex(rhs)?;
                let rows = mat.nrows();
                let cols = mat.ncols();
                let data: Vec<C64> = mat
                    .iter()
                    .map(|&x| match op {
                        Add => x + scalar,
                        Sub => x - scalar,
                        Mul | ElemMul => x * scalar,
                        Div | ElemDiv => x / scalar,
                        Pow | ElemPow => {
                            let ln_x = Complex::new(x.norm().ln(), x.arg());
                            (scalar * ln_x).exp()
                        }
                        _ => unreachable!(),
                    })
                    .collect();
                Ok(Value::Matrix(
                    Array2::from_shape_vec((rows, cols), data).map_err(|e| e.to_string())?,
                ))
            }

            // ── Tensor3 element-wise arithmetic ───────────────────────────────

            // Scalar/Complex broadcast onto Tensor3
            (Value::Scalar(_), Value::Tensor3(_)) | (Value::Complex(_), Value::Tensor3(_)) => {
                let scalar = Self::promote_to_complex(lhs)?;
                let t = match rhs {
                    Value::Tensor3(t) => t,
                    _ => unreachable!(),
                };
                match op {
                    Mul | ElemMul | Div | ElemDiv | Add | Sub | Pow | ElemPow => {
                        let mapped = t.mapv(|x| match op {
                            Add => scalar + x,
                            Sub => scalar - x,
                            Mul | ElemMul => scalar * x,
                            Div | ElemDiv => scalar / x,
                            Pow | ElemPow => {
                                let ln_s = Complex::new(scalar.norm().ln(), scalar.arg());
                                (x * ln_s).exp()
                            }
                            _ => unreachable!(),
                        });
                        Ok(Value::Tensor3(mapped))
                    }
                    _ => Err(format!(
                        "operator {:?} not defined between scalar and tensor3",
                        op
                    )),
                }
            }

            // Tensor3 broadcast with Scalar/Complex
            (Value::Tensor3(_), Value::Scalar(_)) | (Value::Tensor3(_), Value::Complex(_)) => {
                let t = match lhs {
                    Value::Tensor3(t) => t,
                    _ => unreachable!(),
                };
                let scalar = Self::promote_to_complex(rhs)?;
                match op {
                    Mul | ElemMul | Div | ElemDiv | Add | Sub | Pow | ElemPow => {
                        let mapped = t.mapv(|x| match op {
                            Add => x + scalar,
                            Sub => x - scalar,
                            Mul | ElemMul => x * scalar,
                            Div | ElemDiv => x / scalar,
                            Pow | ElemPow => {
                                let ln_x = Complex::new(x.norm().ln(), x.arg());
                                (scalar * ln_x).exp()
                            }
                            _ => unreachable!(),
                        });
                        Ok(Value::Tensor3(mapped))
                    }
                    _ => Err(format!(
                        "operator {:?} not defined between tensor3 and scalar",
                        op
                    )),
                }
            }

            // Tensor3 op Tensor3: element-wise for +, -, .*, ./, .^; error for * / ^
            (Value::Tensor3(a), Value::Tensor3(b)) => {
                if a.shape() != b.shape() {
                    return Err(format!(
                        "tensor3 shape mismatch: {:?} vs {:?}",
                        a.shape(),
                        b.shape()
                    ));
                }
                match op {
                    Add => Ok(Value::Tensor3(a + b)),
                    Sub => Ok(Value::Tensor3(a - b)),
                    ElemMul => Ok(Value::Tensor3(a * b)),
                    ElemDiv => Ok(Value::Tensor3(a / b)),
                    ElemPow => {
                        let data = a
                            .iter()
                            .zip(b.iter())
                            .map(|(&x, &y)| {
                                let ln_x = Complex::new(x.norm().ln(), x.arg());
                                (y * ln_x).exp()
                            })
                            .collect::<Vec<_>>();
                        Ok(Value::Tensor3(
                            ndarray::Array3::from_shape_vec(
                                (a.shape()[0], a.shape()[1], a.shape()[2]),
                                data,
                            )
                            .map_err(|e| e.to_string())?,
                        ))
                    }
                    Mul => Err(
                        "operator * not defined between two tensor3 values; use .* for element-wise (matmul on rank-3 requires a future pagemtimes)"
                            .to_string(),
                    ),
                    Div => Err(
                        "operator / not defined between two tensor3 values; use ./ for element-wise"
                            .to_string(),
                    ),
                    Pow => Err(
                        "operator ^ not defined between two tensor3 values; use .^ for element-wise"
                            .to_string(),
                    ),
                    _ => Err(format!(
                        "operator {:?} not defined between two tensor3 values",
                        op
                    )),
                }
            }

            // Matrix ± Tensor3 / Vector ± Tensor3: broadcast NOT supported — clear error
            (Value::Matrix(_), Value::Tensor3(_)) | (Value::Tensor3(_), Value::Matrix(_)) => {
                Err(format!(
                    "operator {:?} not defined between matrix and tensor3 (broadcasting is not supported; use explicit iteration or cat/repmat)",
                    op
                ))
            }
            (Value::Vector(_), Value::Tensor3(_)) | (Value::Tensor3(_), Value::Vector(_)) => {
                Err(format!(
                    "operator {:?} not defined between vector and tensor3",
                    op
                ))
            }

            // ── TransferFn arithmetic ─────────────────────────────────────────
            (Value::TransferFn { num: n1, den: d1 }, Value::TransferFn { num: n2, den: d2 }) => {
                match op {
                    Add => Ok(Value::TransferFn {
                        num: poly_add(&poly_mul(n1, d2), &poly_mul(n2, d1)),
                        den: poly_mul(d1, d2),
                    }),
                    Sub => Ok(Value::TransferFn {
                        num: poly_sub(&poly_mul(n1, d2), &poly_mul(n2, d1)),
                        den: poly_mul(d1, d2),
                    }),
                    Mul | ElemMul => Ok(Value::TransferFn {
                        num: poly_mul(n1, n2),
                        den: poly_mul(d1, d2),
                    }),
                    Div | ElemDiv => Ok(Value::TransferFn {
                        num: poly_mul(n1, d2),
                        den: poly_mul(d1, n2),
                    }),
                    _ => Err(format!(
                        "operator {:?} not defined between two tf values",
                        op
                    )),
                }
            }

            (Value::TransferFn { num, den }, Value::Scalar(s)) => {
                let s = *s;
                match op {
                    Mul | ElemMul => Ok(Value::TransferFn {
                        num: poly_scale(num, s),
                        den: den.clone(),
                    }),
                    Div | ElemDiv => Ok(Value::TransferFn {
                        num: poly_scale(num, 1.0 / s),
                        den: den.clone(),
                    }),
                    Add => Ok(Value::TransferFn {
                        num: poly_add(num, &poly_scale(den, s)),
                        den: den.clone(),
                    }),
                    Sub => Ok(Value::TransferFn {
                        num: poly_sub(num, &poly_scale(den, s)),
                        den: den.clone(),
                    }),
                    Pow | ElemPow => {
                        if s.fract() != 0.0 || s < 0.0 {
                            return Err(format!(
                                "tf ^ n requires a non-negative integer exponent, got {}",
                                s
                            ));
                        }
                        let n = s as usize;
                        if n == 0 {
                            return Ok(Value::TransferFn {
                                num: vec![1.0],
                                den: vec![1.0],
                            });
                        }
                        let mut rn = num.clone();
                        let mut rd = den.clone();
                        for _ in 1..n {
                            rn = poly_mul(&rn, num);
                            rd = poly_mul(&rd, den);
                        }
                        Ok(Value::TransferFn { num: rn, den: rd })
                    }
                    _ => Err(format!("operator {:?} not defined for tf and scalar", op)),
                }
            }

            (Value::Scalar(s), Value::TransferFn { num, den }) => {
                let s = *s;
                match op {
                    Mul | ElemMul => Ok(Value::TransferFn {
                        num: poly_scale(num, s),
                        den: den.clone(),
                    }),
                    Div | ElemDiv => Ok(Value::TransferFn {
                        // s / (num/den) = (s * den) / num
                        num: poly_scale(den, s),
                        den: num.clone(),
                    }),
                    Add => Ok(Value::TransferFn {
                        num: poly_add(&poly_scale(den, s), num),
                        den: den.clone(),
                    }),
                    Sub => Ok(Value::TransferFn {
                        num: poly_sub(&poly_scale(den, s), num),
                        den: den.clone(),
                    }),
                    _ => Err(format!("operator {:?} not defined for scalar and tf", op)),
                }
            }

            // ── Native sparse arithmetic ─────────────────────────────────────

            // SparseMatrix + SparseMatrix, SparseMatrix - SparseMatrix
            (Value::SparseMatrix(a), Value::SparseMatrix(b)) => {
                match op {
                    Add => Ok(Value::SparseMatrix(a.add(&b).map_err(|e| e)?)),
                    Sub => Ok(Value::SparseMatrix(a.sub(&b).map_err(|e| e)?)),
                    _ => {
                        // Fall back to dense for other ops between two sparse matrices
                        Self::binop(op, Value::Matrix(a.to_dense()), Value::Matrix(b.to_dense()))
                    }
                }
            }

            // SparseMatrix * Scalar / Scalar * SparseMatrix
            (Value::SparseMatrix(sm), Value::Scalar(s)) => match op {
                Mul | ElemMul => Ok(Value::SparseMatrix(sm.scale(Complex::new(*s, 0.0)))),
                Add => Self::binop(op, Value::Matrix(sm.to_dense()), rhs),
                Sub => Self::binop(op, Value::Matrix(sm.to_dense()), rhs),
                Div | ElemDiv => Ok(Value::SparseMatrix(sm.scale(Complex::new(1.0 / s, 0.0)))),
                _ => Self::binop(op, Value::Matrix(sm.to_dense()), rhs),
            },
            (Value::Scalar(s), Value::SparseMatrix(sm)) => match op {
                Mul | ElemMul => Ok(Value::SparseMatrix(sm.scale(Complex::new(*s, 0.0)))),
                _ => Self::binop(op, lhs, Value::Matrix(sm.to_dense())),
            },
            (Value::SparseMatrix(sm), Value::Complex(c)) => match op {
                Mul | ElemMul => Ok(Value::SparseMatrix(sm.scale(*c))),
                Div | ElemDiv => Ok(Value::SparseMatrix(sm.scale(Complex::new(1.0, 0.0) / c))),
                _ => Self::binop(op, Value::Matrix(sm.to_dense()), rhs),
            },
            (Value::Complex(c), Value::SparseMatrix(sm)) => match op {
                Mul | ElemMul => Ok(Value::SparseMatrix(sm.scale(*c))),
                _ => Self::binop(op, lhs, Value::Matrix(sm.to_dense())),
            },

            // SparseMatrix * Matrix (SpMM)
            (Value::SparseMatrix(sm), Value::Matrix(ref b)) => match op {
                Mul => Ok(Value::Matrix(sm.spmm(b).map_err(|e| e)?)),
                _ => Self::binop(op, Value::Matrix(sm.to_dense()), rhs),
            },

            // Matrix * SparseMatrix → dense fallback
            (Value::Matrix(_), Value::SparseMatrix(sm)) => {
                Self::binop(op, lhs, Value::Matrix(sm.to_dense()))
            }

            // SparseVector + SparseVector, SparseVector - SparseVector
            (Value::SparseVector(a), Value::SparseVector(b)) => match op {
                Add => Ok(Value::SparseVector(a.add(&b).map_err(|e| e)?)),
                Sub => Ok(Value::SparseVector(a.sub(&b).map_err(|e| e)?)),
                _ => Self::binop(op, Value::Vector(a.to_dense()), Value::Vector(b.to_dense())),
            },

            // SparseVector * Scalar / Scalar * SparseVector
            (Value::SparseVector(sv), Value::Scalar(s)) => match op {
                Mul | ElemMul => Ok(Value::SparseVector(sv.scale(Complex::new(*s, 0.0)))),
                Div | ElemDiv => Ok(Value::SparseVector(sv.scale(Complex::new(1.0 / s, 0.0)))),
                _ => Self::binop(op, Value::Vector(sv.to_dense()), rhs),
            },
            (Value::Scalar(s), Value::SparseVector(sv)) => match op {
                Mul | ElemMul => Ok(Value::SparseVector(sv.scale(Complex::new(*s, 0.0)))),
                _ => Self::binop(op, lhs, Value::Vector(sv.to_dense())),
            },

            // Remaining sparse fallback: promote to dense
            (Value::SparseVector(sv), _) => Self::binop(op, Value::Vector(sv.to_dense()), rhs),
            (_, Value::SparseVector(sv)) => Self::binop(op, lhs, Value::Vector(sv.to_dense())),
            (Value::SparseMatrix(sm), _) => Self::binop(op, Value::Matrix(sm.to_dense()), rhs),
            (_, Value::SparseMatrix(sm)) => Self::binop(op, lhs, Value::Matrix(sm.to_dense())),

            (a, b) => Err(format!(
                "unsupported operand types for {:?}: {} and {}",
                op,
                a.type_name(),
                b.type_name()
            )),
        }
    }

    /// Build a Value from evaluated matrix literal rows.
    ///
    /// Each element in a row may be Scalar, Complex, Vector, or Matrix.
    /// Matrices in a row are concatenated horizontally; rows separated by `;`
    /// are concatenated vertically. Rows separated by `;`, elements by space or `,`.
    pub fn from_matrix_rows(rows: Vec<Vec<Value>>) -> Result<Value, String> {
        if rows.is_empty() {
            return Ok(Value::Vector(Array1::zeros(0)));
        }

        let mut all_rows: Vec<Vec<C64>> = Vec::new();

        for row in &rows {
            if row.is_empty() {
                continue;
            }

            // Determine how many actual matrix rows this "visual row" contributes.
            let mut height = 1usize;
            for val in row {
                if let Value::Matrix(m) = val {
                    if height == 1 {
                        height = m.nrows();
                    } else if height != m.nrows() {
                        return Err(format!(
                            "matrix concat: vertical dimension mismatch ({} vs {} rows)",
                            height,
                            m.nrows()
                        ));
                    }
                }
            }

            let mut actual_rows: Vec<Vec<C64>> = vec![Vec::new(); height];
            for val in row {
                match val {
                    Value::Scalar(n) => {
                        for r in 0..height {
                            actual_rows[r].push(Complex::new(*n, 0.0));
                        }
                    }
                    Value::Complex(c) => {
                        for r in 0..height {
                            actual_rows[r].push(*c);
                        }
                    }
                    Value::Vector(v) => {
                        if height != 1 {
                            return Err(format!(
                                "matrix concat: cannot mix a vector with matrix blocks of height {}",
                                height
                            ));
                        }
                        actual_rows[0].extend(v.iter().copied());
                    }
                    Value::Matrix(m) => {
                        for r in 0..height {
                            actual_rows[r].extend(m.row(r).iter().copied());
                        }
                    }
                    other => {
                        return Err(format!(
                            "matrix elements must be scalar, complex, vector, or matrix; got {}",
                            other.type_name()
                        ))
                    }
                }
            }
            all_rows.extend(actual_rows);
        }

        if all_rows.is_empty() {
            return Ok(Value::Vector(Array1::zeros(0)));
        }

        if all_rows.len() == 1 {
            Ok(Value::Vector(Array1::from_vec(
                all_rows.into_iter().next().unwrap(),
            )))
        } else {
            let ncols = all_rows[0].len();
            for (i, row) in all_rows.iter().enumerate() {
                if row.len() != ncols {
                    return Err(format!(
                        "matrix concat: row {} has {} columns, expected {}",
                        i + 1,
                        row.len(),
                        ncols
                    ));
                }
            }
            let nrows = all_rows.len();
            let flat: Vec<C64> = all_rows.into_iter().flatten().collect();
            let mat = Array2::from_shape_vec((nrows, ncols), flat).map_err(|e| e.to_string())?;
            Ok(Value::Matrix(mat))
        }
    }

    /// Convert to CVector (for passing to DSP functions).
    /// Accepts 1D vectors, scalars, and n×1 or 1×n matrices (column/row vectors).
    pub fn to_cvector(&self) -> Result<CVector, String> {
        match self {
            Value::Vector(v) => Ok(v.clone()),
            Value::Scalar(n) => Ok(Array1::from_vec(vec![Complex::new(*n, 0.0)])),
            Value::Complex(c) => Ok(Array1::from_vec(vec![*c])),
            Value::Matrix(m) if m.ncols() == 1 => Ok(m.column(0).to_owned()),
            Value::Matrix(m) if m.nrows() == 1 => Ok(m.row(0).to_owned()),
            Value::SparseVector(sv) => Ok(sv.to_dense()),
            other => Err(format!("expected vector, got {}", other.type_name())),
        }
    }

    /// Convert to real f64.
    pub fn to_scalar(&self) -> Result<f64, String> {
        match self {
            Value::Scalar(n) => Ok(*n),
            Value::Complex(c) if c.im.abs() < 1e-10 => Ok(c.re),
            other => Err(format!("expected scalar, got {}", other.type_name())),
        }
    }

    /// Convert to usize.
    pub fn to_usize(&self) -> Result<usize, String> {
        let n = self.to_scalar()?;
        if n < 0.0 || n.fract() != 0.0 {
            return Err(format!("expected non-negative integer, got {}", n));
        }
        Ok(n as usize)
    }

    /// Convert to String.
    pub fn to_str(&self) -> Result<String, String> {
        match self {
            Value::Str(s) => Ok(s.clone()),
            other => Err(format!("expected string, got {}", other.type_name())),
        }
    }

    /// Convert to Vec<String>.
    pub fn to_string_array(&self) -> Result<Vec<String>, String> {
        match self {
            Value::StringArray(v) => Ok(v.clone()),
            other => Err(format!("expected string_array, got {}", other.type_name())),
        }
    }

    /// Build a StringArray from evaluated cell elements (all must be strings).
    pub fn from_cell_elements(vals: Vec<Value>) -> Result<Value, String> {
        let mut strings = Vec::with_capacity(vals.len());
        for v in vals {
            match v {
                Value::Str(s) => strings.push(s),
                other => {
                    return Err(format!(
                        "cell array elements must be strings, got {}",
                        other.type_name()
                    ))
                }
            }
        }
        Ok(Value::StringArray(strings))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const MAX_ELEMS: usize = 8;

        match self {
            Value::Scalar(n) => write!(f, "{}", n),
            Value::Complex(c) => {
                if c.im >= 0.0 {
                    write!(f, "{}+{}j", c.re, c.im)
                } else {
                    write!(f, "{}{}j", c.re, c.im)
                }
            }
            Value::Vector(v) => {
                let n = v.len();
                write!(f, "[1×{}]", n)?;
                if n == 0 {
                    return Ok(());
                }
                write!(f, "  ")?;
                let show = n.min(MAX_ELEMS);
                for (i, c) in v.iter().take(show).enumerate() {
                    if i > 0 {
                        write!(f, "  ")?;
                    }
                    if c.im.abs() < 1e-12 {
                        write!(f, "{:.6}", c.re)?;
                    } else if c.im >= 0.0 {
                        write!(f, "{:.6}+{:.6}j", c.re, c.im)?;
                    } else {
                        write!(f, "{:.6}{:.6}j", c.re, c.im)?;
                    }
                }
                if n > MAX_ELEMS {
                    write!(f, "  ... ({} total)", n)?;
                }
                Ok(())
            }
            Value::Matrix(m) => {
                let nrows = m.nrows();
                let ncols = m.ncols();
                write!(f, "Matrix({}x{})", nrows, ncols)?;
                let show_rows = nrows.min(MAX_ELEMS);
                for r in 0..show_rows {
                    write!(f, "\n  [")?;
                    let show_cols = ncols.min(MAX_ELEMS);
                    for c_idx in 0..show_cols {
                        if c_idx > 0 {
                            write!(f, ", ")?;
                        }
                        let c = m[[r, c_idx]];
                        if c.im.abs() < 1e-12 {
                            write!(f, "{:.6}", c.re)?;
                        } else if c.im >= 0.0 {
                            write!(f, "{:.6}+{:.6}j", c.re, c.im)?;
                        } else {
                            write!(f, "{:.6}{:.6}j", c.re, c.im)?;
                        }
                    }
                    if ncols > MAX_ELEMS {
                        write!(f, ", ...")?;
                    }
                    write!(f, "]")?;
                }
                if nrows > MAX_ELEMS {
                    write!(f, "\n  ... ({} rows total)", nrows)?;
                }
                Ok(())
            }
            Value::Tensor3(t) => {
                let (m, n, p) = (t.shape()[0], t.shape()[1], t.shape()[2]);
                write!(f, "tensor3 [{}×{}×{}]", m, n, p)?;
                if m == 0 || n == 0 || p == 0 {
                    return Ok(());
                }
                // Show up to 2 pages (first and last) with matrix-style formatting.
                let show_pages: Vec<usize> = if p <= 2 { (0..p).collect() } else { vec![0, p - 1] };
                for (idx, &k) in show_pages.iter().enumerate() {
                    if idx > 0 && p > 2 {
                        write!(f, "\n  ... ({} pages omitted)", p - 2)?;
                    }
                    write!(f, "\n  (:, :, {}) =", k + 1)?;
                    let show_rows = m.min(MAX_ELEMS);
                    for r in 0..show_rows {
                        write!(f, "\n    [")?;
                        let show_cols = n.min(MAX_ELEMS);
                        for c_idx in 0..show_cols {
                            if c_idx > 0 {
                                write!(f, ", ")?;
                            }
                            let c = t[[r, c_idx, k]];
                            if c.im.abs() < 1e-12 {
                                write!(f, "{:.6}", c.re)?;
                            } else if c.im >= 0.0 {
                                write!(f, "{:.6}+{:.6}j", c.re, c.im)?;
                            } else {
                                write!(f, "{:.6}{:.6}j", c.re, c.im)?;
                            }
                        }
                        if n > MAX_ELEMS {
                            write!(f, ", ...")?;
                        }
                        write!(f, "]")?;
                    }
                    if m > MAX_ELEMS {
                        write!(f, "\n    ... ({} rows total)", m)?;
                    }
                }
                Ok(())
            }
            Value::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            Value::Str(s) => write!(f, "{}", s),
            Value::QFmt(spec) => {
                let int_bits = (spec.word - 1).saturating_sub(spec.frac);
                write!(
                    f,
                    "QFmt<{}-bit Q{}.{}, round={}, overflow={}>",
                    spec.word,
                    int_bits,
                    spec.frac,
                    spec.round.as_str(),
                    spec.overflow.as_str()
                )
            }
            Value::Tuple(vals) => {
                write!(f, "(")?;
                for (i, v) in vals.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, ")")
            }
            Value::Struct(fields) => {
                write!(f, "struct {{")?;
                let mut sorted: Vec<_> = fields.iter().collect();
                sorted.sort_by_key(|(k, _)| k.as_str());
                for (i, (key, val)) in sorted.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", key, val)?;
                }
                write!(f, "}}")
            }
            Value::All => write!(f, ":"),
            Value::None => write!(f, "None"),
            Value::Lambda { params, .. } => write!(f, "@({}) <expr>", params.join(", ")),
            Value::FuncHandle(name) => write!(f, "@{}", name),
            Value::FirState(buf) => {
                let n = buf.lock().unwrap().len();
                write!(f, "<fir_state {}>", n)
            }
            Value::AudioIn {
                sample_rate,
                frame_size,
            } => write!(f, "<audio_in {:.0} Hz / {}>", sample_rate, frame_size),
            Value::AudioOut {
                sample_rate,
                frame_size,
            } => write!(f, "<audio_out {:.0} Hz / {}>", sample_rate, frame_size),
            Value::LiveFigure(fig) => {
                if fig.lock().unwrap().is_some() {
                    write!(f, "<live_figure>")
                } else {
                    write!(f, "<live_figure closed>")
                }
            }
            Value::StateSpace { a, b, c, d } => {
                write!(
                    f,
                    "ss<{}-state, {} input, {} output>",
                    a.nrows(),
                    b.ncols(),
                    c.nrows()
                )?;
                write!(f, "\n  A: {}x{}", a.nrows(), a.ncols())?;
                write!(f, "  B: {}x{}", b.nrows(), b.ncols())?;
                write!(f, "  C: {}x{}", c.nrows(), c.ncols())?;
                write!(f, "  D: {}x{}", d.nrows(), d.ncols())
            }
            Value::SparseVector(sv) => {
                write!(f, "sparse [1×{}, nnz={}]", sv.len, sv.nnz())?;
                let show = sv.entries.len().min(MAX_ELEMS);
                for &(i, v) in sv.entries.iter().take(show) {
                    if v.im.abs() < 1e-12 {
                        write!(f, "  ({})->{:.6}", i + 1, v.re)?;
                    } else if v.im >= 0.0 {
                        write!(f, "  ({})->{:.6}+{:.6}j", i + 1, v.re, v.im)?;
                    } else {
                        write!(f, "  ({})->{:.6}{:.6}j", i + 1, v.re, v.im)?;
                    }
                }
                if sv.entries.len() > MAX_ELEMS {
                    write!(f, "  ... ({} total)", sv.entries.len())?;
                }
                Ok(())
            }
            Value::SparseMatrix(sm) => {
                write!(f, "sparse [{}×{}, nnz={}]", sm.rows, sm.cols, sm.nnz())?;
                let show = sm.entries.len().min(MAX_ELEMS);
                for &(r, c, v) in sm.entries.iter().take(show) {
                    if v.im.abs() < 1e-12 {
                        write!(f, "\n  ({},{})->{:.6}", r + 1, c + 1, v.re)?;
                    } else if v.im >= 0.0 {
                        write!(f, "\n  ({},{})->{:.6}+{:.6}j", r + 1, c + 1, v.re, v.im)?;
                    } else {
                        write!(f, "\n  ({},{})->{:.6}{:.6}j", r + 1, c + 1, v.re, v.im)?;
                    }
                }
                if sm.entries.len() > MAX_ELEMS {
                    write!(f, "\n  ... ({} total)", sm.entries.len())?;
                }
                Ok(())
            }
            Value::StringArray(arr) => {
                let n = arr.len();
                write!(f, "{{1×{n}}} ")?;
                let show = n.min(MAX_ELEMS);
                for (i, s) in arr.iter().take(show).enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\"", s)?;
                }
                if n > MAX_ELEMS {
                    write!(f, ", ... ({n} total)")?;
                }
                Ok(())
            }
            Value::TransferFn { num, den } => {
                let ns = format_poly(num);
                let ds = format_poly(den);
                // Suppress denominator if it is exactly 1
                if den.len() == 1 && (den[0] - 1.0).abs() < 1e-12 {
                    write!(f, "{}", ns)
                } else if den.len() == 1 {
                    write!(f, "{} / {}", ns, ds)
                } else {
                    // Parenthesise multi-term numerator to avoid ambiguity
                    let ns_display = if ns.contains(' ') {
                        format!("({})", ns)
                    } else {
                        ns
                    };
                    write!(f, "{} / ({})", ns_display, ds)
                }
            }
            Value::SparseFactor(sf) => write!(
                f,
                "<{} factor {}×{}, {}, nnz={}>",
                sf.kind(),
                sf.dim(),
                sf.dim(),
                sf.scalar(),
                sf.nnz()
            ),
        }
    }
}

// ── Comma-formatting helpers ─────────────────────────────────────────────────

/// Insert thousands-separator commas into the integer portion of a numeric string.
/// e.g. "1234567.89" → "1,234,567.89",  "-1234" → "-1,234"
pub fn insert_commas(s: &str) -> String {
    // Split off sign
    let (sign, rest) = if s.starts_with('-') {
        ("-", &s[1..])
    } else {
        ("", s.as_ref())
    };
    // Split at decimal point (or 'e'/'E' for scientific notation)
    let (int_part, suffix) = if let Some(dot) = rest.find('.') {
        (&rest[..dot], &rest[dot..])
    } else if let Some(ep) = rest.find('e').or_else(|| rest.find('E')) {
        (&rest[..ep], &rest[ep..])
    } else {
        (rest, "")
    };
    if int_part.len() <= 3 {
        return format!("{}{}{}", sign, int_part, suffix);
    }
    let mut result = String::new();
    let digits: Vec<char> = int_part.chars().collect();
    let n = digits.len();
    for (i, ch) in digits.iter().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            result.push(',');
        }
        result.push(*ch);
    }
    format!("{}{}{}", sign, result, suffix)
}

impl Value {
    /// Deep clone for snapshot/restore use. Differs from `Clone` only for
    /// the `Arc<Mutex<…>>` variants whose interiors are mutated through
    /// `&self` (e.g. `FirState` accumulating history between blocks). A
    /// plain `Clone` of those keeps the same `Arc`, so mutating the live
    /// value also mutates the snapshot and vice versa. `deep_clone` makes
    /// the snapshot independent by cloning the inner data into a fresh
    /// `Arc<Mutex<_>>`.
    ///
    /// `LiveFigure` is left shared on purpose — it owns an external
    /// resource (terminal or window) that cannot be meaningfully
    /// duplicated. Notebooks using `live_figure()` are an explicit
    /// known limitation of the snapshot cache.
    ///
    /// Read-only `Arc` variants (`SparseFactor`'s cached factorizations)
    /// are safe to share and aren't deep-cloned — sharing is the whole
    /// point of caching the factor.
    pub fn deep_clone(&self) -> Value {
        match self {
            Value::FirState(arc) => {
                let inner = arc.lock().unwrap().clone();
                Value::FirState(Arc::new(Mutex::new(inner)))
            }
            Value::Struct(fields) => {
                Value::Struct(fields.iter().map(|(k, v)| (k.clone(), v.deep_clone())).collect())
            }
            Value::Tuple(items) => Value::Tuple(items.iter().map(|v| v.deep_clone()).collect()),
            Value::Lambda { params, body, captured_env } => Value::Lambda {
                params: params.clone(),
                body: body.clone(),
                captured_env: captured_env
                    .iter()
                    .map(|(k, v)| (k.clone(), v.deep_clone()))
                    .collect(),
            },
            // Everything else is either plain data (Scalar, Vector, Matrix,
            // etc.) or an Arc-shared read-only handle (SparseFactor) or an
            // external-resource handle we intentionally leave shared
            // (LiveFigure). For those, `Clone` already does the right thing.
            other => other.clone(),
        }
    }

    /// Format this value according to the active `NumberFormat`.
    pub fn format_display(&self, nf: NumberFormat) -> String {
        if nf == NumberFormat::Short {
            return format!("{}", self);
        }
        const MAX_ELEMS: usize = 8;

        // ── per-format scalar helpers ──────────────────────────────────────

        fn fmt_scalar(n: f64, nf: NumberFormat) -> String {
            match nf {
                NumberFormat::Short => format!("{}", n),
                NumberFormat::Long => format!("{:.15}", n),
                NumberFormat::Hex => format!("{:016x}", n.to_bits()),
                NumberFormat::Commas => insert_commas(&format!("{}", n)),
            }
        }

        fn fmt_scalar_prec(n: f64, nf: NumberFormat) -> String {
            match nf {
                NumberFormat::Short => format!("{:.6}", n),
                NumberFormat::Long => format!("{:.15}", n),
                NumberFormat::Hex => format!("{:016x}", n.to_bits()),
                NumberFormat::Commas => insert_commas(&format!("{:.6}", n)),
            }
        }

        fn fmt_complex(c: &C64, nf: NumberFormat) -> String {
            if nf == NumberFormat::Hex {
                if c.im == 0.0 {
                    return fmt_scalar(c.re, nf);
                }
                return format!("{}+{}j", fmt_scalar(c.re, nf), fmt_scalar(c.im, nf));
            }
            if c.im >= 0.0 {
                format!("{}+{}j", fmt_scalar(c.re, nf), fmt_scalar(c.im, nf))
            } else {
                format!("{}{}j", fmt_scalar(c.re, nf), fmt_scalar(c.im, nf))
            }
        }

        fn fmt_complex_prec(c: &C64, nf: NumberFormat) -> String {
            if nf == NumberFormat::Hex {
                if c.im == 0.0 {
                    return fmt_scalar(c.re, nf);
                }
                return format!("{}+{}j", fmt_scalar(c.re, nf), fmt_scalar(c.im, nf));
            }
            if c.im.abs() < 1e-12 {
                fmt_scalar_prec(c.re, nf)
            } else if c.im >= 0.0 {
                format!(
                    "{}+{}j",
                    fmt_scalar_prec(c.re, nf),
                    fmt_scalar_prec(c.im, nf)
                )
            } else {
                format!(
                    "{}{}j",
                    fmt_scalar_prec(c.re, nf),
                    fmt_scalar_prec(c.im, nf)
                )
            }
        }

        // ── dispatch on value type ─────────────────────────────────────────

        match self {
            Value::Scalar(n) => fmt_scalar(*n, nf),
            Value::Complex(c) => fmt_complex(c, nf),
            Value::Vector(v) => {
                let n = v.len();
                let mut s = format!("[1×{}]", n);
                if n == 0 {
                    return s;
                }
                s.push_str("  ");
                let show = n.min(MAX_ELEMS);
                for (i, c) in v.iter().take(show).enumerate() {
                    if i > 0 {
                        s.push_str("  ");
                    }
                    s.push_str(&fmt_complex_prec(c, nf));
                }
                if n > MAX_ELEMS {
                    s.push_str(&format!("  ... ({} total)", n));
                }
                s
            }
            Value::Matrix(m) => {
                let nrows = m.nrows();
                let ncols = m.ncols();
                let mut s = format!("Matrix({}x{})", nrows, ncols);
                let show_rows = nrows.min(MAX_ELEMS);
                for r in 0..show_rows {
                    s.push_str("\n  [");
                    let show_cols = ncols.min(MAX_ELEMS);
                    for c_idx in 0..show_cols {
                        if c_idx > 0 {
                            s.push_str(", ");
                        }
                        let c = m[[r, c_idx]];
                        s.push_str(&fmt_complex_prec(&c, nf));
                    }
                    if ncols > MAX_ELEMS {
                        s.push_str(", ...");
                    }
                    s.push(']');
                }
                if nrows > MAX_ELEMS {
                    s.push_str(&format!("\n  ... ({} rows total)", nrows));
                }
                s
            }
            other => format!("{}", other),
        }
    }
}

// ── Polynomial helpers (used by Value::TransferFn arithmetic and Display) ────

/// Multiply two polynomials (descending-power coefficients).
pub(crate) fn poly_mul(a: &[f64], b: &[f64]) -> Vec<f64> {
    if a.is_empty() || b.is_empty() {
        return vec![0.0];
    }
    let n = a.len() + b.len() - 1;
    let mut out = vec![0.0f64; n];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            out[i + j] += ai * bj;
        }
    }
    out
}

/// Add two polynomials, aligning by degree.
pub(crate) fn poly_add(a: &[f64], b: &[f64]) -> Vec<f64> {
    let na = a.len();
    let nb = b.len();
    let n = na.max(nb);
    let mut out = vec![0.0f64; n];
    for (i, &ai) in a.iter().enumerate() {
        out[i + (n - na)] += ai;
    }
    for (i, &bi) in b.iter().enumerate() {
        out[i + (n - nb)] += bi;
    }
    out
}

/// Subtract polynomial `b` from `a`, aligning by degree.
pub(crate) fn poly_sub(a: &[f64], b: &[f64]) -> Vec<f64> {
    let neg: Vec<f64> = b.iter().map(|&x| -x).collect();
    poly_add(a, &neg)
}

/// Scale all coefficients of a polynomial by a constant.
pub(crate) fn poly_scale(a: &[f64], s: f64) -> Vec<f64> {
    a.iter().map(|&x| x * s).collect()
}

/// Format a polynomial (descending-power) as a human-readable string.
/// e.g. [1.0, 2.0, 10.0] → "s^2 + 2s + 10"
fn format_poly(coeffs: &[f64]) -> String {
    if coeffs.is_empty() {
        return "0".to_string();
    }
    let deg = coeffs.len() - 1;
    let mut out = String::new();
    let mut first = true;
    for (i, &c) in coeffs.iter().enumerate() {
        let power = deg - i;
        if c.abs() < 1e-12 {
            continue;
        }
        let neg = c < 0.0;
        let ac = c.abs();
        if first {
            if neg {
                out.push('-');
            }
            first = false;
        } else if neg {
            out.push_str(" - ");
        } else {
            out.push_str(" + ");
        }
        match power {
            0 => out.push_str(&fmt_f64(ac)),
            1 => {
                if (ac - 1.0).abs() > 1e-12 {
                    out.push_str(&fmt_f64(ac));
                }
                out.push('s');
            }
            p => {
                if (ac - 1.0).abs() > 1e-12 {
                    out.push_str(&fmt_f64(ac));
                }
                out.push_str(&format!("s^{}", p));
            }
        }
    }
    if out.is_empty() {
        "0".to_string()
    } else {
        out
    }
}

/// Format an f64 without a trailing `.0` when it is a whole number.
fn fmt_f64(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

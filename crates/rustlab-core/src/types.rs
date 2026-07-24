use ndarray::{Array1, Array2, Array3};
use num_complex::Complex;

/// Complex 64-bit float (the native scalar type throughout rustlab)
pub type C64 = Complex<f64>;
/// Complex column vector
pub type CVector = Array1<C64>;
/// Complex matrix
pub type CMatrix = Array2<C64>;
/// Complex rank-3 tensor, shape `(m, n, p)` — rows × cols × pages
pub type CTensor3 = Array3<C64>;
/// Real vector
pub type RVector = Array1<f64>;
/// Real matrix
pub type RMatrix = Array2<f64>;

/// Near-zero threshold: entries with norm below this are dropped from sparse structures.
const SPARSE_ZERO_TOL: f64 = 1e-15;

/// Sparse vector in COO format.  Entries are sorted by index (0-based internally).
#[derive(Debug, Clone, PartialEq)]
pub struct SparseVec {
    pub len: usize,
    pub entries: Vec<(usize, C64)>,
}

impl SparseVec {
    /// Construct a sparse vector, deduplicating indices (summing duplicates),
    /// dropping near-zeros, and sorting by index.
    pub fn new(len: usize, raw: Vec<(usize, C64)>) -> Self {
        use std::collections::HashMap;
        let mut map: HashMap<usize, C64> = HashMap::new();
        for (i, v) in raw {
            *map.entry(i).or_insert(Complex::new(0.0, 0.0)) += v;
        }
        let mut entries: Vec<(usize, C64)> = map
            .into_iter()
            .filter(|(_, v)| v.norm() >= SPARSE_ZERO_TOL)
            .collect();
        entries.sort_by_key(|(i, _)| *i);
        Self { len, entries }
    }

    pub fn nnz(&self) -> usize {
        self.entries.len()
    }

    /// Look up by 0-based index; returns 0 if absent.
    pub fn get(&self, idx: usize) -> C64 {
        self.entries
            .iter()
            .find(|(i, _)| *i == idx)
            .map(|(_, v)| *v)
            .unwrap_or(Complex::new(0.0, 0.0))
    }

    /// Set a 0-based entry.  Setting to ~0 removes it.
    pub fn set(&mut self, idx: usize, val: C64) {
        // Remove existing
        self.entries.retain(|(i, _)| *i != idx);
        if val.norm() >= SPARSE_ZERO_TOL {
            self.entries.push((idx, val));
            self.entries.sort_by_key(|(i, _)| *i);
        }
    }

    /// Convert to a dense CVector.
    pub fn to_dense(&self) -> CVector {
        let mut v = Array1::from_elem(self.len, Complex::new(0.0, 0.0));
        for &(i, val) in &self.entries {
            v[i] = val;
        }
        v
    }

    /// Create from a dense CVector, dropping near-zeros.
    pub fn from_dense(v: &CVector) -> Self {
        let entries: Vec<(usize, C64)> = v
            .iter()
            .enumerate()
            .filter(|(_, c)| c.norm() >= SPARSE_ZERO_TOL)
            .map(|(i, &c)| (i, c))
            .collect();
        Self {
            len: v.len(),
            entries,
        }
    }

    /// Scale all entries by a complex scalar.
    pub fn scale(&self, c: C64) -> Self {
        let entries: Vec<(usize, C64)> = self
            .entries
            .iter()
            .map(|&(i, v)| (i, v * c))
            .filter(|(_, v)| v.norm() >= SPARSE_ZERO_TOL)
            .collect();
        Self {
            len: self.len,
            entries,
        }
    }

    /// Add two sparse vectors (must have equal length).
    pub fn add(&self, other: &SparseVec) -> Result<Self, String> {
        if self.len != other.len {
            return Err(format!(
                "sparse vector add: length mismatch ({} vs {})",
                self.len, other.len
            ));
        }
        let mut combined = self.entries.clone();
        combined.extend_from_slice(&other.entries);
        Ok(Self::new(self.len, combined))
    }

    /// Subtract another sparse vector.
    pub fn sub(&self, other: &SparseVec) -> Result<Self, String> {
        self.add(&other.scale(Complex::new(-1.0, 0.0)))
    }

    /// Dot product of two sparse vectors.
    pub fn dot(&self, other: &SparseVec) -> C64 {
        // Walk both sorted entry lists with a merge
        let mut sum = Complex::new(0.0, 0.0);
        let (mut ai, mut bi) = (0, 0);
        while ai < self.entries.len() && bi < other.entries.len() {
            let (a_idx, a_val) = self.entries[ai];
            let (b_idx, b_val) = other.entries[bi];
            match a_idx.cmp(&b_idx) {
                std::cmp::Ordering::Less => ai += 1,
                std::cmp::Ordering::Greater => bi += 1,
                std::cmp::Ordering::Equal => {
                    sum += a_val * b_val;
                    ai += 1;
                    bi += 1;
                }
            }
        }
        sum
    }

    /// Dot product of sparse vector with a dense vector.
    pub fn dot_dense(&self, dv: &CVector) -> C64 {
        self.entries.iter().map(|&(i, v)| v * dv[i]).sum()
    }
}

/// Hint that fixes a fill-reducing ordering for the sparse-solve dispatch.
///
/// Builders that produce structurally regular matrices (notably the
/// `laplacian_*` family on a regular grid) attach an `OrderingHint`
/// describing the cheapest ordering for that pattern. The script-layer
/// `spsolve` / `chol` / `lu` dispatch consults the hint before defaulting
/// to AMD; an explicit user-provided ordering still wins.
///
/// Identity (natural) ordering on a 2-D Laplacian factors roughly 5×
/// faster than AMD because the natural ordering already matches the
/// banded fill pattern of a 5-point stencil. AMD's reordering search is
/// pure overhead in that case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderingHint {
    /// Use the identity permutation (natural ordering). The right choice
    /// for grid-natural Laplacians and other matrices whose nonzero
    /// pattern is already in a fill-friendly order.
    Identity,
}

/// Combine two optional ordering hints. Used by structure-preserving
/// arithmetic (`SparseMat::add` / `::sub`) to decide what hint, if any,
/// the result should carry. Conservative rule:
///
/// - **Both `Some` and equal → propagate.** Both operands made the same
///   structural claim; their union still satisfies it. (The canonical
///   case: `L + α·speye(N)` where both `L` and `speye` carry
///   `Identity`. Diagonal additions preserve the nonzero pattern, so
///   identity ordering remains optimal.)
/// - **Anything else → `None`.** A user-built `sparse(I, J, V, m, n)`
///   carries no hint — adding it to a hinted matrix can introduce
///   nonzeros outside the original pattern, so the structural claim
///   no longer holds. Drop to be safe.
///
/// Users who know the result is still grid-banded can re-attach the
/// hint via `with_ordering_hint` or pass `"identity"` explicitly to
/// `spsolve` / `chol` / `lu`.
#[inline]
pub(crate) fn merge_hints(
    a: Option<OrderingHint>,
    b: Option<OrderingHint>,
) -> Option<OrderingHint> {
    match (a, b) {
        (Some(x), Some(y)) if x == y => Some(x),
        _ => None,
    }
}

/// Sparse matrix in COO format.  Entries are sorted row-major (0-based internally).
#[derive(Debug, Clone, PartialEq)]
pub struct SparseMat {
    pub rows: usize,
    pub cols: usize,
    pub entries: Vec<(usize, usize, C64)>,
    /// Optional fill-reducing ordering hint set by structurally-regular
    /// builders. `None` means "no opinion — solver picks its default".
    /// Operations that may scramble the structure (add, sub, from_dense)
    /// drop the hint; structure-preserving operations (scale, transpose,
    /// set) keep it.
    pub ordering_hint: Option<OrderingHint>,
}

impl SparseMat {
    /// Construct a sparse matrix, deduplicating (row,col) pairs (summing duplicates),
    /// dropping near-zeros, and sorting row-major.
    pub fn new(rows: usize, cols: usize, raw: Vec<(usize, usize, C64)>) -> Self {
        use std::collections::HashMap;
        let mut map: HashMap<(usize, usize), C64> = HashMap::new();
        for (r, c, v) in raw {
            *map.entry((r, c)).or_insert(Complex::new(0.0, 0.0)) += v;
        }
        let mut entries: Vec<(usize, usize, C64)> = map
            .into_iter()
            .filter(|(_, v)| v.norm() >= SPARSE_ZERO_TOL)
            .map(|((r, c), v)| (r, c, v))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        Self {
            rows,
            cols,
            entries,
            ordering_hint: None,
        }
    }

    /// Builder-style setter for the ordering hint. Used by structural
    /// builders (`laplacian_*`) to declare "natural ordering is best
    /// for this matrix" without forcing every constructor signature to
    /// take an extra argument.
    pub fn with_ordering_hint(mut self, hint: OrderingHint) -> Self {
        self.ordering_hint = Some(hint);
        self
    }

    /// Fast path constructor for callers that already produce entries
    /// in row-major-then-column-major sorted order. Skips the HashMap
    /// dedupe + full sort that `SparseMat::new` does — runs in O(nnz)
    /// vs O(nnz log nnz). Consecutive duplicate `(row, col)` entries
    /// are summed (handles periodic-BC corner cases at minimum grid
    /// sizes where the wrap column coincides with an interior column).
    /// Near-zero entries are dropped.
    ///
    /// **Caller's contract:** `entries` is sorted ascending by
    /// `(row, col)` lexicographically. Violating the contract produces
    /// an incorrect matrix (entries land in the wrong rows/cols and
    /// downstream `to_csc` quietly mis-files them). Use `SparseMat::new`
    /// when ordering is uncertain.
    ///
    /// `ordering_hint` defaults to `None`. Use `with_ordering_hint` if
    /// the caller knows a fill-friendly ordering applies.
    pub fn from_sorted_entries(
        rows: usize,
        cols: usize,
        entries: Vec<(usize, usize, C64)>,
    ) -> Self {
        if entries.is_empty() {
            return Self {
                rows,
                cols,
                entries,
                ordering_hint: None,
            };
        }
        // Single pass: merge consecutive duplicates by summing values,
        // drop near-zeros. Walks the input sequentially — no allocation
        // beyond the output buffer.
        let mut out: Vec<(usize, usize, C64)> = Vec::with_capacity(entries.len());
        let mut iter = entries.into_iter();
        let (mut cur_r, mut cur_c, mut cur_v) = iter.next().unwrap();
        for (r, c, v) in iter {
            if r == cur_r && c == cur_c {
                cur_v += v;
            } else {
                if cur_v.norm() >= SPARSE_ZERO_TOL {
                    out.push((cur_r, cur_c, cur_v));
                }
                cur_r = r;
                cur_c = c;
                cur_v = v;
            }
        }
        if cur_v.norm() >= SPARSE_ZERO_TOL {
            out.push((cur_r, cur_c, cur_v));
        }
        Self {
            rows,
            cols,
            entries: out,
            ordering_hint: None,
        }
    }

    pub fn nnz(&self) -> usize {
        self.entries.len()
    }

    /// Look up by 0-based (row, col); returns 0 if absent.
    pub fn get(&self, row: usize, col: usize) -> C64 {
        self.entries
            .iter()
            .find(|(r, c, _)| *r == row && *c == col)
            .map(|(_, _, v)| *v)
            .unwrap_or(Complex::new(0.0, 0.0))
    }

    /// Set a 0-based entry.  Setting to ~0 removes it.
    pub fn set(&mut self, row: usize, col: usize, val: C64) {
        self.entries.retain(|(r, c, _)| !(*r == row && *c == col));
        if val.norm() >= SPARSE_ZERO_TOL {
            self.entries.push((row, col, val));
            self.entries
                .sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        }
    }

    /// Convert to a dense CMatrix.
    pub fn to_dense(&self) -> CMatrix {
        let mut m = Array2::from_elem((self.rows, self.cols), Complex::new(0.0, 0.0));
        for &(r, c, val) in &self.entries {
            m[[r, c]] = val;
        }
        m
    }

    /// Create from a dense CMatrix, dropping near-zeros.
    pub fn from_dense(m: &CMatrix) -> Self {
        let mut entries = Vec::new();
        for r in 0..m.nrows() {
            for c in 0..m.ncols() {
                let v = m[[r, c]];
                if v.norm() >= SPARSE_ZERO_TOL {
                    entries.push((r, c, v));
                }
            }
        }
        Self {
            rows: m.nrows(),
            cols: m.ncols(),
            entries,
            ordering_hint: None,
        }
    }

    /// Scale all entries by a complex scalar. Preserves `ordering_hint`
    /// because scaling does not change the nonzero pattern.
    pub fn scale(&self, c: C64) -> Self {
        let entries: Vec<(usize, usize, C64)> = self
            .entries
            .iter()
            .map(|&(r, col, v)| (r, col, v * c))
            .filter(|(_, _, v)| v.norm() >= SPARSE_ZERO_TOL)
            .collect();
        Self {
            rows: self.rows,
            cols: self.cols,
            entries,
            ordering_hint: self.ordering_hint,
        }
    }

    /// Add two sparse matrices (must have equal dimensions).
    ///
    /// Preserves `ordering_hint` when both operands agree on a hint, or
    /// when one is `Some(h)` and the other is `None` (None means "no
    /// opinion" and shouldn't override the explicit hint). This lets
    /// `L + α·speye(N)` keep `Identity` from the grid-Laplacian operand
    /// — the diagonal shift never changes the nonzero pattern, so the
    /// hint remains structurally correct.
    pub fn add(&self, other: &SparseMat) -> Result<Self, String> {
        if self.rows != other.rows || self.cols != other.cols {
            return Err(format!(
                "sparse matrix add: dimension mismatch ({}×{} vs {}×{})",
                self.rows, self.cols, other.rows, other.cols
            ));
        }
        let mut combined = self.entries.clone();
        combined.extend_from_slice(&other.entries);
        let mut out = Self::new(self.rows, self.cols, combined);
        out.ordering_hint = merge_hints(self.ordering_hint, other.ordering_hint);
        Ok(out)
    }

    /// Subtract another sparse matrix. Hint propagation matches `add`.
    pub fn sub(&self, other: &SparseMat) -> Result<Self, String> {
        self.add(&other.scale(Complex::new(-1.0, 0.0)))
    }

    /// Non-conjugate transpose: swap row/col indices. Preserves
    /// `ordering_hint` because identity ordering on `A^T` is still
    /// identity ordering, and grid-banded patterns transpose to grid-
    /// banded patterns.
    pub fn transpose(&self) -> Self {
        let mut entries: Vec<(usize, usize, C64)> =
            self.entries.iter().map(|&(r, c, v)| (c, r, v)).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        Self {
            rows: self.cols,
            cols: self.rows,
            entries,
            ordering_hint: self.ordering_hint,
        }
    }

    /// Sparse matrix × dense vector (SpMV), O(nnz).
    pub fn spmv(&self, x: &CVector) -> Result<CVector, String> {
        if self.cols != x.len() {
            return Err(format!(
                "spmv: matrix is {}×{} but vector has length {}",
                self.rows,
                self.cols,
                x.len()
            ));
        }
        let mut y = Array1::from_elem(self.rows, Complex::new(0.0, 0.0));
        for &(r, c, v) in &self.entries {
            y[r] += v * x[c];
        }
        Ok(y)
    }

    /// Sparse matrix × dense matrix (SpMM), O(nnz * B.ncols).
    pub fn spmm(&self, b: &CMatrix) -> Result<CMatrix, String> {
        if self.cols != b.nrows() {
            return Err(format!(
                "spmm: matrix is {}×{} but rhs is {}×{}",
                self.rows,
                self.cols,
                b.nrows(),
                b.ncols()
            ));
        }
        let mut c = Array2::from_elem((self.rows, b.ncols()), Complex::new(0.0, 0.0));
        for &(r, k, v) in &self.entries {
            for j in 0..b.ncols() {
                c[[r, j]] += v * b[[k, j]];
            }
        }
        Ok(c)
    }

    /// Test whether the matrix is Hermitian within `tol` per-entry: for
    /// every stored entry `(i, j, v)`, there exists a stored entry
    /// `(j, i, conj(v))` (within tolerance), and the diagonal entries
    /// have negligible imaginary component. Returns `false` for
    /// non-square matrices.
    pub fn is_hermitian(&self, tol: f64) -> bool {
        if self.rows != self.cols {
            return false;
        }
        // Build a map (i, j) -> value for O(1) lookup.
        use std::collections::HashMap;
        let mut map: HashMap<(usize, usize), C64> = HashMap::with_capacity(self.entries.len());
        for &(r, c, v) in &self.entries {
            map.insert((r, c), v);
        }
        for &(r, c, v) in &self.entries {
            if r == c {
                if v.im.abs() > tol {
                    return false;
                }
            } else {
                let mirror = match map.get(&(c, r)) {
                    Some(&m) => m,
                    None => return false,
                };
                let expected = Complex::new(v.re, -v.im);
                if (mirror - expected).norm() > tol {
                    return false;
                }
            }
        }
        true
    }

    /// Quick SPD-ish estimate: Hermitian AND every diagonal entry is
    /// real-positive within `tol`. This is necessary for SPD but not
    /// sufficient — an SPD-failure during factorization is still possible
    /// for indefinite Hermitian matrices that pass the diagonal check.
    /// Used as a cheap pre-filter to decide whether to attempt Cholesky.
    pub fn is_spd_estimate(&self, tol: f64) -> bool {
        if !self.is_hermitian(tol) {
            return false;
        }
        // Diagonals must all be present and real-positive.
        let mut seen_diag = vec![false; self.rows];
        for &(r, c, v) in &self.entries {
            if r == c {
                if v.re <= tol || v.im.abs() > tol {
                    return false;
                }
                seen_diag[r] = true;
            }
        }
        seen_diag.iter().all(|&s| s)
    }
}

/// Fixed-point rounding mode.
#[derive(Debug, Clone, PartialEq)]
pub enum RoundMode {
    /// Truncate toward −∞ — free in hardware (default).
    Floor,
    /// Toward +∞.
    Ceil,
    /// Truncate toward zero (symmetric floor).
    Zero,
    /// Round half away from zero.
    Round,
    /// Round half to even (convergent / banker's rounding).
    RoundEven,
}

impl RoundMode {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "floor" | "truncate" | "trunc" => Some(Self::Floor),
            "ceil" => Some(Self::Ceil),
            "zero" => Some(Self::Zero),
            "round" => Some(Self::Round),
            "round_even" | "even" | "convergent" => Some(Self::RoundEven),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Floor => "floor",
            Self::Ceil => "ceil",
            Self::Zero => "zero",
            Self::Round => "round",
            Self::RoundEven => "round_even",
        }
    }
}

/// Fixed-point overflow mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowMode {
    /// Clamp to [min, max] (default).
    Saturate,
    /// 2's complement wrap.
    Wrap,
}

impl OverflowMode {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "saturate" | "sat" => Some(Self::Saturate),
            "wrap" => Some(Self::Wrap),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Saturate => "saturate",
            Self::Wrap => "wrap",
        }
    }
}

/// Integer class (width + signedness) for the tagged-width integer value type.
/// Values are stored in `i128` so the full `uint64` range is representable;
/// the class enforces the visible range on construction and arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntClass {
    Int8,
    Int16,
    Int32,
    Int64,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
}

impl IntClass {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "int8" => Some(Self::Int8),
            "int16" => Some(Self::Int16),
            "int32" => Some(Self::Int32),
            "int64" => Some(Self::Int64),
            "uint8" => Some(Self::Uint8),
            "uint16" => Some(Self::Uint16),
            "uint32" => Some(Self::Uint32),
            "uint64" => Some(Self::Uint64),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Int8 => "int8",
            Self::Int16 => "int16",
            Self::Int32 => "int32",
            Self::Int64 => "int64",
            Self::Uint8 => "uint8",
            Self::Uint16 => "uint16",
            Self::Uint32 => "uint32",
            Self::Uint64 => "uint64",
        }
    }

    pub fn is_signed(&self) -> bool {
        matches!(self, Self::Int8 | Self::Int16 | Self::Int32 | Self::Int64)
    }

    /// Bit width (8, 16, 32, or 64).
    pub fn bits(&self) -> u32 {
        match self {
            Self::Int8 | Self::Uint8 => 8,
            Self::Int16 | Self::Uint16 => 16,
            Self::Int32 | Self::Uint32 => 32,
            Self::Int64 | Self::Uint64 => 64,
        }
    }

    /// Inclusive minimum representable value.
    pub fn min(&self) -> i128 {
        if self.is_signed() {
            -(1i128 << (self.bits() - 1))
        } else {
            0
        }
    }

    /// Inclusive maximum representable value.
    pub fn max(&self) -> i128 {
        if self.is_signed() {
            (1i128 << (self.bits() - 1)) - 1
        } else {
            (1i128 << self.bits()) - 1
        }
    }

    /// Range-enforce `v` under `mode`: `Saturate` clamps to `[min, max]`,
    /// `Wrap` reduces modulo `2^bits` (2's-complement wrap).
    pub fn coerce(&self, v: i128, mode: OverflowMode) -> i128 {
        match mode {
            OverflowMode::Saturate => v.clamp(self.min(), self.max()),
            OverflowMode::Wrap => {
                let modulus = 1i128 << self.bits(); // 2^bits; fits i128 for bits ≤ 64
                let r = v.rem_euclid(modulus); // 0 .. modulus-1
                if self.is_signed() && r > self.max() {
                    r - modulus
                } else {
                    r
                }
            }
        }
    }

    /// Convert a float to this class: round half away from zero (matching the
    /// integer-cast convention), then range-enforce under `mode`. `NaN → 0`.
    pub fn from_f64(&self, x: f64, mode: OverflowMode) -> i128 {
        if x.is_nan() {
            return 0;
        }
        // `f64 as i128` saturates on overflow (Rust ≥ 1.45); `coerce` then
        // clamps/wraps into the class range. `round()` is half-away-from-zero.
        self.coerce(x.round() as i128, mode)
    }

    /// Smallest unsigned class that holds a non-negative value, for literal
    /// typing (`0xFF → uint8`). `None` if it exceeds `uint64`.
    pub fn smallest_unsigned_for(v: u128) -> Option<Self> {
        if v <= u8::MAX as u128 {
            Some(Self::Uint8)
        } else if v <= u16::MAX as u128 {
            Some(Self::Uint16)
        } else if v <= u32::MAX as u128 {
            Some(Self::Uint32)
        } else if v <= u64::MAX as u128 {
            Some(Self::Uint64)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── RoundMode ───────────────────────────────────────────────────────────

    #[test]
    fn round_mode_from_str_all_variants() {
        assert_eq!(RoundMode::from_str("floor"), Some(RoundMode::Floor));
        assert_eq!(RoundMode::from_str("ceil"), Some(RoundMode::Ceil));
        assert_eq!(RoundMode::from_str("zero"), Some(RoundMode::Zero));
        assert_eq!(RoundMode::from_str("round"), Some(RoundMode::Round));
        assert_eq!(
            RoundMode::from_str("round_even"),
            Some(RoundMode::RoundEven)
        );
    }

    #[test]
    fn round_mode_aliases() {
        assert_eq!(RoundMode::from_str("truncate"), Some(RoundMode::Floor));
        assert_eq!(RoundMode::from_str("trunc"), Some(RoundMode::Floor));
        assert_eq!(RoundMode::from_str("even"), Some(RoundMode::RoundEven));
        assert_eq!(
            RoundMode::from_str("convergent"),
            Some(RoundMode::RoundEven)
        );
    }

    #[test]
    fn round_mode_case_insensitive() {
        assert_eq!(RoundMode::from_str("FLOOR"), Some(RoundMode::Floor));
        assert_eq!(
            RoundMode::from_str("Round_Even"),
            Some(RoundMode::RoundEven)
        );
    }

    #[test]
    fn round_mode_hyphen_alias() {
        assert_eq!(
            RoundMode::from_str("round-even"),
            Some(RoundMode::RoundEven)
        );
    }

    #[test]
    fn round_mode_unknown_returns_none() {
        assert_eq!(RoundMode::from_str("banana"), None);
        assert_eq!(RoundMode::from_str(""), None);
    }

    #[test]
    fn round_mode_round_trip() {
        for mode in [
            RoundMode::Floor,
            RoundMode::Ceil,
            RoundMode::Zero,
            RoundMode::Round,
            RoundMode::RoundEven,
        ] {
            assert_eq!(RoundMode::from_str(mode.as_str()), Some(mode));
        }
    }

    // ── OverflowMode ────────────────────────────────────────────────────────

    #[test]
    fn overflow_mode_from_str_all_variants() {
        assert_eq!(
            OverflowMode::from_str("saturate"),
            Some(OverflowMode::Saturate)
        );
        assert_eq!(OverflowMode::from_str("wrap"), Some(OverflowMode::Wrap));
    }

    #[test]
    fn overflow_mode_aliases() {
        assert_eq!(OverflowMode::from_str("sat"), Some(OverflowMode::Saturate));
    }

    #[test]
    fn overflow_mode_case_insensitive() {
        assert_eq!(
            OverflowMode::from_str("SATURATE"),
            Some(OverflowMode::Saturate)
        );
        assert_eq!(OverflowMode::from_str("Wrap"), Some(OverflowMode::Wrap));
    }

    #[test]
    fn overflow_mode_unknown_returns_none() {
        assert_eq!(OverflowMode::from_str("clamp"), None);
        assert_eq!(OverflowMode::from_str(""), None);
    }

    #[test]
    fn overflow_mode_round_trip() {
        for mode in [OverflowMode::Saturate, OverflowMode::Wrap] {
            assert_eq!(OverflowMode::from_str(mode.as_str()), Some(mode));
        }
    }

    // ── IntClass ────────────────────────────────────────────────────────────

    #[test]
    fn int_class_ranges() {
        assert_eq!((IntClass::Int8.min(), IntClass::Int8.max()), (-128, 127));
        assert_eq!((IntClass::Uint8.min(), IntClass::Uint8.max()), (0, 255));
        assert_eq!(IntClass::Int32.min(), -2_147_483_648);
        assert_eq!(IntClass::Int32.max(), 2_147_483_647);
        assert_eq!(IntClass::Uint64.max(), u64::MAX as i128); // full range, exact in i128
        assert_eq!(IntClass::Int64.min(), i64::MIN as i128);
    }

    #[test]
    fn int_class_from_str_round_trip() {
        for c in [
            IntClass::Int8,
            IntClass::Int16,
            IntClass::Int32,
            IntClass::Int64,
            IntClass::Uint8,
            IntClass::Uint16,
            IntClass::Uint32,
            IntClass::Uint64,
        ] {
            assert_eq!(IntClass::from_str(c.name()), Some(c));
        }
        assert_eq!(IntClass::from_str("INT8"), Some(IntClass::Int8));
        assert_eq!(IntClass::from_str("single"), None);
    }

    #[test]
    fn int_class_coerce_saturate() {
        assert_eq!(IntClass::Int8.coerce(200, OverflowMode::Saturate), 127);
        assert_eq!(IntClass::Int8.coerce(-200, OverflowMode::Saturate), -128);
        assert_eq!(IntClass::Uint8.coerce(-5, OverflowMode::Saturate), 0);
        assert_eq!(IntClass::Uint8.coerce(300, OverflowMode::Saturate), 255);
        assert_eq!(IntClass::Int32.coerce(100, OverflowMode::Saturate), 100);
    }

    #[test]
    fn int_class_coerce_wrap() {
        // int8: 128 wraps to -128, 255 -> -1, 256 -> 0.
        assert_eq!(IntClass::Int8.coerce(128, OverflowMode::Wrap), -128);
        assert_eq!(IntClass::Int8.coerce(255, OverflowMode::Wrap), -1);
        assert_eq!(IntClass::Int8.coerce(256, OverflowMode::Wrap), 0);
        // uint8: 256 -> 0, -1 -> 255.
        assert_eq!(IntClass::Uint8.coerce(256, OverflowMode::Wrap), 0);
        assert_eq!(IntClass::Uint8.coerce(-1, OverflowMode::Wrap), 255);
    }

    #[test]
    fn int_class_from_f64_rounds_half_away() {
        assert_eq!(IntClass::Int8.from_f64(2.5, OverflowMode::Saturate), 3);
        assert_eq!(IntClass::Int8.from_f64(-2.5, OverflowMode::Saturate), -3);
        assert_eq!(IntClass::Int8.from_f64(2.4, OverflowMode::Saturate), 2);
        // Out of range saturates after rounding.
        assert_eq!(IntClass::Int8.from_f64(999.9, OverflowMode::Saturate), 127);
        assert_eq!(IntClass::Uint8.from_f64(f64::NAN, OverflowMode::Saturate), 0);
    }

    #[test]
    fn int_class_smallest_unsigned_for() {
        assert_eq!(IntClass::smallest_unsigned_for(0xFF), Some(IntClass::Uint8));
        assert_eq!(
            IntClass::smallest_unsigned_for(0x1_00),
            Some(IntClass::Uint16)
        );
        assert_eq!(
            IntClass::smallest_unsigned_for(0xFFFF),
            Some(IntClass::Uint16)
        );
        assert_eq!(
            IntClass::smallest_unsigned_for(0x1_0000),
            Some(IntClass::Uint32)
        );
        assert_eq!(
            IntClass::smallest_unsigned_for(0x1_0000_0000),
            Some(IntClass::Uint64)
        );
        assert_eq!(
            IntClass::smallest_unsigned_for(u64::MAX as u128),
            Some(IntClass::Uint64)
        );
        assert_eq!(
            IntClass::smallest_unsigned_for(u64::MAX as u128 + 1),
            None
        );
    }

    // ── SparseMat error paths ───────────────────────────────────────────────

    #[test]
    fn spmv_spmm_dimension_mismatch_errors() {
        // 2x3 matrix.
        let a = SparseMat::new(
            2,
            3,
            vec![
                (0, 0, Complex::new(1.0, 0.0)),
                (1, 2, Complex::new(2.0, 0.0)),
            ],
        );

        // spmv requires a length-3 vector; lengths 2 and 4 must error.
        let x_bad2 = Array1::from_elem(2, Complex::new(1.0, 0.0));
        let x_bad4 = Array1::from_elem(4, Complex::new(1.0, 0.0));
        assert!(a.spmv(&x_bad2).is_err());
        assert!(a.spmv(&x_bad4).is_err());
        // Correct length succeeds and yields a length-rows result.
        let x_ok = Array1::from_elem(3, Complex::new(1.0, 0.0));
        let y = a.spmv(&x_ok).unwrap();
        assert_eq!(y.len(), 2);

        // spmm requires rhs with 3 rows.
        let b_bad = Array2::from_elem((2, 2), Complex::new(1.0, 0.0));
        assert!(a.spmm(&b_bad).is_err());
        let b_ok = Array2::from_elem((3, 2), Complex::new(1.0, 0.0));
        let c = a.spmm(&b_ok).unwrap();
        assert_eq!(c.dim(), (2, 2));
    }
}

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

/// Sparse matrix in COO format.  Entries are sorted row-major (0-based internally).
#[derive(Debug, Clone, PartialEq)]
pub struct SparseMat {
    pub rows: usize,
    pub cols: usize,
    pub entries: Vec<(usize, usize, C64)>,
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
        }
    }

    /// Scale all entries by a complex scalar.
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
        }
    }

    /// Add two sparse matrices (must have equal dimensions).
    pub fn add(&self, other: &SparseMat) -> Result<Self, String> {
        if self.rows != other.rows || self.cols != other.cols {
            return Err(format!(
                "sparse matrix add: dimension mismatch ({}×{} vs {}×{})",
                self.rows, self.cols, other.rows, other.cols
            ));
        }
        let mut combined = self.entries.clone();
        combined.extend_from_slice(&other.entries);
        Ok(Self::new(self.rows, self.cols, combined))
    }

    /// Subtract another sparse matrix.
    pub fn sub(&self, other: &SparseMat) -> Result<Self, String> {
        self.add(&other.scale(Complex::new(-1.0, 0.0)))
    }

    /// Non-conjugate transpose: swap row/col indices.
    pub fn transpose(&self) -> Self {
        let mut entries: Vec<(usize, usize, C64)> =
            self.entries.iter().map(|&(r, c, v)| (c, r, v)).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        Self {
            rows: self.cols,
            cols: self.rows,
            entries,
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
#[derive(Debug, Clone, PartialEq)]
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
}

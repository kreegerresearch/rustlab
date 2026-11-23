//! Incomplete Cholesky factorization (`ichol`), MATLAB's options:
//! zero fill (`"nofill"`, IC(0)) or threshold dropping (`"ict"`), the
//! modified variant (`michol`), and a diagonal shift (`diagcomp`).
//!
//! Left-looking, one column at a time, in the natural order. Column `j`
//! is `A(j:n, j)` minus `L(j:n, k) * conj(L(j, k))` for every earlier
//! column `k` with `L(j, k) ≠ 0`; those columns are found through linked
//! lists keyed by each column's next unprocessed row (Davis, *Direct
//! Methods for Sparse Linear Systems*, §4.8; Saad, *Iterative Methods for
//! Sparse Linear Systems*, §10.3). Only the lower triangle of `A` is read.

use crate::sparse_solve::{SparseCsc, SparseScalar, SparseSolveError};

/// Which entries the factor keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IcholType {
    /// IC(0): `L` keeps the pattern of `tril(A)`.
    NoFill,
    /// ICT: an off-diagonal `L(i, j)` is kept when
    /// `|L(i, j)| ≥ droptol · norm(A(j:n, j), 1)`. `droptol = 0` keeps
    /// everything, which is the complete Cholesky factor.
    Threshold { droptol: f64 },
}

/// `opts` of MATLAB's `ichol(A, opts)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IcholOptions {
    pub kind: IcholType,
    /// Modified incomplete Cholesky: every dropped entry is added to the
    /// two diagonals of its row and column, so `L·L'·e = A·e` (row sums
    /// are kept). Defined for real matrices.
    pub michol: bool,
    /// Factor `A + diagcomp · diag(diag(A))` instead of `A`.
    pub diagcomp: f64,
}

impl Default for IcholOptions {
    fn default() -> Self {
        IcholOptions {
            kind: IcholType::NoFill,
            michol: false,
            diagcomp: 0.0,
        }
    }
}

const NONE: usize = usize::MAX;

/// Incomplete Cholesky factor `L` (lower triangular, real positive
/// diagonal) with `L·L' ≈ A`. Errors with [`SparseSolveError::NonpositivePivot`]
/// at the first pivot that is not real positive (including a missing
/// diagonal), which is MATLAB's "nonpositive pivot".
pub fn ichol<T: SparseScalar>(
    a: &SparseCsc<T>,
    opts: &IcholOptions,
) -> Result<SparseCsc<T>, SparseSolveError> {
    if !a.is_square() {
        return Err(SparseSolveError::NotSquare {
            rows: a.nrows(),
            cols: a.ncols(),
        });
    }
    let n = a.ncols();
    let nofill = opts.kind == IcholType::NoFill;
    let droptol = match opts.kind {
        IcholType::Threshold { droptol } => droptol,
        IcholType::NoFill => 0.0,
    };
    let shift = T::from_f64(1.0 + opts.diagcomp);

    let mut col_ptr = Vec::with_capacity(n + 1);
    col_ptr.push(0usize);
    let lower_nnz = (0..n)
        .map(|j| a.col_iter(j).filter(|&(i, _)| i >= j).count())
        .sum::<usize>();
    let mut row_idx: Vec<usize> = Vec::with_capacity(lower_nnz);
    let mut values: Vec<T> = Vec::with_capacity(lower_nnz);

    let mut w = vec![T::zero(); n];
    let mut stamp = vec![NONE; n];
    let mut in_a = vec![NONE; n];
    let mut pattern: Vec<usize> = Vec::new();
    let mut kept: Vec<usize> = Vec::new();
    let mut done: Vec<usize> = Vec::new();
    // head[r]: first column whose next unprocessed entry is in row r;
    // link[k]: the next column in that list; pos[k]: that entry's index.
    let mut head = vec![NONE; n];
    let mut link = vec![NONE; n];
    let mut pos = vec![0usize; n];
    let mut dcomp = vec![T::zero(); if opts.michol { n } else { 0 }];

    for j in 0..n {
        pattern.clear();
        stamp[j] = j;
        w[j] = T::zero();
        pattern.push(j);
        let mut colnorm = 0.0;
        for (i, v) in a.col_iter(j) {
            if i < j {
                continue;
            }
            colnorm += v.abs();
            if stamp[i] != j {
                stamp[i] = j;
                w[i] = T::zero();
                pattern.push(i);
            }
            w[i] += if i == j { v * shift } else { v };
            in_a[i] = j;
        }
        if opts.michol {
            w[j] += dcomp[j];
        }

        done.clear();
        let mut k = std::mem::replace(&mut head[j], NONE);
        while k != NONE {
            let next = link[k];
            let p0 = pos[k];
            let cljk = values[p0].conj();
            for p in p0..col_ptr[k + 1] {
                let i = row_idx[p];
                let u = values[p] * cljk;
                if nofill && i != j && in_a[i] != j {
                    if opts.michol {
                        w[j] -= u.conj();
                        dcomp[i] -= u;
                    }
                    continue;
                }
                if stamp[i] != j {
                    stamp[i] = j;
                    w[i] = T::zero();
                    pattern.push(i);
                }
                w[i] -= u;
            }
            done.push(k);
            k = next;
        }

        let pivot = |d: T| {
            d.checked_sqrt_real_pos(0.0)
                .filter(|l| l.is_finite())
                .ok_or(SparseSolveError::NonpositivePivot { col: j })
        };
        let mut ljj = pivot(w[j])?;

        kept.clear();
        let thresh = droptol * colnorm * ljj.re();
        let mut dropped = false;
        for &i in &pattern[1..] {
            let v = w[i];
            if !nofill && v.abs() < thresh {
                if opts.michol {
                    w[j] += v.conj();
                    dcomp[i] += v;
                    dropped = true;
                }
                continue;
            }
            if v.abs() != 0.0 {
                kept.push(i);
            }
        }
        if dropped {
            ljj = pivot(w[j])?;
        }
        kept.sort_unstable();

        let start = row_idx.len();
        row_idx.push(j);
        values.push(ljj);
        for &i in &kept {
            row_idx.push(i);
            values.push(w[i] / ljj);
        }
        col_ptr.push(row_idx.len());

        if !kept.is_empty() {
            pos[j] = start + 1;
            let r = row_idx[start + 1];
            link[j] = head[r];
            head[r] = j;
        }
        for &k in &done {
            pos[k] += 1;
            if pos[k] < col_ptr[k + 1] {
                let r = row_idx[pos[k]];
                link[k] = head[r];
                head[r] = k;
            }
        }
    }

    Ok(SparseCsc {
        rows: n,
        cols: n,
        col_ptr,
        row_idx,
        values,
    })
}

//! Supernodal multifrontal Cholesky (dev/plans/sparse_spd_solve.md, Phase 5).
//!
//! The columns of `L` are grouped into supernodes: runs of consecutive
//! columns that share one row structure below the run. Fundamental
//! supernodes are merged with their parent when the merge adds few
//! explicit zeros (CHOLMOD's relaxed amalgamation rules, Davis & Hager).
//! Each supernode is factored as a dense front (Liu, *The multifrontal
//! method for sparse matrix solution*, SIAM Review 1992):
//!
//! 1. assemble the supernode's columns of `A` and the children's update
//!    matrices (extend-add) into a dense panel `P` (`mt × k`, column-major,
//!    rows = the supernode's row structure) and an update block `U`
//!    (`m × m`, `m = mt − k`);
//! 2. dense Cholesky of `P`'s top `k × k` block, a triangular solve for the
//!    `m × k` block below it, and `U −= P₂₁ P₂₁ᴴ`;
//! 3. keep `P` as the stored factor columns and pass `U` to the parent.
//!
//! Independent subtrees run on the rayon pool, as do the large dense
//! products. Every front is computed from its children in a fixed order and
//! every dense block is split by size only, so the factor has the same bits
//! for every thread count.
//!
//! The dense kernels are recursive (blocked) Cholesky, triangular solve and
//! rank-k update, with the products going through `matrixmultiply`
//! ([`SparseScalar::gemm_sub_nh`]). Entries of a merged supernode outside
//! the true structure of their column are exact zeros (every product that
//! reaches them has a zero factor); [`Factor::to_csc`] drops them, so the
//! exported pattern is the simplicial pattern.

use crate::sparse_solve::csc::{SparseCsc, SparseScalar};
use rayon::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};

const NONE: usize = usize::MAX;

/// Dense blocks at most this wide use plain loops.
const NB: usize = 48;

/// Factorizations with fewer flops than this run on the calling thread
/// alone (starting the pool costs more than it saves). Stored as `f64`
/// bits so tests can lower it.
static PAR_MIN_FLOPS: AtomicU64 = AtomicU64::new(1.0e9_f64.to_bits());

/// Set the flop count from which a factorization may use the rayon pool.
/// For tests: the result is the same either way.
pub fn set_parallel_min_flops(flops: f64) {
    PAR_MIN_FLOPS.store(flops.to_bits(), Ordering::Relaxed);
}

/// Subtrees with more flops than this run their children in parallel.
const PAR_SUBTREE_FLOPS: f64 = 4.0e6;

/// A subtree whose update stack needs at most this many entries is one
/// task, factored on whichever thread picks it up. Larger subtrees (the top
/// of the tree) are factored on the calling thread, which gets its threads
/// from the dense kernels: memory freed on a worker thread stays in that
/// thread's allocator arena and is not reused by the large allocations
/// that follow. Parallel tasks get their stacks from one buffer the calling
/// thread allocates ([`parallel_subtrees`]).
const PAR_MAX_STACK: usize = 1 << 20;

/// Dense products with more multiply-adds than this are split in two
/// halves that run in parallel.
const PAR_GEMM_MACS: usize = 1 << 22;

/// Symbolic structure: the supernode partition, each supernode's rows, and
/// the supernodal elimination tree.
#[derive(Debug)]
pub(crate) struct Symbolic {
    /// Supernode `s` holds columns `sup_ptr[s] .. sup_ptr[s + 1]`.
    sup_ptr: Vec<usize>,
    /// Rows of `s`: `rows[row_ptr[s] .. row_ptr[s + 1]]`, its own columns
    /// first, then the rows below, ascending.
    row_ptr: Vec<usize>,
    rows: Vec<usize>,
    /// Children of `s`: `children[child_ptr[s] .. child_ptr[s + 1]]`,
    /// ascending.
    child_ptr: Vec<usize>,
    children: Vec<usize>,
    /// Supernodes without a parent.
    roots: Vec<usize>,
    /// Dense flops of `s` and all its descendants.
    subtree_flops: Vec<f64>,
    /// Supernode `s`'s panel starts at `val_ptr[s]`.
    val_ptr: Vec<usize>,
    /// Entries of the update stack that factoring the subtree at `s`
    /// needs, children in order.
    stack_need: Vec<usize>,
}

/// Column width of the blocks an update matrix is stored in.
const UB: usize = 64;

/// An `m × m` update matrix is kept as its lower triangle in column blocks
/// of width [`UB`]: block `b` holds rows `b·UB .. m` of its columns with
/// leading dimension `m − b·UB`, so only the diagonal `UB × UB` blocks
/// carry scratch above the diagonal. Offset of block `b`.
fn ub_block_off(m: usize, b: usize) -> usize {
    UB * (b * m - UB * b * b.saturating_sub(1) / 2)
}

/// Entries of an `m × m` update matrix in block form.
fn ub_len(m: usize) -> usize {
    if m == 0 {
        return 0;
    }
    let last = (m - 1) / UB;
    ub_block_off(m, last) + (m - last * UB) * (m - last * UB)
}

/// Offset of entry `(j, j)`; column `j`'s rows `j .. m` follow it.
fn ub_diag(m: usize, j: usize) -> usize {
    let b = j / UB;
    let j0 = b * UB;
    ub_block_off(m, b) + (j - j0) * (m - j0 + 1)
}

impl Symbolic {
    pub(crate) fn nsuper(&self) -> usize {
        self.sup_ptr.len() - 1
    }

    fn cols(&self, s: usize) -> (usize, usize) {
        (self.sup_ptr[s], self.sup_ptr[s + 1] - self.sup_ptr[s])
    }

    fn rows(&self, s: usize) -> &[usize] {
        &self.rows[self.row_ptr[s]..self.row_ptr[s + 1]]
    }

    fn below(&self, s: usize) -> &[usize] {
        let k = self.sup_ptr[s + 1] - self.sup_ptr[s];
        &self.rows[self.row_ptr[s] + k..self.row_ptr[s + 1]]
    }

    fn children(&self, s: usize) -> &[usize] {
        &self.children[self.child_ptr[s]..self.child_ptr[s + 1]]
    }

    /// Dense flops of the whole factorization.
    pub(crate) fn flops(&self) -> f64 {
        self.roots.iter().map(|&r| self.subtree_flops[r]).sum()
    }
}

/// `A`'s lower triangle as CSC, read from the upper triangle of the
/// Hermitian `c` (conjugated), so the factor reads the same entries as the
/// up-looking kernel. Rows ascending; the diagonal, when stored, first.
pub(crate) fn lower_from_upper<T: SparseScalar>(c: &SparseCsc<T>) -> SparseCsc<T> {
    let n = c.ncols();
    let mut count = vec![0usize; n + 1];
    for i in 0..n {
        for (r, _) in c.col_iter(i) {
            if r <= i {
                count[r + 1] += 1;
            }
        }
    }
    for j in 0..n {
        count[j + 1] += count[j];
    }
    let nnz = count[n];
    let mut next = count.clone();
    let mut row_idx = vec![0usize; nnz];
    let mut values = vec![T::zero(); nnz];
    for i in 0..n {
        for (r, v) in c.col_iter(i) {
            if r <= i {
                let slot = next[r];
                row_idx[slot] = i;
                values[slot] = v.conj();
                next[r] = slot + 1;
            }
        }
    }
    SparseCsc {
        rows: n,
        cols: n,
        col_ptr: count,
        row_idx,
        values,
    }
}

/// CHOLMOD's relaxed amalgamation rule for a merged supernode of `ncols`
/// columns whose fraction `zeros` of stored entries are explicit zeros.
fn merge_ok(ncols: usize, zeros: f64) -> bool {
    ncols <= 4 || (ncols <= 16 && zeros < 0.8) || (ncols <= 48 && zeros < 0.1) || zeros < 0.05
}

/// Stored entries of a `k`-column trapezoid with `m` rows below it.
fn trapezoid(k: usize, m: usize) -> f64 {
    let k = k as f64;
    k * (k + 1.0) / 2.0 + k * m as f64
}

/// Supernode partition (`sup_ptr`) from the elimination tree and the column
/// counts of `L` (diagonal included).
fn partition(parent: &[Option<usize>], cc: &[usize]) -> Vec<usize> {
    let n = cc.len();
    if n == 0 {
        return vec![0];
    }
    let mut nchild = vec![0u32; n];
    for p in parent.iter().flatten() {
        nchild[*p] += 1;
    }
    // Fundamental supernodes: j continues j − 1's run when j − 1's only
    // parent is j, j has no other child, and the structures nest exactly.
    let mut fs = vec![0usize];
    for j in 1..n {
        let chain = parent[j - 1] == Some(j) && cc[j - 1] == cc[j] + 1 && nchild[j] == 1;
        if !chain {
            fs.push(j);
        }
    }
    fs.push(n);
    let nf = fs.len() - 1;
    let mut col_to_f = vec![0usize; n];
    for f in 0..nf {
        for c in col_to_f.iter_mut().take(fs[f + 1]).skip(fs[f]) {
            *c = f;
        }
    }
    let fparent: Vec<usize> = (0..nf)
        .map(|f| parent[fs[f + 1] - 1].map_or(NONE, |p| col_to_f[p]))
        .collect();
    let ncols: Vec<usize> = (0..nf).map(|f| fs[f + 1] - fs[f]).collect();
    let below: Vec<usize> = (0..nf).map(|f| cc[fs[f]] - ncols[f]).collect();

    // Relaxed amalgamation, top down: f joins the group holding f + 1 when
    // f + 1 is its parent (so the columns stay contiguous) and the merged
    // group passes `merge_ok`. A group's rows are its columns plus the
    // rows below its top member.
    let mut leader: Vec<usize> = (0..nf).collect();
    let mut gcols = ncols.clone();
    let mut gzeros = vec![0.0f64; nf];
    for f in (0..nf.saturating_sub(1)).rev() {
        if fparent[f] != f + 1 {
            continue;
        }
        let g = leader[f + 1];
        let k = gcols[g] + ncols[f];
        let total = trapezoid(k, below[g]);
        let added = total - trapezoid(gcols[g], below[g]) - trapezoid(ncols[f], below[f]);
        let zeros = gzeros[g] + added;
        if merge_ok(k, zeros / total) {
            leader[f] = g;
            gcols[g] = k;
            gzeros[g] = zeros;
        }
    }
    let mut sup_ptr = vec![0usize];
    for f in 1..nf {
        if leader[f] != leader[f - 1] {
            sup_ptr.push(fs[f]);
        }
    }
    sup_ptr.push(n);
    sup_ptr
}

/// Symbolic analysis of the permuted matrix whose lower triangle is
/// `lower`, with elimination tree `parent` and column counts `cc`.
pub(crate) fn analyze<T: SparseScalar>(
    lower: &SparseCsc<T>,
    parent: &[Option<usize>],
    cc: &[usize],
) -> Symbolic {
    let n = lower.ncols();
    let sup_ptr = partition(parent, cc);
    let ns = sup_ptr.len() - 1;
    let mut col_to_s = vec![0usize; n];
    for s in 0..ns {
        for c in col_to_s.iter_mut().take(sup_ptr[s + 1]).skip(sup_ptr[s]) {
            *c = s;
        }
    }
    let sparent: Vec<usize> = (0..ns)
        .map(|s| parent[sup_ptr[s + 1] - 1].map_or(NONE, |p| col_to_s[p]))
        .collect();
    let mut child_ptr = vec![0usize; ns + 1];
    for &p in &sparent {
        if p != NONE {
            child_ptr[p + 1] += 1;
        }
    }
    for s in 0..ns {
        child_ptr[s + 1] += child_ptr[s];
    }
    let mut fill = child_ptr.clone();
    let mut children = vec![0usize; child_ptr[ns]];
    let mut roots = Vec::new();
    for (s, &p) in sparent.iter().enumerate() {
        if p == NONE {
            roots.push(s);
        } else {
            children[fill[p]] = s;
            fill[p] += 1;
        }
    }

    // Rows of each supernode: its columns, the rows of A below them, and
    // the rows below each child (children come first: sparent[s] > s).
    let mut row_ptr = vec![0usize; ns + 1];
    let mut rows: Vec<usize> = Vec::new();
    let mut mark = vec![NONE; n];
    let mut subtree_flops = vec![0.0f64; ns];
    let mut val_ptr = vec![0usize; ns + 1];
    let mut stack_need = vec![0usize; ns];
    for s in 0..ns {
        let (f, k) = (sup_ptr[s], sup_ptr[s + 1] - sup_ptr[s]);
        let l = f + k - 1;
        let start = rows.len();
        rows.extend(f..=l);
        mark[f..=l].fill(s);
        for j in f..=l {
            for (i, _) in lower.col_iter(j) {
                if i > l && mark[i] != s {
                    mark[i] = s;
                    rows.push(i);
                }
            }
        }
        for &c in &children[child_ptr[s]..child_ptr[s + 1]] {
            let (cs, ce) = (row_ptr[c], row_ptr[c + 1]);
            let kc = sup_ptr[c + 1] - sup_ptr[c];
            for q in cs + kc..ce {
                let i = rows[q];
                if i > l && mark[i] != s {
                    mark[i] = s;
                    rows.push(i);
                }
            }
        }
        rows[start + k..].sort_unstable();
        row_ptr[s + 1] = rows.len();
        val_ptr[s + 1] = val_ptr[s] + (rows.len() - start) * k;
        let mut held = 0usize;
        let mut need = 0usize;
        for &c in &children[child_ptr[s]..child_ptr[s + 1]] {
            need = need.max(held + stack_need[c]);
            held += ub_len(row_ptr[c + 1] - row_ptr[c] - (sup_ptr[c + 1] - sup_ptr[c]));
        }
        stack_need[s] = need.max(held + ub_len(rows.len() - start - k));
        let (kf, mf) = (k as f64, (rows.len() - start - k) as f64);
        subtree_flops[s] += kf * kf * kf / 3.0 + mf * kf * kf + mf * mf * kf;
        if sparent[s] != NONE {
            let add = subtree_flops[s];
            subtree_flops[sparent[s]] += add;
        }
    }
    Symbolic {
        sup_ptr,
        row_ptr,
        rows,
        child_ptr,
        children,
        roots,
        subtree_flops,
        val_ptr,
        stack_need,
    }
}

/// Numeric supernodal factor: each supernode's columns as a dense
/// column-major `mt × k` panel (rows = the supernode's rows; entries above
/// the diagonal are scratch), all in one buffer.
#[derive(Debug)]
pub(crate) struct Factor<T> {
    sym: Symbolic,
    values: Vec<T>,
}

impl<T: SparseScalar> Factor<T> {
    /// Factor the matrix whose lower triangle is `lower`. `Err(col)` is the
    /// first column (permuted index) whose pivot is not real positive.
    pub(crate) fn compute(sym: Symbolic, lower: &SparseCsc<T>, tol: f64) -> Result<Self, usize> {
        let ns = sym.nsuper();
        let mut values = vec![T::zero(); sym.val_ptr[ns]];
        let ctx = Ctx {
            sym: &sym,
            lower,
            tol,
            out: Ptr(values.as_mut_ptr()),
            parallel: sym.flops() >= f64::from_bits(PAR_MIN_FLOPS.load(Ordering::Relaxed))
                && rayon::current_num_threads() > 1,
        };
        let small = |r: usize| sym.stack_need[r] <= PAR_MAX_STACK;
        if ctx.parallel && sym.roots.len() > 1 && sym.roots.iter().all(|&r| small(r)) {
            parallel_subtrees(&ctx, &sym.roots)?;
        } else {
            let outs: Vec<Result<Vec<T>, usize>> =
                sym.roots.iter().map(|&r| subtree(&ctx, r)).collect();
            if let Some(col) = first_failure(&outs) {
                return Err(col);
            }
        }
        Ok(Factor { sym, values })
    }

    /// Solve `L Lᴴ y = b` in place (permuted space).
    pub(crate) fn solve_in_place(&self, y: &mut [T]) {
        let sym = &self.sym;
        for s in 0..sym.nsuper() {
            let (f, k) = sym.cols(s);
            let rows = sym.rows(s);
            let mt = rows.len();
            let base = sym.val_ptr[s];
            for jj in 0..k {
                let col = &self.values[base + jj * mt + jj..base + (jj + 1) * mt];
                let yj = y[f + jj] / col[0];
                y[f + jj] = yj;
                for (v, &r) in col[1..].iter().zip(&rows[jj + 1..]) {
                    y[r] -= *v * yj;
                }
            }
        }
        for s in (0..sym.nsuper()).rev() {
            let (f, k) = sym.cols(s);
            let rows = sym.rows(s);
            let mt = rows.len();
            let base = sym.val_ptr[s];
            for jj in (0..k).rev() {
                let col = &self.values[base + jj * mt + jj..base + (jj + 1) * mt];
                let mut sum = y[f + jj];
                for (v, &r) in col[1..].iter().zip(&rows[jj + 1..]) {
                    sum -= v.conj() * y[r];
                }
                y[f + jj] = sum / col[0];
            }
        }
    }

    /// `L` as CSC, diagonal first in each column, explicit zeros dropped.
    pub(crate) fn to_csc(&self) -> SparseCsc<T> {
        let sym = &self.sym;
        let n = *sym.sup_ptr.last().unwrap_or(&0);
        // Count first: the panels are still held, so the CSC arrays must not
        // grow by doubling.
        let mut nnz = 0usize;
        for s in 0..sym.nsuper() {
            let (_, k) = sym.cols(s);
            let mt = sym.rows(s).len();
            let base = sym.val_ptr[s];
            for jj in 0..k {
                let col = &self.values[base + jj * mt + jj..base + (jj + 1) * mt];
                nnz += 1 + col[1..].iter().filter(|v| v.abs() != 0.0).count();
            }
        }
        let mut col_ptr = Vec::with_capacity(n + 1);
        col_ptr.push(0);
        let mut row_idx = Vec::with_capacity(nnz);
        let mut values = Vec::with_capacity(nnz);
        for s in 0..sym.nsuper() {
            let (_, k) = sym.cols(s);
            let rows = sym.rows(s);
            let mt = rows.len();
            let base = sym.val_ptr[s];
            for jj in 0..k {
                let col = &self.values[base + jj * mt + jj..base + (jj + 1) * mt];
                row_idx.push(rows[jj]);
                values.push(col[0]);
                for (v, &r) in col[1..].iter().zip(&rows[jj + 1..]) {
                    if v.abs() != 0.0 {
                        row_idx.push(r);
                        values.push(*v);
                    }
                }
                col_ptr.push(row_idx.len());
            }
        }
        SparseCsc {
            rows: n,
            cols: n,
            col_ptr,
            row_idx,
            values,
        }
    }
}

struct Ctx<'a, T: SparseScalar> {
    sym: &'a Symbolic,
    lower: &'a SparseCsc<T>,
    tol: f64,
    /// `values` of the factor being built; each front writes only its
    /// own `val_ptr` range.
    out: Ptr<T>,
    /// Subtrees and dense blocks may use the rayon pool. The split points
    /// do not depend on this or on the pool size, so neither do the bits.
    parallel: bool,
}

fn first_failure<T>(outs: &[Result<Vec<T>, usize>]) -> Option<usize> {
    outs.iter().filter_map(|o| o.as_ref().err().copied()).min()
}

/// Factor the subtree rooted at `s`; returns `s`'s update matrix.
fn subtree<T: SparseScalar>(ctx: &Ctx<'_, T>, s: usize) -> Result<Vec<T>, usize> {
    let sym = ctx.sym;
    let big = |v: usize| {
        ctx.parallel
            && sym.subtree_flops[v] > PAR_SUBTREE_FLOPS
            && sym.stack_need[v] > PAR_MAX_STACK
    };
    if !big(s) {
        return sequential_subtree(ctx, s);
    }
    // A chain of one-child supernodes is walked, not recursed.
    let mut chain = vec![s];
    let mut v = s;
    while sym.children(v).len() == 1 && big(sym.children(v)[0]) {
        v = sym.children(v)[0];
        chain.push(v);
    }
    let kids = sym.children(v);
    let mut u = if kids.len() > 1 && kids.iter().all(|&c| !big(c)) {
        let (buf, lens) = parallel_subtrees(ctx, kids)?;
        let mut updates: Vec<&[T]> = Vec::with_capacity(kids.len());
        let mut off = 0;
        for (&c, &len) in kids.iter().zip(&lens) {
            updates.push(&buf[off..off + len]);
            off += sym.stack_need[c];
        }
        let mut u = vec![T::zero(); ub_len(sym.below(v).len())];
        front_into(ctx, v, &updates, &mut u)?;
        u
    } else {
        let outs: Vec<Result<Vec<T>, usize>> = kids.iter().map(|&c| subtree(ctx, c)).collect();
        if let Some(col) = first_failure(&outs) {
            return Err(col);
        }
        let updates = outs.into_iter().map(|o| o.unwrap_or_default()).collect();
        front(ctx, v, updates)?
    };
    for &w in chain.iter().rev().skip(1) {
        u = front(ctx, w, vec![u])?;
    }
    Ok(u)
}

/// The subtree at `s` without recursion (an elimination tree can be a long
/// chain), in increasing supernode order, children before parents.
///
/// When that order is a postorder (always, unless the ordering is fixed),
/// the pending update matrices form a stack: a node's children are the
/// top entries. They then live in one buffer sized by a dry run, and a
/// node's update is written above its children's and moved down over them
/// (Liu's multifrontal stack). One allocation instead of one per front
/// keeps freed update matrices from piling up in the allocator.
fn sequential_subtree<T: SparseScalar>(ctx: &Ctx<'_, T>, s: usize) -> Result<Vec<T>, usize> {
    let nodes = subtree_nodes(ctx.sym, s);
    match stack_capacity(ctx.sym, &nodes) {
        Some(cap) => {
            let mut stack = vec![T::zero(); cap];
            let len = stacked_subtree(ctx, &nodes, &mut stack)?;
            stack.truncate(len);
            Ok(stack)
        }
        None => unstacked_subtree(ctx, &nodes),
    }
}

/// The subtrees at `roots` in parallel, each in its own slice (of
/// `stack_need` entries, in order) of one buffer allocated here, so the
/// worker threads allocate nothing large. Returns the buffer and the
/// length of each root's update matrix, which starts its slice.
fn parallel_subtrees<T: SparseScalar>(
    ctx: &Ctx<'_, T>,
    roots: &[usize],
) -> Result<(Vec<T>, Vec<usize>), usize> {
    let sym = ctx.sym;
    let total: usize = roots.iter().map(|&r| sym.stack_need[r]).sum();
    let mut buf = vec![T::zero(); total];
    let mut slices: Vec<(usize, &mut [T])> = Vec::with_capacity(roots.len());
    let mut rest: &mut [T] = &mut buf;
    for &r in roots {
        let (mine, tail) = std::mem::take(&mut rest).split_at_mut(sym.stack_need[r]);
        slices.push((r, mine));
        rest = tail;
    }
    let outs: Vec<Result<usize, usize>> = slices
        .into_par_iter()
        .map(|(r, stack)| {
            let nodes = subtree_nodes(sym, r);
            if stack_capacity(sym, &nodes).is_some() {
                stacked_subtree(ctx, &nodes, stack)
            } else {
                let u = unstacked_subtree(ctx, &nodes)?;
                stack[..u.len()].copy_from_slice(&u);
                Ok(u.len())
            }
        })
        .collect();
    let mut lens = Vec::with_capacity(outs.len());
    let mut fail = NONE;
    for o in outs {
        match o {
            Ok(len) => lens.push(len),
            Err(col) => fail = fail.min(col),
        }
    }
    if fail != NONE {
        return Err(fail);
    }
    Ok((buf, lens))
}

/// Supernodes of the subtree at `s`, ascending.
fn subtree_nodes(sym: &Symbolic, s: usize) -> Vec<usize> {
    let mut nodes = vec![s];
    let mut q = 0;
    while q < nodes.len() {
        let v = nodes[q];
        nodes.extend_from_slice(sym.children(v));
        q += 1;
    }
    nodes.sort_unstable();
    nodes
}

/// Entries the update stack needs for `nodes` (ascending), or `None` when
/// the order is not a postorder.
fn stack_capacity(sym: &Symbolic, nodes: &[usize]) -> Option<usize> {
    let mut ids: Vec<usize> = Vec::new();
    let mut offs: Vec<usize> = Vec::new();
    let (mut top, mut cap) = (0usize, 0usize);
    for &v in nodes {
        let kids = sym.children(v);
        let nk = kids.len();
        if ids.len() < nk || ids[ids.len() - nk..] != *kids {
            return None;
        }
        let base = if nk > 0 { offs[offs.len() - nk] } else { top };
        let len = ub_len(sym.below(v).len());
        cap = cap.max(top + len);
        ids.truncate(ids.len() - nk);
        offs.truncate(offs.len() - nk);
        ids.push(v);
        offs.push(base);
        top = base + len;
    }
    Some(cap)
}

/// Factor the postordered subtree `nodes` with its update stack in
/// `stack` (at least `stack_need` entries); the root's update ends up at
/// `stack[..len]` and `len` is returned.
fn stacked_subtree<T: SparseScalar>(
    ctx: &Ctx<'_, T>,
    nodes: &[usize],
    stack: &mut [T],
) -> Result<usize, usize> {
    let sym = ctx.sym;
    // (offset, length, failed column) of each pending update.
    let mut entries: Vec<(usize, usize, usize)> = Vec::new();
    let mut top = 0usize;
    for &v in nodes {
        let nk = sym.children(v).len();
        let kids = &entries[entries.len() - nk..];
        let base = kids.first().map_or(top, |e| e.0);
        let fail = kids.iter().map(|e| e.2).min().unwrap_or(NONE);
        let len = ub_len(sym.below(v).len());
        let mut failed = fail;
        if fail == NONE {
            let (below, above) = stack.split_at_mut(top);
            let updates: Vec<&[T]> = kids.iter().map(|e| &below[e.0..e.0 + e.1]).collect();
            let u = &mut above[..len];
            u.fill(T::zero());
            if let Err(col) = front_into(ctx, v, &updates, u) {
                failed = col;
            }
        }
        entries.truncate(entries.len() - nk);
        if failed == NONE {
            stack.copy_within(top..top + len, base);
            entries.push((base, len, NONE));
            top = base + len;
        } else {
            entries.push((base, 0, failed));
            top = base;
        }
    }
    let (off, len, failed) = entries.pop().expect("the root is the last node");
    if failed != NONE {
        return Err(failed);
    }
    debug_assert_eq!(off, 0);
    Ok(len)
}

/// [`sequential_subtree`] with one allocation per update matrix.
fn unstacked_subtree<T: SparseScalar>(ctx: &Ctx<'_, T>, nodes: &[usize]) -> Result<Vec<T>, usize> {
    let sym = ctx.sym;
    // Pending update (or failed column) of each processed node, by
    // position in `nodes`.
    let mut pending: Vec<Option<Result<Vec<T>, usize>>> = (0..nodes.len()).map(|_| None).collect();
    let pos = |v: usize| nodes.binary_search(&v).expect("child is in the subtree");
    for (idx, &v) in nodes.iter().enumerate() {
        let mut updates = Vec::with_capacity(sym.children(v).len());
        let mut fail = NONE;
        for &c in sym.children(v) {
            match pending[pos(c)].take().expect("children come first") {
                Ok(u) => updates.push(u),
                Err(col) => fail = fail.min(col),
            }
        }
        pending[idx] = Some(if fail != NONE {
            Err(fail)
        } else {
            front(ctx, v, updates)
        });
    }
    pending
        .pop()
        .flatten()
        .expect("the root is the largest node")
}

/// [`front_into`] with a newly allocated update matrix.
fn front<T: SparseScalar>(
    ctx: &Ctx<'_, T>,
    s: usize,
    updates: Vec<Vec<T>>,
) -> Result<Vec<T>, usize> {
    let mut u = vec![T::zero(); ub_len(ctx.sym.below(s).len())];
    let views: Vec<&[T]> = updates.iter().map(Vec::as_slice).collect();
    front_into(ctx, s, &views, &mut u)?;
    Ok(u)
}

/// Assemble and factor the front of supernode `s` from its children's
/// update matrices (in child order) into its panel, and write its own
/// update matrix into `u` (zeroed, [`ub_len`] entries).
fn front_into<T: SparseScalar>(
    ctx: &Ctx<'_, T>,
    s: usize,
    updates: &[&[T]],
    u: &mut [T],
) -> Result<(), usize> {
    let sym = ctx.sym;
    let (f, k) = sym.cols(s);
    let rows = sym.rows(s);
    let mt = rows.len();
    let m = mt - k;
    // SAFETY: `val_ptr[s] .. val_ptr[s + 1]` (mt · k entries, zeroed) is
    // this front's alone.
    let p: &mut [T] =
        unsafe { std::slice::from_raw_parts_mut(ctx.out.0.add(sym.val_ptr[s]), mt * k) };
    for jj in 0..k {
        let mut pos = jj;
        let col = &mut p[jj * mt..(jj + 1) * mt];
        for (i, v) in ctx.lower.col_iter(f + jj) {
            while rows[pos] < i {
                pos += 1;
            }
            debug_assert_eq!(rows[pos], i);
            col[pos] += v;
        }
    }
    let mut rel: Vec<usize> = Vec::new();
    for (&c, uc) in sym.children(s).iter().zip(updates) {
        let crow = sym.below(c);
        let mc = crow.len();
        rel.clear();
        let mut pos = 0;
        for &i in crow {
            while rows[pos] < i {
                pos += 1;
            }
            debug_assert_eq!(rows[pos], i);
            rel.push(pos);
        }
        for jj in 0..mc {
            let tc = rel[jj];
            let src = &uc[ub_diag(mc, jj)..][..mc - jj];
            if tc < k {
                let dst = &mut p[tc * mt..(tc + 1) * mt];
                for (v, &r) in src.iter().zip(&rel[jj..]) {
                    dst[r] += *v;
                }
            } else {
                let dst = &mut u[ub_diag(m, tc - k)..][..mt - tc];
                for (v, &r) in src.iter().zip(&rel[jj..]) {
                    dst[r - tc] += *v;
                }
            }
        }
    }
    // SAFETY: every block below lies inside `p` (mt × k, leading dimension
    // mt) or `u` (block form), and each call's output block is disjoint
    // from the blocks it reads.
    let par = ctx.parallel;
    unsafe {
        let pp = p.as_mut_ptr();
        potrf(par, pp, mt, k, ctx.tol).map_err(|c| f + c)?;
        if m > 0 {
            trsm_rlh(par, pp.add(k), mt, m, pp, mt, k);
            update_blocks(par, u.as_mut_ptr(), m, pp.add(k), mt, k);
        }
    }
    Ok(())
}

/// `rayon::join`, or the two calls in order when `par` is off.
fn join<A: FnOnce() + Send, B: FnOnce() + Send>(par: bool, a: A, b: B) {
    if par {
        rayon::join(a, b);
    } else {
        a();
        b();
    }
}

/// A pointer that may cross to a rayon task. The callers guarantee the
/// blocks the tasks touch are disjoint.
#[derive(Clone, Copy)]
struct Ptr<T>(*mut T);
unsafe impl<T> Send for Ptr<T> {}
unsafe impl<T> Sync for Ptr<T> {}

/// `C −= A Bᴴ`, split in halves on the rayon pool when large. The split
/// depends only on the sizes.
#[allow(clippy::too_many_arguments)]
unsafe fn gemm<T: SparseScalar>(
    par: bool,
    m: usize,
    n: usize,
    k: usize,
    a: *const T,
    lda: usize,
    b: *const T,
    ldb: usize,
    c: *mut T,
    ldc: usize,
) {
    if m.saturating_mul(n).saturating_mul(k) <= PAR_GEMM_MACS || (m < 2 * NB && n < 2 * NB) {
        return T::gemm_sub_nh(m, n, k, a, lda, b, ldb, c, ldc);
    }
    let (a, b, c) = (Ptr(a as *mut T), Ptr(b as *mut T), Ptr(c));
    if m >= n {
        let m1 = m / 2;
        join(
            par,
            move || {
                let (a, b, c) = (a, b, c);
                gemm(par, m1, n, k, a.0, lda, b.0, ldb, c.0, ldc)
            },
            move || {
                let (a, b, c) = (a, b, c);
                gemm(
                    par,
                    m - m1,
                    n,
                    k,
                    a.0.add(m1),
                    lda,
                    b.0,
                    ldb,
                    c.0.add(m1),
                    ldc,
                )
            },
        );
    } else {
        let n1 = n / 2;
        join(
            par,
            move || {
                let (a, b, c) = (a, b, c);
                gemm(par, m, n1, k, a.0, lda, b.0, ldb, c.0, ldc)
            },
            move || {
                let (a, b, c) = (a, b, c);
                gemm(
                    par,
                    m,
                    n - n1,
                    k,
                    a.0,
                    lda,
                    b.0.add(n1),
                    ldb,
                    c.0.add(n1 * ldc),
                    ldc,
                )
            },
        );
    }
}

/// In-place lower Cholesky of the `k × k` block at `a`. `Err(j)`: the
/// pivot of local column `j` is not real positive above `tol`.
unsafe fn potrf<T: SparseScalar>(
    par: bool,
    a: *mut T,
    lda: usize,
    k: usize,
    tol: f64,
) -> Result<(), usize> {
    if k <= NB {
        for j in 0..k {
            let ajj = a.add(j + j * lda);
            let ljj = (*ajj).checked_sqrt_real_pos(tol).ok_or(j)?;
            *ajj = ljj;
            let cj = a.add(j * lda);
            for i in j + 1..k {
                *cj.add(i) = *cj.add(i) / ljj;
            }
            for jj in j + 1..k {
                let fct = (*cj.add(jj)).conj();
                let cjj = a.add(jj * lda);
                for i in jj..k {
                    *cjj.add(i) -= *cj.add(i) * fct;
                }
            }
        }
        return Ok(());
    }
    let k1 = k / 2;
    potrf(par, a, lda, k1, tol)?;
    trsm_rlh(par, a.add(k1), lda, k - k1, a, lda, k1);
    syrk_sub(par, a.add(k1 + k1 * lda), lda, k - k1, a.add(k1), lda, k1);
    potrf(par, a.add(k1 + k1 * lda), lda, k - k1, tol).map_err(|j| j + k1)
}

/// Solve `X Lᴴ = B` in place: `B` (`m × k` at `b`) becomes `X`, `L` is
/// the `k × k` lower block at `l`. Rows are independent, so large `m` is
/// split across the pool.
unsafe fn trsm_rlh<T: SparseScalar>(
    par: bool,
    b: *mut T,
    ldb: usize,
    m: usize,
    l: *const T,
    ldl: usize,
    k: usize,
) {
    if m > 4 * NB && m.saturating_mul(k).saturating_mul(k) > PAR_GEMM_MACS {
        let m1 = m / 2;
        let (bp, lp) = (Ptr(b), Ptr(l as *mut T));
        join(
            par,
            move || {
                let (bp, lp) = (bp, lp);
                trsm_rlh(par, bp.0, ldb, m1, lp.0, ldl, k)
            },
            move || {
                let (bp, lp) = (bp, lp);
                trsm_rlh(par, bp.0.add(m1), ldb, m - m1, lp.0, ldl, k)
            },
        );
        return;
    }
    if k <= NB {
        for j in 0..k {
            let ljj = *l.add(j + j * ldl);
            let bj = b.add(j * ldb);
            for i in 0..m {
                *bj.add(i) = *bj.add(i) / ljj;
            }
            for jj in j + 1..k {
                let fct = (*l.add(jj + j * ldl)).conj();
                let bjj = b.add(jj * ldb);
                for i in 0..m {
                    *bjj.add(i) -= *bj.add(i) * fct;
                }
            }
        }
        return;
    }
    let k1 = k / 2;
    trsm_rlh(par, b, ldb, m, l, ldl, k1);
    gemm(
        par,
        m,
        k - k1,
        k1,
        b,
        ldb,
        l.add(k1),
        ldl,
        b.add(k1 * ldb),
        ldb,
    );
    trsm_rlh(
        par,
        b.add(k1 * ldb),
        ldb,
        m,
        l.add(k1 + k1 * ldl),
        ldl,
        k - k1,
    );
}

/// `U −= A Aᴴ` for an `m × m` update `U` in block form ([`ub_len`]); `A`
/// is `m × k`. One product per column block; blocks run in parallel when
/// the update is large.
unsafe fn update_blocks<T: SparseScalar>(
    par: bool,
    u: *mut T,
    m: usize,
    a: *const T,
    lda: usize,
    k: usize,
) {
    let nb = m.div_ceil(UB);
    let (up, ap) = (Ptr(u), Ptr(a as *mut T));
    let block = move |b: usize| {
        let (up, ap) = (up, ap);
        let j0 = b * UB;
        let w = UB.min(m - j0);
        let ld = m - j0;
        gemm(
            par,
            ld,
            w,
            k,
            ap.0.add(j0),
            lda,
            ap.0.add(j0),
            lda,
            up.0.add(ub_block_off(m, b)),
            ld,
        );
    };
    if nb > 1 && m.saturating_mul(m).saturating_mul(k) > 2 * PAR_GEMM_MACS {
        if par {
            (0..nb).into_par_iter().for_each(block);
        } else {
            (0..nb).for_each(block);
        }
    } else {
        (0..nb).for_each(block);
    }
}

/// `C −= A Aᴴ` on the lower triangle of the `n × n` block at `c`; `A` is
/// `n × k`. Diagonal blocks of at most `NB` are formed whole, so the
/// entries above their diagonal are scratch and never read.
unsafe fn syrk_sub<T: SparseScalar>(
    par: bool,
    c: *mut T,
    ldc: usize,
    n: usize,
    a: *const T,
    lda: usize,
    k: usize,
) {
    if n <= NB {
        return T::gemm_sub_nh(n, n, k, a, lda, a, lda, c, ldc);
    }
    let n1 = n / 2;
    let (cp, ap) = (Ptr(c), Ptr(a as *mut T));
    if n.saturating_mul(n).saturating_mul(k) > 2 * PAR_GEMM_MACS {
        join(
            par,
            move || {
                let (cp, ap) = (cp, ap);
                syrk_sub(par, cp.0, ldc, n1, ap.0, lda, k);
                syrk_sub(
                    par,
                    cp.0.add(n1 + n1 * ldc),
                    ldc,
                    n - n1,
                    ap.0.add(n1),
                    lda,
                    k,
                );
            },
            move || {
                let (cp, ap) = (cp, ap);
                gemm(
                    par,
                    n - n1,
                    n1,
                    k,
                    ap.0.add(n1),
                    lda,
                    ap.0,
                    lda,
                    cp.0.add(n1),
                    ldc,
                );
            },
        );
    } else {
        syrk_sub(par, c, ldc, n1, a, lda, k);
        gemm(par, n - n1, n1, k, a.add(n1), lda, a, lda, c.add(n1), ldc);
        syrk_sub(par, c.add(n1 + n1 * ldc), ldc, n - n1, a.add(n1), lda, k);
    }
}

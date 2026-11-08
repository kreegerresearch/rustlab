//! Materials and PEC for the Yee stepper (Phase 2 of `dev/plans/fdtd3d.md`).
//!
//! `eps_r`, `mu_r` and `sigma` are cell-centred, stored x-fastest like the
//! fields. The bake turns them into per-edge coefficients relative to the
//! vacuum curl coefficients:
//!
//! ```text
//! s  = σ dt / (2 ε0 eps_r)
//! Ca = (1 − s) / (1 + s)            (only stored when some σ > 0)
//! Cb = (1 / eps_r) / (1 + s)        (× dt/(ε0 Δ) in the kernel)
//! Db = 1 / mu_r                     (× dt/(μ0 Δ) in the kernel)
//! ```
//!
//! An E edge takes the **arithmetic** mean of `eps_r` and of `sigma` over the
//! cells that share it (up to four; one or two at a min wall). An H face takes
//! the arithmetic mean of `mu_r` over the (up to two) cells on either side.
//! The tangential edge sits beside both dielectrics, not in series with them.
//! The harmonic mean belongs to the normal D flux of `laplacian_eps_*`; do not
//! "correct" this toward it.
//!
//! PEC edges get `Ca = Cb = 0`. Fields start at 0 and sources may not drive a
//! PEC edge, so such an edge is never changed by the update and stays 0, which
//! is the same as skipping it and writing 0 back.
//!
//! A grid with `eps_r ≡ 1`, `mu_r ≡ 1`, `sigma ≡ 0` and no PEC bakes no arrays
//! and runs the vacuum kernel. Any `Cb` that comes out exactly 1 is the same
//! arithmetic as vacuum, so cells outside the media stay bit-identical.

use super::EPS0;
use rustlab_core::size::ELEM_F64;

/// One cell-centred material map: a single value or one value per cell.
#[derive(Clone, Debug)]
pub enum CellMap {
    Uniform(f64),
    /// x-fastest, `idx(i, j, k) = (k * ny + i) * nx + j`.
    Cells(Vec<f64>),
}

impl CellMap {
    #[inline]
    fn at(&self, c: usize) -> f64 {
        match self {
            CellMap::Uniform(v) => *v,
            CellMap::Cells(v) => v[c],
        }
    }

    pub(super) fn min(&self) -> f64 {
        match self {
            CellMap::Uniform(v) => *v,
            CellMap::Cells(v) => v.iter().copied().fold(f64::INFINITY, f64::min),
        }
    }

    fn all_eq(&self, x: f64) -> bool {
        match self {
            CellMap::Uniform(v) => *v == x,
            CellMap::Cells(v) => v.iter().all(|&c| c == x),
        }
    }

    pub(super) fn values(&self) -> &[f64] {
        match self {
            CellMap::Uniform(v) => std::slice::from_ref(v),
            CellMap::Cells(v) => v,
        }
    }

    fn bytes(&self) -> usize {
        match self {
            CellMap::Uniform(_) => 0,
            CellMap::Cells(v) => v.len() * ELEM_F64,
        }
    }
}

/// Bits of a PEC edge mask entry.
pub(super) const PEC_EX: u8 = 1;
pub(super) const PEC_EY: u8 = 2;
pub(super) const PEC_EZ: u8 = 4;

pub(super) fn pec_bit(c: usize) -> u8 {
    [PEC_EX, PEC_EY, PEC_EZ][c]
}

#[derive(Clone, Debug)]
pub(super) struct Media {
    pub eps: CellMap,
    pub mu: CellMap,
    pub sigma: CellMap,
}

impl Media {
    pub fn bytes(&self) -> usize {
        self.eps.bytes() + self.mu.bytes() + self.sigma.bytes()
    }
}

/// Baked coefficient arrays, each `[Ex, Ey, Ez]` or `[Hx, Hy, Hz]`.
#[derive(Clone, Debug, Default)]
pub(super) struct Baked {
    pub cb: Option<[Vec<f64>; 3]>,
    pub ca: Option<[Vec<f64>; 3]>,
    pub db: Option<[Vec<f64>; 3]>,
}

impl Baked {
    pub fn bytes(&self) -> usize {
        let n = |a: &Option<[Vec<f64>; 3]>| {
            a.as_ref()
                .map_or(0, |a| a.iter().map(|v| v.len() * ELEM_F64).sum())
        };
        n(&self.cb) + n(&self.ca) + n(&self.db)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Grid {
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
}

impl Grid {
    #[inline]
    pub fn idx(&self, i: usize, j: usize, k: usize) -> usize {
        (k * self.ny + i) * self.nx + j
    }
}

/// Arithmetic mean of `map` over the cells `(i − di ..= i, j − dj ..= j,
/// k − dk ..= k)` that exist.
fn mean(
    map: &CellMap,
    g: Grid,
    (i, j, k): (usize, usize, usize),
    (di, dj, dk): (usize, usize, usize),
) -> f64 {
    if let CellMap::Uniform(v) = map {
        return *v;
    }
    let mut sum = 0.0;
    let mut n = 0usize;
    for kk in k.saturating_sub(dk)..=k {
        for ii in i.saturating_sub(di)..=i {
            for jj in j.saturating_sub(dj)..=j {
                sum += map.at(g.idx(ii, jj, kk));
                n += 1;
            }
        }
    }
    sum / n as f64
}

/// Cells around an E edge of component `c`: the two transverse axes.
const E_SPAN: [(usize, usize, usize); 3] = [(1, 0, 1), (0, 1, 1), (1, 1, 0)];
/// Cells on either side of an H face of component `c`: its own axis.
const H_SPAN: [(usize, usize, usize); 3] = [(0, 1, 0), (1, 0, 0), (0, 0, 1)];

/// Bake `media` and `pec` for a step of `dt`. `media = None` is vacuum.
///
/// `extra_sigma` is `(component 0..3, flat edge index, σ)` added to the
/// material mean of that E edge before `Ca`/`Cb`. An empty list leaves the
/// bake unchanged, including a vacuum grid that stores no coefficients.
pub(super) fn bake(
    g: Grid,
    media: Option<&Media>,
    pec: Option<&[u8]>,
    dt: f64,
    extra_sigma: &[(usize, usize, f64)],
) -> Baked {
    let vac = Media {
        eps: CellMap::Uniform(1.0),
        mu: CellMap::Uniform(1.0),
        sigma: CellMap::Uniform(0.0),
    };
    let m = media.unwrap_or(&vac);
    let lossy = !m.sigma.all_eq(0.0) || !extra_sigma.is_empty();
    let need_cb = lossy || !m.eps.all_eq(1.0) || pec.is_some();
    let need_db = !m.mu.all_eq(1.0);
    let n = g.nx * g.ny * g.nz;
    let mut out = Baked::default();

    if need_cb {
        let mut cb: [Vec<f64>; 3] = std::array::from_fn(|_| vec![0.0; n]);
        let mut ca: [Vec<f64>; 3] = std::array::from_fn(|_| vec![0.0; n]);
        for k in 0..g.nz {
            for i in 0..g.ny {
                for j in 0..g.nx {
                    let e = g.idx(i, j, k);
                    for c in 0..3 {
                        if pec.is_some_and(|p| p[e] & pec_bit(c) != 0) {
                            continue;
                        }
                        let eps = mean(&m.eps, g, (i, j, k), E_SPAN[c]);
                        let mut sig = mean(&m.sigma, g, (i, j, k), E_SPAN[c]);
                        if !extra_sigma.is_empty() {
                            for &(pc, pidx, ps) in extra_sigma {
                                if pc == c && pidx == e {
                                    sig += ps;
                                }
                            }
                        }
                        let s = sig * dt / (2.0 * EPS0 * eps);
                        cb[c][e] = (1.0 / eps) / (1.0 + s);
                        ca[c][e] = (1.0 - s) / (1.0 + s);
                    }
                }
            }
        }
        out.cb = Some(cb);
        if lossy {
            out.ca = Some(ca);
        }
    }
    if need_db {
        let mut db: [Vec<f64>; 3] = std::array::from_fn(|_| vec![0.0; n]);
        for k in 0..g.nz {
            for i in 0..g.ny {
                for j in 0..g.nx {
                    let h = g.idx(i, j, k);
                    for c in 0..3 {
                        db[c][h] = 1.0 / mean(&m.mu, g, (i, j, k), H_SPAN[c]);
                    }
                }
            }
        }
        out.db = Some(db);
    }
    out
}

/// Mark the twelve E edges of every nonzero cell of `cells` (x-fastest).
pub(super) fn mark_volume(g: Grid, cells: &[bool], pec: &mut [u8]) {
    for k in 0..g.nz {
        for i in 0..g.ny {
            for j in 0..g.nx {
                if !cells[g.idx(i, j, k)] {
                    continue;
                }
                let mut set = |ii: usize, jj: usize, kk: usize, bit: u8| {
                    if ii < g.ny && jj < g.nx && kk < g.nz {
                        pec[g.idx(ii, jj, kk)] |= bit;
                    }
                };
                for (a, b) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    set(i + a, j, k + b, PEC_EX);
                    set(i, j + a, k + b, PEC_EY);
                    set(i + a, j + b, k, PEC_EZ);
                }
            }
        }
    }
}

/// Mark the tangential E edges of one face. `face` is the 0-based node plane
/// on `axis` (0 = min wall, `n` = max wall). `nodes` is row-major
/// `rows × cols` in the slice layout: z face `ny × nx` (`i`, `j`), y face
/// `nx × nz` (`j`, `k`), x face `ny × nz` (`i`, `k`). An edge is metal when
/// both of its end nodes are; a node on the max wall counts as metal.
pub(super) fn mark_face(g: Grid, axis: super::Axis, face: usize, nodes: &[bool], pec: &mut [u8]) {
    use super::Axis;
    let (rows, cols) = match axis {
        Axis::Z => (g.ny, g.nx),
        Axis::Y => (g.nx, g.nz),
        Axis::X => (g.ny, g.nz),
    };
    let node = |r: usize, c: usize| r >= rows || c >= cols || nodes[r * cols + c];
    let limit = match axis {
        Axis::Z => g.nz,
        Axis::Y => g.ny,
        Axis::X => g.nx,
    };
    if face >= limit {
        return;
    }
    // (edge along the row index, bit), (edge along the column index, bit)
    let (row_bit, col_bit) = match axis {
        Axis::Z => (PEC_EY, PEC_EX),
        Axis::Y => (PEC_EX, PEC_EZ),
        Axis::X => (PEC_EY, PEC_EZ),
    };
    for r in 0..rows {
        for c in 0..cols {
            if !node(r, c) {
                continue;
            }
            let e = match axis {
                Axis::Z => g.idx(r, c, face),
                Axis::Y => g.idx(face, r, c),
                Axis::X => g.idx(r, face, c),
            };
            if node(r + 1, c) {
                pec[e] |= row_bit;
            }
            if node(r, c + 1) {
                pec[e] |= col_bit;
            }
        }
    }
}

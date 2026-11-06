//! Sparse Laplacian stencils on uniform grids.
//!
//! Builders for the 5-point (2-D) and 7-point (3-D) Laplacian operators
//! plus their 1-D tridiagonal cousin, with a unified boundary-condition
//! parameter:
//!
//! - `BoundaryCondition::Dirichlet` — homogeneous Dirichlet (V = 0
//!   outside the grid). Stencil drops cross-boundary off-diagonals.
//! - `BoundaryCondition::Neumann` — homogeneous Neumann (∂V/∂n = 0).
//!   Boundary cells absorb the missing direction's coefficient back
//!   into the diagonal.
//! - `BoundaryCondition::Periodic` — wrap. Edge cells point to their
//!   wrap-around neighbours.
//!
//! All operators approximate `+∇²V`. Sign convention: Poisson
//! `∇²V = -ρ/ε₀` solves as `V = spsolve(L, -rho/eps0)` for Dirichlet,
//! or as a singular-system pin-and-solve for Neumann/Periodic
//! (constants are in the null space).
//!
//! Index conventions (column-major flat indexing):
//! - 1-D: `k = i`.
//! - 2-D: `k = j*ny + i` (rows index y, columns index x).
//! - 3-D: `k = (kk*nx + j)*ny + i` (axis 0 = y, axis 1 = x, axis 2 = z).
//!
//! Helpers `ij2k`, `k2ij`, `ijk2k`, `k2ijk` round-trip these indices.
//!
//! The `laplacian_eps_2d` and `laplacian_eps_3d` builders solve the
//! variable-coefficient case `∇·(ε∇V)` via a flux-conservative
//! discretization with harmonic-mean half-cell coefficients — the
//! standard formulation for Poisson in a piecewise-uniform dielectric.

use crate::error::DspError;
use ndarray::Array3;
use num_complex::Complex;
use rustlab_core::{CMatrix, CTensor3, SparseMat, C64};

/// Boundary-condition selector for `laplacian_*` builders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryCondition {
    Dirichlet,
    Neumann,
    Periodic,
}

impl BoundaryCondition {
    /// Parse the curriculum-facing string form.
    pub fn from_str(s: &str) -> Result<Self, DspError> {
        match s {
            "dirichlet" => Ok(BoundaryCondition::Dirichlet),
            "neumann" => Ok(BoundaryCondition::Neumann),
            "periodic" => Ok(BoundaryCondition::Periodic),
            other => Err(DspError::InvalidParameter(format!(
                "boundary condition must be \"dirichlet\", \"neumann\", or \"periodic\", got \"{other}\""
            ))),
        }
    }
}

fn check_pos(name: &str, label: &str, h: f64) -> Result<(), DspError> {
    if !h.is_finite() || h <= 0.0 {
        return Err(DspError::InvalidParameter(format!(
            "{name}: {label} must be positive and finite, got {h}"
        )));
    }
    Ok(())
}

fn check_min(name: &str, label: &str, n: usize, min: usize) -> Result<(), DspError> {
    if n < min {
        return Err(DspError::InvalidParameter(format!(
            "{name}: {label} must be >= {min}, got {n}"
        )));
    }
    Ok(())
}

fn c(v: f64) -> C64 {
    Complex::new(v, 0.0)
}

/// Harmonic mean `2ab / (a + b)` of two complex face neighbours. Returns 0
/// when the sum underflows (consistent with "no flux through a vacuum
/// face"). Shared by the 2-D and 3-D variable-coefficient builders.
fn hmean(a: C64, b: C64) -> C64 {
    let sum = a + b;
    if sum.norm() < 1e-300 {
        Complex::new(0.0, 0.0)
    } else {
        (Complex::new(2.0, 0.0) * a * b) / sum
    }
}

/// Helper for the row-by-row Laplacian builders: collect up to `MAX`
/// `(col, value)` pairs into a stack-allocated buffer, sort by column,
/// emit into the output entries vec at the given row. The per-row sort
/// is constant-time since the stencil width is bounded.
fn flush_row<const MAX: usize>(
    out: &mut Vec<(usize, usize, C64)>,
    row: usize,
    buf: &mut [(usize, C64); MAX],
    len: usize,
) {
    let row_buf = &mut buf[..len];
    row_buf.sort_by_key(|&(c, _)| c);
    for &(col, val) in row_buf.iter() {
        out.push((row, col, val));
    }
}

/// 1-D tridiagonal Laplacian on a uniform grid with `n` cells, spacing
/// `dx`, and the given boundary condition. Returns an `n × n` sparse
/// matrix approximating `+d²V/dx²`.
pub fn laplacian_1d(n: usize, dx: f64, bc: BoundaryCondition) -> Result<SparseMat, DspError> {
    check_min("laplacian_1d", "n", n, 2)?;
    check_pos("laplacian_1d", "dx", dx)?;
    let inv_dx2 = 1.0 / (dx * dx);
    let mut entries: Vec<(usize, usize, C64)> = Vec::with_capacity(3 * n);
    let mut buf: [(usize, C64); 3] = [(0, c(0.0)); 3];

    for i in 0..n {
        let mut diag = -2.0 * inv_dx2;
        let mut len = 0usize;

        // Left neighbour.
        if i > 0 {
            buf[len] = (i - 1, c(inv_dx2));
            len += 1;
        } else {
            match bc {
                BoundaryCondition::Dirichlet => {}
                BoundaryCondition::Neumann => diag += inv_dx2,
                BoundaryCondition::Periodic => {
                    buf[len] = (n - 1, c(inv_dx2));
                    len += 1;
                }
            }
        }
        // Right neighbour.
        if i + 1 < n {
            buf[len] = (i + 1, c(inv_dx2));
            len += 1;
        } else {
            match bc {
                BoundaryCondition::Dirichlet => {}
                BoundaryCondition::Neumann => diag += inv_dx2,
                BoundaryCondition::Periodic => {
                    buf[len] = (0, c(inv_dx2));
                    len += 1;
                }
            }
        }
        // Diagonal.
        buf[len] = (i, c(diag));
        len += 1;

        flush_row::<3>(&mut entries, i, &mut buf, len);
    }
    Ok(SparseMat::from_sorted_entries(n, n, entries))
}

/// 2-D 5-point Laplacian on an `nx × ny` uniform grid with the given
/// boundary condition. Returns an `(nx·ny) × (nx·ny)` sparse matrix
/// using column-major flat indexing `k = j*ny + i`.
pub fn laplacian_2d_bc(
    nx: usize,
    ny: usize,
    dx: f64,
    dy: f64,
    bc: BoundaryCondition,
) -> Result<SparseMat, DspError> {
    check_min("laplacian_2d", "nx", nx, 2)?;
    check_min("laplacian_2d", "ny", ny, 2)?;
    check_pos("laplacian_2d", "dx", dx)?;
    check_pos("laplacian_2d", "dy", dy)?;
    let inv_dx2 = 1.0 / (dx * dx);
    let inv_dy2 = 1.0 / (dy * dy);
    let n = rustlab_core::checked_elems(&[nx, ny], rustlab_core::ELEM_F64).map_err(|_| {
        DspError::InvalidParameter(rustlab_core::variable_too_large("laplacian_2d", &[nx, ny]))
    })?;
    let mut entries: Vec<(usize, usize, C64)> = Vec::with_capacity(5 * n);
    let mut buf: [(usize, C64); 5] = [(0, c(0.0)); 5];

    // Outer loop walks rows in ascending k = j*ny + i order. Within
    // each row, `buf` collects all stencil contributions; flush_row
    // sorts by column and emits in row-major-then-column-major order.
    for j in 0..nx {
        for i in 0..ny {
            let k = j * ny + i;
            let mut diag = -2.0 * (inv_dx2 + inv_dy2);
            let mut len = 0usize;

            // y-direction (i): down/up
            if i > 0 {
                buf[len] = (k - 1, c(inv_dy2));
                len += 1;
            } else {
                match bc {
                    BoundaryCondition::Dirichlet => {}
                    BoundaryCondition::Neumann => diag += inv_dy2,
                    BoundaryCondition::Periodic => {
                        buf[len] = (k + (ny - 1), c(inv_dy2));
                        len += 1;
                    }
                }
            }
            if i + 1 < ny {
                buf[len] = (k + 1, c(inv_dy2));
                len += 1;
            } else {
                match bc {
                    BoundaryCondition::Dirichlet => {}
                    BoundaryCondition::Neumann => diag += inv_dy2,
                    BoundaryCondition::Periodic => {
                        buf[len] = (k - (ny - 1), c(inv_dy2));
                        len += 1;
                    }
                }
            }

            // x-direction (j): left/right (stride ny)
            if j > 0 {
                buf[len] = (k - ny, c(inv_dx2));
                len += 1;
            } else {
                match bc {
                    BoundaryCondition::Dirichlet => {}
                    BoundaryCondition::Neumann => diag += inv_dx2,
                    BoundaryCondition::Periodic => {
                        buf[len] = (k + (nx - 1) * ny, c(inv_dx2));
                        len += 1;
                    }
                }
            }
            if j + 1 < nx {
                buf[len] = (k + ny, c(inv_dx2));
                len += 1;
            } else {
                match bc {
                    BoundaryCondition::Dirichlet => {}
                    BoundaryCondition::Neumann => diag += inv_dx2,
                    BoundaryCondition::Periodic => {
                        buf[len] = (k - (nx - 1) * ny, c(inv_dx2));
                        len += 1;
                    }
                }
            }

            // Diagonal.
            buf[len] = (k, c(diag));
            len += 1;

            flush_row::<5>(&mut entries, k, &mut buf, len);
        }
    }

    Ok(SparseMat::from_sorted_entries(n, n, entries))
}

/// 3-D 7-point Laplacian on an `nx × ny × nz` uniform grid with the
/// given boundary condition. Returns an `(nx·ny·nz) × (nx·ny·nz)`
/// sparse matrix with the column-major-of-pages flat indexing
/// `k = (kk*nx + j)*ny + i` — axis 0 = y (rows), axis 1 = x (cols),
/// axis 2 = z (pages). Matches the rustlab `Tensor3` convention.
pub fn laplacian_3d(
    nx: usize,
    ny: usize,
    nz: usize,
    dx: f64,
    dy: f64,
    dz: f64,
    bc: BoundaryCondition,
) -> Result<SparseMat, DspError> {
    check_min("laplacian_3d", "nx", nx, 2)?;
    check_min("laplacian_3d", "ny", ny, 2)?;
    check_min("laplacian_3d", "nz", nz, 2)?;
    check_pos("laplacian_3d", "dx", dx)?;
    check_pos("laplacian_3d", "dy", dy)?;
    check_pos("laplacian_3d", "dz", dz)?;
    let inv_dx2 = 1.0 / (dx * dx);
    let inv_dy2 = 1.0 / (dy * dy);
    let inv_dz2 = 1.0 / (dz * dz);
    let n = nx * ny * nz;
    let mut entries: Vec<(usize, usize, C64)> = Vec::with_capacity(7 * n);
    let mut buf: [(usize, C64); 7] = [(0, c(0.0)); 7];

    let stride_y = 1;
    let stride_x = ny;
    let stride_z = nx * ny;

    // Outer loop walks rows in ascending flat-index order.
    for kk in 0..nz {
        for j in 0..nx {
            for i in 0..ny {
                let k = (kk * nx + j) * ny + i;
                let mut diag = -2.0 * (inv_dx2 + inv_dy2 + inv_dz2);
                let mut len = 0usize;

                // y-direction (i)
                if i > 0 {
                    buf[len] = (k - stride_y, c(inv_dy2));
                    len += 1;
                } else {
                    match bc {
                        BoundaryCondition::Dirichlet => {}
                        BoundaryCondition::Neumann => diag += inv_dy2,
                        BoundaryCondition::Periodic => {
                            buf[len] = (k + (ny - 1) * stride_y, c(inv_dy2));
                            len += 1;
                        }
                    }
                }
                if i + 1 < ny {
                    buf[len] = (k + stride_y, c(inv_dy2));
                    len += 1;
                } else {
                    match bc {
                        BoundaryCondition::Dirichlet => {}
                        BoundaryCondition::Neumann => diag += inv_dy2,
                        BoundaryCondition::Periodic => {
                            buf[len] = (k - (ny - 1) * stride_y, c(inv_dy2));
                            len += 1;
                        }
                    }
                }

                // x-direction (j)
                if j > 0 {
                    buf[len] = (k - stride_x, c(inv_dx2));
                    len += 1;
                } else {
                    match bc {
                        BoundaryCondition::Dirichlet => {}
                        BoundaryCondition::Neumann => diag += inv_dx2,
                        BoundaryCondition::Periodic => {
                            buf[len] = (k + (nx - 1) * stride_x, c(inv_dx2));
                            len += 1;
                        }
                    }
                }
                if j + 1 < nx {
                    buf[len] = (k + stride_x, c(inv_dx2));
                    len += 1;
                } else {
                    match bc {
                        BoundaryCondition::Dirichlet => {}
                        BoundaryCondition::Neumann => diag += inv_dx2,
                        BoundaryCondition::Periodic => {
                            buf[len] = (k - (nx - 1) * stride_x, c(inv_dx2));
                            len += 1;
                        }
                    }
                }

                // z-direction (kk)
                if kk > 0 {
                    buf[len] = (k - stride_z, c(inv_dz2));
                    len += 1;
                } else {
                    match bc {
                        BoundaryCondition::Dirichlet => {}
                        BoundaryCondition::Neumann => diag += inv_dz2,
                        BoundaryCondition::Periodic => {
                            buf[len] = (k + (nz - 1) * stride_z, c(inv_dz2));
                            len += 1;
                        }
                    }
                }
                if kk + 1 < nz {
                    buf[len] = (k + stride_z, c(inv_dz2));
                    len += 1;
                } else {
                    match bc {
                        BoundaryCondition::Dirichlet => {}
                        BoundaryCondition::Neumann => diag += inv_dz2,
                        BoundaryCondition::Periodic => {
                            buf[len] = (k - (nz - 1) * stride_z, c(inv_dz2));
                            len += 1;
                        }
                    }
                }

                // Diagonal.
                buf[len] = (k, c(diag));
                len += 1;

                flush_row::<7>(&mut entries, k, &mut buf, len);
            }
        }
    }

    Ok(SparseMat::from_sorted_entries(n, n, entries))
}

/// Variable-coefficient Laplacian `∇·(ε∇V)` on a 2-D uniform grid.
///
/// Flux-conservative discretization with harmonic-mean half-cell
/// coefficients:
///
/// ```text
/// ε_{i,j+1/2} = 2·ε(i,j)·ε(i,j+1) / (ε(i,j) + ε(i,j+1))
/// ```
///
/// and similarly for the other three faces. The harmonic mean is the
/// physically-correct choice for piecewise-uniform media because it
/// preserves flux continuity across material interfaces (where
/// arithmetic-mean discretizations introduce artificial sources).
///
/// `eps_map` is shape `(ny, nx)` matching `meshgrid` / `imagesc`. Real
/// or complex entries (lossy materials are common in FDFD-style work).
/// Index convention: same column-major `k = j*ny + i` as
/// `laplacian_2d_bc`.
///
/// Boundary semantics:
/// - Dirichlet (default): drop the off-diagonal coefficient at the
///   boundary face. The cell still pays the half-cell ε in its
///   diagonal because the ghost cell value is zero.
/// - Neumann: skip the boundary face entirely (zero flux). Diagonal
///   does not absorb the missing coefficient.
/// - Periodic: wrap to the opposite-side cell.
///
/// Setting `eps_map ≡ 1` reduces this to `laplacian_2d` exactly (modulo
/// the boundary-cell diagonal in the Dirichlet case, where the
/// constant-ε flux-conservative form gives the same rows as the
/// fixed-coefficient stencil).
pub fn laplacian_eps_2d(
    eps_map: &CMatrix,
    dx: f64,
    dy: f64,
    bc: BoundaryCondition,
) -> Result<SparseMat, DspError> {
    let (ny, nx) = eps_map.dim();
    check_min("laplacian_eps_2d", "nx (eps_map cols)", nx, 2)?;
    check_min("laplacian_eps_2d", "ny (eps_map rows)", ny, 2)?;
    check_pos("laplacian_eps_2d", "dx", dx)?;
    check_pos("laplacian_eps_2d", "dy", dy)?;
    let inv_dx2 = 1.0 / (dx * dx);
    let inv_dy2 = 1.0 / (dy * dy);
    let n = nx * ny;
    let mut entries: Vec<(usize, usize, C64)> = Vec::with_capacity(5 * n);
    let mut buf: [(usize, C64); 5] = [(0, c(0.0)); 5];

    let eps_at = |i: usize, j: usize| eps_map[[i, j]];

    for j in 0..nx {
        for i in 0..ny {
            let k = j * ny + i;
            let me = eps_at(i, j);
            let mut diag = Complex::new(0.0, 0.0);
            let mut len = 0usize;

            // y-direction (i): down/up
            if i > 0 {
                let eps_face = hmean(me, eps_at(i - 1, j));
                let coeff = eps_face * c(inv_dy2);
                buf[len] = (k - 1, coeff);
                len += 1;
                diag -= coeff;
            } else {
                match bc {
                    BoundaryCondition::Dirichlet => {
                        let eps_face = hmean(me, me);
                        diag -= eps_face * c(inv_dy2);
                    }
                    BoundaryCondition::Neumann => {}
                    BoundaryCondition::Periodic => {
                        let eps_face = hmean(me, eps_at(ny - 1, j));
                        let coeff = eps_face * c(inv_dy2);
                        buf[len] = (k + (ny - 1), coeff);
                        len += 1;
                        diag -= coeff;
                    }
                }
            }
            if i + 1 < ny {
                let eps_face = hmean(me, eps_at(i + 1, j));
                let coeff = eps_face * c(inv_dy2);
                buf[len] = (k + 1, coeff);
                len += 1;
                diag -= coeff;
            } else {
                match bc {
                    BoundaryCondition::Dirichlet => {
                        let eps_face = hmean(me, me);
                        diag -= eps_face * c(inv_dy2);
                    }
                    BoundaryCondition::Neumann => {}
                    BoundaryCondition::Periodic => {
                        let eps_face = hmean(me, eps_at(0, j));
                        let coeff = eps_face * c(inv_dy2);
                        buf[len] = (k - (ny - 1), coeff);
                        len += 1;
                        diag -= coeff;
                    }
                }
            }

            // x-direction (j): left/right (stride ny)
            if j > 0 {
                let eps_face = hmean(me, eps_at(i, j - 1));
                let coeff = eps_face * c(inv_dx2);
                buf[len] = (k - ny, coeff);
                len += 1;
                diag -= coeff;
            } else {
                match bc {
                    BoundaryCondition::Dirichlet => {
                        let eps_face = hmean(me, me);
                        diag -= eps_face * c(inv_dx2);
                    }
                    BoundaryCondition::Neumann => {}
                    BoundaryCondition::Periodic => {
                        let eps_face = hmean(me, eps_at(i, nx - 1));
                        let coeff = eps_face * c(inv_dx2);
                        buf[len] = (k + (nx - 1) * ny, coeff);
                        len += 1;
                        diag -= coeff;
                    }
                }
            }
            if j + 1 < nx {
                let eps_face = hmean(me, eps_at(i, j + 1));
                let coeff = eps_face * c(inv_dx2);
                buf[len] = (k + ny, coeff);
                len += 1;
                diag -= coeff;
            } else {
                match bc {
                    BoundaryCondition::Dirichlet => {
                        let eps_face = hmean(me, me);
                        diag -= eps_face * c(inv_dx2);
                    }
                    BoundaryCondition::Neumann => {}
                    BoundaryCondition::Periodic => {
                        let eps_face = hmean(me, eps_at(i, 0));
                        let coeff = eps_face * c(inv_dx2);
                        buf[len] = (k - (nx - 1) * ny, coeff);
                        len += 1;
                        diag -= coeff;
                    }
                }
            }

            // Diagonal.
            buf[len] = (k, diag);
            len += 1;

            flush_row::<5>(&mut entries, k, &mut buf, len);
        }
    }

    Ok(SparseMat::from_sorted_entries(n, n, entries))
}

/// Variable-coefficient Laplacian `∇·(ε∇V)` on a 3-D uniform grid.
///
/// The 3-D form of [`laplacian_eps_2d`]: each of the six faces of a cell
/// carries the harmonic mean `2·εa·εb / (εa + εb)` of the two cells that
/// share it, scaled by `1/h²` for that axis. `eps_map` is shape
/// `(ny, nx, nz)` — the `Tensor3` convention, axis 0 = y, axis 1 = x,
/// axis 2 = z — real or complex. Flat index and strides are those of
/// [`laplacian_3d`]: `k = (kk*nx + j)*ny + i`.
///
/// Boundary semantics match the 2-D builder:
/// - Dirichlet: the ghost cell is 0, so the boundary face drops its
///   off-diagonal and still subtracts the face coefficient (the cell's
///   own ε) from the diagonal.
/// - Neumann: the boundary face is skipped (zero flux).
/// - Periodic: wrap to the opposite-side cell.
///
/// A uniform `eps_map ≡ c` gives `c * laplacian_3d(...)` entrywise.
pub fn laplacian_eps_3d(
    eps_map: &CTensor3,
    dx: f64,
    dy: f64,
    dz: f64,
    bc: BoundaryCondition,
) -> Result<SparseMat, DspError> {
    let (ny, nx, nz) = eps_map.dim();
    laplacian_eps_3d_with(ny, nx, nz, |i, j, kk| eps_map[[i, j, kk]], dx, dy, dz, bc)
}

/// [`laplacian_eps_3d`] for a real `eps_map`. The result is bit for bit
/// the operator built from the same values stored as complex.
pub fn laplacian_eps_3d_real(
    eps_map: &Array3<f64>,
    dx: f64,
    dy: f64,
    dz: f64,
    bc: BoundaryCondition,
) -> Result<SparseMat, DspError> {
    let (ny, nx, nz) = eps_map.dim();
    laplacian_eps_3d_with(
        ny,
        nx,
        nz,
        |i, j, kk| c(eps_map[[i, j, kk]]),
        dx,
        dy,
        dz,
        bc,
    )
}

/// One face of a cell in the variable-coefficient stencil.
enum Face {
    /// A neighbouring cell (interior or periodic wrap): column and its ε.
    Cell(usize, C64),
    /// Dirichlet ghost cell outside the grid (value 0).
    Ghost,
    /// Neumann boundary: no flux, nothing emitted.
    Closed,
}

#[allow(clippy::too_many_arguments)]
fn laplacian_eps_3d_with(
    ny: usize,
    nx: usize,
    nz: usize,
    eps_at: impl Fn(usize, usize, usize) -> C64,
    dx: f64,
    dy: f64,
    dz: f64,
    bc: BoundaryCondition,
) -> Result<SparseMat, DspError> {
    const NAME: &str = "laplacian_eps_3d";
    check_min(NAME, "ny (eps_map rows)", ny, 2)?;
    check_min(NAME, "nx (eps_map cols)", nx, 2)?;
    check_min(NAME, "nz (eps_map pages)", nz, 2)?;
    check_pos(NAME, "dx", dx)?;
    check_pos(NAME, "dy", dy)?;
    check_pos(NAME, "dz", dz)?;
    let n = rustlab_core::checked_elems(&[ny, nx, nz], rustlab_core::ELEM_F64).map_err(|_| {
        DspError::InvalidParameter(rustlab_core::variable_too_large(NAME, &[ny, nx, nz]))
    })?;
    let inv_dx2 = 1.0 / (dx * dx);
    let inv_dy2 = 1.0 / (dy * dy);
    let inv_dz2 = 1.0 / (dz * dz);
    let mut entries: Vec<(usize, usize, C64)> = Vec::with_capacity(7 * n);
    let mut buf: [(usize, C64); 7] = [(0, c(0.0)); 7];

    let stride_y = 1;
    let stride_x = ny;
    let stride_z = nx * ny;

    // A face on the grid edge; `wrap_*` is the opposite-side cell used
    // only by the periodic case.
    let boundary = |wrap_col: usize, wrap_eps: C64| match bc {
        BoundaryCondition::Dirichlet => Face::Ghost,
        BoundaryCondition::Neumann => Face::Closed,
        BoundaryCondition::Periodic => Face::Cell(wrap_col, wrap_eps),
    };

    for kk in 0..nz {
        for j in 0..nx {
            for i in 0..ny {
                let k = (kk * nx + j) * ny + i;
                let me = eps_at(i, j, kk);
                let mut diag = c(0.0);
                let mut len = 0usize;

                let faces = [
                    // y-direction (i)
                    (
                        if i > 0 {
                            Face::Cell(k - stride_y, eps_at(i - 1, j, kk))
                        } else {
                            boundary(k + (ny - 1) * stride_y, eps_at(ny - 1, j, kk))
                        },
                        inv_dy2,
                    ),
                    (
                        if i + 1 < ny {
                            Face::Cell(k + stride_y, eps_at(i + 1, j, kk))
                        } else {
                            boundary(k - (ny - 1) * stride_y, eps_at(0, j, kk))
                        },
                        inv_dy2,
                    ),
                    // x-direction (j)
                    (
                        if j > 0 {
                            Face::Cell(k - stride_x, eps_at(i, j - 1, kk))
                        } else {
                            boundary(k + (nx - 1) * stride_x, eps_at(i, nx - 1, kk))
                        },
                        inv_dx2,
                    ),
                    (
                        if j + 1 < nx {
                            Face::Cell(k + stride_x, eps_at(i, j + 1, kk))
                        } else {
                            boundary(k - (nx - 1) * stride_x, eps_at(i, 0, kk))
                        },
                        inv_dx2,
                    ),
                    // z-direction (kk)
                    (
                        if kk > 0 {
                            Face::Cell(k - stride_z, eps_at(i, j, kk - 1))
                        } else {
                            boundary(k + (nz - 1) * stride_z, eps_at(i, j, nz - 1))
                        },
                        inv_dz2,
                    ),
                    (
                        if kk + 1 < nz {
                            Face::Cell(k + stride_z, eps_at(i, j, kk + 1))
                        } else {
                            boundary(k - (nz - 1) * stride_z, eps_at(i, j, 0))
                        },
                        inv_dz2,
                    ),
                ];

                for (face, inv_h2) in faces {
                    match face {
                        Face::Cell(col, eps_nb) => {
                            let coeff = hmean(me, eps_nb) * c(inv_h2);
                            buf[len] = (col, coeff);
                            len += 1;
                            diag -= coeff;
                        }
                        Face::Ghost => diag -= hmean(me, me) * c(inv_h2),
                        Face::Closed => {}
                    }
                }

                buf[len] = (k, diag);
                len += 1;

                flush_row::<7>(&mut entries, k, &mut buf, len);
            }
        }
    }

    Ok(SparseMat::from_sorted_entries(n, n, entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + a.abs() + b.abs())
    }

    fn dense(s: &SparseMat) -> Array2<f64> {
        let mut m = Array2::zeros((s.rows, s.cols));
        for &(r, c, v) in &s.entries {
            m[[r, c]] = v.re;
        }
        m
    }

    // ─── 1-D ──────────────────────────────────────────────────────

    #[test]
    fn lap_1d_dirichlet_diag() {
        // Interior diagonal is -2/dx^2, off-diagonals +1/dx^2.
        let l = laplacian_1d(5, 1.0, BoundaryCondition::Dirichlet).unwrap();
        let m = dense(&l);
        for i in 0..5 {
            assert!(close(m[[i, i]], -2.0, 1e-12), "diag[{i}] = {}", m[[i, i]]);
        }
        for i in 0..4 {
            assert!(close(m[[i, i + 1]], 1.0, 1e-12));
            assert!(close(m[[i + 1, i]], 1.0, 1e-12));
        }
    }

    #[test]
    fn lap_1d_neumann_boundary_diag() {
        // Boundary cells absorb the missing direction's coefficient back
        // into the diagonal: -2/dx^2 + 1/dx^2 = -1/dx^2.
        let l = laplacian_1d(5, 1.0, BoundaryCondition::Neumann).unwrap();
        let m = dense(&l);
        assert!(close(m[[0, 0]], -1.0, 1e-12));
        assert!(close(m[[4, 4]], -1.0, 1e-12));
        assert!(close(m[[2, 2]], -2.0, 1e-12)); // interior unchanged
    }

    #[test]
    fn lap_1d_periodic_wrap() {
        let l = laplacian_1d(4, 1.0, BoundaryCondition::Periodic).unwrap();
        let m = dense(&l);
        // Diagonals all -2; off-diagonals at +1 with wrap.
        for i in 0..4 {
            assert!(close(m[[i, i]], -2.0, 1e-12));
        }
        // Boundaries wrap.
        assert!(close(m[[0, 3]], 1.0, 1e-12));
        assert!(close(m[[3, 0]], 1.0, 1e-12));
    }

    #[test]
    fn lap_1d_periodic_constant_in_nullspace() {
        // L * 1 = 0 for the periodic Laplacian (constants are in the null space).
        let n = 6;
        let l = laplacian_1d(n, 0.5, BoundaryCondition::Periodic).unwrap();
        let ones = vec![Complex::new(1.0, 0.0); n];
        let v = l
            .spmv(&ndarray::Array1::from_vec(ones))
            .unwrap();
        for c in v.iter() {
            assert!(c.norm() < 1e-12, "nonzero: {c}");
        }
    }

    // ─── 2-D ──────────────────────────────────────────────────────

    #[test]
    fn lap_2d_dirichlet_matches_existing_convention() {
        // Spot-check a 3x3 grid against the existing builtin_laplacian_2d
        // convention: column-major, diagonal -2(1/dx^2 + 1/dy^2).
        let l = laplacian_2d_bc(3, 3, 1.0, 1.0, BoundaryCondition::Dirichlet).unwrap();
        let m = dense(&l);
        // Centre cell (j=1, i=1) → k = 1*3 + 1 = 4
        assert!(close(m[[4, 4]], -4.0, 1e-12));
        // Centre's four neighbours: rows ±1, cols ±ny=3
        assert!(close(m[[4, 3]], 1.0, 1e-12)); // i-1
        assert!(close(m[[4, 5]], 1.0, 1e-12)); // i+1
        assert!(close(m[[4, 1]], 1.0, 1e-12)); // j-1
        assert!(close(m[[4, 7]], 1.0, 1e-12)); // j+1
    }

    #[test]
    fn lap_2d_neumann_corner_diag() {
        // Corner cell (i=0, j=0) loses two faces in Neumann; diagonal
        // becomes -2(1/dx^2 + 1/dy^2) + 1/dx^2 + 1/dy^2 = -1(1/dx^2 + 1/dy^2).
        let l = laplacian_2d_bc(3, 3, 1.0, 1.0, BoundaryCondition::Neumann).unwrap();
        let m = dense(&l);
        // k = 0
        assert!(close(m[[0, 0]], -2.0, 1e-12));
    }

    #[test]
    fn lap_2d_periodic_constant_in_nullspace() {
        let nx = 4;
        let ny = 4;
        let l = laplacian_2d_bc(nx, ny, 1.0, 1.0, BoundaryCondition::Periodic).unwrap();
        let ones = vec![Complex::new(1.0, 0.0); nx * ny];
        let v = l
            .spmv(&ndarray::Array1::from_vec(ones))
            .unwrap();
        for c in v.iter() {
            assert!(c.norm() < 1e-12);
        }
    }

    // ─── 3-D ──────────────────────────────────────────────────────

    #[test]
    fn lap_3d_dirichlet_centre_diag() {
        // 3x3x3 grid; centre cell (i=1, j=1, kk=1).
        let l = laplacian_3d(3, 3, 3, 1.0, 1.0, 1.0, BoundaryCondition::Dirichlet).unwrap();
        let m = dense(&l);
        // k = (1*3 + 1)*3 + 1 = 13
        // diagonal = -2 * 3 = -6
        assert!(close(m[[13, 13]], -6.0, 1e-12));
        // Each face neighbour at offset ±1 (y), ±3 (x), ±9 (z) with value 1.
        for off in [1usize, 3, 9] {
            assert!(close(m[[13, 13 - off]], 1.0, 1e-12), "offset -{off}");
            assert!(close(m[[13, 13 + off]], 1.0, 1e-12), "offset +{off}");
        }
    }

    #[test]
    fn lap_3d_neumann_corner_diag() {
        // 3x3x3 corner (i=0, j=0, kk=0). Neumann strips three faces.
        // Diagonal: -6 + 3 = -3.
        let l = laplacian_3d(3, 3, 3, 1.0, 1.0, 1.0, BoundaryCondition::Neumann).unwrap();
        let m = dense(&l);
        assert!(close(m[[0, 0]], -3.0, 1e-12));
    }

    #[test]
    fn lap_3d_periodic_constant_in_nullspace() {
        let l = laplacian_3d(3, 3, 3, 0.5, 0.5, 0.5, BoundaryCondition::Periodic).unwrap();
        let ones = vec![Complex::new(1.0, 0.0); 27];
        let v = l
            .spmv(&ndarray::Array1::from_vec(ones))
            .unwrap();
        for c in v.iter() {
            assert!(c.norm() < 1e-12);
        }
    }

    // ─── eps-Laplacian ────────────────────────────────────────────

    #[test]
    fn lap_eps_unit_eps_matches_lap_2d() {
        // Setting eps_map ≡ 1 should reproduce laplacian_2d_bc Dirichlet
        // up to the Dirichlet-boundary half-cell diagonal handling.
        let nx = 3;
        let ny = 3;
        let dx = 1.0;
        let dy = 1.0;
        let eps = Array2::from_elem((ny, nx), Complex::new(1.0, 0.0));
        let le =
            laplacian_eps_2d(&eps, dx, dy, BoundaryCondition::Dirichlet).unwrap();
        let l = laplacian_2d_bc(nx, ny, dx, dy, BoundaryCondition::Dirichlet).unwrap();
        let me = dense(&le);
        let m = dense(&l);
        for i in 0..nx * ny {
            for j in 0..nx * ny {
                assert!(
                    (me[[i, j]] - m[[i, j]]).abs() < 1e-12,
                    "({i},{j}): eps={} vs lap={}",
                    me[[i, j]],
                    m[[i, j]]
                );
            }
        }
    }

    #[test]
    fn lap_eps_flux_conservation_neumann() {
        // For Neumann boundaries with ANY eps_map, applying the Laplacian
        // to a constant vector should give zero (constants are in the
        // null space — true for ∇·(ε∇·) regardless of ε).
        let ny = 4;
        let nx = 4;
        let mut eps = Array2::zeros((ny, nx));
        for i in 0..ny {
            for j in 0..nx {
                eps[[i, j]] = Complex::new(1.0 + (i + j) as f64 * 0.5, 0.0);
            }
        }
        let l =
            laplacian_eps_2d(&eps, 1.0, 1.0, BoundaryCondition::Neumann).unwrap();
        let ones = vec![Complex::new(1.0, 0.0); nx * ny];
        let v = l
            .spmv(&ndarray::Array1::from_vec(ones))
            .unwrap();
        for c in v.iter() {
            assert!(c.norm() < 1e-12, "non-zero entry: {c}");
        }
    }

    #[test]
    fn lap_eps_complex_lossy_runs() {
        // Smoke test: complex eps_map (lossy material) should produce a
        // valid sparse matrix with no panics.
        let ny = 5;
        let nx = 5;
        let mut eps = Array2::zeros((ny, nx));
        for i in 0..ny {
            for j in 0..nx {
                eps[[i, j]] = Complex::new(2.0, -0.1);
            }
        }
        let l =
            laplacian_eps_2d(&eps, 0.1, 0.1, BoundaryCondition::Dirichlet).unwrap();
        assert_eq!(l.rows, ny * nx);
        assert_eq!(l.cols, ny * nx);
        assert!(l.nnz() > 0);
    }

    // ─── 3-D eps-Laplacian ────────────────────────────────────────

    const ALL_BC: [BoundaryCondition; 3] = [
        BoundaryCondition::Dirichlet,
        BoundaryCondition::Neumann,
        BoundaryCondition::Periodic,
    ];

    fn dense_c(s: &SparseMat) -> Array2<C64> {
        let mut m = Array2::zeros((s.rows, s.cols));
        for &(r, c, v) in &s.entries {
            m[[r, c]] += v;
        }
        m
    }

    /// Deterministic values in [0, 1) so the tests do not need an RNG crate.
    fn lcg(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A positive, strongly varying real ε map (range [0.5, 20.5)).
    fn varied_eps(ny: usize, nx: usize, nz: usize, seed: u64) -> CTensor3 {
        let mut s = seed;
        Array3::from_shape_fn((ny, nx, nz), |_| c(0.5 + 20.0 * lcg(&mut s)))
    }

    #[test]
    fn lap_eps_3d_uniform_eps_is_scaled_lap_3d() {
        // ε ≡ c gives c * laplacian_3d entrywise, for every bc, anisotropic
        // spacing, and sizes including the n = 2 periodic duplicate case.
        let shapes = [(2, 2, 2), (3, 4, 5), (5, 3, 2)];
        let consts = [c(3.7), Complex::new(2.0, -0.3)];
        for &(ny, nx, nz) in &shapes {
            for &cst in &consts {
                for bc in ALL_BC {
                    let eps = Array3::from_elem((ny, nx, nz), cst);
                    let le = laplacian_eps_3d(&eps, 0.1, 0.25, 0.04, bc).unwrap();
                    let l = laplacian_3d(nx, ny, nz, 0.1, 0.25, 0.04, bc).unwrap();
                    let (me, m) = (dense_c(&le), dense_c(&l));
                    let scale = m.iter().map(|v| v.norm()).fold(0.0, f64::max) * cst.norm();
                    for (a, b) in me.iter().zip(m.iter()) {
                        assert!(
                            (a - cst * b).norm() <= 1e-12 * scale,
                            "{bc:?} {ny}x{nx}x{nz} c={cst}: {a} vs {}",
                            cst * b
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn lap_eps_3d_constants_in_null_space() {
        // Neumann and periodic: L * 1 = 0 for any ε (zero net flux).
        let eps = varied_eps(4, 5, 3, 7);
        for bc in [BoundaryCondition::Neumann, BoundaryCondition::Periodic] {
            let l = laplacian_eps_3d(&eps, 0.5, 1.0, 0.2, bc).unwrap();
            let v = l.spmv(&ndarray::Array1::from_elem(60, c(1.0))).unwrap();
            let scale = l.entries.iter().map(|e| e.2.norm()).fold(0.0, f64::max);
            for r in v.iter() {
                assert!(r.norm() < 1e-12 * scale, "{bc:?}: residual {r}");
            }
        }
    }

    #[test]
    fn lap_eps_3d_dirichlet_constant_not_in_null_space() {
        // The grounded ghost cells make the Dirichlet operator nonsingular,
        // so a constant leaks flux at every boundary cell.
        let eps = varied_eps(3, 3, 3, 3);
        let l = laplacian_eps_3d(&eps, 1.0, 1.0, 1.0, BoundaryCondition::Dirichlet).unwrap();
        let v = l.spmv(&ndarray::Array1::from_elem(27, c(1.0))).unwrap();
        // Only the centre cell (k = 13) has no boundary face.
        for (k, r) in v.iter().enumerate() {
            if k == 13 {
                assert!(r.norm() < 1e-12);
            } else {
                assert!(r.re < -0.1, "row {k}: {r}");
            }
        }
    }

    #[test]
    fn lap_eps_3d_symmetric() {
        // Each face coefficient is shared by its two cells, so the operator
        // is symmetric (complex-symmetric, not Hermitian, for complex ε).
        let mut s = 11u64;
        let lossy = Array3::from_shape_fn((3, 4, 3), |_| {
            Complex::new(1.0 + 9.0 * lcg(&mut s), -lcg(&mut s))
        });
        for eps in [varied_eps(3, 4, 3, 5), lossy] {
            for bc in ALL_BC {
                let m = dense_c(&laplacian_eps_3d(&eps, 0.3, 0.7, 0.11, bc).unwrap());
                for r in 0..m.nrows() {
                    for cc in 0..m.ncols() {
                        assert_eq!(m[[r, cc]], m[[cc, r]], "{bc:?} ({r},{cc})");
                    }
                }
            }
        }
    }

    #[test]
    fn lap_eps_3d_negative_semidefinite() {
        // Real positive ε: off-diagonals are positive and the diagonal is
        // minus their sum (Neumann/periodic) or below it (Dirichlet), so by
        // Gershgorin every eigenvalue is ≤ 0. Check the dominance directly
        // and the quadratic form xᵀLx ≤ 0 on deterministic vectors.
        let eps = varied_eps(4, 3, 5, 9);
        let n = 60;
        for bc in ALL_BC {
            let l = laplacian_eps_3d(&eps, 0.2, 0.3, 0.05, bc).unwrap();
            let m = dense(&l);
            for r in 0..n {
                let off: f64 = (0..n).filter(|&cc| cc != r).map(|cc| m[[r, cc]]).sum();
                assert!((0..n).all(|cc| cc == r || m[[r, cc]] >= 0.0));
                assert!(m[[r, r]] + off <= 1e-9 * off, "{bc:?} row {r}");
            }
            let mut s = 21u64;
            for _ in 0..20 {
                let x: Vec<f64> = (0..n).map(|_| lcg(&mut s) - 0.5).collect();
                let q: f64 = (0..n)
                    .map(|r| x[r] * (0..n).map(|cc| m[[r, cc]] * x[cc]).sum::<f64>())
                    .sum();
                assert!(q <= 1e-9, "{bc:?}: xᵀLx = {q}");
            }
        }
    }

    #[test]
    fn lap_eps_3d_hand_computed_two_layer() {
        // 2×2×2, ε = 1 on page kk = 0 and ε = 4 on page kk = 1,
        // dx = 1, dy = 2, dz = 0.5 → 1/dx² = 1, 1/dy² = 0.25, 1/dz² = 4.
        // Interface face: hmean(1, 4) = 1.6, times 4 = 6.4.
        let mut eps = Array3::from_elem((2, 2, 2), c(1.0));
        for i in 0..2 {
            for j in 0..2 {
                eps[[i, j, 1]] = c(4.0);
            }
        }
        let l = laplacian_eps_3d(&eps, 1.0, 2.0, 0.5, BoundaryCondition::Dirichlet).unwrap();
        let m = dense(&l);
        // Row 0 = (i 0, j 0, kk 0): y+ → k 1 (0.25), x+ → k 2 (1),
        // z+ → k 4 (6.4); ghosts y (0.25), x (1), z (1·4 = 4).
        // Diagonal −(0.25 + 0.25 + 1 + 1 + 6.4 + 4) = −12.9.
        assert!(close(m[[0, 0]], -12.9, 1e-14));
        assert!(close(m[[0, 1]], 0.25, 1e-14));
        assert!(close(m[[0, 2]], 1.0, 1e-14));
        assert!(close(m[[0, 4]], 6.4, 1e-14));
        // Row 4 = (0, 0, 1): y 4·0.25 = 1 each, x 4 each, z− 6.4, z ghost 16.
        // Diagonal −(1 + 1 + 4 + 4 + 6.4 + 16) = −32.4.
        assert!(close(m[[4, 4]], -32.4, 1e-14));
        assert!(close(m[[4, 5]], 1.0, 1e-14));
        assert!(close(m[[4, 6]], 4.0, 1e-14));
        assert!(close(m[[4, 0]], 6.4, 1e-14));
        // Every row of a 2×2×2 Dirichlet grid has three neighbours.
        for r in 0..8 {
            assert_eq!(l.entries.iter().filter(|e| e.0 == r).count(), 4, "row {r}");
        }
        assert_eq!(
            l.ordering_hint, None,
            "the builtin stamps the hint, not the builder"
        );
    }

    #[test]
    fn lap_eps_3d_interface_face_is_harmonic_mean() {
        // A z-interface between ε = 2 and ε = 50: the face coefficient is
        // 2·2·50/52 / dz², the same on both sides of the face. The
        // arithmetic mean (26) would be ~13.5× too large.
        let (ny, nx, nz) = (3, 3, 4);
        let eps = Array3::from_shape_fn((ny, nx, nz), |(_, _, kk)| {
            c(if kk < 2 { 2.0 } else { 50.0 })
        });
        let dz = 0.1;
        let m = dense(&laplacian_eps_3d(&eps, 1.0, 1.0, dz, BoundaryCondition::Neumann).unwrap());
        let want = 2.0 * 2.0 * 50.0 / 52.0 / (dz * dz);
        let sz = nx * ny;
        for i in 0..ny {
            for j in 0..nx {
                let below = (nx + j) * ny + i; // kk = 1
                assert!(close(m[[below, below + sz]], want, 1e-14));
                assert!(close(m[[below + sz, below]], want, 1e-14));
            }
        }
    }

    #[test]
    fn lap_eps_3d_real_storage_bit_equal() {
        let mut s = 5u64;
        let r = Array3::from_shape_fn((3, 4, 2), |_| 0.25 + 7.0 * lcg(&mut s));
        let cplx = r.mapv(c);
        for bc in ALL_BC {
            let a = laplacian_eps_3d_real(&r, 0.3, 0.7, 0.11, bc).unwrap();
            let b = laplacian_eps_3d(&cplx, 0.3, 0.7, 0.11, bc).unwrap();
            assert_eq!(a.entries.len(), b.entries.len());
            for (x, y) in a.entries.iter().zip(b.entries.iter()) {
                assert_eq!((x.0, x.1), (y.0, y.1));
                assert_eq!(x.2.re.to_bits(), y.2.re.to_bits());
                assert_eq!(x.2.im.to_bits(), y.2.im.to_bits());
            }
        }
    }

    #[test]
    fn lap_eps_3d_z_invariant_matches_lap_eps_2d() {
        // ε independent of z, Neumann: on a field constant in z the z faces
        // carry no flux, so each page of L3·(v ⊗ 1) equals L2·v from the
        // 2-D builder with the same ε slice.
        let (ny, nx, nz) = (4, 5, 3);
        let mut s = 13u64;
        let eps2 = Array2::from_shape_fn((ny, nx), |_| c(1.0 + 9.0 * lcg(&mut s)));
        let eps3 = Array3::from_shape_fn((ny, nx, nz), |(i, j, _)| eps2[[i, j]]);
        let v2 = ndarray::Array1::from_shape_fn(nx * ny, |_| c(lcg(&mut s) - 0.5));
        let v3 = ndarray::Array1::from_shape_fn(nx * ny * nz, |k| v2[k % (nx * ny)]);
        let l2 = laplacian_eps_2d(&eps2, 0.3, 0.2, BoundaryCondition::Neumann).unwrap();
        let l3 = laplacian_eps_3d(&eps3, 0.3, 0.2, 0.7, BoundaryCondition::Neumann).unwrap();
        let (r2, r3) = (l2.spmv(&v2).unwrap(), l3.spmv(&v3).unwrap());
        for k in 0..nx * ny * nz {
            assert!((r3[k] - r2[k % (nx * ny)]).norm() < 1e-10, "k = {k}");
        }
    }

    #[test]
    fn lap_eps_3d_zero_eps_face_drops_entry() {
        // A face between two ε = 0 cells has coefficient 0; the entry is
        // not stored (from_sorted_entries drops exact zeros).
        let mut eps = Array3::from_elem((2, 2, 2), c(1.0));
        eps[[0, 0, 0]] = c(0.0);
        eps[[1, 0, 0]] = c(0.0);
        let l = laplacian_eps_3d(&eps, 1.0, 1.0, 1.0, BoundaryCondition::Neumann).unwrap();
        assert!(!l.entries.iter().any(|e| (e.0, e.1) == (0, 1)));
        // ε = 0 next to ε = 1: hmean = 0, so cell 0 is fully isolated.
        assert!(!l.entries.iter().any(|e| e.0 == 0));
    }

    #[test]
    fn lap_eps_3d_rejects_bad_shapes_and_spacing() {
        let ok = Array3::from_elem((2, 2, 2), c(1.0));
        for shape in [(1, 3, 3), (3, 1, 3), (3, 3, 1), (0, 2, 2)] {
            let eps = Array3::from_elem(shape, c(1.0));
            let e = laplacian_eps_3d(&eps, 1.0, 1.0, 1.0, BoundaryCondition::Dirichlet)
                .unwrap_err()
                .to_string();
            assert!(
                e.contains("laplacian_eps_3d") && e.contains(">= 2"),
                "{shape:?}: {e}"
            );
        }
        for (dx, dy, dz) in [(0.0, 1.0, 1.0), (1.0, -1.0, 1.0), (1.0, 1.0, f64::NAN)] {
            let e = laplacian_eps_3d(&ok, dx, dy, dz, BoundaryCondition::Dirichlet)
                .unwrap_err()
                .to_string();
            assert!(e.contains("positive and finite"), "{e}");
        }
    }

    // ─── Phase 4 (em_performance): direct-sorted vs legacy COO ────

    /// Reference COO emitter using the legacy `SparseMat::new` path.
    /// Drops the row-buf / sort logic and emits triples in stencil
    /// order, letting `SparseMat::new` HashMap-dedupe-then-sort. This
    /// is what the builders did before Phase 4. The Phase 4 `from_
    /// sorted_entries` path must produce a structurally equal result.
    fn legacy_lap_2d(
        nx: usize,
        ny: usize,
        dx: f64,
        dy: f64,
        bc: BoundaryCondition,
    ) -> SparseMat {
        let inv_dx2 = 1.0 / (dx * dx);
        let inv_dy2 = 1.0 / (dy * dy);
        let n = nx * ny;
        let mut entries: Vec<(usize, usize, C64)> = Vec::with_capacity(5 * n);
        for j in 0..nx {
            for i in 0..ny {
                let k = j * ny + i;
                let mut diag = -2.0 * (inv_dx2 + inv_dy2);
                if i > 0 {
                    entries.push((k, k - 1, c(inv_dy2)));
                } else {
                    match bc {
                        BoundaryCondition::Dirichlet => {}
                        BoundaryCondition::Neumann => diag += inv_dy2,
                        BoundaryCondition::Periodic => {
                            entries.push((k, k + (ny - 1), c(inv_dy2)))
                        }
                    }
                }
                if i + 1 < ny {
                    entries.push((k, k + 1, c(inv_dy2)));
                } else {
                    match bc {
                        BoundaryCondition::Dirichlet => {}
                        BoundaryCondition::Neumann => diag += inv_dy2,
                        BoundaryCondition::Periodic => {
                            entries.push((k, k - (ny - 1), c(inv_dy2)))
                        }
                    }
                }
                if j > 0 {
                    entries.push((k, k - ny, c(inv_dx2)));
                } else {
                    match bc {
                        BoundaryCondition::Dirichlet => {}
                        BoundaryCondition::Neumann => diag += inv_dx2,
                        BoundaryCondition::Periodic => {
                            entries.push((k, k + (nx - 1) * ny, c(inv_dx2)))
                        }
                    }
                }
                if j + 1 < nx {
                    entries.push((k, k + ny, c(inv_dx2)));
                } else {
                    match bc {
                        BoundaryCondition::Dirichlet => {}
                        BoundaryCondition::Neumann => diag += inv_dx2,
                        BoundaryCondition::Periodic => {
                            entries.push((k, k - (nx - 1) * ny, c(inv_dx2)))
                        }
                    }
                }
                entries.push((k, k, c(diag)));
            }
        }
        SparseMat::new(n, n, entries)
    }

    fn assert_sparse_eq(a: &SparseMat, b: &SparseMat, tol: f64) {
        assert_eq!(a.rows, b.rows, "row dim");
        assert_eq!(a.cols, b.cols, "col dim");
        assert_eq!(a.nnz(), b.nnz(), "nnz mismatch: {} vs {}", a.nnz(), b.nnz());
        for ((ar, ac, av), (br, bc_, bv)) in a.entries.iter().zip(b.entries.iter()) {
            assert_eq!(ar, br, "row idx mismatch");
            assert_eq!(ac, bc_, "col idx mismatch at row {ar}");
            assert!(
                (av - bv).norm() < tol,
                "value mismatch at ({ar}, {ac}): {av:?} vs {bv:?}"
            );
        }
    }

    #[test]
    fn lap_2d_direct_sorted_matches_legacy_coo_dirichlet() {
        for &(nx, ny) in &[(3, 3), (5, 4), (10, 7), (12, 12)] {
            let new = laplacian_2d_bc(nx, ny, 0.1, 0.2, BoundaryCondition::Dirichlet)
                .unwrap();
            let legacy = legacy_lap_2d(nx, ny, 0.1, 0.2, BoundaryCondition::Dirichlet);
            assert_sparse_eq(&new, &legacy, 1e-15);
        }
    }

    #[test]
    fn lap_2d_direct_sorted_matches_legacy_coo_neumann() {
        for &(nx, ny) in &[(3, 3), (5, 4), (10, 7)] {
            let new =
                laplacian_2d_bc(nx, ny, 0.1, 0.2, BoundaryCondition::Neumann).unwrap();
            let legacy = legacy_lap_2d(nx, ny, 0.1, 0.2, BoundaryCondition::Neumann);
            assert_sparse_eq(&new, &legacy, 1e-15);
        }
    }

    #[test]
    fn lap_2d_direct_sorted_matches_legacy_coo_periodic() {
        // Periodic at minimum sizes (ny=2, nx=2) is the duplicate-column
        // edge case where the wrap col coincides with an interior col.
        // The new path must still match legacy via from_sorted_entries'
        // consecutive-duplicate merge.
        for &(nx, ny) in &[(2, 2), (3, 3), (5, 4), (10, 7)] {
            let new = laplacian_2d_bc(nx, ny, 0.1, 0.2, BoundaryCondition::Periodic)
                .unwrap();
            let legacy = legacy_lap_2d(nx, ny, 0.1, 0.2, BoundaryCondition::Periodic);
            assert_sparse_eq(&new, &legacy, 1e-15);
        }
    }

    #[test]
    fn lap_1d_periodic_minimum_size_dedupes() {
        // n=2 periodic: at i=0 the right neighbour col is 1, the
        // periodic-left wrap col is also 1. Two emissions at the same
        // (row, col); from_sorted_entries' merge sums them.
        let l = laplacian_1d(2, 1.0, BoundaryCondition::Periodic).unwrap();
        // The ground truth: sums of two 1.0 contributions on each
        // off-diagonal. Diagonal is -2.0 (no Neumann absorption).
        // Resulting matrix:
        //   -2  2
        //    2 -2
        let m = dense(&l);
        assert!(close(m[[0, 0]], -2.0, 1e-15));
        assert!(close(m[[1, 1]], -2.0, 1e-15));
        assert!(close(m[[0, 1]], 2.0, 1e-15));
        assert!(close(m[[1, 0]], 2.0, 1e-15));
        // No spurious entries.
        assert_eq!(l.nnz(), 4);
    }
}

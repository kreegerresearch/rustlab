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
//! The `laplacian_eps_2d` builder solves the variable-coefficient case
//! `∇·(ε∇V)` via a flux-conservative discretization with harmonic-mean
//! half-cell coefficients — the standard formulation for Poisson in a
//! piecewise-uniform dielectric.

use crate::error::DspError;
use num_complex::Complex;
use rustlab_core::{CMatrix, SparseMat, C64};

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
    let n = nx * ny;
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

    // Harmonic mean of two complex values, treating the harmonic mean of
    // an arithmetic sum as the complex equivalent. Returns 0 if either
    // value is zero (consistent with "no flux through a vacuum face").
    fn hmean(a: C64, b: C64) -> C64 {
        let sum = a + b;
        if sum.norm() < 1e-300 {
            Complex::new(0.0, 0.0)
        } else {
            (Complex::new(2.0, 0.0) * a * b) / sum
        }
    }

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

use super::*;
use crate::laplacian::{laplacian_2d_bc, BoundaryCondition};
use ndarray::Array2;
use num_complex::Complex;
use rustlab_core::{SparseMat, C64};

const BCS: [BoundaryCondition; 3] = [
    BoundaryCondition::Dirichlet,
    BoundaryCondition::Neumann,
    BoundaryCondition::Periodic,
];

fn dense(s: &SparseMat) -> Array2<C64> {
    let mut m = Array2::zeros((s.rows, s.cols));
    for &(r, c, v) in &s.entries {
        m[[r, c]] += v;
    }
    m
}

fn max_abs(m: &Array2<C64>) -> f64 {
    m.iter().map(|v| v.norm()).fold(0.0, f64::max)
}

fn lcg(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

/// Stretch factors with a positive real part and a sizeable imaginary part,
/// different at every node, so no two coefficients coincide by accident.
fn random_stretch(n: usize, seed: &mut u64) -> Vec<C64> {
    (0..n)
        .map(|_| Complex::new(0.5 + lcg(seed), 3.0 * (lcg(seed) - 0.5)))
        .collect()
}

fn ones(n: usize) -> Vec<C64> {
    vec![Complex::new(1.0, 0.0); n]
}

/// Independent reference: the 1-D operator `-diag(1/s_e) Gᵀ diag(1/s_f) G`
/// built from an explicit face-gradient matrix `G` (faces × cells), with
/// the face stretch `s_f` laid out per boundary condition.
fn reference_1d(s_e: &[C64], s_h: &[C64], h: f64, bc: BoundaryCondition) -> Array2<C64> {
    let n = s_e.len();
    // (left cell, right cell, face stretch); None = ghost cell held at 0.
    let mut faces: Vec<(Option<usize>, Option<usize>, C64)> = Vec::new();
    if bc == BoundaryCondition::Dirichlet {
        faces.push((None, Some(0), s_e[0]));
    }
    for f in 0..n - 1 {
        faces.push((Some(f), Some(f + 1), s_h[f]));
    }
    match bc {
        BoundaryCondition::Dirichlet => faces.push((Some(n - 1), None, s_e[n - 1])),
        BoundaryCondition::Periodic => faces.push((Some(n - 1), Some(0), s_h[n - 1])),
        BoundaryCondition::Neumann => {}
    }
    let mut g = Array2::<C64>::zeros((faces.len(), n));
    for (f, &(l, r, _)) in faces.iter().enumerate() {
        if let Some(l) = l {
            g[[f, l]] -= Complex::new(1.0 / h, 0.0);
        }
        if let Some(r) = r {
            g[[f, r]] += Complex::new(1.0 / h, 0.0);
        }
    }
    let mut flux = g.clone();
    for (f, &(_, _, sf)) in faces.iter().enumerate() {
        for c in 0..n {
            flux[[f, c]] /= sf;
        }
    }
    let mut l1 = g.t().dot(&flux).mapv(|v| -v);
    for (r, se) in s_e.iter().enumerate() {
        for c in 0..n {
            l1[[r, c]] /= *se;
        }
    }
    l1
}

/// `kron(Lx, I_ny) + kron(I_nx, Ly)` in the column-major order `k = j*ny + i`.
fn reference_2d(
    sx_e: &[C64],
    sx_h: &[C64],
    sy_e: &[C64],
    sy_h: &[C64],
    dx: f64,
    dy: f64,
    bc: BoundaryCondition,
) -> Array2<C64> {
    let (nx, ny) = (sx_e.len(), sy_e.len());
    let lx = reference_1d(sx_e, sx_h, dx, bc);
    let ly = reference_1d(sy_e, sy_h, dy, bc);
    let n = nx * ny;
    let mut a = Array2::<C64>::zeros((n, n));
    for j in 0..nx {
        for i in 0..ny {
            let k = j * ny + i;
            for j2 in 0..nx {
                a[[k, j2 * ny + i]] += lx[[j, j2]];
            }
            for i2 in 0..ny {
                a[[k, j * ny + i2]] += ly[[i, i2]];
            }
        }
    }
    a
}

#[test]
fn unit_stretch_unit_spacing_is_laplacian_2d_bit_for_bit() {
    for (nx, ny) in [(5, 4), (2, 2), (3, 7)] {
        for bc in BCS {
            let l = laplacian_scpml_2d(&ones(nx), &ones(nx), &ones(ny), &ones(ny), 1.0, 1.0, bc)
                .unwrap();
            let r = laplacian_2d_bc(nx, ny, 1.0, 1.0, bc).unwrap();
            assert_eq!(l.rows, r.rows);
            assert_eq!(l.entries, r.entries, "{nx}x{ny} {bc:?}");
        }
    }
}

#[test]
fn unit_stretch_matches_laplacian_2d_any_spacing() {
    for bc in BCS {
        let (nx, ny, dx, dy) = (6, 5, 0.3, 0.07);
        let l = laplacian_scpml_2d(&ones(nx), &ones(nx), &ones(ny), &ones(ny), dx, dy, bc)
            .unwrap();
        let r = laplacian_2d_bc(nx, ny, dx, dy, bc).unwrap();
        let pattern = |s: &SparseMat| s.entries.iter().map(|e| (e.0, e.1)).collect::<Vec<_>>();
        assert_eq!(pattern(&l), pattern(&r), "{bc:?}");
        let scale = 1.0 / (dy * dy);
        for (a, b) in l.entries.iter().zip(&r.entries) {
            assert!(a.2.im == 0.0);
            assert!(
                (a.2 - b.2).norm() <= 2.0 * f64::EPSILON * scale,
                "{bc:?} at {:?}: {} vs {}",
                (a.0, a.1),
                a.2,
                b.2
            );
        }
    }
}

#[test]
fn matches_explicit_gradient_product_reference() {
    let mut seed = 7;
    for bc in BCS {
        let (nx, ny, dx, dy) = (7, 5, 0.4, 0.25);
        let sx_e = random_stretch(nx, &mut seed);
        let sx_h = random_stretch(nx, &mut seed);
        let sy_e = random_stretch(ny, &mut seed);
        let sy_h = random_stretch(ny, &mut seed);
        let l = laplacian_scpml_2d(&sx_e, &sx_h, &sy_e, &sy_h, dx, dy, bc).unwrap();
        let r = reference_2d(&sx_e, &sx_h, &sy_e, &sy_h, dx, dy, bc);
        let err = max_abs(&(dense(&l) - &r));
        assert!(err <= 1e-14 * max_abs(&r), "{bc:?}: max error {err:e}");
    }
}

/// The five-band assembly of `rustlab_em`'s `fdfd_tmz_pml_2d`, without the
/// `k0² ε` term: the outer neighbour is dropped from both the off-diagonal
/// and the diagonal, which is the `"neumann"` selector.
#[test]
fn neumann_matches_rustlab_em_five_band_assembly() {
    let mut seed = 99;
    let (nx, ny, dx, dy) = (6, 8, 0.02, 0.03);
    let sx_e = random_stretch(nx, &mut seed);
    let sx_h = random_stretch(nx, &mut seed);
    let sy_e = random_stretch(ny, &mut seed);
    let sy_h = random_stretch(ny, &mut seed);
    let n = nx * ny;
    let one = Complex::new(1.0, 0.0);
    let mut r = Array2::<C64>::zeros((n, n));
    for j in 0..nx {
        for i in 0..ny {
            let k = j * ny + i;
            let mut ap_x = Complex::new(0.0, 0.0);
            let mut am_x = ap_x;
            let mut ap_y = ap_x;
            let mut am_y = ap_x;
            if j + 1 < nx {
                ap_x = one / (sx_e[j] * sx_h[j] * dx * dx);
                r[[k, k + ny]] = ap_x;
            }
            if j > 0 {
                am_x = one / (sx_e[j] * sx_h[j - 1] * dx * dx);
                r[[k, k - ny]] = am_x;
            }
            if i + 1 < ny {
                ap_y = one / (sy_e[i] * sy_h[i] * dy * dy);
                r[[k, k + 1]] = ap_y;
            }
            if i > 0 {
                am_y = one / (sy_e[i] * sy_h[i - 1] * dy * dy);
                r[[k, k - 1]] = am_y;
            }
            r[[k, k]] = -ap_x - am_x - ap_y - am_y;
        }
    }
    let l = laplacian_scpml_2d(&sx_e, &sx_h, &sy_e, &sy_h, dx, dy, BoundaryCondition::Neumann)
        .unwrap();
    let err = max_abs(&(dense(&l) - &r));
    assert!(err <= 1e-14 * max_abs(&r), "max error {err:e}");
    // Five bands: one diagonal, two x bands of ny·(nx−1), two y of nx·(ny−1).
    assert_eq!(l.nnz(), 5 * n - 2 * nx - 2 * ny);
}

#[test]
fn stretch_weighted_operator_is_complex_symmetric() {
    // diag(sx_e(j)·sy_e(i)) · A is symmetric (transpose, no conjugate):
    // the 1/s_e factor is the only thing that breaks A's own symmetry.
    let mut seed = 3;
    for bc in BCS {
        let (nx, ny) = (5, 6);
        let sx_e = random_stretch(nx, &mut seed);
        let sx_h = random_stretch(nx, &mut seed);
        let sy_e = random_stretch(ny, &mut seed);
        let sy_h = random_stretch(ny, &mut seed);
        let l = laplacian_scpml_2d(&sx_e, &sx_h, &sy_e, &sy_h, 0.5, 0.2, bc).unwrap();
        let mut w = dense(&l);
        for j in 0..nx {
            for i in 0..ny {
                let k = j * ny + i;
                let d = sx_e[j] * sy_e[i];
                w.row_mut(k).mapv_inplace(|v| v * d);
            }
        }
        let asym = max_abs(&(&w - &w.t()));
        assert!(asym <= 1e-14 * max_abs(&w), "{bc:?}: asymmetry {asym:e}");
        // The sparsity pattern of A itself is symmetric.
        let a = dense(&l);
        for r in 0..a.nrows() {
            for c in 0..a.ncols() {
                assert_eq!(a[[r, c]] == Complex::new(0.0, 0.0), a[[c, r]] == Complex::new(0.0, 0.0));
            }
        }
    }
}

#[test]
fn unit_stretch_is_real_symmetric() {
    let l = laplacian_scpml_2d(&ones(4), &ones(4), &ones(3), &ones(3), 0.1, 0.2, BoundaryCondition::Dirichlet)
        .unwrap();
    let a = dense(&l);
    assert!(a.iter().all(|v| v.im == 0.0));
    assert_eq!(a, a.t());
}

#[test]
fn constants_in_null_space_for_neumann_and_periodic() {
    let mut seed = 11;
    for bc in [BoundaryCondition::Neumann, BoundaryCondition::Periodic] {
        let (nx, ny) = (6, 4);
        let l = laplacian_scpml_2d(
            &random_stretch(nx, &mut seed),
            &random_stretch(nx, &mut seed),
            &random_stretch(ny, &mut seed),
            &random_stretch(ny, &mut seed),
            0.3,
            0.3,
            bc,
        )
        .unwrap();
        let a = dense(&l);
        for r in 0..a.nrows() {
            let s: C64 = a.row(r).iter().sum();
            assert!(s.norm() <= 1e-12 * max_abs(&a), "{bc:?} row {r}: {s}");
        }
    }
}

#[test]
fn nnz_counts() {
    let (nx, ny) = (5, 4);
    let n = nx * ny;
    let mut seed = 5;
    let sx_e = random_stretch(nx, &mut seed);
    let sx_h = random_stretch(nx, &mut seed);
    let sy_e = random_stretch(ny, &mut seed);
    let sy_h = random_stretch(ny, &mut seed);
    for (bc, want) in [
        (BoundaryCondition::Dirichlet, 5 * n - 2 * nx - 2 * ny),
        (BoundaryCondition::Neumann, 5 * n - 2 * nx - 2 * ny),
        (BoundaryCondition::Periodic, 5 * n),
    ] {
        let l = laplacian_scpml_2d(&sx_e, &sx_h, &sy_e, &sy_h, 1.0, 1.0, bc).unwrap();
        assert_eq!(l.nnz(), want, "{bc:?}");
        // Rows sorted, columns sorted within each row.
        assert!(l.entries.windows(2).all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1)));
    }
}

#[test]
fn bad_sizes_and_values_error() {
    let o = |n| ones(n);
    let bc = BoundaryCondition::Dirichlet;
    let msg = |r: Result<SparseMat, crate::error::DspError>| r.unwrap_err().to_string();
    let m = msg(laplacian_scpml_2d(&o(4), &o(3), &o(5), &o(5), 1.0, 1.0, bc));
    assert!(m.contains("sx_h") && m.contains("4") && m.contains("3"), "{m}");
    let m = msg(laplacian_scpml_2d(&o(4), &o(4), &o(5), &o(6), 1.0, 1.0, bc));
    assert!(m.contains("sy_h"), "{m}");
    let m = msg(laplacian_scpml_2d(&o(1), &o(1), &o(5), &o(5), 1.0, 1.0, bc));
    assert!(m.contains("nx") && m.contains(">= 2"), "{m}");
    let m = msg(laplacian_scpml_2d(&o(4), &o(4), &o(1), &o(1), 1.0, 1.0, bc));
    assert!(m.contains("ny"), "{m}");
    let m = msg(laplacian_scpml_2d(&o(4), &o(4), &o(5), &o(5), 0.0, 1.0, bc));
    assert!(m.contains("dx"), "{m}");
    let m = msg(laplacian_scpml_2d(&o(4), &o(4), &o(5), &o(5), 1.0, f64::NAN, bc));
    assert!(m.contains("dy"), "{m}");
    let mut z = o(4);
    z[2] = Complex::new(0.0, 0.0);
    let m = msg(laplacian_scpml_2d(&o(4), &z, &o(5), &o(5), 1.0, 1.0, bc));
    assert!(m.contains("sx_h(3)") && m.contains("nonzero"), "{m}");
    let mut nan = o(5);
    nan[0] = Complex::new(1.0, f64::NAN);
    let m = msg(laplacian_scpml_2d(&o(4), &o(4), &nan, &o(5), 1.0, 1.0, bc));
    assert!(m.contains("sy_e(1)") && m.contains("finite"), "{m}");
}

// ─── scpml_stretch ───────────────────────────────────────────────

#[test]
fn stretch_without_layer_is_one() {
    let (se, sh) = scpml_stretch(10, 0, 5.0, 1e9, 3.0).unwrap();
    assert!(se.iter().chain(&sh).all(|s| *s == Complex::new(1.0, 0.0)));
    assert_eq!((se.len(), sh.len()), (10, 10));
}

#[test]
fn stretch_matches_cubic_profile() {
    let (n, npml, smax, omega) = (40, 8, 2.5, 2.0 * std::f64::consts::PI * 3e9);
    let (se, sh) = scpml_stretch(n, npml, smax, omega, 3.0).unwrap();
    let s = |sigma: f64| Complex::new(1.0, sigma / (omega * crate::fdtd::EPS0));
    // Outermost cells carry sigma_max; the interior is exactly 1.
    assert!((se[0] - s(smax)).norm() < 1e-15 * se[0].norm());
    assert!((se[n - 1] - s(smax)).norm() < 1e-15 * se[0].norm());
    for i in npml..n - npml {
        assert_eq!(se[i], Complex::new(1.0, 0.0), "cell {i}");
    }
    for i in npml..n - npml - 1 {
        assert_eq!(sh[i], Complex::new(1.0, 0.0), "face {i}");
    }
    // Cell 1 (1-based 2): x = (npml − 1)/npml.
    let x: f64 = (npml as f64 - 1.0) / npml as f64;
    assert!((se[1] - s(smax * x.powi(3))).norm() < 1e-14);
    // Face between cells 1 and 2 (1-based): x = (npml − 0.5)/npml.
    let x: f64 = (npml as f64 - 0.5) / npml as f64;
    assert!((sh[0] - s(smax * x.powi(3))).norm() < 1e-14);
    // Mirror symmetry: cell i ↔ n−1−i, face i ↔ n−2−i.
    for i in 0..n {
        assert_eq!(se[i], se[n - 1 - i]);
    }
    for i in 0..n - 1 {
        assert!((sh[i] - sh[n - 2 - i]).norm() < 1e-14 * sh[i].norm());
    }
    // Imaginary parts grow monotonically into the right layer.
    for i in n - npml..n - 1 {
        assert!(se[i + 1].im > se[i].im && sh[i].im > sh[i - 1].im);
    }
}

#[test]
fn stretch_bad_arguments_error() {
    let m = |r: Result<(Vec<C64>, Vec<C64>), crate::error::DspError>| r.unwrap_err().to_string();
    assert!(m(scpml_stretch(1, 0, 1.0, 1.0, 3.0)).contains("n"));
    assert!(m(scpml_stretch(10, 6, 1.0, 1.0, 3.0)).contains("npml"));
    assert!(m(scpml_stretch(10, 2, -1.0, 1.0, 3.0)).contains("sigma_max"));
    assert!(m(scpml_stretch(10, 2, 1.0, 0.0, 3.0)).contains("omega"));
    assert!(m(scpml_stretch(10, 2, 1.0, 1.0, 0.0)).contains("order"));
}

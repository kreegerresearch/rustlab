//! `laplacian_scpml_2d` and `scpml_stretch`: the frequency-domain
//! stretched-coordinate PML operator `(1/s_x^e)∂x(1/s_x^h)∂x +
//! (1/s_y^e)∂y(1/s_y^h)∂y` (`rustlab_em`
//! `dev/rustlab/requests/lesson-review-findings-2026-11-19.md` item 6).

use rustlab_script::{Evaluator, Value};

fn eval(src: &str) -> Evaluator {
    let src = format!("{src}\n");
    let tokens = rustlab_script::lexer::tokenize(&src).unwrap();
    let stmts = rustlab_script::parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    ev.run(&stmts)
        .unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"));
    ev
}

fn eval_err(src: &str) -> String {
    let src = format!("{src}\n");
    let tokens = rustlab_script::lexer::tokenize(&src).unwrap();
    let stmts = rustlab_script::parser::parse(tokens).unwrap();
    let mut ev = Evaluator::new();
    match ev.run(&stmts) {
        Err(e) => e.to_string(),
        Ok(()) => panic!("expected an error\n{src}"),
    }
}

fn scalar(ev: &Evaluator, name: &str) -> f64 {
    match ev.get(name) {
        Some(Value::Scalar(x)) => *x,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

/// `lessons/_shared/em.rlab` from `rustlab_em`, copied verbatim (comments
/// trimmed). The reference the builder has to reproduce.
const EM_RLAB: &str = r#"
function sigma = pml_sigma_profile(N, npml, sigma_max)
  if npml < 1
    sigma = zeros(1, N);
    return;
  end
  ii = 1:N;
  x_l = (npml - ii + 1) / npml;
  x_r = (ii - (N - npml)) / npml;
  sigma = sigma_max * (max(x_l, 0) .^ 3 + max(x_r, 0) .^ 3);
end

function sigma_h = pml_sigma_profile_h(N, npml, sigma_max)
  if npml < 1
    sigma_h = zeros(1, N);
    return;
  end
  ii = 1:N;
  x_l = (npml - ii + 0.5) / npml;
  x_r = (ii - (N - npml) + 0.5) / npml;
  sigma_h = sigma_max * (max(x_l, 0) .^ 3 + max(x_r, 0) .^ 3);
end

function s = pml_stretching_factor(sigma, omega, eps0)
  s = 1 + j * sigma / (omega * eps0);
end

function A = fdfd_tmz_pml_2d(eps_map, omega, dx, dy, npml, sigma_max)
  mu0  = 4 * pi * 1e-7;
  eps0 = 8.854187817e-12;
  c0   = 1 / sqrt(mu0 * eps0);
  k0sq = (omega / c0)^2;

  sz = size(eps_map);
  ny = sz(1);
  nx = sz(2);
  N  = nx * ny;

  sx_e = pml_stretching_factor(pml_sigma_profile  (nx, npml, sigma_max), omega, eps0);
  sx_h = pml_stretching_factor(pml_sigma_profile_h(nx, npml, sigma_max), omega, eps0);
  sy_e = pml_stretching_factor(pml_sigma_profile  (ny, npml, sigma_max), omega, eps0);
  sy_h = pml_stretching_factor(pml_sigma_profile_h(ny, npml, sigma_max), omega, eps0);

  [SXE, SYE] = meshgrid(sx_e, sy_e);
  [SXH, SYH] = meshgrid(sx_h, sy_h);

  ap_x = zeros(ny, nx);  am_x = zeros(ny, nx);
  ap_y = zeros(ny, nx);  am_y = zeros(ny, nx);
  ap_x(:, 1:nx-1) = 1 ./ (SXE(:, 1:nx-1) .* SXH(:, 1:nx-1) * dx * dx);
  am_x(:, 2:nx)   = 1 ./ (SXE(:, 2:nx)   .* SXH(:, 1:nx-1) * dx * dx);
  ap_y(1:ny-1, :) = 1 ./ (SYE(1:ny-1, :) .* SYH(1:ny-1, :) * dy * dy);
  am_y(2:ny, :)   = 1 ./ (SYE(2:ny, :)   .* SYH(1:ny-1, :) * dy * dy);
  diag_v = -ap_x - am_x - ap_y - am_y + k0sq * eps_map;

  K = reshape(1:N, ny, nx);
  K_w = K(:, 1:nx-1);  K_e = K(:, 2:nx);
  K_s = K(1:ny-1, :);  K_n = K(2:ny, :);
  v_e = ap_x(:, 1:nx-1);  v_w = am_x(:, 2:nx);
  v_n = ap_y(1:ny-1, :);  v_s = am_y(2:ny, :);
  Ii = [K(:); K_w(:); K_e(:); K_s(:); K_n(:)];
  Ji = [K(:); K_e(:); K_w(:); K_n(:); K_s(:)];
  Vv = [diag_v(:); v_e(:); v_w(:); v_n(:); v_s(:)];
  A = sparse(Ii, Ji, Vv, N, N);
end
"#;

#[test]
fn neumann_reproduces_rustlab_em_fdfd_tmz_pml_2d() {
    // A dielectric block in a 30 × 24 grid, 6-cell PML, 10 GHz.
    let src = format!(
        "{EM_RLAB}
         mu0 = 4 * pi * 1e-7; eps0 = 8.854187817e-12; c0 = 1 / sqrt(mu0 * eps0);
         nx = 30; ny = 24; dx = 1.1e-3; dy = 0.9e-3; npml = 6;
         omega = 2 * pi * 10e9; smax = 4 * eps0 * c0 / dx;
         epsr = ones(ny, nx); epsr(10:15, 12:20) = 4.0;
         A_ref = fdfd_tmz_pml_2d(epsr, omega, dx, dy, npml, smax);
         sx_e = pml_stretching_factor(pml_sigma_profile(nx, npml, smax), omega, eps0);
         sx_h = pml_stretching_factor(pml_sigma_profile_h(nx, npml, smax), omega, eps0);
         sy_e = pml_stretching_factor(pml_sigma_profile(ny, npml, smax), omega, eps0);
         sy_h = pml_stretching_factor(pml_sigma_profile_h(ny, npml, smax), omega, eps0);
         L = laplacian_scpml_2d(sx_e, sx_h, sy_e, sy_h, dx, dy, \"neumann\");
         N = nx * ny;
         A = L + spdiags((omega / c0)^2 * epsr(:), 0, N, N);
         rel = norm(full(A - A_ref), \"fro\") / norm(full(A_ref), \"fro\");
         same_nnz = nnz(A) == nnz(A_ref);
         [m, n] = size(A);"
    );
    let ev = eval(&src);
    let rel = scalar(&ev, "rel");
    assert!(rel < 1e-14, "relative Frobenius difference {rel:e}");
    assert_eq!(scalar(&ev, "same_nnz"), 1.0);
    assert_eq!(scalar(&ev, "m"), 720.0);
    assert!(matches!(ev.get("A"), Some(Value::SparseMatrix(_))));
}

#[test]
fn scpml_stretch_matches_rustlab_em_profiles() {
    // em.rlab hard-codes eps0 = 8.854187817e-12; rustlab uses the CODATA 2018
    // 1/(mu0 c0²), 1.4e-10 lower, so the imaginary parts differ at that level.
    let src = format!(
        "{EM_RLAB}
         eps0 = 8.854187817e-12; omega = 2 * pi * 2.4e9; smax = 3.7e3;
         for n = [12, 41]
           for npml = [0, 1, 5]
             se_ref = pml_stretching_factor(pml_sigma_profile(n, npml, smax), omega, eps0);
             sh_ref = pml_stretching_factor(pml_sigma_profile_h(n, npml, smax), omega, eps0);
             [se, sh] = scpml_stretch(n, npml, smax, omega);
             e1 = max(abs(se - se_ref)) / max(abs(se_ref));
             e2 = max(abs(sh - sh_ref)) / max(abs(sh_ref));
             err = max([e1, e2]);
             if n == 12 && npml == 0
               worst = err;
             else
               worst = max([worst, err]);
             end
           end
         end
         [se3, sh3] = scpml_stretch(12, 3, smax, omega);
         one_out = scpml_stretch(12, 3, smax, omega);
         one_is_se = max(abs(one_out - se3));
         [r, c] = size(se);"
    );
    let ev = eval(&src);
    let worst = scalar(&ev, "worst");
    assert!(worst < 1e-9, "worst relative difference {worst:e}");
    assert_eq!(scalar(&ev, "one_is_se"), 0.0);
    // A row vector, like the em.rlab profiles.
    assert_eq!((scalar(&ev, "r"), scalar(&ev, "c")), (1.0, 41.0));
}

#[test]
fn unit_stretch_is_laplacian_2d_exactly() {
    let ev = eval(
        "function d = unit_gap(bc)
           L1 = laplacian_scpml_2d(ones(1, 5), ones(1, 5), ones(4, 1), ones(4, 1), bc);
           d1 = max(max(abs(full(L1 - laplacian_2d(5, 4, bc)))));
           L3 = laplacian_scpml_2d(ones(1, 5), ones(1, 5), ones(1, 4), ones(1, 4), 0.25, 0.5, bc);
           d2 = max(max(abs(full(L3 - laplacian_2d(5, 4, 0.25, 0.5, bc)))));
           d = max([d1, d2]);
         end
         worst = max([unit_gap(\"dirichlet\"), unit_gap(\"neumann\"), unit_gap(\"periodic\")]);
         L5 = laplacian_scpml_2d(ones(1, 6), ones(1, 6), ones(1, 3), ones(1, 3), 0.1, 0.2);
         d_default = max(max(abs(full(L5 - laplacian_2d(6, 3, 0.1, 0.2)))));",
    );
    assert_eq!(scalar(&ev, "worst"), 0.0);
    assert_eq!(scalar(&ev, "d_default"), 0.0);
}

/// A 2-D point source at 12 cells per wavelength with a 16-cell cubic PML.
/// The field must die off across the layer, and the interior must agree
/// with the same source in a much larger PML-terminated box (reflection).
#[test]
fn point_source_is_absorbed_by_the_layer() {
    let ev = eval(
        "c0 = 299792458; eps0 = 1 / (1.25663706212e-6 * c0^2);
         f = 3e9; omega = 2 * pi * f; lam = c0 / f; h = lam / 12;
         npml = 16; smax = 4 * eps0 * c0 / h;
         function E = solve_box(n, npml, smax, omega, h, c0)
           [se, sh] = scpml_stretch(n, npml, smax, omega);
           L = laplacian_scpml_2d(se, sh, se, sh, h, h);
           N = n * n;
           A = L + (omega / c0)^2 * speye(N);
           b = zeros(N, 1);
           mid = (n + 1) / 2;
           b(ij2k(mid, mid, n)) = 1 / h^2;
           E = reshape(A \\ b, n, n);
         end
         n1 = 81; E1 = solve_box(n1, npml, smax, omega, h, c0);
         m1 = (n1 + 1) / 2;
         row = abs(E1(m1, :));
         inner = row(npml + 1);
         outer = row(1);
         atten_db = 20 * log10(inner / outer);
         n2 = 141; E2 = solve_box(n2, npml, smax, omega, h, c0);
         off = (n2 - n1) / 2;
         core = (npml + 1):(n1 - npml);
         D = E1(core, core) - E2(core + off, core + off);
         refl_db = 20 * log10(max(max(abs(D))) / max(max(abs(E2(core + off, npml + 1 + off)))));",
    );
    let atten = scalar(&ev, "atten_db");
    let refl = scalar(&ev, "refl_db");
    eprintln!("attenuation across the layer: {atten:.1} dB; interior difference: {refl:.1} dB");
    assert!(atten > 60.0, "attenuation across the PML only {atten:.1} dB");
    assert!(refl < -50.0, "interior difference vs larger box {refl:.1} dB");
}

#[test]
fn bad_arguments_error() {
    let m = eval_err("laplacian_scpml_2d(ones(1, 4), ones(1, 3), ones(1, 5), ones(1, 5))");
    assert!(m.contains("laplacian_scpml_2d") && m.contains("sx_h"), "{m}");
    let m = eval_err("laplacian_scpml_2d(ones(1, 4), ones(1, 4), ones(1, 5), ones(1, 6), 1, 1)");
    assert!(m.contains("sy_h"), "{m}");
    let m = eval_err("laplacian_scpml_2d(ones(2, 4), ones(1, 8), ones(1, 5), ones(1, 5))");
    assert!(m.contains("sx_e") && m.contains("vector"), "{m}");
    let m = eval_err("laplacian_scpml_2d(1, 1, ones(1, 5), ones(1, 5))");
    assert!(m.contains("nx"), "{m}");
    let m = eval_err("laplacian_scpml_2d(ones(1, 4), ones(1, 4), ones(1, 5), ones(1, 5), -1, 1)");
    assert!(m.contains("dx"), "{m}");
    let m = eval_err("laplacian_scpml_2d(ones(1, 4), ones(1, 4), ones(1, 5), ones(1, 5), \"pec\")");
    assert!(m.contains("dirichlet"), "{m}");
    let m = eval_err("laplacian_scpml_2d(ones(1, 4), [1, 0, 1, 1], ones(1, 5), ones(1, 5))");
    assert!(m.contains("sx_h(2)"), "{m}");
    let m = eval_err("laplacian_scpml_2d(ones(1, 4), ones(1, 4), ones(1, 5))");
    assert!(m.contains("laplacian_scpml_2d"), "{m}");
    let m = eval_err("scpml_stretch(10, 6, 1, 1)");
    assert!(m.contains("npml"), "{m}");
    let m = eval_err("scpml_stretch(10, 2.5, 1, 1)");
    assert!(m.contains("npml"), "{m}");
    let m = eval_err("scpml_stretch(10, 2, 1, -1)");
    assert!(m.contains("omega"), "{m}");
}

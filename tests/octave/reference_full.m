% Octave reference: generates ref2_*.csv files for full function coverage.
% Run with: octave --no-gui reference_full.m  (from the tests/octave directory)
pkg load signal;

fprintf('Writing Octave reference files...\n');

% ── Math functions ────────────────────────────────────────────────────────────
x = [-3.0, -1.5, 0.0, 1.5, 3.0];

csvwrite('ref2_abs.csv',   abs(x));
csvwrite('ref2_sign.csv',  sign(x));
csvwrite('ref2_floor.csv', floor(x));
csvwrite('ref2_ceil.csv',  ceil(x));
csvwrite('ref2_round.csv', round(x));
csvwrite('ref2_sqrt.csv',  sqrt([0.0, 1.0, 4.0, 9.0, 16.0]));
csvwrite('ref2_exp.csv',   exp([-1.0, 0.0, 1.0, 2.0]));
csvwrite('ref2_log.csv',   log([1.0, exp(1), exp(2), 10.0]));
csvwrite('ref2_log10.csv', log10([1.0, 10.0, 100.0, 1000.0]));
csvwrite('ref2_log2.csv',  log2([1.0, 2.0, 4.0, 8.0]));
csvwrite('ref2_mod.csv',   mod([10.0, -7.0, 5.0], [3.0, 3.0, 3.0]));

% Trig
t = [0.0, pi/6, pi/4, pi/3, pi/2, pi];
csvwrite('ref2_sin.csv',   sin(t));
csvwrite('ref2_cos.csv',   cos(t));
csvwrite('ref2_tanh.csv',  tanh([-1.0, 0.0, 0.5, 1.0]));
csvwrite('ref2_sinh.csv',  sinh([-1.0, 0.0, 0.5, 1.0]));
csvwrite('ref2_cosh.csv',  cosh([-1.0, 0.0, 0.5, 1.0]));

% Inverse trig
csvwrite('ref2_asin.csv',  asin([-1.0, -0.5, 0.0, 0.5, 1.0]));
csvwrite('ref2_acos.csv',  acos([-1.0, -0.5, 0.0, 0.5, 1.0]));
csvwrite('ref2_atan.csv',  atan([-1.0, 0.0, 1.0]));
csvwrite('ref2_atan2.csv', atan2([1.0, -1.0, 0.0, 1.0], [1.0, 1.0, -1.0, 0.0]));

% Complex
vc = [1+2j, 3-1j, -2+0j, 0+4j];
csvwrite('ref2_real.csv',  real(vc));
csvwrite('ref2_imag.csv',  imag(vc));
csvwrite('ref2_angle.csv', angle(vc));
csvwrite('ref2_conj_re.csv', real(conj(vc)));
csvwrite('ref2_conj_im.csv', imag(conj(vc)));
csvwrite('ref2_abs_complex.csv', abs(vc));

% ── Array / Stats ─────────────────────────────────────────────────────────────
v = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];

csvwrite('ref2_sum.csv',    sum(v));
csvwrite('ref2_prod.csv',   prod(v));
csvwrite('ref2_cumsum.csv', cumsum(v));
csvwrite('ref2_mean.csv',   mean(v));
csvwrite('ref2_median.csv', median(v));
csvwrite('ref2_std.csv',    std(v));   % N-1 denominator (Bessel-corrected)
csvwrite('ref2_min.csv',    min(v));
csvwrite('ref2_max.csv',    max(v));
csvwrite('ref2_sort.csv',   sort(v));
csvwrite('ref2_argmin.csv', find(v == min(v), 1));   % 1-based index
csvwrite('ref2_argmax.csv', find(v == max(v), 1));   % 1-based index

% trapz
xq = [0.0, 1.0, 2.0, 3.0, 4.0];
yq = [0.0, 1.0, 4.0, 9.0, 16.0];
csvwrite('ref2_trapz.csv', trapz(xq, yq));

% logspace
csvwrite('ref2_logspace.csv', logspace(0, 3, 7));

% ── Matrix operations ─────────────────────────────────────────────────────────
% eye
csvwrite('ref2_eye.csv', reshape(eye(3), 1, 9));

% diag (create from vector)
d = diag([1.0, 2.0, 3.0]);
csvwrite('ref2_diag_create.csv', reshape(d, 1, 9));

% diag (extract from matrix)
A = [1.0, 2.0, 3.0; 4.0, 5.0, 6.0; 7.0, 8.0, 9.0];
csvwrite('ref2_diag_extract.csv', diag(A)');

% trace
csvwrite('ref2_trace.csv', trace(A));

% reshape: column-major in Octave matches Rust column-major
v4 = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
R = reshape(v4, 2, 3);
csvwrite('ref2_reshape.csv', reshape(R, 1, 6));

% repmat
B = [1.0, 2.0; 3.0, 4.0];
RM = repmat(B, 1, 2);
csvwrite('ref2_repmat.csv', reshape(RM, 1, 8));

% transpose
T = A';
csvwrite('ref2_transpose.csv', reshape(T, 1, 9));

% horzcat
H = [A, A(:,1)];   % 3x4
csvwrite('ref2_horzcat.csv', reshape(H, 1, 12));

% vertcat
V = [B; B];   % 4x2
csvwrite('ref2_vertcat.csv', reshape(V, 1, 8));

% ── Linear algebra ────────────────────────────────────────────────────────────
A2 = [4.0, 2.0; 1.0, 3.0];

% dot product
csvwrite('ref2_dot.csv', dot([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]));

% cross product
csvwrite('ref2_cross.csv', cross([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));

% outer product
O = [1.0; 2.0; 3.0] * [4.0, 5.0];
csvwrite('ref2_outer.csv', reshape(O, 1, 6));

% kron
K = kron(eye(2), [1.0, 2.0; 3.0, 4.0]);
csvwrite('ref2_kron.csv', reshape(K, 1, 16));

% norm (vector L2)
csvwrite('ref2_norm_vec.csv', norm([3.0, 4.0]));

% norm (matrix Frobenius)
csvwrite('ref2_norm_mat.csv', norm([1.0, 2.0; 3.0, 4.0], 'fro'));

% det
csvwrite('ref2_det.csv', det(A2));

% inv
Ai = inv(A2);
csvwrite('ref2_inv.csv', reshape(Ai, 1, 4));

% linsolve: Ax=b
Alin = [2.0, 1.0; 1.0, 3.0];
b_lin = [5.0; 10.0];
xsol = Alin \ b_lin;
csvwrite('ref2_linsolve.csv', xsol');

% eig (sorted ascending for comparison)
ev = sort(eig(A2));
csvwrite('ref2_eig.csv', ev');

% svd (singular values only, sorted descending)
sv = svd([1.0, 2.0; 3.0, 4.0; 5.0, 6.0]);
csvwrite('ref2_svd.csv', sv');

% rank
csvwrite('ref2_rank.csv', rank(A2));

% roots (sorted by real part then imag for comparison)
r = roots([1.0, -3.0, 2.0]);
r_sorted = sort(real(r));
csvwrite('ref2_roots.csv', r_sorted');

% expm (matrix exponential of rotation matrix)
Aexp = [0.0, -1.0; 1.0, 0.0];
E = expm(Aexp);
csvwrite('ref2_expm.csv', reshape(E, 1, 4));

% ── DSP ───────────────────────────────────────────────────────────────────────
% filtfilt (FIR)
b_ff = [0.25, 0.5, 0.25];
x_ff = [1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0];
y_ff = filtfilt(b_ff, [1.0], x_ff);
csvwrite('ref2_filtfilt_fir.csv', y_ff);

% filtfilt (IIR - Butterworth 2nd order, LP at 0.3 normalized)
[b2, a2] = butter(2, 0.3);
x_long = sin(2*pi*0.1*(0:19)) + 0.5*sin(2*pi*0.4*(0:19));
y_iir = filtfilt(b2, a2, x_long);
csvwrite('ref2_filtfilt_iir.csv', y_iir);
csvwrite('ref2_butter_b.csv', b2);
csvwrite('ref2_butter_a.csv', a2);

% upfirdn: upsample by 2 with interpolation filter
x_up = [1.0, 2.0, 3.0, 4.0];
h_up = [0.5, 1.0, 0.5];
y_up = upfirdn(x_up, h_up, 2, 1);
csvwrite('ref2_upfirdn.csv', y_up);

% fftfreq: rustlab fftfreq(n, Fs) = k * Fs / n with wrap-around for negative freqs
% This matches numpy's fftfreq(n, d=1/Fs) = k * Fs / n
N_fft = 8;
Fs = 8.0;
freqs = zeros(1, N_fft);
half = floor(N_fft/2);
for k = 0:N_fft-1
  if k <= half - 1 + mod(N_fft,2)
    freqs(k+1) = k * Fs / N_fft;
  else
    freqs(k+1) = (k - N_fft) * Fs / N_fft;
  end
end
csvwrite('ref2_fftfreq.csv', freqs);

% ── Controls / ODE ────────────────────────────────────────────────────────────
% rk4: dx/dt = -x, x(0)=1, integrate to t=1 with 100 steps
% Compare final value: x(1) = exp(-1) ≈ 0.367879441
rk4_exact = exp(-1.0);
csvwrite('ref2_rk4_final.csv', rk4_exact);

% Also save rk4 trajectory at 11 points for comparison
t_rk4 = linspace(0, 1, 11);
x_rk4 = exp(-t_rk4);
csvwrite('ref2_rk4_traj.csv', x_rk4);

% ── Vector calculus on uniform grids ─────────────────────────────────────────
% rustlab uses 2nd-order one-sided at boundaries; Octave's gradient()
% uses 1st-order. Compare only interior cells where both schemes
% return the analytic 2x value exactly.
[Xv, Yv] = meshgrid(linspace(-1.0, 1.0, 11), linspace(-1.0, 1.0, 11));
Fv = Xv .^ 2 + Yv .^ 2;
[Fxv, Fyv] = gradient(Fv, 0.2, 0.2);
csvwrite('ref2_gradient_x_centre.csv', Fxv(6, 6));     % ≈ 0
csvwrite('ref2_gradient_x_interior.csv', Fxv(7, 7));   % 2*0.2 = 0.4
csvwrite('ref2_gradient_y_interior.csv', Fyv(7, 7));   % 2*0.2 = 0.4

% Divergence of (X, Y) is constant 2 everywhere (analytic).
[gxx, gxy] = gradient(Xv, 0.2, 0.2);
[gyx, gyy] = gradient(Yv, 0.2, 0.2);
Dv = gxx + gyy;
csvwrite('ref2_divergence_centre.csv', Dv(6, 6));      % 2
csvwrite('ref2_divergence_corner.csv', Dv(1, 1));      % 2 (boundary one-sided exact for linear)

% Curl of (-Y, X) is constant 2.
NYv = -Yv;
[~, ny_dy] = gradient(NYv, 0.2, 0.2);
[xv_dx, ~] = gradient(Xv, 0.2, 0.2);
% curl_z = dFy/dx - dFx/dy
Cv = xv_dx - ny_dy;
csvwrite('ref2_curl_centre.csv', Cv(6, 6));            % 2

% ── Sparse Laplacian builders ────────────────────────────────────────────────
% Build the 4x3 (nx=4, ny=3) Dirichlet Laplacian matching laplacian_2d.
% rustlab convention: column-major flat index k = (j-1)*ny + i.
% Stencil: -2*(1/dx² + 1/dy²) on diagonal, +1/dx² and +1/dy² off-diagonal.
function L = build_lap2d(nx, ny, dx, dy, bc)
    inv_dx2 = 1 / (dx*dx);
    inv_dy2 = 1 / (dy*dy);
    n = nx * ny;
    L = zeros(n, n);
    for j = 1:nx
        for i = 1:ny
            k = (j-1)*ny + i;
            diag_val = -2 * (inv_dx2 + inv_dy2);
            % y-direction (i)
            if i > 1
                L(k, k-1) = inv_dy2;
            else
                if strcmp(bc, 'neumann'); diag_val = diag_val + inv_dy2; endif
                if strcmp(bc, 'periodic'); L(k, k+(ny-1)) = inv_dy2; endif
            endif
            if i < ny
                L(k, k+1) = inv_dy2;
            else
                if strcmp(bc, 'neumann'); diag_val = diag_val + inv_dy2; endif
                if strcmp(bc, 'periodic'); L(k, k-(ny-1)) = inv_dy2; endif
            endif
            % x-direction (j) — stride ny
            if j > 1
                L(k, k-ny) = inv_dx2;
            else
                if strcmp(bc, 'neumann'); diag_val = diag_val + inv_dx2; endif
                if strcmp(bc, 'periodic'); L(k, k+(nx-1)*ny) = inv_dx2; endif
            endif
            if j < nx
                L(k, k+ny) = inv_dx2;
            else
                if strcmp(bc, 'neumann'); diag_val = diag_val + inv_dx2; endif
                if strcmp(bc, 'periodic'); L(k, k-(nx-1)*ny) = inv_dx2; endif
            endif
            L(k, k) = diag_val;
        endfor
    endfor
endfunction

L2d = build_lap2d(4, 3, 1.0, 1.0, 'dirichlet');
csvwrite('ref2_laplacian_2d_dirichlet.csv', reshape(L2d, 1, 144));
L2d_neu = build_lap2d(4, 3, 1.0, 1.0, 'neumann');
csvwrite('ref2_laplacian_2d_neumann.csv', reshape(L2d_neu, 1, 144));
L2d_per = build_lap2d(4, 3, 1.0, 1.0, 'periodic');
csvwrite('ref2_laplacian_2d_periodic.csv', reshape(L2d_per, 1, 144));

% laplacian_1d (Dirichlet, n=5, dx=1)
L1d = -2 * eye(5);
for k = 1:4
    L1d(k, k+1) = 1;
    L1d(k+1, k) = 1;
endfor
csvwrite('ref2_laplacian_1d.csv', reshape(L1d, 1, 25));

% laplacian_eps_2d with eps≡1 reduces to laplacian_2d Dirichlet (3x4 = 12).
% Note: rustlab's eps_map shape is (ny, nx) = (3, 4) → matches our 4x3 grid.
Le1 = build_lap2d(4, 3, 1.0, 1.0, 'dirichlet');
csvwrite('ref2_laplacian_eps_unit.csv', reshape(Le1, 1, 144));

% ── Sparse direct solve ──────────────────────────────────────────────────────
% Octave: build the same -laplacian matrix and solve.
A_pois = -build_lap2d(5, 4, 1.0, 1.0, 'dirichlet');
v_exact = sin(linspace(0.1, 1.9, 20));
rhs = A_pois * v_exact';
v_sol = A_pois \ rhs;
csvwrite('ref2_spsolve_v.csv', v_sol');

% ── Sparse partial eigensolver ───────────────────────────────────────────────
% Octave's eigs on -laplacian_2d(4, 5).
L_eig = -build_lap2d(4, 5, 1.0, 1.0, 'dirichlet');
% Octave eigs needs sparse input.
L_eig_sp = sparse(L_eig);
% smallest-magnitude eigenvalue
opts.tol = 1e-12;
opts.maxit = 500;
sm_val = eigs(L_eig_sp, 1, 'sm', opts);
csvwrite('ref2_eigs_smallest.csv', real(sm_val));

% ── Geometry masks ────────────────────────────────────────────────────────────
[Xd, Yd] = meshgrid(linspace(-1.5, 1.5, 100), linspace(-1.5, 1.5, 100));
D_mask = double(Xd.^2 + Yd.^2 <= 1.0);
disk_area_oct = sum(sum(D_mask)) * (3.0 / 99.0)^2;
csvwrite('ref2_disk_area.csv', disk_area_oct);

[Xr, Yr] = meshgrid(linspace(0, 1, 11), linspace(0, 1, 11));
R_mask = double(Xr >= 0 & Xr <= 1 & Yr >= 0 & Yr <= 1);
csvwrite('ref2_rect_count.csv', sum(sum(R_mask)));

% Polygon over the unit square should equal rect_mask exactly — diff is 0.
csvwrite('ref2_polygon_vs_rect_diff.csv', 0);

% ── Real-typed elem-ops (em_requests §4 Option A) ─────────────────────────────
% Both inputs are real → result has zero imag in rustlab.
csvwrite('ref2_elemdiv_imag.csv', 0);
csvwrite('ref2_elemmul_imag.csv', 0);

% ── Edge cases: math ──────────────────────────────────────────────────────────
csvwrite('ref2_mod_negative.csv', mod(-7, 3));
csvwrite('ref2_mod_zero.csv',     mod(0, 5));

csvwrite('ref2_single_sin.csv',   sin([0.5]));
csvwrite('ref2_single_sum.csv',   sum([42]));
csvwrite('ref2_single_mean.csv',  mean([3.14]));

csvwrite('ref2_sort_ties.csv',    sort([3 1 3 1 2]));
csvwrite('ref2_sort_descend.csv', sort([3 1 4 1 5 9 2 6], 'descend'));
csvwrite('ref2_find_dense_vec.csv', find([0 5 0 -3 0 7]));
csvwrite('ref2_find_dense_mat.csv', find([0 2; 3 0]));

% Implicit expansion (broadcasting) — octave R2016b semantics.
M_bcast = [1 2 3; 4 5 6];
csvwrite('ref2_bcast_mat_plus_row.csv', M_bcast + [10 20 30]);
csvwrite('ref2_bcast_mat_plus_col.csv', M_bcast + [100; 200]);
csvwrite('ref2_bcast_col_plus_row.csv', [1; 2] + [10 20 30]);

csvwrite('ref2_log_dynamic.csv',  log10([1e-10 1e10]));

csvwrite('ref2_atan2_quadrants.csv', atan2([1 1 -1 -1], [1 -1 -1 1]));

csvwrite('ref2_linspace_single.csv', linspace(3, 7, 1));

% ── Edge cases: linear algebra ────────────────────────────────────────────────
csvwrite('ref2_det_diag2.csv', det(diag([3 7])));
csvwrite('ref2_inv_diag2.csv', reshape(inv(diag([3 7])), 1, 4));

csvwrite('ref2_eig_eye3.csv', sort(eig(eye(3)))');

csvwrite('ref2_norm_zero.csv', norm([0 0 0]));

% ── Aggressive edge cases ─────────────────────────────────────────────────────
csvwrite('ref2_floor_neg_half.csv', floor([-0.5 -1.5 -2.5 0.5 1.5 2.5]));
csvwrite('ref2_ceil_neg_half.csv',  ceil([-0.5 -1.5 -2.5 0.5 1.5 2.5]));
csvwrite('ref2_round_half.csv',     round([-0.5 -1.5 -2.5 0.5 1.5 2.5]));

csvwrite('ref2_sqrt_zero.csv',      sqrt([0.0]));
csvwrite('ref2_sqrt_tiny.csv',      sqrt([1e-300]));

csvwrite('ref2_log_one.csv',        log([1.0, exp(1.0)]));

csvwrite('ref2_angle_real_pos.csv', angle([5.0]));
csvwrite('ref2_angle_real_neg.csv', angle([-5.0]));
csvwrite('ref2_angle_imag.csv',     angle([3i]));
csvwrite('ref2_atan2_zero.csv',     atan2(0, 0));

csvwrite('ref2_std_constant.csv',   std([5 5 5 5]));
csvwrite('ref2_median_two.csv',     median([3 1]));

csvwrite('ref2_cumsum_single.csv',  cumsum([42]));

csvwrite('ref2_trapz_two.csv',      trapz([0 1], [0 1]));

csvwrite('ref2_logspace_two.csv',   logspace(0, 2, 2));

csvwrite('ref2_reshape_to_row.csv', reshape([1 2 3 4 5 6], 1, 6));

csvwrite('ref2_neg_complex_re.csv', real(-[1+2j, 3-1j]));
csvwrite('ref2_neg_complex_im.csv', imag(-[1+2j, 3-1j]));

csvwrite('ref2_fft_zeros_re.csv',   real(fft(zeros(8, 1)))');
csvwrite('ref2_fft_zeros_im.csv',   imag(fft(zeros(8, 1)))');

delta = [1 0 0 0 0 0 0 0];
F_delta = fft(delta);
csvwrite('ref2_fft_delta_mag.csv',  abs(F_delta));

sig = [1 2 3 4 5 6 7 8];
csvwrite('ref2_fft_roundtrip.csv',  real(ifft(fft(sig))));

I3 = eye(3);
b3 = [7; 11; 13];
csvwrite('ref2_linsolve_identity.csv', (I3 \ b3)');

zc = [1+2j, 3+4j];
csvwrite('ref2_ctranspose_re.csv',  real(zc'));    % Octave ' is conjugate transpose
csvwrite('ref2_ctranspose_im.csv',  imag(zc'));
csvwrite('ref2_transpose_im.csv',   imag(zc.'));   % .' is plain transpose

csvwrite('ref2_sort_default.csv',   sort([3 1 2]));

csvwrite('ref2_argmin_pos.csv',     find([5 1 3] == min([5 1 3]), 1));
csvwrite('ref2_argmax_pos.csv',     find([5 9 3] == max([5 9 3]), 1));

csvwrite('ref2_colon_step.csv',     1:2:9);
csvwrite('ref2_colon_decr.csv',     5:-1:1);

csvwrite('ref2_scalar_add.csv',     5 + [1 2 3]);
csvwrite('ref2_scalar_div.csv',     12 ./ [2 3 4]);

fprintf('All reference files written.\n');

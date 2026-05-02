# Rustlab full test script — produces out2_*.csv files matching reference_full.m

# ── Math functions ────────────────────────────────────────────────────────────
x = [-3.0, -1.5, 0.0, 1.5, 3.0]

save("out2_abs.csv",   abs(x))
save("out2_sign.csv",  sign(x))
save("out2_floor.csv", floor(x))
save("out2_ceil.csv",  ceil(x))
save("out2_round.csv", round(x))
save("out2_sqrt.csv",  sqrt([0.0, 1.0, 4.0, 9.0, 16.0]))
save("out2_exp.csv",   exp([-1.0, 0.0, 1.0, 2.0]))
save("out2_log.csv",   log([1.0, exp(1.0), exp(2.0), 10.0]))
save("out2_log10.csv", log10([1.0, 10.0, 100.0, 1000.0]))
save("out2_log2.csv",  log2([1.0, 2.0, 4.0, 8.0]))

# mod: test scalar cases individually
m1 = mod(10.0, 3.0)
m2 = mod(-7.0, 3.0)
m3 = mod(5.0, 3.0)
save("out2_mod.csv", [m1, m2, m3])

# Trig
t = [0.0, 0.5235987755982988, 0.7853981633974483, 1.0471975511965976, 1.5707963267948966, 3.141592653589793]
save("out2_sin.csv",   sin(t))
save("out2_cos.csv",   cos(t))
save("out2_tanh.csv",  tanh([-1.0, 0.0, 0.5, 1.0]))
save("out2_sinh.csv",  sinh([-1.0, 0.0, 0.5, 1.0]))
save("out2_cosh.csv",  cosh([-1.0, 0.0, 0.5, 1.0]))

# Inverse trig
save("out2_asin.csv",  asin([-1.0, -0.5, 0.0, 0.5, 1.0]))
save("out2_acos.csv",  acos([-1.0, -0.5, 0.0, 0.5, 1.0]))
save("out2_atan.csv",  atan([-1.0, 0.0, 1.0]))
y_a2 = [1.0, -1.0, 0.0, 1.0]
x_a2 = [1.0, 1.0, -1.0, 0.0]
save("out2_atan2.csv", atan2(y_a2, x_a2))

# Complex
vc = [1.0+j*2.0, 3.0-j*1.0, -2.0+j*0.0, 0.0+j*4.0]
save("out2_real.csv",      real(vc))
save("out2_imag.csv",      imag(vc))
save("out2_angle.csv",     angle(vc))
save("out2_conj_re.csv",   real(conj(vc)))
save("out2_conj_im.csv",   imag(conj(vc)))
save("out2_abs_complex.csv", abs(vc))

# ── Array / Stats ─────────────────────────────────────────────────────────────
v = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0]

save("out2_sum.csv",    sum(v))
save("out2_prod.csv",   prod(v))
save("out2_cumsum.csv", cumsum(v))
save("out2_mean.csv",   mean(v))
save("out2_median.csv", median(v))
save("out2_std.csv",    std(v))
save("out2_min.csv",    min(v))
save("out2_max.csv",    max(v))
save("out2_sort.csv",   sort(v))
save("out2_argmin.csv", argmin(v))
save("out2_argmax.csv", argmax(v))

# trapz
xq = [0.0, 1.0, 2.0, 3.0, 4.0]
yq = [0.0, 1.0, 4.0, 9.0, 16.0]
save("out2_trapz.csv", trapz(xq, yq))

# logspace
save("out2_logspace.csv", logspace(0.0, 3.0, 7))

# ── Matrix operations ─────────────────────────────────────────────────────────
# eye
E3 = eye(3)
save("out2_eye.csv", reshape(E3, 1, 9))

# diag (create from vector)
d = diag([1.0, 2.0, 3.0])
save("out2_diag_create.csv", reshape(d, 1, 9))

# diag (extract from matrix)
A = [1.0, 2.0, 3.0; 4.0, 5.0, 6.0; 7.0, 8.0, 9.0]
save("out2_diag_extract.csv", diag(A))

# trace
save("out2_trace.csv", trace(A))

# reshape
v4 = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
R = reshape(v4, 2, 3)
save("out2_reshape.csv", reshape(R, 1, 6))

# repmat
B = [1.0, 2.0; 3.0, 4.0]
RM = repmat(B, 1, 2)
save("out2_repmat.csv", reshape(RM, 1, 8))

# transpose
T = A'
save("out2_transpose.csv", reshape(T, 1, 9))

# horzcat: A is 3x3, append column [1;4;7]
col1 = [1.0; 4.0; 7.0]
H = horzcat(A, col1)
save("out2_horzcat.csv", reshape(H, 1, 12))

# vertcat
V = vertcat(B, B)
save("out2_vertcat.csv", reshape(V, 1, 8))

# ── Linear algebra ────────────────────────────────────────────────────────────
A2 = [4.0, 2.0; 1.0, 3.0]

# dot product
save("out2_dot.csv", dot([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]))

# cross product
save("out2_cross.csv", cross([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]))

# outer product
o1 = [1.0, 2.0, 3.0]
o2 = [4.0, 5.0]
O = outer(o1, o2)
save("out2_outer.csv", reshape(O, 1, 6))

# kron
K = kron(eye(2), [1.0, 2.0; 3.0, 4.0])
save("out2_kron.csv", reshape(K, 1, 16))

# norm (vector L2)
save("out2_norm_vec.csv", norm([3.0, 4.0]))

# norm (matrix Frobenius)
save("out2_norm_mat.csv", norm([1.0, 2.0; 3.0, 4.0]))

# det
save("out2_det.csv", det(A2))

# inv
Ai = inv(A2)
save("out2_inv.csv", reshape(Ai, 1, 4))

# linsolve: Ax=b
Alin = [2.0, 1.0; 1.0, 3.0]
b_lin = [5.0; 10.0]
xsol = linsolve(Alin, b_lin)
save("out2_linsolve.csv", xsol)

# eig (sorted ascending)
ev = eig(A2)
ev_sorted = sort(ev)
save("out2_eig.csv", ev_sorted)

# svd (singular values only)
A_svd = [1.0, 2.0; 3.0, 4.0; 5.0, 6.0]
[U_svd, S_svd, V_svd] = svd(A_svd)
sv = sort(S_svd)
# sort gives ascending, we need descending to match octave svd order
sv_desc = [sv(length(sv)), sv(length(sv)-1)]
save("out2_svd.csv", sv_desc)

# rank
save("out2_rank.csv", rank(A2))

# roots (sorted by real part for comparison)
r = roots([1.0, -3.0, 2.0])
r_sorted = sort(r)
save("out2_roots.csv", r_sorted)

# expm
Aexp = [0.0, -1.0; 1.0, 0.0]
E = expm(Aexp)
save("out2_expm.csv", reshape(E, 1, 4))

# ── DSP ───────────────────────────────────────────────────────────────────────
# filtfilt (FIR)
b_ff = [0.25, 0.5, 0.25]
x_ff = [1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0]
y_ff = filtfilt(b_ff, [1.0], x_ff)
save("out2_filtfilt_fir.csv", y_ff)

# upfirdn: upsample by 2
x_up = [1.0, 2.0, 3.0, 4.0]
h_up = [0.5, 1.0, 0.5]
y_up = upfirdn(x_up, h_up, 2, 1)
save("out2_upfirdn.csv", y_up)

# fftfreq: rustlab takes sample_rate (Fs), not sample spacing
f_freq = fftfreq(8, 8.0)
save("out2_fftfreq.csv", f_freq)

# ── Controls / ODE ────────────────────────────────────────────────────────────
# rk4: dx/dt = -x, x(0)=1, integrate to t=1 with 11 steps
f_decay = @(x, t) -x
t_rk4 = linspace(0.0, 1.0, 11)
x_rk4 = rk4(f_decay, 1.0, t_rk4)
save("out2_rk4_traj.csv", x_rk4)
save("out2_rk4_final.csv", x_rk4(11))

# ── Vector calculus on uniform grids ─────────────────────────────────────────
# Test on a smooth analytic field: F(x, y) = x² + y². Gradient = (2x, 2y),
# divergence of (x, y) = 2, curl of (-y, x) = 2 everywhere.
# NOTE: at boundary cells, rustlab uses 2nd-order one-sided differences
# (exact for quadratic) while Octave's gradient() uses 1st-order one-sided.
# We compare *interior* points only; rustlab is intentionally more accurate
# at boundaries than Octave-default-gradient.
[Xv, Yv] = meshgrid(linspace(-1.0, 1.0, 11), linspace(-1.0, 1.0, 11))
Fv = Xv .^ 2 + Yv .^ 2
[Fxv, Fyv] = gradient(Fv, 0.2, 0.2)
save("out2_gradient_x_centre.csv", Fxv(6, 6))   % interior: ≈ 0 (matches)
save("out2_gradient_x_interior.csv", Fxv(7, 7)) % interior: 2*0.2 = 0.4 (matches)
save("out2_gradient_y_interior.csv", Fyv(7, 7)) % interior: 2*0.2 = 0.4 (matches)

# Divergence of radial field (x, y) is constant 2
Dv = divergence(Xv, Yv, 0.2, 0.2)
save("out2_divergence_centre.csv", Dv(6, 6))    % should be 2
save("out2_divergence_corner.csv", Dv(1, 1))    % should be 2 too (boundary stencil exact for linear)

# Curl of solid-body rotation (-y, x) is constant 2
Cv = curl(-1 * Yv, Xv, 0.2, 0.2)
save("out2_curl_centre.csv", Cv(6, 6))          % should be 2

# ── Sparse Laplacian builders ────────────────────────────────────────────────
# laplacian_2d on 4x3 grid with Dirichlet BC. Densify and save.
L2d = full(laplacian_2d(4, 3, 1.0, 1.0))
save("out2_laplacian_2d_dirichlet.csv", reshape(L2d, 1, 144))

# laplacian_2d with Neumann BC — corner cell diag becomes -2
L2d_neu = full(laplacian_2d(4, 3, 1.0, 1.0, "neumann"))
save("out2_laplacian_2d_neumann.csv", reshape(L2d_neu, 1, 144))

# laplacian_2d with periodic BC — wrap-around entries
L2d_per = full(laplacian_2d(4, 3, 1.0, 1.0, "periodic"))
save("out2_laplacian_2d_periodic.csv", reshape(L2d_per, 1, 144))

# laplacian_1d
L1d = full(laplacian_1d(5, 1.0, "dirichlet"))
save("out2_laplacian_1d.csv", reshape(L1d, 1, 25))

# laplacian_eps_2d with eps≡1 should equal laplacian_2d
eps_unit = ones(3, 4)
Le1 = full(laplacian_eps_2d(eps_unit, 1.0, 1.0))
save("out2_laplacian_eps_unit.csv", reshape(Le1, 1, 144))

# ── Sparse direct solve ──────────────────────────────────────────────────────
# Build a SPD Poisson system and check the solve.
A_pois = -1 * laplacian_2d(5, 4)
# Build a known v_exact, compute rhs = A * v_exact, then verify spsolve recovers.
v_exact = sin(linspace(0.1, 1.9, 20))
rhs = full(A_pois) * transpose(v_exact)
v_sol = spsolve(A_pois, transpose(rhs))
save("out2_spsolve_v.csv", v_sol)

# ── Sparse partial eigensolver ───────────────────────────────────────────────
# Smallest eigenvalue of -laplacian_2d on a 4x5 grid.
L_eig = -1 * laplacian_2d(4, 5)
[V_eig, D_eig] = eigs(L_eig, 1, "sm")
save("out2_eigs_smallest.csv", real(D_eig(1)))

# ── Geometry masks ────────────────────────────────────────────────────────────
# Disk mask area should approximate π * r² for fine grids.
[Xd, Yd] = meshgrid(linspace(-1.5, 1.5, 100), linspace(-1.5, 1.5, 100))
D_mask = disk_mask(Xd, Yd, 0.0, 0.0, 1.0)
disk_area = sum(sum(D_mask)) * (3.0 / 99.0) ^ 2
save("out2_disk_area.csv", disk_area)

# Rectangle mask — exact count
[Xr, Yr] = meshgrid(linspace(0.0, 1.0, 11), linspace(0.0, 1.0, 11))
R_mask = rect_mask(Xr, Yr, 0.0, 0.0, 1.0, 1.0)
save("out2_rect_count.csv", sum(sum(R_mask)))

# Polygon mask: square should equal rect_mask. Use OFF-GRID sampling
# so that no test cell sits exactly on a polygon edge — PNPOLY's tie-
# break behaviour is implementation-defined on edges, which would
# otherwise produce spurious mismatches. The unit-test in
# rustlab-dsp/src/rasterize.rs uses the same trick.
[Xr2, Yr2] = meshgrid(linspace(-0.25, 1.25, 60) + 0.0125, linspace(-0.25, 1.25, 60) + 0.0125)
R2 = rect_mask(Xr2, Yr2, 0.0, 0.0, 1.0, 1.0)
P2 = polygon_mask(Xr2, Yr2, [0,0; 1,0; 1,1; 0,1])
save("out2_polygon_vs_rect_diff.csv", sum(sum(abs(P2 - R2))))

# ── Real-typed elem-ops (em_requests §4 Option A) ─────────────────────────────
u_real = [1.0, 2.0, 3.0, 4.0]
v_real = [5.0, 6.0, 7.0, 8.0]
w_div = u_real ./ v_real
save("out2_elemdiv_imag.csv", max(abs(imag(w_div))))   % should be exactly 0

w_mul = u_real .* v_real
save("out2_elemmul_imag.csv", max(abs(imag(w_mul))))   % should be exactly 0

# ── Edge cases: math ──────────────────────────────────────────────────────────
# mod with negative dividend (rustlab uses Octave-compatible modulo)
save("out2_mod_negative.csv", mod(-7.0, 3.0))      % should be 2 (Octave: -7 mod 3 = 2)
save("out2_mod_zero.csv",     mod(0.0, 5.0))       % should be 0

# Single-element operations
save("out2_single_sin.csv",   sin([0.5]))
save("out2_single_sum.csv",   sum([42.0]))
save("out2_single_mean.csv",  mean([3.14]))

# Sort with ties (should be stable for equal values)
save("out2_sort_ties.csv",    sort([3.0, 1.0, 3.0, 1.0, 2.0]))

# Sort descending — string-flag form
save("out2_sort_descend.csv", sort([3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0], "descend"))

# find() on a dense vector — 1-based positions of nonzeros
save("out2_find_dense_vec.csv",   find([0.0, 5.0, 0.0, -3.0, 0.0, 7.0]))
# find() on a dense matrix — column-major linear indices (octave convention)
save("out2_find_dense_mat.csv",   find([0.0, 2.0; 3.0, 0.0]))

# Implicit expansion (broadcasting): row vector down rows, column vector across cols.
M_bcast = [1.0, 2.0, 3.0; 4.0, 5.0, 6.0]
save("out2_bcast_mat_plus_row.csv", M_bcast + [10.0, 20.0, 30.0])
save("out2_bcast_mat_plus_col.csv", M_bcast + [100.0; 200.0])
save("out2_bcast_col_plus_row.csv", [1.0; 2.0] + [10.0, 20.0, 30.0])

# Matrix axis reductions — default is column-wise (dim 1).
M_red = [1.0, 2.0, 3.0; 4.0, 5.0, 6.0]
save("out2_sum_matrix_default.csv",  sum(M_red))
save("out2_sum_matrix_dim2.csv",     sum(M_red, 2))
save("out2_mean_matrix_default.csv", mean(M_red))
save("out2_mean_matrix_dim2.csv",    mean(M_red, 2))
save("out2_prod_matrix_default.csv", prod(M_red))
save("out2_max_matrix_default.csv",  max(M_red))
save("out2_min_matrix_default.csv",  min(M_red))

# Large dynamic range
save("out2_log_dynamic.csv",  log10([1e-10, 1e10]))

# atan2 quadrant boundaries
save("out2_atan2_quadrants.csv", atan2([1.0, 1.0, -1.0, -1.0], [1.0, -1.0, -1.0, 1.0]))

# linspace with single point (degenerate)
# rustlab: linspace(a, b, 1) returns [a]
save("out2_linspace_single.csv", linspace(3.0, 7.0, 1))

# ── Edge cases: linear algebra ────────────────────────────────────────────────
# Diagonal 2x2 — det and inv have closed-form answers (sanity)
D2 = [3.0, 0.0; 0.0, 7.0]
save("out2_det_diag2.csv", det(D2))
save("out2_inv_diag2.csv", reshape(inv(D2), 1, 4))

# eig of identity
save("out2_eig_eye3.csv", sort(eig(eye(3))))

# Norm of zero vector
save("out2_norm_zero.csv", norm([0.0, 0.0, 0.0]))

# ── Aggressive edge cases ─────────────────────────────────────────────────────
# floor/ceil/round on negatives, halves, small-magnitude
save("out2_floor_neg_half.csv", floor([-0.5, -1.5, -2.5, 0.5, 1.5, 2.5]))
save("out2_ceil_neg_half.csv",  ceil([-0.5, -1.5, -2.5, 0.5, 1.5, 2.5]))
save("out2_round_half.csv",     round([-0.5, -1.5, -2.5, 0.5, 1.5, 2.5]))   % banker's vs half-to-even

# sqrt at boundaries
save("out2_sqrt_zero.csv",      sqrt([0.0]))
save("out2_sqrt_tiny.csv",      sqrt([1e-300]))

# log near singularity
save("out2_log_one.csv",        log([1.0, exp(1.0)]))    % log(1)=0, log(e)=1

# angle / atan2 of pure-imaginary and pure-real
save("out2_angle_real_pos.csv", angle([5.0]))            % 0
save("out2_angle_real_neg.csv", angle([-5.0]))           % π
save("out2_angle_imag.csv",     angle([j*3.0]))          % π/2
save("out2_atan2_zero.csv",     atan2(0.0, 0.0))         % 0 by convention

# stats on degenerate inputs
save("out2_std_constant.csv",   std([5.0, 5.0, 5.0, 5.0]))    % 0 exactly
save("out2_median_two.csv",     median([3.0, 1.0]))           % 2 (mean of two middles)

# cumsum with single element
save("out2_cumsum_single.csv",  cumsum([42.0]))

# trapz with 2 points (single trapezoid)
save("out2_trapz_two.csv",      trapz([0.0, 1.0], [0.0, 1.0]))   % 0.5

# logspace edge cases
save("out2_logspace_two.csv",   logspace(0.0, 2.0, 2))         % [1, 100]

# matrix ops on 1x1 (use a 1x1 via `[[x]]`)
M11 = [3.5; 0.0]
M11 = [3.5]                                                      % row vector — det/inv don't accept
% Skip — handled via 2x2 above.

# Reshape boundary
save("out2_reshape_to_row.csv", reshape([1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 1, 6))

# Negate a complex vector via element op
neg_test = -1 * [1.0+j*2.0, 3.0-j*1.0]
save("out2_neg_complex_re.csv", real(neg_test))
save("out2_neg_complex_im.csv", imag(neg_test))

# fft of all-zeros
save("out2_fft_zeros_re.csv",   real(fft(zeros(8, 1))))
save("out2_fft_zeros_im.csv",   imag(fft(zeros(8, 1))))

# fft of a delta — should have constant magnitude
delta = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
F_delta = fft(delta)
save("out2_fft_delta_mag.csv",  abs(F_delta))             % all 1s

# inverse fft round-trip
sig = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
roundtrip = real(ifft(fft(sig)))
save("out2_fft_roundtrip.csv",  roundtrip)                % equals sig

# Matrix power-edge: solve I x = b returns b
I3 = eye(3)
b3 = [7.0; 11.0; 13.0]
save("out2_linsolve_identity.csv", linsolve(I3, b3))

# Hermitian conjugate-transpose vs plain transpose
zc = [1.0+j*2.0, 3.0+j*4.0]
save("out2_ctranspose_re.csv",  real(zc'))                % conj transpose: [1, 3]
save("out2_ctranspose_im.csv",  imag(zc'))                % conj transpose: [-2, -4]
save("out2_transpose_im.csv",   imag(transpose(zc)))      % plain: [+2, +4]

# Sort descending — rustlab default is ascending
save("out2_sort_default.csv",   sort([3.0, 1.0, 2.0]))    % [1, 2, 3]

# argmin/argmax 1-based
save("out2_argmin_pos.csv",     argmin([5.0, 1.0, 3.0]))  % 2
save("out2_argmax_pos.csv",     argmax([5.0, 9.0, 3.0]))  % 2

# Test `:` operator
save("out2_colon_step.csv",     1:2:9)                    % [1, 3, 5, 7, 9]
save("out2_colon_decr.csv",     5:-1:1)                   % [5, 4, 3, 2, 1]

# Vector/scalar broadcasting
save("out2_scalar_add.csv",     5.0 + [1.0, 2.0, 3.0])
save("out2_scalar_div.csv",     12.0 ./ [2.0, 3.0, 4.0])

print("done")

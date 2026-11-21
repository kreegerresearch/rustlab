---
title: A patch antenna on a finite ground
---

# A patch antenna on a finite ground

The patch and its ground are metal sheets on a 1.6 mm substrate of $\varepsilon_r = 4.4$. Air surrounds that board, and CPML sits on all six faces of the mesh. The near-to-far box is in the air, not on the ground, so the pattern is the finite-ground pattern. There is no image theory.

The mesh is 2 mm in the plane. Vertical air is 20 mm. A 10 mm gap above the patch puts the box in the reactive field and the pattern integral reads about 2% high; that case is a release diagnostic, not this notebook. The feed is a 50 $\Omega$ series port on the first metal node of the centre row.

## Presets

The first line of the cell picks the mesh. The default, `preset = "quick"`, puts one 1.6 mm cell through the substrate and runs in about five seconds on four cores. `preset = "full"` is two 0.8 mm cells through the substrate. It takes about five times as long and moves the dip from 2.358 GHz to 2.375 GHz, closer to the 1 mm reference at 2.437 GHz; directivity is 6.92 dBi on quick and 6.94 dBi on full. Edit that line to `"full"` to run it.

| preset | substrate cells | air beside the ground | pulse $\tau$ | cells | lossless steps | lossy steps |
|---|---:|---:|---:|---:|---:|---:|
| `quick` (default) | 1 | 4 mm | $1/f_0$ | 152,280 | 4,500 | 3,500 |
| `full` | 2 | 10 mm | $1/(2f_0)$ | 285,120 | 16,000 | 8,000 |

Both keep 20 mm of air above and below the board, for the reason above.

The field on the near-to-far box sets the length of the run: `fdtd_farfield` refuses a surface tail above $10^{-3}$ of the peak, and the port voltage dies well before that. With $\tau = 1/(2f_0)$ the pulse spectrum at DC is 8.5% of its peak. That charges the patch, and the static field it leaves on the box decays slowly: on the quick mesh the tail is still $7.0\times 10^{-4}$ after 8,000 steps. With $\tau = 1/f_0$ the DC content is 0.005% of the peak, and the tail is $1.4\times 10^{-4}$ after 4,500 lossless steps and $3.9\times 10^{-4}$ after 3,500 lossy ones. The spectrum is still $-12$ dB at 1.5 GHz and $-18$ dB at 3.5 GHz, the edges of the band the script reads. $S_{11}$, directivity, and gain are ratios at each frequency, so the pulse only sets how long the run must be: the quick preset with the shorter pulse, 10 mm of side air, and 8,000 and 5,000 steps prints the same dip, $|S_{11}|$, directivity, gains, front-to-back ratio, and lossy efficiency. The pattern integral moves from 0.9991 to 0.9990 of the flux (gate 0.5%), and the power balance from $-4\times 10^{-6}$ to $1\times 10^{-4}$ (gate $10^{-2}$).

## Loss

The lossy rerun builds a new grid with $\sigma = 2\pi f_0 \varepsilon_r \varepsilon_0 \tan\delta$ at $\tan\delta = 0.02$. Nothing is reset. Directivity should stay put. Gain should fall by about $10\log_{10}\eta$, with $\eta$ near one half. Only broadside is evaluated for the lossy pattern: the flux and the accepted power do not depend on the angle grid.

The edge resistance is an independent check on that efficiency. At a dip that sits near a real impedance, $R = 50(1+|\Gamma|)/(1-|\Gamma|)$. On the quick mesh the minima are 206 $\Omega$ with no loss ($|S_{11}| = 0.610$) and 101 $\Omega$ at $\tan\delta = 0.02$ ($|S_{11}| = 0.336$), so the ratio is 0.487. The flux divided by the accepted port power is $\eta = 0.486$. The two come from different paths, and they agree.

The script stops if a gate misses. The numbers it prints are the measurement.

```rustlab
% Patch antenna on a finite ground, floating in air.
% Demonstrates: fdtd_media, fdtd_pec, fdtd_cpml, fdtd_port, fdtd_s11,
% fdtd_ntff, fdtd_farfield. The lossy rerun rebuilds the grid.
%
% preset is "quick" or "full". quick is the default: one cell through the
% 1.6 mm substrate, 4 mm of air beside the ground, a pulse with
% tau = 1/f0, 4 500 lossless steps and 3 500 lossy steps. The dip and the
% directivity are approximate. full is the 2 mm lesson mesh, two cells
% through the substrate, 10 mm of air beside the ground, tau = 1/(2 f0),
% 16 000 lossless steps and 8 000 lossy steps, about five times the run
% time. Each step count is where the near-to-far box has decayed under the
% 1e-3 pulse check with margin; the lossy grid decays faster. At
% tau = 1/(2 f0) the pulse spectrum at DC is 8.5% of its peak, which leaves
% a static field on the box that decays slowly; at tau = 1/f0 it is 0.005%.
% S11, directivity, and gain are ratios at each frequency, so the pulse
% only sets how long the run must be. The 1 mm comparison with the Phase 4
% reference is a release test, not this script. Vertical air is 20 mm so
% the near-to-far box is out of the reactive field above the patch. The
% feed is the edge feed: the first metal node of the centre row, a 50 ohm
% series port.

preset = "quick";

function mesh = patch_preset(name)
    switch name
        case "quick"
            fprintf("quick preset: one cell through the substrate, 4500 + 3500 steps. The dip and directivity are approximate.\n");
            mesh = struct("dx", 2e-3, "dy", 2e-3, "nsub", 1, ...
                "gpad", 16e-3, "air_xy", 4e-3, "air_z", 20e-3, ...
                "npml", 8, "margin", 2, "nsteps", 4500, "nlossy", 3500, ...
                "tau_periods", 1);
        case "full"
            mesh = struct("dx", 2e-3, "dy", 2e-3, "nsub", 2, ...
                "gpad", 16e-3, "air_xy", 10e-3, "air_z", 20e-3, ...
                "npml", 8, "margin", 2, "nsteps", 16000, "nlossy", 8000, ...
                "tau_periods", 0.5);
        otherwise
            error(sprintf("unknown preset %s: use quick or full", name));
    end
end

mesh = patch_preset(preset);

c0 = 299792458;
mu0 = 1.25663706212e-6;
eps0 = 1 / (mu0 * c0 * c0);
eps_r = 4.4;
h = 1.6e-3;
f0 = 2.4e9;
tand = 0.02;

W = c0 / (2 * f0) * sqrt(2 / (eps_r + 1));
ee = (eps_r + 1) / 2 + (eps_r - 1) / 2 / sqrt(1 + 12 * h / W);
dL = 0.412 * h * (ee + 0.3) * (W / h + 0.264) / ((ee - 0.258) * (W / h + 0.8));
L = c0 / (2 * f0 * sqrt(ee)) - 2 * dL;
fprintf("design  L = %.2f mm   W = %.2f mm   eps_eff = %.4f   dL = %.3f mm\n", ...
    L * 1e3, W * 1e3, ee, dL * 1e3);

function sim = patch_sim(mesh, sigma, L, W, f0, eps_r)
    dx = mesh.dx;
    dy = mesh.dy;
    nsub = mesh.nsub;
    dz = 1.6e-3 / nsub;
    gpad = mesh.gpad;
    air_xy = mesh.air_xy;
    air_z = mesh.air_z;
    npml = mesh.npml;
    margin = mesh.margin;
    ngx = 2 * round((L / 2 + gpad) / dx);
    ngy = 2 * round((W / 2 + gpad) / dy);
    nax = round(air_xy / dx);
    nay = round(air_xy / dy);
    naz = round(air_z / dz);
    nx = ngx + 2 * (npml + margin + nax);
    ny = ngy + 2 * (npml + margin + nay);
    nz = nsub + 2 * (npml + margin + naz);
    jg0 = npml + margin + nax;
    ig0 = npml + margin + nay;
    kg = npml + margin + naz;

    sim = fdtd3d(nx, ny, nz, dx, dy, dz);
    eps = ones3(ny, nx, nz);
    eps(ig0 + 1:ig0 + ngy, jg0 + 1:jg0 + ngx, kg + 1:kg + nsub) = eps_r;
    sig = zeros3(ny, nx, nz);
    if sigma > 0
        sig(ig0 + 1:ig0 + ngy, jg0 + 1:jg0 + ngx, kg + 1:kg + nsub) = sigma;
    end
    fdtd_media(sim, eps, 1, sig);
    fdtd_cpml(sim, npml);

    xv = (0:nx - 1) * dx;
    yv = (0:ny - 1) * dy;
    [X, Y] = meshgrid(xv, yv);
    ground = zeros(ny, nx);
    ground(ig0 + 1:ig0 + ngy + 1, jg0 + 1:jg0 + ngx + 1) = 1;
    fdtd_pec(sim, "z", kg + 1, ground);
    px = nx / 2 * dx - L / 2;
    py = ny / 2 * dy - W / 2;
    patch = rect_mask(X, Y, px, py, L, W);
    fdtd_pec(sim, "z", kg + nsub + 1, patch);

    cols = find(sum(patch, 1) > 0);
    rows = find(sum(patch, 2) > 0);
    span_y = rows(end) - rows(1);
    if span_y != 2 * round(span_y / 2)
        error("patch width is an odd number of cells, so the feed is off the mirror plane");
    end
    iy = round((py + W / 2) / dy) + 1;
    if iy != (rows(1) + rows(end)) / 2
        error("feed row is not the mirror plane of the patch");
    end
    ix = find(patch(iy, :) > 0);
    ix = ix(1);
    tau = mesh.tau_periods / f0;
    fdtd_port(sim, "ez", iy, ix, kg + 1:kg + nsub, 50, "modulated", 4 * tau, tau, f0);
    fdtd_ntff(sim, 2.34e9:0.02e9:2.40e9, "margin", margin);
end

function s = dip_of(sim)
    rec = fdtd_record(sim);
    s = fdtd_s11(rec.V(:, 1), rec.I(:, 1), rec.dt, 50, "nfft", 8 * rec.steps);
end

sigma = 2 * pi * f0 * eps_r * eps0 * tand;
lossless = patch_sim(mesh, 0, L, W, f0, eps_r);
out = fdtd_run(lossless, mesh.nsteps);
fprintf("lossless  %.1f s   %.1f Mcell/s   %d cells\n", out.seconds, out.cell_rate / 1e6, ...
    fdtd_info(lossless).nx * fdtd_info(lossless).ny * fdtd_info(lossless).nz);
rec = fdtd_record(lossless);
nv = rec.steps;
last = round(0.9 * nv);
tv = max(abs(rec.V(last:end, 1))) / max(abs(rec.V(:, 1)));
ti = max(abs(rec.I(last:end, 1))) / max(abs(rec.I(:, 1)));
fprintf("port tail  |V| %.2e   |I| %.2e\n", tv, ti);
if tv > 1e-3 || ti > 1e-3
    error(sprintf("port tail |V| %.2e |I| %.2e", tv, ti));
end
s0 = dip_of(lossless);
f = s0.frequencies;
a = abs(sij(s0, 1, 1));
band = find(and(f > 1.8e9, f < 3.2e9));
k = argmin(a(band));
f_dip = f(band(k));
a_dip = a(band(k));
wide = find(and(f > 1.5e9, f < 3.5e9));
a_max = max(a(wide));
fprintf("dip %.4f GHz   |S11| %.3f (%.1f dB)   max |S11| %.4f\n", ...
    f_dip / 1e9, a_dip, 20 * log10(a_dip), a_max);

% The staircased metal is the node span. One extra cell is the Yee edge
% sitting half a cell outside each end, which is what the TL check uses.
dx = mesh.dx;
dy = mesh.dy;
Lm = 2 * floor(L / (2 * dx)) * dx;
Wm = 2 * floor(W / (2 * dy)) * dy;
w_eff = Wm + dx;
ee_m = (eps_r + 1) / 2 + (eps_r - 1) / 2 / sqrt(1 + 12 * h / w_eff);
dL_m = 0.412 * h * (ee_m + 0.3) * (w_eff / h + 0.264) / ((ee_m - 0.258) * (w_eff / h + 0.8));
f_model = c0 / (2 * (Lm + dx + 2 * dL_m) * sqrt(ee_m));
fprintf("TL+cell %.4f GHz   err %+.2f %%\n", f_model / 1e9, (f_dip / f_model - 1) * 100);

if abs(f_dip / f0 - 1) > 0.05
    error(sprintf("dip %.4f GHz is more than 5%% from 2.4 GHz", f_dip / 1e9));
end
if abs(f_dip / f_model - 1) > 0.015
    error(sprintf("dip is %.2f%% from the staircased TL model", (f_dip / f_model - 1) * 100));
end
if a_max > 1
    error(sprintf("max |S11| = %.4f on a decayed record", a_max));
end

th = (0:3:180) * pi / 180;
ph = (0:3:357) * pi / 180;
pat = fdtd_farfield(lossless, th, ph);
q = argmin(abs(pat.freq - f_dip));
D = squeeze(pat.directivity(q, :, :));
U = squeeze(pat.U(q, :, :));
dth = 3 * pi / 180;
dph = 3 * pi / 180;
wth = th * 0 + dth;
wth(1) = wth(1) / 2;
wth(end) = wth(end) / 2;
wcol = (sin(th) .* wth)';
psph = sum(sum(U .* (wcol * ones(1, length(ph))))) * dph;
bal = (pat.paccepted(q) - pat.prad(q)) / pat.paccepted(q);
d0 = pat.directivity_dBi(q, 1, 1);
g0 = pat.gain_dBi(q, 1, 1);
gr0 = pat.realized_gain_dBi(q, 1, 1);
fb = 10 * log10(D(1, 1) / D(end, 1));
fprintf("D %.2f dBi   G %.2f   Gr %.2f   F/B %.1f dB   Psph/Pflux %.4f   balance %.2e\n", ...
    d0, g0, gr0, fb, psph / pat.prad(q), bal);

[mx, imax] = max(D(:));
[ti, pj] = ind2sub(size(D), imax);
if ti != 1
    error("broadside is not the directivity peak");
end
if d0 < 6 || d0 > 8
    error(sprintf("broadside directivity %.2f dBi is outside 6 to 8", d0));
end
if fb < 10
    error(sprintf("front/back %.1f dB is under 10", fb));
end
if abs(psph / pat.prad(q) - 1) > 5e-3
    error(sprintf("pattern integral is %.2f%% off the flux", (psph / pat.prad(q) - 1) * 100));
end
if abs(bal) > 1e-2
    error(sprintf("power balance %.2e", bal));
end
if abs(g0 - d0) > 0.05
    error(sprintf("lossless gain and directivity differ by %.3f dB", g0 - d0));
end

lossy = patch_sim(mesh, sigma, L, W, f0, eps_r);
fdtd_run(lossy, mesh.nlossy);
s1 = dip_of(lossy);
f1 = s1.frequencies;
a1 = abs(sij(s1, 1, 1));
% The lossy record is shorter, so its frequency grid is not the lossless one.
band1 = find(and(f1 > 1.8e9, f1 < 3.2e9));
k1 = argmin(a1(band1));
f1_dip = f1(band1(k1));
% Only broadside is read here. Flux and accepted power do not depend on the
% angle grid, so one direction gives the same efficiency and dBi values.
pat1 = fdtd_farfield(lossy, 0, 0);
q1 = argmin(abs(pat1.freq - f1_dip));
eta = pat1.prad(q1) / pat1.paccepted(q1);
d1 = pat1.directivity_dBi(q1, 1, 1);
g1 = pat1.gain_dBi(q1, 1, 1);
fprintf("lossy    dip %.4f GHz   |S11| %.3f   eta %.3f   D %.2f dBi   G %.2f dBi\n", ...
    f1_dip / 1e9, a1(band1(k1)), eta, d1, g1);
if abs(f1_dip / f_dip - 1) > 0.005
    error("loss moved the dip");
end
if a1(band1(k1)) >= a_dip
    error("loss did not deepen the match");
end
if eta < 0.40 || eta > 0.60
    error(sprintf("efficiency %.3f is outside 0.40 to 0.60", eta));
end
if abs(d1 - d0) > 0.05
    error(sprintf("loss moved directivity by %.3f dB", d1 - d0));
end
if abs(g1 - (d1 + 10 * log10(eta))) > 0.01
    error("gain is not directivity plus 10 log10(efficiency)");
end

clf;
plot(f / 1e9, 20 * log10(max(a, 1e-6)), "b", f1 / 1e9, 20 * log10(max(a1, 1e-6)), "r");
xlabel("Frequency (GHz)");
ylabel("|S11| (dB)");
title("Edge-fed patch, lossless and tan delta 0.02");
```

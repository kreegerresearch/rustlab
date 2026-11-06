# FDTD cavity — the lowest mode of a metal box

A closed box with perfectly conducting (PEC) walls rings only at its
resonant frequencies. For a rectangular box of size
$L_x \times L_y \times L_z$ they are

$$f_{mnp} = \frac{c}{2}\sqrt{\left(\frac{m}{L_x}\right)^2 + \left(\frac{n}{L_y}\right)^2 + \left(\frac{p}{L_z}\right)^2}.$$

The Yee grid does not reproduce these exactly. A sampled wave travels a
little slower than light, by an amount set by the cell size and the
time step. The grid's own resonance is the solution of its dispersion
relation, with $k_x = m\pi/L_x$ and so on:

$$\sin\frac{\omega\,\Delta t}{2} = c\,\Delta t\,\sqrt{\frac{\sin^2(k_x \Delta x/2)}{\Delta x^2} + \frac{\sin^2(k_y \Delta y/2)}{\Delta y^2} + \frac{\sin^2(k_z \Delta z/2)}{\Delta z^2}}.$$

A correct kernel matches that second number closely. The continuum
formula is a few hundredths of a percent away on this grid.

## The box

The cavity is 24 × 20 × 10 mm in 1 mm cells, inside a grid one cell
larger on every side. `fdtd_pec(sim, axis, index, mask)` makes one face
of the grid metal: `index` is the node plane at $(\text{index}-1)\,\Delta$
on that axis, and `true` covers the whole face. Six calls close the
box. The outer shell of the grid is cut off by the metal and stays at
zero.

A hard Gaussian on every $E_z$ edge of one column drives the box and
then lets go. A column through the full height excites only modes that
are uniform in $z$ ($p = 0$). The lowest of those is TM$_{110}$, at about
9.76 GHz. The next $p = 0$ mode is about 50% higher. The probe's
ring-down goes through a Hann window and a zero-padded FFT. A parabola
through the three bins around the peak gives the frequency.

The same box is then filled with $\varepsilon_r = 4$ using
`fdtd_media(sim, 4)`, which puts the mode at $1/\sqrt{4}$ of the vacuum
frequency. Materials are baked before the first step; the time step
stays the vacuum one, because no wave on the grid is faster than light.

```rustlab
clf
d = 1.0e-3;                             # 1 mm cells
cx = 24; cy = 20; cz = 10;              # cavity size in cells
nx = cx + 2; ny = cy + 2; nz = cz + 2;
Lx = cx * d; Ly = cy * d;
c0 = 299792458;

function [f, ez, dt] = ring(nx, ny, nz, d, eps_r, nsteps)
  sim = fdtd3d(nx, ny, nz, d, d, d);
  if eps_r != 1
    fdtd_media(sim, eps_r);
  end
  # Six faces 1 cell in from the outer walls.
  fdtd_pec(sim, "x", 2, true);
  fdtd_pec(sim, "x", nx, true);
  fdtd_pec(sim, "y", 2, true);
  fdtd_pec(sim, "y", ny, true);
  fdtd_pec(sim, "z", 2, true);
  fdtd_pec(sim, "z", nz, true);
  dt = fdtd_info(sim).dt;
  # Hard Gaussian on every Ez edge of one column (k = 2..nz-1 lie inside),
  # off centre so the source does not sit on a node of the next modes.
  tau = 14 * dt;
  nw = round(8 * tau / dt);
  g = gaussian_pulse((1:nw) * dt, 4 * tau, tau);
  fdtd_source(sim, "ez", 9, 10, 2:(nz - 1), "hard", g);
  id = fdtd_probe(sim, "ez", 14, 17, round(nz / 2));
  out = fdtd_run(sim, nsteps);
  ez = out.probe(:, id);
  # Hann window, zero padding, and a parabola through the peak bin.
  # Skip the hard source's own interval: the ring-down starts after it.
  x = ez(nw + 1:end);
  x = x .* window("hann", length(x));
  nfft = 8 * 2^nextpow2(length(x));
  X = abs(fft(x, nfft));
  fr = (0:nfft - 1) / (nfft * dt);
  band = find((fr > 2e9) .* (fr < 13e9));
  at = argmax(X(band));
  b = band(at);
  delta = 0.5 * (X(b - 1) - X(b + 1)) / (X(b - 1) - 2 * X(b) + X(b + 1));
  f = (b - 1 + delta) / (nfft * dt);
end

nsteps = 8000;
[f_vac, ez_vac, dt] = ring(nx, ny, nz, d, 1, nsteps);

f_an = c0 / 2 * sqrt(1 / Lx^2 + 1 / Ly^2);
s = c0 * dt * sqrt(sin(pi / (2 * cx))^2 / d^2 + sin(pi / (2 * cy))^2 / d^2);
f_yee = 2 * asin(s) / (2 * pi * dt);

fprintf("TM110 analytic   %.4f GHz\n", f_an / 1e9);
fprintf("TM110 Yee        %.4f GHz\n", f_yee / 1e9);
fprintf("measured         %.4f GHz  (%+.3f%% vs analytic, %+.3f%% vs Yee)\n", ...
        f_vac / 1e9, 100 * (f_vac / f_an - 1), 100 * (f_vac / f_yee - 1));
if abs(f_vac / f_an - 1) > 0.02 || abs(f_vac / f_yee - 1) > 0.005
  error("cavity: the TM110 peak is outside the 2% / 0.5% checks");
end

# Fill the whole grid with eps_r = 4: the mode drops by 1/sqrt(4).
[f_eps, ez_eps] = ring(nx, ny, nz, d, 4, nsteps);
fprintf("eps_r = 4        %.4f GHz  (ratio %.4f, expected 0.5)\n", ...
        f_eps / 1e9, f_eps / f_vac);
if abs(f_eps / f_vac - 0.5) > 0.005
  error("cavity: the dielectric shift is outside 1% of 1/sqrt(4)");
end

m = 600;                                # the first 1.1 ns of 8000 steps
t = (1:m) * dt;
subplot(2, 1, 1);
plot(t * 1e9, ez_vac(1:m), t * 1e9, ez_eps(1:m));
xlabel("t (ns)");
ylabel("Ez (V/m)");
title("Probe in the cavity, first 600 steps");
legend("vacuum", "eps_r = 4");
subplot(2, 1, 2);
nfft = 2^nextpow2(nsteps) * 4;
fr = (0:nfft / 2 - 1) / (nfft * dt);
Xv = abs(fft(ez_vac .* window("hann", nsteps), nfft));
Xe = abs(fft(ez_eps .* window("hann", nsteps), nfft));
plot(fr / 1e9, Xv(1:nfft / 2) / max(Xv), fr / 1e9, Xe(1:nfft / 2) / max(Xe));
xlim([0, 20]);
xlabel("f (GHz)");
ylabel("|FFT| (normalised)");
title("Spectrum: TM110 at the first peak");
```

The measured peak sits on the Yee prediction, and both are a few
hundredths of a percent below the continuum $f_{110}$. That gap is the
grid's numerical dispersion, and it shrinks as the cells get smaller.
The filled box rings at half the frequency. Its trace swings more
slowly in the upper panel, and its peak sits at half the vacuum one in
the lower panel.

The script stops with an error if either check fails: the vacuum peak
must be within 2% of $f_{110}$ and 0.5% of the Yee value, and the
filled box within 1% of half the vacuum frequency.

## Materials and metal in general

`fdtd_media(sim, eps_r, mu_r, sigma)` takes a real scalar or an
$n_y \times n_x \times n_z$ `Tensor3` for each map, one value per cell.
Each E edge takes the mean of the cells around it, so an edge on an
interface between two materials gets the average.
A loss tangent goes in through
$\sigma = 2\pi f\,\varepsilon_r\varepsilon_0 \tan\delta$; a complex
`eps_r` is refused. `fdtd_pec(sim, mask)` with a `Tensor3` mask makes
whole cells metal instead of one face.

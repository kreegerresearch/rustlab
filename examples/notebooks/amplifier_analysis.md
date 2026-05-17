# RF Amplifier Analysis — rfplot, Stability, Gain, Polish

This is the second half of the rustlab RF S-parameter walkthrough. The
companion notebook `sparameters_intro.md` covered loading, conversions,
cascading, and Smith charts. Here we focus on what a measurement
actually *tells* you about a 2-port device: the standard frequency-domain
review panel, port-level metrics (VSWR, return / insertion loss),
stability assessment via Rollett K and µ-parameters, simultaneous-
conjugate-match design with Γms / Γml, gain limits (MAG / MSG), and
graphical stability / gain analysis via circles on the Smith chart.

The final section covers the Phase 6 polish features: frequency-grid
interpolation for cross-sweep cascading, time-domain reflectometry via
IFFT, Touchstone noise-parameter access, and mixed-mode 4-port
conversion for differential designs.

## Setup — the same synthetic LNA

We start from the same hand-built 2-port LNA as the intro notebook so
the numerical results are comparable:

```rustlab
clf
f = [1e9, 2e9, 3e9, 4e9, 5e9, 6e9];
mag = [0.50  2.50  0.05  0.40;
       0.45  3.00  0.06  0.38;
       0.40  3.20  0.07  0.36;
       0.38  3.10  0.08  0.34;
       0.36  2.80  0.09  0.32;
       0.34  2.40  0.10  0.30];
ang_deg = [ 150  -45   90  170;
            140  -60   85  165;
            130  -75   80  160;
            120  -90   75  155;
            110 -105   70  150;
            100 -120   65  145];
S = zeros3(6, 2, 2);
for k = 1:6
  S(k, 1, 1) = mag(k, 1) * exp(j * ang_deg(k, 1) * pi/180);
  S(k, 2, 1) = mag(k, 2) * exp(j * ang_deg(k, 2) * pi/180);
  S(k, 1, 2) = mag(k, 3) * exp(j * ang_deg(k, 3) * pi/180);
  S(k, 2, 2) = mag(k, 4) * exp(j * ang_deg(k, 4) * pi/180);
end
s = sparameters(S, f);
disp(s)
```

## The 2×2 review panel — `rfplot(s)`

The default `rfplot` call gives you the canonical 2×2 "what did the VNA
measure" panel: `|S11|` and `|S21|` on the top row, `|S12|` and `|S22|`
on the bottom, all in dB on a log-frequency axis. This is the standard
RF first-look.

```rustlab
clf
rfplot(s)
```

Layout convention matches every commercial RF tool:

|        |        |
|---|---|
| `|S11|` (input return loss)  | `|S21|` (forward gain) |
| `|S12|` (reverse isolation)  | `|S22|` (output return loss) |

For non-2-port networks the same call falls back to a single `|S11|` dB
trace; the multi-trace 2×2 layout is specifically the 2-port review.

## Single-trace forms — phase, unwrap, group delay

Five single-trace variants pull out specific quantities:

```rustlab
clf
rfplot(s, "db", 2, 1)
title("Forward gain |S21| in dB")
```

```rustlab
clf
rfplot(s, "phase", 2, 1)
title("S21 phase (wrapped, degrees)")
```

For a delay-line or amplifier with non-trivial phase response, wrapped
phase is hard to read. The `"unwrap"` form removes the ±2π jumps:

```rustlab
clf
rfplot(s, "unwrap", 2, 1)
title("S21 unwrapped phase (degrees)")
```

And group delay $\tau_g = -d\varphi/d\omega$ is the natural metric for
linearity / dispersion:

```rustlab
clf
rfplot(s, "groupdelay", 2, 1)
title("S21 group delay (s)")
```

Group delay uses central differences on the unwrapped phase with
forward/backward differences at the endpoints. Constant group delay
across the band of interest indicates a linear-phase response.

## Port-level metrics

Three quick scalar metrics per frequency: voltage standing-wave ratio
at each port, return loss, and insertion loss between any two ports.

```rustlab
v1  = vswr(s, 1);
v2  = vswr(s, 2);
rl1 = return_loss(s, 1);
rl2 = return_loss(s, 2);
il  = insertion_loss(s, 2, 1);

fprintf("%-6s  %7s  %7s  %7s  %7s\n", ...
        "f/GHz", "VSWR1", "VSWR2", "RL1/dB", "IL/dB")
for k = 1:len(f)
  fprintf("%-6.2f  %7.2f  %7.2f  %7.2f  %7.2f\n", ...
          f(k)/1e9, v1(k), v2(k), rl1(k), il(k))
end
```

`vswr` caps at $10^6$ for fully-reflecting ports (`|S| → 1`); return
loss floors at 200 dB for matched ports (`|S| → 0`). Both keep the
results plottable and finite.

## Stability — Rollett K and µ-parameters

A 2-port is **unconditionally stable** at a given frequency if and only
if both of these hold:

- $K > 1$ where $K = \dfrac{1 - |S_{11}|^2 - |S_{22}|^2 + |\Delta|^2}{2\,|S_{12}\,S_{21}|}$
  is Rollett's stability factor and $\Delta = S_{11}\,S_{22} - S_{12}\,S_{21}$.
- $|\Delta| < 1$.

Equivalently, a single-number test exists: $\mu_1 > 1$, where
$\mu_1 = \dfrac{1 - |S_{11}|^2}{|S_{22} - \Delta\,S_{11}^*| + |S_{12}\,S_{21}|}$.

`stabilitymu` returns both µ1 and µ2; either > 1 is necessary and
sufficient for unconditional stability.

```rustlab
K  = stabilityk(s);
[m1, m2] = stabilitymu(s);
fprintf("%-6s  %6s  %6s  %6s\n", "f/GHz", "K", "mu1", "mu2")
for k = 1:len(f)
  fprintf("%-6.2f  %6.2f  %6.2f  %6.2f\n", f(k)/1e9, K(k), m1(k), m2(k))
end
```

K > 1 and µ1 > 1 across the entire 1–6 GHz band — this amplifier is
unconditionally stable, so the conjugate-match design that follows is
valid.

## Maximum available gain (MAG) and the conjugate match

For an unconditionally-stable amplifier, the optimum source and load
terminations satisfying $\Gamma_{\text{in}}(\Gamma_L) = \Gamma_S^*$ and
$\Gamma_{\text{out}}(\Gamma_S) = \Gamma_L^*$ exist in closed form:

$$\Gamma_{ms} = \frac{B_1 - \text{sign}(B_1)\sqrt{B_1^2 - 4|C_1|^2}}{2\,C_1}$$

where $B_1 = 1 + |S_{11}|^2 - |S_{22}|^2 - |\Delta|^2$ and
$C_1 = S_{11} - \Delta\,S_{22}^*$. Γml has the symmetric form with
ports swapped. With those terminations, the transducer power gain
equals the **maximum available gain**:

$$\text{MAG} = \frac{|S_{21}|}{|S_{12}|}\left(K - \sqrt{K^2 - 1}\right)$$

For K ≤ 1 (potentially unstable), MAG is undefined and `gainmax`
returns instead the **maximum stable gain** $\text{MSG} = |S_{21}/S_{12}|$.

```rustlab
gms = gammams(s);
gml = gammaml(s);
mag = gainmax(s);
fprintf("%-6s  %16s  %16s  %7s\n", ...
        "f/GHz", "Γms", "Γml", "MAG/dB")
for k = 1:len(f)
  fprintf("%-6.2f  %6.3f∠%5.1f°    %6.3f∠%5.1f°    %7.2f\n", ...
          f(k)/1e9, ...
          abs(gms(k)), angle(gms(k))*180/pi, ...
          abs(gml(k)), angle(gml(k))*180/pi, ...
          mag(k))
end
```

Both Γms and Γml lie inside the unit disk (their magnitudes < 1) —
expected for an unconditionally-stable network. MAG ranges from about
8 dB at the band edges to 12 dB at the peak around 3 GHz.

## Reflection coefficient with termination

`gammain(s, Γ_load)` and `gammaout(s, Γ_source)` compute the input or
output reflection for a given termination. The defining identity for
the simultaneous conjugate match is that

$$\Gamma_{\text{in}}(\Gamma_{ml}) = \Gamma_{ms}^* \quad \text{and} \quad \Gamma_{\text{out}}(\Gamma_{ms}) = \Gamma_{ml}^*$$

`gammain` accepts a scalar (broadcast across frequencies) or a per-
frequency vector matching `n_freqs`.

```rustlab
gin_check = gammain(s, gml);
gout_check = gammaout(s, gms);
fprintf("Γin(Γml) - conj(Γms) at 3 GHz: %.2e\n", ...
        abs(gin_check(3) - conj(gms(3))))
fprintf("Γout(Γms) - conj(Γml) at 3 GHz: %.2e\n", ...
        abs(gout_check(3) - conj(gml(3))))
```

Both diffs come out at machine precision — the identity holds exactly.

## Stability circles on the Smith chart

Even for unconditionally-stable networks it's useful to visualize the
stability boundary: the locus of source (or load) reflections at which
the *other* port's reflection touches the unit disk. `stability_circles`
returns a tagged struct containing per-frequency centre and radius
vectors; render them with `smith_circle()` in a loop:

```rustlab
clf
smith(s)
marker(0, "matched")

in_circles = stability_circles(s, "input");
cs = in_circles.centres;
rs = in_circles.radii;
for k = 1:len(f)
  smith_circle(cs(k), real(rs(k)))
end
title("S11, S22 with input stability circles overlaid")
```

For this LNA the input stability circles lie entirely outside the unit
disk — the geometric witness of unconditional stability. (If a circle
crossed into the unit disk, sources with reflections inside that
intersection would drive `|Γout| > 1` at that frequency, i.e.
oscillation.)

## Constant-gain circles

`gain_circles(s, gain_db)` returns the locus of load reflections that
achieve a specified operating-power gain. Useful for amplifier matching
design: pick an operating gain a couple of dB below MAG, draw the
circle, and any load termination on that locus delivers that gain
(trading off bandwidth, noise figure, etc.).

```rustlab
clf
smith(s)
marker(0, "matched")

% At 3 GHz (MAG ≈ 11.7 dB), draw circles at 10 dB and 11 dB.
% (Stash struct fields in scalars first — `struct.field(k)` parses as a
% call to a function named `field`, not as field-access + index.)
g10 = gain_circles(s, 10);
g11 = gain_circles(s, 11);
c10 = g10.centres;  r10 = g10.radii;
c11 = g11.centres;  r11 = g11.radii;
smith_circle(c10(3), real(r10(3)), "10 dB")
smith_circle(c11(3), real(r11(3)), "11 dB")
title("Gain circles at 3 GHz (MAG ≈ 11.7 dB)")
```

As the requested gain approaches MAG the circle shrinks to a single
point at Γml; gains beyond MAG return NaN radii (no real solution).

## Phase 6 — polish features

The remaining features cover four common practical needs.

### Cross-grid cascading via `interp_freq`

When two measurements live on different VNA sweeps, you can't `cascade`
them directly — the frequency grids must match. `interp_freq` resamples
linearly onto a new monotonic grid:

```rustlab
% Resample the LNA onto a finer 200 MHz grid (26 points across 1–6 GHz):
f_fine = linspace(1e9, 6e9, 26);
s_fine = interp_freq(s, f_fine);
fprintf("Original: %d points,  Interpolated: %d points\n", ...
        len(freqs(s)), len(freqs(s_fine)))
fprintf("|S21| at 2.5 GHz (interp): %.2f dB\n", ...
        mag2db(abs(s21(s_fine)(8))))
```

Extrapolation past the original sweep range is **rejected** with a clear
error — RF data is bandlimited; extrapolated S-parameters are garbage.

### Time-domain reflectometry — `s2td`

`s2td(s, i, j)` IFFTs a single Sij(f) trace into the time domain. Useful
for TDR (locating impedance discontinuities along a cable) and impulse-
response inspection. The frequency grid must be uniform — that's exactly
what `interp_freq` produces.

```rustlab
clf
[t, step] = s2td(s_fine, 2, 1, "step");
plot(t * 1e9, step)
title("S21 step response (TDR-style)")
xlabel("Time (ns)")
ylabel("Step response")
grid on
```

Two modes available: `"step"` (default, integrates the impulse response —
the standard TDR view) and `"impulse"` (the raw impulse response). The
internal IFFT uses a conjugate-symmetric 2N-point spectrum so the time-
domain signal is real, zero-padded to the next power of two for finer
time resolution.

### Touchstone noise parameters

Many real `.s2p` files include a noise-parameter block after the
S-parameter rows: five columns per row with frequency, $F_{\min}$ in dB,
$|\Gamma_{\text{opt}}|$, ∠Γopt in degrees, and the normalised equivalent
noise resistance $R_n / Z_0$.

The reader picks them up automatically when present and attaches them as
extra fields on the sparameters struct. Build a small noise-bearing
network inline (the repo ships
`examples/sparameters/data/lna_with_noise.s2p` as a ready file you can
load with `sparameters(path)` outside the notebook):

```rustlab
% Synthesize a small 2-port S + noise block in memory by writing a
% Touchstone file to a scratch path and reading it back. (The same
% data is bundled in examples/sparameters/data/lna_with_noise.s2p.)
tmp = "/tmp/_notebook_noise.s2p";
% S-only network for clarity:
S = zeros3(4, 2, 2);
for k = 1:4
  S(k, 2, 1) = 10^(-3/20);
  S(k, 1, 2) = 10^(-3/20);
end
s_clean = sparameters(S, [1e9, 2e9, 3e9, 4e9]);
save(tmp, s_clean)
% (A future writer enhancement will round-trip noise data; today the
% noise block needs to be hand-prepended to the .s2p file.)
if has_noise(s_clean)
  fprintf("has_noise(s_clean): yes\n")
else
  fprintf("has_noise(s_clean): no (this synthesised network has no noise block)\n")
end
```

When loading a real noise-bearing `.s2p`, the accessor builtins return
the per-noise-frequency vectors:

```text
nfmin(s_noise)      → real Vector, dB
gamma_opt(s_noise)  → complex Vector
rn(s_noise)         → real Vector (Rn/Z0)
noise_freqs(s_noise) → real Vector, Hz
has_noise(s_noise)  → Bool
```

The noise frequencies need not match the S grid — VNAs commonly sweep
many fewer noise points than S points.

### Mixed-mode for 4-port differential designs

For high-speed differential designs (SerDes, USB, PCIe, RF balanced
amps) the single-ended 4-port S-parameter matrix isn't the natural
representation — you want differential / common-mode (`Sdd`, `Sdc`,
`Scd`, `Scc`) instead. `s2smm` transforms a 4-port single-ended network
to its mixed-mode form via the standard Bockelman/Eisenstadt orthogonal
transform; `smm2s` is its inverse.

Port pairing convention: ports 1 (positive) and 3 (negative) form
differential pair 1; ports 2 and 4 form differential pair 2. The result
is organised as the block matrix `[Sdd | Sdc; Scd | Scc]` with port
order `[d1, d2, c1, c2]`.

```rustlab
% Build a near-ideal 4-port differential pair: strong within-pair
% transmission, small cross-coupling, small return loss.
S4 = zeros3(2, 4, 4);
for k = 1:2
  for i = 1:4; S4(k, i, i) = 0.10; end
  S4(k, 2, 1) = 0.70; S4(k, 1, 2) = 0.70;
  S4(k, 4, 3) = 0.70; S4(k, 3, 4) = 0.70;
  S4(k, 4, 1) = -0.05; S4(k, 1, 4) = -0.05;
  S4(k, 2, 3) = -0.05; S4(k, 3, 2) = -0.05;
end
s4 = sparameters(S4, [1e9, 2e9]);
smm = s2smm(s4);
fprintf("Single-ended tag: %s   Mixed-mode tag: %s\n", ...
        parameter_type(s4), parameter_type(smm))

P = smm.parameters;
fprintf("Sdd21 (diff → diff): %.3f∠%.1f°\n", ...
        abs(P(1, 2, 1)), angle(P(1, 2, 1))*180/pi)
fprintf("Scd21 (common → diff, ideally zero): %.3f\n", abs(P(1, 4, 1)))
```

A round-trip through `smm2s` recovers the single-ended network exactly
because the transform is orthogonal:

```rustlab
back = smm2s(smm);
P_orig = s4.parameters;
P_back = back.parameters;
diff = abs(P_back(1, 2, 1) - P_orig(1, 2, 1));
fprintf("smm2s round-trip diff at port (2,1): %.2e\n", diff)
```

## Summary

Across the two notebooks, the toolbox surface is:

- **Data + plotting** (intro): `sparameters`, `nports/freqs/sij/s11..s22`, parameter conversions `s2z/y/t/abcd` and reverses, `cascade`, `deembed`, `newref`, `save`, `smith`, `marker`.
- **Analysis** (this notebook): `rfplot` 2×2 + single-trace variants, `vswr`, `return_loss`, `insertion_loss`, `gammain`/`gammaout`, `stabilityk`, `stabilitymu`, `gammams`/`gammaml`, `gainmax`, `stability_circles`, `gain_circles`, `smith_circle`.
- **Polish** (this notebook): `interp_freq`, `s2td`, noise accessors (`nfmin`, `gamma_opt`, `rn`, `noise_freqs`, `has_noise`), mixed-mode `s2smm`/`smm2s`, Touchstone v2 keyword tolerance.

The standalone runnable scripts in `examples/sparameters/` are
organised by phase if you want a smaller starting point for any single
topic: `load_s2p.rlab`, `cascade_attenuator.rlab`, `smith_chart.rlab`,
`measurement_review.rlab`, `amplifier_stability.rlab`,
`polish_features.rlab`.

# RF S-Parameters — Loading, Conversions, Smith Charts

This notebook walks through the first half of the rustlab RF S-parameter
toolbox: reading a Touchstone file off a vector network analyzer (VNA),
inspecting the network, converting between the standard parameter
representations (S / Z / Y / T / ABCD), cascading two networks into one,
de-embedding a known fixture, and plotting reflection coefficients on a
Smith chart. The companion notebook `amplifier_analysis.md` continues with
stability, gain, and per-frequency analysis plots.

The math conventions follow every commercial RF tool and every textbook
on the subject (Pozar, "Microwave Engineering"). Where rustlab makes a
specific choice (matrix layout, default reference impedance, frame
strategies), the choice is called out so a reader coming from a different
toolbox knows what to expect.

## A synthetic 2-port LNA

For a self-contained walkthrough, we build a small illustrative LNA
inline: 6 frequencies from 1–6 GHz with hand-picked magnitudes and phases
that resemble a real low-noise amplifier (high `|S21|`, moderate input
return loss, low reverse leakage). The `sparameters(S, freqs)`
constructor takes a Tensor3 + frequency vector:

```rustlab
clf
f = [1e9, 2e9, 3e9, 4e9, 5e9, 6e9];
% Hand-picked magnitude / angle table (matches the bundled
% examples/rf/data/lna_demo.s2p):
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
  S(k, 1, 1) = mag(k, 1) * exp(j * ang_deg(k, 1) * pi/180);   % S11
  S(k, 2, 1) = mag(k, 2) * exp(j * ang_deg(k, 2) * pi/180);   % S21
  S(k, 1, 2) = mag(k, 3) * exp(j * ang_deg(k, 3) * pi/180);   % S12
  S(k, 2, 2) = mag(k, 4) * exp(j * ang_deg(k, 4) * pi/180);   % S22
end
s = sparameters(S, f);
disp(s)
```

The one-line `Display` summary tells you the port count, frequency span,
parameter type, and reference impedance. Underneath it's a struct with
the fields `s.parameters` (a 3-D complex tensor of shape
`[n_freqs, n_ports, n_ports]`), `s.frequencies` (real Hz), `s.num_ports`,
`s.impedance`. The tagged `__kind__` field lets the Display impl render
the summary instead of dumping every field.

### Inspecting the network

```rustlab
disp(nports(s))
disp(len(freqs(s)))
disp(s.impedance)
```

The convenience accessors `s11(s)`, `s12(s)`, `s21(s)`, `s22(s)` pull the
four 2-port reflection / transmission traces; `sij(s, i, j)` is the
general form. Each returns a complex vector of length `n_freqs`.

```rustlab
db21 = mag2db(abs(s21(s)));
db11 = mag2db(abs(s11(s)));
plot(freqs(s)/1e9, db21)
hold on
plot(freqs(s)/1e9, db11)
title("LNA: forward gain S21 (dB) and input return loss S11 (dB)")
xlabel("Frequency (GHz)")
ylabel("Magnitude (dB)")
legend("|S21|", "|S11|")
grid on
```

Forward gain peaks around 3 GHz at about 10 dB; input return loss
gradually improves with frequency. This is the simplest possible
"first-look" plot before bringing out the dedicated `rfplot` 2×2 review
panel (covered in the next notebook).

## Building a network from raw arrays

When you have measurement data in memory rather than in a file, the
`sparameters(S, freqs)` and `sparameters(S, freqs, Z0)` forms take a
Tensor3 plus a real frequency vector:

```rustlab
% A matched 10 dB attenuator, hand-built — useful as an analytic anchor.
mag = 10 ^ (-10/20);     % |S21| = |S12| = 0.31623
S = zeros3(3, 2, 2);
for k = 1:3
  S(k, 1, 2) = mag;
  S(k, 2, 1) = mag;
end
att = sparameters(S, [1e9, 2e9, 3e9]);
disp(att)
```

Reference impedance defaults to 50 Ω if omitted; pass an explicit value
as the third argument when working in a 75 Ω system.

## Parameter conversions — S ↔ Z, Y, T, ABCD

For network analysis you frequently want a different representation. The
named-conversion family covers the four standard transforms; each takes
a tagged `sparameters` struct and returns one tagged with the new type
letter:

| Direction | Builtin | Notes |
|---|---|---|
| S ↔ Z | `s2z(s)` / `z2s(z)` | Z-parameters; impedance domain. General N-port. |
| S ↔ Y | `s2y(s)` / `y2s(y)` | Y-parameters; admittance domain. General N-port. |
| S ↔ T | `s2t(s)` / `t2s(t)` | T-parameters (cascade form). 2-port only. |
| S ↔ ABCD | `s2abcd(s)` / `abcd2s(a)` | Voltage/current chain. 2-port only. |

The `parameter_type(s)` builtin returns the tag string and the `Display`
summary shows it. Conversions error if you hand them the wrong source
type — no silent guessing.

```rustlab
z = s2z(att);
y = s2y(att);
a = s2abcd(att);
disp(z)
disp(y)
disp(a)
```

A useful sanity check: ABCD of a series-impedance element is
`[[1, Z], [0, 1]]`. For a 25 Ω series resistor at 50 Ω reference, ABCD
should read `A = D = 1`, `B = 25`, `C = 0`:

```rustlab
% Pure series-25Ω 2-port: S11 = r/(r+2) at Z0 = 50 with r' = r/Z0 = 0.5
% So S11 = 0.5/2.5 = 0.2, S21 = 2/2.5 = 0.8.
S = zeros3(1, 2, 2);
S(1, 1, 1) = 0.2; S(1, 2, 2) = 0.2;
S(1, 1, 2) = 0.8; S(1, 2, 1) = 0.8;
res = sparameters(S, [1e9]);
a_res = s2abcd(res);
P = a_res.parameters;
fprintf("ABCD of series-25Ω: A=%.3f B=%.3f C=%.3f D=%.3f\n", ...
        real(P(1,1,1)), real(P(1,1,2)), real(P(1,2,1)), real(P(1,2,2)))
```

The `B = 25.000` value is the impedance in ohms — the lumped-element
ABCD identity holds exactly.

## Cascading and de-embedding

`cascade(s1, s2, ...)` chains two-port networks via T-parameter
multiplication. All inputs must share the same frequency grid and
reference impedance — no auto-interpolation (use `interp_freq` first if
your sweeps differ).

```rustlab
% Two matched 10 dB pads cascade to a single 20 dB pad. |S21| = 0.1
% (i.e. -20 dB), and S11 stays matched.
pair = cascade(att, att);
fprintf("Cascade |S21|: %.4f dB     |S11|: %.4f\n", ...
        mag2db(abs(s21(pair)(1))), abs(s11(pair)(1)))
```

`deembed(meas, left, right)` recovers the device-under-test from a
cascade containing two known fixtures on either side. Numerically it
inverts the T-product:
$T_{\text{DUT}} = T_{\text{left}}^{-1} \cdot T_{\text{meas}} \cdot T_{\text{right}}^{-1}$.

```rustlab
% Synthetic experiment: 3 dB + DUT + 2 dB attenuators in series.
L = zeros3(1, 2, 2); D = zeros3(1, 2, 2); R = zeros3(1, 2, 2);
L(1,1,2) = 10^(-3/20); L(1,2,1) = 10^(-3/20);
D(1,1,2) = 10^(-6/20); D(1,2,1) = 10^(-6/20);
R(1,1,2) = 10^(-2/20); R(1,2,1) = 10^(-2/20);
left = sparameters(L, [1e9]);
dut  = sparameters(D, [1e9]);
right = sparameters(R, [1e9]);
meas = cascade(left, dut, right);
recovered = deembed(meas, left, right);
fprintf("Synthetic DUT |S21| = %.4f, recovered = %.4f\n", ...
        abs(s21(dut)(1)), abs(s21(recovered)(1)))
```

The recovered value matches the synthetic DUT to machine precision.

## Re-normalising the reference impedance

`newref(s, Z_new)` converts to a different reference impedance by
detouring through the Z-domain. Scalar `Z_new` only — per-port
renormalisation is not supported.

```rustlab
s75 = newref(att, 75);
fprintf("After newref(att, 75): Z0 = %g Ω\n", s75.impedance)
% Round-trip 50 → 75 → 50 returns the original parameters:
back = newref(s75, 50);
fprintf("Round-trip |S21| diff = %.2e\n", ...
        abs(s21(back)(1) - s21(att)(1)))
```

## Smith chart — reflection coefficients on the unit disk

The `smith(...)` builtin renders S-parameter reflection coefficients on
a Smith chart with the conventional impedance grid. Multiple calling
forms cover the common cases:

```rustlab
clf
smith(s)
```

`smith(s)` plots `S11` (and `S22` if the network has at least 2 ports)
on an impedance grid; the chart frame, real-axis baseline, constant-R
circles, and constant-X arcs all render automatically. Axes lock to
`equal` aspect and the unit disk fills the panel.

The grid is synthesized as ordinary dashed line series with empty labels,
so it renders identically across every backend — terminal, SVG, PNG,
HTML/Plotly, LaTeX/PDF, the live `rustlab-viewer`, and animation paths
— with no per-backend code. The empty-label legend suppression keeps the
chart legend uncluttered (just one entry per data trace).

### Annotating cardinal points

`marker(gamma, label)` drops a labelled scatter point on the active
Smith axes. The Γ = 0 / -1 / +1 cardinal points are useful sanity
markers:

```rustlab
clf
smith(s)
marker(0,  "matched")     % chart centre — perfect 50 Ω match
marker(-1, "short")       % left edge of real axis
marker(1,  "open")        % right edge
```

### Picking a specific port pair

`smith(s, i, j)` plots just `Sij`; `smith(s, "ports", [i j])` (matrix
form) is reserved for future multi-pair selection.

```rustlab
clf
smith(s, 2, 1)             % forward-transmission locus
title("S21 locus")
```

For S21 of an amplifier the locus typically spirals outside the unit
disk at frequencies where the device has gain — a useful at-a-glance
indicator.

### Grid families: Z, Y, ZY

The `"grid"` name-value option switches between impedance (default),
admittance, or both overlaid. The immittance overlay is the conventional
matching-network design tool — you can read both Z and Y values off the
same chart.

```rustlab
clf
smith(s, "grid", "ZY")
```

The Y-grid arcs render in a muted blue-green so they're visually
distinguishable from the Z grid (which renders in light gray).

## A raw reflection-coefficient locus

`smith(gamma)` accepts a complex vector directly when you have
reflection-coefficient data that isn't an sparameters struct — for
example a matching-network path or a load-pull contour:

```rustlab
clf
theta = linspace(0, 2*pi, 200);
gamma = 0.5 * exp(j * theta);
smith(gamma)
title("|Γ| = 0.5 circle")
```

The circle of radius 0.5 centred at the origin is a standard
"constant-|Γ| = -6 dB" return-loss contour.

## Saving to Touchstone

`save("out.s2p", s)` writes an `sparameters` value back to a Touchstone
file. The writer always emits RI format in Hz at 15 significant figures
— lossless against f64 round-trip. The `.sNp` extension is inspected to
dispatch the save; the digit doesn't have to match the network's port
count (the writer always emits the actual count), but using the matching
suffix is the convention.

```rustlab
% Save and re-read from a scratch path. (The repo ships a bundled
% example at examples/rf/data/lna_demo.s2p that you can load
% with `sparameters("path/to/file.s2p")` directly when running outside
% the notebook renderer.)
tmp = "/tmp/lna_copy.s2p";
save(tmp, s)
s_copy = sparameters(tmp);
fprintf("Round-trip |S21| diff at 1 GHz: %.2e\n", ...
        abs(s21(s_copy)(1) - s21(s)(1)))
```

The writer currently doesn't round-trip noise parameters (covered in
the next notebook); save them separately if you need them preserved.

## What's in the box

This notebook covered the data-handling and visualization half of the
toolbox. The companion notebook `amplifier_analysis.md` continues with:

- **Network plots:** `rfplot(s)` 2×2 review panel; magnitude/phase/group-delay
  variants.
- **Analysis:** VSWR, return loss, insertion loss, Rollett K and µ-parameters
  for stability assessment, simultaneous-conjugate-match terminations Γms / Γml,
  maximum available / maximum stable gain.
- **Circles on the Smith chart:** input and output stability circles, constant-gain
  circles, overlaid via the `smith_circle()` helper.
- **Polish features:** `interp_freq` for cross-grid cascading, `s2td` for
  time-domain reflectometry, Touchstone noise-parameter accessors
  (`nfmin`, `gamma_opt`, `rn`), mixed-mode 4-port conversion (`s2smm`,
  `smm2s`).

Together the two notebooks cover the full toolbox surface; the standalone
runnable scripts in `examples/rf/` are organised by phase if
you want a smaller starting point per topic.

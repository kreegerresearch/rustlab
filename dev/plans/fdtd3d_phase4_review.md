# Phase 4 review: lumped port and S11 (physics review + implementation brief)

**Status:** implemented on draft PR #142 (0.3.59, unshipped). The gates below are unchanged. `port_reads_an_independent_resistor`, `port_short_reads_minus_one`, and `lossless_patch_s11_minimum_near_2_4_ghz` lock items 8 and 10. The recovered-resistance test and the inset-feed ring test stay diagnostics. Phase 5 is not started.
**Scope of this file:** what is right, what is wrong, the failing tests to write first, and the exact gates. The review commit changed no runtime code. The implementation that followed is in the FDTD crates, the example, and the docs.
**Reviewer runs:** release binary `./target/release/rustlab` at 0.3.59 (commit `c516fae`). Scripts in `dev/plans/fdtd3d_phase4_review/`. Run each one from the repo root with `--plot none`.

The original Phase 4 gates are unchanged and remain the bar:

1. **Validation item 8.** A matched lumped resistor reads `V/I → Z0` within 1% after a slow Gaussian. The load must be independent of the port's own update.
2. **Small matched S11.**
3. **Validation item 10.** The patch `|S11|` minimum is within 5% of 2.4 GHz on the teaching mesh. Dip depth is not gated.

A ring-frequency check does not replace item 10, and the port's own recurrence does not replace item 8.

---

## 1. Discrete port equations (checked against the Yee update, not the plan prose)

### Ez gap edge, one cell of the stack

- Cell area is `A = dx·dy`, and the gap edge length is `dz`.
- The stored voltage is `V = −Σ_k E_k·dz`.
- The port bakes the edge conductivity `σ_p = dz/((Z0/N)·A)`, so each edge carries `R_e = Z0/N`.
- The source term is `J = Vg/(Z0·A)`.

The Yee update, multiplied by `A`:

```text
ε0 A (E^{n+1} − E^n)/dt + σ_p A (E^{n+1} + E^n)/2 + J A = (curl H)^{n+1/2} · A
```

The right side is the Ampère loop:

```text
I^{n+1/2} = (Hy − Hy_left)·dy − (Hx − Hx_below)·dx
```

Each `ΔH` is multiplied by the length of the face side it sits on. This is Decision A, and it is correct.

With `E = −V_e/dz`, `C_e = ε0 A/dz`, and `R_e = dz/(σ_p A)`, the update becomes:

```text
I_loop = Vg/Z0 − V̄_e/R_e − C_e dV_e/dt          (V̄ = (V^{n+1}+V^n)/2)
```

That is a Norton source `Vg/Z0` in parallel with `R_e` and `C_e`, which is the same as a Thevenin source `Vg/N` in series with `Z0/N`. Stacking `N` edges in series gives `Vg` in series with `Z0`, plus a gap capacitance `C_e/N` on the source side.

`I_loop` is the conduction current that leaves the gap through the surrounding H loop, i.e. the current into the external grid. So:

- the reference plane is the gap terminals;
- `Z_L(ω) = V(ω)/I(ω)` is the impedance of everything outside the gap;
- `S11 = (Z_L − Z0)/(Z_L + Z0)`.

The gap's own `C` and any material `σ` already on the port edge belong to the source side. They load the generator, but they do not appear in `V/I`. Displacement current in the gap cannot make `|S11| > 1`. That would need `Re Z_L < 0`, which a passive grid cannot have.

### Passivity

For a passive load, `Re(V·I*) ≥ 0` in every bin, which is the same as `|S11| ≤ 1`. Every S11 result in this review is checked against that.

### Half-step

- `V[n]` is `E^{n+1}` at time `(n+1)dt`. `I[n]` is `H^{n+1/2}` at time `(n+½)dt`.
- So `DFT(I) = e^{−jω dt/2}·I(ω)`, and multiplying by `e^{+jω dt/2}` is the correct correction.
- Pairing `V̄` with the edge instead of `V^{n+1}` adds a `cos(ω dt/2)` factor. On the patch mesh (`dt ≈ 1.15e−12` s) at 2.4 GHz, `ω dt/2 ≈ 0.009` rad, so neither term matters for `|S11|`.

### Polarity

Flipping `I → −I` sends `Z → −Z` and `S11 → 1/S11`. A true 1/3 would therefore read as 3. Polarity is tested directly (§3, short and −I rows).

### Decision B: series source, not an impressed field

Decision B is required, not optional. With the plan sketch's `E := E − Cb_rel·Vg/(N dz)` applied every step and no current flowing, the steady state is:

```text
E (1 − Ca) = −Cb_rel Vg/(N dz)   ⇒   V = Vg / (2 s)
```

Here `s = σ dt/(2 ε0)`, so `V` is huge and depends on `dt`. That is not a Thevenin source. The shipped source `J = Vg/(Z0 A)` gives `V/Vg = R/(R + Z0)` on an independent load (measured exactly at DC, §3).

### Why `fdtd_s11`'s default fails: the window

`S11 = B(ω)/A(ω)` requires the full transforms of the incident and reflected waves `a(t)` and `b(t)`. A window `w(t)` returns:

```text
(B ∗ W)/(A ∗ W)
```

which is not `B/A`. On a transient, `w` gives very different weights to the early incident pulse and the late reflection. For a Hann window over 6000 steps with the pulse centre `t0` at step 724, `w(t0) ≈ 0.137`, while a reflection near mid-record keeps weight ≈ 1. The reflection is effectively amplified about 7× relative to the incident wave. That is the `|S11| > 1`, with **every** bin non-passive (measured in §3).

Windowing belongs to steady-state tones, not to a pulse that starts and ends inside the record. The plan's "Window: Hann" line is the error.

### Resolution and truncation

- **Resolution.** Bin spacing is `1/(N_steps·dt)`. For the patch at 6000 steps that is 145 MHz, about 6% of 2.4 GHz, which is too coarse for a 5% gate. Zero padding after the record has decayed interpolates the spectrum exactly, because a decayed record is zero beyond its end.
- **Truncation.** If the record stops while the structure is still ringing, the rectangular transform convolves the response with a sinc. This produces non-passive bins (measured: 35% of in-band bins at 6000 steps on the inset feed).

---

## 2. Independent fixture: a resistive pillar under a pad

This fixture is not an inversion of the port recurrence (files `resistor_load.rlab`, `resistor_bins.rlab`, `resistor_stack.rlab`).

**Grid.** `dx = 1 mm`, `dy = 1.5 mm`, `dz = 1 mm`. Because `dx ≠ dy`, a swapped Ampère length shows up as a wrong `R`.

**Load.**
- An `n × n = 4 × 4` block of cells, `kp` cells tall, has conductivity `σ = kp·dz/(R·n·dx·n·dy)` and sits on the `zmin` PEC wall.
- A PEC pad (`fdtd_pec` face at node plane `kp+1`) covers the block and overhangs it by one node toward the port.

**Why the DC resistance is exact.** The arithmetic-mean edge bake gives interior z-edges weight 1, side edges ½, and corner edges ¼ (`media.rs`). The weights sum to `n²`, so the pillar's DC conductance is exactly:

```text
G = σ·n²·dx·dy/(kp·dz) = 1/R
```

**Port.** The port is an `ez` stack `1:kp` one node outside the block, under the pad overhang. Its edges carry no material `σ`. The port and the load are separate edges.

**Expected impedance.** `Z(ω) = jωL + R/(1 + jωRC)`, where:
- `L` is the loop inductance of port → pad → pillar → ground;
- `C` is pad-to-ground capacitance, shunting the pillar.

Fitting the R = 25 and R = 100 bins at 500 MHz gives `L ≈ 1.29 nH` and `C ≈ 0.46 pF`. That fit predicts the independent R = 50 bin as `49.86 + 0.44j` (measured `49.79 + 0.40j`). A PEC block instead of the pillar (`short_load.rlab`) gives `L = 0.859 nH`, constant from 10 to 300 MHz.

**Source and runtime.** The source is a Gaussian with `τ = 1 ns`, `t0 = 4τ`, run for `14τ–16τ`, i.e. 6628–7575 steps at `dt = 2.112e−12` s. The record tail falls to `3e−11` of its peak. Runtime is 0.16 s per load in release.

---

## 3. Measured facts

Measured with the shipped 0.3.59 runtime, no changes. Nothing in this section is a hypothesis.

### Pillar, per-bin Z with 8× zero padding (`resistor_bins.rlab`)

Records of 6628 and 26512 steps agree to the printed digits, so the record length does not matter here.

| R (Ω) | 0 Hz | ~100 MHz | ~300 MHz | 500 MHz |
|---|---|---|---|---|
| 25 | 25.0000 | 25.0004 + 0.62j | 25.0033 + 1.92j | 25.0091 + 3.16j |
| 50 | 50.0000 | 49.9920 + 0.08j | 49.9233 + 0.24j | 49.7923 + 0.40j |
| 100 | 100.0000 | 99.92 − 2.10j | 99.24 − 6.44j | 97.97 − 10.42j |

### Pillar, band f ≤ 300 MHz, 5 unpadded bins (`resistor_load.rlab`)

| case | max \|Re Z/R − 1\| | max \|Im Z\|/R | V/Vg vs R/(R+Z0) | S11 error | Re(V I*) < 0 bins | time-domain V/I at \|I\| peak |
|---|---|---|---|---|---|---|
| R = 50 | 1.36e−3 | 4.47e−3 | 3.97e−3 | ≤ 2.34e−3 | 0/5 | 49.958 Ω |
| R = 25 | 1.19e−4 | 7.22e−2 | 4.99e−2 | 3.21e−2 (expect −1/3) | 0/5 | 25.0005 Ω |
| R = 100 | 6.73e−3 | 6.06e−2 | 1.88e−2 | 2.72e−2 (expect +1/3) | 0/5 | 99.587 Ω |
| R = 50, I → −I | 2.00 | — | — | — | 5/5 | −49.958 Ω |

The R = 25 and R = 100 errors in the S11, Im and Thevenin columns are the `L` and `C` of the fixture, not the port (see the bins table). `fdtd_s11(sim)` on R = 50 gives max `|S11|` of 0.0023 with `"rect"` and 0.0021 with `"hann"` for f ≤ 300 MHz.

### Stack depth and CPML (`resistor_stack.rlab`, R = 50)

| N edges | CPML | Z(DC) | V/Vg(DC) | \|S11\| at 100 MHz | Re(V I*) < 0 bins |
|---|---|---|---|---|---|
| 1 | off | 50.00000 | 0.50000 | 0.0062 | 0 |
| 1 | on | 50.00000 | 0.50000 | 0.0057 | 0 |
| 2 | off | 50.00000 | 0.50000 | 0.0008 | 0 |
| 2 | on | 49.99999 | 0.50000 | 0.0016 | 0 |
| 3 | off | 50.00000 | 0.50000 | 0.0056 | 0 |
| 3 | on | 50.00000 | 0.50000 | 0.0064 | 0 |

### Short (`short_load.rlab`)

| frequency | S11 | Z |
|---|---|---|
| 10 MHz | −1.0000 + 0.0017j | 0.0000 + 0.0422j Ω |
| 100 MHz | \|S11\| = 1.00000 | 0.0000 + 0.548j Ω |
| 300 MHz | \|S11\| = 1.00000 | 0.0000 + 1.603j Ω |

### Patch, teaching mesh (`patch_s11.rlab`, `patch_decay.rlab`)

Mesh: 60×60×20, 1 × 1 × 0.4 mm, 4 cells of εr = 4.4, 6-cell CPML, `zmin` off, port `ez 1:4`.

Transmission-line design: `L = 29.4216 mm`, `W = 38.0100 mm`, `px = 15.2892 mm`, `py = 10.9950 mm`. With the node rule of `rect_mask`, the metal spans nodes x = 16 … 44 mm, which is 28 mm long, not 29.4 mm.

The plan's edge feed `ix = round(px/dx) + 1` lands on x = 15 mm, **one node outside the metal**. The port then drives an open stub, not the patch.

| feed | steps (release s) | V tail/peak | min \|S11\| 1.5–3.5 GHz | Re(V I*) < 0 fraction |
|---|---|---|---|---|
| plan edge x = 15 mm, Hann (shipped default) | 6000 (3.3) | 6.9e−4 | 1.009 at 2.3194 GHz | 1.00 |
| plan edge x = 15 mm, rect, 8× pad | 6000 | 6.9e−4 | 0.989 at 2.3919 GHz | — |
| on-metal edge x = 16 mm, Hann | 6000 | 0.51 (late/peak) | 1.286 at 1.5946 GHz | 1.00 |
| on-metal edge, rect, 8× pad | 6000 (3.3) | 3.3e−2 | 0.674 at 2.4282 GHz | 0.045 |
| on-metal edge, rect, 8× pad | 16000 (8.2) | 2.7e−4 | **0.698 at 2.4395 GHz (+1.64%)** | **0.000** |
| on-metal edge, rect, 8× pad | 30000 (15.4) | 1.3e−5 | 0.698 at 2.4390 GHz (+1.63%) | 0.000 |
| inset L/3, Hann | 6000 | 0.43 (late/peak) | 1.178 at 1.5946 GHz | 1.00 |
| inset L/3, rect, 8× pad | 6000 (3.1) | 5.9e−2 | 0.140 at 2.3919 GHz | 0.351 |
| inset L/3, rect, 8× pad | 16000 (8.2) | 5.3e−3 | 0.206 at 2.3987 GHz | 0.007 |
| inset L/3, rect, 8× pad | 30000 (16.5) | 1.7e−4 | **0.222 at 2.3992 GHz (−0.03%)** | **0.000** |

The "tail/peak V" column is `max|V|` over the last 10% of the record divided by `max|V|`.

The old example's small PEC box (`|S11|` 0.912) is a lossless closed cavity read through a Hann window on a truncated record. It is not a matched-load test.

---

## 4. Conclusions from the measurements

1. **The port physics is correct.** This covers the σ split, the series source, stack normalisation, the Ampère segment lengths (dx ≠ dy fixture), the current sign, and the half-step. An independent resistor reads `R` exactly at DC and within 0.14% up to 300 MHz for R = 50, and the short reads S11 = −1.
2. **The `fdtd_s11` default (Hann) is wrong for a transient.** It produced every `|S11| > 1` seen in PR #142.
3. **The edge-feed "no dip" is the plan's feed index, not physics.** `round(px/dx)+1` is off the metal. The plan's intent is a stack "from the ground up to the metal", which means the first metal node.
4. **Item 10 needs a decayed record and sub-bin resolution.** With both, the connected edge feed dips at +1.6% and the inset at −0.03%. Both pass the original 5% gate with no geometry change beyond the feed index.
5. **The recovered-resistance check and the inset ring frequency are diagnostics.** The recovered resistance is the port's own KCL rearranged.

## 5. Hypotheses (not measured)

- The +1.6% edge-feed shift is mostly the 28 mm staircased metal (shorter than 29.42 mm raises the frequency) plus edge-feed reactance. Not separated.
- The pillar inductance (1.29 nH) is larger than the short's (0.86 nH) because current spreads through the resistive column. Not separated; it does not affect any gate.
- Other port components (`ex`, `ey`) use the same code path by permutation. They are not exercised by an independent load here. A permuted pillar test would cover them.

## 6. Decisions for Michael

Each item is a plan change; the review recommends the option given.

- **D1 — window default.** Make `fdtd_s11` default to no window. Keep `"hann"` as an explicit opt-in documented for steady-state tones only, or remove it.
- **D2 — zero padding.** Add `"nfft", M` (M ≥ steps) to `fdtd_s11`. Recommended default: `nfft = 8·steps` rounded to an FFT-friendly length.
- **D3 — decay check.** `fdtd_s11` should refuse (error), or warn once on stderr, when `max(|a|, |b|)` over the last 10% of the record exceeds `1e−3` of the peak. The patch needs about 16000 steps to pass that. Recommended: an error, with a hint naming the tail ratio and suggesting more steps. **Implemented:** the check is on the raw samples (last 10% of `|V|` and of `|I|`), because `a` and `b` are spectra. It applies to `fdtd_s11(sim)` unless a window is named or `"decay", "off"` is passed. `fdtd_s11(V, I, dt, Z0)` and an explicit window stay usable for a steady tone unless `"decay", "pulse"` is set. There is no unconditional refusal of every input.
- **D4 — feed index.** Replace the plan's edge feed `round(px/dx)+1` with the first metal node of the mask, e.g. `find(patch(iy, :), 1)`.
- **D5 — patch snapping.** Leave the mesh as is; do not snap `L` to the grid. It passes at +1.6% without snapping.
- **D6 — Decisions A and B.** Sign-off (§1 shows both are forced by the equations).

---

## 7. Implementation brief (for the coder)

Write these failing tests first, then make them pass. Do not loosen any number below. If a gate cannot be met, stop and report.

### Failing tests

1. **`crates/rustlab-dsp/src/fdtd/tests.rs`: `port_reads_independent_resistor`.**
   - Build the pillar fixture of §2 in Rust: `set_media` with σ on the block, `add_pec_face` for the pad, and the port at `(ip + n/2, jp − 1, 1..=kp)`.
   - Use R ∈ {25, 50, 100}, Gaussian `τ = 1 ns`, `t0 = 4τ`, 16τ.
   - Compute `Z(f) = V(f)/(I(f)·e^{jω dt/2})` on an 8× zero-padded rectangular DFT, using the in-test DFT helper, not `s11`.
   - Gates in §8 (G1–G4). Expected to pass on the current runtime; it locks the physics.
2. **Same file: `port_short_reads_minus_one` and `port_polarity_is_passive`.**
   - Short (PEC block): G5.
   - Negating `I` in the test must turn `Re Z(DC)` negative. This asserts the test can see a sign error.
3. **`s11_of_a_transient_is_passive`.**
   - Feed the R = 50 and R = 100 pillar records to `fdtd::s11` with its **default** options.
   - Assert `|S11| ≤ 1 + 1e−9` and `Re(V I*) ≥ 0` in every bin up to 300 MHz.
   - This failed on the Hann default. The pulse default is now rectangular, and `port_reads_an_independent_resistor` locks the passivity gate.
4. **`s11_zero_pad_matches_direct_dft`.** The padded `s11` at a bin equals a direct DFT of `V` and `I` at that frequency to 1e−9 relative. New `nfft` option.
5. **`s11_rejects_undecayed_record`** (if D3 = error). A 6000-step inset patch, or a synthetic ringing record, must be refused.
6. **Patch: replace `lossless_patch_inset_feed_rings_at_2_4_ghz` with `lossless_patch_s11_minimum_near_2_4_ghz`.**
   - Use the edge feed on the first metal node, 16000 steps, default `s11` (no window, 8× pad).
   - Assert G6 and G7.
   - Mark it `#[ignore]` only if the debug runtime exceeds the suite budget, and add the same check to `crates/rustlab-script/tests/fdtd.rs` or a release-run example so CI exercises it. 16000 steps is 8.2 s in release.
   - Also assert the inset feed (G6/G7) if runtime allows. Its measured margin is larger.
7. **Script-level test in `crates/rustlab-script/tests/fdtd.rs`.** The R = 50 pillar through `fdtd_port` and `fdtd_s11(sim)`: max `|S11|` ≤ 0.01 for f ≤ 300 MHz with default options.

### Runtime changes, after the tests fail for the right reason

- **`crates/rustlab-dsp/src/fdtd/port.rs::s11`.** Default no window. Add a pad length (`nfft`). Add the decay check (D3). Keep the half-step factor and the `|a|` floor.
- **`crates/rustlab-script/src/eval/fdtd.rs`.** `"window"` default `"rect"`. Add a `"nfft"` option, validated as an integer ≥ the record length. Surface the decay error.
- **Do not change** the update kernel, the σ bake, the source, or the Ampère current. §3 shows they are correct.

### Example and notebook

Rewrite them; this is not runtime work, but it is required.

- `examples/pde/fdtd_port.rlab` and `examples/notebooks/fdtd_port.md` use the pillar instead of the PEC box.
- The 1% gate is raw `V/I` against the independent `R`, not the recovered resistance.
- Print `|S11|` for R = 50 and show `smith`/`rfplot` of the R = 25/50/100 sweep.
- Regenerate `gallery/fdtd_port.md` and its SVGs.

### Docs

- `docs/functions.md`, `docs/quickref.md`, the REPL `HelpEntry` for `fdtd_s11` (`repl.rs`), and the CHANGELOG get the new defaults and options.
- Mark Phase 4 complete in `dev/plans/fdtd3d.md` and `AGENTS.md` **only after** G1–G8 pass.
- Fix the plan's "Window: Hann" line, its feed index, and its impressed-source sketch (D1, D4, D6).

### Constraints

- Keep 0.3.59; it has not shipped.
- No new dependencies. No Phase 5/6.
- Format only the edited files (`cargo fmt --all` reformats unrelated files).
- The script and CLI link need `LIBRARY_PATH=/usr/lib/gcc/x86_64-linux-gnu/13` on this VM.

---

## 8. Acceptance gates (exact)

Pillar fixture of §2: `dx = 1`, `dy = 1.5`, `dz = 1` mm; `n = 4`; `kp = 2` (and `kp = 1, 3` for G3); Gaussian `τ = 1 ns`, 16τ; 8× zero-padded rectangular DFT.

| id | quantity | gate | measured (0.3.59) |
|---|---|---|---|
| G1 | R = 50: `|S11(f)|` for 0 < f ≤ 300 MHz | ≤ 0.01 | ≤ 0.0025 |
| G2 | R ∈ {25, 50, 100}: `|Z(f)/R − 1|` for f ≤ 25 MHz | ≤ 0.01 | ≈ 0.0063 (R = 25) and 0.0053 (R = 100) at 25 MHz, interpolated from the 54 MHz bins; Im is the fixture's L, C |
| G2b | R ∈ {25, 50, 100}: `|Re Z(f)/R − 1|` for f ≤ 200 MHz | ≤ 0.01 | ≤ 0.0033 |
| G3 | R = 50, N ∈ {1, 2, 3}, CPML off/on: `|V(0)/Vg(0) − ½|` and `|Z(0)/R − 1|` | ≤ 1e−3 | ≤ 2e−7 |
| G4 | time domain, item 8 wording: `V/I` at the `|I|` peak vs R | within 1% | 0.08%, 0.002%, 0.41% |
| G5 | short: `|S11(f) + 1|` at f ≤ 10 MHz; `|S11|` for f ≤ 300 MHz | ≤ 0.01; within 1e−3 of 1 | 0.0017; 1.00000 |
| G6 | patch (teaching mesh, feed on first metal node, decayed record): frequency of min `|S11|` over 1.5–3.5 GHz | within 5% of 2.4 GHz | +1.64% edge, −0.03% inset |
| G7 | patch: `Re(V I*) ≥ 0` (equivalently `|S11| ≤ 1`) every bin 1.5–3.5 GHz | all bins | all bins at 16000 steps (edge); inset 0.7% fail at 16000, 0 at 30000 |
| G8 | `fdtd_s11` defaults on any record that passes the decay check | `|S11| ≤ 1 + 1e−9` every bin | fails today (Hann) |

G7 for the inset feed needs about 30000 steps (16.5 s release); gate the inset only if that runtime is acceptable, otherwise gate the edge feed. G6 dip depth is not gated, as in the plan.

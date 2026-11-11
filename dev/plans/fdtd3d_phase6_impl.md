# FDTD Phase 6 — patch-antenna capstone, width sweep, parallel stepping (implementation brief)

**Status:** implemented in 0.3.64. The review (`fdtd3d_phase6_impl_review.md`) asked to keep the 72,000-cell gate off the pool. After the CPML correction moved onto the same row split, that mesh is 1.88× faster on the pool than serial (4 threads, release), and faster than the 0.3.63 serial step (4.536 s). The threshold is 8,192 cells: the pool loses at 4,096 and 5,832, ties at 8,000, and wins from 10,648. Task size is `min(65_536/nx, n_rows/(4·threads))` rows. The 32,768 figure below is the brief's original number.
**Target version:** 0.3.64 (one implementation PR, staged commits, §10).
**Parent:** `dev/plans/fdtd3d.md` § Phase 6. This brief replaces that section's geometry (ground on the `zmin` wall, image theory) and its `fdtd_reset` step.
**Read first:** `dev/plans/fdtd3d.md`, `dev/plans/fdtd3d_phase4_review.md`, `dev/plans/fdtd3d_phase5_impl.md`, `dev/plans/fdtd3d_phase5_impl_review.md`.
**Prototype:** `dev/plans/fdtd3d_phase6_impl/` (scripts, runner, logs). Every number in §4 comes from those scripts on the 0.3.63 release binary on a 4-vCPU cloud VM, serial stepping.

Decided by Michael (not open in this brief):

- A finite ground plane and substrate float in air on a larger mesh. CPML on all six faces. A closed NTFF box in homogeneous air. No image theory, no ground on a wall.
- Order: serial capstone (convergence study, S11 dip against the Phase 4 reference, broadside lobe, power balance, D / G / realized gain kept separate as in Phase 5), then the lossy rerun, then the `parmap` width sweep, then parallel stepping with a serial-equivalence gate and a performance gate. The runtime target is revisited here with measured numbers (§4.6, §5.5).
- S21 is out of scope.

---

## 1. What exists and is reused unchanged

| Need | Already in 0.3.63 | Phase 6 use |
|---|---|---|
| Materials | `fdtd_media` (cell `eps_r`, `mu_r`, `sigma`, arithmetic-mean edge bake) | FR-4 slab `eps_r = 4.4` on the ground footprint only; `sigma` for the lossy run |
| Metal | `fdtd_pec(sim, "z", k, mask)` (node rule) | ground plane at node page `kg`, patch at `kg + nsub` |
| Absorber | `fdtd_cpml(sim, n)` | 8 cells, all six faces, defaults |
| Feed | `fdtd_port` (series lumped port, one per grid) | `ez` stack of `nsub` edges, ground → patch, at the first metal node of the centre row |
| S11 | `fdtd_s11(sim [, "nfft", n])`, decay check | dip frequency and depth, passivity |
| Pattern | `fdtd_ntff`, `fdtd_farfield` | D (flux), G (accepted), G_r (incident), `prad`, `paccepted` |
| Sweep | `parmap` (global rayon pool, vector returns pack into a matrix) | one `fdtd3d` per trial |
| Ctrl+C | `InterruptGuard` (process-wide depth counter under a mutex) | safe from several `parmap` workers at once |

The floating geometry passes `validate_ntff` as is: background read from cell (0,0,0) is air, the substrate cells are inside `[m_lo+1, m_hi−2]` on every axis, and every PEC / port edge has both end nodes in `[m_lo+1, m_hi−1]`. No NTFF code change is needed for the capstone (§4 runs confirm it).

The FDTD builtins are not on `IMPURE_BUILTINS` (`crates/rustlab-script/src/purity.rs`), so a `parmap` lambda may build and run a grid. They must stay off it.

## 2. Geometry and mesh

Coordinates: x along the patch length (resonant direction), y across the width, z normal. Storage is x-fastest, `idx = (k*ny + i)*nx + j`. Node `j` is at `x = j·dx` (script index `j+1`).

### 2.1 Physical design (unchanged from Phase 4)

| Quantity | Value | Source |
|---|---|---|
| Substrate | FR-4, `eps_r = 4.4`, `h = 1.6 mm`, lossless or `tanδ = 0.02` | parent plan |
| Design frequency | `f0 = 2.4 GHz` | parent plan |
| `W` | `c0/(2 f0)·sqrt(2/(eps_r+1))` = 38.0100 mm | TL model |
| `eps_eff`, `ΔL` | 4.0857, 0.7388 mm | Hammerstad |
| `L` | `c0/(2 f0 sqrt(eps_eff)) − 2ΔL` = 29.4216 mm | TL model |
| Ground and substrate | patch footprint + `gpad = 16 mm` (10 h) on every side, rounded to an even cell count: 62 × 70 mm at 1 mm, 60 × 72 mm at 2 mm | this brief |
| Substrate extent | exactly the ground footprint (flush edges), `nsub` cell pages above the ground node page | this brief |
| Feed | series lumped port, `Z0 = 50 Ω`, `ez` edges `kg+1 … kg+nsub` (script), row `iy` = centre row, column = first metal node of that row (edge feed, as Phase 4 item 10) | Phase 4 |
| Pulse | `"modulated"`, `tau = 1/(2 f0)`, `t0 = 4 tau`, `f0 = 2.4 GHz` | Phase 4 |

The script computes `L` and `W` and staircases them with `rect_mask` centred on a node, exactly as Phase 4 did. On 1 mm the metal is 28 × 38 mm (nodes 16…44 × 11…49 on the 60 mm Phase 4 grid). On 2 mm it is 28 × 36 mm.

**Symmetry rule (gate C9, measured §4.4).** The metal must span an even number of cells across y and the feed row must be its mirror plane. A 2 mm mesh with 38 mm metal (19 cells) puts the feed half a cell off the mirror plane, excites the orthogonal TM100 width mode near 1.85 GHz, and the port record no longer decays (tail 2.7e-3 after 8000 steps versus 5.4e-5 for the symmetric 40 mm metal). The example asserts the parity; a test pins the failure.

### 2.2 Box layout along each axis

From the low wall inward, then mirrored:

```text
CPML (npml) | margin (NTFF, cells) | air | ground/substrate span | air | margin | CPML
```

- `npml = 8`, CPML defaults (`m = 3`, `sigma_factor = 1`, `kappa_max = 1`, `alpha_max = 0`).
- `margin = 2` (the Phase 5 default). The Huygens box is the node plane `npml + margin` from each wall.
- Air from the substrate edge to the box: **`air_xy = 10 mm` laterally, `air_z = 20 mm` above the patch and below the ground.** §4.3 shows why z needs 20 mm and x/y does not.

`nx = ngx + 2(npml + margin + air_xy/dx)`, the same for y, and `nz = nsub + 2(npml + margin + air_z/dz)`. The ground node page is `kg = npml + margin + air_z/dz` (0-based).

### 2.3 The two meshes

| | Lesson mesh (example + notebook) | Reference mesh (gated release tests) |
|---|---|---|
| `dx = dy`, `dz` | 2 mm, 0.8 mm (`nsub = 2`) | 1 mm, 0.4 mm (`nsub = 4`, same as Phase 4) |
| Metal (staircased) | 28 × 36 mm | 28 × 38 mm |
| Grid | 60 × 66 × 72 = 285,120 cells | 102 × 110 × 124 = 1,391,280 cells |
| `fdtd_info.bytes` | 60 MB | 291 MB |
| `dt` | 2.2994 ps | 1.1497 ps |
| Steps (record ≈ 18 ns / 14 ns) | 8000 | 12000 |
| Cells per λ0 at 2.4 GHz | 62 (x, y) | 125 (x, y) |
| Cells per λ in FR-4 | 30 | 60 |
| Serial wall, one run, 4 NTFF freqs | 23.7 s | ≈ 185 s (90.4 Mcell/s, §4.6) |

The lesson mesh is the example and the notebook because the reference mesh does not fit the `run-examples` 300 s timeout serially (two runs ≈ 370 s) and CI renders every notebook. The reference mesh is the one compared with Phase 4 (same `dz`, same staircased metal, same feed rule). Both carry the same gates except where §5 gives a mesh-specific bound.

## 3. Expected physics and independent checks

`dev/plans/fdtd3d_phase6_impl/estimates.py` reproduces every closed-form number here (numpy only).

### 3.1 Resonance

- **Design:** 2.4000 GHz for the continuous 29.42 mm patch (TL model, fringing included). Cavity TM010 without fringing, same `L`: 2.4288 GHz.
- **Staircase-aware TL model ("TL+cell"):** the Yee tangential E at a metal edge sits half a cell outside the last metal node on each side, so the electrical length is the metal span plus one cell, plus `2ΔL`. For a span `L_m × W_m` on cell `d`: `f = c0 / (2 (L_m + d + 2ΔL(W_m + d)) sqrt(eps_eff(W_m + d)))`.

  | Mesh | Metal | TL+cell | Bare metal span (no cell) |
  |---|---|---|---|
  | 2 mm | 28 × 36 | 2.3559 GHz | — |
  | 1.5 mm | 27 × 36 | 2.4748 GHz | — |
  | 1 mm | 28 × 38 | 2.4314 GHz | 2.5158 GHz |
  | 0.5 mm | 28 × 38 | 2.4729 GHz | — |

  This is the independent check per mesh. It is a model of the staircase, so it is checked at every mesh level (C2), not just one.
- **Phase 4 reference:** 2.4395 GHz (edge feed, ground on the `zmin` wall, substrate to the CPML, 1 mm × 0.4 mm, 16000 steps, rectangular window, 8× pad). The floating ground changes the fringing at the substrate edge and the back side; the cavity is the same, so the dip should move by well under 1%.

### 3.2 Radiation Q, bandwidth, decay

Jackson–Alexopoulos with `L_eff × W_eff ≈ 29 × 39 mm` at 2.44 GHz: `Q_space = 64.0`, surface-wave efficiency `e_sw = 0.899`, `Q_rad = 57.6`. VSWR-2 bandwidth about `1/(sqrt2 Q) ≈ 1.2%`. The amplitude time constant `2Q/ω` is 7.5 ns unloaded and roughly half that with the 50 Ω port; reaching `1e-3` takes 26–52 ns of ring-down in the worst case. Measured: the edge-fed record is under `1e-3` after 14 ns (12000 steps at 1 mm), because the port loads the cavity.

### 3.3 Directivity

Two radiating slots over an **infinite** ground: 4.09 (6.11 dBi). A finite ground with ~λ/8 of overhang typically adds 0.5–1 dB at broadside and puts a back lobe 10–20 dB down. Expected broadside D on this ground: **6–8 dBi**. A short dipole (1.76 dBi) or a factor-2 normalisation error (+3 dB → ~10 dBi) falls outside the band.

Polarisation: the patch radiates like two y-directed magnetic currents separated along x. In the E-plane (`φ = 0`, xz) the co-polar component is `Eθ`. In the H-plane (`φ = 90°`, yz) it is `Eφ`. With the feed on the y mirror plane, `Eφ(θ, φ = 0)` is zero by symmetry, and so is the broadside cross-pol.

### 3.4 Efficiency

Lossless: radiated power `P_flux` equals the accepted power `P_acc` (no conductor or dielectric loss; the closed box encloses the substrate's surface waves, which diffract off the finite edge and radiate). Lossy, `tanδ = 0.02` modelled as `σ = 2π f0 eps_r ε0 tanδ = 0.011750 S/m` on the substrate cells (so `Q_d = 50.8` at 2.44 GHz):

`η = (1/Q_rad) / (1/Q_rad + p/Q_d)` with `p` the electric filling factor of the substrate. `p = 1` gives 0.469, `p = 0.85` gives 0.510. **Expected η ≈ 0.47–0.51**, gate band 0.40–0.60. D should not change with loss (the current distribution is the same mode); G = D + 10 log10 η, about 3 dB below D.

Independent checks on the efficiency:

1. `P_acc − P_flux` against the substrate dissipation `½ Σ σ |Ê|² ΔV` (the Phase 5 `dissipation_balance` method in `fdtd/ntff_tests.rs`, end-of-step DFT on every lossy edge). This is the gate (L3).
2. The Q-ratio from a port-free ring-down. **Measured to be unreliable here (§4.5)**; diagnostic only.

## 4. Prototype measurements

Scripts: `patch_core.rlab` (geometry and all checks, knobs at the top), `run_proto.sh` (named configs; `bash run_proto.sh` lists them), `sweep_proto.rlab`, `dipole_aniso.rlab`, `estimates.py`. Logs in `logs/`, named after the `run_proto.sh` config (plus `final_*` for the last reruns with the passivity line, `sweep_proto.log`, `dipole_aniso.log`; `coarse.log` / `conv2.log` are the 2 mm 28 × 38 asymmetric runs). Serial stepping, release binary 0.3.63. "Contended" timings ran 4–5 jobs at once; §4.6 is the clean timing.

### 4.1 Resonance and the Phase 4 comparison

Edge feed, lossless, `|S11|` minimum in 1.8–3.2 GHz with `"nfft", 8n`:

| Config | Mesh | Steps | V tail/peak | Dip | vs Phase 4 2.4395 | vs TL+cell | min \|S11\| |
|---|---|---|---|---|---|---|---|
| `ref_edge` | 1 mm, air 10/20 | 12000 | 4.8e-4 | 2.4372 GHz | −0.09% | +0.24% | 0.564 |
| `edge24k` | 1 mm, air 10/10 | 24000 | 2.9e-6 | 2.4372 GHz | −0.09% | +0.24% | 0.569 |
| `air5` | 1 mm, air 5/5 | 12000 | 5.4e-4 | 2.4372 GHz | | | 0.582 |
| `air20` | 1 mm, air 20/20 | 12000 | 4.8e-4 | 2.4372 GHz | | | 0.564 |
| `lesson0` | 2 mm, air 10/20 | 8000 | 1.7e-4 | 2.3783 GHz | | +0.95% | 0.610 |
| `mid15` | 1.5 mm, air 10/10 | 12000 | 5.4e-5 | 2.4892 GHz | | +0.58% | 0.587 |
| `conv05` | 0.5 mm, air 5/5, metal 28 × 38 | 24000 | 6.6e-4 | 2.4644 GHz | | −0.34% | 0.571 |

Passivity (`logs/final_ref_edge.log`): `max |S11|` over 1.5–3.5 GHz is 1.0270 when the reference record is cut at 4000 steps (tail 0.10), 1.0012 at 6000, 0.9989 at 8000, and 0.9977 at 12000. A truncated record leaks the cut into the spectrum and reads above 1. C4 therefore only applies to a record that passed the decay check. A test pins the truncated reading so nobody "fixes" it by clipping (Phase 4 already decided not to clip `|S11|`).

On the edge24k record the dip wanders 2.4354–2.4395 GHz (±2 MHz) as the record is cut at 12k…24k steps. That is the zero-pad interpolation of a decayed record, and it sets the tolerance floor for frequency gates (§5).

Inset feed (`inset30k`, feed column at `L/3`, otherwise as `edge24k`): dip 2.3992 GHz, the Phase 4 inset value to the digit, `|S11|` 0.01–0.05 (−26 to −38 dB, not stable as a number). It needs ~20000 steps to reach `1e-3`. The capstone keeps the edge feed: it decays in 12000 steps, it is the Phase 4 item-10 reference, and its mismatch makes realized gain visibly different from gain (5.27 vs 6.93 dBi), which is the D / G / G_r lesson.

### 4.2 Convergence

Fixed physical metal 28 × 38 mm where the mesh allows it, otherwise the staircased design:

| Δxy | dz | nsub | Metal | Dip | TL+cell | err |
|---|---|---|---|---|---|---|
| 2 mm | 0.8 mm | 2 | 28 × 36 | 2.3783 | 2.3559 | +0.95% |
| 2 mm | 0.8 mm | 2 | 28 × 40 | 2.3647 | 2.3493 | +0.66% |
| 1.5 mm | 0.6 mm | 3 | 27 × 36 | 2.4892 | 2.4748 | +0.58% |
| 1 mm | 0.4 mm | 4 | 28 × 38 | 2.4372 | 2.4314 | +0.24% |
| 0.5 mm | 0.2 mm | 8 | 28 × 38 | 2.4644 | 2.4729 | −0.34% |

- The TL+cell residual shrinks with Δ. Every level is within 1%.
- First-order Richardson in Δ on the fixed-metal pair (1 mm, 0.5 mm): `f∞ = 2·2.4644 − 2.4372 = 2.4916 GHz`, 0.96% below the bare-span TL value 2.5158 GHz (the continuum limit of a 28 mm patch). The resonance converges to the right continuum answer; the grid-to-grid change is the staircase, not dispersion error.
- Directivity barely moves: 6.94 dBi on both the lesson mesh and the reference mesh at `air_z = 20 mm` (7.00 / 7.04 / 7.01 at 2 / 1 / 1.5 mm with `air_z = 10 mm`).
- 0.5 mm cost: 2.27M cells (475 MB), 24000 steps, 699 s contended. Not a CI test. With parallel stepping it is a recorded manual run (§9).

### 4.3 Pattern, power balance, and the air-padding finding

At the armed frequency nearest the dip (2.44 GHz on 1 mm, 2.38 GHz on 2 mm). `Psph` is the trapezoid integral of `U` over a 61 θ × 120 φ sphere (3° steps), the Phase 5 G7 check.

| Config | air xy / z | D(θ=0) | (Pacc−Pflux)/Pacc | Psph/Pflux − 1 | front/back |
|---|---|---|---|---|---|
| `air5` | 5 / 5 mm | 7.23 dBi | 1.6e-3 | **+5.76e-2** | 13.4 dB |
| `air10` | 10 / 10 mm | 7.04 dBi | 1.6e-3 | **+2.01e-2** | 13.8 dB |
| `airxy20` | 20 / 10 mm | 7.04 dBi | 1.6e-3 | **+2.01e-2** | 13.8 dB |
| `airz20` | 10 / 20 mm | 6.94 dBi | 1.6e-3 | +1.1e-3 | 14.0 dB |
| `air20` | 20 / 20 mm | 6.94 dBi | 1.6e-3 | +1.1e-3 | 14.0 dB |
| `lesson0` | 10 / 20 mm (2 mm mesh) | 6.94 dBi | 1.1e-3 | +3e-4 | 13.0 dB |

- Power balance is ~1e-3 everywhere and does not depend on the padding. The flux, and so D, G, and G_r through it, are consistent with the port.
- **The pattern integral overshoots the flux when the top/bottom Huygens faces are within 10 mm (λ0/12) of the patch or ground.** Lateral padding has no effect (`airxy20` equals `air10` to the digit; `airz20` equals `air20`). Because D is normalised by the flux, the far-field magnitudes are what is off: D reads 0.10 dB high at `air_z = 10 mm` and 0.29 dB high at 5 mm.
- It is not the anisotropic cell. `dipole_aniso.rlab` repeats the Phase 5 Hertzian dipole with `dz = 0.4 dx`: `Psph/Pflux − 1` is −2.3e-4 (cubic), +7.0e-4 (`ez`, anisotropic), −1.7e-4 (`ex`, anisotropic).
- Likely cause: the z faces sample the reactive fringing field above the slots on a 1 mm × 1 mm lattice. Its evanescent, short-transverse-wavelength content is not resolved by the surface sampling. The error falls off faster than a power law (5.8% → 2.0% → 0.1% for 5 → 10 → 20 mm), as an evanescent tail would. Root cause is not proven; §11 R1.
- Shape: E-plane D(θ) at 0/15/30/45/60/75/90° is 6.9 / 6.6 / 5.6 / 4.0 / 2.0 / −0.4 / −3.3 dBi; H-plane 6.9 / 6.5 / 5.3 / 3.3 / 0.8 / −2.2 / −5.2 dBi (`air20`). Both cuts peak at θ = 0, and the H-plane is narrower than the E-plane, as for a patch. Broadside cross-pol (`|Eφ/Eθ|` at θ = 0, φ = 0) is ~1e-17: exact by symmetry.
- Lossless gain: G − D ≤ 0.01 dB. Realized gain at 2.44 GHz is 5.27 dBi, so `1 − |S11|² = 0.682` (|S11| = 0.564 at the dip).

### 4.4 Lossy rerun (`tanδ = 0.02`)

| Config | Dip | min \|S11\| | D(0) | G(0) | G_r(0) | η = Pflux/Pacc |
|---|---|---|---|---|---|---|
| `lossy24k` (1 mm, air 10/10), 2.44 GHz | 2.4372 GHz | 0.302 | 7.04 dBi | 4.11 dBi | 3.69 dBi | 0.510 |
| `ref_lossy` (1 mm, air 10/20), 2.40 GHz | 2.4372 GHz | 0.299 | 6.88 dBi | 3.99 dBi | 3.29 dBi | 0.514 |
| `lesson1` (2 mm, air 10/20), 2.38 GHz | 2.3783 GHz | 0.340 | 6.94 dBi | 3.83 dBi | 3.30 dBi | 0.489 |

η is flat across the armed frequencies (0.513–0.514 on `ref_lossy` from 2.34 to 2.40 GHz). The `ref_*` runs armed 2.34:0.02:2.40 GHz, below the 1 mm dip. The reference test must arm 2.40:0.02:2.46 GHz (as `air20` did) so one armed frequency is within 10 MHz of the dip.

- η falls inside the closed-form band 0.469–0.510.
- D is unchanged by the loss to 0.01 dB.
- The dip frequency does not move. The dip deepens because the loss resistance pulls the edge impedance toward 50 Ω.
- The lossy record decays faster (tail 1.7e-5 at 8000 steps on 2 mm).

### 4.5 Q from a port-free ring-down (diagnostic, not a gate)

`qring0` / `qring1` use a soft `ez` source at the feed and an `Ez` probe under the far radiating edge, fit a single exponential to the envelope of the last 60% of 16000 steps:

- Fitted Q: 83.3 lossless, 28.9 lossy, so `Q_lossy/Q_lossless = 0.35`.
- Power balance on the same geometry gives η = 0.51, and the closed form gives 0.47–0.51.

The single-exponential fit picks up slower components (the record is not one mode; the port-loaded fits also moved from Q 12 to 18 with the window). Recommendation: report Q only as a teaching diagnostic, never gate on it. If a Q check is wanted later, fit the S11 bandwidth or use a matrix-pencil fit, not an envelope.

### 4.6 Runtime (clean, one job at a time)

| Run | Cells | NTFF freqs | Mcell/s | Wall |
|---|---|---|---|---|
| `bench_fdtd gate` (60×60×20 vacuum, CPML six faces, 8000 steps) | 72,000 | 0 | 124.2 | 4.64 s |
| Reference mesh, 2000 steps | 1,391,280 | 0 | 124.3 | 22.4 s |
| Reference mesh, 2000 steps | 1,391,280 | 4 | 90.4 | 30.8 s |
| Lesson mesh, 8000 steps | 285,120 | 0 | 123.2 | 18.5 s |
| Lesson mesh, 8000 steps | 285,120 | 4 | 96.1 | 23.7 s |
| `sweep_proto.rlab` (5 widths, lesson mesh, `parmap`, no NTFF) | 5 × 285k | 0 | — | 43.4 s (38.2 s on the logged rerun) |
| `final_ref_edge` / `final_ref_lossy`, 12000 steps, NTFF 4 (contended: 4 jobs) | 1,391,280 | 4 | 76.3 | 218.8 s each |
| `final_lesson0` / `final_lesson1`, 8000 steps, NTFF 4 (contended) | 285,120 | 4 | 95–98 | 23–24 s each |

- The kernel is not cache-bound here: 124 Mcell/s on 72k cells and on 1.39M cells.
- NTFF accumulation with 4 frequencies costs 22–27% serially. It is serial per step today, so under a parallel kernel it becomes the Amdahl term unless it is parallelised too (§7.3).
- Projections, serial: one 12000-step reference run is ≈ 185 s; the reference lossless + lossy pair is ≈ 370 s. The lesson example (two 8000-step runs plus patterns) is ≈ 50–55 s.
- Aggregate throughput of 4–5 independent concurrent jobs on this VM reached ~3.4× one serial job's cell rate. That is the ceiling a parallel kernel can approach here, so a ≥ 2× speedup at 4 threads is a fair gate (R2).

### 4.7 Width sweep (`sweep_proto.rlab`)

Lesson mesh, metal length fixed at 28 mm, widths 28:4:44 mm (even cell counts), 8000 steps, `parmap` returns `[f_res, min|S11|]` per trial:

| W | f_res | TL+cell | err | min \|S11\| |
|---|---|---|---|---|
| 28 mm | 2.3987 | 2.3733 | +1.07% | 0.729 |
| 32 mm | 2.3851 | 2.3638 | +0.90% | 0.672 |
| 36 mm | 2.3783 | 2.3559 | +0.95% | 0.610 |
| 40 mm | 2.3647 | 2.3493 | +0.66% | 0.545 |
| 44 mm | 2.3579 | 2.3436 | +0.61% | 0.475 |

- Monotone in both columns. The measured span is −40.8 MHz against −29.7 MHz in the model (1.37×).
- The W = 36 mm trial reproduces the single lesson run to the digit.
- At 6000 steps the 44 mm trial failed the decay check (tail 1.28e-3). The check worked; the example uses 8000.
- `parmap` of `fdtd3d` works today on the serial kernel.

## 5. Gates

"Ref" = reference mesh, release `#[ignore]` test. "Les" = lesson mesh: the example asserts it with `error()`, and a release `#[ignore]` test pins it. Measured values are from §4.

### 5.1 Capstone, lossless (C)

| ID | Check | Tolerance | Measured |
|---|---|---|---|
| C1 | Ref dip against the Phase 4 edge-feed reference 2.4395 GHz | ≤ 1% | −0.09% |
| C1b | Dip within 5% of the 2.4 GHz design (Phase 4 G6) | Ref and Les | +1.55% / −0.91% |
| C2 | Dip against TL+cell for that mesh's metal | ≤ 1.5% | +0.24% / +0.95% |
| C3 | Port V and I tail/peak (last 10%) and the NTFF surface decay (`"decay", "pulse"`, the default) | ≤ 1e-3 | 4.8e-4 / 1.7e-4 |
| C4 | Passivity: `max |S11|` over 1.5–3.5 GHz, on a record that passed C3 | ≤ 1.0 | 0.9977 / 0.9978 (lossy 0.9898 / 0.9894) |
| C5 | Power balance `|Pacc − Pflux|/Pacc` at every armed frequency | ≤ 1e-2 | 1.1–1.6e-3 |
| C6 | Pattern integral `|Psph/Pflux − 1|`, 61 θ × 120 φ | ≤ 5e-3 with `air_z ≥ 20 mm` | 1.1e-3 / 3e-4 |
| C7 | Broadside: the sphere-grid maximum of D is at θ = 0; the E-plane `|Eθ|` and H-plane `|Eφ|` cut maxima are at θ = 0 | one grid step (3°) | θ = 0 |
| C8 | D(θ = 0) | 6.0–8.0 dBi | 6.94 / 6.94 |
| C9 | Symmetry: `|Eφ(0, 0)|/|Eθ(0, 0)|` and `|D(θ, 90°) − D(θ, 270°)|/D` | ≤ 1e-6 | ~1e-17 |
| C10 | Front/back `D(0)/D(180°)` | ≥ 10 dB | 14.0 / 13.0 |
| C11 | Lossless `|G − D|`; `G_r − G = 10 log10(1 − |S11(f)|²)` with S11 from the far-field struct | ≤ 0.05 dB; ≤ 1e-6 relative | 0.01 dB; by construction |

C6 is the gate that would have caught `air_z = 10 mm`. A companion diagnostic test (not a gate, `#[ignore]`) records the 2% at 10 mm, so a future NTFF change that fixes it shows up as a change and is not silently absorbed.

### 5.2 Lossy rerun (L)

| ID | Check | Tolerance | Measured |
|---|---|---|---|
| L1 | Radiation efficiency `η = Pflux/Pacc` at the armed frequency nearest the dip | 0.40–0.60 | 0.514 (1 mm), 0.489 (2 mm) |
| L2 | `G − D = 10 log10 η` | ≤ 0.01 dB | by construction |
| L3 | `(Pacc − Pflux)` against `½ Σ σ |Ê|² ΔV` over the substrate edges (end-of-step DFT, material σ only, not the port's gap σ) | ≤ 1e-2 relative | not measured (needs the Rust harness; Phase 5 got 1.4e-3 at 1 mm on a cube) |
| L4 | Lossy dip within 0.5% of the lossless dip; lossy `min |S11|` < lossless | | same bin; 0.302 < 0.569 |
| L5 | `|D_lossy − D_lossless|` at θ = 0 | ≤ 0.05 dB | 0.00 dB |

### 5.3 Width sweep (S)

| ID | Check | Tolerance | Measured |
|---|---|---|---|
| S1 | `parmap` over 5 widths returns a 5 × 2 matrix `[f_res, min|S11|]` | exact shape | 5 × 2 |
| S2 | `f_res` strictly decreasing in W | — | yes, steps 7–14 MHz |
| S3 | Each `f_res` against TL+cell | ≤ 1.5% | +0.61…+1.07% |
| S4 | Span `f(44) − f(28)` against the TL+cell span | 0.5–2.0× | 1.37× |
| S5 | `min |S11|` strictly decreasing in W | — | yes |
| S6 | `parmap` result equals the `arrayfun` result bit for bit, serial and parallel kernel | exact | not run (release test) |
| S7 | One pool: inside a `parmap` worker the kernel's parallel path runs on the global pool (`rayon::current_thread_index().is_some()`, `current_num_threads()` unchanged), and no `ThreadPoolBuilder` is built in `fdtd/` | exact | design rule |

### 5.4 Parallel stepping (P)

| ID | Check |
|---|---|
| P1 | Parallel equals serial **bit for bit** (`f64::to_bits` on all six fields, every CPML `q` array, the port `V`/`I` record, probe record, every NTFF phasor and the surface decay trace, and `far_field` outputs) at threshold 0, for pool sizes 1, 2, 3, 4 (each test builds a private `ThreadPool` and `install`s it; production code never does). Fixtures: vacuum; lossy dielectric + `mu_r`; PEC volume and faces; CPML on six faces; soft and hard sources; one port; NTFF armed with 3 frequencies; a floating patch. |
| P2 | The Phase 3 split-run identity holds on the parallel path: `run(150)` equals `run(70); run(80)`. |
| P3 | Below the threshold (`nx·ny·nz < 32,768`) the stepper never enters rayon (a test-only counter). |
| P4 | `fdtd3d(..., "threads", 1)` forces serial at any size (counter) and the bits match P1. |
| P5 | Ctrl+C / notebook cancel still stops between steps; the stop callback is polled on the calling thread between steps, never inside a parallel region. |

Bit-identity is achievable, and required, because every output element is computed by the same expression on the same inputs in the same order. Rows of one half-step are independent (each writes its own row of one field and reads only the other field), the NTFF accumulators are per-sample, and the decay trace takes a `max`, which is order-independent. There is no reduction across rows in the step. If an implementation needs one (it should not), it must use a fixed chunk order, and the P1 test is the referee. A tolerance-based gate is not accepted.

### 5.5 Performance (R) — revised targets

The parent's gate (60×60×20 × 8000 steps under 60 s) is already met serially at 4.64 s and stays as a regression check. The meaningful targets are on the meshes Phase 6 actually runs:

| ID | Check | Gate | Serial today |
|---|---|---|---|
| R1 | Parent gate, release | < 60 s (record parallel) | 4.64 s |
| R2 | Reference mesh, 2000 steps, NTFF 4 freqs: speedup at 4 threads | ≥ 2.0× (target 2.5×) | 90.4 Mcell/s |
| R3 | Reference lossless capstone run (12000 steps, NTFF 4) wall | ≤ 100 s | ≈ 185 s projected |
| R4 | `patch_antenna.rlab` (lesson mesh, both runs, patterns) | ≤ 60 s serial, ≤ 35 s parallel | ≈ 50–55 s |
| R5 | `fdtd_sweep.rlab` (5 widths) | ≤ 90 s | 43.4 s |
| R6 | NTFF share of a parallel step on the reference mesh | ≤ 30% | 27% serial |

These are run by hand in release and recorded in the PR (`bench_fdtd` gains a `patch` mode, §7.5), not `#[test]` wall-clock asserts. If R2 misses, the order to try is the parent's: confirm unit stride, confirm autovectorisation, chunk size, then the threshold. No new crate.

## 6. API

**No new physics builtin.** The capstone, the lossy rerun, and the sweep run on 0.3.63 builtins (§4 is the proof).

One addition, from the parent plan:

```text
sim = fdtd3d(nx, ny, nz, dx, dy, dz, "threads", 1)   % force serial stepping
sim = fdtd3d(nx, ny, nz, dx, dy, dz, "threads", 0)   % default: shared pool above the threshold
```

- Name-value pair, composable with `"courant"` in either order (same parser).
- v1 accepts 0 and 1 only. Any other value is an error naming the two. Sizing the pool per grid would need a private `ThreadPool`, which the parent forbids.
- `fdtd_info(sim).threads` reports the setting (0 or 1).
- `fdtd_run` output is unchanged.
- Docs, `HelpEntry` detail for `fdtd3d` and `fdtd_info`, `docs/functions.md`, `docs/quickref.md`, the `AGENTS.md` table and the `fdtd3d` Active Plans row, `CHANGELOG.md` under 0.3.64 (*Added*: `"threads"`; *Behavior*: parallel stepping above 32,768 cells, results bit-identical). It ships in the same commit as the parser (Workflow Rule 3).

Deliberately **not** added:

- **`fdtd_reset`.** The parent's lossy step reset the grid and rebaked. Rebuilding is simpler, MATLAB-like, and has no edge cases with the armed NTFF, the port record, or `dt` changing on rebake. The example builds both runs through one script function `sim = patch_sim(sigma)`. §11 Q1 asks Michael to confirm dropping it.
- **An `efficiency` field on the far-field struct.** `pat.prad ./ pat.paccepted` is one line, and Phase 5 kept the three normalisations separate on purpose.
- **A patch helper builtin.** Geometry stays in the script, where students read it.

## 7. Parallel stepping design

### 7.1 Kernel

- Refactor each half-step into a per-row function `h_row(k, i, …)` / `e_row(k, i, …)` that the serial loop and the parallel driver both call. One body, two drivers, so identity is structural.
- Parallel driver: split each written field array into contiguous chunks of whole rows with `par_chunks_mut(nx * rows_per_task)` and zip the three components (`hx`, `hy`, `hz` together; then `ex`, `ey`, `ez`). The read-only arrays are shared slices. A chunk is a run of `(k, i)` rows; the row index is recovered from the chunk offset.
- Granularity: contiguous runs of whole rows, about 64k cells per task (`65_536 / nx` rows). A 16k target left the 4-thread reference step at 1.82×; 64k reached 1.89×. The chunk size does not change the bits. CPML stays serial.
- The `ERow` / `HRow` arm (vacuum / lossless / lossy / scaled) is chosen per row exactly as now.
- **CPML correction** (`correct_h` / `correct_e`): same per-row split over the slab rows. Each row owns its `q` (ψ) entries. Parallelise only if R2/R6 profiling shows it matters; it touches the slabs only.
- Sources, the lumped port, probes, and `record_port`: serial, between the parallel regions (a handful of edges).
- Threshold: parallel only when `nx·ny·nz ≥ 32,768` and `threads != 1`. Test override `__test_set_par_threshold` (same idea as `vector_calc`).

### 7.2 Pool

Global rayon pool only (`par_chunks_mut`, `join`). No `ThreadPoolBuilder`, no `std::thread::scope`, no thread per step. Inside a `parmap` worker the same calls nest on the same pool (work stealing), so a sweep does not create a second pool (S7). A trial that wants one core per trial passes `"threads", 1`; the sweep example does **not**, and R5 records both.

### 7.3 NTFF

- `accumulate`: the per-step loop over `freqs × samples` writes one accumulator per (frequency, sample). Parallelise over samples in fixed chunks; per-sample arithmetic is unchanged, so bits are unchanged.
- The decay trace takes a max over the surface, which is order-independent: compute per-chunk maxima, then the max of those.
- `far_field`: parallel over directions (each direction's sum over samples stays a sequential loop in sample order). `prad` stays a single sequential sum.
- The `drivers_inside` O(cells) PEC scan runs once at arm time; leave it serial.

### 7.4 What must not change

Step order (H, H-CPML, H probes, E, E-CPML, sources + port at `steps·dt`, E probes, `record_port`, `accumulate_ntff`), `dt`, the bake, the record layout, every error message, and the bits.

### 7.5 Benchmark

`crates/rustlab-dsp/examples/bench_fdtd.rs` gains a `patch` mode: builds the reference mesh in Rust (same geometry as `patch_core.rlab`), runs 2000 steps with NTFF 4 frequencies serial (`threads = 1`) and parallel, and prints both rates and the ratio. The existing `gate` mode stays.

## 8. TDD test list (write each test first and watch it fail)

Tests that pass on 0.3.63 the moment they are written are characterisation tests. They pin today's physics before the code around them moves. They are marked (char).

**Stage A — serial capstone** (`crates/rustlab-dsp/src/fdtd/patch_tests.rs`, new module, `mod patch_tests` in `fdtd/mod.rs` under `#[cfg(test)]`; a shared `patch_sim(PatchSpec)` builder mirrors `patch_core.rlab`):

1. (char) `floating_patch_box_passes_ntff_validation`: tiny coarse floating patch (e.g. 4 mm cells, 2 cells of air); `set_ntff` + one step succeed. Paired negative `ground_touching_inner_layer_is_refused` (air 0: the PEC lies on the inner layer) asserts the existing error text.
2. (char) `patch_feed_on_mirror_plane_has_no_cross_pol`: same tiny mesh, a few hundred steps, `|Eφ(0,0)| ≤ 1e-6 |Eθ(0,0)|` with `"decay", "off"`. Default suite (debug-fast).
3. (char, `#[ignore]` release) `lesson_patch_gates`: C1b–C11 on the lesson mesh (~25 s release).
4. (char, `#[ignore]` release) `reference_patch_against_phase4`: C1–C11 on the reference mesh (~3 min serial; ~1.5 min after Stage D).
4b. (char, `#[ignore]` release) `truncated_patch_record_is_not_passive`: the reference mesh cut at 4000 steps reads `max |S11| > 1` and `fdtd_s11(sim)` refuses it; at 12000 steps it reads ≤ 1 (C4).
5. (char, `#[ignore]` release) `odd_width_patch_rings_in_the_width_mode`: 2 mm, metal 28 × 38, tail after 8000 steps > 1e-3 while 28 × 40 is < 1e-4. Locks the §2.1 symmetry rule.
6. (char, `#[ignore]` release, diagnostic) `air_z_10mm_pattern_integral_overshoots`: records `Psph/Pflux − 1 > 1e-2` at `air_z = 10 mm`. Its doc comment says it is a known limitation, not a requirement.
7. Script level, `crates/rustlab-script/tests/fdtd.rs`: (char) `floating_patch_script_runs` (tiny mesh through the builtins, struct fields present).

**Stage B — lossy rerun:**

8. `substrate_loss_balances_accepted_minus_flux` (`#[ignore]` release, lesson mesh): L1, L3, L4, L5. Reuses `lossy_edges` / `sim_field` / the end-of-step DFT from `ntff_tests.rs`. Move them to a shared `fdtd/test_util.rs` first, in its own commit, with no behaviour change. Expected to fail first only if L3's bookkeeping is wrong (port σ included, missing ½); write it with the port edges deliberately included once to see it fail, then exclude them.

**Stage C — sweep:**

9. Script test `parmap_patch_sweep_shape_and_order` (tiny mesh, 2 widths, short run, `"decay"` irrelevant: return `[steps, max |V|]` style scalars): S1 shape, S6 `parmap == arrayfun` bits. Default suite.
10. (`#[ignore]` release) `lesson_width_sweep_gates`: S2–S5 through the script layer (`rustlab_script::run` of the sweep body).

**Stage D — parallel** (fails first: the parallel driver does not exist):

11. `parallel_step_matches_serial_bitwise` (P1). Threshold forced to 0, pools of 1–4 threads, every fixture listed in P1, 200 steps each, debug-fast sizes (≤ 24³). Must fail on the first run because the parallel path is absent (assert the counter is > 0).
12. `parallel_split_run_identity` (P2).
13. `small_grid_stays_serial` (P3), `threads_one_forces_serial` (P4): counter-based.
14. `parallel_run_stops_between_steps` (P5): stop callback returns true at step 5; exactly 5 steps ran, fields equal the serial 5-step fields.
15. `ntff_accumulate_parallel_matches_serial` and `far_field_parallel_matches_serial`: bits.
16. Script: `fdtd3d_threads_option` (accepts 0 / 1 with `"courant"` in either order, rejects 2, `fdtd_info.threads`), and `help_coverage_tests` stay green.
17. (`#[ignore]` release) `parmap_sweep_parallel_kernel_matches_threads_one`: S6 with the kernel parallel inside `parmap`.

`cargo test --workspace` in debug must stay within today's CI budget. Everything above the debug-fast sizes is `#[ignore = "release: cargo test --release -p rustlab-dsp -- --ignored <name>"]`, as `cpml_reflection_hello_geometry` already is.

## 9. Examples and notebooks

Follow `docs/agent-guide.md` § Script style for agents: flat script, comments that say what the physics is, `error()` on a failed gate with the measured number in the message, no `break`/`continue`.

**`examples/pde/patch_antenna.rlab`** (lesson mesh):

1. Design: TL `W`, `L`, `eps_eff`, `ΔL`. Print them and the TL+cell prediction for the staircased metal.
2. `function sim = patch_sim(sigma)`: builds the floating ground, substrate, patch, CPML, port, NTFF (4 frequencies bracketing the expected dip, e.g. 2.34:0.02:2.40 GHz on 2 mm). Asserts the even-width / mirror-feed rule.
3. Lossless run. The port tail is under `1e-3` at 8000 steps. The near-to-far surface is not: on the lesson mesh `|E|` tail/peak is `1.886e-3` at 8000, `1.027e-3` at 14000, and `9.501e-4` at 16000. On the 1 mm mesh it is `3.603e-3` at 12000 and `8.213e-4` at 48000. The example runs 16000 steps, and the reference release test runs 48000, so `fdtd_farfield` can keep `"decay", "pulse"`. `fdtd_s11` uses `"nfft", 8*n`. Dip and `|S11|` in dB.
4. `fdtd_farfield` on a 61 × 120 sphere at the armed frequency nearest the dip: D / G / G_r at broadside, the power balance, the pattern integral, the E- and H-plane cuts, front/back.
5. Lossy run, same function with `sigma = 2*pi*f0*eps_r*eps0*0.02`: η, G = D + 10 log10 η.
6. Gates C1b, C2, C3, C5–C10, L1, L4, L5 as `error()` checks. Print one summary table.
7. Plots after the numbers: `|S11|` dB lossless and lossy on one axis; E/H-plane cuts in dB (or `polar` in linear). `savefig` only in the notebook.

**`examples/notebooks/patch_antenna.md`:** design and staircase (with the TL+cell table), geometry (`imagesc` of the ground and patch masks, a side-view sketch in prose), the run, S11, pattern cuts, the D / G / G_r table and what each normalisation means, power balance as the honesty check, the lossy rerun and η against the closed form, then a **recorded** (not executed) convergence and air-padding table from §4.2 / §4.3 labelled with the commit and machine. Close with "what this mesh cannot do" (staircase, the `air_z` finding, no finite-conductivity copper, no coax probe).

**`examples/pde/fdtd_sweep.rlab` + `examples/notebooks/fdtd_sweep.md`:** widths 28:4:44 mm on the lesson mesh, `R = parmap(@(w) patch_res(w), widths)` returning `[f_res, min|S11|]`, plotted after `parmap` returns (f_res vs W with the TL+cell curve; min |S11| vs W). Gates S1–S5. The notebook explains why the edge resistance falls as W grows (wider radiating slots, more conductance) and why each trial must build its own `fdtd3d` (a captured handle shares one grid). It also says `"threads", 1` is the one-core-per-trial option.

Both scripts pass `make examples` (`dev/run-examples.sh`, 300 s timeout) and both notebooks pass `rustlab-notebook check --strict` and `render --strict` in html, markdown, and latex. Commit the gallery outputs per the notebook rules.

Manual, recorded in the PR, not CI: `bash dev/plans/fdtd3d_phase6_impl/run_proto.sh conv05` (0.5 mm level) and `ref_edge ref_lossy` after Stage D, with wall times.

## 10. Staged commit order (one PR, 0.3.64)

1. Test utilities move (`fdtd/test_util.rs`), no behaviour change.
2. Stage A tests (1–7) + `patch_sim` Rust builder. Version bump to 0.3.64 in `Cargo.toml` / `Cargo.lock` (the examples are new observable output), `CHANGELOG.md` 0.3.64 section opened.
3. `examples/pde/patch_antenna.rlab` lossless half + notebook draft; `make examples` green.
4. Stage B test (8) + lossy half of the example and notebook.
5. Stage C tests (9–10) + `fdtd_sweep.rlab` + `fdtd_sweep.md`.
6. Stage D tests (11–15), written against the serial code, failing.
7. Parallel driver for `update_h` / `update_e` (+ threshold, test override, counter). Tests 11–14 pass.
8. Parallel NTFF accumulate / far field (test 15).
9. CPML correction parallel, only if R6 / profiling says so.
10. `"threads"` option, `fdtd_info.threads`, docs, help, quickref, AGENTS.md, CHANGELOG (test 16).
11. `bench_fdtd patch` mode; R1–R6 recorded in the PR body; test 17; the gallery render.
12. `dev/plans/fdtd3d.md` and the AGENTS.md Active Plans row: Phase 6 shipped in provisional 0.3.64.

Each commit builds and passes `cargo test --workspace`. Commits 2–5 are serial-only; the parallel path does not exist until 7, so Stages A–C are measured on the code Phases 1–5 shipped.

## 11. Open risks and questions

- **R1 — near-field sampling on the z faces.** With the box 10 mm (λ0/12) above the patch, the transformed pattern integrates 2% above the flux (5.8% at 5 mm). The capstone avoids it with `air_z = 20 mm` (C6). The root cause is not proven. If it is a transform defect that only shows with strong reactive fields (rather than unresolved evanescent content), it also affects Phase 5 users who put the box close. A follow-up worth a separate issue: repeat with `dx = dy = 0.5 mm` at `air_z = 10 mm`. If the error drops roughly 4×, it is sampling; if it stays, look at the transform.
- **R2 — parallel speedup on a 4-vCPU VM.** The concurrency ceiling measured here is ~3.4× aggregate. Rayon's per-step fork/join overhead on 1.39M cells is small, but on the 285k lesson mesh it is ~2–4 ms per half-step region at 12 regions per step (if CPML and NTFF are separate regions). R4's parallel bound may need region fusion; measure before optimising.
- **R3 — CI time.** Every physics gate is `#[ignore]` release. If nobody runs them, regressions slip. Recommendation: a `make fdtd-gates` target that runs `cargo test --release -p rustlab-dsp -- --ignored patch_ lesson_ reference_` and is listed in the PR checklist.
- **R4 — notebook render time in CI.** `validate-notebooks` executes every notebook. `patch_antenna.md` is ~55 s serial and `fdtd_sweep.md` ~45 s on 4 vCPU. That is acceptable but not free. If CI runners are slower, use the notebook cache, or set `"threads", 0` (default) after Stage D.
- **R5 — dip-frequency jitter.** The zero-pad estimate of the dip moves ±2–4 MHz with record length. Gate tolerances (C1 1%, S2 strict monotone with ≥ 7 MHz steps) sit above it, but S2's smallest step (40 → 44 mm, 6.8 MHz) is close. If it flakes, widen the sweep step to 6 mm (28:6:46) rather than loosening the check.
- **R6 — memory.** Reference mesh 291 MB, 0.5 mm level 475 MB per grid. A 5-trial `parmap` of reference meshes would be 1.5 GB; the sweep stays on the lesson mesh (60 MB per trial).
- **Q1 (Michael):** drop `fdtd_reset` from the plan in favour of rebuilding (§6)?
- **Q2 (Michael):** lesson mesh (2 mm) for the student-facing example and notebook, with the 1 mm reference behind release tests — or 1 mm in the example once Stage D makes it ~2 × 80 s?
- **Q3 (Michael):** edge feed for the capstone (decays in 12k steps, matches the Phase 4 reference, shows G_r ≠ G), with the inset feed only mentioned in the notebook as the matched alternative?

## 12. Validation commands (implementation PR)

```sh
export LIBRARY_PATH=/usr/lib/gcc/x86_64-linux-gnu/13   # cloud VM link path
cargo +stable fmt --all -- --check
cargo +stable build --locked --release --bin rustlab --bin rustlab-notebook
cargo +stable test --locked --workspace
cargo +stable test --locked --release -p rustlab-dsp -- --ignored patch_ lesson_ reference_
cargo +stable run --locked --release --example bench_fdtd -p rustlab-dsp -- gate
cargo +stable run --locked --release --example bench_fdtd -p rustlab-dsp -- patch
RUSTLAB=./target/release/rustlab bash dev/run-examples.sh
./target/release/rustlab-notebook check --strict examples/notebooks/patch_antenna.md examples/notebooks/fdtd_sweep.md
./target/release/rustlab-notebook render --strict examples/notebooks/patch_antenna.md -o /tmp/patch.html
./target/release/rustlab-notebook render --strict examples/notebooks/fdtd_sweep.md -o /tmp/sweep.html
```

The toolchain on the cloud image is 1.83; the workspace needs ≥ 1.89 (edition 2024 dependencies), hence `+stable`. Building `rustlab-plot` needs `libfontconfig1-dev libfreetype-dev pkg-config`.

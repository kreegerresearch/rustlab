# FDTD Phase 5 — near-to-far transform: implementation brief

**Status:** plan only. No runtime code is in this branch. It is written for the implementation run that follows. Michael approved starting Phase 5 (2026-11-07); Phase 6 (patch capstone, sweep, parallel stepping) and S21 stay out.

**Depends on:** draft PR #142 (`cursor/fdtd-lumped-port-a607`, Phase 4 lumped port, 0.3.59), which has **not** merged. This brief's branch (`cursor/fdtd-ntff-phase5-plan-4fbd`) starts from #142's head `874c4ab`, and its draft PR is based on #142's branch so the diff shows only Phase 5. If #142 changes, rebase this branch onto it; do not edit #142's branch from here. Keep every Phase 4 gate and fix (independent-load S11, patch S11 dip, pulse decay check, rectangular default window, `nfft` padding rules) green and unchanged.

**Parent plan:** `dev/plans/fdtd3d.md` (§ Near-to-far, § Near-to-far approach, validation item 12). Where this brief and the parent disagree, this brief is the corrected version. §2 derives each correction. The implementation PR should edit the parent so that the two agree (§10).

**Reviewer measurement:** `dev/plans/fdtd3d_phase5_review/ntff_probe.rs`. This is an independent NTFF written against the public `FdtdSim` API. It is not built by the workspace (instructions are at the top of the file). Every tolerance in §7 is set from its numbers in §3, including the planted-bug runs.

---

## 1. Scope

In:

1. On-the-fly DFT of tangential E and H on a closed six-face Huygens box, at a handful of armed frequencies.
2. Far-field `Eθ`, `Eφ` for any `theta × phi` grid, with a configurable phase origin.
3. Directivity, and, when a port exists, gain and realized gain. Each is computed from a defined power. Nothing is normalized to make a number look right (§2.7).
4. Lifecycle and validity checks: a homogeneous exterior, CPML on all six faces, sources inside the box, a decayed record.
5. `examples/pde/fdtd_dipole.rlab` and `examples/notebooks/fdtd_dipole.md`, plus `gallery/fdtd_dipole.md` and its plots.
6. Docs, REPL help, quickref, AGENTS, CHANGELOG, a version bump, and tests.

Out: the patch antenna notebook, `fdtd_sweep`, parallel stepping (all Phase 6), S21 and second ports, TF/SF plane waves, `Js`/`Ms` export, infinite-ground image theory (see §12, item 5), and a lossy or dispersive background.

---

## 2. Physics

### 2.1 Conventions (fixed for the whole feature)

- Time-harmonic `e^{+jωt}`, which matches `fft` / `fdtd_s11`.
- The phasor of a sampled signal is a direct DFT at the **true sample time**:

  ```text
  X(f) = Σ_s x_s · e^{−j 2π f t_s} · dt
  ```

  For step `s = 1..steps`: E, V, and the soft-source waveform value `g(s·dt)` are at `t_s = s·dt`. H and the port current I are at `t_s = (s − ½)·dt`. There is no separate "fold `dt/2` into H" step: using the true time *is* that fold. Port I gets the same correction that `fdtd_s11` applies (`exp(+jωdt/2)` relative to the FFT at `s·dt`).
- Spherical basis (θ measured from +z, φ from +x toward +y):

  ```text
  r̂ = ( sinθ cosφ,  sinθ sinφ,  cosθ)
  θ̂ = ( cosθ cosφ,  cosθ sinφ, −sinθ)
  φ̂ = (−sinφ,       cosφ,       0   )
  ```

- Background medium (§2.6): `k = 2πf·sqrt(εr μr)/c0` and `η = η0·sqrt(μr/εr)`, with `η0 = sqrt(μ0/ε0)`. Use the continuum `k`, not the Yee numerical `k`. The dispersion error is part of the measured tolerance.

### 2.2 Far-field formulas (Balanis, *Advanced Engineering Electromagnetics*, §6.8; Taflove & Hagness ch. 8)

With outward `n̂` on each face:

```text
Js = n̂ × H        Ms = −n̂ × E
N  = Σ Js · w · e^{+jk r̂·(r′ − o)}        L = Σ Ms · w · e^{+jk r̂·(r′ − o)}

Nθ = Nx cosθ cosφ + Ny cosθ sinφ − Nz sinθ        Nφ = −Nx sinφ + Ny cosφ   (same for L)

Fθ = −j k/(4π) · (Lφ + η Nθ)
Fφ = +j k/(4π) · (Lθ − η Nφ)
```

`F = r·e^{jkr}·E_far` is the angular part, returned as `Etheta` / `Ephi`. Units are V·s: V/m × m, times the `dt` in the DFT. `w` is the area weight (§2.3) and `o` the origin (§2.5). Do not include `e^{−jkr}/r`.

### 2.3 Huygens surface on the Yee lattice

The field storage and stagger are those of `fdtd/mod.rs`. In cell units: Ex `(j+½, i, k)`, Ey `(j, i+½, k)`, Ez `(j, i, k+½)`, Hx `(j, i+½, k+½)`, Hy `(j+½, i, k+½)`, Hz `(j+½, i+½, k)`.

**Box.** On each axis, with CPML thicknesses `n_lo` / `n_hi` and `len = nx|ny|nz`, the faces sit on E **node planes**:

```text
m_lo = n_lo + margin        m_hi = len − n_hi − margin        (0-based node index)
```

A node plane carries the two tangential E components exactly. The tangential H needed at the same point is half a cell off on each side, at planes `m − ½` and `m + ½`. Use the **mean of those two H samples**. Both must lie outside the CPML slab: by `cpml::in_slab`, H half-node `m` is interior when `n_lo ≤ m ≤ len − n_hi − 1`. Both conditions hold for `margin ≥ 1`. **`margin ≥ 1` is required.**

The two-plane mean costs a factor `cos(kΔ/2)` on H: 1.2% at λ/Δ = 20 and 0.55% at λ/Δ = 30. This is part of the measured error. Do not compensate it in v1, because a correction factor would hide a sign or timing slip elsewhere.

**Sample points and weights.** Each tangential component is used at its own lattice point on the face. There is no in-plane interpolation: E and the averaged H share in-plane coordinates once the off-plane H pair is averaged (for example, on a z-face Ex and Hy are both at `(j+½, i)`). On a face, a component runs over **half-node** positions along its own axis, with full weight Δ. Along the other in-plane axis it runs over **node** positions, with trapezoid weight Δ/2 at the two face edges and Δ elsewhere. `w` is the product. Example on a z-face: Ex samples `j = lo..hi−1` and `i = lo..=hi`, with `w = dx·dy·(½ if i ∈ {lo, hi} else 1)`.

The trapezoid half-weight is not optional. Without it, the pattern integral disagrees with the surface flux by 5.8% (§3.3).

**Currents per face**, `s = −1` on the lo face and `+1` on the hi face (`n̂ = s·axis`). J always lies along the E component at that sample; M is perpendicular to it.

| Face | Sample | E used | mean-H used | J | M |
|---|---|---|---|---|---|
| z | Ex loc | Ex | Hy (k−1, k) | `Jx = −s·Hy` | `My = −s·Ex` |
| z | Ey loc | Ey | Hx (k−1, k) | `Jy = +s·Hx` | `Mx = +s·Ey` |
| x | Ey loc | Ey | Hz (j−1, j) | `Jy = −s·Hz` | `Mz = −s·Ey` |
| x | Ez loc | Ez | Hy (j−1, j) | `Jz = +s·Hy` | `My = +s·Ez` |
| y | Ex loc | Ex | Hz (i−1, i) | `Jx = +s·Hz` | `Mz = +s·Ex` |
| y | Ez loc | Ez | Hx (i−1, i) | `Jz = −s·Hx` | `Mx = −s·Ez` |

Derivation of the first row: on the z-hi face `n̂ = +ẑ`, so `ẑ × H = (−Hy, Hx, 0)` and `−ẑ × E = (Ey, −Ex, 0)`. The others follow by cycling `x → y → z`.

The "(k−1, k)" column gives the H storage indices whose mean sits on node plane k. For example, Hy at `(i, j, k−1)` lives at `z = (k−½)Δ`, and Hy at `(i, j, k)` lives at `(k+½)Δ`.

**Surface Poynting flux** uses the same samples and weights, so it is the exact discrete counterpart of the pattern integral:

```text
P_flux = ½ Re Σ (E × H*)·n̂ · w = −½ Re Σ E_t · J_t* · w      (J_t = the J entry of that row)
```

### 2.4 Soft-source current moment (reference for the analytic tests)

A soft source does `E −= (dt/ε0)·Cb_rel·g/(N·Δ)` after the E curl update. That is Ampère's law with an impressed `J = g/(N·Δ)` on each of its N edges. Its current moment `Iℓ = J·(cross-section)·(N·Δ)` is `g·dx·dy` for `ez`, independent of N (and `g·dy·dz` for `ex`, `g·dx·dz` for `ey`).

In the leapfrog, that J sits at the half step `(s−½)·dt`, but the code evaluates `g` at `s·dt`. So the reference phasor is:

```text
Iℓ(f) = A · Σ_s g(s·dt) · e^{−j2πf (s − ½)dt} · dt
```

Using `s·dt` instead gives a phase error of `ωdt/2`: 0.058 rad at λ/Δ = 30 and 0.079 rad at λ/Δ = 20. The `src_time` planted bug in §3.3 is exactly this, and gate G1's 0.02 rad phase limit catches it.

Hertzian dipole along `ẑ` (`N = ẑ Iℓ`, `L = 0`):

```text
Nθ = −Iℓ sinθ,  Nφ = 0  ⇒  Fθ = +j η k Iℓ sinθ / (4π),   Fφ = 0
P  = η k² |Iℓ|² / (12π),   D = 1.5  (1.761 dBi)
```

Along `x̂`: `F = −(jηkIℓ/4π)·x̂_⊥`, so `Fθ = −F0 cosθ cosφ` and `Fφ = +F0 sinφ`, where `F0 = jηkIℓ/(4π)`.

### 2.5 Origin

The origin only multiplies each direction by `e^{−jk r̂·(o − o′)}`. It never changes `|F|`, D, G, or flux. Default: the **centroid of the Huygens box**, `((m_lo + m_hi)/2 · dx, …)` in metres from node (0, 0, 0), where script node index `j` (1-based) is at `x = (j − 1)·dx`.

The parent plan's `((nx−1)dx/2, …)` is the middle of the node index range. That is not the physical box centre `nx·dx/2` (the max wall is at node `nx`), and it is half a cell off. With symmetric CPML the box centroid equals `nx·dx/2`. Correct the parent (§12, item 3).

### 2.6 Validity: a closed surface in a homogeneous, source-free exterior

The equivalence theorem needs the true fields on the surface and a homogeneous, unbounded, source-free exterior. On this grid, that means:

1. **CPML on all six faces.** A PEC wall beyond the box reflects, and the image currents are not on the surface. The parent plan allows "from the PEC wall on a face where PML is off"; v1 refuses that (§12, item 4).
2. **Homogeneous exterior.** Every cell outside the box, plus the one-cell layer just inside each face, must have the same `eps_r` and `mu_r` (exact `==`) and `sigma = 0`. The inner layer matters because the bake averages the cells around each face E edge, so a different material there changes the update coefficients of the very samples being transformed. Concretely, cell index `c` on each axis may differ from background only when `m_lo + 1 ≤ c ≤ m_hi − 2` on all three axes. The background values are read from the corner cell `(0, 0, 0)`. Lossy background (`sigma > 0`) is an error: k would be complex, and v1 does not do that.
3. **No metal on or outside the surface.** Every PEC edge must have both end nodes in `[m_lo + 1, m_hi − 1]` on every axis.
4. **Every source and port edge strictly inside**, under the same node rule as item 3.
5. **Resolution.** Refuse `c_bg/(f·max(dx, dy, dz)) < 10` at any armed f. §3 shows 2–4% amplitude error at λ/Δ = 15, and the Yee dispersion error grows fast below that. Also refuse `f ≥ 1/(2·dt)`.
6. `m_hi − m_lo ≥ 2` on every axis.

Probes may be anywhere.

### 2.7 Directivity, gain, realized gain — what each needs

Radiation intensity: `U(θ,φ) = (|Fθ|² + |Fφ|²)/(2η)`, in the DFT's squared units. These cancel in every ratio below.

| Quantity | Definition | Power it needs | Available when |
|---|---|---|---|
| Directivity `D` | `4π U / P_rad` | radiated power through the box: `P_flux` (§2.3) | always |
| Gain `G` (IEEE) | `4π U / P_acc` | power accepted at the port: `P_acc = ½ Re(V I*) = ½(|a|² − |b|²)` | port present |
| Realized gain `G_r` | `4π U / P_inc` | power incident from a matched `Z0` source: `P_inc = ½|a|²` | port present |

Here `a = (V + Z0 I)/(2√Z0)` and `b = (V − Z0 I)/(2√Z0)` (the Phase 4 waves), with V and I phasors at the armed frequencies from the stored port record, using the timing of §2.1. Identities: `G_r = G·(1 − |S11|²)` with `S11 = b/a`, and `D = G` when nothing inside the box dissipates (`P_acc = P_flux`).

Corrections to the parent plan:

- **Normalization by the surface flux, not the pattern integral.** The parent normalizes D by a trapezoid integral of the requested grid and makes a single cut return NaN. `P_flux` is the same discrete surface integral, exact for any requested grid. It gives D for a single cut, which is what the lesson plots. It agrees with the full-sphere pattern integral to 0.01–0.2% on a correct transform; a planted bug pushes that to 0.75–25% (§3.3). The pattern integral stays as a **test** that cross-checks the transform against the flux (G7). It is not the normalizer (§12, item 1).
- **Realized gain is not `D·(1 − |S11|²)` with S11 interpolated from `fdtd_s11`.** That equals `4πU/P_inc` only if `P_acc = P_rad`, which is false once the substrate is lossy (the Phase 6 `tanδ` rerun). It also interpolates FFT bins when the exact frequency is available. Use the port phasors at each armed frequency.
- **No port, no gain.** A soft source has no available power, so `gain*` and `realized_gain*` are NaN. Nothing is fabricated from `Iℓ`.

### 2.8 Correction of parent validation item 12 (polarization)

Item 12 says that for a z-directed source "`Ephi` dominates in the xy-plane cut", that the cut is symmetric under `φ → φ+π`, and that broadside is the maximum of that cut. From §2.4, `Fφ ≡ 0` for a z-directed electric dipole and `F = Fθ θ̂`. In the xy-plane (θ = π/2), `θ̂ = −ẑ`, so the far E is vertical and **`Etheta` dominates**. `Ephi` would dominate for a z-directed *magnetic* dipole (a small loop), or as `|Fφ| ∝ |sinφ|` for an x-directed electric dipole.

The xy cut of a z dipole is omnidirectional, so "broadside is the maximum of that cut" is ill-posed. The maximum is θ = π/2 in an **elevation** cut. `φ → φ+π` symmetry holds trivially for a centred z dipole and catches almost nothing. Item 12's "D between 0 and 4 dBi" is far too loose: a flipped `L` sign gives 1.4–2.6 (still inside that window), while the correct transform holds 1.5 to 0.6% (§3).

Replace item 12 with gates G1–G9 (§7).

---

## 3. Measured feasibility (reviewer probe, release build)

Setup: `dx = dy = dz = 1 mm`, Courant 0.99 (`dt = 1.9066 ps`), CPML default parameters, margin 2, soft source driven by the derivative of a Gaussian (`tau = √2/(2π f_c)`, `t0 = 5 tau`, `f_c` at λ/Δ = 20), 700 steps. Frequencies are reported as λ/Δ. "Grid A" is 32³ with 6-cell CPML (box nodes 8..24, 3264 samples). "Grid B" is 40³ with 8-cell CPML (box 10..30, 5040 samples).

### 3.1 Short dipole vs analytic (centre source, `ez`; `F/F_an` at θ = 90°, origin at the source)

| λ/Δ | grid | \|F/F_an\| | arg (rad) | max \|Fφ\|/\|Fθ\| (xy cut) | sinθ shape err | az ripple | P_flux/P_an | P_pat/P_flux | D |
|---|---|---|---|---|---|---|---|---|---|
| 40 | A | 1.0009 | +0.0012 | 1.2e-6 | 7.6e-4 | 6.2e-4 | 1.0027 | 0.9983 | 1.4986 |
| 30 | A | 1.0040 | +0.0009 | 4.9e-6 | 3.3e-3 | 2.1e-3 | 1.0051 | 0.9999 | 1.5044 |
| 20 | A | 1.0101 | −0.0094 | 3.0e-5 | 8.8e-3 | 4.3e-3 | 1.0099 | 1.0034 | 1.5154 |
| 40 | B | 1.0019 | +0.0005 | 2.4e-6 | 1.5e-3 | 1.1e-3 | 1.0025 | 0.9998 | 1.5019 |
| 30 | B | 1.0050 | −0.0015 | 7.6e-6 | 4.2e-3 | 2.7e-3 | 1.0043 | 1.0017 | 1.5085 |
| 20 | B | 1.0027 | −0.0108 | 4.2e-5 | 1.3e-2 | 1.3e-3 | 1.0097 | 0.9970 | 1.4975 |
| 15 | B | 1.0181 | −0.0049 | 1.4e-4 | 4.0e-2 | 1.5e-2 | 1.0175 | 0.9954 | 1.5279 |

Box size (margin 2 vs 4 on A, 2 vs 5 on B) moves `|F|` by 0.20–0.23% at λ/Δ = 30. The record's surface-E tail/peak over the last 10% is 2–4e-4 at 700 steps. 350 steps give the same numbers to 4 digits (the short-dipole pulse is gone within ~300 steps).

### 3.2 x-dipole, offset source, port

- **x-dipole** (`ex` at the centre), max over the cut of `|F − F_ref|/|F0|`. At θ = 90°, `Fφ` vs `F0 sinφ`: 1.5e-3 / 4.1e-3 / 1.4e-2 at λ/Δ = 40 / 30 / 20 on A. In the φ = 0 elevation cut, `Fθ` vs `−F0 cosθ`: the same numbers. `|Fθ(90°, φ)|` is 3e-16 (exact by symmetry).
- **Offset source**: `ez` moved `d = +4Δ` in x, origin at the box centre. The phase of `F(90°, 0)/F(90°, π)` should be `2kd`. Error: 5e-5 / 2.0e-3 / 2.2e-4 rad on A at λ/Δ = 40 / 30 / 20, and 7.5e-4 / 3.1e-3 / 1.4e-2 on B. `|ratio| − 1` ≤ 0.009.
- **Port** (Phase 4 `add_port`, `ez`, one edge, `Z0 = 50`, same waveform as `Vg`): `P_acc/P_flux` = 0.99994–1.00002, and `G/D − 1` ≤ 6e-5. `G_r/(G(1 − |S11|²))` = 1 to 1e-6. The sign of `½Re(VI*)` is positive (power into the grid). This one-edge gap is a poor radiator (`Zin ≈ 1.9 − 870j Ω` at λ/Δ = 30, `|S11| = 0.99975`), yet the power bookkeeping still holds to 1e-5.

### 3.3 Planted bugs (Grid B, λ/Δ = 30, offset phase on Grid B)

Each row reruns §3.1 with one deliberate error. The gate that catches it is in bold. Thresholds are from §7.

| Planted bug | \|F/F_an\| | arg | xpol | shape | P_pat/P_flux | D | offset phase err |
|---|---|---|---|---|---|---|---|
| none (correct) | 1.0050 | −0.0015 | 7.6e-6 | 4.2e-3 | 1.0017 | 1.5085 | 3.1e-3 |
| exponent sign `e^{−jk r̂·r′}` | **0.838** | **3.12** | **3.6e-3** | **0.20** | **0.755** | **1.407** | **0.12** |
| M sign flipped | **0.838** | **3.12** | **3.6e-3** | **0.20** | **0.755** | **1.407** | **3.06** |
| H at `s·dt` (no half step) | 1.0053 | −0.0065 | **1.0e-4** | **0.023** | **0.985** | 1.506 | **0.014** |
| source Iℓ timed at `s·dt` | 1.0050 | **+0.058** | 7.6e-6 | 4.2e-3 | 1.0017 | 1.5085 | 3.1e-3 |
| H from one plane, not the mean | 1.0016 | **−0.064** | **1.6e-3** | **0.022** | **1.0075** | **1.540** | **0.075** |
| no trapezoid edge weights | **1.050** | 0.010 | **4.2e-4** | **0.041** | **1.058** | **1.646** | **0.021** |

At λ/Δ = 20 the H-timing bug is also caught by phase (−0.099 rad) and by D (1.653).

### 3.4 Half-wave wire dipole (example candidate, release, 48³, CPML 8, margin 2)

A port on one centre `ez` edge with `Z0 = 50`. The arms are PEC edges, 7 cells each, on the node line through the gap (two `add_pec_face` calls on the x plane through the port, one per arm, so the gap edge itself is not covered). Total length 15 mm.

| f (GHz) | Zin (Ω) | \|S11\| | D (dBi) | G (dBi) | G_r (dBi) |
|---|---|---|---|---|---|
| 8.0 | 55.4 − 44.6j | 0.393 | 2.077 | 2.077 | 1.350 |
| 8.5 | 66.7 − 12.2j | 0.177 | 2.120 | 2.120 | 1.983 |
| 9.0 | 80.3 + 19.4j | 0.273 | 2.165 | 2.165 | 1.829 |
| 10.0 | 116.2 + 80.6j | 0.565 | 2.261 | 2.261 | 0.595 |

`P_acc/P_flux` = 1 ± 5e-6 at every frequency from 7 to 13 GHz. Tail/peak is 2.6e-5 at 1000 steps; 2000 steps change nothing to the printed digits. Resonance (X = 0) falls between 8.5 and 9 GHz with R ≈ 72 Ω and D ≈ 2.13 dBi. Thin-wire theory gives 73 Ω and 2.15 dBi. The resonant length is 15 mm ≈ 0.44 λ, which is the expected thin-wire shortening on a staircase wire. These are the example's printed numbers (§8). They are a demonstration, not a gate tighter than ±0.1 dBi, because the wire's effective radius is a grid property.

---

## 4. API contract

### 4.1 Rust (`rustlab-dsp`, new `src/fdtd/ntff.rs`, hooks in `mod.rs`)

```rust
/// Resolved Huygens box: 0-based node planes (lo, hi) on x, y, z.
pub struct NtffBox { pub x: (usize, usize), pub y: (usize, usize), pub z: (usize, usize) }

pub struct PortPower {
    pub z0: f64,
    pub v: Vec<C64>, pub i: Vec<C64>,           // phasors at the armed freqs, §2.1 timing
    pub s11: Vec<C64>, pub accepted: Vec<f64>, pub incident: Vec<f64>,
}

pub struct FarField {
    pub freqs: Vec<f64>, pub theta: Vec<f64>, pub phi: Vec<f64>,
    pub e_theta: Vec<C64>, pub e_phi: Vec<C64>,  // index (q * nθ + a) * nφ + b
    pub prad: Vec<f64>,                          // P_flux per freq
    pub k: Vec<f64>, pub eta: f64, pub origin: [f64; 3],
    pub decay: f64,                              // surface-E tail/peak, last 10%
    pub port: Option<PortPower>,
}

impl FdtdSim {
    /// Arm (or, with empty `freqs`, disarm) the transform. `steps == 0` only.
    /// Replaces an earlier call. Checks only its own arguments (§5).
    pub fn set_ntff(&mut self, freqs: &[f64], margin: usize) -> Result<(), DspError>;
    /// Box for the current CPML / margin, with every §2.6 check applied.
    pub fn ntff_box(&self) -> Result<NtffBox, DspError>;
    pub fn ntff_freqs(&self) -> &[f64];
    pub fn far_field(&self, theta: &[f64], phi: &[f64], origin: Option<[f64; 3]>,
                     decay: RecordDecay) -> Result<FarField, DspError>;
}
```

Implementation notes (requirements, not suggestions):

- **Resolve at the first step.** `run` / `step` with `steps == 0` and NTFF armed calls the `ntff_box` checks. On failure it returns the error with nothing stepped (`steps` stays 0, fields untouched). Only then does it build the sample tables (positions, weights, field indices, J/M coefficients) and the zeroed accumulators. Do not resolve in `set_ntff`: `fdtd_cpml`, `fdtd_media`, and `fdtd_pec` may follow it, and `set_media` changes `dt`.
- **Accumulate once at the end of every step**, after `record_port`: E is at `s·dt` and H at `(s−½)·dt` (H was updated at the start of the step). For each armed f, compute `cos`/`sin` of `ω·s·dt` and `ω·(s−½)·dt` **directly from the step index** (`s as f64`), not by recursive rotation, so that split runs match one run bit for bit. Store one E phasor and one mean-H phasor per sample per frequency (`2 × 16 B`).
- **Decay trace**: push one `f64` per step, the max `|E|` over the surface samples. `decay` = `port::tail_over_peak` of that trace. Reuse `PULSE_TAIL_LIMIT` (1e-3) and `RecordDecay`.
- `far_field` reads the port record (`port_v`, `port_i`) for `PortPower` with a direct DFT at the armed frequencies. With `RecordDecay::Pulse` it also applies the Phase 4 V and I tail check.
- Far-field evaluation: per direction, build per-axis phase tables over the half-integer lattice (`e^{jk ux (x − ox)}` and so on, length `2·len + 2`), so each sample costs two complex multiplies and no `exp`. Serial in v1 (parallel is Phase 6).
- NTFF must never write a field. With NTFF off, nothing new runs in the step loop except one `Option` check.
- `bytes()` includes the accumulators, the sample tables, and the decay trace.

### 4.2 Script (`rustlab-script/src/eval/fdtd.rs`)

```text
fdtd_ntff(sim, freqs)
fdtd_ntff(sim, freqs, "margin", m)        % m integer ≥ 1, default 2
fdtd_ntff(sim, [])                        % disarm
pat = fdtd_farfield(sim, theta, phi)
pat = fdtd_farfield(sim, theta, phi, "origin", [x, y, z])
pat = fdtd_farfield(sim, theta, phi, "decay", "off")
```

`freqs` is a real vector, Hz. `theta` and `phi` are real vectors in radians; any lengths ≥ 1, so a single cut is `theta = pi/2` with `phi = linspace(0, 2*pi, 181)`. Options are name/value pairs in any order. A repeated option is an error, the same as in `parse_s11_options`.

`pat` fields:

| Field | Shape / type | Meaning |
|---|---|---|
| `freq` | 1 × nf | armed frequencies |
| `theta`, `phi` | as given | echoes |
| `Etheta`, `Ephi` | complex Tensor3 `(nf, nθ, nφ)` | `r·e^{jkr}·E`, V·s (§2.2) |
| `U` | real Tensor3 | `(|Eθ|² + |Eφ|²)/(2η)` |
| `prad` | 1 × nf | `P_flux` |
| `directivity`, `directivity_dBi` | real Tensor3 | `4πU/prad`, `10·log10` |
| `gain`, `gain_dBi` | real Tensor3 | `4πU/paccepted`; NaN with no port |
| `realized_gain`, `realized_gain_dBi` | real Tensor3 | `4πU/pincident`; NaN with no port |
| `paccepted`, `pincident` | 1 × nf | NaN with no port |
| `s11` | complex 1 × nf | `b/a` at the armed freqs; NaN with no port |
| `origin` | 1 × 3 | metres, the one used |
| `box` | 1 × 6 | 1-based node planes `[xlo xhi ylo yhi zlo zhi]` |
| `k`, `eta` | 1 × nf, scalar | background values |
| `decay` | scalar | surface tail/peak |

`fdtd_info(sim)` gains `ntff` (the number of armed frequencies, 0 when off), and its `bytes` includes the NTFF storage.

---

## 5. Lifecycle and error contract

Every message names the function and the fix. Tests assert a distinctive substring of each.

| When | Condition | Result |
|---|---|---|
| `fdtd_ntff` | `steps > 0` | error: arm before the first step |
| `fdtd_ntff` | `freqs` not a real vector, any entry non-finite or ≤ 0, or ≥ `1/(2·dt)` at the current dt | error |
| `fdtd_ntff` | margin not an integer ≥ 1 | error |
| `fdtd_ntff` | called twice | second call replaces the first |
| first `fdtd_run` / `fdtd_step` | any §2.6 rule broken (name the face, cell, or edge, and the rule) | error, `steps == 0`, nothing stepped |
| `fdtd_farfield` | not armed | error: call `fdtd_ntff` before the run |
| `fdtd_farfield` | `steps == 0` | error: run first |
| `fdtd_farfield` | empty or non-finite `theta` / `phi`; `origin` not 3 finite reals; unknown or repeated option | error |
| `fdtd_farfield` | surface tail/peak > 1e-3, or port V/I tail > 1e-3 (when a port exists), without `"decay", "off"` | error with the measured ratios. Text in the Phase 4 style: run longer; padding does not help; a steady tone is not a pulse |
| after an interrupted run | — | `fdtd_farfield` works on the partial accumulation, subject to the decay rule |

`fdtd_media`, `fdtd_cpml`, and `fdtd_pec` after `fdtd_ntff` are allowed (they are all still before step 1); the box is resolved at step 1. `fdtd_farfield` can be called any number of times with different grids and origins without stepping again.

---

## 6. Files

| File | Change |
|---|---|
| `crates/rustlab-dsp/src/fdtd/ntff.rs` (new) | sample tables, accumulators, flux, far field, port phasors |
| `crates/rustlab-dsp/src/fdtd/mod.rs` | `mod ntff;`, re-exports, the `ntff: Option<…>` field, `set_ntff` / `ntff_box` / `far_field`, end-of-step hook, first-step resolve, `bytes()` |
| `crates/rustlab-dsp/src/fdtd/tests.rs` or new `ntff_tests.rs` | §7 DSP tests |
| `crates/rustlab-dsp/examples/bench_fdtd.rs` | NTFF rows (§9) |
| `crates/rustlab-script/src/eval/fdtd.rs` | `builtin_fdtd_ntff`, `builtin_fdtd_farfield`, the `fdtd_info` field |
| `crates/rustlab-script/src/eval/builtins.rs` | register both |
| `crates/rustlab-script/tests/fdtd.rs` | §7 script tests |
| `crates/rustlab-cli/src/commands/repl.rs` | two `HelpEntry` and the `pde` "FDTD (3-D)" `CategoryRow` |
| `examples/pde/fdtd_dipole.rlab`, `examples/notebooks/fdtd_dipole.md`, `gallery/fdtd_dipole.md`, `gallery/plots/fdtd_dipole/*.svg` | §8 |
| `docs/functions.md`, `docs/quickref.md`, `AGENTS.md`, `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock` | §10 |
| `dev/plans/fdtd3d.md` | Phase 5 status and corrections (§10) |

No new crates, dependencies, or feature flags. Plotting uses the existing `plot`, `polar`, `subplot`, and `savefig`.

---

## 7. Tests (write first, watch them fail, then implement)

Unless a test says otherwise, it uses Grid A: 32³, `dx = dy = dz = 1 mm`, `fdtd_cpml` thickness 6 on all six faces, margin 2, a soft source with the derivative of a Gaussian (`tau = √2/(2π·15 GHz)`, `t0 = 5·tau`), and 700 steps. Primary frequency 9.993 GHz (λ/Δ = 30); secondary 14.99 GHz (λ/Δ = 20). The reference `Iℓ` is computed in the test from the waveform samples with the `(s − ½)·dt` timing of §2.4. These run in the debug `cargo test --workspace`. If one takes more than ~15 s in debug, mark only that test `#[ignore = "release: cargo test --release -p rustlab-dsp -- --ignored ntff"]` and keep a cheaper debug variant (350 steps; §3.1 shows the same digits).

**Physics gates (rustlab-dsp).** No gate may be loosened. If one fails, stop and report it with the numbers.

| Id | Test | Gate (λ/Δ = 30 unless noted) | Measured (A / B) | What it catches |
|---|---|---|---|---|
| G1 | `ntff_z_dipole_matches_hertzian_amplitude_and_phase` | `abs(abs(F/F_an) − 1) ≤ 0.01` and `abs(arg(F/F_an)) ≤ 0.02` rad at θ = 90°, φ = 0, origin at the source; at λ/Δ = 20: ≤ 0.02 and ≤ 0.03 | 0.0040 / 0.0050; 0.0009 / 0.0015 | η, k, 4π, sign of j, M sign, exponent sign, source timing, missing trapezoid |
| G2 | `ntff_z_dipole_has_no_cross_pol` | max over a 72-point xy cut of `abs(Fφ)/abs(Fθ) ≤ 2e-5`; at λ/Δ = 20 ≤ 2e-4 | 4.9e-6 / 7.6e-6 | H timing, one-plane H, weights. This is the item-12 correction: Eθ dominates |
| G3 | `ntff_z_dipole_sin_theta_shape_and_azimuth` | **Wording check (implementation):** the written formula is already a normalized magnitude shape, `abs(abs(Fθ)/(abs(F_an)·sinθ) − 1)`, not an absolute residual `abs(Fθ − F_an sinθ)`. Leave it. θ = 5°…175°; azimuth ripple `max/min − 1 ≤ 5e-3`. Grid A remeasure: shape 3.32e-3, ripple 2.14e-3 | 3.3e-3 / 4.2e-3; 2.1e-3 / 2.7e-3 | same as G2 |
| G4 | `ntff_x_dipole_exercises_phi_components` | `ex` source: max over φ of `abs(Fφ − F0·sinφ)/abs(F0) ≤ 0.01` at θ = 90°; max over θ of `abs(Fθ + F0·cosθ)/abs(F0) ≤ 0.01` at φ = 0; `abs(Fθ(90°, φ))/abs(F0) ≤ 1e-12` | 4.1e-3 / 5.2e-3; 3e-16 | the `Nφ` / `Lθ` paths and the x/y-face rows that a z dipole barely uses |
| G5 | `ntff_offset_source_phase_is_2kd` | `ez` at `+4Δ` in x, default origin: `abs(arg(F(90°, 0)/F(90°, π)) − 2kd) ≤ 0.01` rad (compare `arg(ratio·e^{−j2kd})`, never raw args); `abs(abs(ratio) − 1) ≤ 0.01` | 2.0e-3 / 3.1e-3 | exponent sign (0.12), M sign (3.06), H timing (0.014), weights (0.021), position offsets of ½ cell |
| G6 | `ntff_flux_matches_hertzian_power` | `abs(P_flux/P_an − 1) ≤ 0.01`; at λ/Δ = 20 ≤ 0.02 | 0.0051 / 0.0043 | flux sign and weights; the source normalization of §2.4 |
| G7 | `ntff_pattern_integral_matches_flux` | full sphere, 61 θ (trapezoid incl. poles) × 120 φ: `abs(P_pat/P_flux − 1) ≤ 0.005` | 1.2e-4 / 1.7e-3 | transform vs flux consistency (H timing 0.015, weights 0.058, one-plane 0.0075) |
| G8 | `ntff_short_dipole_directivity_is_1_5` | **Wording correction.** `D(θ) = 1.5 sin²θ`, so pointwise `D` equals 1.5 only at θ = 90°. The old “max of abs(D/1.5 − 1)” is 1 at the poles and is not the gate. Same 0.01 tolerance, at λ/Δ = 30 and 40 only (λ/Δ = 20 measured peak error 1.02e-2 and angular 1.32e-2, outside 0.01; not added). Gate: `abs(D_peak/1.5 − 1) ≤ 0.01` and `abs(D/(D_peak sin²θ) − 1) ≤ 0.01` for θ in [5°, 175°]. Grid A: peak 2.95e-3 / angular 5.16e-3 at λ/Δ = 30; peak 9.14e-4 / angular 1.32e-3 at λ/Δ = 40 | 0.0029 was the peak statistic, not the pointwise one | flux normalization end to end |
| G9 | `ntff_box_size_invariance` | margin 2 vs margin 4: `abs(F₂/F₄ − 1) ≤ 0.005` at θ = 90° | 0.0020 / 0.0022 | box-dependent errors (weights at face edges, CPML leakage) |
| G10 | `ntff_port_power_matches_flux` | one-edge `ez` port, `Z0 = 50`: `abs(P_acc/P_flux − 1) ≤ 1e-3`, `abs(G/D − 1) ≤ 1e-3`, `abs(G_r/(G·(1 − abs(S11)²)) − 1) ≤ 1e-9` | ≤ 6e-5; 1e-6 | port phasor timing and sign, the §2.7 definitions. The last identity is self-consistency and is labelled as such |
| G11 | `ntff_port_s11_matches_fdtd_s11_bin` | armed f = an exact FFT bin `m/(N·dt)` of the record: `abs(S11_ntff − S11_fdtd_s11) ≤ 1e-9·abs(S11)` | — | the two timing conventions agree |
| G12 | `ntff_background_eps_r_2` | uniform `eps_r = 2` everywhere, f at medium λ/Δ = 30: G1 amplitude ≤ 0.02, G6 ≤ 0.02 (using η and k of the medium), peak `|D/1.5 − 1|` ≤ 0.02. Decay is **off** in the test | 700 steps: `|F/Fan|` err 0.0036, arg +0.0005, power err 0.0048, peak D err 0.0024, angular 5.03e-3. 1400 steps stays inside 0.02. Surface tail 2.75e-3 at 700 and 2.40e-3 at 1400 (a floor: CPML σ is not scaled by eps_r). The 1e-3 decay limit is not weakened | k and η of the background |

**Exactness and lifecycle (rustlab-dsp).**

| Id | Test | Gate |
|---|---|---|
| L1 | `ntff_does_not_touch_fields` | `field_bits` after 200 steps are identical with NTFF on and off; `PHASE1_GOLDEN` still matches on the golden sim (NTFF off) |
| L2 | `ntff_split_run_is_bit_identical` | `run(300)` vs `run(130)` + `run(170)`: every `e_theta` / `e_phi` / `prad` bit-identical |
| L3 | `ntff_origin_shift_is_a_phase` | `F(o2) = F(o1)·e^{−jk r̂·(o2 − o1)}` to 1e-12 relative; `abs(F)` and D unchanged. Self-consistency, labelled |
| L4 | `ntff_rejects_missing_cpml_face` | each of six faces off in turn → first-step error naming that face; `steps == 0` |
| L5 | `ntff_rejects_inhomogeneous_exterior` | one cell `eps_r = 2` at `c = m_lo` (inner layer) → error; the same at `c = m_lo + 1` → accepted; `sigma > 0` in the background → error |
| L6 | `ntff_rejects_source_or_pec_on_surface` | source edge, port edge, or PEC edge touching a face plane → error; one node inside → accepted |
| L7 | `ntff_resolution_and_nyquist` | `c/(f·Δ) = 9.9` → error; 10.1 → accepted; `f ≥ 1/(2dt)` → error |
| L8 | `ntff_lifecycle` | arm after step 1 → error; re-arm replaces (frequency count changes); `[]` disarms; `far_field` unarmed → error; at `steps == 0` → error; `margin = 0` → error; `fdtd_cpml` after `fdtd_ntff` moves the box (`ntff_box`) |
| L9 | `ntff_decay_gate` | 60 steps (pulse still on the surface) → error quoting the tail ratio; `RecordDecay::Off` → returns; a steady modulated tone → error |
| L10 | `ntff_bytes_accounting` | `bytes()` grows by exactly `nsamples × nf × 32` + tables + `8 × steps` after the first step |

**Script layer (`crates/rustlab-script/tests/fdtd.rs`).**

| Id | Test | Gate |
|---|---|---|
| S1 | `fdtd_farfield_struct_shape_and_layout` | every field in §4.2 exists with its shape; `Etheta(q, a, b)` equals the Rust `e_theta[(q·nθ + a)·nφ + b]` bit for bit (catches a Tensor3 axis swap) |
| S2 | `fdtd_farfield_dipole_end_to_end` | G1 at λ/Δ = 30 through `.rlab` (`Iℓ` computed in the script) — catches a dropped factor between Rust and the script |
| S3 | `fdtd_farfield_no_port_gain_is_nan` | `gain`, `realized_gain`, `paccepted`, `s11` are NaN; `directivity` is finite |
| S4 | `fdtd_ntff_option_errors` | unknown or repeated option, bad margin, `origin` of length 2, a non-numeric `theta` → errors |
| S5 | `fdtd_info_reports_ntff` | `ntff` field; `bytes` grows |

The help-coverage tests in `repl.rs` must pass with the two new names.

---

## 8. Example and notebook

`examples/pde/fdtd_dipole.rlab`: flat, top-level code with no nested functions unless a loop body would otherwise repeat, following [Script style for agents](../../docs/agent-guide.md#script-style-for-agents). Two parts:

1. **Short dipole vs Hertzian** (Grid A, `ez` at the centre, 700 steps, one armed frequency at λ/Δ = 30). Prints: `abs(F/F_an)` and its phase, `max abs(Ephi)/abs(Etheta)` on the xy cut, D at θ = 90° (expected 1.504, 1.774 dBi), `prad/P_an` (1.005). It asserts nothing beyond what `error()` does when a value leaves the G1/G2/G8 band (use the same thresholds as the gates, so a regression fails `run-examples`).
2. **Half-wave wire dipole with a port** (§3.4 geometry, 1000 steps, freqs `8:0.5:10` GHz). Prints a table of `f`, `Zin = V/I` (from `pat.s11` and `Z0`), `abs(S11)`, D, G, and `G_r` in dBi at θ = 90°. Expected values are in §3.4 (±0.01 dBi at 48³). Errors if `abs(paccepted/prad − 1) > 1e-3`.

   **Runtime constraint:** `storage_modes.rs` runs every example twice in debug. If 48³ × 1000 steps takes more than ~20 s in a debug `rustlab run`, use 40³ with CPML 8 and arms 7 (box nodes 10..30 still clears the wire ends by 3 cells). Re-measure and replace the §3.4 numbers in the example comment. Do not reduce steps below the decay gate.

Plots (only existing surfaces, all through `savefig` in the notebook): the elevation cut `abs(Etheta)/max` vs θ, with `sin(theta)` overlaid for part 1; `polar(theta, abs(Etheta)/max)` for part 2 at the resonant frequency; G and `G_r` vs f.

`examples/notebooks/fdtd_dipole.md`: the same two parts with prose. It explains why Eθ is the co-polar component in the xy plane (§2.8), what D, G, and `G_r` each divide by (§2.7), why the box must sit in air inside CPML on all six faces, and why the record must decay. Run `make notebooks` (or `dev/build-notebooks.sh` for this one notebook) and commit `gallery/fdtd_dipole.md` and `gallery/plots/fdtd_dipole/*.svg`.

---

## 9. Runtime and memory

- **Accumulation**: per step, `nsamples × nf × 2` complex multiply-adds, plus 4·nf `sin`/`cos`. Grid A has 3264 samples, and the §3.4 48³ grid has about 9.7k. The Yee update is ~40 flops per cell per step, so on Grid B (64k cells, 5040 samples) one frequency adds roughly 2–3%.
- **Bench gate** (`bench_fdtd`, release): add `ntff` rows on 40³ with CPML 8, at 1 and at 8 frequencies. The NTFF-off rows must stay within ±3% of the same session's run on #142's head. The 1-frequency NTFF row must be ≤ 10% slower than CPML-on without NTFF. Report the 8-frequency row; it is not gated.
- **Memory**: `32 B × nsamples × nf`, plus about `64 B × nsamples` of tables, plus `8 B × steps`. Grid A, one frequency: about 0.3 MB. 48³, five frequencies: about 2.2 MB. Reported in `bytes`.
- **Far field**: `nθ·nφ·nf·nsamples` table-driven complex multiplies. A 181 × 361 sphere at one frequency on 48³ is about 6e8, a few seconds in release. The example uses cuts of ≤ 361 points.

---

## 10. Docs, help, version

- `docs/functions.md`: `fdtd_ntff` and `fdtd_farfield` sections (signatures, the §4.2 table, the §2.6 rules, the §2.7 definitions, an example). Add the `ntff` field to `fdtd_info`.
- `crates/rustlab-cli/src/commands/repl.rs`: one `HelpEntry` per function, each with the D/G/`G_r` definitions, units, and the decay rule. Add both names to the existing `pde` "FDTD (3-D)" `CategoryRow`.
- `docs/quickref.md`; AGENTS.md: two rows in "All builtin functions", `ntff.rs` under `rustlab-dsp` key files, and an Active Plans row update.
- `CHANGELOG.md`, *Added*: the two builtins and the example. *Breaking / behavior changes*: none, because NTFF off is bit-identical (L1).
- `dev/plans/fdtd3d.md`: Phase 5 status and corrections. Item 12 → G1–G9; default origin → box centroid; directivity normalized by `P_flux`; realized gain from port power; CPML required on all six faces. Point to this brief.
- **Version**: bump in the implementation PR. Open drafts already claim **0.3.59**: #142 (this base) and #139 (coding guidelines). **0.3.60** is claimed by #141 (comparison broadcast). Use **0.3.61** provisionally on this stacked branch. Re-number at merge time against whatever has landed, keeping "same version ⇒ same behaviour" (AGENTS rule 12). Resolve the 0.3.59 collision between #139 and #142 in the merge order, not here.

---

## 11. Validation and CI (implementation run)

This VM's default toolchain is 1.83, but the workspace has `rust-version = "1.89"` and a dependency needs edition 2024. Use `cargo +stable` (installed by this review run) and `--locked`. Do not change `Cargo.lock` beyond the version bump.

```sh
export LIBRARY_PATH=/usr/lib/gcc/x86_64-linux-gnu/13
cargo +stable test --locked -p rustlab-dsp fdtd            # G*, L*
cargo +stable test --locked -p rustlab-script --test fdtd   # S*
cargo +stable test --locked --release -p rustlab-dsp -- --ignored ntff   # only if any test was moved there
cargo +stable test --locked --workspace
rustfmt --edition 2021 --check <only the files you edited>
cargo +stable build --locked --release --bin rustlab --bin rustlab-notebook
RUSTLAB=$PWD/target/release/rustlab bash dev/run-examples.sh
./target/release/rustlab-notebook render examples/notebooks/fdtd_dipole.md -o /tmp/fdtd_dipole.html --strict
./target/release/rustlab-notebook check examples/notebooks/fdtd_dipole.md --strict
cargo +stable run --locked --release -p rustlab-dsp --example bench_fdtd
```

Also rerun the Phase 4 gates unchanged: the `rustlab-dsp` port/S11 tests, and the reviewer scripts in `dev/plans/fdtd3d_phase4_review/` (independent load, patch S11). Push, and confirm CI on the draft PR is green before reporting.

---

## 12. Decisions for Michael

These are the reviewer's recommendations. They are **not** Michael's signed approval of a Phase 6 design. Phase 6 geometry stays undecided. This implementation does not choose among the three options in item 5, and it does not implement image theory, a sweep, parallel stepping, or S21.

A non-positive accepted or incident power is returned as `NaN` gain (and `NaN` dBi). That is so a negative power is not printed as a gain. It is not a tolerance change.

1. **Directivity normalizer.** This brief uses the surface flux `P_flux`, so a single cut has a directivity and the pattern integral is a test (G7). The parent plan uses the pattern integral, with NaN for a cut. *Recommendation: flux.*
2. **Realized gain.** `4πU/P_inc` from port phasors at the armed frequencies, plus IEEE gain `4πU/P_acc`. The parent plan uses `D·(1 − |S11|²)` with S11 interpolated from `fdtd_s11`, which is wrong once there is loss. *Recommendation: as in this brief.*
3. **Default origin.** The box centroid (equal to `n·Δ/2` with symmetric CPML), not `(n − 1)·Δ/2`. Phase only.
4. **CPML on all six faces required in v1.** A PEC-wall face is refused.
5. **Phase 6 geometry (flag only; nothing is decided here).** The capstone sketch puts the ground on the `zmin` wall with `zmin` CPML off and a full-width substrate. Neither fits a closed homogeneous box. Phase 6 needs one of: (a) a finite ground and substrate floating in air with CPML on all six faces (a larger mesh; the 60×60×20 perf mesh has no room in z for CPML 8 plus margin 2); (b) a five-face box with image theory over an infinite PEC ground (a new formula and new tests); or (c) a box above the substrate, which is only approximate. Decide before Phase 6.
6. **Resolution floor** of 10 cells per background wavelength (§2.6, item 5) as a hard error, not a warning.

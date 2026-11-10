# Phase 5 implementation review (d0d1bfe)

Review only. No runtime code changed. Release build, `cargo +stable --locked`, this VM (4 cores).

## Verdict

**Not ready.** One blocker and two corrections inside Phase 5. The physics, the port power, the timing and the sign conventions hold. An independent transform agrees with the implementation to four or more digits.

## Blocker

**B1. `pat.Etheta` / `pat.Ephi` come out wrong under ordinary script indexing at ordinary source amplitude.**

The far-field phasor is in V·s and is about 1e-14 for a unit-amplitude source. The interpreter turns small complex values real:
- `value.rs:1263` (vector `v(i)`) and the Tensor3 single-element path drop `im` when `|im| < 1e-12`.
- `builtins.rs:5616` `is_real_vector` makes `abs()` use `|re|` when every `|im| < 1e-10`.

Repro: `/tmp/rv/scale.rlab`. Grid A z-dipole with the `ez` soft source scaled by A, at θ = 90°:

| A | `abs(pat.Etheta(1,:,1))(18)/A` | `sqrt(2 η U)/A` (true) | `pat.Etheta(1,18,1)` | `1e12`-scaled `abs` / A |
|---|---|---|---|---|
| 1e-6 | 3.743e-15 | 6.333e-14 | real 3.7e-21 (im dropped) | 3.743e-15 (workaround fails) |
| 1 | 3.743e-15 | 6.333e-14 | real 3.7e-15 (im dropped) | 6.333e-14 |
| 1e6 | 6.333e-14 | 6.333e-14 | complex, correct | 6.333e-14 |

At A = 1 the magnitude is 17× low and the phase is 0 instead of −1.512 rad. `angle()` and `imag()` on the vector stay correct.

What is already right:
- `U`, `directivity(_dBi)`, `gain`, `realized_gain`, and `prad` are computed in Rust on `f64`.
- `prad` scales as A² exactly, and D90 = 1.504421869 at all three amplitudes.

The `1e12` multiplier in the example and in `fdtd_farfield_matches_the_hertzian_dipole_at_thirty_cells` hides this for one amplitude only.

Fix inside Phase 5, either of:
- (a) Return a normalized far field so the values are O(1). For example, divide by the |phasor| of the drive, or document a fixed scale.
- (b) Make those two snaps relative (`|im| ≤ tol·|z|`), or exact-zero only. This is a language behavior change, so it needs a CHANGELOG entry and a run across the example and storage modes.

Afterwards, remove the `1e12` and add a script test at A = 1e-6, 1 and 1e6 that indexes one element and calls `abs` on a row.

## Corrections (should fix in Phase 5)

**C1. G12 does not need `decay = off`. The recorded "floor" does not reproduce.**

The brief's G12 row says the surface tail is 2.75e-3 at 700 steps and 2.40e-3 at 1400 because of "CPML σ not scaled by eps_r".

On d0d1bfe the test itself, temporarily switched to `RecordDecay::Pulse` (reverted, not committed), passes with tail 6.6455e-4, under the 1e-3 limit.

Convergence for eps_r = 2, λ/Δ = 30 in the medium (`/tmp/rv/g12.rlab`). Columns are amplitude, flux and peak-D error:

| grid / CPML / steps / extra | tail | `|F/Fan|−1` | `P/Pan−1` | `Dpk/1.5−1` |
|---|---|---|---|---|
| 32 / 6 / 700 | 6.6e-4 | +0.0045 | +0.0055 | +0.0035 |
| 32 / 6 / 1400 | 6.6e-4 | +0.0036 | +0.0042 | +0.0030 |
| 32 / 6 / 2800 | 2.0e-4 | +0.0040 | +0.0051 | +0.0029 |
| 32 / 6 / 5600 | 2.1e-4 | +0.0038 | +0.0046 | +0.0029 |
| 32 / 6 / 1400, `sigma_factor` 2 | 3.9e-4 | +0.0043 | +0.0067 | +0.0019 |
| 32 / 10 / 1400 | 2.4e-4 | +0.0008 | +0.0052 | −0.0037 |
| 32 / 6 / 1400, `alpha_max` 0.05 | 3.9e-8 | +0.0040 | +0.0052 | +0.0029 |
| 48 / 6 / 1400 | 1.4e-4 | +0.0008 | +0.0048 | −0.0032 |
| vacuum 32 / 6 / 700 (reference) | 4.3e-4 | +0.0040 | +0.0051 | +0.0030 |

What this shows:
- Extraction is converged in record length; values move by under 1e-3 from 700 to 5600 steps.
- CPML thickness and domain move the amplitude by about 0.003–0.004, the same size as G9's box effect.
- No hidden bug. The residual is the usual α = 0 low-frequency CPML tail: `alpha_max` removes it, and the far field does not change.

Changes needed:
- Set G12 to `RecordDecay::Pulse`.
- Correct the G12 row in `fdtd3d_phase5_impl.md`, including the 2.75e-3 / 2.40e-3 figures and the σ rationale.
- Keep 1e-3.

**C2. `loss_separates_accepted_power_from_flux` also uses `decay = off` without need, and it checks no balance.**

With `Pulse` it passes: surface 4.6e-4, port V 7.4e-10, I 2.0e-9. σ = 5 S/m around the port gives Pacc/Pflux = 21,872 with |S11| = 0.35. The test checks only signs (`Pacc > Pflux`, `G < D`) and a 1e-3 separation.

Recommendation:
- Use `Pulse`.
- Add a quantitative loss gate. For example, a weaker σ (Pacc/Pflux of about 1.2–3) and Pacc − Pflux against an independent `½ Σ σ|E|² ΔV` from DFT'd interior E. A smaller alternative is to keep the qualitative test and document it as qualitative.

## Nonblocking findings

- **N1. Throughput ±3% is consistent with a pass but not resolved.** Interleaved in one session, #142 head (874c4ab) vs d0d1bfe, `bench_fdtd`, vacuum CPML off (Mcell/s):
  - Old 354.0, 365.4, 346.9; new 352.2, 359.4, 365.9. Means +1.1%, best-of-3 +0.1%.
  - CPML on: old 137.8, 142.2, 132.4; new 134.8, 140.2, 144.0.
  - Run-to-run spread is about 5%, so the ±3% bound is not proven. No regression is visible.
  - The 1-frequency NTFF gate (≤ 10% slower) passes: 98.0%, 94.2%, 95.6% of off. Eight frequencies: 80–83%, not gated.
- **N2. NaN fields are not flagged.** `accumulate` (`ntff.rs` ~166) and `tail_over_peak` use `f64::max`, which drops NaN. A blown-up grid could pass the decay gate and return a NaN pattern without error. `directive_gain` (`ntff.rs:610`) returns NaN for NaN or ≤ 0 power and 0 for +Inf power. Suggest an `is_finite` check on `prad` and on the decay trace in `far_field`.
- **N3. Coverage gaps (the gates that exist are real):**
  - L1 checks field noninterference only for vacuum with a soft source. Nothing covers a port, media or PEC run with NTFF on vs off.
  - L5 tests one cell on the x-lo side. Nothing covers the hi side, `mu`, or `sigma` maps.
  - L6 does not test a port outside the box.
  - L9 does not test the port V/I decay refusal.
  - No script test reads `gain`, `realized_gain` or `s11` with a port. The example covers Pacc/Pflux only.
- **N4. Self-confirming checks (labeled honestly in the code):**
  - G10's `Gr/(G(1−|S11|²))` at 1e-9 is algebra.
  - G11 compares `port_power` with `s11()` on the same records, which checks the definitions, not the physics.
  - G10's Pacc/Pflux ≤ 1e-3 and G7's pattern-vs-flux are independent and carry the weight.

## Checked and correct

- **Surface currents.** Per face and per component, against `J = n̂×H` and `M = −n̂×E`:
  - z: `Jx = −sHy`, `Jy = sHx`, `Mx = sEy`, `My = −sEx`.
  - x: `Jy = −sHz`, `Jz = sHy`, `My = sEz`, `Mz = −sEy`.
  - y: `Jx = sHz`, `Jz = −sHx`, `Mx = −sEz`, `Mz = sEx`.
- **H averaging and lattice.** The two-plane H averages land on the E sample position. The half-cell lattice indices match the stagger. Trapezoid weights halve only along node axes.
- **Flux.** `−½Re(E·(sjH)*)` equals `½Re((E×H*)·n̂)`.
- **Timing.** E is at `s·dt` and H at `(s−½)dt`. `steps` is incremented before accumulation. The port uses V at `(n+1)dt` and I at `(n+½)dt`. sin/cos are recomputed each step, so there is no recurrence drift. G1 phase is +0.0009 rad.
- **Radiation formula.** `Fθ = −jk/4π(Lφ+ηNθ)`, `Fφ = jk/4π(Lθ−ηNφ)` with `+jk r̂·r′` in N and L.
- **Independent transform.** The reviewer probe (`fdtd3d_phase5_review/ntff_probe.rs`, Grid A, separate code) gives:
  - At λ/Δ = 30: |F/Fan| 1.00400, arg +0.0009, ripple 2.14e-3, shape 3.32e-3, P/Pan 1.00505, D 1.50442. These match the implementation's 0.0040 / 0.0009 / 2.14e-3 / 3.32e-3 / 0.0050 / 1.504421869.
  - Port case: Pacc/Pflux 0.99994–1.00000.
- **Validity checks.** All six CPML faces, homogeneous exterior plus the one-cell inner layer, background σ = 0, drivers and PEC strictly inside, 10 cells per background λ, and Nyquist. All are rejected at `steps == 0` before any field write.
- **Split runs.** L2 compares the whole far field bit for bit. NTFF off leaves fields bit-identical (L1). The Phase 1 golden test still passes.
- **G8 wording.** `D(θ) = 1.5 sin²θ` is pointwise 1.5 only at 90°, so "max |D/1.5 − 1|" would be 1 at the poles. The corrected gate is peak `|Dpk/1.5−1| ≤ 0.01` plus `|D/(Dpk sin²θ)−1| ≤ 0.01` on 5°–175°, at λ/Δ = 30 and 40 with the tolerance unchanged. λ/Δ = 20 is excluded because it measures 1.02e-2 / 1.32e-2. The probe agrees: D_flux 1.515 at λ/Δ = 20. The correction is fine.
- **Gate inventory.** G1–G12, the loss case and L1–L10 are all in `ntff_tests.rs` with the brief's tolerances. Five script tests are in `rustlab-script/tests/fdtd.rs`.
- **Phase 4.** `port.rs` changed one line, the visibility of `tail_over_peak`. Results: release `rustlab-dsp --lib fdtd` 80 passed, 1 ignored (the Phase 3 reference run); `rustlab-script --test fdtd` 34 passed.

## CI

PR #143 at d0d1bfe: `cargo-test / test` success, `validate-notebooks / validate` success.

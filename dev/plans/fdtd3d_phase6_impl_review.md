# FDTD Phase 6 — implementation review

Independent physics and engineering review of draft PR #144 at
`7342af5aecd5c438ccfc3af7b34847c6e263ea0e` against
`dev/plans/fdtd3d_phase6_impl.md` (the brief). Review only; no
implementation code was changed. All runs below were on the same 4-vCPU
cloud VM, release build, `cargo +stable`.

## Verdict: NOT READY

The physics is sound and the capstone gates are tested as written. Two
things block it:

1. **Both performance misses come from the code, not the host.** The CPML
   correction is still serial and takes about 40% of a serial step on the
   reference mesh, so Amdahl caps the 4-thread speedup near 1.7×. On the
   lesson mesh the pool gives no gain at all. The default (`threads = 0`)
   also makes the 60×60×20 CPML parent gate about 2× slower than 0.3.63.
   The brief already named the fix (§7.3: parallelise `correct_h` /
   `correct_e` "if R2/R6 profiling shows it matters") — it does.
2. **The P1 bit-identity test never splits a field update into more than
   one task.** Every P1 fixture is a single rayon task, so a wrong chunk
   offset for any task after the first would still pass. A temporary
   multi-task check I ran passed, so this is a proof gap, not a known bug.

Once those are fixed (or Michael explicitly waives R2/R4 with the numbers
below), the rest is nice-to-have.

## 1. Gates as written

All capstone gates are implemented at the tolerances in the brief and are
not loosened. Specifics:

| Gate | Where | Finding |
|---|---|---|
| C3 port decay | `assert_capstone`, example lines 96–103 | `tail_over_peak` ≤ 1e-3 on both V and I. |
| C3 NTFF surface decay | `pattern_at(.., RecordDecay::Pulse)`, example `fdtd_farfield` default | `far_field` refuses an undecayed surface, so the `unwrap` is the gate. Correct. |
| C4 passivity | `assert_capstone` line 479, example line 133 | Checked over 1.5–3.5 GHz **after** the decay asserts in the same function, so only on a decayed record. The reference test also checks the 4,000-step prefix has max\|S11\| > 1 and that `S11Spec::pulse()` refuses it. |
| C5 power balance | `pattern_at` | ≤ 1e-2 at **every** armed frequency, not just the dip. |
| C6 Psph/Pflux | `sphere()` | 61 θ × 120 φ trapezoid, ≤ 5e-3, air_z = 20 mm. The 10 mm diagnostic pins the known overshoot (1.0201). |
| C7 broadside | `pattern_at` | Requires the peak at θ index 0 exactly; stricter than "one grid step". |
| C9 | `pattern_at` | Cross-pol at θ = 0 ≤ 1e-6, and H-plane mirror φ = 90° vs 270° (indices 30 and 90 on the 3° grid) ≤ 1e-6. Matches the brief. |
| C10 | `pattern_at` | `D(0)/D(180°)` ≥ 10 dB. |
| C11 D / G / Gr | `pattern_at`, example | Three separate normalisations (`prad`, `accepted`, `incident`). \|G−D\| ≤ 0.05 dB at every frequency. |
| L1 η | `substrate_loss_…`, example | 0.40–0.60; measured 0.489. |
| L3 dissipation | `substrate_loss_…` | End-of-step DFT, σ = arithmetic edge mean of the **material** map (port gap σ excluded), ½σ\|E\|²ΔV. ≤ 1e-2; measured 3.261e-3 over 8,972 edges. Same E-sampling convention as the NTFF phasors. |
| L4, L5 | both | Dip within 0.5%, deeper match, \|D_lossy − D_lossless\| ≤ 0.05 dB. |

Weak spots (not weakenings):

- **The lossy test hardcodes 2.38 GHz** (`q_near`) instead of choosing the
  armed frequency nearest the dip, as L1 says. For the lesson mesh, the dip
  (2.3749 GHz) is nearest 2.38 anyway, so the result does not change, but
  the test would silently compare the wrong bin if the mesh changed. The
  example does it right (`argmin(abs(pat1.freq - f1_dip))`).
- **The C11 realized-gain identity and L2 are true by construction**:
  Gr − G = 10 log10(1 − \|S11\|²) and G = D + 10 log10 η follow from the
  definitions. Keep them as plumbing checks, but they are not physics
  evidence. C11's \|G − D\| ≤ 0.05 dB on the lossless run is C5 restated
  (a 1e-2 balance is 0.043 dB).
- **P2 runs 40 steps split 15 + 25**, not the brief's `run(150)` split
  70 + 80. The split point is what matters, so this is fine, but it shares
  P1's single-task fixture (see §3).

## 2. Physics

Geometry: the brief is followed. FR-4 εr = 4.4, h = 1.6 mm. The ground
plane is 16 mm larger than the design patch on each side. There is 10 mm of
lateral air, 20 mm of vertical air, and an 8-cell CPML with a 2-cell margin.
The series 50 Ω lumped port is a stack of `ez` edges from the ground node to
the patch node, at the first metal node of the centre row (the radiating
edge, on the y mirror plane). The ground and the patch are node-rule
`fdtd_pec` faces.

**Independent reproduction (Python, no rustlab code):**

| Quantity | Brief / implementation | Reproduced |
|---|---|---|
| Design W | 38.01 mm | 38.01 mm |
| Design L | 29.42 mm | 29.42 mm |
| ε_eff | 4.0857 | 4.0857 |
| ΔL | 0.739 mm | 0.739 mm |
| TL+cell, lesson metal 28 × 36 mm, d = 2 mm | 2.3559 GHz | 2.3559 GHz |
| TL+cell, reference metal 28 × 38 mm, d = 1 mm | 2.4314 GHz | 2.4314 GHz |

The lesson dip is 2.3749 GHz (+0.81% vs TL+cell) and the reference is
+0.24%. Both are within the 1.5% gate. The reference matches Phase 4's
2.4395 GHz to −0.094%.

**Efficiency, a sharper check.** In the cavity model the edge resistance
at resonance scales as 1/G_total, so with loss
`R_lossy / R_lossless = G_rad / (G_rad + G_d) = η`. If the dips sit at
nearly real impedance, the measured minima give
R = 50(1 + \|Γ\|)/(1 − \|Γ\|): 206 Ω lossless (\|S11\| 0.610) and 101 Ω
lossy (0.339). The ratio is **0.491**. Power balance gave
**η = 0.489** from a completely separate path (NTFF flux / port power).
They agree to 0.4%.

The closed form is less precise. Jackson–Alexopoulos Q_space at 2.375 GHz
is 70 for both the design and the staircased dimensions. That gives
η = 0.41–0.42 with tanδ = 0.02 and no surface-wave correction. With the
brief's e_sw ≈ 0.9, it gives about 0.44. The brief's 0.47–0.51 assumed
L × W ≈ 29 × 39 mm at 2.44 GHz. The FDTD's 0.489 is 5–15% above these
closed forms and well inside the 0.40–0.60 gate. That spread is normal for
Q formulas on a finite ground.

The Balanis edge resistance, 1/(2(G₁ + G₁₂)), is 330–354 Ω. That is
higher than the 206 Ω the FDTD dip implies. The model ignores the finite
ground, the lumped-gap reactance, and the fact that the dip need not sit
at a purely real Z. This is informational only, not a gate, but the
lesson text should not quote the Balanis number as the expected edge
impedance.

D = 6.94 dBi, F/B 13–14 dB, cross-pol around 1e-17, and Psph/Pflux 1.0003
are all what a patch on a ground about λ/2 larger than the patch should
give. The odd/even width test (tail 2.635e-3 vs 4.930e-5) correctly pins
the TM01 ring-down from an off-mirror feed.

## 3. Parallel stepping

**Safety.** There is no `unsafe` in `fdtd/`. The H half-step writes
disjoint `par_chunks_mut` row blocks of `hx/hy/hz` and reads only E; the E
half-step is the mirror. The borrow checker therefore rules out data races.
The chunk-to-row mapping is correct by reading:
`row0 = cidx·rows_per`, `(k, i) = (row/ny, row%ny)`, and `Dims::row(i, k)`
`= (k·ny + i)·nx = cidx·chunk + t·nx`. The short last chunk uses
`len/nx`. The serial and parallel paths call the same `update_*_row`.

**Determinism.**

- The NTFF accumulators are per sample, with the same expression.
- The decay trace is a `reduce(|| 0.0, f64::max)`, while the serial path
  folds `emax.max(..)` starting at 0. `f64::max` ignores NaN on either
  side and \|E\| ≥ +0, so the result does not depend on order, including
  with NaN.
- `far_field` parallelises over (frequency, θ) rows. Each direction still
  sums samples in sample order (`direction_field`).
- CPML, sources, port, and probes stay serial.

Bit identity holds by construction.

**Proof gap (required fix).** `rows_per_task = 65_536 / nx`. The P1
fixtures are 16×14×12 (168 rows), 20×18×16 (288 rows), and the tiny patch
(under 20,000 cells). Each needs 3,276–4,096 rows per task, so **every P1
field update is one task.** `par_hits > 0` proves the parallel driver ran,
but not that more than one task did. A bug that only affects chunks after
the first (for example `row0 = cidx`) would pass P1 at 1–4 threads. The
NTFF and far-field halves of P1 do split across tasks: rayon splits the
sample iterator, and `far_field` has 9 rows of 3.

Independent check: a temporary, uncommitted test on the **lossy** lesson
mesh. It is 60×66×72 = 285,120 cells, 4,752 rows at 1,092 per task, so
**5 tasks**, with port, CPML, σ > 0, and NTFF at 4 frequencies. It ran
1,500 steps (max\|V\| 0.32, so the pulse is well inside the grid) at 2, 3,
and 4 threads. `par_hits` was 3,000, exactly two per step. All six fields,
port V and I, the NTFF E and H phasors, the decay trace, and the full
61 × 120 `far_field` (Eθ, Eφ, prad) were **bit-identical** to `threads = 1`.

P3–P5 are correct. P5 polls the stop callback on the calling thread
between steps.

## 4. Performance misses

Measured with a temporary benchmark (uncommitted, same builder as
`bench_fdtd patch`). Each mesh was timed with CPML and NTFF toggled,
`threads = 1` against the pool:

| Mesh | CPML | NTFF | Serial Mcell/s | Pool Mcell/s | Speedup |
|---|---|---|---|---|---|
| Reference 102×110×124 (1.39M cells, 22 tasks) | off | off | 232.3 | 758.1 | **3.26×** |
| | on | off | 138.5 | 225.7 | 1.63× |
| | on | on | 120.5 | 198.8 | 1.65× |
| Lesson 60×66×72 (285k cells, 5 tasks) | off | off | 228.5 | 621.4 | 2.72× |
| | on | off | 118.2 | 114.9 | **0.97×** |
| | on | on | 95.4 | 101.4 | 1.06× |

These were 600 / 3,000 steps, so the absolute rates differ from the PR's
2,000-step 1.89×. The pattern is what matters:

- **The curl alone scales 3.26× on 4 vCPU.** The host is not the limit;
  the brief's "~3.4× aggregate ceiling" is not being hit.
- **The CPML correction is serial.** On the reference mesh it is
  1 − 138.5/232.3 ≈ **40%** of the serial step. Amdahl with 60% at 3.26×
  predicts 1/(0.4 + 0.6/3.26) = **1.71×**; measured 1.63–1.65×. That *is*
  the R2 miss.
- **On the lesson mesh the CPML share is larger.** It has 8 of 72 z layers
  per side plus the x/y slabs, and the curl gets only **5 tasks for 4
  threads**, so one thread runs two. The pool buys nothing (0.97–1.06×).
  So R4 (`patch_antenna.rlab`, ≤ 35 s parallel / ≤ 60 s serial) misses: the
  example effectively runs serially, and the 43.8 s lossless half alone is
  already over 35 s.
- **Default regression.** `bench_fdtd gate` (60×60×20 vacuum + six-face
  CPML, 8,000 steps) takes **9.0 s at 64.0 Mcell/s** at this head. Phase 3
  through 0.3.63 recorded 4.6 s. 72,000 cells is above the 32,768-cell
  threshold, but that is 1,200 rows in **2 tasks**, and the serial CPML
  dominates. A user with a mid-size grid gets about 2× slower stepping by
  default in 0.3.64. R1 (< 60 s) still passes, but this is a user-visible
  regression.

The implementation tried only the chunk size (16k → 64k cells: 1.82 →
1.89×). The brief's order (unit stride, autovectorisation, chunk size,
threshold) does not reach the real term. §7.3 and §8 item 9 make the CPML
correction conditional on profiling, and the profile says to do it.

Recommendation:

1. **Parallelise `correct_h` / `correct_e`** with the same row split. When
   CPML is on, the `q` arrays are full grid size (`cpml.rs`
   `qlen = nx·ny·nz`), so each (k, i) row owns its own field row and `q`
   rows. Bit identity follows as for the curl. Expected reference step: the
   CPML term would scale like the curl, so about 2.5–3× overall.
2. **Size tasks by count, not just cells.** For example,
   `rows_per_task = clamp(rows / (4·threads), 1, 65_536/nx)`, so that the
   lesson mesh gets about 16 or more tasks. Chunking cannot change the
   bits.
3. **Don't take the pool when it cannot pay.** Raise the threshold or
   require a minimum task count, so that the parent gate is back at about
   4.6 s with `threads = 0`.
4. Re-measure R1, R2, R4, and R6. If R2 or R4 still miss after (1)–(3),
   record that as the host limit with these numbers and ask Michael to
   waive it.

## 5. CI fix: skipping the two examples in the dual-storage test

The decision is sound; the comment overstates it.

- The claim "their stepping runs in Rust, so array storage mode does not
  affect them" is only partly true. Both examples also build `ones3` /
  `zeros3` ε and σ maps, `meshgrid`, and `rect_mask` masks in script
  arrays, pass them into `fdtd_media` / `fdtd_pec`, and post-process S11
  and \|E\|² sums in script. Those paths do depend on storage mode. The
  conversions are exact (real storage, or complex with im = +0), and the
  `real_ops` kernels are tested bit-equal, so the risk is low.
- Five other FDTD examples (`fdtd_hello`, `fdtd_cavity`, `fdtd_pml`,
  `fdtd_port`, `fdtd_dipole`) still run in both modes and cover `fdtd_media`,
  `fdtd_pec`, `fdtd_port`, `fdtd_s11`, and `fdtd_farfield`. `fdtd_sweep`
  also exercised `parmap` under both modes; `parmap` is covered by other
  examples.
- **The skip loses no gate enforcement, because there never was any.**
  `storage_modes.rs` compares the two runs' `ok` flags and outputs; a script
  that `error()`s identically in both modes passes. So the examples' physics
  `error()` checks were never enforced by `cargo test`.

Nice-to-have: reword the comment to "their cost is the Rust stepper; the
script-side inputs and post-processing are covered by the other FDTD
examples". The named list and the existence assertion are good.

## 6. Test coverage

What CI actually enforces for Phase 6:

- `cargo test`: the P1–P5 tests (with the single-task gap above), the tiny
  patch build, NTFF box validation, the 400-step cross-pol check, the
  `threads` option, and `parmap` vs `arrayfun`.
- **None of the C, L, S, or R gates.** The five physics tests are
  `#[ignore]`. The examples' `error()` gates are not enforced (§5).
  `validate-notebooks` renders without `--strict`, so a notebook cell that
  `error()`s still renders and validates. That workflow also only triggers
  on `crates/rustlab-notebook/**`, `crates/rustlab-plot/**`, and
  `examples/notebooks/**`, never on `crates/rustlab-dsp/**`.

So a change to the stepper that broke the patch resonance would pass CI.
Gates are enforced only by `dev/run-examples.sh` and the release tests run
by hand. This was already true for Phases 4 and 5. Phase 6 is the first
phase where the parallel kernel can change results only on large grids,
which makes it riskier.

S6 runs only the serial kernel: the trial grid is 12×10×8 with
`"threads", 1`. The "parallel kernel under `parmap`" half of S6 and the S7
nested-pool rule have no test (brief test 17 was not written). They are
exercised only by `fdtd_sweep.rlab`, whose 5 trials step on the global pool
inside `parmap`. The test also compares `max(abs(A − P)) == 0`, not bits.

## 7. Tests that may not fail without the feature

No separate failing-test-first commits, so judged by reading:

| Test | Would it fail without Phase 6? |
|---|---|
| `parallel_step_matches_serial_bitwise` | Fails if the parallel driver is missing (`par_hits > 0`). **Would not catch a chunk-offset bug** (single task). |
| `small_grid_stays_serial` | Passes trivially with no parallel path. Meaningful only paired with P1. |
| `parallel_split_run_identity` | Same single-task fixture. Would pass with a broken multi-task mapping. |
| `parmap_of_fdtd_matches_arrayfun` | Passes on 0.3.63 apart from the `"threads"` keyword. Serial kernel only. |
| `floating_patch_box_passes_ntff_validation`, `ground_on_the_box_is_refused`, `patch_feed_on_mirror_plane_has_no_cross_pol` | Pass on Phase 5 code. Regression pins for the new geometry, not tests of new code. That is acceptable. |
| C11 realized-gain identity, example L2 | True by construction. |
| `odd_width_patch_rings_in_the_width_mode`, `air_z_10mm_…` | Physics pins of known behaviour; fine as such. |
| Release capstone / lossy / sweep gates | Real tests of physics, but run by hand only. |

## Required fixes (priority order)

1. **Parallel CPML correction, task-count sizing, and no default slowdown**
   (§4 items 1–3). Re-measure R1, R2, R4, and R6. Restore the
   60×60×20 / 8,000-step CPML gate to about 4.6 s with default threads.
   If R2 or R4 still miss after that, get an explicit waiver from Michael
   with the numbers.
2. **Make P1 and P2 multi-task.** Add a test-only `rows_per_task` override
   (thread-local, like `PAR_CELLS`) set to 1–3 rows, so every fixture spans
   many tasks at 2–4 threads. Assert the task count is > threads, or add a
   fixture over 65,536 cells. Use the brief's 150 = 70 + 80 split. Once
   CPML is parallel, P1 must also cover the parallel correction (the CPML
   and port fixtures already compare every `q` array).

## Nice-to-haves

- Write brief test 17: a `parmap` builder above the threshold with
  `threads = 0`, compared bit for bit (`to_bits`) against `arrayfun` and
  against `threads = 1`, plus the S7 pool assertions.
- Choose the lossy test's frequency from the dip, not a hardcoded 2.38 GHz.
- Add a scheduled or `workflow_dispatch` job that runs the five `#[ignore]`
  release tests. Or render the two FDTD notebooks with `--strict` in that
  job, so the gates are enforced somewhere other than by hand.
- Reword the `SKIP_LONG_FDTD` comment (§5).
- In the lesson text, cite the lossy/lossless edge-resistance ratio (0.491)
  as the independent check on η = 0.489, and avoid quoting the Balanis
  edge resistance as the expected value.

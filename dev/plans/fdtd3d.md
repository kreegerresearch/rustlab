# Development Plan: Native 3-D FDTD (Yee + CPML + port + near-to-far)

**Status:** draft — plan only, no code
**Current phase:** not started
**Asked by:** Michael, for a real microstrip patch lesson in `kreegerresearch/rustlab_em` (resonance, S11, 3-D far-field / gain)

A hand-rolled Yee loop in `.rlab` over `zeros3` / `Tensor3` cannot hit the lesson. This plan is the native stepper that can: a 60×60×20 grid for 5–10k steps in seconds to a minute, pure Rust, no new dependencies.

Do not implement a phase until the open questions at the bottom are answered and Michael says to start. Per the usual plan protocol, finish a phase, run `cargo test --workspace`, and wait for an explicit yes before the next one.

---

## What already exists

There is no FDTD, Yee, PML, or near-to-far code in this repo. The pieces the stepper should sit next to:

| Piece | Where | What it actually is |
|---|---|---|
| `Tensor3` | `Value::Tensor3(Shared<CTensor3>)`, `CTensor3 = Array3<C64>` | Script array, shape `(rows, cols, pages)` = `(ny, nx, nz)`. Axis 0 = y, axis 1 = x, axis 2 = z. 1-based `A(i,j,k)`. `ndarray` standard layout is **z-contiguous**. Real storage exists (`Shared::as_real`, `Value::real_tensor3`) and must not be widened just to read a material map. |
| `zeros3` / `ones3` / `reshape` / `cat` / `permute` | `builtins.rs` | Constructors. `meshgrid` is **2-D only**. |
| `laplacian_3d` | `rustlab-dsp/src/laplacian.rs` | 7-point **sparse** stencil, flat index `k = (kk*nx + j)*ny + i` (y fastest). Not a time stepper. `laplacian_value` stamps `OrderingHint::Identity`. |
| `laplacian_eps_2d` | same file | `∇·(ε∇)` with harmonic-mean face coefficients. 3-D form was requested and is **not** implemented. |
| `gradient3` / `divergence3` / `curl3` | `rustlab-dsp/src/vector_calc.rs` | Collocated 2nd-order differences on `Tensor3`, complex, rayon above 4096 elements. Useful as a reference for axis order. **Not** a Yee curl (those operators are not staggered, and they allocate). |
| `rect_mask` / `disk_mask` / `polygon_mask` | `rustlab-dsp/src/rasterize.rs` | 2-D masks on `meshgrid` output. The patch rectangle reuses `rect_mask`. |
| `fft` / `ifft` | `rustlab-dsp` | Length-preserving. S11 uses this, not a new FFT. |
| `mag2db`, `plot`, `imagesc`, `surf` | plot builtins | No new plot type. Slices and the gain pattern go through these. |
| Ctrl+C for a long op | `rustlab_plot::InterruptGuard` + `interrupt_requested` | Used by the dense matrix 2-norm (`matrix_spectral_norm` in `builtins.rs`). The handler stores an atomic; the op polls it. `ScriptError::Interrupted` returns the REPL to `>> ` and makes `rustlab run` exit 0. |
| Notebook preemption | `Evaluator.cancel: Option<Arc<AtomicBool>>` | Polled in `check_cancelled` **between statements and loop iterations**, not inside a builtin. A single `fdtd_run(...)` call is invisible to it unless the stepper polls the flag itself. |
| Parallelism already in-tree | `rayon` 1.10 on `rustlab-dsp` and `rustlab-script`; `std::thread` | No SIMD crate. `std::simd` / portable SIMD is not a stable dependency we can add without a nightly feature. Do not add one. |

Workspace crates that must stay out of the hot loop: `rustlab-plot`, `rustlab-notebook`, `rustlab-viewer`. The stepper returns numbers; existing plot builtins draw them.

### Sibling-repo requests (read 2026-11-02)

`kreegerresearch/rustlab_em` is public. Two requests matter:

1. **`dev/rustlab/requests/laplacian-eps-3d.md`** (filed 2026-07-10, Lesson 15 MIM stack and a 3-D exercise). API they asked for:

   ```text
   A = laplacian_eps_3d(eps_map, dx, dy, dz)
   A = laplacian_eps_3d(eps_map, dx, dy, dz, bc)
   ```

   `eps_map` is a `Tensor3` (axis 0 = y, axis 1 = x, axis 2 = z), real or complex. Face coefficients are the harmonic mean, same as 2-D. Ordering is `ijk2k`. Anisotropic `dx`, `dy`, `dz` are required. Validation: uniform ε matches `eps * laplacian_3d`; a two-layer stack between `pin_dirichlet` plates satisfies `1/C = 1/C1 + 1/C2`; complex ε still solves. Axisymmetric `∇·(μ⁻¹∇)` is explicitly out of scope there. Phase 0 of this plan is that request.

2. **`dev/rustlab/requests/yee-and-pml-builders.md`** (Phase 1 landed in the curriculum as scripted 2-D **FDFD**: `pml_sigma_profile`, `fdfd_tmz_pml_2d`). Phase 2 was a hypothetical `rustlab-em` crate of **sparse** curl matrices (`yee_curl_2d`, `scpml_stretch`) behind a default-off feature, blocked on a graduation trigger.

This plan does **not** build those sparse operators. Michael's ask is a time-domain loop, which is a different algorithm: the cost is 5–10k field updates, not one matrix assembly. Sparse 3-D Yee-curl for FDFD stays a separate request if a later lesson wants frequency-domain assembly. The old "script it in `em.rlab`" path does not meet the cell-update target, so it is not the proposal here.

`dev/plans/em_lesson_review_2026_07.md` already records `laplacian_eps_3d` as a real gap that was left out of that batch on purpose.

---

## Recommendation

Put the math in `rustlab-dsp` (`src/fdtd/`), and the script surface in `rustlab-script` as an opaque handle plus ordinary structs of `Tensor3` / vectors. Do **not** add a crate, a feature flag, or a dependency.

Reasons:

- `laplacian`, `vector_calc`, and `rasterize` already live in `rustlab-dsp`. The curriculum binary is `rustlab`, which already links that crate. A default-off `em` feature would make a normal `cargo install` fail the lesson.
- The hot state cannot be six script `Tensor3`s. A `.rlab` loop that calls a builtin 10k times is acceptable only because each step is a native kernel; the fields themselves stay in Rust between steps. Snapshots copy out on demand.
- `rayon` is already a `rustlab-dsp` dependency. `std::thread` is std. Both are allowed. Nothing else is needed.
- Estimated code size in the release binary is well under a megabyte of straight-line numeric code (the plotly / notebook decision was about megabytes of assets, not this).

UPML is rejected for v1. CPML (Roden & Gedney) leaves the interior update identical to vacuum Yee and adds auxiliary variables only where the absorber is. UPML rewrites the material update throughout the layer and is harder to check against the vacuum kernel. Same absorbing job, more ways to get the sign wrong.

---

## Proposed `.rlab` API

Names are new; nothing here collides with a registered builtin. Toolbox: **`pde`**, new subcategory `FDTD (3-D)`, plus `laplacian_eps_3d` next to the existing Laplacian row. `gaussian_pulse` can sit in that subcategory too (it is the source waveform, not a general DSP window).

Indexing is 1-based, matching the rest of the language. Component names are `"ex"`, `"ey"`, `"ez"`, `"hx"`, `"hy"`, `"hz"`.

### Handle

```text
sim = fdtd3d(nx, ny, nz, dx, dy, dz)
sim = fdtd3d(nx, ny, nz, dx, dy, dz, "courant", 0.99)
```

`sim` is an opaque value (`type_name` → `"fdtd3d"`), `Arc<Mutex<...>>` like `FirState`. Assignment shares the same grid; it does not copy. `save` / `load` / arithmetic / `+` error with a clear message. The grid is created with E = H = 0 and no materials (vacuum, no PML, PEC outer boundary).

`dx`, `dy`, `dz` are metres, finite and positive. `nx, ny, nz >= 2`.

```text
info = fdtd_info(sim)
```

Struct: `nx`, `ny`, `nz`, `dx`, `dy`, `dz`, `dt`, `courant`, `steps` (completed), `bytes` (field + coefficient + PML storage), `c0`.

`dt` is fixed by the CFL limit at media-bake time (see kernels). Default Courant number is 0.99. Passing `"courant", s` with `s` in `(0, 1]` overrides it.

```text
fdtd_reset(sim)
```

Zeros fields, probes' recorded samples, port records, and the step counter. Keeps materials, PEC, PML, port geometry, and the waveform definition.

### Materials and metal

```text
fdtd_media(sim, eps_r, mu_r, sigma)
```

Each argument is a **real scalar** or a real `Tensor3` of shape `(ny, nx, nz)`.

- `eps_r` — relative permittivity, cell-centred, must be finite and `> 0`.
- `mu_r` — relative permeability, same, finite and `> 0`.
- `sigma` — electric conductivity in S/m, finite and `>= 0`. This is how FR-4 loss is expressed (`sigma = ω ε0 εr tanδ` at the design frequency). A complex `eps_r` is an error that says to use `sigma`. Time-domain Yee does not take a complex permittivity. (`laplacian_eps_3d` still does; that operator is frequency-domain / static.)

Calling `fdtd_media` bakes per-edge coefficients and recomputes `dt` from the fastest wave speed on the grid. Allowed only when `steps == 0`; otherwise error and point at `fdtd_reset`.

```text
fdtd_pec(sim, mask)
fdtd_pec(sim, "z", k, mask2d)
fdtd_pec(sim, "x", i, mask2d)
fdtd_pec(sim, "y", j, mask2d)
```

- Volume form: `mask` is a `(ny, nx, nz)` tensor. Nonzero cells are PEC. Every E edge that belongs to that cell is held at 0 (not updated).
- Face form: a 2-D mask of tangential E on one staggered face. `"z", k` zeros `Ex` and `Ey` on z-face `k`, with `k` in `1 .. nz+1` (the extra face is the top boundary; see the stagger table). The ground plane is `fdtd_pec(sim, "z", 1, ones(ny, nx))`. The patch is `fdtd_pec(sim, "z", nsub+1, rect_mask(...))`.

PEC is applied after the E update and also skips those edges inside the update, so a metal face does not get a one-step leak. H is not forced; the surrounding E = 0 is what enforces the tangential-E condition.

### Absorber

```text
fdtd_cpml(sim, n)
fdtd_cpml(sim, n, "sigma_factor", 1, "kappa_max", 7, "alpha_max", 0.05, ...
               "order", 3, "alpha_order", 1)
fdtd_cpml(sim, "zmin", 0)          % turn one face off
```

`n` is the number of cells, same on every face that is still on. `n = 0` removes the absorber. Face names: `xmin`, `xmax`, `ymin`, `ymax`, `zmin`, `zmax`. A face set to 0 is a PEC wall (the outer boundary already is). The patch lesson turns `zmin` off and lays a PEC ground on z-face 1.

Defaults are in the CPML section. Unknown keys error. Like media, PML geometry is baked only at `steps == 0`.

### Sources

```text
y = gaussian_pulse(t, t0, tau)
y = gaussian_pulse(t, t0, tau, f0)
```

`t` is a real scalar or vector (seconds). `tau > 0`, `t0` finite, `f0` finite (default 0).

```text
y = exp(-((t - t0) / tau)^2) * cos(2π f0 (t - t0))
```

`f0 = 0` is a baseband Gaussian. This is a pure function; it does not touch `sim`.

```text
fdtd_source(sim, "ez", i, j, k, "soft", "gaussian", t0, tau)
fdtd_source(sim, "ez", i, j, k, "soft", "modulated", t0, tau, f0)
fdtd_source(sim, "ez", i, j, k, "hard", waveform)
fdtd_source(sim, "ez", i, j, krange, "soft", waveform)
```

- `"soft"` adds the source into the update (the field keeps its history).
- `"hard"` overwrites that E edge every step. Useful for a cavity ring-down. A hard source reflects; do not use it as an antenna port.
- `waveform` is a real vector sampled at `sim.dt`, sample 1 at step 1. Past the end of the vector the source is 0.
- `"gaussian"` / `"modulated"` are sampled internally at `sim.dt` for however many steps `fdtd_run` asks for. The pulse is centred on `t0`.
- A range on the last index stacks several collinear E edges (the feed through a substrate). Soft sources on a stack share one voltage, divided across the cells.

Several soft sources may coexist. A hard source and a port on the same edge is an error.

### Lumped port

```text
id = fdtd_port(sim, "ez", i, j, k, Z0, "modulated", t0, tau, f0)
id = fdtd_port(sim, "ez", i, j, krange, Z0, waveform)
```

`Z0` is a real positive resistance in ohms (50 for the lesson). The port is a soft source in series with `Z0`:

- The gap resistance is split equally across the edges in `krange`.
- Each edge's extra conductivity is `σ = Δ_along / (R_edge · Δ_cross1 · Δ_cross2)`, baked into that edge's `Ca`, `Cb` (same formulas as a material σ).
- The source term is the open-circuit voltage of the series source, scaled into the E update (kernel section).
- `id` is a 1-based scalar. v1 may create more than one port; `fdtd_s11` defaults to port 1. A full multi-port S matrix (calibration thru) is **not** v1.

Voltage and current are recorded every step on the handle:

- `V = −Σ E_along · Δ_along` over the stack, sampled after the E update (integer time).
- `I` is the discrete Ampère loop of the H edges around one cell of the stack (the first cell), sampled at the half-step just used. The half-step stagger is corrected in `fdtd_s11`, not in the stored `I`.

### Probes and snapshots

```text
id = fdtd_probe(sim, "ez", i, j, k)
rec = fdtd_record(sim)
```

`fdtd_record` returns a struct:

| Field | Contents |
|---|---|
| `t` | real row, seconds, length `steps`, sample `n` at time `n·dt` |
| `dt` | scalar |
| `steps` | scalar |
| `probe` | real matrix, `steps × nprobes`, column `id` is that probe's E or H component |
| `V`, `I` | real matrices, `steps × nports` |
| `Z0` | real row, one resistance per port |

Probes store the component after the half-step that updated it.

```text
M = fdtd_slice(sim, "ez", "z", k)     % matrix, y-rows × x-cols, real storage
F = fdtd_fields(sim)                  % struct of six Tensor3, real storage
```

`fdtd_fields` copies the whole grid. Fine for debugging and for a figure; do not call it every step of a 10k run. `fdtd_slice` is the lesson figure (patch-plane `|Ez|` or a cut).

### Run

```text
out = fdtd_run(sim, nsteps)
out = fdtd_step(sim)                  % one leapfrog step
out = fdtd_step(sim, n)               % n steps; same kernel as fdtd_run
```

Both return the same struct as `fdtd_record`, plus:

| Field | Contents |
|---|---|
| `seconds` | wall time of this call |
| `cell_updates` | `nx*ny*nz*nsteps_this_call` |
| `cell_rate` | `cell_updates / seconds` |

`nsteps` must be a positive integer and the product `nx*ny*nz*nsteps` must pass the existing size check only for the **stored records** (probes and ports are `nsteps` long). The fields themselves are not replicated per step.

There is no `.rlab` callback per step. A callback would put the interpreter back in the inner loop and miss the target. Record probes, then plot after the call.

### S11

```text
s = fdtd_s11(V, I, dt, Z0)
s = fdtd_s11(sim)
s = fdtd_s11(sim, "port", id)
s = fdtd_s11(sim, "window", "hann")
```

Pure form (the one the lesson should show):

```text
Vw = fft(V .* w)
Iw = fft(I .* w) .* exp(j * 2π * freq * dt / 2)    % I was sampled dt/2 earlier
a  = (Vw + Z0 .* Iw) / (2 * sqrt(Z0))
b  = (Vw - Z0 .* Iw) / (2 * sqrt(Z0))
S11 = b ./ a
```

`w` is a Hann window of `length(V)` by default (`"window", "rect"` leaves it off). `freq` is the positive axis including DC and Nyquist, in Hz, from the length-preserving `fft` (bin `m` is `m / (N dt)` for `m = 0 .. floor(N/2)`). Bins where `|a|` is tiny (below `1e-12` times peak `|a|`, or where the source spectrum is in the noise) are returned as `NaN` so a plot does not show a 0/0 spike at high frequency.

Struct: `freq` (real row), `s11` (complex row), `z` (complex row, `Vw./Iw` after the same phase correction).

`fdtd_s11(sim)` reads port 1's stored `V`, `I`, `dt`, `Z0`.

### Near-to-far

```text
fdtd_ntff(sim, freqs)
fdtd_ntff(sim, freqs, "margin", m)
pat = fdtd_farfield(sim, theta, phi)
```

`fdtd_ntff` must be called at `steps == 0`. It arms an on-the-fly DFT at the given real frequencies (Hz, finite, `> 0`, a vector). `margin` (default 2) insets the Huygens box by that many cells from the **interior** face of the PML, or from the PEC wall on a face where PML is off. The box has to sit in a homogeneous region (the lesson: air, above the substrate, inside the PML). If the inset is not in range, error before the run.

`fdtd_farfield` is cheap relative to the run. `theta` and `phi` are real vectors in radians (`theta` from the +z axis, `phi` from +x toward +y). Returns a struct:

| Field | Shape |
|---|---|
| `freq` | the armed frequencies |
| `theta`, `phi` | echoes of the request |
| `Etheta`, `Ephi` | complex `Tensor3` `(nfreq, ntheta, nphi)` — pages are not used; this is a matrix per frequency stored as a tensor so one object holds the sweep |
| `directivity_dBi` | real, same shape. Normalised by the pattern integral of that frequency |
| `realized_gain_dBi` | real, same shape. Directivity scaled by accepted-power fraction `1 − \|S11\|²` of port 1 at that frequency, linearly interpolated in frequency from `fdtd_s11`. `NaN` where `S11` is `NaN` |

No far-field is stored during the run except the surface phasors (see NTFF section). Calling `fdtd_farfield` without `fdtd_ntff` errors.

---

## Patch-antenna script sketch

Not shipped in this PR. This is the shape of `examples/pde/patch_antenna.rlab` once Phase 4 exists. Constants below are the transmission-line model for a 2.4 GHz patch on 1.6 mm FR-4 (`εr = 4.4`); the script computes `L` and `W`, it does not hard-code a published millimetre drawing.

```text
eps_r = 4.4
h     = 1.6e-3
f0    = 2.4e9
c0    = 299792458
W = c0 / (2*f0) * sqrt(2 / (eps_r + 1))
ee = (eps_r+1)/2 + (eps_r-1)/2 / sqrt(1 + 12*h/W)
dL = 0.412*h * (ee+0.3)*(W/h+0.264) / ((ee-0.258)*(W/h+0.8))
L  = c0 / (2*f0*sqrt(ee)) - 2*dL

dx = 1.0e-3
dy = 1.0e-3
dz = 0.4e-3          % 4 cells through the substrate
nx = 60
ny = 60
nz = 20
nsub = 4
npml = 6

sim = fdtd3d(nx, ny, nz, dx, dy, dz)
eps = ones3(ny, nx, nz)
eps(:, :, 1:nsub) = eps_r
fdtd_media(sim, eps, 1, 0)

fdtd_cpml(sim, npml)
fdtd_cpml(sim, "zmin", 0)

fdtd_pec(sim, "z", 1, ones(ny, nx))
xv = (0:nx-1) * dx
yv = (0:ny-1) * dy
[X, Y] = meshgrid(xv, yv)
px = (nx*dx - L) / 2
py = (ny*dy - W) / 2
fdtd_pec(sim, "z", nsub+1, rect_mask(X, Y, px, py, L, W))

% Feed: Ez stack at one edge of the patch, from the ground up to the metal.
ix = round(px/dx) + 1
iy = round((py + W/2)/dy) + 1
tau = 1 / (2*f0)
fdtd_port(sim, "ez", iy, ix, 1:nsub, 50, "modulated", 4*tau, tau, f0)
fdtd_ntff(sim, f0, "margin", 2)

out = fdtd_run(sim, 6000)
s = fdtd_s11(sim)
plot(s.freq/1e9, mag2db(abs(s.s11)))

th = linspace(0, pi, 37)
ph = linspace(0, 2*pi, 72)
pat = fdtd_farfield(sim, th, ph)
```

What this mesh is, and is not:

- Domain is 60 mm × 60 mm × 8 mm. At 2.4 GHz, λ0 is 125 mm, so the box is about half a wavelength across and a few millimetres of air above the patch. That is enough to see an S11 dip near the transmission-line prediction and a broadside lobe. It is not enough to quote gain to a decibel against a textbook pattern.
- The performance gate (below) is this class of grid. A lesson figure that needs a thicker air box is a larger run, still expected to finish in about a minute if the gate is met, and it is a curriculum choice rather than a second kernel.
- `iy, ix` in the call are **row, column** order only in the sense of the signature `fdtd_port(sim, comp, i, j, k, ...)` where `i` is y (row) and `j` is x (column). The sketch follows that. Getting this backwards is an easy lesson bug; the help text should show one numeric example.

Lossy FR-4 is a one-line change the lesson can add later: `sigma = 2*pi*f0*eps_r*8.854187817e-12*0.02` on the substrate pages. The transmission-line formula above is lossless, so the default comparison run uses `sigma = 0`.

---

## Crate and module placement

```text
crates/rustlab-dsp/src/fdtd/mod.rs      public types, bake, step, CFL
crates/rustlab-dsp/src/fdtd/yee.rs      E/H kernels on slices
crates/rustlab-dsp/src/fdtd/media.rs    edge coefficients from cell eps/mu/sigma
crates/rustlab-dsp/src/fdtd/cpml.rs     1-D profiles, ψ update
crates/rustlab-dsp/src/fdtd/port.rs     V, I, series-R source term
crates/rustlab-dsp/src/fdtd/ntff.rs     running DFT + far-field sums
crates/rustlab-dsp/src/fdtd/pulse.rs    gaussian_pulse
crates/rustlab-dsp/src/lib.rs           pub mod fdtd; re-exports
```

No change to `rustlab-core`. The stepper does not need a new `Value` representation down there; it is not a matrix type.

Script side:

- `crates/rustlab-script/src/eval/value.rs` — `Value::Fdtd3d(Arc<Mutex<FdtdSim>>)`. `Clone` is the derived `Arc` clone. `negate` / `binop` / indexing / `save` error. `Display` prints a one-line summary (`fdtd3d 60×60×20, dt=…, steps=…`) the way other opaque handles do.
- `crates/rustlab-script/src/eval/builtins.rs` — thin wrappers, registered in `with_defaults()`. They lock, call `rustlab-dsp`, wrap records as real `Value`s (`real_matrix` / `real_tensor3` / `real_vector` when the data are real).
- Builtin signature stays `fn(Vec<Value>) -> Result<Value, ScriptError>`. The cancel flag is **not** added to that signature (it would touch every builtin). See cancellation.
- REPL help: `HelpEntry` plus one `CategoryRow` in `crates/rustlab-cli/src/commands/repl.rs`. `help_coverage_tests` will fail the build until both exist.
- Docs, in the implementation PRs only: `docs/functions.md`, `docs/quickref.md` (PDE section), `AGENTS.md` builtin table, `CHANGELOG.md`, workspace version bump. This plan PR does none of that except the Active Plans row, because nothing user-visible changes.

`FdtdSim` owns the arrays. The script never holds an alias into them. Lock poisoning is a script error, not a panic.

---

## Data layout and update kernels

### Why the hot loop does not walk a `Tensor3`

`Tensor3` standard layout is z-contiguous: index `(i*nx + j)*nz + k`. On the lesson grid the long axes are x and y (60) and the short axis is z (20). An inner loop of length 20 autovectorises poorly (four `f64`s per AVX register, five iterations, then a remainder).

Internal storage is **structure of arrays, x-fastest**:

```text
idx(i, j, k) = (k * ny + i) * nx + j     // j contiguous
stride_x = 1
stride_y = nx
stride_z = nx * ny
```

Six `Vec<f64>`: `ex, ey, ez, hx, hy, hz`, each length `ny*nx*nz` (plus the extra boundary face, below). Coefficients are the same shape, x-fastest, baked once. Export to a script `Tensor3` transposes into `(ny, nx, nz)` standard layout at snapshot time. One transpose of 72k doubles is noise next to thousands of steps. Ingest of `fdtd_media` does the opposite transpose once.

Do not call `ndarray`'s `[[i,j,k]]` in the step. Take `&[f64]` / `&mut [f64]` and index with the strides. That is what lets LLVM see a unit-stride inner loop.

### Stagger

Course-style collocated index space (Schneider / Taflove), not a separate allocation size per component. Every component array has shape `(ny, nx, nz)` in script coordinates, which is `(ny, nx, nz)` cells. The physical edges:

| Component | Stored at index `(i, j, k)` (0-based) | Location | Held at 0 (PEC wall) |
|---|---|---|---|
| `Ex` | y-row `i`, x-col `j`, z-page `k` | edge along x at the "lower" yz corner of the cell | `j` runs the edge; the **xmax** wall is the missing edge past `j = nx-1`, implemented by never reading `Ex` from outside and by zeroing the outer tangential E that the face API names |
| `Ey` | same | edge along y | analogous |
| `Ez` | same | edge along z | analogous |
| `Hx, Hy, Hz` | same indices | the face-centred H that the Yee curl pairs with those E edges | not forced |

This is the "same shape as the cell grid" convention, which matches `zeros3(ny, nx, nz)` and `rect_mask`. The cost is that the outer boundary face in each direction is a PEC wall of one cell: the E edges on the min faces (`i = 0`, `j = 0`, `k = 0` in 0-based storage) stay zero and are not updated, and the max faces are closed by the one-sided curl (no neighbour outside the array). The face API's extra index (`k = nz+1` in 1-based) addresses that outer closing face; internally it is a flag on the max-side tangential components, not a `nz+1` allocation. Phase 1's cavity test locks this table. Do not "fix" the stagger later without updating the port loop and the NTFF normals.

Cell-centred `eps_r` / `mu_r` / `sigma` are averaged onto edges:

- An E edge along x, between two cells, gets the **arithmetic** mean of the two cells' `eps_r` and of their `sigma` (the edge sits in series with nothing; the two dielectrics are side by side along a tangential edge). A boundary edge has only one cell.
- H edges (face centres) get the arithmetic mean of `mu_r` from the cells that share that face.
- Harmonic means are for the **normal** D flux in a Poisson operator (`laplacian_eps_*`), not for Yee E edges. Using the harmonic mean here would be the wrong physics. State this in the module comment so a later edit does not "correct" it toward the Laplacian.

### Leapfrog

One step `n → n+1`, fields real:

```text
H^{n+1/2} = Da * H^{n-1/2} + Db * curl_E(E^n)      % Db includes dt/μ
E^{n+1}   = Ca * E^n       + Cb * curl_H(H^{n+1/2})
```

with the PEC edges written back to 0 after the E update (and skipped during it).

For an E edge with permittivity `ε = eps_r ε0` and conductivity `σ`:

```text
Ca = (1 - σ dt / (2ε)) / (1 + σ dt / (2ε))
Cb = (dt / ε)         / (1 + σ dt / (2ε))
```

Lossless μ:

```text
Da = 1
Db = dt / μ
```

(The general lossy-μ form is the same shape as `Ca`/`Cb` with magnetic conductivity. v1 has no magnetic loss; `mu_r` still varies. `Da` is stored so a later magnetic loss does not reshuffle the kernel.)

Vacuum constants, defined once, not `3e8`:

```text
c0   = 299792458
μ0   = 1.25663706212e-6
ε0   = 1 / (μ0 * c0 * c0)
```

CFL, using the **maximum** phase speed on the grid (`eps_r` and `mu_r` both at their minima, and at least the vacuum speed):

```text
dt = courant / (c_max * sqrt(1/dx^2 + 1/dy^2 + 1/dz^2))
```

Default `courant = 0.99`. Bake `dt` when media is baked. If the user never calls `fdtd_media`, the grid is vacuum and `dt` is the vacuum limit.

Curl at an interior Ex edge (signs follow the standard right-handed Yee pair; the cavity test locks them):

```text
curl_h_x = (hz[i,j,k] - hz[i-1,j,k]) / dy - (hy[i,j,k] - hy[i,j,k-1]) / dz
```

Same pattern for the other five components. The inner loop runs over `j` (contiguous). Outer parallel loop runs over `i` (y rows). `k` is the middle loop.

A row update reads H and writes E, or reads E and writes H, never both in one parallel region. Half-steps are separated by the end of the parallel iterator (rayon join). No double buffer: each component is only stored once.

Soft source on an Ez edge, open-circuit voltage `Vg` (volts) at this step:

```text
Ez := Ez - Cb * (Vg / Δz)
```

Hard source, applied after the Ca/Cb update, overwriting the edge:

```text
Ez := -Vg / Δz
```

Recorded gap voltage is `V = −Ez · Δz` (and a sum over a stack). Both formulas make a positive `Vg` a positive `V`. The port section splits `Vg` across the stack. One scalar test in `port.rs` locks the sign.

### PML auxiliary in the curl

When CPML is on, each Cartesian derivative in the curl is replaced as in the next section. Interior coefficients have `κ = 1`, `a = 0`, `b = 1`, and the auxiliary stays 0, so the interior curl is the same arithmetic as the no-PML kernel. Implement that as **one** kernel with the `κ` and `ψ` terms always present, and require a test that a grid with PML allocated but `sigma_factor = 0` matches a no-PML run **bit for bit** for a few hundred steps. Two kernels will drift.

---

## CPML formulation

Convolutional PML, Roden & Gedney, IEEE Trans. Antennas Propagat., 2000. One auxiliary per derivative that the curl takes inside the layer. Twelve auxiliaries in the worst case (each of 6 components has two derivatives), stored as full x-fastest `Vec<f64>` in v1.

Full arrays on the 60×60×20 target are about 12 × 72e3 × 8 ≈ 7 MB. Simple indexing is worth that. A 200×200×80 grid would be ~300 MB of auxiliaries; `fdtd_info.bytes` reports it, and `checked_elems` still applies. Slab-packed ψ is a later optimisation, not a v1 requirement (open question if Michael wants it before any large lesson mesh).

For a derivative along x of a field `u`, with the difference already the two-point Yee difference `du = u[j] - u[j-1]`:

```text
b = exp(-(σ/(κ) + α) * dt / ε0)
a = 0                                              if σ = 0 and α = 0
a = σ / (κ * (σ + κ * α)) * (b - 1)               otherwise

ψ := b * ψ + a * (du / Δx)
d/dx |stretched = du / (κ Δx) + ψ
```

`a ≤ 0` because `(b − 1) ≤ 0`. The sign is whatever this formula produces; do not flip it to "make absorption happen" without changing the formula and the test together.

Profile along a local coordinate `ρ ∈ [0, 1]`, 0 at the inner interface and 1 at the outer PEC wall, evaluated at the **staggered** location of that derivative (half-cell offsets, per axis scale `Δ`):

```text
σ(ρ) = σ_max * ρ^m
κ(ρ) = 1 + (κ_max - 1) * ρ^m
α(ρ) = α_max * (1 - ρ)^{mα}

σ_max = sigma_factor * (m + 1) / (150 * π * Δ)
```

Defaults: `n = 8` is the recommended lesson thickness, `m = 3`, `mα = 1`, `κ_max = 7`, `α_max = 0.05`, `sigma_factor = 1`. `σ_max` uses that axis's `Δ` (so `dx`, `dy`, `dz` each get their own profile). `ρ` is computed in metres, not in cell counts, so a graded mesh spacing still ramps smoothly — v1 spacing is uniform per axis, but the profile should still be in metres so a later non-uniform axis does not silently change the polynomial.

Outside the slab: `σ = 0`, `κ = 1`, `α = 0`, so `b = 1`, `a = 0`.

The outer boundary remains PEC (tangential E stays 0). CPML does not replace that; it damps the wave before the wave hits it.

Disabled face (`"zmin", 0`): that side's `σ, κ, α` stay at the interior values all the way to the wall.

---

## Port and S11 method

A microstrip patch fed against a ground needs a **stack** of Ez edges through the substrate, not a single cell. `1:nsub` in the sketch is the whole gap.

Series model on that stack (total resistance `Z0`):

```text
N      = number of edges in the stack
R_edge = Z0 / N
σ_edge = Δz / (R_edge * Δx * Δy)          % for an Ez edge; permute for Ex/Ey
```

`σ_edge` is **added** to whatever material σ the edge already has, then `Ca`/`Cb` are baked from the sum. The incident source is an ideal voltage `Vg(t)` in series with `Z0`. Each edge of the stack gets the same share of that voltage:

```text
Ez := Ez - Cb_edge * (Vg / (N * Δz))
```

The `N` splits `Vg` the same way the resistance was split. This pair (σ split + Vg split) is the lumped series source. Phase 4 tests it on a port cell terminated by its own `Z0` before it is trusted on the patch:

- A wide Gaussian into that matched cell gives `V/I → Z0` after the transient, within 1%.
- Current is the Ampère loop around the first Ez edge of the stack, right-hand rule about +z, 0-based storage indices:

```text
I = (Hy(i, j, k) - Hy(i, j-1, k)) * dx
  - (Hx(i, j, k) - Hx(i-1, j, k)) * dy
```

Positive `V` and positive `I` mean power into the grid (`V*I > 0` while the Gaussian is rising into an open domain). `fdtd_s11` treats `V + Z0 I` as the incident wave under that sign. If a stagger review moves an H index by one, update this formula and keep the sign test as the authority.

Half-step: stored `I[n]` comes from `H^{n+1/2}` while stored `V[n]` comes from `E^{n+1}`. Before `a` and `b`, multiply the FFT of `I` by `exp(+j ω dt / 2)` so both phasors refer to the integer time of `V`. Magnitude of S11 on a resonance dip barely moves if this is omitted; the phase does. The pure-function test applies a known `dt/2` shift and checks the correction undoes it.

Window: Hann, generated in-tree (the window builtin already exists; calling it from the wrapper is fine). Rectangular is available for the resistor test, where a window would bias a long steady tail.

This is one-port S11. S21 of a line needs a second port and a definition of the reference planes. Data structures store N ports from day one so Phase 4 does not paint itself into a corner; the helper does not grow a full S matrix in v1.

---

## Near-to-far approach

Store **surface phasors**, not the time history of the box.

A closed Huygens surface in the homogeneous air region, axis-aligned, inset by `margin` cells from the inner PML boundary. Tangential E and H on the six faces. On-the-fly DFT at each armed frequency `f` (a handful, not `nsteps` bins):

```text
U(f) += u(t) * exp(-j 2π f t) * dt
```

sampled at the integer time for E and at the half-step for H, with the same `dt/2` phase folded into the H phasor so both are at integer times. `t` is the simulation time of that sample.

Memory: surface of the perf grid is on the order of `2*(60*60 + 60*20 + 60*20) ≈ 1e4` faces, times 4 tangential components, times 16 bytes, times `nfreq`. Twenty frequencies is a few megabytes. Saving every step of those faces for 10k steps is a few gigabytes and is not the design.

After the run, `fdtd_farfield` builds equivalent currents on each face (`n̂` outward):

```text
Js =  n̂ × H
Ms = −n̂ × E
```

and the far-field integrals (Taflove ch. 8; Balanis far-field of electric and magnetic currents):

```text
N(θ,φ) = Σ Js exp(j k r′ · r̂) ΔS
L(θ,φ) = Σ Ms exp(j k r′ · r̂) ΔS
k = 2π f / c0
```

`r′` is the face-centre of each Yee face in metres, origin at the grid corner `(0,0,0)` unless we later add an origin argument (open question). Then `Etheta`, `Ephi` from the standard `Nθ, Nφ, Lθ, Lφ` combination, including the `η0 = sqrt(μ0/ε0)` factor. Drop the `exp(-jkr)/r` factor (same for every angle) so the pattern is the angular part only.

Directivity: integrate `(|Eθ|² + |Eφ|²) sinθ` over the requested `theta`/`phi` grid with a trapezoid rule, normalise so the isotropic level is 0 dBi. The test grid must cover full `θ ∈ [0, π]`, `φ ∈ [0, 2π]`; a single cut errors and says so, returning only `Etheta`/`Ephi` and `NaN` directivity.

Realized gain multiplies the linear directivity by `1 − |S11(f)|²` from port 1. That treats accepted power as radiated plus the (unknown) loss in the substrate and the PML. On a lossless cavity-free run it is the right lesson number only if PML and numerical dissipation are small. Report both, and do not call realized gain "IEEE gain" in the docs.

v1 does not do a total-field/scattered-field plane wave. The patch is a driven port in total field; the Huygens box encloses the patch and the substrate interface it needs, sitting in air. If the box cuts through the substrate, error. That constraint is what `margin` plus a check on `eps_r` variance inside the surface cells enforces.

Doing the surface integrals in `.rlab` instead was considered (export the six faces' phasors, let the notebook sum them). The arithmetic is easy to get a normal-vector wrong in, and it is not the slow part. The stepper keeps the phasors internally and returns the pattern. A debug builtin that returns the phasors is not in v1; add it only if a lesson wants to show the integral (open question).

---

## Threading and expected performance

### Strategy

- Serial correctness first (Phases 1–5). Phase 6 turns on parallelism after the bit-exact tests exist.
- Use `rayon`, already in `rustlab-dsp`. Do not spawn a `std::thread::scope` per step; thread start would dominate a 72k-cell step. Rayon's pool stays up. A persistent `std::thread` pool is the fallback only if a profile shows the rayon join is a large fraction of a step, and it is still not a new dependency.
- Parallelise over y-rows (`i`), inner loop x (`j`). Each row writes its own E (or H) locations and its own ψ. The neighbouring row is only **read** (the y-derivative). That is race-free without a double buffer, as long as one parallel region does not update E and H together.
- Threshold: parallel only when `nx*ny*nz >= 32_768`. Below that, one thread. Unit tests on small cavities stay on the serial path. A test-only threshold override, same idea as `vector_calc`'s `__test_set_par_threshold`, runs the parallel path on a tiny grid and compares every field bit-for-bit against serial.
- No `std::simd`, no packed_simd crate, no nightly feature. The inner loop is plain `f64` on a unit stride so LLVM can autovectorise. If Phase 6's profile shows it did not, a later change can add `std::arch` AVX2/NEON behind `cfg` — still no new crate, and not part of this plan's commitment.
- Do not use `parmap` and do not call back into the interpreter from a worker.

### Back-of-envelope

Perf grid: `N = 60*60*20 = 72_000` cells. Take `S = 8_000` steps (middle of the 5–10k ask).

```text
cell-updates = N * S = 5.76e8
```

One cell-update means one full leapfrog step of one Yee cell (three E components and three H components).

Work per cell per step, interior, coefficients pre-baked: about 8 flops per component × 6 ≈ 50 flops, plus a similar number of loads of neighbours and `Ca`/`Cb`. Call it ~50 flops and ~40 `f64` of memory traffic.

```text
total flops ≈ 50 * 5.76e8 ≈ 2.9e10     (29 GFLOP)
```

Working set, 72k cells, 8 bytes each: 6 field arrays (3.5 MB), 12 coefficient arrays `Ca,Cb,Da,Db` per component (6.9 MB; a uniform μ can store `Db` once instead of three arrays), and 12 ψ arrays if PML auxiliaries are full-size (6.9 MB). About 17 MB. That fits in L3 on the machines this repo already times, and a small server L3 is the pessimistic case. The run is L3-resident, not a DRAM stream, when the access pattern is the x-contiguous one above. A z-contiguous walk of a `Tensor3` is still L3-resident, but the inner loop is 20 long and the y stride is `nx*nz = 1200` doubles (9.6 KB), which fights the prefetcher.

Plausible single-thread release rates for this kernel:

| Inner loop | Rate | Wall time for 5.76e8 updates |
|---|---|---|
| Scalar, strided, not autovectorised | 30–80 Mcell/s | 7–19 s |
| Unit-stride, autovectorised, L3-resident | 80–200 Mcell/s | 3–7 s |
| Same, 4 cores, y-slabs (this grid is small; expect ~2–3×, not 4×) | 150–400 Mcell/s | 1.5–4 s |

The 30 Mcell/s floor is about 1.5 GFLOP/s, which is what a clumsy scalar walk actually gets. The 200 Mcell/s figure is about 10 GFLOP/s, reasonable for a vectorised L3-resident stencil and not a claim of peak FLOPS.

**Gate:** release build, 60×60×20, vacuum `eps_r`, CPML on all six faces with the defaults, 8000 steps, no NTFF, under **60 s** wall. Expected band once Phase 6 has been profiled: a few seconds to about 20 s. Debug builds are not gated.

The gate is a `perf/` script or a `rustlab-dsp` example binary, run by hand and recorded in the PR, not a `#[test]` with a wall-clock assert (those flake in CI). CI covers a small step count for correctness only.

If the first honest measurement is over 60 s, the order of things to try is: confirm x-fastest unit stride, confirm the inner loop vectorised (`cargo asm` or a perf annotate, not a guess), then the parallel threshold. Not a new dependency, and not storing the fields as `Complex`.

---

## Ctrl+C and notebook cancel

Two different flags exist today.

1. **Ctrl+C on a TTY** — `InterruptGuard::arm()` installs the plot handler, which only stores an atomic. `interrupt_requested()` is what `norm` polls. Drop restores the previous disposition. When stdin is not a terminal the handler is not installed; `request_interrupt()` still works, which is how tests stop a run.
2. **Notebook / watch preemption** — `Evaluator.cancel`. Polled at statement boundaries only. A builtin that runs for 20 s will not notice a new save.

`fdtd_run` / `fdtd_step` of more than one step:

- Arm `InterruptGuard` for the call.
- Every step (the step is the right granularity; 72k cells is far more work than one atomic load), poll `interrupt_requested()`.
- Also poll a thread-local `Option<Arc<AtomicBool>>` that `Evaluator` sets around `builtins.call_with_nargout` and clears after. That is a few lines in `eval/mod.rs`. The builtin function pointer type stays unchanged. `norm` does not have to be retrofitted in this work.
- On Ctrl+C: leave the handle's fields, probes, and port records at the last completed step (the `Arc` is already in the workspace under `sim`). Print `interrupted` to stderr, matching `norm`. Return `ScriptError::Interrupted`. The REPL stays up, so `fdtd_record(sim)` and `fdtd_s11(sim)` still work on the partial run. `rustlab run` exits 0, matching `norm` — a batch script does not get a partial binding, because the error aborts the script. That is acceptable: Ctrl+C during `rustlab run` already throws the process result away for `norm`.
- On the notebook flag: return `ScriptError::Cancelled` so the watch server's preemption path stays the one it has. Partial notebook output is discarded by the renderer anyway.
- Do not hold the `FdtdSim` mutex across a `.rlab` callback. There is no callback. The lock is held for the whole `fdtd_run`, which also means a probe copy happens after the loop, under the same lock, then the lock drops before `Value`s are built if we copy into `Vec` first. Copy into `Vec<f64>` under the lock, wrap `Value` after. Poisoned lock → script error.

Tests: a unit test calls `request_interrupt()` after arming (or sets the thread-local cancel) and asserts the run returns within a step or two and that `steps` on the handle is `< nsteps` and `> 0`.

---

## `laplacian_eps_3d` (Phase 0)

Independent of the stepper. Implement even if FDTD waits on an open question.

Mirror `laplacian_eps_2d` in `crates/rustlab-dsp/src/laplacian.rs`:

```text
pub fn laplacian_eps_3d(
    eps_map: &CTensor3,
    dx: f64, dy: f64, dz: f64,
    bc: BoundaryCondition,
) -> Result<SparseMat, DspError>
```

- Shape `(ny, nx, nz)`. Flat index and strides identical to `laplacian_3d` (`stride_y = 1`, `stride_x = ny`, `stride_z = nx*ny`).
- Six faces. Harmonic mean `2 εa εb / (εa + εb)` of the two cells sharing the face, complex-safe, matching the 2-D `hmean` (0 if the sum underflows).
- Dirichlet: drop the neighbour, still subtract the face coefficient from the diagonal (ghost cell 0, face ε is the boundary cell's own ε, same as 2-D).
- Neumann: skip the face.
- Periodic: wrap.
- Builtin argument forms match `laplacian_eps_2d`: `(eps)`, `(eps, dx, dy, dz)`, `(eps, bc)`, `(eps, dx, dy, dz, bc)`.
- Return through `laplacian_value` so the `Identity` ordering hint is set.
- Uniform `eps_map ≡ c` matches `c * laplacian_3d(...)` entrywise (the 2-D test `lap_eps_unit_eps_matches_lap_2d` is the template; for a non-1 constant the comparison is against `c` times the unit operator).
- Neumann or periodic with a varying ε: `L * ones = 0`.
- Script-level: two-layer capacitor, `pin_dirichlet` on the two plates, `spsolve`, `1/C = 1/C1 + 1/C2` to a few percent on a modest grid. That is the Lesson 15 number.
- Docs and REPL help in the same phase. Version bump in that PR (observable new builtin).

This does not share code with the Yee coefficient bake beyond the axis convention. Do not try to make one function serve both.

---

## Validation plan

All of these are headless `cargo test` except the perf gate.

### Yee kernel

1. **Bit-exact PML-off identity.** CPML structures allocated, `sigma_factor = 0`, versus a build with no CPML. Fields equal bit for bit after a few hundred steps (same source).
2. **CFL smoke.** `courant = 1.5` on a vacuum cube diverges (max `|E|` exceeds a bound, or a documented `fdtd_run` error if we choose to reject Courant `> 1` — open question; default is to allow it and let the cavity test use 0.99). Default 0.99 on a vacuum run stays bounded over 2k steps.
3. **Numerical dispersion, not just continuum frequency.** A long thin cavity or a periodic 1-D-like mode. The measured FFT peak of a probe matches the Yee dispersion relation

   ```text
   (1/c²dt²) sin²(ω dt/2) = sin²(kx dx/2)/dx² + sin²(ky dy/2)/dy² + sin²(kz dz/2)/dz²
   ```

   to well under a bin width. Comparing only to `f = c/λ` hides a correct kernel inside a coarse-grid shift.
4. **PEC cavity.** Rectangular box, all six faces PEC, no PML, no loss. Analytic

   ```text
   f_mnp = (c / 2) * sqrt((m/Lx)² + (n/Ly)² + (p/Lz)²)
   ```

   Lowest mode within 2% on a coarse grid (about 20 cells on a side) and within 0.5% of the **dispersion-relation** prediction. A dielectric fill `eps_r = 4` shifts the same mode by `1/sqrt(4)` to 1%.
5. **Parallel = serial**, bit for bit, on a grid just above the parallel threshold.

### CPML

6. **Reflection.** Point soft source in a homogeneous grid, 8-cell CPML, polynomial defaults. Record a probe. Repeat with a larger grid (or stop the small-grid record before a PEC reflection would return, using a reference run with PML off that is large enough that the window ends first). Late-time residual over the incident peak **< −40 dB**. −50 dB is the goal, not the gate. Normal incidence (probe on axis). Grazing incidence is not a v1 gate.
7. The bit-exact test (1) so a "reflection" bug is not actually a broken interior update.

### Port and patch

8. **Matched lumped resistor.** `V/I → Z0` to 1% after a slow Gaussian. Locks σ split, source split, and the Ampère-loop sign.
9. **Half-step phase.** Synthetic `V`, `I` with a known `dt/2` shift; `fdtd_s11` correction recovers a purely real `Z0` at one frequency.
10. **Patch vs transmission-line model.** Geometry from the sketch (`εr = 4.4`, `h = 1.6 mm`, design 2.4 GHz, lossless). On the 60×60×20 class mesh, the frequency of the `|S11|` minimum is within **5%** of 2.4 GHz. The dip depth is **not** gated (the feed is a coarse edge feed; a −4 dB dip can still be the right frequency). A finer mesh, if Phase 6 is fast enough to run it in CI, tightens the frequency gate to 2%; otherwise that finer run is nightly / manual.
11. **Published curve, optional.** Not a CI dependency and not invented here. Candidates, if Michael wants a second anchor: Sheen, Ali, Abouzahra, Kong, IEEE Trans. MTT, vol. 38, no. 7, pp. 849–857, 1990 (the classic microstrip FDTD S-parameter paper); an openEMS microstrip-patch example, cited with its revision. The ACES Journal Nov 2020 comparison (resonance quoted at 6.564 GHz) does not include the metal dimensions in the abstract; do not treat 6.564 GHz as a number we can hit without copying that geometry from the paper. No MATLAB Antenna Toolbox license is assumed in CI.

### Near-to-far

12. **Dipole sanity.** A small z-directed soft source in a large vacuum PML box, one frequency, no metal. `Ephi` dominates in the xy-plane cut, the pattern is symmetric under `φ → φ+π`, and broadside (θ = π/2) is the maximum of that cut. Directivity of an ideal short dipole is 1.76 dBi; on a coarse box, gate **directivity between 0 and 4 dBi** and symmetry to a few percent. This catches a flipped `n̂` or a missing `η0`, which a patch test will not isolate.
13. **Patch lobe.** On the lesson mesh, broadside (`theta` near 0) is a peak of the principal cut, and directivity is `> 0 dBi`. Do not gate against a Balanis pattern table on a λ/2 ground plane.

### Laplacian

14. Uniform ε, null-space, and the series-capacitor identity from the sibling request.

---

## MATLAB-compatibility notes

MATLAB has no `fdtd3d`, no Yee builtin, and no CPML builtin. Do not invent MATLAB names or a class hierarchy (`patchMicrostrip` is Antenna Toolbox, a different product, and it is not the API to clone).

What should match existing rustlab / MATLAB array habits:

- `zeros(ny, nx, nz)` in MATLAB is the same axis order as `zeros3(ny, nx, nz)` here: rows, columns, pages. Document that `i` is the row (y), `j` is the column (x), `k` is the page (z). MATLAB `meshgrid` is a 2-D trap; our `meshgrid` is already the `'xy'` form (`X` varies along columns). There is no 3-D `meshgrid` to be compatible with.
- `fft` length is the length of the record, same as MATLAB `fft`. Positive-frequency slicing in `fdtd_s11` is a helper choice, not a change to `fft`.
- S11 from `(V ± Z0 I)` is the circuit definition MATLAB RF tools use for a lumped port. It is not `sparameters()` of a Touchstone file. The lesson can pass `s11` into a small struct if it wants `smith()`, but v1 does not auto-build an `sparameters` object (open question). Frequency in Hz, `Z0` scalar, same numbers those builtins already use.
- 1-based indices, including port ids and probe ids.
- No `end` inside `fdtd_port`'s `krange` beyond what the language already does for a range expression (`1:nsub` is just a vector by the time the builtin sees it).

Divergences to document on purpose:

- Complex permittivity is rejected by `fdtd_media`. MATLAB FDTD examples sometimes pass a complex ε into a frequency-domain solver. Loss here is `sigma`.
- Field components share the cell-grid shape. Some MATLAB file-exchange Yee codes allocate `Ex` as `(nx, ny+1, nz+1)`. Our help text shows the stagger table so a port of such a code is not a silent off-by-one.
- `fdtd_run` does not return the fields. MATLAB-style scripts that expect `[Ex, Ey, Ez] = fdtd(...)` after one call should call `fdtd_fields`.

---

## Phases

Estimates are implementer time for someone who already knows this tree, including tests and the docs/version bump that phase requires. They are not a promise about review latency. CPML signs and the NTFF normals are where a slip would land. Do not start Phase N+1 without an explicit yes.

| Phase | Delivers | Rough ETA | Done when |
|---|---|---|---|
| **0** | `laplacian_eps_3d`, docs, REPL help, capacitor test. No FDTD. | 1–2 days | Uniform-ε identity, Neumann null-space, series `1/C` test, `cargo test --workspace`. |
| **1** | Opaque handle, vacuum Yee, CFL, `gaussian_pulse`, probes, `fdtd_run` / `fdtd_step` / `fdtd_record` / `fdtd_slice`, Ctrl+C and notebook cancel. Serial only. | 4–6 days | Cavity mode vs dispersion relation; interrupt test; bit-stable repeat of a short run. |
| **2** | `fdtd_media`, edge bake, volume and face `fdtd_pec`. | 2–3 days | Dielectric cavity shifts by `1/sqrt(eps_r)`; PEC face actually zeros a probe on that face. |
| **3** | CPML, per-face disable. | 3–5 days | Bit-exact against Phase 1 when `sigma_factor = 0`. Reflection < −40 dB. |
| **4** | Lumped port, `fdtd_s11` pure and handle forms, patch example script. | 3–4 days | Resistor test to 1%. Patch `|S11|` minimum within 5% of 2.4 GHz on the teaching mesh. |
| **5** | On-the-fly NTFF, `fdtd_farfield`, directivity and realized gain. | 4–6 days | Dipole symmetry and 0–4 dBi directivity. Patch broadside peak. |
| **6** | Rayon y-slabs, perf gate recorded, example notebook under `examples/notebooks/`, changelog and version already bumped per phase as builtins appeared. | 2–3 days | Parallel matches serial bit for bit. 60×60×20 × 8000 steps under 60 s in release. |

Total roughly **three to five weeks**. Phase 0 can merge alone and unblocks rustlab_em Lesson 15 immediately.

Each implementation phase is its own PR on a feature branch, version-bumped, with the Workflow Rules 2, 3, 5, 6, and 7 docs. This document's PR is plan-only: no version bump, no builtin, no CHANGELOG entry.

Suggested first lesson order once the phases exist: cavity notebook (Phase 1–2), then PML reflection (Phase 3), then the patch (Phase 4–5). The patch script sketch above is the capstone, not the first example a student runs.

---

## Test and doc checklist (for the implementation PRs, not this one)

- `crates/rustlab-dsp/src/fdtd/*.rs` unit tests for CFL, dispersion, CPML reflection, resistor, dipole pattern.
- `crates/rustlab-script/src/tests.rs` (or a focused `tests/fdtd.rs`) for the handle, 1-based indices, error paths (complex ε, NTFF after steps, port on a hard source), and `fdtd_s11` on a synthetic V/I.
- `laplacian_eps_3d` tests beside the existing ones in `laplacian.rs`, plus one script-level `spsolve` capacitor test.
- REPL `HelpEntry` + exactly one `CategoryRow`.
- `docs/functions.md`, `docs/quickref.md`, `AGENTS.md` builtin table, `CHANGELOG.md`, version bump — the phase that adds the builtin, not a follow-up.
- Octave: there is no stock Octave FDTD to add to `make octave-compare`. Do not block on it. The analytic cavity and the dispersion relation are the reference.
- Plot backends: no new plot kind, so Rule 9 does not add a backend matrix. The notebook example still gets rendered once in Phase 6 so the sketch is not only an `.rlab` file.

---

## Open questions for Michael

1. **Phase 0 first?** `laplacian_eps_3d` is the Lesson 15 request and does not depend on FDTD. Should it merge on its own before any Yee work?
2. **Opaque handle.** The hot fields stay inside `sim`, with `fdtd_slice` / `fdtd_fields` to copy out. A design where `Ex` is a real `Tensor3` the script can poke every step will not hit the timing target. Is the handle acceptable for the lesson, or do you want a slower "export every step" mode for a short demo?
3. **Single port vs S matrix.** v1 records N ports but only computes S11 of port 1. Do you want S21 of a microstrip line in the same milestone?
4. **Reference geometry.** The proposed science check is the lossless transmission-line 2.4 GHz FR-4 patch (`εr = 4.4`, `h = 1.6 mm`), frequency of the S11 dip within 5% on the coarse mesh. Do you instead want a specific published drawing (Sheen 1990, an openEMS example, or the ACES 2020 board)? If so, which one should the lesson treat as truth?
5. **Realized gain on a half-wave ground.** The 60×60×20 box is about `λ/2` across and only 8 mm tall. Directivity there is a sanity check, not a number to compare with Balanis. Is a qualitative broadside lobe enough for the lesson, with a finer mesh left as an exercise?
6. **Huygens origin and exported currents.** Far-field phase origin is proposed as the grid corner `(0,0,0)`. Pattern magnitude does not care. Do you want the origin at the patch centre, and do you want the surface `Js`/`Ms` phasors returned so a notebook can show the integral?
7. **CPML defaults.** Proposed `n = 8`, order 3, `κ_max = 7`, `α_max = 0.05`, `σ_max = (m+1)/(150 π Δ)`. These are the usual textbook defaults, not a fit to a particular absorber paper. Any defaults you already use in `rustlab_em`'s 2-D FDFD PML that this should match so the lessons feel like one method?
8. **Courant above 1.** Reject `courant > 1` at setup, or allow it and let the field blow up? Rejection is kinder in a classroom.
9. **Ctrl+C result.** Proposed: `Interrupted`, partial data left on `sim` for the REPL, `rustlab run` exits 0 with no `out` binding (same as `norm`). Prefer instead that `fdtd_run` return the partial struct and keep going, so a script can plot what it has?
10. **Where it lives.** Proposed: always-on modules in `rustlab-dsp`, no feature flag. The old §2.6 note suggested a default-off `rustlab-em` crate for sparse curl builders. Those builders are out of scope here. Any reason the time stepper should still be behind a flag?
11. **Loss model in the lesson.** Default patch comparison is lossless, because the transmission-line formula is. Should the shipped example use `tanδ = 0.02` and only check that the dip stays near 2.4 GHz, so students see a realistic FR-4 from the start?
12. **`sparameters` object.** Should `fdtd_s11` also return something `smith()` / `rfplot()` already accept, or is a `freq` + `s11` struct enough and the lesson can wrap it?

---

## References

- A. Taflove and S. C. Hagness, *Computational Electrodynamics: The FDTD Method*, 3rd ed. Artech House, 2005. Ch. 7 (CPML), ch. 8 (near-to-far).
- J. A. Roden and S. D. Gedney, "Convolution PML (CPML): An efficient FDTD implementation of the CFS-PML for arbitrary media," *IEEE Trans. Antennas Propagat.*, vol. 48, no. 12, pp. 1630–1639, 2000.
- D. M. Sheen, S. M. Ali, M. D. Abouzahra, and J. A. Kong, "Application of the three-dimensional finite-difference time-domain method to the analysis of planar microstrip circuits," *IEEE Trans. Microwave Theory Tech.*, vol. 38, no. 7, pp. 849–857, 1990.
- C. A. Balanis, *Antenna Theory*, transmission-line model for a rectangular patch (the formulas in the script sketch).
- K. E. Schneider, *Understanding the FDTD Method*, the collocated-index Yee layout this plan standardises on. Online notes, the course text several of the EM lessons already sit next to.
- rustlab_em requests: `dev/rustlab/requests/laplacian-eps-3d.md`, `dev/rustlab/requests/yee-and-pml-builders.md`.
- In-tree precedents: `crates/rustlab-dsp/src/laplacian.rs` (`laplacian_eps_2d`, `laplacian_3d`), `crates/rustlab-dsp/src/vector_calc.rs` (rayon threshold), `matrix_spectral_norm` and `rustlab_plot::InterruptGuard` (cancel).
